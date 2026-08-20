// 异步打标任务：创建 / 幂等复用 / 取消（不启动 Worker，任务保持 pending）

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

async fn register_and_login(app: &mut axum::Router) -> (String, String) {
    let username = format!("tgtsk_{}", Uuid::new_v4().simple());
    let email = format!("{username}@test.com");
    let _ = request(
        app,
        Method::POST,
        "/api/v1/auth/register",
        Some(json!({
            "username": username,
            "email": email,
            "password": "test123",
            "display_name": "打标任务用户"
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

#[tokio::test]
async fn test_tagging_task_create_reuse_get_cancel() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token, user_id) = register_and_login(&mut app).await;
    let content = format!("已知函数 f(x)=x^2，求 f(1)。{}", Uuid::new_v4());

    let (status, body) = request_auth(
        &mut app,
        Method::POST,
        "/api/v1/questions/ai-tagging-tasks",
        Some(json!({ "content": content })),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["reused"], false);
    assert_eq!(body["status"], "pending");

    let used: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ai_usage_log WHERE user_id = $1 AND endpoint = 'tagging_task' AND created_at >= CURRENT_DATE",
    )
    .bind(Uuid::parse_str(&user_id).unwrap())
    .fetch_one(&pool)
    .await
    .expect("查询配额失败");
    assert!(used >= 1);

    let (status, body2) = request_auth(
        &mut app,
        Method::POST,
        "/api/v1/questions/ai-tagging-tasks",
        Some(json!({ "content": content })),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body2}");
    assert_eq!(body2["id"], json!(id));
    assert_eq!(body2["reused"], true);

    let used_after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ai_usage_log WHERE user_id = $1 AND endpoint = 'tagging_task' AND created_at >= CURRENT_DATE",
    )
    .bind(Uuid::parse_str(&user_id).unwrap())
    .fetch_one(&pool)
    .await
    .expect("查询配额失败");
    assert_eq!(used_after, used, "复用进行中任务不应再扣配额");

    let (status, get_body) = request_auth(
        &mut app,
        Method::GET,
        &format!("/api/v1/questions/ai-tagging-tasks/{id}"),
        None,
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{get_body}");
    assert_eq!(get_body["status"], "pending");
    assert!(get_body.get("content").is_none());

    let (token2, _) = register_and_login(&mut app).await;
    let (status, _) = request_auth(
        &mut app,
        Method::GET,
        &format!("/api/v1/questions/ai-tagging-tasks/{id}"),
        None,
        &token2,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, cancel_body) = request_auth(
        &mut app,
        Method::POST,
        &format!("/api/v1/questions/ai-tagging-tasks/{id}/cancel"),
        Some(json!({})),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cancel_body}");
    assert_eq!(cancel_body["status"], "cancelled");

    let (status, _) = request_auth(
        &mut app,
        Method::POST,
        &format!("/api/v1/questions/ai-tagging-tasks/{id}/cancel"),
        Some(json!({})),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_tagging_task_empty_content() {
    let Some((mut app, _pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token, _) = register_and_login(&mut app).await;
    let (status, _) = request_auth(
        &mut app,
        Method::POST,
        "/api/v1/questions/ai-tagging-tasks",
        Some(json!({ "content": "   " })),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

fn unique_name(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

async fn insert_tree(pool: &sqlx::PgPool, kind: &str) -> Uuid {
    let id = Uuid::new_v4();
    let code = unique_name("tgapi_tree");
    sqlx::query(
        "INSERT INTO knowledge_trees (id, code, name, kind, is_active, created_at, updated_at) VALUES ($1, $2, $3, $4::knowledge_tree_kind, TRUE, NOW(), NOW())",
    )
    .bind(id)
    .bind(&code)
    .bind(&code)
    .bind(kind)
    .execute(pool)
    .await
    .expect("insert tree");
    id
}

async fn insert_node(pool: &sqlx::PgPool, tree_id: Uuid, name: &str) -> Uuid {
    let id = Uuid::new_v4();
    let path = format!("n{}", Uuid::new_v4().simple());
    sqlx::query(
        r#"
        INSERT INTO knowledge_nodes (id, tree_id, path, depth, name, aliases, is_active, status, source, created_at, updated_at)
        VALUES ($1, $2, $3::ltree, 1, $4, '[]'::jsonb, TRUE, 'active', 'system', NOW(), NOW())
        "#,
    )
    .bind(id)
    .bind(tree_id)
    .bind(&path)
    .bind(name)
    .execute(pool)
    .await
    .expect("insert node");
    id
}

fn solution_payload(stem: &str) -> Value {
    json!({
        "stem": stem,
        "question_type": "solution",
        "difficulty": 3,
        "correct_answer": {"kind": "solution", "value": {"subs": [{"sub_id": 1, "content": "解。"}]}},
        "analysis": "解析。"
    })
}

#[tokio::test]
async fn test_finalizer_apply_source_idempotent_and_candidates() {
    use mathset::ai::tagging::{
        run_tagging, TaggingContext, TaggingDimension, TaggingInput, TaggingPolicy,
    };
    use mathset::ai::types::{ParsedQuestion, SolutionMethod};

    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token, user_id) = register_and_login(&mut app).await;
    let uid = Uuid::parse_str(&user_id).unwrap();

    let tree = insert_tree(&pool, "knowledge").await;
    let keep_name = unique_name("保留知识点");
    let drop_name = unique_name("删除知识点");
    let keep_id = insert_node(&pool, tree, &keep_name).await;
    let drop_id = insert_node(&pool, tree, &drop_name).await;
    let unmatched_raw = unique_name("未匹配候选词");

    let q = ParsedQuestion {
        question_type: "solution".into(),
        sub_type: None,
        difficulty: Some("medium".into()),
        stem: unique_name("finalizer题干"),
        options: None,
        correct_answer: None,
        analysis: vec![],
        knowledge_points: vec![keep_name.clone(), drop_name.clone(), unmatched_raw.clone()],
        confidence: 0.9,
        warnings: vec![],
        image_placeholders: vec![],
        image_urls: vec![],
        kp_matches: vec![],
        question_no: None,
        display_order: None,
        score: None,
        chapter_path: vec![],
        solution_methods: vec![SolutionMethod {
            name: String::new(),
            confidence: None,
        }],
    };
    let mut policy = TaggingPolicy::default();
    policy.run_llm_extract = false;
    policy.run_llm_converge = false;
    policy.fail_on_persist = true;
    let suggestion = run_tagging(
        &pool,
        None,
        None,
        TaggingInput::Parsed(Box::new(q)),
        &TaggingContext {
            user_id: uid,
            ..TaggingContext::default()
        },
        &policy,
    )
    .await
    .expect("run_tagging persist");
    let sid = suggestion.suggestion_id.expect("应写入 suggestion");
    let unmatched_id = suggestion
        .unmatched
        .iter()
        .find(|u| u.dimension == TaggingDimension::Knowledge && u.raw_name == unmatched_raw)
        .map(|u| u.id.clone())
        .expect("应有未匹配项");
    assert!(suggestion
        .matches
        .iter()
        .any(|m| m.target_id == keep_id && m.dimension == TaggingDimension::Knowledge));
    assert!(suggestion
        .matches
        .iter()
        .any(|m| m.target_id == drop_id && m.dimension == TaggingDimension::Knowledge));

    let extra_manual = insert_node(&pool, tree, &unique_name("手工节点")).await;

    let mut body = solution_payload(&unique_name("确认保存题"));
    body["knowledge_node_ids"] = json!([keep_id, extra_manual]);
    body["metadata"] = json!({
        "grade": "高三",
        "cognitive_level": "apply"
    });
    body["ai_tagging_confirmation"] = json!({
        "suggestion_id": sid,
        "unmatched_ids": [unmatched_id]
    });

    let (status, created) = request_auth(
        &mut app,
        Method::POST,
        "/api/v1/questions",
        Some(body),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let qid = created["id"].as_str().unwrap().to_string();
    let q_uuid = Uuid::parse_str(&qid).unwrap();

    assert_eq!(created["metadata"]["grade"], "高三");
    assert_eq!(created["metadata"]["cognitive_level"], "apply");

    let links: Vec<(Uuid, String, Option<Uuid>)> = sqlx::query_as(
        "SELECT node_id, source::text, suggestion_id FROM question_knowledge_nodes WHERE question_id = $1",
    )
    .bind(q_uuid)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(
        links.iter().all(|l| l.0 != drop_id),
        "用户未保留的 AI 节点不应被 Finalizer 恢复: {links:?}"
    );
    let keep_link = links.iter().find(|l| l.0 == keep_id).expect("保留节点应关联");
    assert_eq!(keep_link.1, "ai");
    assert_eq!(keep_link.2, Some(sid));
    let manual_link = links
        .iter()
        .find(|l| l.0 == extra_manual)
        .expect("手工节点应关联");
    assert_eq!(manual_link.1, "manual");

    let cand: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tag_candidates WHERE source_question_id = $1 AND raw_name = $2",
    )
    .bind(q_uuid)
    .bind(&unmatched_raw)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cand, 1, "确认保存后应写入候选");

    let st: String = sqlx::query_scalar("SELECT status FROM ai_tagging_suggestions WHERE id = $1")
        .bind(sid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(st, "applied");

    let mut again = solution_payload(&unique_name("幂等再保存"));
    again["knowledge_node_ids"] = json!([keep_id, extra_manual]);
    again["ai_tagging_confirmation"] = json!({
        "suggestion_id": sid,
        "unmatched_ids": [unmatched_id]
    });
    let (status, updated) = request_auth(
        &mut app,
        Method::PUT,
        &format!("/api/v1/questions/{qid}"),
        Some(again),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    let cand2: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tag_candidates WHERE source_question_id = $1 AND raw_name = $2",
    )
    .bind(q_uuid)
    .bind(&unmatched_raw)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cand2, 1, "重复应用不应再写候选");
}

#[tokio::test]
async fn test_unconfirmed_does_not_write_candidates_and_foreign_suggestion_forbidden() {
    use mathset::ai::tagging::{
        run_tagging, TaggingContext, TaggingInput, TaggingPolicy,
    };
    use mathset::ai::types::ParsedQuestion;

    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token, user_id) = register_and_login(&mut app).await;
    let uid = Uuid::parse_str(&user_id).unwrap();
    let unmatched_raw = unique_name("未确认候选词");

    let q = ParsedQuestion {
        question_type: "solution".into(),
        sub_type: None,
        difficulty: Some("medium".into()),
        stem: unique_name("未确认题干"),
        options: None,
        correct_answer: None,
        analysis: vec![],
        knowledge_points: vec![unmatched_raw.clone()],
        confidence: 0.9,
        warnings: vec![],
        image_placeholders: vec![],
        image_urls: vec![],
        kp_matches: vec![],
        question_no: None,
        display_order: None,
        score: None,
        chapter_path: vec![],
        solution_methods: vec![],
    };
    let mut policy = TaggingPolicy::default();
    policy.run_llm_extract = false;
    policy.run_llm_converge = false;
    policy.fail_on_persist = true;
    let suggestion = run_tagging(
        &pool,
        None,
        None,
        TaggingInput::Parsed(Box::new(q)),
        &TaggingContext {
            user_id: uid,
            ..TaggingContext::default()
        },
        &policy,
    )
    .await
    .expect("persist suggestion");
    let sid = suggestion.suggestion_id.unwrap();
    let unmatched_id = suggestion.unmatched[0].id.clone();

    let (status, created) = request_auth(
        &mut app,
        Method::POST,
        "/api/v1/questions",
        Some(solution_payload(&unique_name("无确认保存"))),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let qid = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    let cand: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tag_candidates WHERE source_question_id = $1 AND raw_name = $2",
    )
    .bind(qid)
    .bind(&unmatched_raw)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cand, 0, "未带 confirmation 不应写候选");

    let (token2, _) = register_and_login(&mut app).await;
    let mut steal = solution_payload(&unique_name("盗用建议"));
    steal["ai_tagging_confirmation"] = json!({
        "suggestion_id": sid,
        "unmatched_ids": [unmatched_id]
    });
    let (status, body) = request_auth(
        &mut app,
        Method::POST,
        "/api/v1/questions",
        Some(steal),
        &token2,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn test_alias_maps_write_suggested_node_id() {
    use mathset::ai::tagging::{
        run_tagging, TaggingContext, TaggingInput, TaggingPolicy,
    };
    use mathset::ai::types::ParsedQuestion;

    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token, user_id) = register_and_login(&mut app).await;
    let uid = Uuid::parse_str(&user_id).unwrap();
    let tree = insert_tree(&pool, "knowledge").await;
    let target_id = insert_node(&pool, tree, &unique_name("别名目标叶子")).await;
    let unmatched_raw = unique_name("集合的交集运算变体");

    let q = ParsedQuestion {
        question_type: "solution".into(),
        sub_type: None,
        difficulty: Some("medium".into()),
        stem: unique_name("别名映射题干"),
        options: None,
        correct_answer: None,
        analysis: vec![],
        knowledge_points: vec![unmatched_raw.clone()],
        confidence: 0.9,
        warnings: vec![],
        image_placeholders: vec![],
        image_urls: vec![],
        kp_matches: vec![],
        question_no: None,
        display_order: None,
        score: None,
        chapter_path: vec![],
        solution_methods: vec![],
    };
    let mut policy = TaggingPolicy::default();
    policy.run_llm_extract = false;
    policy.run_llm_converge = false;
    policy.fail_on_persist = true;
    let suggestion = run_tagging(
        &pool,
        None,
        None,
        TaggingInput::Parsed(Box::new(q)),
        &TaggingContext {
            user_id: uid,
            ..TaggingContext::default()
        },
        &policy,
    )
    .await
    .expect("run_tagging");
    let sid = suggestion.suggestion_id.expect("suggestion");
    let unmatched_id = suggestion.unmatched[0].id.clone();

    let mut body = solution_payload(&unique_name("别名确认保存"));
    body["knowledge_node_ids"] = json!([target_id]);
    body["ai_tagging_confirmation"] = json!({
        "suggestion_id": sid,
        "unmatched_ids": [unmatched_id],
        "alias_maps": [{
            "unmatched_id": unmatched_id,
            "node_id": target_id
        }]
    });
    let (status, created) = request_auth(
        &mut app,
        Method::POST,
        "/api/v1/questions",
        Some(body),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let qid = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    let row: Option<(Option<Uuid>, String)> = sqlx::query_as(
        "SELECT suggested_node_id, raw_name FROM tag_candidates WHERE source_question_id = $1 AND raw_name = $2",
    )
    .bind(qid)
    .bind(&unmatched_raw)
    .fetch_optional(&pool)
    .await
    .unwrap();
    let (suggested, raw) = row.expect("应写入带 suggested_node_id 的候选");
    assert_eq!(suggested, Some(target_id));
    assert_eq!(raw, unmatched_raw);
}

