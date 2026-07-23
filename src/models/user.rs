use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// 枚举
// ---------------------------------------------------------------------------

/// 全局系统角色（双轨制：全局身份，与空间角色独立）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "global_role", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum GlobalRole {
    SuperAdmin,
    Teacher,
}

/// 系统级角色（旧枚举 — 兼容期保留，后续版本 DROP）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "user_role", rename_all = "lowercase")]
pub enum UserRole {
    Admin,
    User,
}

// ---------------------------------------------------------------------------
// 用户
// ---------------------------------------------------------------------------

/// 用户（数据库行）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub display_name: String,

    /// 旧全局角色（兼容期保留，用于 JWT 签发与 is_admin 判断）
    pub role: UserRole,
    /// 新全局角色（双轨制身份）
    pub global_role: GlobalRole,

    pub is_active: bool,

    // ── 用户头像 ──
    pub avatar_url: Option<String>,

    // ── OCR / AI 额度 ──
    /// 每日 OCR 免费额度
    pub ocr_quota_daily: i32,
    /// 今日已使用 OCR 次数
    pub ocr_quota_used: i32,
    /// 额度重置时间
    pub ocr_quota_reset_at: DateTime<Utc>,
    /// AI 解析 Token 剩余额度
    pub ai_token_quota: i32,

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
    pub global_role: GlobalRole,
    pub avatar_url: Option<String>,
}

/// 公开用户信息（返回时不暴露密码等敏感字段）
#[derive(Debug, Serialize)]
pub struct UserPublic {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub display_name: String,
    pub role: UserRole,
    pub global_role: GlobalRole,
    pub avatar_url: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

impl From<User> for UserPublic {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            username: u.username,
            email: u.email,
            display_name: u.display_name,
            role: u.role,
            global_role: u.global_role,
            avatar_url: u.avatar_url,
            is_active: u.is_active,
            created_at: u.created_at,
        }
    }
}

/// 用户个人资料更新请求
#[derive(Debug, Deserialize)]
pub struct UpdateProfileRequest {
    pub display_name: Option<String>,
    pub email: Option<String>,
}

/// 修改密码请求
#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

/// 用户配额查询响应
#[derive(Debug, Serialize)]
pub struct UserQuota {
    pub ocr_quota_daily: i32,
    pub ocr_quota_used: i32,
    pub ocr_quota_remaining: i32,
    pub ocr_quota_reset_at: DateTime<Utc>,
    pub ai_token_quota: i32,
}

impl From<&User> for UserQuota {
    fn from(u: &User) -> Self {
        Self {
            ocr_quota_daily: u.ocr_quota_daily,
            ocr_quota_used: u.ocr_quota_used,
            ocr_quota_remaining: (u.ocr_quota_daily - u.ocr_quota_used).max(0),
            ocr_quota_reset_at: u.ocr_quota_reset_at,
            ai_token_quota: u.ai_token_quota,
        }
    }
}
