// V2.1.1 P0-A：Document 上传 / 分类 / 确认 集成测试
//
// 复用 tests/api.rs 的真实 DB 模式：create_test_app → run_migrations → oneshot Router。
// 注意：分类端点依赖真实 LLM API Key，本文件不触发真实分类调用
// （仅验证 404 等不依赖 LLM 的路径）；Case 12 分类 fallback 走人工验收。

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use base64::Engine as _;
use mathset::build_app;
use mathset::db;
use mathset::AppState;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// 辅助函数（与 tests/api.rs 一致的私有实现）
// ---------------------------------------------------------------------------

async fn create_test_app() -> Option<axum::Router> {
    let _ = dotenvy::dotenv();
    let database_url = std::env::var("DATABASE_URL_TEST")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .ok()?;
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
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
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
    let body_bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: Value =
        serde_json::from_slice(&body_bytes).unwrap_or(json!({"error": "parse failed"}));
    (status, json)
}

async fn request_auth(
    app: &mut axum::Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    token: &str,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", format!("Bearer {}", token));
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
    let body_bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: Value =
        serde_json::from_slice(&body_bytes).unwrap_or(json!({"error": "parse failed"}));
    (status, json)
}

async fn post_auth(app: &mut axum::Router, uri: &str, body: Value, token: &str) -> (StatusCode, Value) {
    request_auth(app, Method::POST, uri, Some(body), token).await
}

async fn get_auth(app: &mut axum::Router, uri: &str, token: &str) -> (StatusCode, Value) {
    request_auth(app, Method::GET, uri, None, token).await
}

async fn register_and_login(app: &mut axum::Router) -> String {
    let username = format!("doc_{}", Uuid::new_v4().to_string().split('-').next().unwrap());
    let email = format!("{}@test.com", username);
    let (_, _) = request(
        app,
        Method::POST,
        "/api/v1/auth/register",
        Some(json!({
            "username": username,
            "email": email,
            "password": "test123",
            "display_name": "测试用户"
        })),
    )
    .await;
    let (_, body) = request(
        app,
        Method::POST,
        "/api/v1/auth/login",
        Some(json!({ "username": username, "password": "test123" })),
    )
    .await;
    body["token"].as_str().unwrap().to_string()
}

/// 20x20 合法 PNG（满足视觉模型最小尺寸限制）
const PNG_1PX_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAABQAAAAUCAIAAAAC64paAAAAEklEQVR4nGNgGAWjYBSMgqELAATEAAE0eCSYAAAAAElFTkSuQmCC";

async fn upload_multipart(app: &mut axum::Router, token: &str, field_name: &str) -> (StatusCode, Value) {
    let png = base64::engine::general_purpose::STANDARD
        .decode(PNG_1PX_B64)
        .unwrap();
    let boundary = "----dshtestboundary";
    let body = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"{f}\"; filename=\"page1.png\"\r\nContent-Type: image/png\r\n\r\n",
        b = boundary,
        f = field_name
    )
    .into_bytes();
    let mut body = body;
    body.extend_from_slice(&png);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/ai/documents")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    let status = response.status();
    let body_bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: Value =
        serde_json::from_slice(&body_bytes).unwrap_or(json!({"error": "parse failed"}));
    (status, json)
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_document_upload_list_get() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL");
        return;
    };
    let token = register_and_login(&mut app).await;

    // 上传
    let (status, body) = upload_multipart(&mut app, &token, "pages").await;
    assert_eq!(status, StatusCode::CREATED, "上传失败: {body}");
    let doc = &body["data"];
    assert_eq!(doc["status"], "uploaded");
    assert_eq!(doc["page_count"], 1);
    assert_eq!(doc["mime"], "image/png");
    assert_eq!(doc["document_type"], Value::Null);
    let doc_id = doc["id"].as_str().unwrap().to_string();

    // 列表
    let (status, body) = get_auth(&mut app, "/api/v1/ai/documents", &token).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["id"].as_str())
        .collect();
    assert!(ids.contains(&doc_id.as_str()));

    // 详情
    let (status, body) = get_auth(&mut app, &format!("/api/v1/ai/documents/{doc_id}"), &token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["id"], doc_id);
}

#[tokio::test]
async fn test_document_upload_rejects_non_image() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL");
        return;
    };
    let token = register_and_login(&mut app).await;

    let boundary = "----dshtestboundary";
    let body = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"pages\"; filename=\"evil.txt\"\r\nContent-Type: text/plain\r\n\r\nnot an image\r\n--{b}--\r\n",
        b = boundary
    );
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/ai/documents")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn test_document_upload_requires_pages() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL");
        return;
    };
    let token = register_and_login(&mut app).await;
    let (status, _) = post_auth(&mut app, "/api/v1/ai/documents", json!({}), &token).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_document_upload_rejects_tiny_image() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL");
        return;
    };
    let token = register_and_login(&mut app).await;

    // 1x1 PNG：视觉模型要求宽高 > 10 → 400
    let tiny = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==")
        .unwrap();
    let boundary = "----dshtestboundary";
    let mut body = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"pages\"; filename=\"tiny.png\"\r\nContent-Type: image/png\r\n\r\n",
        b = boundary
    )
    .into_bytes();
    body.extend_from_slice(&tiny);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/ai/documents")
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_document_confirm_validation_branches() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL");
        return;
    };
    let token = register_and_login(&mut app).await;
    let (_, body) = upload_multipart(&mut app, &token, "pages").await;
    let doc_id = body["data"]["id"].as_str().unwrap().to_string();
    let base = format!("/api/v1/ai/documents/{doc_id}");

    // exam 缺 paper_meta → 400
    let (status, body) = post_auth(
        &mut app,
        &format!("{base}/confirm"),
        json!({ "document_type": "exam" }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // exam 带 paper_meta.title → 200，快照落库
    let (status, body) = post_auth(
        &mut app,
        &format!("{base}/confirm"),
        json!({
            "document_type": "exam",
            "paper_meta": {
                "title": "2025高一数学期中考试",
                "year": 2025,
                "stage": "senior",
                "grade": "高一",
                "subject": "数学",
                "semester": "first",
                "region_province": "浙江省",
                "region_city": "杭州市",
                "school_name": "示例中学",
                "source_type": "exam",
                "sub_source_type": "期中"
            }
        }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc = &body["data"];
    assert_eq!(doc["status"], "confirmed");
    assert_eq!(doc["document_type"], "exam");
    assert_eq!(doc["metadata"]["paper_meta"]["title"], "2025高一数学期中考试");
    assert_eq!(doc["title"], "2025高一数学期中考试");

    // unknown → 400（不允许提交）
    let (status, _) = post_auth(
        &mut app,
        &format!("{base}/confirm"),
        json!({ "document_type": "unknown" }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // other 缺 type_label → 400
    let (status, _) = post_auth(
        &mut app,
        &format!("{base}/confirm"),
        json!({ "document_type": "other" }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // other 带 type_label → 200
    let (status, body) = post_auth(
        &mut app,
        &format!("{base}/confirm"),
        json!({ "document_type": "other", "type_label": "校本资料", "title": "导数专题" }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["type_label"], "校本资料");
    // 自动补默认单集合
    let collections = body["data"]["metadata"]["collections"].as_array().unwrap();
    assert_eq!(collections.len(), 1);
    assert_eq!(collections[0]["collection_type"], "other");

    // mixed 缺 collections → 400
    let (status, _) = post_auth(
        &mut app,
        &format!("{base}/confirm"),
        json!({ "document_type": "mixed" }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // mixed 带集合 → 200
    let (status, body) = post_auth(
        &mut app,
        &format!("{base}/confirm"),
        json!({
            "document_type": "mixed",
            "collections": [
                { "title": "课堂例题", "collection_type": "class_example" },
                { "title": "课堂练习", "collection_type": "class_exercise" }
            ]
        }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let collections = body["data"]["metadata"]["collections"].as_array().unwrap();
    assert_eq!(collections.len(), 2);

    // 非试卷类型自动补默认单集合（class_exercise）
    let (status, body) = post_auth(
        &mut app,
        &format!("{base}/confirm"),
        json!({ "document_type": "class_exercise", "title": "二次函数课堂练习" }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let collections = body["data"]["metadata"]["collections"].as_array().unwrap();
    assert_eq!(collections.len(), 1);
    assert_eq!(collections[0]["title"], "二次函数课堂练习");
    assert_eq!(collections[0]["collection_type"], "class_exercise");
}

#[tokio::test]
async fn test_document_permissions_and_classify_404() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL");
        return;
    };
    let token_a = register_and_login(&mut app).await;
    let token_b = register_and_login(&mut app).await;

    let (_, body) = upload_multipart(&mut app, &token_a, "pages").await;
    let doc_id = body["data"]["id"].as_str().unwrap().to_string();

    // 他人不可见（统一 404）
    let (status, _) = get_auth(&mut app, &format!("/api/v1/ai/documents/{doc_id}"), &token_b).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 他人不可分类
    let (status, _) = post_auth(
        &mut app,
        &format!("/api/v1/ai/documents/{doc_id}/classify"),
        json!({}),
        &token_b,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 不存在的文档 → 404
    let (status, _) = post_auth(
        &mut app,
        "/api/v1/ai/documents/00000000-0000-0000-0000-000000000000/classify",
        json!({}),
        &token_a,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
