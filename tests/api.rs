use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use mathset::auth::jwt::verify_token;
use mathset::build_app;
use mathset::db;
use mathset::AppState;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

async fn create_test_app() -> Option<axum::Router> {
    let database_url = std::env::var("DATABASE_URL").ok()?;
    let pool = db::create_pool(&database_url).await;
    db::run_migrations(&pool).await;
    let state = AppState {
        pool,
        jwt_secret: "test-secret-for-integration-tests".to_string(),
        jwt_expiry_hours: 24,
    };
    Some(build_app(state))
}

async fn request(
    app: &mut axum::Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(ref b) = body {
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
    if let Some(ref b) = body {
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

async fn get_json(app: &mut axum::Router, uri: &str) -> (StatusCode, Value) {
    request(app, Method::GET, uri, None).await
}

async fn post_json(app: &mut axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    request(app, Method::POST, uri, Some(body)).await
}

async fn put_json(app: &mut axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    request(app, Method::PUT, uri, Some(body)).await
}

async fn delete_req(app: &mut axum::Router, uri: &str) -> (StatusCode, Value) {
    request(app, Method::DELETE, uri, None).await
}

async fn get_auth(app: &mut axum::Router, uri: &str, token: &str) -> (StatusCode, Value) {
    request_auth(app, Method::GET, uri, None, token).await
}

async fn post_auth(
    app: &mut axum::Router,
    uri: &str,
    body: Value,
    token: &str,
) -> (StatusCode, Value) {
    request_auth(app, Method::POST, uri, Some(body), token).await
}

async fn put_auth(
    app: &mut axum::Router,
    uri: &str,
    body: Value,
    token: &str,
) -> (StatusCode, Value) {
    request_auth(app, Method::PUT, uri, Some(body), token).await
}

async fn delete_auth(app: &mut axum::Router, uri: &str, token: &str) -> (StatusCode, Value) {
    request_auth(app, Method::DELETE, uri, None, token).await
}

/// 注册测试用户并返回 token
async fn register_and_login(app: &mut axum::Router) -> String {
    let username = format!("test_{}", Uuid::new_v4().to_string().split('-').next().unwrap());
    let email = format!("{}@test.com", username);

    let (_, _) = post_json(
        app,
        "/api/v1/auth/register",
        json!({
            "username": username,
            "email": email,
            "password": "test123",
            "display_name": "测试用户"
        }),
    )
    .await;

    let (_, body) = post_json(
        app,
        "/api/v1/auth/login",
        json!({ "username": username, "password": "test123" }),
    )
    .await;

    body["token"].as_str().unwrap().to_string()
}

/// 注册一个组长用户并返回 token（用于审核测试）
async fn register_leader_and_login(app: &mut axum::Router) -> String {
    let username = format!("leader_{}", Uuid::new_v4().to_string().split('-').next().unwrap());
    let email = format!("{}@test.com", username);

    let (_, _) = post_json(
        app,
        "/api/v1/auth/register",
        json!({
            "username": username,
            "email": email,
            "password": "test123",
            "display_name": "组长用户"
        }),
    )
    .await;

    let (_, login_body) = post_json(
        app,
        "/api/v1/auth/login",
        json!({ "username": username, "password": "test123" }),
    )
    .await;
    let token = login_body["token"].as_str().unwrap().to_string();

    // 解码 token 获取 user_id
    let claims = mathset::auth::jwt::verify_token(
        &token,
        "test-secret-for-integration-tests",
    )
    .unwrap();

    // 用独立连接池更新角色
    let pool = mathset::db::create_pool(&std::env::var("DATABASE_URL").unwrap()).await;
    sqlx::query("UPDATE users SET role = 'groupleader' WHERE id = $1")
        .bind(claims.sub)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    // 重新登录获取带新角色的 token
    let (_, body) = post_json(
        app,
        "/api/v1/auth/login",
        json!({ "username": username, "password": "test123" }),
    )
    .await;
    body["token"].as_str().unwrap().to_string()
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
// 2. 用户注册 + 登录
// ---------------------------------------------------------------------------

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

    let (status, body) = post_json(
        &mut app,
        "/api/v1/auth/register",
        json!({
            "username": username,
            "email": format!("{}@test.com", username),
            "password": "password123",
            "display_name": "测试用户"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "注册失败: {:?}", body);

    let (status, _) = post_json(
        &mut app,
        "/api/v1/auth/register",
        json!({
            "username": username,
            "email": format!("{}@test.com", username),
            "password": "password123",
            "display_name": "重复用户"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, body) = post_json(
        &mut app,
        "/api/v1/auth/login",
        json!({ "username": username, "password": "password123" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "登录失败: {:?}", body);
    assert!(body["token"].as_str().unwrap().len() > 20);

    let token = body["token"].as_str().unwrap();
    let claims =
        verify_token(token, "test-secret-for-integration-tests").expect("token 应可验证");
    assert_eq!(claims.username, username);

    let (status, _) = post_json(
        &mut app,
        "/api/v1/auth/login",
        json!({ "username": username, "password": "wrong-password" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_auth_register_missing_fields() {
    let mut app = match create_test_app().await {
        Some(app) => app,
        None => return,
    };
    let (status, _) = post_json(
        &mut app,
        "/api/v1/auth/register",
        json!({ "username": "nopass", "email": "a@b.com", "display_name": "x" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_auth_login_nonexistent_user() {
    let mut app = match create_test_app().await {
        Some(app) => app,
        None => return,
    };
    let (status, _) = post_json(
        &mut app,
        "/api/v1/auth/login",
        json!({ "username": "no_such_user", "password": "x" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// 3. 知识点树 CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_knowledge_points_crud() {
    let mut app = match create_test_app().await {
        Some(app) => app,
        None => return,
    };
    let token = register_and_login(&mut app).await;

    // 获取初始树（可能已有其他测试残留的节点）
    let (status, body) = get_auth(&mut app, "/api/v1/knowledge-points", &token).await;
    assert_eq!(status, StatusCode::OK, "获取知识点树失败: {} {:?}", status, body);
    let tree = body.as_array().unwrap();
    let initial_count = tree.len();

    // 创建根节点
    let (status, body) = post_auth(
        &mut app,
        "/api/v1/knowledge-points",
        json!({ "name": "数与代数", "sort_order": 1 }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let kp_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["name"], "数与代数");

    // 创建子节点
    let (status, body) = post_auth(
        &mut app,
        "/api/v1/knowledge-points",
        json!({ "parent_id": kp_id, "name": "有理数", "sort_order": 1 }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let child_id = body["id"].as_str().unwrap().to_string();

    // 再创建一个根节点
    let (status, body) = post_auth(
        &mut app,
        "/api/v1/knowledge-points",
        json!({ "name": "图形与几何", "sort_order": 2 }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // 获取树 — 应包含初始节点 + 两个新根节点
    let (status, body) = get_auth(&mut app, "/api/v1/knowledge-points", &token).await;
    assert_eq!(status, StatusCode::OK);
    let tree = body.as_array().unwrap();
    assert_eq!(tree.len(), initial_count + 2, "新增了两个根节点");
    // 查找"数与代数"节点验证子节点
    let shu = tree.iter().find(|n| n["name"] == "数与代数").expect("应找到数与代数节点");
    assert_eq!(shu["children"].as_array().unwrap().len(), 1);
    assert_eq!(shu["children"][0]["name"], "有理数");

    // 更新子节点名称
    let (status, body) = put_auth(
        &mut app,
        &format!("/api/v1/knowledge-points/{}", child_id),
        json!({ "name": "有理数（更新）" }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "有理数（更新）");

    // 删除子节点
    let (status, _) =
        delete_auth(&mut app, &format!("/api/v1/knowledge-points/{}", child_id), &token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // 删除根节点（不能有子节点时才能删，现在有 0 个子节点，可以删）
    let (status, _) =
        delete_auth(&mut app, &format!("/api/v1/knowledge-points/{}", kp_id), &token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // 删除不存在的节点
    let (status, _) = delete_auth(
        &mut app,
        &format!("/api/v1/knowledge-points/{}", Uuid::new_v4()),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// 4. 题目完整生命周期
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_question_full_lifecycle() {
    let mut app = match create_test_app().await {
        Some(app) => app,
        None => return,
    };
    let token = register_and_login(&mut app).await;       // 教师用户（创建题目）
    let leader_token = register_leader_and_login(&mut app).await; // 组长用户（审核）

    // 先建一个知识点用于关联
    let (_, kp) = post_auth(
        &mut app,
        "/api/v1/knowledge-points",
        json!({ "name": "测试知识点", "sort_order": 1 }),
        &token,
    )
    .await;
    let kp_id = kp["id"].as_str().unwrap();

    // 1. 创建题目（选择题，草稿状态）
    let (status, body) = post_auth(
        &mut app,
        "/api/v1/questions",
        json!({
            "stem": "1 + 1 = ?",
            "question_type": "choice",
            "difficulty": "easy",
            "default_score": 5,
            "options": [
                {"label": "A", "content": "1"},
                {"label": "B", "content": "2"},
                {"label": "C", "content": "3"},
                {"label": "D", "content": "4"}
            ],
            "correct_answer": ["B"],
            "analysis": "1+1=2",
            "grade": "初一",
            "semester": "上学期",
            "knowledge_point_ids": [kp_id]
        }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "创建题目失败: {:?}", body);
    assert_eq!(body["status"], "draft");
    assert_eq!(body["question_type"], "choice");
    assert_eq!(body["difficulty"], "easy");
    assert_eq!(body["grade"], "初一");
    assert_eq!(body["knowledge_points"].as_array().unwrap().len(), 1);
    assert_eq!(body["version"], 1);

    let question_id = body["id"].as_str().unwrap().to_string();

    // 2. 获取题目详情
    let (status, body) = get_auth(&mut app, &format!("/api/v1/questions/{}", question_id), &token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["stem"], "1 + 1 = ?");
    assert_eq!(body["status"], "draft");

    // 3. 编辑题目
    let (status, body) = put_auth(
        &mut app,
        &format!("/api/v1/questions/{}", question_id),
        json!({ "stem": "1 + 1 = ? (更新版)", "difficulty": "medium" }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "编辑题目失败: {:?}", body);
    assert_eq!(body["stem"], "1 + 1 = ? (更新版)");
    assert_eq!(body["difficulty"], "medium");
    assert_eq!(body["version"], 2); // 版本递增

    // 4. 提交审核
    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/questions/{}/submit", question_id),
        json!({}),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "提交审核失败: {:?}", body);
    assert_eq!(body["status"], "pending");

    // 5. 审核通过（组长操作）
    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/questions/{}/review", question_id),
        json!({ "action": "approved", "comment": "审核通过" }),
        &leader_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "审核通过失败: {:?}", body);
    assert_eq!(body["status"], "published");

    // 6. 已发布题目不可编辑
    let (status, _) = put_auth(
        &mut app,
        &format!("/api/v1/questions/{}", question_id),
        json!({ "stem": "试图修改" }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // 7. 列选题目列表
    let (status, body) = get_auth(&mut app, "/api/v1/questions?status=published", &token).await;
    assert_eq!(status, StatusCode::OK);
    let list = body.as_array().unwrap();
    assert!(!list.is_empty());
    assert_eq!(list[0]["id"], question_id);
}

// ---------------------------------------------------------------------------
// 5. 题目搜索与过滤
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_question_search() {
    let mut app = match create_test_app().await {
        Some(app) => app,
        None => return,
    };
    let token = register_and_login(&mut app).await;

    // 创建三道不同题型的题目
    post_auth(
        &mut app,
        "/api/v1/questions",
        json!({
            "stem": "选择题：2+2=?",
            "question_type": "choice",
            "difficulty": "easy",
            "correct_answer": ["C"],
            "options": [{"label":"A","content":"3"},{"label":"B","content":"4"},{"label":"C","content":"4"},{"label":"D","content":"5"}]
        }),
        &token,
    )
    .await;

    post_auth(
        &mut app,
        "/api/v1/questions",
        json!({
            "stem": "填空题：3+3=____",
            "question_type": "fill",
            "difficulty": "hard",
            "correct_answer": [{"position":1, "answer":"6"}]
        }),
        &token,
    )
    .await;

    post_auth(
        &mut app,
        "/api/v1/questions",
        json!({
            "stem": "解答题：证明1+1=2",
            "question_type": "solution",
            "difficulty": "medium",
            "correct_answer": ["证明略"]
        }),
        &token,
    )
    .await;

    // 按题型过滤
    let (status, body) = get_auth(&mut app, "/api/v1/questions?question_type=choice", &token).await;
    assert_eq!(status, StatusCode::OK);
    let list = body.as_array().unwrap();
    assert!(!list.is_empty(), "应至少有一道选择题");
    assert_eq!(list[0]["question_type"], "choice");

    // 按难度过滤
    let (status, body) = get_auth(&mut app, "/api/v1/questions?difficulty=hard", &token).await;
    assert_eq!(status, StatusCode::OK);
    let list = body.as_array().unwrap();
    assert!(!list.is_empty(), "应至少有一道困难题");
    assert_eq!(list[0]["difficulty"], "hard");

    // 关键词搜索
    let (status, body) = get_auth(&mut app, "/api/v1/questions?keyword=证明", &token).await;
    assert_eq!(status, StatusCode::OK);
    let list = body.as_array().unwrap();
    assert!(!list.is_empty(), "关键词搜索应有结果");
    assert!(list[0]["stem"].as_str().unwrap().contains("证明"));
}

// ---------------------------------------------------------------------------
// 6. 异常场景
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_question_delete_draft_only() {
    let mut app = match create_test_app().await {
        Some(app) => app,
        None => return,
    };
    let token = register_and_login(&mut app).await;

    // 创建题目
    let (_, body) = post_auth(
        &mut app,
        "/api/v1/questions",
        json!({
            "stem": "临时题目",
            "question_type": "judgment",
            "difficulty": "easy",
            "correct_answer": [true]
        }),
        &token,
    )
    .await;
    let qid = body["id"].as_str().unwrap().to_string();

    // 提交审核
    post_auth(&mut app, &format!("/api/v1/questions/{}/submit", qid), json!({}), &token).await;

    // 审核状态不可删除
    let (status, _) = delete_auth(&mut app, &format!("/api/v1/questions/{}", qid), &token).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // 不存在的题目
    let (status, _) =
        delete_auth(&mut app, &format!("/api/v1/questions/{}", Uuid::new_v4()), &token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_question_review_reject() {
    let mut app = match create_test_app().await {
        Some(app) => app,
        None => return,
    };
    let token = register_and_login(&mut app).await;
    let leader_token = register_leader_and_login(&mut app).await;

    // 创建 + 提交
    let (_, body) = post_auth(
        &mut app,
        "/api/v1/questions",
        json!({
            "stem": "驳回测试题",
            "question_type": "choice",
            "difficulty": "easy",
            "correct_answer": ["A"],
            "options": [{"label":"A","content":"OK"},{"label":"B","content":"NO"}]
        }),
        &token,
    )
    .await;
    let qid = body["id"].as_str().unwrap().to_string();
    post_auth(&mut app, &format!("/api/v1/questions/{}/submit", qid), json!({}), &token).await;

    // 审核驳回（组长操作）
    let (status, body) = post_auth(
        &mut app,
        &format!("/api/v1/questions/{}/review", qid),
        json!({ "action": "rejected", "comment": "题干不够清晰" }),
        &leader_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "rejected");

    // 驳回后可重新编辑
    let (status, body) = put_auth(
        &mut app,
        &format!("/api/v1/questions/{}", qid),
        json!({ "stem": "驳回测试题（已修改）" }),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "驳回后应可编辑: {:?}", body);
    assert_eq!(body["status"], "draft"); // 编辑后回到草稿
}

// ---------------------------------------------------------------------------
// 7. 教研组 CRUD + 成员管理
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_groups_crud() {
    let mut app = match create_test_app().await {
        Some(app) => app,
        None => return,
    };
    let token = register_and_login(&mut app).await;

    // 获取初始列表（可能已有其他测试残留）
    let (status, body) = get_auth(&mut app, "/api/v1/groups", &token).await;
    assert_eq!(status, StatusCode::OK);
    let initial_count = body.as_array().unwrap().len();

    // 创建教研组
    let (status, body) = post_auth(
        &mut app,
        "/api/v1/groups",
        json!({"name": "初一数学组", "description": "初一数学教师团队"}),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "创建失败: {:?}", body);
    let group_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["name"], "初一数学组");

    // 创建第二个组
    let (status, _) = post_auth(
        &mut app,
        "/api/v1/groups",
        json!({"name": "初三几何组"}),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // 列表应包含初始 + 2 个新组
    let (status, body) = get_auth(&mut app, "/api/v1/groups", &token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), initial_count + 2);

    // 获取详情
    let (status, body) =
        get_auth(&mut app, &format!("/api/v1/groups/{}", group_id), &token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "初一数学组");
    assert_eq!(body["members"].as_array().unwrap().len(), 0);

    // 更新组名
    let (status, body) = put_auth(
        &mut app,
        &format!("/api/v1/groups/{}", group_id),
        json!({"name": "初一数学组（更新）"}),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "初一数学组（更新）");

    // 删除组
    let (status, _) =
        delete_auth(&mut app, &format!("/api/v1/groups/{}", group_id), &token).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) =
        get_auth(&mut app, &format!("/api/v1/groups/{}", group_id), &token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 删除不存在的组
    let (status, _) =
        delete_auth(&mut app, &format!("/api/v1/groups/{}", Uuid::new_v4()), &token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_group_members() {
    let mut app = match create_test_app().await {
        Some(app) => app,
        None => return,
    };

    // 注册两个用户
    let token1 = register_and_login(&mut app).await;
    let token2 = register_and_login(&mut app).await;

    // 获取两个用户的 ID（从登录响应中提取）
    // 由于 login 不返回 user_id，我们创建一个组来测试成员管理流程

    // 创建组
    let (_, body) = post_auth(
        &mut app,
        "/api/v1/groups",
        json!({"name": "测试组"}),
        &token1,
    )
    .await;
    let group_id = body["id"].as_str().unwrap().to_string();

    // 从 token1 中解码获取 user_id
    let claims = mathset::auth::jwt::verify_token(
        &token1,
        "test-secret-for-integration-tests",
    )
    .expect("token 应可解码");
    let my_user_id = claims.sub;

    // 添加自己为成员
    let (status, _) = post_auth(
        &mut app,
        &format!("/api/v1/groups/{}/members", group_id),
        json!({"user_id": my_user_id, "is_leader": true}),
        &token1,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "添加成员失败");

    // 查看组成员
    let (status, body) =
        get_auth(&mut app, &format!("/api/v1/groups/{}", group_id), &token1).await;
    assert_eq!(status, StatusCode::OK);
    let members = body["members"].as_array().unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["is_leader"], true);

    // 重复添加（幂等）
    let (status, _) = post_auth(
        &mut app,
        &format!("/api/v1/groups/{}/members", group_id),
        json!({"user_id": my_user_id, "is_leader": true}),
        &token1,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // 取消组长
    let (status, _) = put_auth(
        &mut app,
        &format!("/api/v1/groups/{}/members/{}", group_id, my_user_id),
        json!({"is_leader": false}),
        &token1,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) =
        get_auth(&mut app, &format!("/api/v1/groups/{}", group_id), &token1).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["members"][0]["is_leader"], false);

    // 移除成员
    let (status, _) = delete_auth(
        &mut app,
        &format!("/api/v1/groups/{}/members/{}", group_id, my_user_id),
        &token1,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) =
        get_auth(&mut app, &format!("/api/v1/groups/{}", group_id), &token1).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["members"].as_array().unwrap().len(), 0);

    // 移除不存在的成员
    let (status, _) = delete_auth(
        &mut app,
        &format!("/api/v1/groups/{}/members/{}", group_id, Uuid::new_v4()),
        &token1,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
