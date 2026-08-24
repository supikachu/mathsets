use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::jwt::create_token;
use crate::auth::middleware::AuthUser;
use crate::auth::permissions::is_admin_user;
use crate::models::user::{
    GlobalRole, LoginRequest, LoginResponse, RegisterRequest, User, UserRole, UserPublic,
};
use crate::AppState;

/// 复制全局知识树（space_id = NULL 的 trees + nodes）到新用户的个人空间
///
/// 旧实现读写已废弃的 knowledge_points 表（20260721000001 迁移已将其
/// RENAME 为 knowledge_points_deprecated，数据迁入 knowledge_nodes），
/// 自该迁移起本函数一直静默失败。现按新 schema 重写：
/// - tree：code 在个人空间内唯一（部分唯一索引允许与全局同 code），整树复制
/// - node：path/depth 由 code 链构成、与 id 无关，层级不变则直接沿用；
///   question_count 清 0（个人空间计数独立于全局树）
/// - 递归保持 parent_id 关系（父节点先插入）
async fn copy_default_knowledge_tree(
    pool: &sqlx::PgPool,
    target_space_id: Uuid,
) -> Result<(), sqlx::Error> {
    // 1. 复制全局树（space_id IS NULL）
    let global_trees = sqlx::query_as::<_, (Uuid, String, String, String, Option<String>)>(
        "SELECT id, code, name, kind::text, description FROM knowledge_trees WHERE space_id IS NULL AND is_active = TRUE",
    )
    .fetch_all(pool)
    .await?;

    if global_trees.is_empty() {
        return Ok(());
    }

    let now = chrono::Utc::now();
    let mut tree_map: std::collections::HashMap<Uuid, Uuid> = std::collections::HashMap::new();
    for (old_id, code, name, kind, description) in &global_trees {
        let new_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO knowledge_trees (id, code, name, kind, space_id, description, is_active, created_at, updated_at)
            VALUES ($1, $2, $3, $4::knowledge_tree_kind, $5, $6, TRUE, $7, $7)
            "#,
        )
        .bind(new_id)
        .bind(code)
        .bind(name)
        .bind(kind)
        .bind(target_space_id)
        .bind(description)
        .bind(now)
        .execute(pool)
        .await?;
        tree_map.insert(*old_id, new_id);
    }

    // 2. 复制全局节点（按 depth 排序保证父节点先于子节点插入）
    // 注：knowledge_nodes 无 space_id 列，空间隔离经 tree_id → knowledge_trees.space_id
    let global_nodes = sqlx::query_as::<_, (Uuid, Option<Uuid>, Uuid)>(
        r#"
        SELECT n.id, n.parent_id, n.tree_id
        FROM knowledge_nodes n
        JOIN knowledge_trees t ON t.id = n.tree_id
        WHERE t.space_id IS NULL
          AND n.is_active = TRUE
        ORDER BY n.depth, n.sort_order
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut node_map: std::collections::HashMap<Uuid, Uuid> = std::collections::HashMap::new();
    for (old_id, old_parent, old_tree) in &global_nodes {
        // 节点的 tree 不在全局树集合内（数据异常）则跳过，避免外键错误阻断注册
        let Some(new_tree) = tree_map.get(old_tree) else {
            continue;
        };
        let new_id = Uuid::new_v4();
        let new_parent = old_parent.and_then(|p| node_map.get(&p).copied());
        // path/depth 由 code 链构成、与 id 无关，层级不变直接由源行复制
        sqlx::query(
            r#"
            INSERT INTO knowledge_nodes
              (id, tree_id, parent_id, code, path, depth, name, aliases, description,
               sort_order, question_count, is_active, created_at, updated_at)
            SELECT $1, $2, $3, src.code, src.path, src.depth, src.name, src.aliases,
                   src.description, src.sort_order, 0, src.is_active, $4, $4
            FROM knowledge_nodes src WHERE src.id = $5
            "#,
        )
        .bind(new_id)
        .bind(new_tree)
        .bind(new_parent)
        .bind(now)
        .bind(old_id)
        .execute(pool)
        .await?;
        node_map.insert(*old_id, new_id);
    }

    println!(
        "[register] 已为空间 {} 复制 {} 棵全局树 / {} 个知识点",
        target_space_id,
        global_trees.len(),
        global_nodes.len()
    );
    Ok(())
}

/// POST /api/v1/auth/register
pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<UserPublic>), (StatusCode, Json<serde_json::Value>)> {
    // 检查用户名是否已存在
    let existing = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE username = $1",
    )
    .bind(&req.username)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("数据库错误: {}", e)})),
        )
    })?;

    if existing > 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "用户名已存在"})),
        ));
    }

    // 密码哈希 — 卸载到 blocking 线程池，防止阻塞 Tokio 工作线程
    let password_plain = req.password.clone();
    let password_hash = tokio::task::spawn_blocking(move || {
        bcrypt::hash(&password_plain, bcrypt::DEFAULT_COST)
    })
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("系统任务调度失败: {}", e)})),
        )
    })?
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("密码加密失败: {}", e)})),
        )
    })?;

    let user_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    // 插入新用户 + 个人空间（事务）
    let mut tx = state.pool.begin().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("开启事务失败: {}", e)})),
        )
    })?;

    sqlx::query(
        r#"
        INSERT INTO users (id, username, email, password_hash, display_name, role, is_active, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(user_id)
    .bind(&req.username)
    .bind(&req.email)
    .bind(&password_hash)
    .bind(&req.display_name)
    .bind(crate::models::user::UserRole::User)
    .bind(true)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("创建用户失败: {}", e)})),
        )
    })?;

    let space_id = Uuid::new_v4();
    let space_name = format!("{} 的题库", req.display_name);
    let settings = serde_json::json!({
        "allow_creator_self_review": true,
        "require_review_duty": false
    });
    sqlx::query(
        r#"
        INSERT INTO spaces (id, kind, name, owner_user_id, settings, created_at, updated_at)
        VALUES ($1, 'personal', $2, $3, $4, $5, $6)
        "#,
    )
    .bind(space_id)
    .bind(&space_name)
    .bind(user_id)
    .bind(&settings)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("创建个人空间失败: {}", e)})),
        )
    })?;

    tx.commit().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("提交事务失败: {}", e)})),
        )
    })?;

    // 为新用户复制默认知识点树（visualtest 创建的全局知识点）
    // 复制到用户的个人空间，使其可自定义
    if let Err(e) = copy_default_knowledge_tree(&state.pool, space_id).await {
        eprintln!("[register] 复制默认知识点树失败: {}", e);
        // 不阻断注册流程，用户仍可手动添加知识点
    }

    let user_public = UserPublic {
        id: user_id,
        username: req.username.clone(),
        email: req.email.clone(),
        display_name: req.display_name.clone(),
        role: crate::models::user::UserRole::User,
        global_role: crate::models::user::GlobalRole::Teacher,
        avatar_url: None,
        is_active: true,
        created_at: now,
    };

    Ok((StatusCode::CREATED, Json(user_public)))
}

/// POST /api/v1/auth/login
pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<(StatusCode, Json<LoginResponse>), (StatusCode, Json<serde_json::Value>)> {
    println!("[backend/login] received username: {:?}, password length: {}", req.username, req.password.len());

    // 查找用户
    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE username = $1 AND is_active = true",
    )
    .bind(&req.username)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("数据库错误: {}", e)})),
        )
    })?
    .ok_or_else(|| {
        println!("[backend/login] user not found: {:?}", req.username);
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "用户名或密码错误"})),
        )
    })?;

    println!("[backend/login] user found: {:?}, verifying password", user.username);
    // 验证密码 — 卸载到 blocking 线程池，防止阻塞 Tokio 工作线程
    let password_plain = req.password.clone();
    let password_hash_stored = user.password_hash.clone();
    let valid = tokio::task::spawn_blocking(move || {
        bcrypt::verify(&password_plain, &password_hash_stored).unwrap_or(false)
    })
    .await
    .unwrap_or(false);

    if !valid {
        println!("[backend/login] password verification failed for user: {:?}", req.username);
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "用户名或密码错误"})),
        ));
    }
    println!("[backend/login] password verified, issuing token");

    // 签发 JWT（双轨角色同时打入 Token）
    // 旧 role：序列化为 "admin" / "user"
    let role_str = serde_json::to_value(&user.role)
        .map(|v| v.as_str().unwrap_or("User").to_string())
        .unwrap_or_else(|_| "User".to_string());
    // 新 global_role：序列化为 "super_admin" / "teacher"
    let global_role_str = serde_json::to_value(&user.global_role)
        .map(|v| v.as_str().unwrap_or("teacher").to_string())
        .unwrap_or_else(|_| "teacher".to_string());

    let token = create_token(
        user.id,
        &user.username,
        &role_str,
        &global_role_str,
        &state.jwt_secret,
        24,
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("Token 签发失败: {}", e)})),
        )
    })?;

    Ok((
        StatusCode::OK,
        Json(LoginResponse {
            token,
            user_id: user.id,
            display_name: user.display_name,
            role: user.role,
            global_role: user.global_role,
            avatar_url: user.avatar_url,
        }),
    ))
}

/// GET /api/v1/admin/users — 管理员查看用户列表
pub async fn list_users(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Json<Vec<UserPublic>>, (StatusCode, Json<serde_json::Value>)> {
    if !is_admin_user(&auth_user) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权操作"}))));
    }

    let users = sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY created_at DESC")
        .fetch_all(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询用户失败: {}", e)})),
            )
        })?;

    Ok(Json(users.into_iter().map(|u| u.into()).collect()))
}

/// GET /api/v1/auth/me — 获取当前登录用户信息
pub async fn me(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Json<UserPublic>, (StatusCode, Json<serde_json::Value>)> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1 AND is_active = true")
        .bind(auth_user.id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("数据库错误: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "用户不存在"}))))?;

    Ok(Json(UserPublic::from(user)))
}

// ===========================================================================
// 管理员用户管理 API（阶段三新增）
// ===========================================================================

/// 创建新用户请求
#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    pub display_name: String,
    /// 初始全局角色（默认 teacher）
    pub global_role: Option<GlobalRole>,
}

/// 修改用户全局角色请求
#[derive(Debug, Deserialize)]
pub struct UpdateUserRoleRequest {
    pub global_role: GlobalRole,
}

/// 修改用户状态请求
#[derive(Debug, Deserialize)]
pub struct UpdateUserStatusRequest {
    pub is_active: bool,
}

/// 将 GlobalRole 映射为兼容的旧 UserRole
///
/// 双轨制同步：super_admin → admin，teacher → user
/// 保证 `is_admin()` 旧轨道判定与 `is_super_admin()` 新轨道一致
fn global_role_to_legacy(gr: &GlobalRole) -> UserRole {
    match gr {
        GlobalRole::SuperAdmin => UserRole::Admin,
        GlobalRole::Teacher => UserRole::User,
    }
}

/// POST /api/v1/admin/users — 管理员创建新用户
///
/// 与 /auth/register 的差异：
/// - 由管理员调用，可指定初始 global_role（register 固定为 teacher）
/// - 不需要邮箱验证流程
/// - 自动同步新旧角色双轨
pub async fn create_user(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserPublic>), (StatusCode, Json<serde_json::Value>)> {
    if !is_admin_user(&auth_user) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "仅管理员可创建用户"})),
        ));
    }

    let global_role = req.global_role.unwrap_or(GlobalRole::Teacher);
    let legacy_role = global_role_to_legacy(&global_role);

    // 1. 用户名查重
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE username = $1")
        .bind(&req.username)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("数据库错误: {}", e)})),
            )
        })?;
    if existing > 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "用户名已存在"})),
        ));
    }

    // 2. 邮箱查重
    let existing_email: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = $1")
        .bind(&req.email)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("数据库错误: {}", e)})),
            )
        })?;
    if existing_email > 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "邮箱已被使用"})),
        ));
    }

    // 3. 密码哈希 — 卸载到 blocking 线程池
    let password_plain = req.password.clone();
    let password_hash = tokio::task::spawn_blocking(move || {
        bcrypt::hash(&password_plain, bcrypt::DEFAULT_COST)
    })
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("系统任务调度失败: {}", e)})),
        )
    })?
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("密码加密失败: {}", e)})),
        )
    })?;

    let user_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    // 4. 插入新用户 + 个人空间（事务）
    let mut tx = state.pool.begin().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("开启事务失败: {}", e)})),
        )
    })?;

    sqlx::query(
        r#"
        INSERT INTO users (id, username, email, password_hash, display_name, role, global_role, is_active, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, TRUE, $8, $9)
        "#,
    )
    .bind(user_id)
    .bind(&req.username)
    .bind(&req.email)
    .bind(&password_hash)
    .bind(&req.display_name)
    .bind(legacy_role.clone())
    .bind(global_role.clone())
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("创建用户失败: {}", e)})),
        )
    })?;

    // 5. 创建个人空间
    let space_id = Uuid::new_v4();
    let space_name = format!("{} 的题库", req.display_name);
    let settings = serde_json::json!({
        "allow_creator_self_review": true,
        "require_review_duty": false
    });
    sqlx::query(
        r#"
        INSERT INTO spaces (id, kind, name, owner_user_id, settings, created_at, updated_at)
        VALUES ($1, 'personal', $2, $3, $4, $5, $6)
        "#,
    )
    .bind(space_id)
    .bind(&space_name)
    .bind(user_id)
    .bind(&settings)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("创建个人空间失败: {}", e)})),
        )
    })?;

    tx.commit().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("提交事务失败: {}", e)})),
        )
    })?;

    // 6. 复制默认知识树（失败不阻断）
    if let Err(e) = copy_default_knowledge_tree(&state.pool, space_id).await {
        eprintln!("[admin/create_user] 复制默认知识点树失败: {}", e);
    }

    let user_public = UserPublic {
        id: user_id,
        username: req.username.clone(),
        email: req.email.clone(),
        display_name: req.display_name.clone(),
        role: legacy_role,
        global_role,
        avatar_url: None,
        is_active: true,
        created_at: now,
    };

    Ok((StatusCode::CREATED, Json(user_public)))
}

/// PUT /api/v1/admin/users/{id}/role — 修改用户全局角色
///
/// 双轨同步：同时更新 global_role 和旧 role 字段，保证 is_admin / is_super_admin 判定一致
pub async fn update_user_role(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(target_id): Path<Uuid>,
    Json(req): Json<UpdateUserRoleRequest>,
) -> Result<Json<UserPublic>, (StatusCode, Json<serde_json::Value>)> {
    if !is_admin_user(&auth_user) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "仅管理员可修改用户角色"})),
        ));
    }

    let legacy_role = global_role_to_legacy(&req.global_role);

    let user = sqlx::query_as::<_, User>(
        r#"
        UPDATE users
        SET global_role = $1,
            role        = $2,
            updated_at  = NOW()
        WHERE id = $3
        RETURNING *
        "#,
    )
    .bind(req.global_role)
    .bind(legacy_role)
    .bind(target_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("更新用户角色失败: {}", e)})),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "目标用户不存在"})),
        )
    })?;

    Ok(Json(UserPublic::from(user)))
}

/// GET /api/v1/admin/users/{id} — 获取单个用户详情
///
/// 仅管理员可调用；用于前端"查看用户信息"弹窗
pub async fn get_user(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(target_id): Path<Uuid>,
) -> Result<Json<UserPublic>, (StatusCode, Json<serde_json::Value>)> {
    if !is_admin_user(&auth_user) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "仅管理员可查看用户详情"})),
        ));
    }

    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(target_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("数据库错误: {}", e)})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "用户不存在"})),
            )
        })?;

    Ok(Json(UserPublic::from(user)))
}

/// DELETE /api/v1/admin/users/{id} — 删除用户
///
/// 安全约束：
/// 1. 仅管理员可调用
/// 2. **绝对禁止管理员删除自己**（target_id == auth_user.id → 403）
///
/// 资产继承策略（而非 SET NULL）：
/// 删除用户时，其名下的核心业务资产（题目、审核记录、版本历史、试卷）
/// 所有权全部转移给执行删除操作的管理员（`auth_user.id`），确保数据可追溯。
/// - 转移至管理员：questions.creator_id / questions.updated_by
///                  / review_records.reviewer_id / question_versions.created_by
///                  / papers.creator_id
/// - 删除：ai_usage_log（纯遥测日志，无业务价值）
/// - 自动处理（DB 级约束）：
///   * CASCADE：space_members / question_reviewers / groups / ai_parse_tasks / ai_settings
///   * SET NULL：spaces.owner_user_id / question_reviewers.assigned_by
pub async fn delete_user(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(target_id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    if !is_admin_user(&auth_user) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "仅管理员可删除用户"})),
        ));
    }

    // 安全红线：禁止管理员删除自己
    if target_id == auth_user.id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "不允许删除自己的账号"})),
        ));
    }

    let admin_id = auth_user.id;

    // 确认目标用户存在
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
        .bind(target_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("数据库错误: {}", e)})),
            )
        })?;
    if !exists {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "目标用户不存在"})),
        ));
    }

    // 事务：资产转移 → 清理遥测 → 删除用户
    let mut tx = state.pool.begin().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("开启事务失败: {}", e)})),
        )
    })?;

    // 1. 转移题目归属（creator_id NOT NULL，必须转移而非置空）
    let q_creator = sqlx::query("UPDATE questions SET creator_id = $1 WHERE creator_id = $2")
        .bind(admin_id)
        .bind(target_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("转移题目创建者失败: {}", e)})),
            )
        })?;
    let q_updater = sqlx::query("UPDATE questions SET updated_by = $1 WHERE updated_by = $2")
        .bind(admin_id)
        .bind(target_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("转移题目更新者失败: {}", e)})),
            )
        })?;

    // 2. 转移审核记录归属（reviewer_id NOT NULL）
    let r_reviewer =
        sqlx::query("UPDATE review_records SET reviewer_id = $1 WHERE reviewer_id = $2")
            .bind(admin_id)
            .bind(target_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": format!("转移审核记录失败: {}", e)})),
                )
            })?;

    // 3. 转移版本历史归属
    let v_creator =
        sqlx::query("UPDATE question_versions SET created_by = $1 WHERE created_by = $2")
            .bind(admin_id)
            .bind(target_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": format!("转移版本历史失败: {}", e)})),
                )
            })?;

    // 4. 转移试卷归属
    let p_creator = sqlx::query("UPDATE papers SET creator_id = $1 WHERE creator_id = $2")
        .bind(admin_id)
        .bind(target_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("转移试卷归属失败: {}", e)})),
            )
        })?;

    println!(
        "[delete_user] 资产转移至 {}: 题目创建者 {} 行, 更新者 {} 行, 审核记录 {} 行, 版本历史 {} 行, 试卷 {} 行",
        admin_id,
        q_creator.rows_affected(),
        q_updater.rows_affected(),
        r_reviewer.rows_affected(),
        v_creator.rows_affected(),
        p_creator.rows_affected(),
    );

    // 5. 清理 AI 用量日志（纯遥测，无业务价值，不转移）
    sqlx::query("DELETE FROM ai_usage_log WHERE user_id = $1")
        .bind(target_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("清理 AI 用量日志失败: {}", e)})),
            )
        })?;

    // 6. 删除用户（CASCADE 自动清理 space_members / question_reviewers / groups
    //    / ai_parse_tasks / ai_settings；SET NULL 自动处理 spaces.owner_user_id
    //    / question_reviewers.assigned_by）
    let deleted = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(target_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("删除用户失败: {}", e)})),
            )
        })?;
    if deleted.rows_affected() == 0 {
        // 极端情况：事务内并发删除
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "目标用户已被删除"})),
        ));
    }

    tx.commit().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("提交事务失败: {}", e)})),
        )
    })?;

    Ok(StatusCode::NO_CONTENT)
}

/// PUT /api/v1/admin/users/{id}/status — 启用/禁用用户
///
/// 安全约束：
/// - 不允许管理员禁用自己（避免误锁）
/// - 禁用后用户无法登录（login 查询含 is_active = true 条件）
pub async fn update_user_status(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(target_id): Path<Uuid>,
    Json(req): Json<UpdateUserStatusRequest>,
) -> Result<Json<UserPublic>, (StatusCode, Json<serde_json::Value>)> {
    if !is_admin_user(&auth_user) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "仅管理员可修改用户状态"})),
        ));
    }

    // 防止管理员禁用自己
    if !req.is_active && target_id == auth_user.id {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "不允许禁用自己的账号"})),
        ));
    }

    let user = sqlx::query_as::<_, User>(
        r#"
        UPDATE users
        SET is_active  = $1,
            updated_at = NOW()
        WHERE id = $2
        RETURNING *
        "#,
    )
    .bind(req.is_active)
    .bind(target_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("更新用户状态失败: {}", e)})),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "目标用户不存在"})),
        )
    })?;

    Ok(Json(UserPublic::from(user)))
}
