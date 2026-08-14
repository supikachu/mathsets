use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// 枚举
// ---------------------------------------------------------------------------

/// AI 解析任务状态（对应数据库 ai_task_status 枚举）
///
/// 状态机（计划书 §五）：pending → processing →（retrying ⇄ pending）→
/// success / partial_success / failed / cancelled
/// 历史值 completed 保留兼容，API 读出时映射为 success。
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "ai_task_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum AiTaskStatus {
    /// 排队中，等待 worker 拾取
    Pending,
    /// 解析中，LLM 正在处理
    Processing,
    /// 可重试失败（LLM 超时/上游错误/JSON 非法），回到队列
    Retrying,
    /// 全部成功
    Success,
    /// 部分成功（部分题目失败）
    PartialSuccess,
    /// 失败（不可重试或重试次数用尽）
    Failed,
    /// 用户取消（已落库题目保留）
    Cancelled,
    /// 历史兼容：旧成功状态，读出时映射为 success
    Completed,
}

impl AiTaskStatus {
    /// 是否终态
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            AiTaskStatus::Success
                | AiTaskStatus::PartialSuccess
                | AiTaskStatus::Failed
                | AiTaskStatus::Cancelled
        )
    }

    /// 读出视图：completed → success
    pub fn to_view(&self) -> AiTaskStatus {
        match self {
            AiTaskStatus::Completed => AiTaskStatus::Success,
            other => other.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// 实体
// ---------------------------------------------------------------------------

/// AI 解析任务（数据库行）— V2.1.1 扩展：Document 关联 + 计数 + 租约 + 取消
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AiParseTask {
    pub id: Uuid,
    /// 发起任务的用户
    pub creator_id: Uuid,
    /// 历史字段：旧文本任务用（新任务为空字符串）
    pub raw_text: String,
    pub status: AiTaskStatus,
    /// 历史字段：旧单题任务的题目 ID（新任务用 progress.idempotency_map）
    pub question_id: Option<Uuid>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // ── V2.1.1 ──
    /// 来源 Document（1:N，不唯一）
    pub document_id: Option<Uuid>,
    /// 输入快照：{document_type, title, paper_meta?, collections[]}
    pub paper_meta: serde_json::Value,
    pub total_count: i32,
    pub processed_count: i32,
    pub success_count: i32,
    pub failed_count: i32,
    pub retry_count: i32,
    pub current_page: Option<i32>,
    pub total_pages: Option<i32>,
    pub current_question_no: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    /// 幂等映射：{"idempotency_map": {"p1_i2": "<question_id>"}}
    pub progress: serde_json::Value,
    /// 租约：认领时间 / worker 标识 / 心跳时间
    pub locked_at: Option<DateTime<Utc>>,
    pub worker_id: Option<String>,
    pub heartbeat_at: Option<DateTime<Utc>>,
    /// 用户取消标记（pending/processing/retrying 可取消）
    pub cancel_requested_at: Option<DateTime<Utc>>,
}

/// 任务进度视图（GET /ai/parse-task/{id} 响应）
#[derive(Debug, Clone, Serialize)]
pub struct TaskStatusResponse {
    pub id: Uuid,
    /// completed → success 映射后的视图状态
    pub status: AiTaskStatus,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // 计数与进度
    pub total_count: i32,
    pub processed_count: i32,
    pub success_count: i32,
    pub failed_count: i32,
    pub retry_count: i32,
    pub current_page: Option<i32>,
    pub total_pages: Option<i32>,
    pub current_question_no: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,

    // 结果关联（懒查询填充）
    pub paper_id: Option<Uuid>,
    pub collection_ids: Vec<Uuid>,
    /// 任务产出的题目 ID 列表（按解析顺序）
    pub question_ids: Vec<Uuid>,
}
