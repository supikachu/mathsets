// ===========================================================================
// V2.1.1 QuestionCollection / CollectionQuestion 数据模型
//
// 集合 = 文件中一组题目的业务容器（计划书 §三/§五）：
// - 复用规则：同文档内按 (document_id, title) 幂等；跨文档同名一律新建
// - collection_questions.question_no 自由格式（1/1(1)/一、1），不设唯一约束
// ===========================================================================

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// 题目集合（数据库行）
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct QuestionCollection {
    pub id: Uuid,
    pub document_id: Uuid,
    pub creator_id: Uuid,
    pub title: String,
    pub collection_type: String,
    pub type_label: Option<String>,
    pub source_type: Option<String>,
    pub subject: Option<String>,
    pub stage: Option<String>,
    pub grade: Option<String>,
    pub semester: Option<String>,
    pub chapter_id: Option<Uuid>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 集合-题目关联（数据库行）
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CollectionQuestion {
    pub id: Uuid,
    pub collection_id: Uuid,
    pub question_id: Uuid,
    pub question_no: Option<String>,
    pub display_order: i32,
    pub section: Option<String>,
    pub score: Option<i32>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// 集合详情（含来源 Document 信息与题目列表）
#[derive(Debug, Clone, Serialize)]
pub struct CollectionDetail {
    pub id: Uuid,
    pub document_id: Uuid,
    pub creator_id: Uuid,
    pub title: String,
    pub collection_type: String,
    pub type_label: Option<String>,
    pub source_type: Option<String>,
    pub subject: Option<String>,
    pub stage: Option<String>,
    pub grade: Option<String>,
    pub semester: Option<String>,
    pub chapter_id: Option<Uuid>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// 来源 Document 摘要（来源链路 Document → Collection → Questions）
    pub document_title: Option<String>,
    pub document_type: Option<String>,
    /// 题目项
    pub questions: Vec<CollectionQuestionItem>,
}

/// 集合详情中的题目项
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CollectionQuestionItem {
    pub id: Uuid,
    pub question_id: Uuid,
    pub question_no: Option<String>,
    pub display_order: i32,
    pub score: Option<i32>,
    pub stem: String,
    pub question_type: String,
    pub difficulty: String,
}

/// 批量添加题目请求（Mixed 人工分组 / Collection 详情页补分）
#[derive(Debug, Deserialize)]
pub struct BatchAddQuestionsRequest {
    pub questions: Vec<BatchAddQuestionInput>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BatchAddQuestionInput {
    pub question_id: Uuid,
    /// 缺省时后端按该集合内最大 display_order + 1 自动编号
    pub question_no: Option<String>,
    pub display_order: Option<i32>,
    pub score: Option<i32>,
    pub section: Option<String>,
}
