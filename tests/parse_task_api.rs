// V2.1.1 P0-C：AI 解析任务 API 集成测试
//
// 覆盖：提交前置校验（404/400/409）、任务快照、取消、终态取消拒绝。
// Worker 核心逻辑（persist_question/租约恢复）在 src/workers/ai_parse_worker.rs
// 的 cfg(test) 模块直测（无 LLM 依赖）；LLM 全链路走人工验收。

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
// 辅助函数
// ---------------------------------------------------------------------------

async fn create_test_app() -> Option<(axum::Router, sqlx::PgPool)> {
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

async fn register_and_login(app: &mut axum::Router) -> (String, String) {
    let username = format!("pt_{}", Uuid::new_v4().to_string().split('-').next().unwrap());
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
    (
        body["token"].as_str().unwrap().to_string(),
        body["user_id"].as_str().unwrap().to_string(),
    )
}

/// 20x20 合法 PNG（满足视觉模型最小尺寸限制）
const PNG_1PX_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAABQAAAAUCAIAAAAC64paAAAAEklEQVR4nGNgGAWjYBSMgqELAATEAAE0eCSYAAAAAElFTkSuQmCC";

/// 上传单页图并返回 document_id
async fn upload_document(app: &mut axum::Router, token: &str) -> String {
    let png = base64::engine::general_purpose::STANDARD
        .decode(PNG_1PX_B64)
        .unwrap();
    let boundary = "----ptboundary";
    let mut body = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"pages\"; filename=\"page1.png\"\r\nContent-Type: image/png\r\n\r\n",
        b = boundary
    )
    .into_bytes();
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
    assert_eq!(response.status(), StatusCode::CREATED);
    let body_bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    json["data"]["id"].as_str().unwrap().to_string()
}

async fn confirm_document(app: &mut axum::Router, token: &str, doc_id: &str, body: Value) {
    let (status, body) = post_auth(
        app,
        &format!("/api/v1/ai/documents/{doc_id}/confirm"),
        body,
        token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_parse_task_lifecycle_and_409() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token, _) = register_and_login(&mut app).await;
    let doc_id = upload_document(&mut app, &token).await;
    confirm_document(
        &mut app,
        &token,
        &doc_id,
        json!({ "document_type": "class_exercise", "title": "二次函数课堂练习" }),
    )
    .await;

    // 创建任务 → 202
    let (status, body) = post_auth(
        &mut app,
        "/api/v1/ai/parse-task",
        json!({ "document_id": doc_id }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let task_id = body["task_id"].as_str().unwrap().to_string();

    // 快照落库：paper_meta 含 document_type 与集合信息
    let snapshot: serde_json::Value =
        sqlx::query_scalar("SELECT paper_meta FROM ai_parse_tasks WHERE id = $1")
            .bind(Uuid::parse_str(&task_id).unwrap())
            .fetch_one(&pool)
            .await
            .expect("查询快照失败");
    assert_eq!(snapshot["document_type"], "class_exercise");
    assert_eq!(snapshot["collections"][0]["title"], "二次函数课堂练习");

    // 同文档再次提交 → 409 + existing_task_id
    let (status, body) = post_auth(
        &mut app,
        "/api/v1/ai/parse-task",
        json!({ "document_id": doc_id }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "ERR_TASK_ACTIVE");
    assert_eq!(body["existing_task_id"], task_id);

    // GET 进度 → pending + total_pages
    let (status, body) = get_auth(&mut app, &format!("/api/v1/ai/parse-task/{task_id}"), &token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "pending");
    assert_eq!(body["total_pages"], 1);

    // 取消 → 200（worker 未运行，任务保持 pending + cancel_requested_at）
    let (status, _) = post_auth(
        &mut app,
        &format!("/api/v1/ai/parse-task/{task_id}/cancel"),
        json!({}),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let cancel_at: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT cancel_requested_at FROM ai_parse_tasks WHERE id = $1",
    )
    .bind(Uuid::parse_str(&task_id).unwrap())
    .fetch_one(&pool)
    .await
    .expect("查询取消标记失败");
    assert!(cancel_at.is_some(), "cancel_requested_at 应已写入");

    // 终态任务 → 取消被拒 409
    sqlx::query("UPDATE ai_parse_tasks SET status = 'success', completed_at = NOW() WHERE id = $1")
        .bind(Uuid::parse_str(&task_id).unwrap())
        .execute(&pool)
        .await
        .expect("更新任务状态失败");
    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/ai/parse-task/{task_id}/cancel"),
        json!({}),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    // completed → success 视图映射
    let (status, body) = get_auth(&mut app, &format!("/api/v1/ai/parse-task/{task_id}"), &token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "success");
}

#[tokio::test]
async fn test_parse_task_requires_confirmed_document() {
    let Some((mut app, _)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token, _) = register_and_login(&mut app).await;

    // 不存在的文档 → 404
    let (status, _) = post_auth(
        &mut app,
        "/api/v1/ai/parse-task",
        json!({ "document_id": "00000000-0000-0000-0000-000000000000" }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 未确认的文档（仅上传）→ 400
    let doc_id = upload_document(&mut app, &token).await;
    let (status, body) = post_auth(
        &mut app,
        "/api/v1/ai/parse-task",
        json!({ "document_id": doc_id }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "ERR_DOCUMENT_NOT_CONFIRMED");
}

#[tokio::test]
async fn test_parse_task_paper_snapshot_and_quota() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token, user_id) = register_and_login(&mut app).await;
    let doc_id = upload_document(&mut app, &token).await;
    confirm_document(
        &mut app,
        &token,
        &doc_id,
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
                "school_name": "示例中学"
            }
        }),
    )
    .await;

    let (status, body) = post_auth(
        &mut app,
        "/api/v1/ai/parse-task",
        json!({ "document_id": doc_id }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let task_id = body["task_id"].as_str().unwrap().to_string();

    // paper_meta 快照包含试卷元数据
    let snapshot: serde_json::Value =
        sqlx::query_scalar("SELECT paper_meta FROM ai_parse_tasks WHERE id = $1")
            .bind(Uuid::parse_str(&task_id).unwrap())
            .fetch_one(&pool)
            .await
            .expect("查询快照失败");
    assert_eq!(snapshot["document_type"], "exam");
    assert_eq!(snapshot["paper_meta"]["title"], "2025高一数学期中考试");
    assert_eq!(snapshot["paper_meta"]["year"], 2025);

    // 配额日志落库
    let quota: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ai_usage_log WHERE user_id = $1 AND endpoint = 'parse_task'",
    )
    .bind(Uuid::parse_str(&user_id).unwrap())
    .fetch_one(&pool)
    .await
    .expect("查询配额失败");
    assert!(quota >= 1, "应写入 parse_task 配额日志");
}

#[tokio::test]
async fn test_parse_task_permission_isolation() {
    let Some((mut app, _)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token_a, _) = register_and_login(&mut app).await;
    let (token_b, _) = register_and_login(&mut app).await;

    let doc_id = upload_document(&mut app, &token_a).await;
    confirm_document(
        &mut app,
        &token_a,
        &doc_id,
        json!({ "document_type": "homework", "title": "A 的作业" }),
    )
    .await;
    let (status, body) = post_auth(
        &mut app,
        "/api/v1/ai/parse-task",
        json!({ "document_id": doc_id }),
        &token_a,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let task_id = body["task_id"].as_str().unwrap().to_string();

    // B 不可见/不可取消 A 的任务（统一 404）
    let (status, _) = get_auth(&mut app, &format!("/api/v1/ai/parse-task/{task_id}"), &token_b).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = post_auth(
        &mut app,
        &format!("/api/v1/ai/parse-task/{task_id}/cancel"),
        json!({}),
        &token_b,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
