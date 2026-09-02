//! T2.8 导出端点集成测试：`POST /api/v1/export/docx`
//!
//! 覆盖端点契约（实施计划 §四 + T2.8 DoD）：鉴权、OOXML content type、RFC 5987 中文文件名、
//! OPC 包结构（zip 魔数 + 必需部件）、**公式真变成可编辑 OMML**（`m:oMath` 计数与题面
//! 公式数一致、display 那条外套 `m:oMathPara`），以及「一处坏公式不中断整卷」（200 +
//! `X-Export-Warnings` 非空 + 好公式照常产出）。
//!
//! 复用 tests/api.rs 的测试库约定：未设置 `DATABASE_URL_TEST` 时跳过。

use axum::{
    body::Body,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use mathset::AppState;
use mathset::build_app;
use mathset::db;
use mathset::export::docx::{NS_M, NS_W};
use serde_json::{Value, json};
use std::io::{Cursor, Read};
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

/// 原始请求：返回状态码 + 响应头 + 字节体（文件类响应不是 JSON）
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
    let bytes = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    (status, headers, bytes.to_vec())
}

async fn post_json(app: &mut axum::Router, uri: &str, body: Value, token: &str) -> Value {
    let (_, _, bytes) = request_raw(app, Method::POST, uri, Some(body), Some(token)).await;
    serde_json::from_slice(&bytes).unwrap_or(json!({"error": "parse failed"}))
}

async fn register_and_login(app: &mut axum::Router) -> String {
    let username = format!("expdocx_{}", &Uuid::new_v4().to_string()[..8]);
    let _ = post_json(
        app,
        "/api/v1/auth/register",
        json!({
            "username": username,
            "email": format!("{}@test.com", username),
            "password": "test123",
            "display_name": "DOCX 导出测试用户"
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

/// 解包 → 部件文本。缺失部件直接 panic（OPC 包少了它 Word 会报「文件已损坏」）
fn part(bytes: &[u8], name: &str) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .unwrap_or_else(|e| panic!("响应不是合法 zip: {e}"));
    // 先取包内清单：by_name 会可变借用 archive，之后就拿不到名字列表了
    let names: Vec<String> = archive.file_names().map(String::from).collect();
    let mut entry = match archive.by_name(name) {
        Ok(e) => e,
        Err(_) => panic!("缺部件 {name}（包内 {names:?}）"),
    };
    let mut buf = String::new();
    entry.read_to_string(&mut buf).unwrap();
    buf
}

/// 部件内是否存在（用于只判存在性的场景，如 `[Content_Types].xml` 的 Override）
fn has_part(bytes: &[u8], name: &str) -> bool {
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes)) else {
        return false;
    };
    archive.by_name(name).is_ok()
}

/// 按命名空间 + 局部名数元素（`m:oMath` 与 `m:oMathPara` 不能靠子串匹配区分）
fn count_nodes(xml: &str, ns: &str, local: &str) -> usize {
    let doc = roxmltree::Document::parse(xml).expect("部件必须是良构 XML");
    doc.descendants()
        .filter(|n| n.is_element() && n.has_tag_name((ns, local)))
        .count()
}

/// 纸上正文：按文档顺序拼所有 `w:t` / `m:t`。
/// 文字断言一律走这里 —— 直接对 XML 串取子串会因为 run 切分而假失败。
fn text_of(xml: &str) -> String {
    let doc = roxmltree::Document::parse(xml).expect("部件必须是良构 XML");
    doc.descendants()
        .filter(|n| n.has_tag_name((NS_W, "t")) || n.has_tag_name((NS_M, "t")))
        .map(|n| n.text().unwrap_or_default())
        .collect()
}

/// 两道题的题干（选项 C 另带 1 条行内公式）
const FILL_STEM: &str = "已知集合 $A=\\{1,2\\}$，则 $A$ 的子集个数为______个";
const CHOICE_STEM: &str =
    "已知函数 $$f(x)=\\begin{cases}x^2,&x\\ge 0\\\\-x,&x<0\\end{cases}$$，则下列说法正确的是";
/// 题面公式总数：填空干 2 条行内 + 选择题干 1 条 display + 选项 C 1 条行内
/// （options 关掉答案与解析，解析里的 `$2^n$` 不参与计数）
const PAPER_FORMULAS: usize = 4;

fn exam_body(fill_id: Uuid, choice_id: Uuid, options: Value) -> Value {
    json!({
        "title": "集合单元测验",
        "exam_meta": {
            "school": "实验中学",
            "duration": 90,
            "total_score": 13,
            "instructions": ["闭卷作答，时长 90 分钟"]
        },
        "mode": "student",
        "sections": [
            {
                "title": "一、填空题",
                "questions": [{ "id": fill_id, "default_score": 8 }]
            },
            {
                "title": "二、单选题",
                "instruction": "每题 5 分",
                "questions": [{ "id": choice_id, "default_score": 5 }]
            }
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

#[tokio::test]
async fn docx_export_requires_auth() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过 DOCX 导出端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let (status, _, _) = request_raw(
        &mut app,
        Method::POST,
        "/api/v1/export/docx",
        Some(json!({ "title": "x", "mode": "student", "sections": [] })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn docx_export_is_a_package_with_editable_math() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过 DOCX 导出端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;
    let fill_id = create_question(&mut app, &token, fill_json(FILL_STEM)).await;
    let choice_id = create_question(&mut app, &token, choice_json(CHOICE_STEM)).await;

    let (status, headers, bytes) = request_raw(
        &mut app,
        Method::POST,
        "/api/v1/export/docx",
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
        Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
    );

    // 与 markdown 端点同一套文件名契约：ASCII 回退 + RFC 5987 UTF-8 名（"集" = E9 9B 86）
    let cd = header_str(&headers, header::CONTENT_DISPOSITION).expect("content-disposition");
    assert!(cd.contains("filename*=UTF-8''%E9%9B%86"), "{}", cd);
    assert!(cd.ends_with(".docx"), "{}", cd);

    // OPC 包：zip 魔数 + 必需部件
    assert_eq!(&bytes[..4], b"PK\x03\x04", "docx 必须是 zip");
    for name in [
        "[Content_Types].xml",
        "_rels/.rels",
        "word/_rels/document.xml.rels",
        "word/document.xml",
        "word/styles.xml",
        "word/settings.xml",
        "word/footer1.xml",
    ] {
        assert!(has_part(&bytes, name), "缺部件 {name}");
    }

    let doc = part(&bytes, "word/document.xml");
    // DoD：公式是真 OMML 对象，数量与题面公式数一致；display 那条单独套 oMathPara
    assert_eq!(
        count_nodes(&doc, NS_M, "oMath"),
        PAPER_FORMULAS,
        "m:oMath 数应等于题面公式数"
    );
    assert_eq!(count_nodes(&doc, NS_M, "oMathPara"), 1);
    let text = text_of(&doc);
    // 关掉解析开关后，解析里的 $2^n$ 不该出现在卷上
    assert!(!text.contains("2^n"), "未开启解析开关时解析文本不得入卷");
    // 中文正文与选项照常
    for want in [
        "已知集合",
        "下列说法正确的是",
        "空集是任何集合的子集",
        "一、填空题",
    ] {
        assert!(text.contains(want), "缺「{want}」");
    }
    // 没有 LaTeX 原文残留（分段函数经归一后可编译）
    assert!(
        !text.contains("\\begin{cases}"),
        "display 公式应以 OMML 呈现"
    );
    assert!(!text.contains("PARSE ERROR"), "归一后的公式不得再降级");
    assert!(
        headers.get("X-Export-Warnings").is_none(),
        "全部题目可见且公式可编译 → 不应有警告头"
    );
}

#[tokio::test]
async fn docx_export_degrades_bad_latex_without_failing_the_paper() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过 DOCX 导出端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;
    // 坏公式与好公式混在同一道题、同一份卷里
    let bad_fill_id = create_question(
        &mut app,
        &token,
        fill_json("计算 $\\frac{1}{$ 与 $y^2$ 的值"),
    )
    .await;
    let choice_id = create_question(&mut app, &token, choice_json(CHOICE_STEM)).await;

    let (status, headers, bytes) = request_raw(
        &mut app,
        Method::POST,
        "/api/v1/export/docx",
        Some(exam_body(
            bad_fill_id,
            choice_id,
            json!({ "include_answer": false, "include_analysis": false }),
        )),
        Some(&token),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "单处坏公式不得中断整卷导出");
    let doc = part(&bytes, "word/document.xml");
    // 坏公式降级成原文，好公式照常是 OMML：4 条公式里 3 条成对象
    assert_eq!(count_nodes(&doc, NS_M, "oMath"), PAPER_FORMULAS - 1);
    let text = text_of(&doc);
    assert!(text.contains(r"\frac{1}{"), "降级公式须以原文留在纸上");
    assert!(text.contains("与"), "坏公式所在题干的其他文字不得丢");

    let raw = header_str(&headers, "X-Export-Warnings").expect("warnings header");
    assert!(raw.is_ascii());
    let arr: Value = serde_json::from_str(&percent_decode(&raw)).expect("警告头是 JSON 数组");
    let items = arr.as_array().unwrap();
    assert!(!items.is_empty());
    let bad = items
        .iter()
        .find(|i| i["latex"] == json!(r"\frac{1}{"))
        .unwrap_or_else(|| panic!("降级警告缺 latex 字段: {items:?}"));
    assert_eq!(bad["field"], "stem");
    assert_eq!(bad["question_no"], json!(1));
    assert!(
        !bad["reason"].as_str().unwrap().is_empty(),
        "降级警告必须给出原因: {bad}"
    );
    // 未超上限 → 不该出现截断哨兵
    assert!(
        items.iter().all(|i| i.get("truncated").is_none()),
        "{items:?}"
    );
}

#[tokio::test]
async fn docx_export_survives_invisible_questions() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过 DOCX 导出端点测试: DATABASE_URL_TEST 未设置");
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
        "/api/v1/export/docx",
        Some(body),
        Some(&token),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "缺失题应跳过并记警告");
    let text = text_of(&part(&bytes, "word/document.xml"));
    assert!(text.contains("已知集合"), "可见题题干必须保留");
    // 教师卷 + 卷末汇总：答案区在，且缺失题的警告进了头
    assert!(text.contains("参考答案"), "卷末答案区缺失");
    let raw = header_str(&headers, "X-Export-Warnings").expect("warnings header");
    let arr: Value = serde_json::from_str(&percent_decode(&raw)).unwrap();
    let items = arr.as_array().unwrap();
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["field"], "other");
    assert!(items[0]["reason"].as_str().unwrap().contains("不存在"));
}
