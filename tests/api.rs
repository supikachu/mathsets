use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mathset::auth::jwt::verify_token;
use mathset::build_app;
use mathset::db;
use mathset::AppState;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// 辅助函数：构建测试 App（需要真实的 PostgreSQL 连接）
// ---------------------------------------------------------------------------

/// 从 DATABASE_URL 环境变量创建测试 App，如果未设置则跳过
async fn create_test_app() -> Option<axum::Router> {
    let database_url = std::env::var("DATABASE_URL").ok()?;

    let pool = db::create_pool(&database_url).await;
    // 运行迁移，确保表结构存在
    db::run_migrations(&pool).await;

    let state = AppState {
        pool,
        jwt_secret: "test-secret-for-integration-tests".to_string(),
        jwt_expiry_hours: 24,
    };

    Some(build_app(state))
}

async fn post_json(
    app: &mut axum::Router,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body_bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap_or(json!({"error": "parse failed"}));
    (status, json)
}

// ---------------------------------------------------------------------------
// 1. 健康检查
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_health_check_direct() {
    let (status, json) = mathset::handlers::health::health_check().await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json.status, "ok");
    assert_eq!(json.version, "0.1.0");
}

// ---------------------------------------------------------------------------
// 2. 用户注册 + 登录 集成测试（需要真实的 PostgreSQL）
// ---------------------------------------------------------------------------

/// 注册 → 登录 → 验证 token 可解析
#[tokio::test]
async fn test_auth_register_and_login() {
    let mut app = match create_test_app().await {
        Some(app) => app,
        None => {
            eprintln!("⚠️  跳过 auth 测试: DATABASE_URL 未设置");
            return;
        }
    };

    let username = format!("testuser_{}", Uuid::new_v4().to_string().split('-').next().unwrap());

    // 注册
    let (status, body) = post_json(
        &mut app,
        "/api/v1/auth/register",
        json!({
            "username": username,
            "email": format!("{}@test.com", username),
            "password": "password123",
            "display_name": "测试用户"
        }),
    ).await;
    assert_eq!(status, StatusCode::CREATED, "注册失败: {:?}", body);
    assert_eq!(body["username"], username);
    assert_eq!(body["role"], "Teacher");

    // 用相同用户名注册应冲突
    let (status, _) = post_json(
        &mut app,
        "/api/v1/auth/register",
        json!({
            "username": username,
            "email": format!("{}@test.com", username),
            "password": "password123",
            "display_name": "重复用户"
        }),
    ).await;
    assert_eq!(status, StatusCode::CONFLICT, "重复注册应该返回 409");

    // 登录 — 正确密码
    let (status, body) = post_json(
        &mut app,
        "/api/v1/auth/login",
        json!({
            "username": username,
            "password": "password123"
        }),
    ).await;
    assert_eq!(status, StatusCode::OK, "登录失败: {:?}", body);
    assert!(body["token"].as_str().unwrap().len() > 20);
    assert_eq!(body["display_name"], "测试用户");

    // 验证 token 可被服务端解析
    let token = body["token"].as_str().unwrap();
    let claims = verify_token(token, "test-secret-for-integration-tests")
        .expect("服务端签发的 token 应能被验证");
    assert_eq!(claims.username, username);

    // 登录 — 错误密码
    let (status, _) = post_json(
        &mut app,
        "/api/v1/auth/login",
        json!({
            "username": username,
            "password": "wrong-password"
        }),
    ).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "错误密码应返回 401");
}

/// 注册时表单不完整应导致服务器错误（目前是 500，后续可改为 422）
#[tokio::test]
async fn test_auth_register_missing_fields() {
    let mut app = match create_test_app().await {
        Some(app) => app,
        None => {
            eprintln!("⚠️  跳过 auth 测试: DATABASE_URL 未设置");
            return;
        }
    };

    // 缺少 password
    let (status, _) = post_json(
        &mut app,
        "/api/v1/auth/register",
        json!({
            "username": "nopass_user",
            "email": "nopass@test.com",
            "display_name": "无密码"
        }),
    ).await;
    // 缺少必填字段 → axum 自动返回 422 Unprocessable Entity
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// 登录不存在的用户
#[tokio::test]
async fn test_auth_login_nonexistent_user() {
    let mut app = match create_test_app().await {
        Some(app) => app,
        None => {
            eprintln!("⚠️  跳过 auth 测试: DATABASE_URL 未设置");
            return;
        }
    };

    let (status, _) = post_json(
        &mut app,
        "/api/v1/auth/login",
        json!({
            "username": "nonexistent_user_xyz",
            "password": "password123"
        }),
    ).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "不存在的用户应返回 401");
}
