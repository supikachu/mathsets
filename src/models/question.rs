use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ===========================================================================
// 枚举类型
// ===========================================================================

/// 题型
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "question_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum QuestionType {
    Choice,
    /// 多选题（B2 新增）
    Multiple,
    Fill,
    Solution,
}

/// 难度（1-5 星制，B2 重构）
///
/// B1 迁移：从旧 enum (Easy/Medium/Hard) 转为 i16 (1-5)。
/// 迁移公式：easy=2, medium=3, hard=4。
///
/// 使用 newtype + `#[sqlx(transparent)]` 让 sqlx 直接代理 i16 编解码，
/// 序列化为 JSON 数字（如 `2`），反序列化接受数字。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, sqlx::Type)]
#[sqlx(transparent)]
#[serde(transparent)]
pub struct Difficulty(pub i16);

impl Difficulty {
    /// 构造难度，校验 1-5 范围
    pub fn new(value: i16) -> Result<Self, String> {
        if (1..=5).contains(&value) {
            Ok(Self(value))
        } else {
            Err(format!("difficulty must be 1-5, got {}", value))
        }
    }

    pub fn value(&self) -> i16 {
        self.0
    }
}

impl From<Difficulty> for i16 {
    fn from(d: Difficulty) -> Self {
        d.0
    }
}

impl std::fmt::Display for Difficulty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
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

/// 考试类型（B2 新增，B1 已将 VARCHAR 迁移为 enum）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "exam_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum ExamType {
    Midterm,
    Final,
    Gaokao,
    Mock,
    Entrance,
    Daily,
    Other,
}

/// 标签类别（B2 新增，B1 已将 VARCHAR 迁移为 enum）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "tag_category", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum TagCategory {
    /// 核心素养
    CoreCompetence,
    /// 解题方法 / 数学思想（扁平 tags，与题型专题树拆分）
    Method,
    /// 学校来源
    School,
    /// 应用场景
    Scene,
    /// 易错点
    ErrorProne,
}

/// 知识树类型（B2 新增）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "knowledge_tree_kind", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum KnowledgeTreeKind {
    /// 知识树（核心，按数学学科结构）
    Knowledge,
    /// 题型专题 / 专题技法树（存于 math_method_*，历史枚举名 ability）
    Ability,
    /// 章节树（教材版本：高中人教 A 版 / 初中浙教版）
    Chapter,
}

/// 知识点关联来源（B2 新增，用于 AI 智能打标审计）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "knowledge_link_source", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum KnowledgeLinkSource {
    /// 手工标注
    Manual,
    /// AI 自动标注
    Ai,
}

// ===========================================================================
// 题目
// ===========================================================================

/// 题目（数据库行）— B2 重构：移除已 DROP 的旧字段，新增 metadata JSONB
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Question {
    pub id: Uuid,

    // ── 内容 ──────────────────────────────────────
    pub stem: String,
    pub stem_text: Option<String>,
    pub images: Option<serde_json::Value>,

    // ── 题型与答案 ────────────────────────────────
    pub question_type: QuestionType,
    pub options: Option<serde_json::Value>,
    /// 答案 JSONB。允许为空（None / JSON null）以支持「异步补全」草稿
    pub correct_answer: Option<serde_json::Value>,
    pub analysis: Option<String>,

    // ── 难度与评估 ────────────────────────────────
    /// 难度 1-5（5 星制）
    pub difficulty: Difficulty,

    // ── 元数据 ──────────────────────────────────
    /// 长尾元数据 JSONB（academic_year, exam_region, paper_name,
    /// paper_page, textbook_version 等长尾字段统一存此 JSON）
    pub metadata: serde_json::Value,

    // ── V2.1.1 去重 hash（SHA-256；历史数据由离线 Job 回填） ──────
    pub content_hash: Option<String>,
    pub normalized_content_hash: Option<String>,

    // ── 复合题结构 ────────────────────────────────
    pub parent_id: Option<Uuid>,
    pub sub_order: Option<i16>,

    // ── 统计缓存（反规范化） ──────────────────────
    pub paper_count: i32,
    pub attempt_count: i32,
    pub accuracy_rate: Option<rust_decimal::Decimal>,
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
}

/// 创建题目请求（B2 重构）
#[derive(Debug, Deserialize)]
pub struct CreateQuestionRequest {
    pub stem: String,
    pub question_type: QuestionType,
    pub difficulty: Difficulty,
    pub options: Option<serde_json::Value>,
    /// 答案（允许为空，支持「异步补全」草稿；空值将写入 system_flags.pending_answer）
    #[serde(default)]
    pub correct_answer: Option<serde_json::Value>,
    pub analysis: Option<String>,
    /// 长尾元数据（academic_year, exam_region, paper_name 等）
    pub metadata: Option<serde_json::Value>,
    // 配图
    pub images: Option<serde_json::Value>,
    // 复合题
    pub parent_id: Option<Uuid>,
    pub sub_order: Option<i16>,
    // 标签 ID 列表（核心素养 + 解题方法 + 学校 + 场景 + 易错点）
    pub tag_ids: Option<Vec<Uuid>>,
    /// 自建标签（尚未入库的名称，后端 Upsert 后合并到 tag_ids）
    #[serde(default)]
    pub new_tags: Option<Vec<NewTagInput>>,
    // 知识点节点 ID 列表（B2 替代旧 knowledge_point_ids）
    pub knowledge_node_ids: Option<Vec<Uuid>>,
    /// 主知识点节点 ID（每题最多 1 个 primary）
    pub primary_knowledge_node_id: Option<Uuid>,
    pub space_id: Option<Uuid>,
    /// 录入方式（"manual" | "ocr" | "ai_parse"）— 仅 "ocr" 触发配额扣减
    pub input_method: Option<String>,
    /// 关联试卷 ID 列表（同步写入 paper_questions 关联表）
    #[serde(default)]
    pub paper_ids: Option<Vec<Uuid>>,
    /// AI 智能录入来源元数据：提供时后端从 ai_parse_tasks.progress.staged_questions
    /// 读取对应暂存项，完成容器关联、AI 标签写入、未匹配候选写入，并标记暂存项已保存。
    #[serde(default)]
    pub ai_meta: Option<AiCreateMeta>,
    /// 统一打标确认：建议 ID + 勾选进入候选的 unmatched.id
    #[serde(default)]
    pub ai_tagging_confirmation: Option<crate::ai::tagging::AiTaggingConfirmation>,
}

/// AI 智能录入创建来源（确认保存时携带，指向待落库的暂存项）
#[derive(Debug, Clone, Deserialize)]
pub struct AiCreateMeta {
    /// 解析任务 ID
    pub task_id: Uuid,
    /// 暂存项 index（如 `p1_i0` / `c0_i2`）
    pub staged_index: String,
}

/// 更新题目请求（B2 重构）
#[derive(Debug, Deserialize)]
pub struct UpdateQuestionRequest {
    pub stem: Option<String>,
    pub question_type: Option<QuestionType>,
    pub difficulty: Option<Difficulty>,
    pub options: Option<serde_json::Value>,
    pub correct_answer: Option<serde_json::Value>,
    pub analysis: Option<String>,
    pub metadata: Option<serde_json::Value>,
    // 配图
    pub images: Option<serde_json::Value>,
    // 复合题
    pub parent_id: Option<Uuid>,
    pub sub_order: Option<i16>,
    // 标签
    pub tag_ids: Option<Vec<Uuid>>,
    #[serde(default)]
    pub new_tags: Option<Vec<NewTagInput>>,
    pub knowledge_node_ids: Option<Vec<Uuid>>,
    pub primary_knowledge_node_id: Option<Uuid>,
    /// 关联试卷 ID 列表（同步写入 paper_questions 关联表，全量覆盖）
    #[serde(default)]
    pub paper_ids: Option<Vec<Uuid>>,
    /// 统一打标确认（编辑页 AI 打标后保存）
    #[serde(default)]
    pub ai_tagging_confirmation: Option<crate::ai::tagging::AiTaggingConfirmation>,
}

/// 自建标签输入（B2 重构：category 改为 enum，新增 parent_id 支持层级）
#[derive(Debug, Deserialize, Clone)]
pub struct NewTagInput {
    pub name: String,
    pub category: TagCategory,
    /// 可选父标签 ID（支持层级）
    pub parent_id: Option<Uuid>,
}

/// 题目列表查询参数（B2 重构：新增多知识点/多标签/范围过滤）
#[derive(Debug, Deserialize)]
pub struct QuestionQuery {
    pub status: Option<QuestionStatus>,
    pub question_type: Option<QuestionType>,
    /// 按难度精确匹配（1-5）
    pub difficulty: Option<Difficulty>,
    /// 按难度范围过滤（与 difficulty 互斥，min/max 同时存在时生效）
    pub difficulty_min: Option<i16>,
    pub difficulty_max: Option<i16>,
    /// 多知识点过滤（默认 OR 关系：命中任一即返回）
    pub knowledge_node_ids: Option<Vec<Uuid>>,
    /// 是否包含所选知识点的所有子孙节点（LTREE 子树查询，B3 实现）
    #[serde(default)]
    pub include_descendants: bool,
    /// 多标签过滤（默认 OR 关系）
    pub tag_ids: Option<Vec<Uuid>>,
    pub creator_id: Option<Uuid>,
    pub keyword: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub space_id: Option<Uuid>,
    /// 学段过滤（junior / senior）— 对应 metadata->>'stage'
    pub stage: Option<String>,
    /// 学科过滤（math / physics）— 对应 metadata->>'subject'
    pub subject: Option<String>,
    /// 仅返回当前用户可审核的待审题
    pub reviewable_by_me: Option<bool>,
    /// 待补全筛选（pending_answer / missing_analysis）— 命中 GIN 索引
    ///
    /// 使用 `@>` 包含操作符过滤 `metadata->'system_flags'`，避免 `->>` 退化为 Seq Scan。
    pub system_flag: Option<String>,

    // ── V2.1.1 来源/试卷元数据过滤（P1 检索，计划书 §五十三） ──
    /// 试卷年份（题目被某年份试卷引用）
    pub year: Option<i32>,
    /// 试卷学期（first/second/full_year）
    pub semester: Option<String>,
    /// 试卷地区（省或市匹配）
    pub region: Option<String>,
    /// 试卷来源类型
    pub source_type: Option<String>,
    /// 资料类型（题目来源 Document 的 document_type）
    pub document_type: Option<String>,
    /// 集合 ID（题目属于该集合）
    pub collection_id: Option<Uuid>,
}

/// 题目列表响应项
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct QuestionSummary {
    pub id: Uuid,
    pub stem: String,
    pub question_type: QuestionType,
    pub difficulty: Difficulty,
    pub status: QuestionStatus,
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
            status: q.status,
            creator_id: q.creator_id,
            creator_name: None,
            created_at: q.created_at,
            updated_at: q.updated_at,
            version: q.version,
            space_id: q.space_id,
        }
    }
}

/// 题目详情响应
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
    pub correct_answer: Option<serde_json::Value>,
    pub analysis: Option<String>,

    // ── 难度与评估 ──
    pub difficulty: Difficulty,

    // ── 元数据 ──
    pub metadata: serde_json::Value,

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
    /// 知识点节点列表（替代旧 knowledge_points）
    pub knowledge_nodes: Vec<KnowledgeNodeSummary>,
    /// 关联标签（核心素养 + 解题方法 + 学校 + 场景 + 易错点）
    pub tags: Vec<TagSummary>,
    pub reviewer_ids: Vec<Uuid>,
    pub can_review: bool,
}

impl From<(Question, Vec<KnowledgeNodeSummary>)> for QuestionDetail {
    fn from((q, kns): (Question, Vec<KnowledgeNodeSummary>)) -> Self {
        Self {
            id: q.id,
            stem: q.stem,
            stem_text: q.stem_text,
            images: q.images,
            question_type: q.question_type,
            options: q.options,
            correct_answer: q.correct_answer,
            analysis: q.analysis,
            difficulty: q.difficulty,
            metadata: q.metadata,
            parent_id: q.parent_id,
            sub_order: q.sub_order,
            paper_count: q.paper_count,
            attempt_count: q.attempt_count,
            accuracy_rate: q.accuracy_rate,
            favorite_count: q.favorite_count,
            status: q.status,
            space_id: q.space_id,
            origin_question_id: q.origin_question_id,
            creator_id: q.creator_id,
            creator_name: None,
            created_at: q.created_at,
            updated_by: q.updated_by,
            updated_at: q.updated_at,
            version: q.version,
            knowledge_nodes: kns,
            tags: vec![],
            reviewer_ids: vec![],
            can_review: false,
        }
    }
}

// ===========================================================================
// 异步补全：空答案检测 + 系统标签刷新
// ===========================================================================

/// 检测 correct_answer 是否为「空」
///
/// 空值定义：
/// - `None` / `Value::Null`
/// - `{"value":{"options":[]}}` 等 options/blanks/subs 为空数组
/// - `{"value":"   "}` 纯空格字符串（trim 后为空，覆盖 text/math 题型）
pub fn is_answer_empty(answer: &Option<serde_json::Value>) -> bool {
    match answer {
        None => true,
        Some(serde_json::Value::Null) => true,
        Some(v) => {
            if let Some(value) = v.get("value") {
                // 数组类答案：options/blanks/subs 为空数组
                if let Some(arr) = value.get("options").and_then(|o| o.as_array()) {
                    return arr.is_empty();
                }
                if let Some(arr) = value.get("blanks").and_then(|b| b.as_array()) {
                    return arr.is_empty();
                }
                if let Some(arr) = value.get("subs").and_then(|s| s.as_array()) {
                    return arr.is_empty();
                }
                // 字符串类答案：纯空格判定（覆盖 text/math 题型）
                if let Some(s) = value.as_str() {
                    return s.trim().is_empty();
                }
            }
            false
        }
    }
}

/// 根据答案与解析状态刷新 `metadata.system_flags`
///
/// - `pending_answer`：`is_answer_empty(answer)`
/// - `missing_analysis`：analysis 为空且 `no_analysis_needed != true`
/// - `no_analysis_needed=true` 时强制 `missing_analysis=false`（豁免，T2-7）
///
/// 注意：调用方需保证传入的 metadata 为 JSON 对象（非对象时会被重置为 `{}`）。
pub fn refresh_system_flags(
    metadata: &mut serde_json::Value,
    answer: &Option<serde_json::Value>,
    analysis: &Option<String>,
) {
    if !metadata.is_object() {
        *metadata = serde_json::json!({});
    }
    let flags = metadata
        .as_object_mut()
        .expect("metadata 已确保为对象")
        .entry("system_flags")
        .or_insert_with(|| serde_json::json!({}));

    let flags_obj = flags
        .as_object_mut()
        .expect("system_flags 应为 JSON 对象");
    let no_analysis_needed = flags_obj
        .get("no_analysis_needed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    flags_obj.insert(
        "pending_answer".to_string(),
        serde_json::Value::Bool(is_answer_empty(answer)),
    );
    let analysis_empty = analysis.as_ref().map_or(true, |s| s.trim().is_empty());
    flags_obj.insert(
        "missing_analysis".to_string(),
        serde_json::Value::Bool(analysis_empty && !no_analysis_needed),
    );
}

/// 驳回请求（reject_question 专用）
#[derive(Debug, Deserialize)]
pub struct RejectRequest {
    /// 驳回原因（可选，记录到日志便于审计追踪）
    pub reject_reason: Option<String>,
}

/// 提交审核请求
#[derive(Debug, Deserialize)]
pub struct SubmitReviewRequest {
    /// 指定审题人（团队空间必填，个人空间后端自动设为 creator）
    pub reviewer_id: Option<Uuid>,
}

/// 贡献到公共库 / 从公共导入
#[derive(Debug, Deserialize)]
pub struct TransferQuestionRequest {
    /// 导入时的目标空间；贡献到公共时可省略
    pub target_space_id: Option<Uuid>,
}

// ===========================================================================
// 推库申请（公共题库终审流程，与空间内部审核解耦）
// ===========================================================================

/// 推库申请状态
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "submission_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum SubmissionStatus {
    Pending,
    Approved,
    Rejected,
}

/// 推库申请记录
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PublicLibrarySubmission {
    pub id: Uuid,
    pub question_id: Uuid,
    pub source_space_id: Uuid,
    pub submitted_by: Uuid,
    pub status: SubmissionStatus,
    pub review_comment: Option<String>,
    pub reviewed_by: Option<Uuid>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// 推库待审列表项（JOIN 题目 + 来源空间 + 申请人）
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PublicLibrarySubmissionDetail {
    pub id: Uuid,
    pub question_id: Uuid,
    pub source_space_id: Uuid,
    pub source_space_name: String,
    pub submitted_by: Uuid,
    pub submitter_name: String,
    pub status: SubmissionStatus,
    pub review_comment: Option<String>,
    pub reviewed_by: Option<Uuid>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,

    // 题目摘要
    pub stem: String,
    pub question_type: QuestionType,
    pub difficulty: Difficulty,
}

// ===========================================================================
// 知识树与知识点（B2 全新设计，替代旧 KnowledgePoint 系列）
// ===========================================================================

/// 知识树（多树支持：知识树 / 题型专题树 / 章节树）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct KnowledgeTree {
    pub id: Uuid,
    /// 树编码（全局唯一）：math_knowledge / math_ability / math_chapter_renjiiao
    pub code: String,
    pub name: String,
    pub kind: KnowledgeTreeKind,
    /// NULL = 全局预置；非 NULL = 空间私有
    pub space_id: Option<Uuid>,
    pub version: i32,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 知识点节点（数据库行）— 物化路径 + 邻接表双轨
///
/// `path` 字段在 SQL 中是 LTREE 类型，sqlx 不直接支持 LTREE，
/// handler 层 SELECT 时需显式转换 `path::text AS path` 以用 String 接收。
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct KnowledgeNode {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub parent_id: Option<Uuid>,
    /// 节点独立 code（仅当前层级标识，如 "3" 或 "quadratic"），
    /// 层次关系由 path 字段表达
    pub code: Option<String>,
    /// LTREE 物化路径，如 'n1.n12.n123'
    pub path: String,
    pub depth: i16,
    pub name: String,
    /// 同义词数组 JSONB，如 [{"alias":"抛物线函数","locale":"zh"}]
    /// 用于 AI 智能打标的模糊匹配
    pub aliases: serde_json::Value,
    pub description: Option<String>,
    pub sort_order: i32,
    /// 反规范化缓存：关联题目数
    pub question_count: i32,
    pub is_active: bool,
    /// V2.1.1：合并目标（status=merged 时指向最终 active 标签；不物理删除）
    pub canonical_id: Option<Uuid>,
    /// V2.1.1：生命周期 pending_review/active/merged/deprecated/rejected
    pub status: String,
    /// V2.1.1：来源 system/admin/ai/import
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 知识点树节点（带 children，用于前端树形展示）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeNodeTreeNode {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub code: Option<String>,
    pub path: String,
    pub depth: i16,
    pub name: String,
    pub aliases: serde_json::Value,
    pub description: Option<String>,
    pub sort_order: i32,
    pub question_count: i32,
    /// V2.1.1
    pub canonical_id: Option<Uuid>,
    pub status: String,
    pub source: String,
    pub children: Vec<KnowledgeNodeTreeNode>,
}

impl From<KnowledgeNode> for KnowledgeNodeTreeNode {
    fn from(n: KnowledgeNode) -> Self {
        Self {
            id: n.id,
            tree_id: n.tree_id,
            parent_id: n.parent_id,
            code: n.code,
            path: n.path,
            depth: n.depth,
            name: n.name,
            aliases: n.aliases,
            description: n.description,
            sort_order: n.sort_order,
            question_count: n.question_count,
            canonical_id: n.canonical_id,
            status: n.status,
            source: n.source,
            children: vec![],
        }
    }
}

/// 知识点摘要（用于题目详情中的关联展示）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct KnowledgeNodeSummary {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub name: String,
    /// 物化路径（handler 层 SELECT 时用 `path::text`）
    pub path: String,
    pub depth: i16,
    /// 所属知识树类型（chapter / knowledge / ability）— 前端按维度着色
    #[serde(default)]
    pub kind: String,
    /// 是否主知识点（每题最多 1 个 is_primary=true）
    pub is_primary: bool,
    /// AI 匹配置信度（0.0000-1.0000）
    pub ai_confidence: Option<rust_decimal::Decimal>,
    /// 关联来源（manual / ai）
    pub source: KnowledgeLinkSource,
}

/// 题目-知识点关联（数据库行）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct QuestionKnowledgeNode {
    pub question_id: Uuid,
    pub node_id: Uuid,
    pub is_primary: bool,
    /// 相关度评分 0-100
    pub relevance_score: Option<i16>,
    /// AI 匹配置信度（0.0000-1.0000）
    pub ai_confidence: Option<rust_decimal::Decimal>,
    pub source: KnowledgeLinkSource,
    pub created_at: DateTime<Utc>,
}

/// 创建知识树请求
#[derive(Debug, Deserialize)]
pub struct CreateKnowledgeTreeRequest {
    pub code: String,
    pub name: String,
    pub kind: Option<KnowledgeTreeKind>,
    pub space_id: Option<Uuid>,
    pub description: Option<String>,
}

/// 更新知识树请求
#[derive(Debug, Deserialize)]
pub struct UpdateKnowledgeTreeRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
}

/// 创建知识点节点请求
#[derive(Debug, Deserialize)]
pub struct CreateKnowledgeNodeRequest {
    pub tree_id: Uuid,
    pub parent_id: Option<Uuid>,
    /// 节点独立 code（仅当前层级标识，不含父级路径段）
    pub code: Option<String>,
    pub name: String,
    /// 同义词数组，如 ["抛物线函数", "quadratic_function"]
    #[serde(default)]
    pub aliases: Option<serde_json::Value>,
    pub description: Option<String>,
    pub sort_order: Option<i32>,
}

/// 更新知识点节点请求
#[derive(Debug, Deserialize)]
pub struct UpdateKnowledgeNodeRequest {
    pub name: Option<String>,
    pub code: Option<String>,
    pub aliases: Option<serde_json::Value>,
    pub description: Option<String>,
    pub sort_order: Option<i32>,
    pub is_active: Option<bool>,
}

/// 移动知识点节点请求（改 parent_id，后端重算 path 与 depth）
#[derive(Debug, Deserialize)]
pub struct MoveKnowledgeNodeRequest {
    pub new_parent_id: Option<Uuid>,
}

// ===========================================================================
// 审核记录
// ===========================================================================

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ReviewRecord {
    pub id: Uuid,
    pub question_id: Uuid,
    pub reviewer_id: Uuid,
    pub action: String,
    pub comment: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ===========================================================================
// 标签（B2 重构：增加层级 + 枚举 category + aliases）
// ===========================================================================

/// 标签摘要（用于题目详情中的关联展示）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TagSummary {
    pub id: Uuid,
    pub name: String,
    pub category: TagCategory,
}

/// 标签（数据库行，B2 支持层级 + aliases）
///
/// `path` 字段在 SQL 中是 LTREE 类型，handler 层 SELECT 时需 `path::text AS path`。
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Tag {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub category: TagCategory,
    /// LTREE 物化路径
    pub path: String,
    /// 同义词数组 JSONB
    pub aliases: serde_json::Value,
    pub description: Option<String>,
    pub space_id: Option<Uuid>,
    pub use_count: i32,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

/// 标签树节点（带 children，用于前端树形展示）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagTreeNode {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub category: TagCategory,
    pub path: String,
    pub aliases: serde_json::Value,
    pub use_count: i32,
    pub children: Vec<TagTreeNode>,
}

impl From<Tag> for TagTreeNode {
    fn from(t: Tag) -> Self {
        Self {
            id: t.id,
            parent_id: t.parent_id,
            name: t.name,
            category: t.category,
            path: t.path,
            aliases: t.aliases,
            use_count: t.use_count,
            children: vec![],
        }
    }
}

/// 创建标签请求
#[derive(Debug, Deserialize)]
pub struct CreateTagRequest {
    pub name: String,
    pub category: TagCategory,
    /// 可选父标签 ID（支持层级）
    pub parent_id: Option<Uuid>,
    #[serde(default)]
    pub aliases: Option<serde_json::Value>,
    pub description: Option<String>,
    pub space_id: Option<Uuid>,
}

/// 更新标签请求（部分更新；不允许修改 category 以保证树一致性）
#[derive(Debug, Deserialize)]
pub struct UpdateTagRequest {
    pub name: Option<String>,
    pub aliases: Option<serde_json::Value>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
}

/// 移动标签请求（改 parent_id，后端重算 path）
#[derive(Debug, Deserialize)]
pub struct MoveTagRequest {
    pub new_parent_id: Option<Uuid>,
}

/// 标签查询参数（B2 增强：支持树形返回与按 parent_id 过滤）
#[derive(Debug, Deserialize)]
pub struct TagQuery {
    pub category: Option<TagCategory>,
    pub space_id: Option<Uuid>,
    /// 是否返回树形结构（带 children）；false 时返回平铺列表
    #[serde(default)]
    pub as_tree: bool,
    /// 按父节点过滤（NULL = 仅根节点）
    pub parent_id: Option<Uuid>,
}
