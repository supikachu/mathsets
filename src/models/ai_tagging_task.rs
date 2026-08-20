//! 独立异步打标任务（编辑页轮询，不复用 ai_parse_tasks）

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AiTaggingTaskStatus {
    Pending,
    Processing,
    Retrying,
    Success,
    Failed,
    Cancelled,
}

impl AiTaggingTaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Retrying => "retrying",
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Success | Self::Failed | Self::Cancelled)
    }

    pub fn from_db(s: &str) -> Self {
        match s {
            "processing" => Self::Processing,
            "retrying" => Self::Retrying,
            "success" => Self::Success,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct AiTaggingTask {
    pub id: Uuid,
    pub creator_id: Uuid,
    pub space_id: Option<Uuid>,
    pub question_id: Option<Uuid>,
    pub input_hash: String,
    pub content: String,
    /// junior | senior（对应树 code 后缀 _junior / _high）
    pub stage: Option<String>,
    pub status: String,
    pub retry_count: i32,
    pub error_message: Option<String>,
    pub suggestion_id: Option<Uuid>,
    pub locked_at: Option<DateTime<Utc>>,
    pub worker_id: Option<String>,
    pub heartbeat_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub cancel_requested_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub const TAGGING_TASK_COLUMNS: &str = "id, creator_id, space_id, question_id, input_hash, content, stage, \
     status, retry_count, error_message, suggestion_id, locked_at, worker_id, heartbeat_at, \
     started_at, completed_at, cancel_requested_at, created_at, updated_at";
