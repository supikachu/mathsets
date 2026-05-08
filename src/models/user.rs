use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 用户角色
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "user_role", rename_all = "lowercase")]
pub enum UserRole {
    Admin,
    GroupLeader,
    Teacher,
    Viewer,
}

/// 用户
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub display_name: String,
    pub role: UserRole,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 注册请求
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    pub display_name: String,
}

/// 登录请求
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// 登录响应
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user_id: Uuid,
    pub display_name: String,
    pub role: UserRole,
}

/// 公开用户信息（返回时不暴露密码等敏感字段）
#[derive(Debug, Serialize)]
pub struct UserPublic {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub role: UserRole,
    pub created_at: DateTime<Utc>,
}

impl From<User> for UserPublic {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            username: u.username,
            display_name: u.display_name,
            role: u.role,
            created_at: u.created_at,
        }
    }
}
