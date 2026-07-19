use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// 枚举
// ---------------------------------------------------------------------------

/// AI 解析任务状态（对应数据库 ai_task_status 枚举）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "ai_task_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum AiTaskStatus {
    /// 排队中，等待 worker 拾取
    Pending,
    /// 解析中，LLM 正在处理
    Processing,
    /// 成功，已生成题目（question_id 已填入）
    Completed,
    /// 失败，error_message 记录详细原因
    Failed,
}

// ---------------------------------------------------------------------------
// 实体
// ---------------------------------------------------------------------------

/// AI 解析任务（数据库行）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AiParseTask {
    pub id: Uuid,
    /// 发起任务的教师
    pub creator_id: Uuid,
    /// 前端传来的 OCR 原始生肉文本
    pub raw_text: String,
    pub status: AiTaskStatus,
    /// 当状态为 completed 时，填入生成的题目 ID
    pub question_id: Option<Uuid>,
    /// 当状态为 failed 时，记录大模型超时或解析失败的详细原因
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
