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
        "correct_answer": null,
        "analysis": null,
        "structure": mathset::testing::solution_structure_json("解。", "解析。")
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
        parts: vec![],
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
        parts: vec![],
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

async fn insert_parse_task(pool: &sqlx::PgPool, creator_id: Uuid) -> Uuid {
    sqlx::query_scalar("INSERT INTO ai_parse_tasks (creator_id) VALUES ($1) RETURNING id")
        .bind(creator_id)
        .fetch_one(pool)
        .await
        .expect("insert parse task")
}

async fn insert_tagging_task(
    pool: &sqlx::PgPool,
    creator_id: Uuid,
    parse_task_id: Uuid,
    status: &str,
) -> Uuid {
    sqlx::query_scalar(
        r#"
        INSERT INTO ai_tagging_tasks
          (creator_id, input_hash, content, status, parse_task_id, source_index)
        VALUES ($1, $2, '题干', $3, $4, $5)
        RETURNING id
        "#,
    )
    .bind(creator_id)
    .bind(unique_name("hash"))
    .bind(status)
    .bind(parse_task_id)
    .bind(unique_name("p1_i"))
    .fetch_one(pool)
    .await
    .expect("insert tagging task")
}

/// 题目全部保存后终止残留打标任务：pending 立即终止，processing 打取消标记，
/// 已结束的任务不动；他人无权终止。
#[tokio::test]
async fn test_cancel_parse_tagging_tasks() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token, user_id) = register_and_login(&mut app).await;
    let uid = Uuid::parse_str(&user_id).unwrap();
    let parse_id = insert_parse_task(&pool, uid).await;

    let pending_id = insert_tagging_task(&pool, uid, parse_id, "pending").await;
    let processing_id = insert_tagging_task(&pool, uid, parse_id, "processing").await;
    let done_id = insert_tagging_task(&pool, uid, parse_id, "success").await;

    // 他人无权终止：计数为 0，任务状态不变
    let (token2, _) = register_and_login(&mut app).await;
    let (status, body) = request_auth(
        &mut app,
        Method::POST,
        &format!("/api/v1/ai/parse-task/{parse_id}/cancel-tagging"),
        Some(json!({})),
        &token2,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["cancelled"], 0);
    assert_eq!(body["cancelling"], 0);
    let still: String = sqlx::query_scalar("SELECT status FROM ai_tagging_tasks WHERE id = $1")
        .bind(pending_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(still, "pending", "他人调用不应影响任务");

    let (status, body) = request_auth(
        &mut app,
        Method::POST,
        &format!("/api/v1/ai/parse-task/{parse_id}/cancel-tagging"),
        Some(json!({})),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["cancelled"], 1, "pending 应立即终止");
    assert_eq!(body["cancelling"], 1, "processing 应打取消标记");

    let (st, cancel_at): (String, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT status, cancel_requested_at FROM ai_tagging_tasks WHERE id = $1",
    )
    .bind(pending_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(st, "cancelled");
    assert!(cancel_at.is_none(), "pending 直接终止，无需取消标记");

    let (st, cancel_at): (String, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT status, cancel_requested_at FROM ai_tagging_tasks WHERE id = $1",
    )
    .bind(processing_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(st, "processing", "processing 由 Worker 收敛，不直接改状态");
    assert!(cancel_at.is_some(), "processing 应写入取消标记");

    let st: String = sqlx::query_scalar("SELECT status FROM ai_tagging_tasks WHERE id = $1")
        .bind(done_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(st, "success", "已结束的任务不应被改写");

    // 幂等：再调一次不再产生变更
    let (status, body) = request_auth(
        &mut app,
        Method::POST,
        &format!("/api/v1/ai/parse-task/{parse_id}/cancel-tagging"),
        Some(json!({})),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["cancelled"], 0);
    assert_eq!(body["cancelling"], 0);
}

/// 站外结构化：导入后 idle，点击 start-tagging 才入队；他人无权启动。
#[tokio::test]
async fn test_start_parse_tagging_tasks() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token, user_id) = register_and_login(&mut app).await;
    let uid = Uuid::parse_str(&user_id).unwrap();
    let parse_id = insert_parse_task(&pool, uid).await;
    let space_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM spaces WHERE kind = 'personal' AND owner_user_id = $1",
    )
    .bind(uid)
    .fetch_one(&pool)
    .await
    .expect("查询个人空间");

    sqlx::query(
        r#"
        UPDATE ai_parse_tasks
        SET progress = $1
        WHERE id = $2
        "#,
    )
    .bind(json!({
        "pipeline": "ocr_export",
        "ocr_export_ctx": { "space_id": space_id },
        "staged_questions": [{
            "index": "p1_i0",
            "saved": false,
            "tagging_status": "idle",
            "space_id": space_id,
            "parsed": {
                "question_type": "solution",
                "sub_type": null,
                "difficulty": "medium",
                "stem": "求函数 f(x) 的最大值"
            }
        }]
    }))
    .bind(parse_id)
    .execute(&pool)
    .await
    .expect("写入暂存");

    let (token2, _) = register_and_login(&mut app).await;
    let (status, body) = request_auth(
        &mut app,
        Method::POST,
        &format!("/api/v1/ai/parse-task/{parse_id}/start-tagging"),
        Some(json!({})),
        &token2,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ai_tagging_tasks WHERE parse_task_id = $1",
    )
    .bind(parse_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(before, 0);

    let (status, body) = request_auth(
        &mut app,
        Method::POST,
        &format!("/api/v1/ai/parse-task/{parse_id}/start-tagging"),
        Some(json!({})),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["started"], 1, "{body}");

    let queued: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ai_tagging_tasks WHERE parse_task_id = $1 AND source_index = 'p1_i0' AND status = 'pending'",
    )
    .bind(parse_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(queued, 1);

    let tagging_status: String = sqlx::query_scalar(
        "SELECT progress->'staged_questions'->0->>'tagging_status' FROM ai_parse_tasks WHERE id = $1",
    )
    .bind(parse_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tagging_status, "pending");

    sqlx::query(
        r#"
        UPDATE ai_parse_tasks
        SET progress = jsonb_set(
              progress,
              '{staged_questions,0,tagging_status}',
              '"pending"'
            )
        WHERE id = $1
        "#,
    )
    .bind(parse_id)
    .execute(&pool)
    .await
    .ok();

    let (status, body) = request_auth(
        &mut app,
        Method::POST,
        &format!("/api/v1/ai/parse-task/{parse_id}/cancel-tagging"),
        Some(json!({})),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["cancelled"], 1, "{body}");
    let tagging_status: String = sqlx::query_scalar(
        "SELECT progress->'staged_questions'->0->>'tagging_status' FROM ai_parse_tasks WHERE id = $1",
    )
    .bind(parse_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tagging_status, "idle", "停止后暂存应回到 idle");
}

/// 打标晚于题目保存完成：建议应被认领到已保存的题目上（否则标签永久丢失），
/// 但未匹配项未经教师确认，不得进入候选审核队列。
#[tokio::test]
async fn test_claim_suggestion_after_question_saved() {
    use mathset::ai::tagging::{
        claim_suggestion_for_saved_question, run_tagging, TaggingContext, TaggingInput,
        TaggingPolicy,
    };
    use mathset::ai::types::ParsedQuestion;

    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token, user_id) = register_and_login(&mut app).await;
    let uid = Uuid::parse_str(&user_id).unwrap();

    let tree = insert_tree(&pool, "knowledge").await;
    let node_name = unique_name("晚到知识点");
    let node_id = insert_node(&pool, tree, &node_name).await;
    let unmatched_raw = unique_name("晚到未匹配词");

    let q = ParsedQuestion {
        question_type: "solution".into(),
        sub_type: None,
        difficulty: Some("medium".into()),
        stem: unique_name("先保存后打标题干"),
        options: None,
        correct_answer: None,
        analysis: vec![],
        knowledge_points: vec![node_name.clone(), unmatched_raw.clone()],
        confidence: 0.9,
        warnings: vec![],
        image_placeholders: vec![],
        image_urls: vec![],
        kp_matches: vec![],
        parts: vec![],
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
    let sid = suggestion.suggestion_id.expect("应写入 suggestion");

    // 用户等不到标签就保存：不带 confirmation，落库时没有任何知识点
    let (status, created) = request_auth(
        &mut app,
        Method::POST,
        "/api/v1/questions",
        Some(solution_payload(&unique_name("先保存后打标"))),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let qid = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();

    let before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM question_knowledge_nodes WHERE question_id = $1")
            .bind(qid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(before, 0, "打标未完成就保存，此时应无任何关联");

    let claimed = claim_suggestion_for_saved_question(&pool, sid, qid)
        .await
        .expect("claim");
    assert!(claimed, "pending 建议应被认领到已保存题目");

    let links: Vec<(Uuid, String, Option<Uuid>)> = sqlx::query_as(
        "SELECT node_id, source::text, suggestion_id FROM question_knowledge_nodes WHERE question_id = $1",
    )
    .bind(qid)
    .fetch_all(&pool)
    .await
    .unwrap();
    let link = links
        .iter()
        .find(|l| l.0 == node_id)
        .expect("匹配节点应被补写");
    assert_eq!(link.1, "ai");
    assert_eq!(link.2, Some(sid));

    let (st, bound): (String, Option<Uuid>) = sqlx::query_as(
        "SELECT status, question_id FROM ai_tagging_suggestions WHERE id = $1",
    )
    .bind(sid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(st, "applied");
    assert_eq!(bound, Some(qid), "建议应绑定到该题目");

    let cand: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tag_candidates WHERE source_question_id = $1 AND raw_name = $2",
    )
    .bind(qid)
    .bind(&unmatched_raw)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cand, 0, "认领不应把未确认的未匹配项写入候选");

    // 已 applied 的建议不能再被认领到别的题目
    let (status, other) = request_auth(
        &mut app,
        Method::POST,
        "/api/v1/questions",
        Some(solution_payload(&unique_name("另一道题"))),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{other}");
    let other_qid = Uuid::parse_str(other["id"].as_str().unwrap()).unwrap();
    let again = claim_suggestion_for_saved_question(&pool, sid, other_qid)
        .await
        .expect("claim again");
    assert!(!again, "已应用的建议不应被再次认领");
    let stolen: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM question_knowledge_nodes WHERE question_id = $1")
            .bind(other_qid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stolen, 0, "第二道题不应拿到别人的标签");
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
        parts: vec![],
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

/// 丢弃未确认暂存：未保存项删除、已保存项保留；进行中的打标被终止；他人 403。
#[tokio::test]
async fn test_clear_parse_staged_questions() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token, user_id) = register_and_login(&mut app).await;
    let uid = Uuid::parse_str(&user_id).unwrap();
    let parse_id = insert_parse_task(&pool, uid).await;

    sqlx::query(
        r#"
        UPDATE ai_parse_tasks
        SET progress = $1
        WHERE id = $2
        "#,
    )
    .bind(json!({
        "staged_questions": [
            {"index": "p1_i0", "saved": false, "parsed": {"stem": "未保存甲"}},
            {"index": "p1_i1", "saved": true, "parsed": {"stem": "已保存"}},
            {"index": "p1_i2", "saved": false, "parsed": {"stem": "未保存乙"}}
        ]
    }))
    .bind(parse_id)
    .execute(&pool)
    .await
    .expect("写入暂存");

    let pending_id = insert_tagging_task(&pool, uid, parse_id, "pending").await;
    let processing_id = insert_tagging_task(&pool, uid, parse_id, "processing").await;
    let done_id = insert_tagging_task(&pool, uid, parse_id, "success").await;

    let missing = Uuid::new_v4();
    let (status, body) = request_auth(
        &mut app,
        Method::POST,
        &format!("/api/v1/ai/parse-task/{missing}/clear-staged"),
        Some(json!({})),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (token2, _) = register_and_login(&mut app).await;
    let (status, body) = request_auth(
        &mut app,
        Method::POST,
        &format!("/api/v1/ai/parse-task/{parse_id}/clear-staged"),
        Some(json!({})),
        &token2,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let still: i32 = sqlx::query_scalar(
        "SELECT jsonb_array_length(COALESCE(progress->'staged_questions', '[]'::jsonb)) FROM ai_parse_tasks WHERE id = $1",
    )
    .bind(parse_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(still, 3, "他人调用不应改写暂存");

    let (status, body) = request_auth(
        &mut app,
        Method::POST,
        &format!("/api/v1/ai/parse-task/{parse_id}/clear-staged"),
        Some(json!({})),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["removed"], json!(2), "{body}");
    assert_eq!(body["kept"], json!(1), "{body}");

    let kept: Vec<(String, bool)> = sqlx::query_as(
        r#"
        SELECT elem->>'index', COALESCE((elem->>'saved')::boolean, false)
        FROM ai_parse_tasks,
             jsonb_array_elements(COALESCE(progress->'staged_questions', '[]'::jsonb)) AS elem
        WHERE id = $1
        ORDER BY elem->>'index'
        "#,
    )
    .bind(parse_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(kept, vec![("p1_i1".to_string(), true)]);

    let pending_st: String = sqlx::query_scalar("SELECT status FROM ai_tagging_tasks WHERE id = $1")
        .bind(pending_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(pending_st, "cancelled");

    let (proc_st, cancel_at): (String, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT status, cancel_requested_at FROM ai_tagging_tasks WHERE id = $1",
    )
    .bind(processing_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(proc_st, "processing");
    assert!(cancel_at.is_some(), "processing 应写入取消标记");

    let done_st: String = sqlx::query_scalar("SELECT status FROM ai_tagging_tasks WHERE id = $1")
        .bind(done_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(done_st, "success");

    let (status, body) = request_auth(
        &mut app,
        Method::POST,
        &format!("/api/v1/ai/parse-task/{parse_id}/clear-staged"),
        Some(json!({})),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["removed"], json!(0));
    assert_eq!(body["kept"], json!(1));
}

/// 丢弃全部未保存暂存后，应删掉该资料下 0 题的草稿试卷；已有题目的卷保留。
#[tokio::test]
async fn test_clear_staged_deletes_empty_draft_paper() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token, user_id) = register_and_login(&mut app).await;
    let uid = Uuid::parse_str(&user_id).unwrap();

    let doc_id: Uuid = sqlx::query_scalar(
        "INSERT INTO documents (creator_id, file_name, status) VALUES ($1, 'gaokao.pdf', 'confirmed') RETURNING id",
    )
    .bind(uid)
    .fetch_one(&pool)
    .await
    .expect("insert document");

    let empty_paper: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO papers (id, title, subject, status, creator_id, document_id, created_at, updated_at, version)
        VALUES ($1, '2024年高考数学试卷（新课标Ⅰ卷）（解析卷）', '数学', 'draft', $2, $3, NOW(), NOW(), 1)
        RETURNING id
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(uid)
    .bind(doc_id)
    .fetch_one(&pool)
    .await
    .expect("insert empty paper");

    let filled_paper: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO papers (id, title, subject, status, creator_id, created_at, updated_at, version)
        VALUES ($1, '手动空卷应保留', '数学', 'draft', $2, NOW(), NOW(), 1)
        RETURNING id
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(uid)
    .fetch_one(&pool)
    .await
    .expect("insert manual paper");

    sqlx::query("UPDATE documents SET metadata = jsonb_build_object('linked_paper_id', $2::text) WHERE id = $1")
        .bind(doc_id)
        .bind(empty_paper.to_string())
        .execute(&pool)
        .await
        .expect("link paper metadata");

    let parse_id: Uuid = sqlx::query_scalar(
        "INSERT INTO ai_parse_tasks (creator_id, document_id, progress) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(uid)
    .bind(doc_id)
    .bind(json!({
        "staged_questions": [
            {"index": "p1_i0", "saved": false, "parsed": {"stem": "未保存"}}
        ]
    }))
    .fetch_one(&pool)
    .await
    .expect("insert parse task");

    let (status, body) = request_auth(
        &mut app,
        Method::POST,
        &format!("/api/v1/ai/parse-task/{parse_id}/clear-staged"),
        Some(json!({})),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["removed"], json!(1), "{body}");
    assert_eq!(body["kept"], json!(0), "{body}");

    let empty_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM papers WHERE id = $1)")
        .bind(empty_paper)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!empty_exists, "丢弃后应删除 0 题草稿试卷");

    let filled_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM papers WHERE id = $1)")
        .bind(filled_paper)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(filled_exists, "无 document_id 的手动试卷不应被删");

    let linked: Option<String> = sqlx::query_scalar("SELECT metadata->>'linked_paper_id' FROM documents WHERE id = $1")
        .bind(doc_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(linked.is_none() || linked.as_deref() == Some(""), "应清掉 linked_paper_id");
}

