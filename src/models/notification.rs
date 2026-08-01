use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// 通知模型（数据库行）
// ---------------------------------------------------------------------------

/// 消息通知（数据库行）
///
/// kind 字段为 VARCHAR，取值约定：
/// - `"workflow"`：工作流通知（提交审核、审核通过/驳回）
/// - `"system"`：系统级通知（角色变更、空间邀请）
///
/// resource_type 字段约定（前端跳转闭环）：
/// - `"question"`：跳转题目详情 `/questions/:id`
/// - `"question_edit"`：跳转题目编辑 `/questions/:id/edit`（驳回场景）
/// - `"space"`：跳转空间设置 `/spaces/:id/settings`
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Notification {
    pub id: Uuid,
    pub user_id: Uuid,
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<Uuid>,
    pub is_read: bool,
    pub created_at: DateTime<Utc>,
}

/// 创建通知请求（内部使用，Phase 2 工作流事件注入时调用）
#[derive(Debug, Clone)]
pub struct CreateNotification {
    pub user_id: Uuid,
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<Uuid>,
}

// ---------------------------------------------------------------------------
// SSE 广播事件
// ---------------------------------------------------------------------------

/// 广播事件 — 通过 `tokio::sync::broadcast` 通道推送给所有 SSE 连接
///
/// 每个 SSE 连接根据 `user_id` 过滤，仅接收自己的通知
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcastEvent {
    pub user_id: Uuid,
    pub notification: Notification,
}

// ---------------------------------------------------------------------------
// SSE 一次性票据
// ---------------------------------------------------------------------------

/// SSE 连接票据（一次性，30 秒过期）
///
/// 安全流程：
/// 1. 前端通过标准 JWT 请求 `POST /notifications/ticket` 获取 ticket
/// 2. 前端用 `new EventSource('/notifications/stream?ticket=xxx')` 建立连接
/// 3. 后端验证并销毁 ticket，避免 JWT 暴露在 URL/日志中
#[derive(Debug, Clone)]
pub struct TicketInfo {
    pub user_id: Uuid,
    pub expires_at: Instant,
}

impl TicketInfo {
    /// 票据有效期：30 秒
    pub const TTL: std::time::Duration = std::time::Duration::from_secs(30);

    pub fn new(user_id: Uuid) -> Self {
        Self {
            user_id,
            expires_at: Instant::now() + Self::TTL,
        }
    }

    pub fn is_expired(&self) -> bool {
        Instant::now() > self.expires_at
    }
}
