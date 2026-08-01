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

/// 试卷
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
    pub created_at: DateTime<Utc>,
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
}

/// 试卷中的题目项
#[derive(Debug, Clone, Serialize)]
pub struct PaperQuestionItem {
    pub id: Uuid,
    pub question_id: Uuid,
    pub sort_order: i32,
    pub score: i32,
    pub section: Option<String>,
    pub stem: String,
    pub question_type: String,
    pub difficulty: String,
}

/// 创建试卷请求
#[derive(Debug, Deserialize)]
pub struct CreatePaperRequest {
    pub title: String,
    pub description: Option<String>,
    pub subject: Option<String>,
    pub grade: Option<String>,
    pub total_score: Option<i32>,
    pub duration_minutes: Option<i32>,
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
}

/// 添加题目到试卷请求
#[derive(Debug, Deserialize)]
pub struct AddQuestionRequest {
    pub question_id: Uuid,
    pub score: Option<i32>,
    pub section: Option<String>,
    pub sort_order: Option<i32>,
}

/// 更新试卷题目请求
#[derive(Debug, Deserialize)]
pub struct UpdatePaperQuestionRequest {
    pub score: Option<i32>,
    pub sort_order: Option<i32>,
    pub section: Option<String>,
}
