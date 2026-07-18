use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// 枚举
// ---------------------------------------------------------------------------

/// 空间类型
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "space_kind", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum SpaceKind {
    Personal,
    Team,
    Public,
}

/// 空间上下文角色（与全局 GlobalRole 独立）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "space_role", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum SpaceRole {
    Owner,
    Editor,
    Reviewer,
    Viewer,
}

impl SpaceRole {
    /// 是否有录入/编辑题目的权限
    pub fn can_write(&self) -> bool {
        matches!(self, SpaceRole::Owner | SpaceRole::Editor | SpaceRole::Reviewer)
    }

    /// 是否有审核发布的权限
    pub fn can_review(&self) -> bool {
        matches!(self, SpaceRole::Owner | SpaceRole::Reviewer)
    }

    /// 是否为空间管理员
    pub fn is_owner(&self) -> bool {
        matches!(self, SpaceRole::Owner)
    }
}

/// 空间默认设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceSettings {
    /// 个人空间：允许创建者自审自发
    #[serde(default = "default_true")]
    pub allow_creator_self_review: bool,
    /// 团队空间：需要具有 review 职责才能审核
    #[serde(default)]
    pub require_review_duty: bool,
    /// 团队空间：强制录审分离（Maker-Checker Rule）
    #[serde(default = "default_true")]
    pub enforce_maker_checker: bool,
}

fn default_true() -> bool {
    true
}

impl Default for SpaceSettings {
    fn default() -> Self {
        Self {
            allow_creator_self_review: true,
            require_review_duty: false,
            enforce_maker_checker: true,
        }
    }
}

// ---------------------------------------------------------------------------
// 空间
// ---------------------------------------------------------------------------

/// 空间（数据库行）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Space {
    pub id: Uuid,
    pub kind: SpaceKind,
    pub name: String,
    pub owner_user_id: Option<Uuid>,
    pub settings: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Space {
    pub fn settings_parsed(&self) -> SpaceSettings {
        serde_json::from_value(self.settings.clone()).unwrap_or_default()
    }
}

/// 空间成员（数据库行）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SpaceMember {
    pub space_id: Uuid,
    pub user_id: Uuid,
    pub role: SpaceRole,
    pub duties: Vec<String>,
    pub joined_at: DateTime<Utc>,
}

/// 空间列表项
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SpaceSummary {
    pub id: Uuid,
    pub kind: SpaceKind,
    pub name: String,
    pub owner_user_id: Option<Uuid>,
    pub member_count: Option<i64>,
    pub my_role: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// 空间成员信息（含用户展示字段）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SpaceMemberInfo {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub duties: Vec<String>,
    pub joined_at: DateTime<Utc>,
}

/// 空间详情
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceDetail {
    pub id: Uuid,
    pub kind: SpaceKind,
    pub name: String,
    pub owner_user_id: Option<Uuid>,
    pub settings: serde_json::Value,
    pub members: Vec<SpaceMemberInfo>,
    pub created_at: DateTime<Utc>,
}

/// 创建团队空间
#[derive(Debug, Deserialize)]
pub struct CreateTeamSpaceRequest {
    pub name: String,
}

/// 更新空间
#[derive(Debug, Deserialize)]
pub struct UpdateSpaceRequest {
    pub name: Option<String>,
    pub settings: Option<SpaceSettings>,
}

/// 添加成员
#[derive(Debug, Deserialize)]
pub struct AddSpaceMemberRequest {
    pub user_id: Uuid,
    pub role: Option<String>,
    pub duties: Option<Vec<String>>,
}

/// 更新成员
#[derive(Debug, Deserialize)]
pub struct UpdateSpaceMemberRequest {
    pub role: Option<String>,
    pub duties: Option<Vec<String>>,
}
