use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// 试卷状态
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "varchar", rename_all = "lowercase")]
pub enum PaperStatus {
    #[serde(rename = "draft")]
    Draft,
    #[serde(rename = "published")]
    Published,
    #[serde(rename = "archived")]
    Archived,
}

impl std::fmt::Display for PaperStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PaperStatus::Draft => write!(f, "draft"),
            PaperStatus::Published => write!(f, "published"),
            PaperStatus::Archived => write!(f, "archived"),
        }
    }
}

/// 试卷（V2.1.1 扩展：试卷元数据列 + document_id 幂等复用键 + metadata JSONB）
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Paper {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub subject: String,
    pub grade: Option<String>,
    pub total_score: i32,
    pub duration_minutes: Option<i32>,
    pub status: PaperStatus,
    pub creator_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,

    // ── V2.1.1 试卷元数据（计划书 §三：Paper 层字段） ──
    pub year: Option<i32>,
    pub stage: Option<String>,
    pub semester: Option<String>,
    pub region_province: Option<String>,
    pub region_city: Option<String>,
    pub school_name: Option<String>,
    pub source_type: Option<String>,
    pub sub_source_type: Option<String>,
    /// 来源 Document（幂等复用键：同一文档重跑只建一张试卷）
    pub document_id: Option<Uuid>,
    pub metadata: serde_json::Value,
}

/// 试卷简要信息（用于下拉选择）
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PaperBrief {
    pub id: Uuid,
    pub title: String,
}

/// 题目被引用的试卷项（反向查询响应）
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct QuestionPaperItem {
    pub paper_id: Uuid,
    pub title: String,
    pub sort_order: i32,
    pub score: i32,
    pub section: Option<String>,
    /// V2.1.1：题号（可空，历史数据兼容）
    pub question_no: Option<String>,
    pub display_order: i32,
    pub created_at: DateTime<Utc>,
}

/// V2.1.1 统一来源视图（GET /questions/{id}/sources）
///
/// 回答"这道题从哪里来"：kind = paper | collection，
/// 同时携带 Document 层信息（Document → Paper/Collection → Question 全链路）。
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct QuestionSourceItem {
    /// paper | collection
    pub kind: String,
    /// 容器 ID（paper_id / collection_id）
    pub id: Uuid,
    /// 容器标题（试卷名 / 集合名）
    pub title: String,
    /// 容器类型：paper 时为 sub_source_type，collection 时为 collection_type
    pub type_label: Option<String>,
    pub question_no: Option<String>,
    pub display_order: i32,
    pub score: Option<i32>,
    pub section: Option<String>,
    /// 来源 Document（可能为 NULL，历史数据兼容）
    pub document_id: Option<Uuid>,
    pub document_title: Option<String>,
    pub document_type: Option<String>,
}

/// 试卷列表响应
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PaperSummary {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub subject: String,
    pub grade: Option<String>,
    pub total_score: i32,
    pub duration_minutes: Option<i32>,
    pub status: PaperStatus,
    pub creator_id: Option<Uuid>,
    pub creator_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,
    pub question_count: i64,

    // ── V2.1.1 元数据 ──
    pub year: Option<i32>,
    pub stage: Option<String>,
    pub semester: Option<String>,
    pub region_province: Option<String>,
    pub region_city: Option<String>,
    pub school_name: Option<String>,
    pub source_type: Option<String>,
    pub sub_source_type: Option<String>,
    pub document_id: Option<Uuid>,
    pub metadata: serde_json::Value,
}

/// 试卷-题目关联
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PaperQuestion {
    pub id: Uuid,
    pub paper_id: Uuid,
    pub question_id: Uuid,
    pub sort_order: i32,
    pub score: i32,
    pub section: Option<String>,
    /// V2.1.1：题号（自由格式 1/1(1)/一、1，不唯一）
    pub question_no: Option<String>,
    pub display_order: i32,
    pub created_at: DateTime<Utc>,
}

/// 试卷详情（含题目列表）
#[derive(Debug, Clone, Serialize)]
pub struct PaperDetail {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub subject: String,
    pub grade: Option<String>,
    pub total_score: i32,
    pub duration_minutes: Option<i32>,
    pub status: PaperStatus,
    pub creator_id: Option<Uuid>,
    pub creator_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,
    pub questions: Vec<PaperQuestionItem>,

    // ── V2.1.1 元数据 ──
    pub year: Option<i32>,
    pub stage: Option<String>,
    pub semester: Option<String>,
    pub region_province: Option<String>,
    pub region_city: Option<String>,
    pub school_name: Option<String>,
    pub source_type: Option<String>,
    pub sub_source_type: Option<String>,
    pub document_id: Option<Uuid>,
    pub metadata: serde_json::Value,
}

/// 试卷中的题目项
#[derive(Debug, Clone, Serialize)]
pub struct PaperQuestionItem {
    pub id: Uuid,
    pub question_id: Uuid,
    pub sort_order: i32,
    pub score: i32,
    pub section: Option<String>,
    pub question_no: Option<String>,
    pub display_order: i32,
    pub stem: String,
    pub question_type: String,
    pub difficulty: String,
}

/// 创建试卷请求（V2.1.1：支持元数据 + document_id 幂等复用键）
#[derive(Debug, Deserialize)]
pub struct CreatePaperRequest {
    pub title: String,
    pub description: Option<String>,
    pub subject: Option<String>,
    pub grade: Option<String>,
    pub total_score: Option<i32>,
    pub duration_minutes: Option<i32>,

    // ── V2.1.1 ──
    pub year: Option<i32>,
    pub stage: Option<String>,
    pub semester: Option<String>,
    pub region_province: Option<String>,
    pub region_city: Option<String>,
    pub school_name: Option<String>,
    pub source_type: Option<String>,
    pub sub_source_type: Option<String>,
    pub document_id: Option<Uuid>,
    pub metadata: Option<serde_json::Value>,
}

/// 更新试卷请求
#[derive(Debug, Deserialize)]
pub struct UpdatePaperRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub subject: Option<String>,
    pub grade: Option<String>,
    pub total_score: Option<i32>,
    pub duration_minutes: Option<i32>,

    // ── V2.1.1 ──
    pub year: Option<i32>,
    pub stage: Option<String>,
    pub semester: Option<String>,
    pub region_province: Option<String>,
    pub region_city: Option<String>,
    pub school_name: Option<String>,
    pub source_type: Option<String>,
    pub sub_source_type: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

/// 添加题目到试卷请求
#[derive(Debug, Deserialize)]
pub struct AddQuestionRequest {
    pub question_id: Uuid,
    pub score: Option<i32>,
    pub section: Option<String>,
    pub sort_order: Option<i32>,
    /// V2.1.1：题号
    pub question_no: Option<String>,
    /// V2.1.1：展示顺序（缺省 = sort_order）
    pub display_order: Option<i32>,
}

/// 更新试卷题目请求
#[derive(Debug, Deserialize)]
pub struct UpdatePaperQuestionRequest {
    pub score: Option<i32>,
    pub sort_order: Option<i32>,
    pub section: Option<String>,
    /// V2.1.1：题号
    pub question_no: Option<String>,
    /// V2.1.1：展示顺序
    pub display_order: Option<i32>,
}
