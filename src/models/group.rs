use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 教研组（数据库行）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 教研组成员
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct GroupMember {
    pub group_id: Uuid,
    pub user_id: Uuid,
    pub is_leader: bool,
    pub joined_at: DateTime<Utc>,
}

/// 教研组列表项（含成员数）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct GroupSummary {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub member_count: Option<i64>,
    pub created_at: DateTime<Utc>,
}

/// 教研组详情（含成员列表）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupDetail {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub members: Vec<GroupMemberInfo>,
    pub created_at: DateTime<Utc>,
}

/// 成员信息（含用户名和显示名）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct GroupMemberInfo {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub is_leader: bool,
    pub joined_at: DateTime<Utc>,
}

/// 创建教研组请求
#[derive(Debug, Deserialize)]
pub struct CreateGroupRequest {
    pub name: String,
    pub description: Option<String>,
}

/// 更新教研组请求
#[derive(Debug, Deserialize)]
pub struct UpdateGroupRequest {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// 添加成员请求
#[derive(Debug, Deserialize)]
pub struct AddMemberRequest {
    pub user_id: Uuid,
    pub is_leader: Option<bool>,
}

/// 设置组长请求
#[derive(Debug, Deserialize)]
pub struct SetLeaderRequest {
    pub is_leader: bool,
}
