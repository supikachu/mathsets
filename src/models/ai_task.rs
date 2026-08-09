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

/// AI 解析任务来源类型（M4 新增，对应数据库 ai_task_source_type 枚举）
///
/// - `Text` — 前端粘贴的 OCR 文本，走纯 LLM 解析
/// - `Image` — 上传图片，走 OCR + Stage 2 LLM 解析
/// - `Pdf` — 上传 PDF，走 OCR（仅 doc2x/mineru 支持） + Stage 2 LLM 解析
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "ai_task_source_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum AiTaskSourceType {
    /// 文本输入（旧路径，保持向后兼容）
    Text,
    /// 单图上传
    Image,
    /// PDF 文档上传
    Pdf,
}

// ---------------------------------------------------------------------------
// 实体
// ---------------------------------------------------------------------------

/// AI 解析任务（数据库行）
///
/// v1.1（M4）：扩展支持 image/pdf 来源任务。
/// - `raw_text` 改为 `Option<String>`，image/pdf 任务不填
/// - `image_b64` / `pdf_bytes` 用于 image/pdf 任务的二进制负载
/// - `question_ids` 用于多题批处理场景，前端优先读取此字段获取所有题目 UUID
/// - `question_id` 保留为单个 UUID，存首题 ID（向后兼容旧前端）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AiParseTask {
    pub id: Uuid,
    /// 发起任务的教师
    pub creator_id: Uuid,
    /// 文本类任务的原始 OCR Markdown（image/pdf 任务为 None）
    pub raw_text: Option<String>,
    /// 任务来源类型（text/image/pdf）
    pub source_type: AiTaskSourceType,
    /// base64 编码的图片数据（source_type=image 时填）
    pub image_b64: Option<String>,
    /// 原始 PDF 二进制（source_type=pdf 时填）
    pub pdf_bytes: Option<Vec<u8>>,
    /// 可选 OCR 引擎覆盖（用户上传时临时指定，覆盖个人偏好）
    pub ocr_provider_override: Option<String>,
    pub status: AiTaskStatus,
    /// 当状态为 completed 时，填入生成的题目 ID（首题，向后兼容）
    pub question_id: Option<Uuid>,
    /// M4：所有生成的题目 UUID 数组（多题批处理场景）
    pub question_ids: Option<serde_json::Value>,
    /// 当状态为 failed 时，记录大模型超时或解析失败的详细原因
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
