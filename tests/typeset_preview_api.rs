//! T5.2 预览端点集成测试：`POST /api/v1/typeset/preview`
//!
//! 这一层只钉 HTTP 契约（几何与三条判据的正确性在 `typeset::preflight` 与 `export::pdf` 的
//! 用例里）：鉴权、裸载荷 `{pages, page_count, issues, warnings}` 的形状、请求 `spec` 真的换
//! 了纸、「一处坏公式不弄坏整卷」，以及预检的发现**确实随预览 JSON 回来**（§6.5）—— 最后这条
//! 是本端点存在的理由，掉了没人发现。
//!
//! SVG 页的判据取根元素的 `width`（pt）：typst-svg 把纸张尺寸明文写在属性上，A4 = 595.28、
//! A3 横 = 1190.55，比看像素可靠。
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

const A4_WIDTH_PT: f32 = 595.28;
const A3_LANDSCAPE_WIDTH_PT: f32 = 1190.55;

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

async fn request(
    app: &mut axum::Router,
    uri: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> (StatusCode, HeaderMap, Value) {
    let mut builder = Request::builder().method(Method::POST).uri(uri);
    if let Some(t) = token {
        builder = builder.header("Authorization", format!("Bearer {t}"));
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
    let bytes = axum::body::to_bytes(response.into_body(), 32 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(json!({"error": "not json"})),
    )
}

async fn post_json(app: &mut axum::Router, uri: &str, body: Value, token: &str) -> Value {
    request(app, uri, Some(body), Some(token)).await.2
}

async fn register_and_login(app: &mut axum::Router) -> String {
    let username = format!("prev_{}", &Uuid::new_v4().to_string()[..8]);
    let _ = post_json(
        app,
        "/api/v1/auth/register",
        json!({
            "username": username,
            "email": format!("{username}@test.com"),
            "password": "test123",
            "display_name": "预览端点测试用户"
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
        .unwrap_or_else(|| panic!("create question failed: {resp}"))
        .parse()
        .unwrap()
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

fn exam_body(ids: &[Uuid]) -> Value {
    json!({
        "title": "集合单元测验",
        "exam_meta": { "school": "实验中学", "duration": 90, "total_score": 13 },
        "mode": "student",
        "sections": [{
            "title": "一、填空题",
            "questions": ids.iter().map(|id| json!({ "id": id, "default_score": 5 })).collect::<Vec<_>>()
        }],
        "options": { "include_answer": false, "include_analysis": false }
    })
}

/// 请求体本身也要过一遍真实的 `ExamRequest` 反序列化（与 `/export/pdf` 同一条口径）
fn with_spec(body: Value, spec: &LayoutSpec) -> Value {
    let mut req: ExamRequest = serde_json::from_value(body).expect("请求体必须是合法 ExamRequest");
    req.spec = Some(spec.clone());
    serde_json::to_value(&req).unwrap()
}

async fn preview(app: &mut axum::Router, token: &str, body: Value) -> (StatusCode, Value) {
    let (status, _, resp) = request(app, "/api/v1/typeset/preview", Some(body), Some(token)).await;
    (status, resp)
}

/// 预览响应的最小形状检查：裸载荷、四字段、每页都是整页 SVG
fn assert_preview_shape(resp: &Value) {
    assert_eq!(
        resp["page_count"].as_u64().unwrap() as usize,
        resp["pages"].as_array().unwrap().len(),
        "page_count 必须与 pages 长度同源（同一次编译的产物）"
    );
    assert!(resp["issues"].is_array(), "issues 必须是数组：{resp}");
    assert!(resp["warnings"].is_array(), "warnings 必须是数组：{resp}");
    for page in resp["pages"].as_array().unwrap() {
        let svg = page.as_str().unwrap();
        assert!(
            svg.starts_with("<svg") && svg.ends_with("</svg>"),
            "页不是整页 SVG：{}",
            &svg[..60.min(svg.len())]
        );
    }
}

fn svg_width(page: &str) -> f32 {
    let at = page.find("width=\"").expect("SVG 根元素没有 width");
    let nums: String = page[at + 7..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    nums.parse()
        .unwrap_or_else(|_| panic!("width 读数异常：{}", &page[at..at + 30]))
}

/// 鉴权：预览不是公开接口，没 token 一律 401
#[tokio::test]
async fn preview_requires_authentication() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过预览端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let (status, _, _) = request(&mut app, "/api/v1/typeset/preview", Some(json!({})), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn preview_returns_one_svg_per_page() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过预览端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;
    let fill_id = create_question(
        &mut app,
        &token,
        fill_json("已知集合 $A=\\{1,2\\}$，子集个数为______"),
    )
    .await;
    let choice_id = create_question(&mut app, &token, choice_json("下列说法正确的是")).await;

    let (status, resp) = preview(&mut app, &token, exam_body(&[fill_id, choice_id])).await;
    assert_eq!(status, StatusCode::OK, "预览失败：{resp}");
    assert_preview_shape(&resp);
    let pages = resp["pages"].as_array().unwrap();
    assert!(!pages.is_empty(), "一页 SVG 都没有");
    // 学生练习预设 = A4 双栏（§6.1），两题一页就排完
    assert_eq!(pages.len(), 1, "两题的卷子不该分页：{resp}");
    assert!(
        (svg_width(pages[0].as_str().unwrap()) - A4_WIDTH_PT).abs() < 1.0,
        "学生预设不是 A4：{}",
        pages[0].as_str().unwrap()
    );
}

/// 请求带 `spec` 就整体换预设 —— 预览与导出走同一个 `resolve_spec`，两边不该排出两种纸
#[tokio::test]
async fn preview_honours_the_request_spec() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过预览端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;
    let ids = vec![
        create_question(
            &mut app,
            &token,
            fill_json("集合 $A=\\{1\\}$ 的子集个数为______"),
        )
        .await,
        create_question(&mut app, &token, choice_json("下列说法正确的是")).await,
    ];

    let a3 = LayoutSpec::preset("a3_tri_exam").expect("预设存在");
    let (status, resp) = preview(&mut app, &token, with_spec(exam_body(&ids), &a3)).await;
    assert_eq!(status, StatusCode::OK, "预览失败：{resp}");
    assert_preview_shape(&resp);
    let width = svg_width(resp["pages"][0].as_str().unwrap());
    assert!(
        (width - A3_LANDSCAPE_WIDTH_PT).abs() < 1.0,
        "A3 对折三栏预设没换成 A3 横版：{width}pt"
    );
}

/// 一条坏公式不许弄坏整卷：降级记进 `issues`，其余各页照常出
#[tokio::test]
async fn preview_degrades_bad_latex_without_failing_the_paper() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过预览端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;
    // `\argmax_x`：mitex 转得动、typst 不认，静态守卫会拦下来降级（同 /export/pdf 那枚探针）
    let bad_id = create_question(
        &mut app,
        &token,
        fill_json("计算 $\\argmax_x f$ 与 $y^2$ 的值"),
    )
    .await;
    let good_id = create_question(
        &mut app,
        &token,
        fill_json("已知 $A=\\{1,2\\}$，子集个数为______"),
    )
    .await;

    let (status, resp) = preview(&mut app, &token, exam_body(&[bad_id, good_id])).await;
    assert_eq!(status, StatusCode::OK, "单处坏公式不得中断整卷预览");
    assert_preview_shape(&resp);
    let issues = resp["issues"].as_array().unwrap();
    let bad = issues
        .iter()
        .find(|i| i["latex"] == json!(r"\argmax_x f"))
        .unwrap_or_else(|| panic!("降级警告缺 latex 字段：{issues:?}"));
    assert_eq!(bad["field"], "stem");
    assert_eq!(bad["question_no"], json!(1));
    assert_eq!(bad["severity"], "warning");
}

/// 预检清单确实随预览回来（§6.5）：溢流是帧树事实，typst 自己一句诊断都不给
#[tokio::test]
async fn preview_carries_the_preflight_report() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过预览端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;
    // 400 个不间断拉丁字母：A4 双栏的栏宽约 85mm，这一串一定画出纸外
    let loud_id = create_question(
        &mut app,
        &token,
        fill_json(&format!("{}______", "A".repeat(400))),
    )
    .await;
    let calm_id = create_question(
        &mut app,
        &token,
        fill_json("已知 $A=\\{1,2\\}$，子集个数为______"),
    )
    .await;

    let (status, resp) = preview(&mut app, &token, exam_body(&[loud_id, calm_id])).await;
    assert_eq!(status, StatusCode::OK, "预览失败：{resp}");
    let issues = resp["issues"].as_array().unwrap();
    let overflow = issues
        .iter()
        .find(|i| i["reason"].as_str().is_some_and(|r| r.contains("超出纸张")))
        .unwrap_or_else(|| panic!("溢流没被预检报出来：{issues:?}"));
    assert_eq!(overflow["severity"], "warning");
    let reason = overflow["reason"].as_str().unwrap();
    assert!(reason.contains('A'), "溢流该带上原文片段：{reason}");
    // R14：预览面板按 `page` 跳页，所以这个数字必须真的到了客户端，且与文案里的页码同值
    let page = overflow["page"]
        .as_u64()
        .unwrap_or_else(|| panic!("预检条目没带机器可读页码：{overflow}"));
    assert!(
        page >= 1 && page <= resp["page_count"].as_u64().unwrap(),
        "页码越界：{page} / 共 {} 页",
        resp["page_count"].as_u64().unwrap()
    );
    assert!(
        reason.contains(&format!("第 {page} 页")),
        "`page` 与文案里的页码分叉：{reason}"
    );

    // 好那一题不该被连坐：整页 SVG 照常出
    assert_preview_shape(&resp);
}

/// 响应是 JSON 而不是文件：预览没有 `Content-Disposition`
#[tokio::test]
async fn preview_is_not_a_file_download() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过预览端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;
    let id = create_question(
        &mut app,
        &token,
        fill_json("已知 $A=\\{1\\}$，子集个数为______"),
    )
    .await;
    let (status, headers, _) = request(
        &mut app,
        "/api/v1/typeset/preview",
        Some(exam_body(&[id])),
        Some(&token),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/json"),
        "预览的响应应当是 JSON"
    );
    assert!(
        headers.get(header::CONTENT_DISPOSITION).is_none(),
        "预览不是下载，不该带文件名",
    );
}
