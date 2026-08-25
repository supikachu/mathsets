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
    create_test_app_with_pool().await.map(|(app, _)| app)
}

async fn create_test_app_with_pool() -> Option<(axum::Router, sqlx::PgPool)> {
    let database_url = mathset::testing::database_url()?;
    let pool = db::create_pool(&database_url, 5).await;
    db::run_migrations(&pool).await;
    let state = AppState::new(
        pool.clone(),
        "test-secret-for-integration-tests".to_string(),
        24,
        mathset::config::AiConfig::from_env(),
        "./uploads".to_string(),
    );
    Some((build_app(state), pool))
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
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
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
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
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
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let token = register_and_login(&mut app).await;
    let (status, _) = post_auth(&mut app, "/api/v1/ai/documents", json!({}), &token).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_document_upload_rejects_tiny_image() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
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
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let token = register_and_login(&mut app).await;
    let (_, body) = upload_multipart(&mut app, &token, "pages").await;
    let doc_id = body["data"]["id"].as_str().unwrap().to_string();
    let base = format!("/api/v1/ai/documents/{doc_id}");

    // create_paper=true 但缺 paper_meta → 400
    let (status, body) = post_auth(
        &mut app,
        &format!("{base}/confirm"),
        json!({
            "source_category": "paper",
            "source_kind": "monthly_test",
            "create_paper": true
        }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // 旧扁平 exam 兼容写入 paper:monthly_test；未开 create_paper 时不强制 paper_meta
    let (status, body) = post_auth(
        &mut app,
        &format!("{base}/confirm"),
        json!({
            "document_type": "exam",
            "create_paper": true,
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
    assert_eq!(doc["document_type"], "paper:monthly_test");
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

    // other 带 type_label → 200，映射 other:special
    let (status, body) = post_auth(
        &mut app,
        &format!("{base}/confirm"),
        json!({ "document_type": "other", "type_label": "校本资料", "title": "导数专题" }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["type_label"], "校本资料");
    assert_eq!(body["data"]["document_type"], "other:special");

    // mixed 已废弃 → 400
    let (status, _) = post_auth(
        &mut app,
        &format!("{base}/confirm"),
        json!({ "document_type": "mixed" }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = post_auth(
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
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // 旧 class_exercise → practice:in_class；方案 A 不再自动建集合
    let (status, body) = post_auth(
        &mut app,
        &format!("{base}/confirm"),
        json!({ "document_type": "class_exercise", "title": "二次函数课堂练习" }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["document_type"], "practice:in_class");
    assert_eq!(body["data"]["title"], "二次函数课堂练习");
}

#[tokio::test]
async fn test_document_permissions_and_classify_404() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
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

#[tokio::test]
async fn test_confirm_create_paper_then_update_metadata() {
    let Some(mut app) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let token = register_and_login(&mut app).await;
    let (_, body) = upload_multipart(&mut app, &token, "pages").await;
    let doc_id = body["data"]["id"].as_str().unwrap().to_string();
    let base = format!("/api/v1/ai/documents/{doc_id}");

    // 第一次确认：仅标题（模拟用户刚打开「创建试卷」）
    let (status, body) = post_auth(
        &mut app,
        &format!("{base}/confirm"),
        json!({
            "source_category": "paper",
            "source_kind": "final",
            "create_paper": true,
            "paper_meta": { "title": "未命名资料" }
        }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let paper_id = body["paper_id"].as_str().expect("应创建试卷").to_string();

    let (status, paper) = get_auth(&mut app, &format!("/api/v1/papers/{paper_id}"), &token).await;
    assert_eq!(status, StatusCode::OK, "{paper}");
    assert_eq!(paper["title"], "未命名资料");
    assert!(paper["year"].is_null());
    assert!(paper["stage"].is_null());

    // 第二次确认：回写完整试卷信息（此前会因已有卷而跳过）
    let (status, body) = post_auth(
        &mut app,
        &format!("{base}/confirm"),
        json!({
            "source_category": "paper",
            "source_kind": "final",
            "create_paper": true,
            "paper_meta": {
                "title": "宁波市 2025 期末九校联考高一数学",
                "year": 2025,
                "stage": "senior",
                "grade": "高一",
                "subject": "数学",
                "semester": "second",
                "region_province": "浙江",
                "region_city": "宁波市"
            }
        }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["paper_id"], paper_id);

    let (status, paper) = get_auth(&mut app, &format!("/api/v1/papers/{paper_id}"), &token).await;
    assert_eq!(status, StatusCode::OK, "{paper}");
    assert_eq!(paper["title"], "宁波市 2025 期末九校联考高一数学");
    assert_eq!(paper["year"], 2025);
    assert_eq!(paper["stage"], "senior");
    assert_eq!(paper["grade"], "高一");
    assert_eq!(paper["semester"], "second");
    assert_eq!(paper["region_province"], "浙江");
    assert_eq!(paper["region_city"], "宁波市");
    assert_eq!(paper["source_type"], "final");

    let (status, listed) = get_auth(
        &mut app,
        "/api/v1/papers?stage=senior&subject=数学",
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let items = listed["items"].as_array().expect("试卷列表");
    assert!(
        items.iter().all(|p| p["id"] != paper_id),
        "尚未录入题目的空草稿卷不应出现在试卷导航: {listed}"
    );
}

#[tokio::test]
async fn test_ai_save_question_links_paper_created_after_staging() {
    let Some((mut app, pool)) = create_test_app_with_pool().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let token = register_and_login(&mut app).await;
    let (_, body) = upload_multipart(&mut app, &token, "pages").await;
    let doc_id = body["data"]["id"].as_str().unwrap().to_string();
    let creator_id: Uuid = sqlx::query_scalar("SELECT creator_id FROM documents WHERE id = $1")
        .bind(Uuid::parse_str(&doc_id).unwrap())
        .fetch_one(&pool)
        .await
        .expect("查询资料创建者");

    // 模拟 OCR 先行：暂存题 paper_id 为空，之后用户才确认建卷
    let task_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO ai_parse_tasks (id, creator_id, raw_text, status, document_id, progress, created_at, updated_at)
        VALUES ($1, $2, '', 'success', $3, $4, NOW(), NOW())
        "#,
    )
    .bind(task_id)
    .bind(creator_id)
    .bind(Uuid::parse_str(&doc_id).unwrap())
    .bind(json!({
        "staged_questions": [{
            "index": "p1_i0",
            "parsed": {
                "question_type": "solution",
                "difficulty": "medium",
                "stem": "已知集合 A",
                "correct_answer": {"kind": "solution", "value": {"subs": []}},
                "analysis": [],
                "knowledge_points": [],
                "confidence": 0.9
            },
            "paper_id": null,
            "saved": false
        }]
    }))
    .execute(&pool)
    .await
    .expect("写入暂存任务");

    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/ai/documents/{doc_id}/confirm"),
        json!({
            "source_category": "paper",
            "source_kind": "midterm",
            "create_paper": true,
            "paper_meta": {
                "title": "杭州学军中学2025学年第一学期期中考试高一数学试卷",
                "year": 2025,
                "stage": "senior",
                "grade": "高一",
                "subject": "数学"
            }
        }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let paper_id = body["paper_id"].as_str().expect("应创建试卷").to_string();

    // 前端此时可能仍传空 paper_ids（来源条尚未带回 paper_id）
    let (status, qbody) = post_auth(
        &mut app,
        "/api/v1/questions",
        json!({
            "stem": "已知集合 A",
            "question_type": "solution",
            "difficulty": 3,
            "paper_ids": [],
            "ai_meta": { "task_id": task_id, "staged_index": "p1_i0" }
        }),
        &token,
    )
    .await;
    assert!(status.is_success(), "保存识别题失败: {status} {qbody}");

    let (status, paper) = get_auth(&mut app, &format!("/api/v1/papers/{paper_id}"), &token).await;
    assert_eq!(status, StatusCode::OK, "{paper}");
    let questions = paper["questions"].as_array().expect("试卷题目列表");
    assert_eq!(questions.len(), 1, "保存后应关联到试卷: {paper}");
}
