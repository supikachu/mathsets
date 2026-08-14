// V2.1.1 P0-D：统一来源视图 + 数据质量概览 集成测试

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use mathset::build_app;
use mathset::db;
use mathset::AppState;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

async fn create_test_app() -> Option<(axum::Router, sqlx::PgPool)> {
    let _ = dotenvy::dotenv();
    let database_url = std::env::var("DATABASE_URL_TEST")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .ok()?;
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
    let username = format!("src_{}", Uuid::new_v4().to_string().split('-').next().unwrap());
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

async fn create_question(app: &mut axum::Router, token: &str, stem: &str) -> String {
    let (status, body) = post_auth(
        app,
        "/api/v1/questions",
        json!({
            "stem": stem,
            "question_type": "solution",
            "difficulty": 3,
            "correct_answer": {"kind": "solution", "value": {"subs": [{"sub_id": 1, "content": "解。"}]}},
            "analysis": "解析。"
        }),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn test_question_sources_unified_view() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL");
        return;
    };
    let (token, user_id) = register_and_login(&mut app).await;
    let user_uuid = Uuid::parse_str(&user_id).unwrap();

    // 造数据：document → paper + collection → question
    let doc_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO documents (id, creator_id, file_name, page_count, status, document_type, title)
        VALUES ($1, $2, '期中考试.pdf', 1, 'confirmed', 'exam', '2025高一数学期中考试')
        "#,
    )
    .bind(doc_id)
    .bind(user_uuid)
    .execute(&pool)
    .await
    .expect("插入文档失败");

    let paper_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO papers (id, title, subject, status, creator_id, document_id, created_at, updated_at, version)
        VALUES ($1, '2025高一数学期中考试', '数学', 'draft', $2, $3, NOW(), NOW(), 1)
        "#,
    )
    .bind(paper_id)
    .bind(user_uuid)
    .bind(doc_id)
    .execute(&pool)
    .await
    .expect("插入试卷失败");

    let collection_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO question_collections (id, document_id, creator_id, title, collection_type)
        VALUES ($1, $2, $3, '课堂练习', 'class_exercise')
        "#,
    )
    .bind(collection_id)
    .bind(doc_id)
    .bind(user_uuid)
    .execute(&pool)
    .await
    .expect("插入集合失败");

    let q1 = create_question(&mut app, &token, "第 17 题题干").await;
    let q1_uuid = Uuid::parse_str(&q1).unwrap();

    // 关联：试卷第 17 题 + 集合练习 3
    sqlx::query(
        r#"
        INSERT INTO paper_questions (id, paper_id, question_id, sort_order, score, question_no, display_order, created_at)
        VALUES ($1, $2, $3, 17, 8, '17(2)', 17, NOW())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(paper_id)
    .bind(q1_uuid)
    .execute(&pool)
    .await
    .expect("插入试卷关联失败");

    sqlx::query(
        r#"
        INSERT INTO collection_questions (id, collection_id, question_id, question_no, display_order, score, created_at)
        VALUES ($1, $2, $3, '练习3', 3, 10, NOW())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(collection_id)
    .bind(q1_uuid)
    .execute(&pool)
    .await
    .expect("插入集合关联失败");

    // 统一来源视图
    let (status, body) = get_auth(&mut app, &format!("/api/v1/questions/{q1}/sources"), &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let sources = body.as_array().unwrap();
    assert_eq!(sources.len(), 2, "应有试卷 + 集合两个来源: {sources:?}");

    // 试卷来源（在前）
    let paper_src = sources.iter().find(|s| s["kind"] == "paper").expect("缺少试卷来源");
    assert_eq!(paper_src["title"], "2025高一数学期中考试");
    assert_eq!(paper_src["question_no"], "17(2)");
    assert_eq!(paper_src["display_order"], 17);
    assert_eq!(paper_src["score"], 8);
    assert_eq!(paper_src["document_id"], doc_id.to_string());
    assert_eq!(paper_src["document_title"], "2025高一数学期中考试");
    assert_eq!(paper_src["document_type"], "exam");

    // 集合来源
    let col_src = sources.iter().find(|s| s["kind"] == "collection").expect("缺少集合来源");
    assert_eq!(col_src["title"], "课堂练习");
    assert_eq!(col_src["type_label"], "class_exercise");
    assert_eq!(col_src["question_no"], "练习3");
    assert_eq!(col_src["document_type"], "exam");

    // 无来源题目 → 空数组
    let q2 = create_question(&mut app, &token, "无来源题目").await;
    let (status, body) = get_auth(&mut app, &format!("/api/v1/questions/{q2}/sources"), &token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_data_quality_summary_admin_only() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL");
        return;
    };
    let (token, user_id) = register_and_login(&mut app).await;

    // 普通用户 → 403
    let (status, _) = get_auth(&mut app, "/api/v1/admin/data-quality/summary", &token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 提升为管理员后 → 200 + 字段齐全
    sqlx::query("UPDATE users SET role = 'admin', global_role = 'super_admin' WHERE id = $1")
        .bind(Uuid::parse_str(&user_id).unwrap())
        .execute(&pool)
        .await
        .expect("提升管理员失败");

    // 重新登录获取新 token
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(Uuid::parse_str(&user_id).unwrap())
        .fetch_one(&pool)
        .await
        .expect("查询用户名失败");
    let (_, login) = request(
        &mut app,
        Method::POST,
        "/api/v1/auth/login",
        Some(json!({ "username": username, "password": "test123" })),
    )
    .await;
    let admin_token = login["token"].as_str().unwrap().to_string();

    // 建一道无来源题目
    let q = create_question(&mut app, &admin_token, "数据质量测试题").await;
    let _ = q;

    let (status, body) = get_auth(&mut app, "/api/v1/admin/data-quality/summary", &admin_token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for key in [
        "orphan_paper_questions",
        "orphan_collection_questions",
        "papers_without_questions",
        "collections_without_questions",
        "documents_without_sources",
        "duplicate_paper_question_no_groups",
        "duplicate_collection_question_no_groups",
        "questions_without_sources",
    ] {
        assert!(body[key].is_number(), "缺少字段 {key}: {body}");
    }
    assert!(body["questions_without_sources"].as_i64().unwrap() >= 1, "应检测到无来源题目: {body}");
}
