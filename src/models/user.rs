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

/// 每日 AI 解析任务额度（单一计量源：ai_usage_log，见 ai_tasks.rs 原子抢占）
pub const DAILY_TASK_QUOTA: i64 = 50;

/// 用户配额查询响应
///
/// 配额统一以 ai_usage_log 为单一计量源：
/// - used_today = 当日（UTC）ai_usage_log 记录数，与任务创建时的抢占校验口径一致
/// - reset_at = 次日 UTC 零点（CURRENT_DATE 跨日自动重置）
#[derive(Debug, Serialize)]
pub struct UserQuota {
    pub daily_quota: i64,
    pub used_today: i64,
    pub remaining: i64,
    pub reset_at: DateTime<Utc>,
}

impl UserQuota {
    /// 查询用户当日配额使用情况（ai_usage_log 单一计量）
    pub async fn today(
        pool: &sqlx::PgPool,
        user_id: Uuid,
    ) -> Result<Self, sqlx::Error> {
        let used_today: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM ai_usage_log WHERE user_id = $1 AND created_at >= CURRENT_DATE",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        let tomorrow = (chrono::Utc::now().date_naive() + chrono::Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .expect("次日零点必然合法");
        Ok(Self {
            daily_quota: DAILY_TASK_QUOTA,
            used_today,
            remaining: (DAILY_TASK_QUOTA - used_today).max(0),
            reset_at: chrono::DateTime::from_naive_utc_and_offset(tomorrow, Utc),
        })
    }
}
