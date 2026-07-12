use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// 枚举
// ---------------------------------------------------------------------------

/// 题型
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "question_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum QuestionType {
    Choice,
    Fill,
    Solution,
    Judgment,
}

/// 难度
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "difficulty", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

/// 题目状态
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "question_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum QuestionStatus {
    Draft,
    Pending,
    Rejected,
    Published,
    Disabled,
}

// ---------------------------------------------------------------------------
// 题目
// ---------------------------------------------------------------------------

/// 题目（数据库行）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Question {
    pub id: Uuid,
    pub stem: String,
    pub question_type: QuestionType,
    pub difficulty: Difficulty,
    pub default_score: i32,
    pub status: QuestionStatus,
    pub options: Option<serde_json::Value>,
    pub correct_answer: serde_json::Value,
    pub analysis: Option<String>,
    pub grading_criteria: Option<serde_json::Value>,
    pub grade: Option<String>,
    pub semester: Option<String>,
    pub source: Option<String>,
    pub creator_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_by: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,
    pub space_id: Uuid,
    pub origin_question_id: Option<Uuid>,
}

/// 创建题目请求
#[derive(Debug, Deserialize)]
pub struct CreateQuestionRequest {
    pub stem: String,
    pub question_type: QuestionType,
    pub difficulty: Difficulty,
    pub default_score: Option<i32>,
    pub options: Option<serde_json::Value>,
    pub correct_answer: serde_json::Value,
    pub analysis: Option<String>,
    pub grading_criteria: Option<serde_json::Value>,
    pub grade: Option<String>,
    pub semester: Option<String>,
    pub source: Option<String>,
    pub knowledge_point_ids: Option<Vec<Uuid>>,
    /// 所属空间；缺省为当前用户个人空间
    pub space_id: Option<Uuid>,
}

/// 更新题目请求
#[derive(Debug, Deserialize)]
pub struct UpdateQuestionRequest {
    pub stem: Option<String>,
    pub question_type: Option<QuestionType>,
    pub difficulty: Option<Difficulty>,
    pub default_score: Option<i32>,
    pub options: Option<serde_json::Value>,
    pub correct_answer: Option<serde_json::Value>,
    pub analysis: Option<String>,
    pub grading_criteria: Option<serde_json::Value>,
    pub grade: Option<String>,
    pub semester: Option<String>,
    pub source: Option<String>,
    pub knowledge_point_ids: Option<Vec<Uuid>>,
}

/// 题目列表查询参数
#[derive(Debug, Deserialize)]
pub struct QuestionQuery {
    pub status: Option<QuestionStatus>,
    pub question_type: Option<QuestionType>,
    pub difficulty: Option<Difficulty>,
    pub grade: Option<String>,
    pub knowledge_point_id: Option<Uuid>,
    pub creator_id: Option<Uuid>,
    pub keyword: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    /// 按空间过滤
    pub space_id: Option<Uuid>,
    /// 仅返回当前用户可审核的待审题
    pub reviewable_by_me: Option<bool>,
}

/// 题目列表响应项
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct QuestionSummary {
    pub id: Uuid,
    pub stem: String,
    pub question_type: QuestionType,
    pub difficulty: Difficulty,
    pub default_score: i32,
    pub status: QuestionStatus,
    pub grade: Option<String>,
    pub creator_id: Option<Uuid>,
    pub creator_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,
    pub space_id: Uuid,
}

impl From<Question> for QuestionSummary {
    fn from(q: Question) -> Self {
        Self {
            id: q.id,
            stem: q.stem,
            question_type: q.question_type,
            difficulty: q.difficulty,
            default_score: q.default_score,
            status: q.status,
            grade: q.grade,
            creator_id: q.creator_id,
            creator_name: None,
            created_at: q.created_at,
            updated_at: q.updated_at,
            version: q.version,
            space_id: q.space_id,
        }
    }
}

/// 题目详情响应（含知识点和审核记录）
#[derive(Debug, Serialize)]
pub struct QuestionDetail {
    pub id: Uuid,
    pub stem: String,
    pub question_type: QuestionType,
    pub difficulty: Difficulty,
    pub default_score: i32,
    pub status: QuestionStatus,
    pub options: Option<serde_json::Value>,
    pub correct_answer: serde_json::Value,
    pub analysis: Option<String>,
    pub grading_criteria: Option<serde_json::Value>,
    pub grade: Option<String>,
    pub semester: Option<String>,
    pub source: Option<String>,
    pub creator_id: Option<Uuid>,
    pub creator_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_by: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,
    pub space_id: Uuid,
    pub origin_question_id: Option<Uuid>,
    pub knowledge_points: Vec<KnowledgePointSummary>,
    pub reviewer_ids: Vec<Uuid>,
    pub can_review: bool,
}

impl From<(Question, Vec<KnowledgePointSummary>)> for QuestionDetail {
    fn from((q, kps): (Question, Vec<KnowledgePointSummary>)) -> Self {
        Self {
            id: q.id,
            stem: q.stem,
            question_type: q.question_type,
            difficulty: q.difficulty,
            default_score: q.default_score,
            status: q.status,
            options: q.options,
            correct_answer: q.correct_answer,
            analysis: q.analysis,
            grading_criteria: q.grading_criteria,
            grade: q.grade,
            semester: q.semester,
            source: q.source,
            creator_id: q.creator_id,
            creator_name: None,
            created_at: q.created_at,
            updated_by: q.updated_by,
            updated_at: q.updated_at,
            version: q.version,
            space_id: q.space_id,
            origin_question_id: q.origin_question_id,
            knowledge_points: kps,
            reviewer_ids: vec![],
            can_review: false,
        }
    }
}

/// 提交审核请求
#[derive(Debug, Deserialize)]
pub struct SubmitReviewRequest {
    pub comment: Option<String>,
    /// 指定审题人（可多人）；空或省略则走空间默认规则
    pub reviewer_ids: Option<Vec<Uuid>>,
}

/// 贡献到公共库 / 从公共导入
#[derive(Debug, Deserialize)]
pub struct TransferQuestionRequest {
    /// 导入时的目标空间；贡献到公共时可省略
    pub target_space_id: Option<Uuid>,
}

/// 审核请求
#[derive(Debug, Deserialize)]
pub struct ReviewActionRequest {
    pub action: String, // "approved" 或 "rejected"
    pub comment: Option<String>,
}

// ---------------------------------------------------------------------------
// 知识点
// ---------------------------------------------------------------------------

/// 知识点（数据库行）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct KnowledgePoint {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub grade: Option<String>,
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
    pub space_id: Option<Uuid>,
}

/// 知识点树节点（带 children）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgePointTreeNode {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub grade: Option<String>,
    pub sort_order: i32,
    pub children: Vec<KnowledgePointTreeNode>,
}

impl From<KnowledgePoint> for KnowledgePointTreeNode {
    fn from(kp: KnowledgePoint) -> Self {
        Self {
            id: kp.id,
            parent_id: kp.parent_id,
            name: kp.name,
            grade: kp.grade,
            sort_order: kp.sort_order,
            children: vec![],
        }
    }
}

/// 知识点摘要（用于题目详情中的关联展示）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct KnowledgePointSummary {
    pub id: Uuid,
    pub name: String,
}

/// 创建知识点请求
#[derive(Debug, Deserialize)]
pub struct CreateKnowledgePointRequest {
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub grade: Option<String>,
    pub sort_order: Option<i32>,
    pub space_id: Option<Uuid>,
}

/// 更新知识点请求
#[derive(Debug, Deserialize)]
pub struct UpdateKnowledgePointRequest {
    pub parent_id: Option<Uuid>,
    pub name: Option<String>,
    pub grade: Option<String>,
    pub sort_order: Option<i32>,
}

// ---------------------------------------------------------------------------
// 审核记录
// ---------------------------------------------------------------------------

/// 审核记录
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ReviewRecord {
    pub id: Uuid,
    pub question_id: Uuid,
    pub reviewer_id: Uuid,
    pub action: String,
    pub comment: Option<String>,
    pub created_at: DateTime<Utc>,
}
