use axum::{extract::State, http::StatusCode, Json};
use serde_json::json;
use uuid::Uuid;

use crate::auth::jwt::create_token;
use crate::models::user::{LoginRequest, LoginResponse, RegisterRequest, User, UserPublic};
use crate::AppState;

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

    // 密码哈希
    let password_hash = bcrypt::hash(&req.password, bcrypt::DEFAULT_COST).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("密码加密失败: {}", e)})),
        )
    })?;

    let user_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    // 插入新用户
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
    .bind(crate::models::user::UserRole::Teacher) // 默认角色为教师
    .bind(true)
    .bind(now)
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("创建用户失败: {}", e)})),
        )
    })?;

    let user_public = UserPublic {
        id: user_id,
        username: req.username,
        display_name: req.display_name,
        role: crate::models::user::UserRole::Teacher,
        created_at: now,
    };

    Ok((StatusCode::CREATED, Json(user_public)))
}

/// POST /api/v1/auth/login
pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<(StatusCode, Json<LoginResponse>), (StatusCode, Json<serde_json::Value>)> {
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
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "用户名或密码错误"})),
        )
    })?;

    // 验证密码
    let valid = bcrypt::verify(&req.password, &user.password_hash).unwrap_or(false);
    if !valid {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "用户名或密码错误"})),
        ));
    }

    // 签发 JWT
    let role_str = serde_json::to_value(&user.role)
        .map(|v| v.as_str().unwrap_or("teacher").to_string())
        .unwrap_or_else(|_| "teacher".to_string());

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
