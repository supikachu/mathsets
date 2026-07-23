use axum::{
    body::Body,
    extract::State,
    http::Request,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use uuid::Uuid;

use crate::auth::jwt::verify_token;
use crate::AppState;

/// 认证用户信息（由中间件注入到请求扩展）
///
/// 双轨制角色：
/// - `role`：旧枚举字符串（"admin" / "user"），兼容 `is_admin()`
/// - `global_role`：新枚举字符串（"super_admin" / "teacher"），供 `is_super_admin()` 判定
///
/// 业务层推荐使用 `is_admin_user(&auth_user)` 做统一管理员判定，
/// 自动覆盖两条轨道，避免双轨不一致导致权限丢失。
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: Uuid,
    pub username: String,
    pub role: String,
    pub global_role: String,
}

/// JWT 认证中间件 — 要求请求携带有效的 Bearer Token
pub async fn require_auth(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    // 1. 提取 Authorization 头
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                axum::http::StatusCode::UNAUTHORIZED,
                Json(json!({"error": "缺少 Authorization 头"})),
            )
                .into_response()
        })?;

    // 2. 解析 Bearer token
    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| {
            (
                axum::http::StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Authorization 格式必须是 Bearer <token>"})),
            )
                .into_response()
        })?;

    // 3. 验证 JWT — 用 Debug format 暴露完整错误链，避免底层错误被吞噬
    let claims = verify_token(token, &state.jwt_secret).map_err(|e| {
        tracing::warn!(
            "JWT 验证失败, token_prefix='{}...', error={:?}",
            token.chars().take(20).collect::<String>(),
            e
        );
        (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Token 无效或已过期"})),
        )
            .into_response()
    })?;

    // 4. 注入 AuthUser 到请求扩展（同时携带新旧角色）
    let auth_user = AuthUser {
        id: claims.sub,
        username: claims.username,
        role: claims.role,
        global_role: claims.global_role,
    };
    req.extensions_mut().insert(auth_user);

    // 5. 继续处理请求
    Ok(next.run(req).await)
}
