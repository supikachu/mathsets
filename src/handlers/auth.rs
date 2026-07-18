use axum::{extract::Extension, extract::State, http::StatusCode, Json};
use serde_json::json;
use uuid::Uuid;

use crate::auth::jwt::create_token;
use crate::auth::middleware::AuthUser;
use crate::models::user::{LoginRequest, LoginResponse, RegisterRequest, User, UserPublic};
use crate::AppState;

/// 复制全局知识点树（space_id = NULL）到新用户的个人空间
/// 递归复制，保持 parent_id 关系
async fn copy_default_knowledge_tree(
    pool: &sqlx::PgPool,
    target_space_id: Uuid,
) -> Result<(), sqlx::Error> {
    // 查询所有全局知识点（space_id IS NULL）
    let global_kps = sqlx::query_as::<_, (Uuid, Option<Uuid>, String, Option<String>, i32)>(
        "SELECT id, parent_id, name, grade, sort_order FROM knowledge_points WHERE space_id IS NULL ORDER BY sort_order, name",
    )
    .fetch_all(pool)
    .await?;

    if global_kps.is_empty() {
        return Ok(());
    }

    let now = chrono::Utc::now();

    // 建立 旧ID -> 新ID 的映射
    let mut id_map: std::collections::HashMap<Uuid, Uuid> = std::collections::HashMap::new();
    for (old_id, _, _, _, _) in &global_kps {
        id_map.insert(*old_id, Uuid::new_v4());
    }

    // 逐条插入（parent_id 使用映射后的新 ID）
    for (old_id, old_parent, name, grade, sort_order) in &global_kps {
        let new_id = id_map[old_id];
        let new_parent = old_parent.map(|p| id_map[&p]);
        sqlx::query(
            r#"
            INSERT INTO knowledge_points (id, parent_id, name, grade, sort_order, created_at, space_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(new_id)
        .bind(new_parent)
        .bind(name)
        .bind(grade)
        .bind(sort_order)
        .bind(now)
        .bind(target_space_id)
        .execute(pool)
        .await?;
    }

    println!(
        "[register] 已为空间 {} 复制 {} 个默认知识点",
        target_space_id,
        global_kps.len()
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
        username: req.username,
        display_name: req.display_name,
        role: crate::models::user::UserRole::User,
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

    // 签发 JWT（序列化为 Admin / User）
    let role_str = serde_json::to_value(&user.role)
        .map(|v| v.as_str().unwrap_or("User").to_string())
        .unwrap_or_else(|_| "User".to_string());

    let token = create_token(user.id, &user.username, &role_str, &state.jwt_secret, 24).map_err(|e| {
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
        }),
    ))
}

/// GET /api/v1/admin/users — 管理员查看用户列表
pub async fn list_users(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Json<Vec<UserPublic>>, (StatusCode, Json<serde_json::Value>)> {
    if !crate::auth::permissions::is_admin(&auth_user.role) {
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
