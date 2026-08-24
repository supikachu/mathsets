// V2.1.1 P1：标签治理（候选审核四分支 / canonical 合并与环检测 / 检索过滤）集成测试

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

fn unique_name(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

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
    let username = format!("tg_{}", Uuid::new_v4().to_string().split('-').next().unwrap());
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

/// 注册 → 提升为管理员 → 重新登录
async fn register_admin(app: &mut axum::Router, pool: &sqlx::PgPool) -> (String, String) {
    let (token, user_id) = register_and_login(app).await;
    sqlx::query("UPDATE users SET role = 'admin', global_role = 'super_admin' WHERE id = $1")
        .bind(Uuid::parse_str(&user_id).unwrap())
        .execute(pool)
        .await
        .expect("提升管理员失败");
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(Uuid::parse_str(&user_id).unwrap())
        .fetch_one(pool)
        .await
        .expect("查询用户名失败");
    let (_, login) = request(
        app,
        Method::POST,
        "/api/v1/auth/login",
        Some(json!({ "username": username, "password": "test123" })),
    )
    .await;
    (login["token"].as_str().unwrap().to_string(), user_id)
}

async fn create_question(app: &mut axum::Router, token: &str, stem: &str) -> String {
    let (status, body) = post_auth(
        app,
        "/api/v1/questions",
        json!({
            "stem": stem,
            "question_type": "solution",
            "difficulty": 3,
            "correct_answer": null,
            "analysis": null,
            "structure": mathset::testing::solution_structure_json("解。", "解析。")
        }),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn test_candidate_review_four_branches() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (admin_token, _) = register_admin(&mut app, &pool).await;

    // 准备：知识树 + 目标节点 + 题目
    let tree_id = Uuid::new_v4();
    let tree_code = format!("tg_tree_{}", Uuid::new_v4().simple());
    sqlx::query(
        r#"
        INSERT INTO knowledge_trees (id, code, name, kind, is_active, created_at, updated_at)
        VALUES ($1, $2, '测试知识树', 'knowledge', TRUE, NOW(), NOW())
        "#,
    )
    .bind(tree_id)
    .bind(&tree_code)
    .execute(&pool)
    .await
    .expect("插入知识树失败");

    let target_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO knowledge_nodes (id, tree_id, path, depth, name, is_active, status, source, created_at, updated_at)
        VALUES ($1, $2, 'a_b', 1, $3, TRUE, 'active', 'system', NOW(), NOW())
        "#,
    )
    .bind(target_id)
    .bind(tree_id)
    .bind(unique_name("二次函数"))
    .execute(&pool)
    .await
    .expect("插入节点失败");

    let q = create_question(&mut app, &admin_token, "候选审核测试题").await;
    let q_uuid = Uuid::parse_str(&q).unwrap();

    // 插入 4 条候选（直连 DB，模拟 Worker 写入）
    async fn make_candidate(pool: &sqlx::PgPool, raw: &str, question_id: Uuid) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO tag_candidates (id, kind, raw_name, normalized_name, ai_confidence, match_score, source_task_id, source_question_id)
            VALUES ($1, 'knowledge', $2, $2, 0.8, 0, NULL, $3)
            "#,
        )
        .bind(id)
        .bind(raw)
        .bind(question_id)
        .execute(pool)
        .await
        .expect("插入候选失败");
        id
    }
    let name_new = unique_name("参数分离法");
    let name_alias = unique_name("二次函数的图像");
    let name_merge = unique_name("二次函数的最值问题");
    let name_reject = unique_name("无关术语");
    let c_new = make_candidate(&pool, &name_new, q_uuid).await;
    let c_alias = make_candidate(&pool, &name_alias, q_uuid).await;
    let c_merge = make_candidate(&pool, &name_merge, q_uuid).await;
    let c_reject = make_candidate(&pool, &name_reject, q_uuid).await;

    // 非管理员 → 403
    let (t2, _) = register_and_login(&mut app).await;
    let (status, _) = get_auth(&mut app, "/api/v1/admin/tag-candidates", &t2).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 列表 + 详情
    let (status, body) = get_auth(&mut app, "/api/v1/admin/tag-candidates", &admin_token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["total"].as_i64().unwrap() >= 4);
    let (status, body) = get_auth(
        &mut app,
        &format!("/api/v1/admin/tag-candidates/{c_new}"),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["candidate"]["raw_name"], name_new);
    assert!(body["source_stem"].is_string());
    assert!(body["source_question"]["stem"].is_string());
    assert!(body["source_question"]["question_type"].is_string());
    // 建议节点/标签摘要字段始终返回（无建议时为 null）
    assert!(body.get("suggested_node").is_some());
    assert!(body.get("suggested_tag").is_some());

    // ── 分支 1：new_node（接受为新标签） ──
    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/admin/tag-candidates/{c_new}/approve"),
        json!({ "action": "new_node", "tree_id": tree_id, "name": name_new }),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let new_node_id = body["target_node_id"].as_str().unwrap().to_string();
    let node_status: String = sqlx::query_scalar("SELECT status FROM knowledge_nodes WHERE id = $1")
        .bind(Uuid::parse_str(&new_node_id).unwrap())
        .fetch_one(&pool)
        .await
        .expect("查询新节点失败");
    assert_eq!(node_status, "active");
    // 题目已关联到新节点
    let linked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM question_knowledge_nodes WHERE question_id = $1 AND node_id = $2",
    )
    .bind(q_uuid)
    .bind(Uuid::parse_str(&new_node_id).unwrap())
    .fetch_one(&pool)
    .await
    .expect("查询关联失败");
    assert_eq!(linked, 1, "题目应关联到新标签");

    // ── 分支 2：alias（加为已有标签别名） ──
    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/admin/tag-candidates/{c_alias}/approve"),
        json!({ "action": "alias", "target_node_id": target_id }),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let aliases: serde_json::Value =
        sqlx::query_scalar("SELECT aliases FROM knowledge_nodes WHERE id = $1")
            .bind(target_id)
            .fetch_one(&pool)
            .await
            .expect("查询别名失败");
    assert!(
        aliases.to_string().contains(&name_alias),
        "别名应写入: {aliases}"
    );

    // ── 分支 3：merge（并入已有标签 + 审计） ──
    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/admin/tag-candidates/{c_merge}/approve"),
        json!({ "action": "merge", "target_node_id": target_id, "reason": "同义标签" }),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let records: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tag_merge_records WHERE source_tag_id = $1")
            .bind(c_merge)
            .fetch_one(&pool)
            .await
            .expect("查询审计失败");
    assert_eq!(records, 1, "merge 分支应写审计记录");
    let merge_status: String =
        sqlx::query_scalar("SELECT status FROM tag_candidates WHERE id = $1")
            .bind(c_merge)
            .fetch_one(&pool)
            .await
            .expect("查询 merge 状态失败");
    assert_eq!(merge_status, "merged");

    // ── 分支 4：reject ──
    let (status, _) = post_auth(
        &mut app,
        &format!("/api/v1/admin/tag-candidates/{c_reject}/reject"),
        json!({}),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let st: String = sqlx::query_scalar("SELECT status FROM tag_candidates WHERE id = $1")
        .bind(c_reject)
        .fetch_one(&pool)
        .await
        .expect("查询候选失败");
    assert_eq!(st, "rejected");

    // 重复处理 → 409
    let (status, _) = post_auth(
        &mut app,
        &format!("/api/v1/admin/tag-candidates/{c_reject}/reject"),
        json!({}),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_merge_knowledge_node_cycle_and_relations() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (admin_token, _) = register_admin(&mut app, &pool).await;

    let tree_id = Uuid::new_v4();
    let tree_code = format!("tg_tree2_{}", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO knowledge_trees (id, code, name, kind, is_active, created_at, updated_at) VALUES ($1, $2, '树2', 'knowledge', TRUE, NOW(), NOW())",
    )
    .bind(tree_id)
    .bind(&tree_code)
    .execute(&pool)
    .await
    .expect("插入知识树失败");

    async fn insert_node(pool: &sqlx::PgPool, id: Uuid, tree_id: Uuid, name: &str, path: &str) {
        sqlx::query(
            "INSERT INTO knowledge_nodes (id, tree_id, path, depth, name, is_active, status, source, created_at, updated_at) VALUES ($1, $2, $3::ltree, 1, $4, TRUE, 'active', 'system', NOW(), NOW())",
        )
        .bind(id)
        .bind(tree_id)
        .bind(path)
        .bind(name)
        .execute(pool)
        .await
        .expect("插入节点失败");
    }
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let c = Uuid::new_v4();
    insert_node(&pool, a, tree_id, "A 标签", "n_a").await;
    insert_node(&pool, b, tree_id, "B 标签", "n_b").await;
    insert_node(&pool, c, tree_id, "C 标签", "n_c").await;

    // 题目关联到 A
    let q = create_question(&mut app, &admin_token, "合并测试题").await;
    let q_uuid = Uuid::parse_str(&q).unwrap();
    sqlx::query(
        "INSERT INTO question_knowledge_nodes (question_id, node_id, source, created_at) VALUES ($1, $2, 'manual', NOW())",
    )
    .bind(q_uuid)
    .bind(a)
    .execute(&pool)
    .await
    .expect("插入关联失败");

    // A → B 合并
    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/knowledge-nodes/{a}/merge"),
        json!({ "target_id": b, "reason": "同义" }),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["migrated_relations"], 1);

    let (a_status, a_canonical): (String, Option<Uuid>) = sqlx::query_as(
        "SELECT status, canonical_id FROM knowledge_nodes WHERE id = $1",
    )
    .bind(a)
    .fetch_one(&pool)
    .await
    .expect("查询 A 失败");
    assert_eq!(a_status, "merged", "A 应标记 merged");
    assert_eq!(a_canonical, Some(b), "A.canonical_id 应指向 B");

    // 题目关联已迁移到 B，A 上无残留
    let on_b: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM question_knowledge_nodes WHERE question_id = $1 AND node_id = $2",
    )
    .bind(q_uuid)
    .bind(b)
    .fetch_one(&pool)
    .await
    .expect("查询 B 关联失败");
    assert_eq!(on_b, 1);
    let on_a: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM question_knowledge_nodes WHERE question_id = $1 AND node_id = $2",
    )
    .bind(q_uuid)
    .bind(a)
    .fetch_one(&pool)
    .await
    .expect("查询 A 关联失败");
    assert_eq!(on_a, 0, "源节点关联应迁移走");

    // 审计记录
    let records: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tag_merge_records WHERE source_tag_id = $1 AND target_tag_id = $2",
    )
    .bind(a)
    .bind(b)
    .fetch_one(&pool)
    .await
    .expect("查询审计失败");
    assert_eq!(records, 1);

    // 环检测：B → A 被拒（A 已是 merged 目标；链检测同样命中）
    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/knowledge-nodes/{b}/merge"),
        json!({ "target_id": a }),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"].as_str().unwrap().contains("环") || body["error"].as_str().unwrap().contains("合并标签"),
        "{body}"
    );

    // 自合并 → 400
    let (status, _) = post_auth(
        &mut app,
        &format!("/api/v1/knowledge-nodes/{c}/merge"),
        json!({ "target_id": c }),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // 正常合并 C → B（B 是最终标签）
    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/knowledge-nodes/{c}/merge"),
        json!({ "target_id": b }),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let c_canonical: Option<Uuid> =
        sqlx::query_scalar("SELECT canonical_id FROM knowledge_nodes WHERE id = $1")
            .bind(c)
            .fetch_one(&pool)
            .await
            .expect("查询 C 失败");
    assert_eq!(c_canonical, Some(b));
}

#[tokio::test]
async fn test_search_filters_year_region_document_type() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (token, user_id) = register_and_login(&mut app).await;
    let user_uuid = Uuid::parse_str(&user_id).unwrap();

    // document(exam) + paper(2025, 杭州) + question 关联
    let doc_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO documents (id, creator_id, file_name, page_count, status, document_type, title) VALUES ($1, $2, '卷.pdf', 1, 'confirmed', 'exam', '2025期中')",
    )
    .bind(doc_id)
    .bind(user_uuid)
    .execute(&pool)
    .await
    .expect("插入文档失败");

    let paper_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO papers (id, title, subject, status, creator_id, document_id, year, semester, region_province, region_city, source_type, created_at, updated_at, version)
        VALUES ($1, '2025高一期中', '数学', 'draft', $2, $3, 2025, 'first', '浙江省', '杭州市', 'exam', NOW(), NOW(), 1)
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
        "INSERT INTO question_collections (id, document_id, creator_id, title, collection_type) VALUES ($1, $2, $3, '课堂练习', 'class_exercise')",
    )
    .bind(collection_id)
    .bind(doc_id)
    .bind(user_uuid)
    .execute(&pool)
    .await
    .expect("插入集合失败");

    let q_paper = create_question(&mut app, &token, "试卷来源题").await;
    let q_col = create_question(&mut app, &token, "集合来源题").await;
    sqlx::query(
        "INSERT INTO paper_questions (id, paper_id, question_id, sort_order, score, display_order, created_at) VALUES ($1, $2, $3, 1, 5, 1, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(paper_id)
    .bind(Uuid::parse_str(&q_paper).unwrap())
    .execute(&pool)
    .await
    .expect("插入试卷关联失败");
    sqlx::query(
        "INSERT INTO collection_questions (id, collection_id, question_id, display_order, created_at) VALUES ($1, $2, $3, 1, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(collection_id)
    .bind(Uuid::parse_str(&q_col).unwrap())
    .execute(&pool)
    .await
    .expect("插入集合关联失败");

    // 题目检索：year / document_type / collection_id / region
    let (status, body) = get_auth(&mut app, "/api/v1/questions?year=2025", &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ids: Vec<&str> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|q| q["id"].as_str())
        .collect();
    assert!(ids.contains(&q_paper.as_str()), "year=2025 应命中试卷来源题: {body}");
    assert!(!ids.contains(&q_col.as_str()), "year=2025 不应命中集合来源题");

    let (_, body) = get_auth(&mut app, "/api/v1/questions?document_type=exam", &token).await;
    let ids: Vec<&str> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|q| q["id"].as_str())
        .collect();
    assert!(ids.contains(&q_paper.as_str()) && ids.contains(&q_col.as_str()), "document_type=exam 应命中两类来源题: {body}");

    let (_, body) = get_auth(
        &mut app,
        &format!("/api/v1/questions?collection_id={collection_id}"),
        &token,
    )
    .await;
    let ids: Vec<&str> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|q| q["id"].as_str())
        .collect();
    assert!(ids.contains(&q_col.as_str()) && !ids.contains(&q_paper.as_str()), "collection_id 过滤错误: {body}");

    // 试卷检索：year + region 组合
    let (status, body) = get_auth(&mut app, "/api/v1/papers?year=2025&region=杭州市", &token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["items"].as_array().unwrap().iter().any(|p| p["id"] == json!(paper_id.to_string())), "试卷组合过滤应命中: {body}");
    let (_, body) = get_auth(&mut app, "/api/v1/papers?document_type=exam", &token).await;
    assert!(body["items"].as_array().unwrap().iter().any(|p| p["id"] == json!(paper_id.to_string())));
}

#[tokio::test]
async fn test_tag_usage_endpoint() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    // 全局预置标签需要管理员权限
    let (token, _) = register_admin(&mut app, &pool).await;

    let (status, body) = post_auth(
        &mut app,
        "/api/v1/tags",
        json!({ "name": "数形结合", "category": "method" }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let tag_id = body["id"].as_str().unwrap().to_string();

    let (status, body) = get_auth(&mut app, &format!("/api/v1/tags/{tag_id}/usage"), &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "数形结合");
    assert_eq!(body["category"], "method");
    assert!(body["question_count"].is_number());
}

/// tag_candidates.kind 支持 pattern（题型专题），与 method（通用方法）隔离
#[tokio::test]
async fn test_pattern_kind_candidate_list_filter() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (admin_token, _) = register_admin(&mut app, &pool).await;
    let q = create_question(&mut app, &admin_token, "题型专题候选测试").await;
    let q_uuid = Uuid::parse_str(&q).unwrap();
    let cid = Uuid::new_v4();
    let raw = format!("凹凸反转_{}", cid.simple());
    sqlx::query(
        r#"
        INSERT INTO tag_candidates (id, kind, raw_name, normalized_name, ai_confidence, match_score, source_question_id)
        VALUES ($1, 'pattern', $2, $2, 0.7, 0, $3)
        "#,
    )
    .bind(cid)
    .bind(&raw)
    .bind(q_uuid)
    .execute(&pool)
    .await
    .expect("插入 pattern 候选失败");

    let (status, body) = get_auth(
        &mut app,
        "/api/v1/admin/tag-candidates?kind=pattern&status=pending",
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body["items"].as_array().expect("items 应为数组");
    assert!(
        items.iter().any(|c| c["id"] == json!(cid.to_string()) && c["kind"] == "pattern"),
        "应按 kind=pattern 过滤到刚插入的候选: {body}"
    );
}

async fn insert_candidate(
    pool: &sqlx::PgPool,
    kind: &str,
    target_type: &str,
    raw: &str,
    question_id: Uuid,
) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO tag_candidates (id, kind, target_type, raw_name, normalized_name, ai_confidence, match_score, source_question_id)
        VALUES ($1, $2, $3, $4, $4, 0.8, 0, $5)
        "#,
    )
    .bind(id)
    .bind(kind)
    .bind(target_type)
    .bind(raw)
    .bind(question_id)
    .execute(pool)
    .await
    .expect("插入候选失败");
    id
}

async fn insert_tree(pool: &sqlx::PgPool, kind: &str) -> Uuid {
    let tree_id = Uuid::new_v4();
    let code = format!("tg_{}_{}", kind, Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO knowledge_trees (id, code, name, kind, is_active, created_at, updated_at) VALUES ($1, $2, $3, $4::knowledge_tree_kind, TRUE, NOW(), NOW())",
    )
    .bind(tree_id)
    .bind(&code)
    .bind(format!("树-{kind}"))
    .bind(kind)
    .execute(pool)
    .await
    .expect("插入知识树失败");
    tree_id
}

async fn insert_node(pool: &sqlx::PgPool, tree_id: Uuid, name: &str, path: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO knowledge_nodes (id, tree_id, path, depth, name, is_active, status, source, question_count, created_at, updated_at) VALUES ($1, $2, $3::ltree, 1, $4, TRUE, 'active', 'system', 0, NOW(), NOW())",
    )
    .bind(id)
    .bind(tree_id)
    .bind(path)
    .bind(name)
    .execute(pool)
    .await
    .expect("插入节点失败");
    id
}

#[tokio::test]
async fn test_candidate_kind_tree_mismatch_rejected() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (admin_token, _) = register_admin(&mut app, &pool).await;
    let knowledge_tree = insert_tree(&pool, "knowledge").await;
    let q = create_question(&mut app, &admin_token, "维度校验题").await;
    let cid = insert_candidate(
        &pool,
        "chapter",
        "knowledge_node",
        &unique_name("人教A版高一上"),
        Uuid::parse_str(&q).unwrap(),
    )
    .await;

    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/admin/tag-candidates/{cid}/approve"),
        json!({ "action": "new_node", "tree_id": knowledge_tree }),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"].as_str().unwrap_or("").contains("章节"),
        "{body}"
    );
}

#[tokio::test]
async fn test_candidate_parent_must_belong_to_tree() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (admin_token, _) = register_admin(&mut app, &pool).await;
    let tree_a = insert_tree(&pool, "knowledge").await;
    let tree_b = insert_tree(&pool, "knowledge").await;
    let parent = insert_node(&pool, tree_a, &unique_name("函数"), &format!("fn{}", Uuid::new_v4().simple())).await;
    let q = create_question(&mut app, &admin_token, "父节点树校验题").await;
    let node_name = unique_name("二次函数");
    let cid = insert_candidate(
        &pool,
        "knowledge",
        "knowledge_node",
        &node_name,
        Uuid::parse_str(&q).unwrap(),
    )
    .await;

    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/admin/tag-candidates/{cid}/approve"),
        json!({ "action": "new_node", "tree_id": tree_b, "parent_id": parent, "name": node_name }),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"].as_str().unwrap_or("").contains("父节点"),
        "{body}"
    );
}

#[tokio::test]
async fn test_method_candidate_creates_tag_and_reject_persists_reason() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (admin_token, _) = register_admin(&mut app, &pool).await;
    let q = create_question(&mut app, &admin_token, "方法候选题").await;
    let q_uuid = Uuid::parse_str(&q).unwrap();
    let unique = format!("参数分离_{}", Uuid::new_v4().simple());
    let c_new = insert_candidate(&pool, "method", "tag", &unique, q_uuid).await;
    let c_reject = insert_candidate(&pool, "method", "tag", &unique_name("应拒绝的方法"), q_uuid).await;

    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/admin/tag-candidates/{c_new}/approve"),
        json!({ "action": "new_node", "name": unique }),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let tag_id = body["target_tag_id"].as_str().expect("应返回 target_tag_id");
    let tag_id = Uuid::parse_str(tag_id).unwrap();
    let category: String = sqlx::query_scalar("SELECT category::text FROM tags WHERE id = $1")
        .bind(tag_id)
        .fetch_one(&pool)
        .await
        .expect("查询新标签失败");
    assert_eq!(category, "method");
    let linked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM question_tags_relation WHERE question_id = $1 AND tag_id = $2",
    )
    .bind(q_uuid)
    .bind(tag_id)
    .fetch_one(&pool)
    .await
    .expect("查询标签关联失败");
    assert_eq!(linked, 1);
    let use_count: i32 = sqlx::query_scalar("SELECT use_count FROM tags WHERE id = $1")
        .bind(tag_id)
        .fetch_one(&pool)
        .await
        .expect("查询 use_count 失败");
    assert_eq!(use_count, 1);

    let (status, _) = get_auth(
        &mut app,
        "/api/v1/admin/tag-candidates?target_type=tag&kind=method&status=approved",
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/admin/tag-candidates/{c_reject}/reject"),
        json!({ "reason": "不是通用方法" }),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let note: Option<String> =
        sqlx::query_scalar("SELECT review_note FROM tag_candidates WHERE id = $1")
            .bind(c_reject)
            .fetch_one(&pool)
            .await
            .expect("查询拒绝原因失败");
    assert_eq!(note.as_deref(), Some("不是通用方法"));
}

#[tokio::test]
async fn test_approve_does_not_increment_count_when_relation_exists() {
    let Some((mut app, pool)) = create_test_app().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let (admin_token, _) = register_admin(&mut app, &pool).await;
    let tree_id = insert_tree(&pool, "knowledge").await;
    let node_id = insert_node(&pool, tree_id, &unique_name("二次函数"), &format!("q{}", Uuid::new_v4().simple())).await;
    let q = create_question(&mut app, &admin_token, "计数幂等题").await;
    let q_uuid = Uuid::parse_str(&q).unwrap();
    sqlx::query(
        "INSERT INTO question_knowledge_nodes (question_id, node_id, source, created_at) VALUES ($1, $2, 'manual', NOW())",
    )
    .bind(q_uuid)
    .bind(node_id)
    .execute(&pool)
    .await
    .expect("预置关联失败");
    let before: i32 = sqlx::query_scalar("SELECT question_count FROM knowledge_nodes WHERE id = $1")
        .bind(node_id)
        .fetch_one(&pool)
        .await
        .expect("查询计数失败");
    let cid = insert_candidate(&pool, "knowledge", "knowledge_node", &unique_name("二次函数图像"), q_uuid).await;

    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/admin/tag-candidates/{cid}/approve"),
        json!({ "action": "alias", "target_node_id": node_id }),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let count: i32 = sqlx::query_scalar("SELECT question_count FROM knowledge_nodes WHERE id = $1")
        .bind(node_id)
        .fetch_one(&pool)
        .await
        .expect("查询计数失败");
    assert_eq!(count, before, "已有关联时不应再增加 question_count");
}
