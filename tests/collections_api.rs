// V2.1.1 P0-B：Paper 元数据 / Question hash / QuestionCollection 集成测试
//
// 集合行通过直连 DB 插入（集合由 Worker 阶段 2 创建，P0-C 实现），
// 本文件只测集合的 HTTP 管理面（批量分组/详情/移除/权限）。

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

async fn delete_auth(app: &mut axum::Router, uri: &str, token: &str) -> (StatusCode, Value) {
    request_auth(app, Method::DELETE, uri, None, token).await
}

async fn register_and_login(app: &mut axum::Router) -> (String, String) {
    let username = format!("pb_{}", Uuid::new_v4().to_string().split('-').next().unwrap());
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

/// 创建题目并返回 question_id
async fn create_question(
    app: &mut axum::Router,
    token: &str,
    stem: &str,
    analysis: &str,
) -> String {
    let (status, body) = post_auth(
        app,
        "/api/v1/questions",
        json!({
            "stem": stem,
            "question_type": "solution",
            "difficulty": 3,
            "correct_answer": null,
            "analysis": null,
            "structure": mathset::testing::solution_structure_json("解。", analysis)
        }),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "创建题目失败: {body}");
    body["id"].as_str().unwrap().to_string()
}

/// 直连 DB 查询题目 hash
async fn question_hashes(pool: &sqlx::PgPool, question_id: &str) -> (Option<String>, Option<String>) {
    let row: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT content_hash, normalized_content_hash FROM questions WHERE id = $1",
    )
    .bind(Uuid::parse_str(question_id).unwrap())
    .fetch_one(pool)
    .await
    .expect("查询题目 hash 失败");
    row
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_question_create_writes_dedup_hashes() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token, _) = register_and_login(&mut app).await;

    let q1 = create_question(&mut app, &token, "已知 $f(x)=x^2$，求极值。", "求导。").await;
    let (c1, n1) = question_hashes(&pool, &q1).await;
    assert!(c1.as_ref().is_some_and(|h| h.len() == 64), "content_hash 未写入");
    assert!(n1.as_ref().is_some_and(|h| h.len() == 64), "normalized_content_hash 未写入");

    // 同题干同解析 → hash 完全一致
    let q2 = create_question(&mut app, &token, "已知 $f(x)=x^2$，求极值。", "求导。").await;
    let (c2, n2) = question_hashes(&pool, &q2).await;
    assert_eq!(c1, c2);
    assert_eq!(n1, n2);

    // 同题干不同解析 → content_hash 不同、normalized_content_hash 相同（跨资料去重核心）
    let q3 = create_question(&mut app, &token, "已知 $f(x)=x^2$，求极值。", "另一种解法。").await;
    let (c3, n3) = question_hashes(&pool, &q3).await;
    assert_ne!(c1, c3);
    assert_eq!(n1, n3);

    // 排版差异（全角/空白/行尾标点）→ normalized 相同
    let q4 = create_question(&mut app, &token, "已知　$f(x)=x^2$，求极值。", "求导。").await;
    let (_, n4) = question_hashes(&pool, &q4).await;
    assert_eq!(n1, n4, "规范化 hash 应无视排版差异");
}

#[tokio::test]
async fn test_collections_batch_add_detail_remove() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token, user_id) = register_and_login(&mut app).await;
    let user_uuid = Uuid::parse_str(&user_id).unwrap();

    // 造一个集合（Worker 阶段 2 才创建，这里直连 DB 模拟：先建 documents 行满足 FK）
    let document_id = Uuid::new_v4();
    let collection_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO documents (id, creator_id, file_name, page_count, status, document_type, title)
        VALUES ($1, $2, '课堂练习.pdf', 1, 'confirmed', 'class_exercise', '课堂练习')
        "#,
    )
    .bind(document_id)
    .bind(user_uuid)
    .execute(&pool)
    .await
    .expect("插入 documents 失败");
    sqlx::query(
        r#"
        INSERT INTO question_collections (id, document_id, creator_id, title, collection_type)
        VALUES ($1, $2, $3, '课堂练习', 'class_exercise')
        "#,
    )
    .bind(collection_id)
    .bind(document_id)
    .bind(user_uuid)
    .execute(&pool)
    .await
    .expect("插入集合失败");

    let q1 = create_question(&mut app, &token, "第一题题干", "解析1").await;
    let q2 = create_question(&mut app, &token, "第二题题干", "解析2").await;

    // 批量添加
    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/collections/{collection_id}/questions/batch"),
        json!({
            "questions": [
                { "question_id": q1, "question_no": "1", "display_order": 1, "score": 10 },
                { "question_id": q2, "question_no": "2", "display_order": 2, "score": 12 }
            ]
        }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["inserted"], 2);

    // 重复添加 → skipped
    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/collections/{collection_id}/questions/batch"),
        json!({ "questions": [{ "question_id": q1 }] }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["inserted"], 0);
    assert_eq!(body["skipped"], 1);

    // 详情：含来源链路与题目列表（题号/顺序正确）
    let (status, body) = get_auth(
        &mut app,
        &format!("/api/v1/collections/{collection_id}"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["title"], "课堂练习");
    assert_eq!(body["collection_type"], "class_exercise");
    let questions = body["questions"].as_array().unwrap();
    assert_eq!(questions.len(), 2);
    assert_eq!(questions[0]["question_no"], "1");
    assert_eq!(questions[0]["display_order"], 1);

    // 列表
    let (status, body) = get_auth(&mut app, "/api/v1/collections", &token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["id"] == json!(collection_id.to_string())));

    // 移除一题
    let (status, _) = delete_auth(
        &mut app,
        &format!("/api/v1/collections/{collection_id}/questions/{q1}"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = get_auth(
        &mut app,
        &format!("/api/v1/collections/{collection_id}"),
        &token,
    )
    .await;
    assert_eq!(body["questions"].as_array().unwrap().len(), 1);

    // 不存在的题目 → 404
    let (status, _) = post_auth(
        &mut app,
        &format!("/api/v1/collections/{collection_id}/questions/batch"),
        json!({ "questions": [{ "question_id": "00000000-0000-0000-0000-000000000000" }] }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_collections_permission_isolation() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token_a, user_a) = register_and_login(&mut app).await;
    let (token_b, _) = register_and_login(&mut app).await;
    let user_a_uuid = Uuid::parse_str(&user_a).unwrap();

    let document_id = Uuid::new_v4();
    let collection_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO documents (id, creator_id, file_name, page_count, status, document_type, title)
        VALUES ($1, $2, 'A资料.pdf', 1, 'confirmed', 'homework', 'A 的作业')
        "#,
    )
    .bind(document_id)
    .bind(user_a_uuid)
    .execute(&pool)
    .await
    .expect("插入 documents 失败");
    sqlx::query(
        r#"
        INSERT INTO question_collections (id, document_id, creator_id, title, collection_type)
        VALUES ($1, $2, $3, 'A 的集合', 'homework')
        "#,
    )
    .bind(collection_id)
    .bind(document_id)
    .bind(user_a_uuid)
    .execute(&pool)
    .await
    .expect("插入集合失败");

    // B 看不到 A 的集合（统一 404）
    let (status, _) = get_auth(
        &mut app,
        &format!("/api/v1/collections/{collection_id}"),
        &token_b,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = delete_auth(
        &mut app,
        &format!("/api/v1/collections/{collection_id}/questions/{}/", Uuid::new_v4()),
        &token_b,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A 自己可访问
    let (status, _) = get_auth(
        &mut app,
        &format!("/api/v1/collections/{collection_id}"),
        &token_a,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_paper_metadata_and_question_no_roundtrip() {
    let Some((mut app, _)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token, _) = register_and_login(&mut app).await;

    // 创建试卷（V2.1.1 元数据）
    let (status, body) = post_auth(
        &mut app,
        "/api/v1/papers",
        json!({
            "title": "2025高一数学期中考试",
            "subject": "数学",
            "grade": "高一",
            "year": 2025,
            "stage": "senior",
            "semester": "first",
            "region_province": "浙江省",
            "region_city": "杭州市",
            "school_name": "示例中学",
            "source_type": "exam",
            "sub_source_type": "期中"
        }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let paper_id = body["id"].as_str().unwrap().to_string();

    // 详情返回元数据
    let (status, body) = get_auth(&mut app, &format!("/api/v1/papers/{paper_id}"), &token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["year"], 2025);
    assert_eq!(body["stage"], "senior");
    assert_eq!(body["region_city"], "杭州市");
    assert_eq!(body["school_name"], "示例中学");

    // 添加题目（含题号）
    let q1 = create_question(&mut app, &token, "第 17 题题干", "解析").await;
    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/papers/{paper_id}/questions"),
        json!({
            "question_id": q1,
            "score": 8,
            "question_no": "17(2)",
            "display_order": 17
        }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // 详情题目项含题号
    let (_, body) = get_auth(&mut app, &format!("/api/v1/papers/{paper_id}"), &token).await;
    let qs = body["questions"].as_array().unwrap();
    assert_eq!(qs.len(), 1);
    assert_eq!(qs[0]["question_no"], "17(2)");
    assert_eq!(qs[0]["display_order"], 17);

    // 反向查询 sources 侧（/questions/{id}/papers）含题号
    let (status, body) = get_auth(&mut app, &format!("/api/v1/questions/{q1}/papers"), &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let papers = body.as_array().unwrap();
    assert_eq!(papers[0]["question_no"], "17(2)");
    assert_eq!(papers[0]["display_order"], 17);

    // 列表返回元数据
    let (status, body) = get_auth(&mut app, "/api/v1/papers?page=1&page_size=10", &token).await;
    assert_eq!(status, StatusCode::OK);
    let item = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == json!(paper_id))
        .expect("试卷应在列表中");
    assert_eq!(item["year"], 2025);
}
