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
}

/// 难度（粗粒度枚举，用于快速筛选）
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

/// 年级
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "grade_level", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum GradeLevel {
    Grade7,
    Grade8,
    Grade9,
    Grade10,
    Grade11,
    Grade12,
    Other,
}

/// 学期
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "semester_type", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum SemesterType {
    First,
    Second,
    FullYear,
}

/// 认知层次（布鲁姆分类法）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "cognitive_level", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum CognitiveLevel {
    Remember,
    Understand,
    Apply,
    Analyze,
    Evaluate,
    Create,
}

// ---------------------------------------------------------------------------
// 题目
// ---------------------------------------------------------------------------

/// 题目（数据库行）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Question {
    pub id: Uuid,

    // ── 内容 ──────────────────────────────────────
    /// 题干原始文本（LaTeX / Markdown）
    pub stem: String,
    /// 题干纯文本（去公式标记，用于全文检索与列表摘要）
    pub stem_text: Option<String>,
    /// 配图数组 [{"url": "...", "alt": "..."}]
    pub images: Option<serde_json::Value>,

    // ── 题型与答案 ────────────────────────────────
    pub question_type: QuestionType,
    /// 选项列表（选择题专用 JSONB）
    pub options: Option<serde_json::Value>,
    /// 正确答案
    pub correct_answer: serde_json::Value,
    /// 解析 / 解题过程
    pub analysis: Option<String>,
    /// 评分标准（解答题专用 JSONB）
    pub grading_criteria: Option<serde_json::Value>,

    // ── 难度与评估 ────────────────────────────────
    /// 粗粒度难度标签（快速筛选）
    pub difficulty: Difficulty,
    /// 精细难度系数 1-10
    pub difficulty_score: Option<i16>,
    /// 默认分值
    pub default_score: i32,
    /// 预估作答时间（分钟）
    pub estimated_minutes: Option<i16>,
    /// 认知层次（布鲁姆分类法）
    pub cognitive_level: Option<CognitiveLevel>,

    // ── 教研分类 ──────────────────────────────────
    /// 适用年级（枚举化）
    pub grade_level: Option<GradeLevel>,
    /// 学期（枚举化）— 对应 SQL 列 semester_new
    pub semester_new: Option<SemesterType>,

    // ── 来源元数据 ────────────────────────────────
    /// 出处备注（自由文本：书名、网址、"原创"等）
    pub source: Option<String>,
    /// 学年 (如 "2024-2025")
    pub academic_year: Option<String>,
    /// 考试类型 (期中/期末/高考/模拟)
    pub exam_type: Option<String>,
    /// 考试地区
    pub exam_region: Option<String>,

    // ── 复合题结构 ────────────────────────────────
    /// 父题 ID（NULL = 独立题或父题本身）
    pub parent_id: Option<Uuid>,
    /// 在父题下的排序序号
    pub sub_order: Option<i16>,

    // ── 统计缓存（反规范化） ──────────────────────
    /// 被组卷次数
    pub paper_count: i32,
    /// 累计作答次数
    pub attempt_count: i32,
    /// 累计正确率 (0.0000 ~ 1.0000)
    pub accuracy_rate: Option<rust_decimal::Decimal>,
    /// 被收藏次数
    pub favorite_count: i32,

    // ── 归属与审计 ────────────────────────────────
    pub status: QuestionStatus,
    pub space_id: Uuid,
    pub origin_question_id: Option<Uuid>,
    pub creator_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_by: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,

    // ── 已废弃字段（兼容期保留，后续版本 DROP） ──
    /// DEPRECATED: 使用 grade_level 替代
    #[serde(skip_serializing)]
    pub grade: Option<String>,
    /// DEPRECATED: 使用 semester_new 替代
    #[serde(skip_serializing)]
    pub semester: Option<String>,
    /// DEPRECATED: 使用 grade_level + semester_new 替代
    #[serde(skip_serializing)]
    pub grade_semester: Option<String>,
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
    // 来源元数据
    pub source: Option<String>,
    pub academic_year: Option<String>,
    pub exam_type: Option<String>,
    pub exam_region: Option<String>,
    // 教研维度（新）
    pub grade_level: Option<GradeLevel>,
    pub semester_new: Option<SemesterType>,
    pub cognitive_level: Option<CognitiveLevel>,
    pub difficulty_score: Option<i16>,
    pub estimated_minutes: Option<i16>,
    // 配图
    pub images: Option<serde_json::Value>,
    // 复合题
    pub parent_id: Option<Uuid>,
    pub sub_order: Option<i16>,
    // 标签 ID 列表（核心素养 + 解题方法 + 学校）
    pub tag_ids: Option<Vec<Uuid>>,
    /// 自建标签（尚未入库的名称，后端 Upsert 后合并到 tag_ids）
    #[serde(default)]
    pub new_tags: Option<Vec<NewTagInput>>,
    pub knowledge_point_ids: Option<Vec<Uuid>>,
    /// 所属空间；缺省为当前用户个人空间
    pub space_id: Option<Uuid>,
    /// 录入方式（"manual" | "ocr" | "ai_parse"）— 仅 "ocr" 触发配额扣减
    pub input_method: Option<String>,
    // DEPRECATED 旧字段 — 仍接受前端传入以兼容，但后续将移除
    pub grade: Option<String>,
    pub semester: Option<String>,
    pub grade_semester: Option<String>,
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
    // 来源元数据
    pub source: Option<String>,
    pub academic_year: Option<String>,
    pub exam_type: Option<String>,
    pub exam_region: Option<String>,
    // 教研维度（新）
    pub grade_level: Option<GradeLevel>,
    pub semester_new: Option<SemesterType>,
    pub cognitive_level: Option<CognitiveLevel>,
    pub difficulty_score: Option<i16>,
    pub estimated_minutes: Option<i16>,
    // 配图
    pub images: Option<serde_json::Value>,
    // 复合题
    pub parent_id: Option<Uuid>,
    pub sub_order: Option<i16>,
    // 标签 ID 列表（核心素养 + 解题方法 + 学校）
    pub tag_ids: Option<Vec<Uuid>>,
    /// 自建标签（尚未入库的名称，后端 Upsert 后合并到 tag_ids）
    #[serde(default)]
    pub new_tags: Option<Vec<NewTagInput>>,
    pub knowledge_point_ids: Option<Vec<Uuid>>,
    // DEPRECATED 旧字段 — 仍接受前端传入以兼容，但后续将移除
    pub grade: Option<String>,
    pub semester: Option<String>,
    pub grade_semester: Option<String>,
}

/// 自建标签输入（前端提交尚未入库的标签）
#[derive(Debug, Deserialize, Clone)]
pub struct NewTagInput {
    pub name: String,
    pub category: String,
}

/// 题目列表查询参数
#[derive(Debug, Deserialize)]
pub struct QuestionQuery {
    pub status: Option<QuestionStatus>,
    pub question_type: Option<QuestionType>,
    pub difficulty: Option<Difficulty>,
    pub grade: Option<String>,
    pub grade_level: Option<GradeLevel>,
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
    pub grade_level: Option<GradeLevel>,
    pub creator_id: Uuid,
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
            grade_level: q.grade_level,
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

    // ── 内容 ──
    pub stem: String,
    pub stem_text: Option<String>,
    pub images: Option<serde_json::Value>,

    // ── 题型与答案 ──
    pub question_type: QuestionType,
    pub options: Option<serde_json::Value>,
    pub correct_answer: serde_json::Value,
    pub analysis: Option<String>,
    pub grading_criteria: Option<serde_json::Value>,

    // ── 难度与评估 ──
    pub difficulty: Difficulty,
    pub difficulty_score: Option<i16>,
    pub default_score: i32,
    pub estimated_minutes: Option<i16>,
    pub cognitive_level: Option<CognitiveLevel>,

    // ── 教研分类 ──
    pub grade_level: Option<GradeLevel>,
    pub semester_new: Option<SemesterType>,

    // ── 来源元数据 ──
    pub source: Option<String>,
    pub academic_year: Option<String>,
    pub exam_type: Option<String>,
    pub exam_region: Option<String>,

    // ── 复合题结构 ──
    pub parent_id: Option<Uuid>,
    pub sub_order: Option<i16>,

    // ── 统计缓存 ──
    pub paper_count: i32,
    pub attempt_count: i32,
    pub accuracy_rate: Option<rust_decimal::Decimal>,
    pub favorite_count: i32,

    // ── 归属与审计 ──
    pub status: QuestionStatus,
    pub space_id: Uuid,
    pub origin_question_id: Option<Uuid>,
    pub creator_id: Uuid,
    pub creator_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_by: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,

    // ── 关联数据 ──
    pub knowledge_points: Vec<KnowledgePointSummary>,
    /// 关联标签（核心素养 + 解题方法 + 学校）
    pub tags: Vec<TagSummary>,
    pub reviewer_ids: Vec<Uuid>,
    pub can_review: bool,
}

impl From<(Question, Vec<KnowledgePointSummary>)> for QuestionDetail {
    fn from((q, kps): (Question, Vec<KnowledgePointSummary>)) -> Self {
        Self {
            id: q.id,
            // 内容
            stem: q.stem,
            stem_text: q.stem_text,
            images: q.images,
            // 题型与答案
            question_type: q.question_type,
            options: q.options,
            correct_answer: q.correct_answer,
            analysis: q.analysis,
            grading_criteria: q.grading_criteria,
            // 难度与评估
            difficulty: q.difficulty,
            difficulty_score: q.difficulty_score,
            default_score: q.default_score,
            estimated_minutes: q.estimated_minutes,
            cognitive_level: q.cognitive_level,
            // 教研分类
            grade_level: q.grade_level,
            semester_new: q.semester_new,
            // 来源元数据
            source: q.source,
            academic_year: q.academic_year,
            exam_type: q.exam_type,
            exam_region: q.exam_region,
            // 复合题结构
            parent_id: q.parent_id,
            sub_order: q.sub_order,
            // 统计缓存
            paper_count: q.paper_count,
            attempt_count: q.attempt_count,
            accuracy_rate: q.accuracy_rate,
            favorite_count: q.favorite_count,
            // 归属与审计
            status: q.status,
            space_id: q.space_id,
            origin_question_id: q.origin_question_id,
            creator_id: q.creator_id,
            creator_name: None,
            created_at: q.created_at,
            updated_by: q.updated_by,
            updated_at: q.updated_at,
            version: q.version,
            // 关联数据
            knowledge_points: kps,
            tags: vec![],
            reviewer_ids: vec![],
            can_review: false,
        }
    }
}

/// 驳回请求（reject_question 专用）
#[derive(Debug, Deserialize)]
pub struct RejectRequest {
    /// 驳回原因（可选，记录到日志便于审计追踪）
    pub reject_reason: Option<String>,
}

/// 贡献到公共库 / 从公共导入
#[derive(Debug, Deserialize)]
pub struct TransferQuestionRequest {
    /// 导入时的目标空间；贡献到公共时可省略
    pub target_space_id: Option<Uuid>,
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

// ---------------------------------------------------------------------------
// 标签
// ---------------------------------------------------------------------------

/// 标签摘要（用于题目详情中的关联展示）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TagSummary {
    pub id: Uuid,
    pub name: String,
    pub category: String,
}

/// 标签（数据库行）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Tag {
    pub id: Uuid,
    pub name: String,
    pub category: String,
    pub space_id: Option<Uuid>,
    pub use_count: i32,
    pub created_at: DateTime<Utc>,
}

/// 创建标签请求
#[derive(Debug, Deserialize)]
pub struct CreateTagRequest {
    pub name: String,
    pub category: String,
    pub space_id: Option<Uuid>,
}

/// 更新标签请求（部分更新）
#[derive(Debug, Deserialize)]
pub struct UpdateTagRequest {
    pub name: Option<String>,
    pub category: Option<String>,
}

/// 标签查询参数
#[derive(Debug, Deserialize)]
pub struct TagQuery {
    pub category: Option<String>,
    pub space_id: Option<Uuid>,
}
