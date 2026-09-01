//! T1.7 导出端点集成测试：`POST /api/v1/export/markdown[?bundle=true]`
//!
//! 覆盖端点契约（实施计划 §四 + T1.7 DoD）：鉴权、Content-Type、
//! `Content-Disposition` RFC 5987 中文文件名、`X-Export-Warnings` 警告头
//! （含 B3 截断语义）、bundle zip 与图片重写。
//!
//! 复用 tests/api.rs 的测试库约定：未设置 `DATABASE_URL_TEST` 时跳过。

use axum::{
    body::Body,
    http::{header, HeaderMap, Method, Request, StatusCode},
};
use mathset::build_app;
use mathset::db;
use mathset::AppState;
use serde_json::{json, Value};
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
    let username = format!("expapi_{}", &Uuid::new_v4().to_string()[..8]);
    let _ = post_json(
        app,
        "/api/v1/auth/register",
        json!({
            "username": username,
            "email": format!("{}@test.com", username),
            "password": "test123",
            "display_name": "导出端点测试用户"
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

fn header_str(
    headers: &HeaderMap,
    name: impl header::AsHeaderName,
) -> Option<String> {
    headers.get(name).and_then(|v| v.to_str().ok()).map(String::from)
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

/// 一道填空题 + 一道选择题的标准请求体
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

const FILL_STEM: &str = "已知集合 $A=\\{1,2\\}$，则 $A$ 的子集个数为______个";
const CHOICE_STEM: &str = "下列说法正确的是";

#[tokio::test]
async fn markdown_export_requires_auth() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过导出端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let (status, _, _) = request_raw(
        &mut app,
        Method::POST,
        "/api/v1/export/markdown",
        Some(json!({ "title": "x", "mode": "student", "sections": [] })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn markdown_export_returns_file_with_rfc5987_chinese_name() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过导出端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;

    let fill_id = create_question(
        &mut app,
        &token,
        json!({
            "stem": FILL_STEM,
            "question_type": "fill",
            "difficulty": 3,
            "correct_answer": {"kind": "fill", "value": {"blanks": [{"position": 1, "answer": "4"}]}},
            "analysis": "子集个数公式 $2^n$"
        }),
    )
    .await;
    let choice_id = create_question(
        &mut app,
        &token,
        json!({
            "stem": CHOICE_STEM,
            "question_type": "choice",
            "difficulty": 2,
            "options": [
                {"label": "A", "content": "空集没有子集"},
                {"label": "B", "content": "空集是任何集合的子集"},
                {"label": "C", "content": "0 表示空集"},
                {"label": "D", "content": "{0} 是空集"}
            ],
            "correct_answer": {"kind": "choice", "value": {"options": ["B"]}}
        }),
    )
    .await;

    let (status, headers, bytes) = request_raw(
        &mut app,
        Method::POST,
        "/api/v1/export/markdown",
        Some(exam_body(fill_id, choice_id, json!({}))),
        Some(&token),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_str(&headers, header::CONTENT_TYPE).as_deref(),
        Some("text/markdown; charset=utf-8")
    );

    // RFC 5987：ASCII 回退名 + filename*=UTF-8''<pct-encoded>（"集" = E9 9B 86）
    let cd = header_str(&headers, header::CONTENT_DISPOSITION).expect("content-disposition");
    assert!(cd.starts_with("attachment; filename="), "{}", cd);
    assert!(cd.contains("filename*=UTF-8''%E9%9B%86"), "{}", cd);
    assert!(cd.ends_with(".md"), "{}", cd);

    let md = String::from_utf8(bytes).unwrap();
    // frontmatter 与卷头
    assert!(md.starts_with("---\n"), "should start with YAML frontmatter");
    assert!(md.contains("title: \"集合单元测验\""));
    assert!(md.contains("mode: student"));
    assert!(md.contains("实验中学"));
    assert!(md.contains("**考试说明**"));
    // 大题统计 + 大题说明引用块
    assert!(md.contains("## 一、填空题（共 1 题 · 8 分）"), "{}", md);
    assert!(md.contains("## 二、单选题（共 1 题 · 5 分）"));
    assert!(md.contains("> 每题 5 分"));
    // 题号连续、公式原样保留（KaTeX 可直接渲染）
    assert!(md.contains("**1.**（8 分）"));
    assert!(md.contains("$A=\\{1,2\\}$"), "formula must stay verbatim");
    // B2：stem 已含挖空 → 直接沿用（文本段经 escape_md，下划线转义后仍渲染为 ______）
    assert!(
        md.replace('\\', "").contains("______"),
        "existing blanks reused (B2)"
    );
    assert!(md.contains("**2.**（5 分）"));
    assert!(md.contains("B. 空集是任何集合的子集"));
    // 默认 options：含答案 + 卷末汇总
    assert!(md.contains("## 参考答案"));
    assert!(md.contains("2. B"));
    assert!(!md.contains("**答案**：B"), "内嵌答案仅在 answer_at_end=false 时出现");
    // 未开解析开关
    assert!(!md.contains("## 试题解析"));
    // 全部题目可见 → 无警告头
    assert!(
        headers.get("X-Export-Warnings").is_none(),
        "unexpected warnings header"
    );
}

#[tokio::test]
async fn markdown_export_switches_embed_answers_and_analysis() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过导出端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;
    let fill_id = create_question(
        &mut app,
        &token,
        json!({
            "stem": FILL_STEM,
            "question_type": "fill",
            "difficulty": 3,
            "correct_answer": {"kind": "fill", "value": {"blanks": [{"position": 1, "answer": "4"}]}},
            "analysis": "子集个数公式 $2^n$"
        }),
    )
    .await;
    let choice_id = create_question(
        &mut app,
        &token,
        json!({
            "stem": CHOICE_STEM,
            "question_type": "choice",
            "difficulty": 2,
            "options": [{"label": "A", "content": "甲"}, {"label": "B", "content": "乙"}],
            "correct_answer": {"kind": "choice", "value": {"options": ["B"]}},
            "analysis": "选 B 的理由"
        }),
    )
    .await;

    let body = exam_body(
        fill_id,
        choice_id,
        json!({ "include_answer": true, "include_analysis": true, "answer_at_end": false }),
    );
    let (status, _, bytes) = request_raw(
        &mut app,
        Method::POST,
        "/api/v1/export/markdown",
        Some(body),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let md = String::from_utf8(bytes).unwrap();
    // 内嵌模式：答案/解析跟在题后，不出卷末汇总区
    assert!(md.contains("**答案**：B"));
    assert!(md.contains("**解析**："));
    assert!(md.contains("$2^n$"));
    assert!(!md.contains("## 参考答案"));
    assert!(!md.contains("## 试题解析"));

    // 学生卷（不含答案）：题干完整但无答案区
    let mut bare = exam_body(fill_id, choice_id, json!({ "include_answer": false }));
    bare["options"] = json!({ "include_answer": false, "include_analysis": false });
    let (_, _, bytes) = request_raw(
        &mut app,
        Method::POST,
        "/api/v1/export/markdown",
        Some(bare),
        Some(&token),
    )
    .await;
    let md = String::from_utf8(bytes).unwrap();
    assert!(md.contains(CHOICE_STEM));
    assert!(!md.contains("**答案**"));
    assert!(!md.contains("## 参考答案"));
}

#[tokio::test]
async fn markdown_export_survives_invisible_questions_and_emits_warnings() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过导出端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;
    let fill_id = create_question(
        &mut app,
        &token,
        json!({
            "stem": FILL_STEM,
            "question_type": "fill",
            "difficulty": 3,
            "correct_answer": {"kind": "fill", "value": {"blanks": [{"position": 1, "answer": "4"}]}}
        }),
    )
    .await;
    let missing = Uuid::new_v4();

    // 请求里混入不存在的题：整卷不中断，200 + 警告头
    let body = json!({
        "title": "含缺失题的测验",
        "mode": "student",
        "sections": [{
            "title": "一、填空题",
            "questions": [{ "id": missing }, { "id": fill_id }]
        }],
        "options": { "include_answer": false }
    });
    let (status, headers, bytes) = request_raw(
        &mut app,
        Method::POST,
        "/api/v1/export/markdown",
        Some(body),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "缺失题不得中断整卷导出");
    let md = String::from_utf8(bytes).unwrap();
    assert!(md.contains(r"$A=\{1,2\}$"), "可见题题干应保留: {}", md);
    // 缺失题被跳过，可见题仍从 1 起连续编号
    assert!(md.contains("**1.**（"), "{}", md);
    assert!(!md.contains("**2.**（"));

    let raw = header_str(&headers, "X-Export-Warnings").expect("warnings header");
    // 头值必须是纯 ASCII（已 percent-encode）
    assert!(raw.is_ascii());
    assert!(raw.len() <= 8000, "warnings header must respect size limit");
    let decoded = percent_decode(&raw);
    let arr: Value = serde_json::from_str(&decoded).expect("warnings must be JSON array");
    let items = arr.as_array().unwrap();
    assert_eq!(items.len(), 1, "{:?}", items);
    assert_eq!(items[0]["field"], "other");
    assert!(items[0]["reason"].as_str().unwrap().contains("不存在"));
    assert!(items[0].get("truncated").is_none());
    assert_eq!(items[0]["question_no"], Value::Null);
}

#[tokio::test]
async fn markdown_bundle_returns_zip_with_rewritten_local_images() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过导出端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;

    // 落一张本地图到 upload_dir（AppState 用 ./uploads），供 /uploads/** 映射命中
    let dir = std::path::Path::new("./uploads/questions");
    std::fs::create_dir_all(dir).unwrap();
    let name = format!("export-test-{}.png", Uuid::new_v4());
    let path = dir.join(&name);
    let png: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    std::fs::write(&path, &png).unwrap();

    let fill_id = create_question(
        &mut app,
        &token,
        json!({
            "stem": format!("如图所示，求阴影部分。![图1](/uploads/questions/{})", name),
            "question_type": "fill",
            "difficulty": 3,
            "correct_answer": {"kind": "fill", "value": {"blanks": [{"position": 1, "answer": "A"}]}}
        }),
    )
    .await;
    let choice_id = create_question(
        &mut app,
        &token,
        json!({
            "stem": CHOICE_STEM,
            "question_type": "choice",
            "difficulty": 2,
            "options": [{"label": "A", "content": "甲"}, {"label": "B", "content": "乙"}],
            "correct_answer": {"kind": "choice", "value": {"options": ["A"]}}
        }),
    )
    .await;

    let (status, headers, bytes) = request_raw(
        &mut app,
        Method::POST,
        "/api/v1/export/markdown?bundle=true",
        Some(exam_body(fill_id, choice_id, json!({}))),
        Some(&token),
    )
    .await;
    let _ = std::fs::remove_file(&path);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_str(&headers, header::CONTENT_TYPE).as_deref(),
        Some("application/zip")
    );
    let cd = header_str(&headers, header::CONTENT_DISPOSITION).expect("content-disposition");
    assert!(cd.ends_with(".zip"), "{}", cd);

    // zip 结构：exam.md + images/*.png，且 md 内 URL 已重写为相对路径
    let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let names: Vec<String> = archive.file_names().map(String::from).collect();
    assert!(names.contains(&"exam.md".to_string()), "{:?}", names);
    assert_eq!(names.iter().filter(|n| n.starts_with("images/")).count(), 1);
    let mut entry = archive.by_name("exam.md").unwrap();
    let mut md = String::new();
    entry.read_to_string(&mut md).unwrap();
    assert!(md.contains("](images/"), "{}", md);
    assert!(!md.contains(&name), "原始 /uploads URL 应被重写");
}

/// 空题单请求（sections 全空）应返回 200 与仅含卷头的 md
#[tokio::test]
async fn markdown_export_accepts_empty_sections() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("⚠️  跳过导出端点测试: DATABASE_URL_TEST 未设置");
        return;
    };
    let token = register_and_login(&mut app).await;
    let (status, headers, bytes) = request_raw(
        &mut app,
        Method::POST,
        "/api/v1/export/markdown",
        Some(json!({
            "title": "空卷",
            "mode": "student",
            "sections": [{ "title": "一、填空题", "questions": [] }]
        })),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_str(&headers, header::CONTENT_TYPE).as_deref(),
        Some("text/markdown; charset=utf-8")
    );
    let md = String::from_utf8(bytes).unwrap();
    assert!(md.contains("title: \"空卷\""));
    assert!(md.contains("## 一、填空题（共 0 题 · 0 分）"), "{}", md);
}
