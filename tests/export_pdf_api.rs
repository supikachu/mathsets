//! T3.7 端点集成测试：`POST /api/v1/export/pdf` 与 `GET /api/v1/typeset/profiles`
//!
//! PDF 与 docx 的分工不同：docx 能直接断言 XML 明文，PDF 里的汉字是矢量轮廓（搜关键词恒为
//! false），所以**纸面文字断言留在 `typst_gen` 的帧树用例**，这一层只钉 HTTP 契约：
//! 鉴权、`application/pdf`、RFC 5987 中文文件名、`%PDF` 文件头与 `%%EOF` 尾、
//! 「一处坏公式不中断整卷」（200 + 降级警告进 `X-Export-Warnings`），以及 R1 的两条口径 ——
//! profiles 只吐预设、`/typeset/render` 根本不存在。
//!
//! 复用 tests/api.rs 的测试库约定：未设置 `DATABASE_URL_TEST` 时跳过。

use axum::{
    body::Body,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use mathset::export::model::ExamRequest;
use mathset::typeset::spec::LayoutSpec;
use mathset::{AppState, build_app, db};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

async fn create_test_app() -> Option<axum::Router> {
    let database_url = mathset::testing::database_url()?;
    let pool = db::create_pool(&database_url, 5).await;
    db::run_migrations(&pool).await;
    let state = AppState::new(
        pool,
        "test-secret-for-integration-tests".to_string(),
        24,
        mathset::config::AiConfig::from_env(),
        "./uploads".to_string(),
    );
    Some(build_app(state))
}

/// 文件类响应不是 JSON：状态码 + 头 + 原始字节
async fn request_raw(
    app: &mut axum::Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        builder = builder.header("Authorization", format!("Bearer {}", t));
    }
    if body.is_some() {
        builder = builder.header("Content-Type", "application/json");
    }
    let req = builder
        .body(match body {
            Some(b) => Body::from(serde_json::to_vec(&b).unwrap()),
            None => Body::empty(),
        })
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .unwrap();
    (status, headers, bytes.to_vec())
}

async fn post_json(app: &mut axum::Router, uri: &str, body: Value, token: &str) -> Value {
    let (_, _, bytes) = request_raw(app, Method::POST, uri, Some(body), Some(token)).await;
    serde_json::from_slice(&bytes).unwrap_or(json!({"error": "parse failed"}))
}

async fn get_json(app: &mut axum::Router, uri: &str, token: &str) -> Value {
    let (_, _, bytes) = request_raw(app, Method::GET, uri, None, Some(token)).await;
    serde_json::from_slice(&bytes).unwrap_or(json!({"error": "parse failed"}))
}

async fn register_and_login(app: &mut axum::Router) -> String {
    let username = format!("exppdf_{}", &Uuid::new_v4().to_string()[..8]);
    let _ = post_json(
        app,
        "/api/v1/auth/register",
        json!({
            "username": username,
            "email": format!("{}@test.com", username),
            "password": "test123",
            "display_name": "PDF 导出测试用户"
        }),
        "none",
    )
    .await;
    let body = post_json(
        app,
        "/api/v1/auth/login",
        json!({ "username": username, "password": "test123" }),
        "none",
    )
    .await;
    body["token"].as_str().unwrap().to_string()
}

async fn create_question(app: &mut axum::Router, token: &str, body: Value) -> Uuid {
    let resp = post_json(app, "/api/v1/questions", body, token).await;
    resp["id"]
        .as_str()
        .unwrap_or_else(|| panic!("create question failed: {}", resp))
        .parse()
        .unwrap()
}

fn header_str(headers: &HeaderMap, name: impl header::AsHeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(String::from)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
            out.push(u8::from_str_radix(hex, 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

fn warnings(headers: &HeaderMap) -> Vec<Value> {
    let raw = header_str(headers, "X-Export-Warnings").expect("warnings header");
    assert!(raw.is_ascii(), "警告头必须是 ASCII：{raw}");
    let arr: Value = serde_json::from_str(&percent_decode(&raw)).expect("警告头是 JSON 数组");
    arr.as_array().unwrap().clone()
}

/// 页面尺寸（pt）：第一个 `/MediaBox [0 0 W H]` 的宽高
///
/// typst 把页面字典明文写进 PDF，所以「spec 到底生效没有」不必肉眼开文件，
/// 纸宽这一个数就能判。
fn media_box(bytes: &[u8]) -> (f32, f32) {
    let s = String::from_utf8_lossy(bytes);
    let at = s.find("/MediaBox").unwrap_or_else(|| {
        panic!(
            "PDF 里没有明文 MediaBox，前 200 字节 {:?}",
            &bytes[..bytes.len().min(200)]
        )
    });
    let nums: Vec<f32> = s[at + "/MediaBox".len()..]
        .split(|c: char| c != '.' && !c.is_ascii_digit())
        .filter(|t| !t.is_empty() && *t != ".")
        .take(4)
        .filter_map(|t| t.parse().ok())
        .collect();
    assert_eq!(nums.len(), 4, "MediaBox 读数异常：{}", &s[at..at + 40]);
    (nums[2], nums[3])
}

/// 是 PDF 就得是个能收口的 PDF：文件头、文件尾与体积
fn assert_pdf(bytes: &[u8]) {
    assert!(
        bytes.starts_with(b"%PDF"),
        "产物不是 PDF：{:?}",
        &bytes[..8.min(bytes.len())]
    );
    let tail = &bytes[bytes.len().saturating_sub(64)..];
    assert!(
        tail.windows(5).any(|w| w == b"%%EOF"),
        "PDF 缺收尾，像是被截断"
    );
    assert!(bytes.len() > 1500, "PDF 小得可疑：{} 字节", bytes.len());
}

const FILL_STEM: &str = "已知集合 $A=\\{1,2\\}$，则 $A$ 的子集个数为______个";
const CHOICE_STEM: &str =
    "已知函数 $$f(x)=\\begin{cases}x^2,&x\\ge 0\\\\-x,&x<0\\end{cases}$$，则下列说法正确的是";

fn exam_body(fill_id: Uuid, choice_id: Uuid, options: Value) -> Value {
    json!({
        "title": "集合单元测验",
        "exam_meta": { "school": "实验中学", "duration": 90, "total_score": 13 },
        "mode": "student",
        "sections": [
            { "title": "一、填空题", "questions": [{ "id": fill_id, "default_score": 8 }] },
            { "title": "二、单选题", "instruction": "每题 5 分", "questions": [{ "id": choice_id, "default_score": 5 }] }
        ],
        "options": options
    })
}

fn choice_json(stem: &str) -> Value {
    json!({
        "stem": stem,
        "question_type": "choice",
        "difficulty": 2,
        "options": [
            {"label": "A", "content": "空集没有子集"},
            {"label": "B", "content": "空集是任何集合的子集"},
            {"label": "C", "content": "$0 \\in \\varnothing$"},
            {"label": "D", "content": "{0} 是空集"}
        ],
        "correct_answer": {"kind": "choice", "value": {"options": ["B"]}},
        "analysis": "子集个数公式 $2^n$"
    })
}

fn fill_json(stem: &str) -> Value {
    json!({
        "stem": stem,
        "question_type": "fill",
        "difficulty": 3,
        "correct_answer": {"kind": "fill", "value": {"blanks": [{"position": 1, "answer": "4"}]}},
        "analysis": "子集个数公式 $2^n$"
    })
}

/// 请求体本身也要过一遍真实的 `ExamRequest` 反序列化：`spec` 是 `Option<LayoutSpec>`，
/// 手工拼 JSON 容易拼出一个后端根本收不下来的形状（T3.8 前端就吃这条契约）
fn with_spec(body: Value, spec: &LayoutSpec) -> Value {
    let mut req: ExamRequest = serde_json::from_value(body).expect("请求体必须是合法 ExamRequest");
    req.spec = Some(spec.clone());
    serde_json::to_value(&req).unwrap()
}

// ═══════════════════════════════ /export/pdf ═══════════════════════════════

#[tokio::test]
async fn pdf_export_requires_auth() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过 PDF 导出端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let (status, _, _) = request_raw(
        &mut app,
        Method::POST,
        "/api/v1/export/pdf",
        Some(json!({ "title": "x", "mode": "student", "sections": [] })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn pdf_export_is_a_pdf_with_the_rfc5987_name() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过 PDF 导出端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;
    let fill_id = create_question(&mut app, &token, fill_json(FILL_STEM)).await;
    let choice_id = create_question(&mut app, &token, choice_json(CHOICE_STEM)).await;

    let (status, headers, bytes) = request_raw(
        &mut app,
        Method::POST,
        "/api/v1/export/pdf",
        Some(exam_body(
            fill_id,
            choice_id,
            json!({ "include_answer": false, "include_analysis": false }),
        )),
        Some(&token),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_str(&headers, header::CONTENT_TYPE).as_deref(),
        Some("application/pdf")
    );
    // 与 markdown / docx 同一套文件名契约（"集" = E9 9B 86）
    let cd = header_str(&headers, header::CONTENT_DISPOSITION).expect("content-disposition");
    assert!(cd.contains("filename*=UTF-8''%E9%9B%86"), "{}", cd);
    assert!(cd.ends_with(".pdf"), "{}", cd);
    assert_pdf(&bytes);
    assert!(
        headers.get("X-Export-Warnings").is_none(),
        "全部题目可见且公式可编译 → 不应有警告头：{:?}",
        header_str(&headers, "X-Export-Warnings")
    );
}

#[tokio::test]
async fn pdf_export_degrades_bad_latex_without_failing_the_paper() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过 PDF 导出端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;
    // 坏公式与好公式混在同一道题、同一份卷里。
    // `\argmax_x` 是 PDF 侧的坏公式：mitex 转得动、typst 不认，静态守卫会拦下来降级。
    // （Word 侧那枚 `$\frac{1}{$` 在这里不成立 —— mitex 对括号不配平很宽容，
    // to_typst 直接返回 Ok，typst 也编得过，见 docs/dev-diary.md）
    let bad_fill_id = create_question(
        &mut app,
        &token,
        fill_json("计算 $\\argmax_x f$ 与 $y^2$ 的值"),
    )
    .await;
    let choice_id = create_question(&mut app, &token, choice_json(CHOICE_STEM)).await;

    let (status, headers, bytes) = request_raw(
        &mut app,
        Method::POST,
        "/api/v1/export/pdf",
        Some(exam_body(
            bad_fill_id,
            choice_id,
            json!({ "include_answer": false, "include_analysis": false }),
        )),
        Some(&token),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "单处坏公式不得中断整卷导出");
    assert_pdf(&bytes);

    let items = warnings(&headers);
    let bad = items
        .iter()
        .find(|i| i["latex"] == json!(r"\argmax_x f"))
        .unwrap_or_else(|| panic!("降级警告缺 latex 字段: {items:?}"));
    assert_eq!(bad["field"], "stem");
    assert_eq!(bad["question_no"], json!(1));
    let reason = bad["reason"].as_str().unwrap();
    assert!(
        reason.starts_with("PDF 公式降级："),
        "降级警告必须给出原因: {reason}"
    );
    assert!(reason.contains("argmax"), "原因要点名可疑标识符: {reason}");
    assert_eq!(
        items.iter().filter(|i| i["field"] == "stem").count(),
        1,
        "同题的好公式不该被牵连: {items:?}"
    );
    assert!(
        items.iter().all(|i| i.get("truncated").is_none()),
        "未超上限不该出现截断哨兵: {items:?}"
    );
}

#[tokio::test]
async fn pdf_export_accepts_a_preset_spec_round_tripped_from_profiles() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过 PDF 导出端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;
    let fill_id = create_question(&mut app, &token, fill_json(FILL_STEM)).await;
    let choice_id = create_question(&mut app, &token, choice_json(CHOICE_STEM)).await;

    // T3.8 的真实数据流：profiles 拿预设 → 前端微调 → 整份 spec 回传 /export/pdf
    let profiles = get_json(&mut app, "/api/v1/typeset/profiles", &token).await;
    let tri = profiles
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == json!("a3_tri_exam"))
        .unwrap_or_else(|| panic!("预设里没有 a3_tri_exam: {profiles:?}"));
    let spec: LayoutSpec = serde_json::from_value(tri["spec"].clone()).expect("预设里的 spec");
    assert_eq!(spec.columns, 3, "A3 三栏预设的栏数");

    let body = with_spec(
        exam_body(
            fill_id,
            choice_id,
            json!({ "include_answer": true, "include_analysis": true, "answer_at_end": true }),
        ),
        &spec,
    );
    let (status, headers, bytes) = request_raw(
        &mut app,
        Method::POST,
        "/api/v1/export/pdf",
        Some(body),
        Some(&token),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_pdf(&bytes);
    // 请求里没开留白（options.answer_space 缺省）→ 也不该有留白样式冲突之类的卷级警告
    assert!(
        headers.get("X-Export-Warnings").is_none(),
        "{:?}",
        header_str(&headers, "X-Export-Warnings")
    );

    // 覆盖确实生效：同一份卷不带 spec 时后端按 mode 取默认预设（student → A4 双栏），
    // 两次导出的纸宽差一倍 —— 这是「前端微调回传」这条链路唯一的机器判据
    let (w_a3, h_a3) = media_box(&bytes);
    let plain = exam_body(
        fill_id,
        choice_id,
        json!({ "include_answer": true, "include_analysis": true, "answer_at_end": true }),
    );
    let (status_a4, _, bytes_a4) = request_raw(
        &mut app,
        Method::POST,
        "/api/v1/export/pdf",
        Some(plain),
        Some(&token),
    )
    .await;
    assert_eq!(status_a4, StatusCode::OK);
    let (w_a4, h_a4) = media_box(&bytes_a4);
    assert!(
        (w_a4 - 595.28).abs() < 1.0,
        "A4 纸宽应约 595pt，实为 {w_a4}"
    );
    assert!(
        (w_a3 - 1190.55).abs() < 2.0,
        "回传的 A3 三栏 spec 没生效：纸宽 {w_a3}（A4 是 {w_a4}）"
    );
    assert!(
        (h_a3 - h_a4).abs() < 1.0,
        "A3 对开与 A4 同高，{h_a3} vs {h_a4}"
    );
}

#[tokio::test]
async fn pdf_export_survives_invisible_questions() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过 PDF 导出端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;
    let fill_id = create_question(&mut app, &token, fill_json(FILL_STEM)).await;
    let missing = Uuid::new_v4();

    let body = json!({
        "title": "含缺失题的测验",
        "mode": "teacher",
        "sections": [{
            "title": "一、填空题",
            "questions": [{ "id": missing }, { "id": fill_id }]
        }],
        "options": { "include_answer": true, "include_analysis": true, "answer_at_end": true }
    });
    let (status, headers, bytes) = request_raw(
        &mut app,
        Method::POST,
        "/api/v1/export/pdf",
        Some(body),
        Some(&token),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "缺失题应跳过并记警告");
    assert_pdf(&bytes);
    let items = warnings(&headers);
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["field"], "other");
    assert!(items[0]["reason"].as_str().unwrap().contains("不存在"));
}

// ═══════════════════════════ /typeset/profiles ═══════════════════════════

#[tokio::test]
async fn profiles_endpoint_lists_the_four_presets() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过 PDF 导出端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;
    let (status, _, _) = request_raw(
        &mut app,
        Method::GET,
        "/api/v1/typeset/profiles",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "预设也要登录");

    let body = get_json(&mut app, "/api/v1/typeset/profiles", &token).await;
    let items = body.as_array().expect("预设是数组");
    let ids: Vec<&str> = items.iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        vec!["a4_lecture", "a4_practice", "a3_fold_exam", "a3_tri_exam"]
    );
    for p in items {
        assert!(!p["label"].as_str().unwrap().is_empty(), "下拉要有中文名");
        let spec = &p["spec"];
        assert!(
            (1..=3).contains(&spec["columns"].as_u64().unwrap()),
            "{spec}"
        );
        assert!(spec["margins"]["top_mm"].as_f64().unwrap() > 0.0);
    }
}

/// R1：PDF 出口唯一化为 `/export/pdf` —— 排版不另开渲染通道（两条流水线必然漂移）
#[tokio::test]
async fn there_is_no_typeset_render_route() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过 PDF 导出端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;
    for (method, uri) in [
        (Method::GET, "/api/v1/typeset/render"),
        (Method::POST, "/api/v1/typeset/render"),
        (Method::POST, "/api/v1/typeset/preview"),
    ] {
        let (status, _, _) = request_raw(
            &mut app,
            method.clone(),
            uri,
            (method == Method::POST).then(|| json!({})),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri} 不该存在");
    }
}
