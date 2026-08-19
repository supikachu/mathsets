//! AI 智能打标 API（两阶段 RAG 分类重构）
//!
//! 核心流程（三步走、三维度）：
//! 1. **阶段一（发散提词）**：LLM 轻量提取 —— 从题干/解析分别提取
//!    【章节】【知识点】【解题方法】三维度的核心考点关键词（chapter_keys /
//!    knowledge_keys / method_keys），同时提取难度/题型/年级/认知层次。
//! 2. **阶段二（三维并发召回）**：Rust 后端用 pg_trgm 对三个维度做**独立隔离**
//!    的模糊检索（按 knowledge_trees.kind 严格隔离：chapter / knowledge /
//!    ability），各自截取 Top N（章节 10 / 知识点 20 / 解题方法 15），
//!    并设置 similarity >= 0.3 的底线阈值避免劣质召回。
//! 3. **阶段三（精准收敛）**：第二次 LLM 调用 —— 把三份候选菜单与题目内容
//!    一并交给模型做"选择题"：只能输出候选列表中存在的名称，严禁编造；
//!    某维度无合适候选则留空。后端把选择结果映射回真实节点 UUID 后
//!    打包 AiTaggingResponse 返回前端。
//!
//! 设计动机：知识树庞大且分三维度，直接让 LLM 生成标签会产生"幻觉造词"；
//! 候选召回把搜索空间收敛到真实节点，LLM 只做分类选择，既消灭幻觉又节约 Token。

use axum::{
    extract::{Extension, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::ai::cleaner::clean_and_parse;
use crate::ai::provider::create_provider;
use crate::auth::middleware::AuthUser;
use crate::auth::permissions::{can_access_space, get_space};
use crate::handlers::ai::{map_ai_error, resolve_ai_config, ModelKind};
use crate::AppState;

// ===========================================================================
// 阶段一 Prompt：发散提词（轻量提取三维关键词 + 元数据）
// ===========================================================================

const AI_EXTRACT_KEYS_PROMPT: &str = r#"你是一名数学教研专家。请阅读题目与解析，提取核心考点关键词，并判断难度与题型。

**输出格式（严格 JSON，不要 markdown 代码块，不要任何解释文字）**：
{
  "chapter_keys": ["章节关键词1", "章节关键词2"],
  "knowledge_keys": ["知识点关键词1", "知识点关键词2"],
  "method_keys": ["方法关键词1"],
  "core_competencies": ["核心素养1"],
  "difficulty": 1到5的整数,
  "question_type": "choice" | "multiple" | "fill" | "solution",
  "grade_level": "grade_7" | "grade_8" | "grade_9" | "grade_10" | "grade_11" | "grade_12" | "other",
  "cognitive_level": "remember" | "understand" | "apply" | "analyze" | "evaluate" | "create"
}

**三维关键词规则**（这是为后续数据库精确匹配服务的，命名要贴近教材术语）：
1. chapter_keys: 2-6 个，对应教材【章节】级概念（如"函数"、"导数及其应用"、"三角函数与解三角形"）
2. knowledge_keys: 3-8 个，对应具体【知识点】（如"二次函数的最值"、"利用导数研究函数的单调性"、"正弦定理"）
3. method_keys: 0-5 个，对应【解题方法/数学思想】（如"数形结合"、"配方法"、"分类讨论"、"换元法"、"构造法"）
4. core_competencies: 从"数学抽象"、"逻辑推理"、"数学建模"、"直观想象"、"数学运算"、"数据分析"中选 0-3 个
5. difficulty: 1=极易, 2=容易, 3=中等, 4=较难, 5=极难
6. question_type: choice=单选, multiple=多选, fill=填空, solution=解答（含证明、计算）
7. grade_level: grade_7~grade_12 对应初一~高三, other=跨年级或不明确
8. cognitive_level: remember/understand/apply/analyze/evaluate/create（布鲁姆层次）

**示例**：
输入："已知函数 f(x)=x³-3x，求 f(x) 的单调区间。"
输出：{"chapter_keys":["导数及其应用","函数"],"knowledge_keys":["利用导数研究函数的单调性","函数的单调性"],"method_keys":["数形结合","分类讨论"],"core_competencies":["数学运算","逻辑推理"],"difficulty":3,"question_type":"solution","grade_level":"grade_12","cognitive_level":"apply"}

**重要**：
- 关键词用简洁标准的教材术语，宁少勿滥；无法识别时对应字段返回空数组 []
- 只输出 JSON，不要任何 markdown 标记或额外文字"#;

// ===========================================================================
// 阶段三 Prompt：精准收敛（严格分类器，只能选候选列表内名称）
// ===========================================================================

const AI_CONVERGE_PROMPT: &str = r#"你是一个严格的标签分类器。题目内容与三份候选列表将一并提供。

**硬性规则（必须严格遵守）**：
1. 你必须且只能输出候选列表中【完整原名】的标签（名称与候选列表逐字一致）。
2. 严禁输出任何候选列表之外的词汇 —— 不存在则留空，绝不编造、绝不改写、绝不拼接。
3. 每个维度最多选择 3 个，按匹配程度从高到低排序。
4. 若某维度候选列表为空或没有合适的项，该维度返回空数组 []。

**输出格式（严格 JSON，不要 markdown 代码块）**：
{
  "chapter_names": ["章节候选原名1"],
  "knowledge_names": ["知识点候选原名1", "知识点候选原名2"],
  "method_names": ["方法候选原名1"]
}"#;

// ===========================================================================
// 请求/响应类型
// ===========================================================================

#[derive(Debug, Deserialize)]
pub struct AiTaggingRequest {
    /// 题目文本（题干 + 选项 + 答案 + 解析，越完整越准确）
    pub content: String,
    /// 可选空间 ID（限定在该空间的知识树 + 全局树内匹配）
    pub space_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct AiTaggingResponse {
    /// 匹配成功的知识树节点列表（章节/知识点/解题方法三维合一，
    /// 前端按 tree_id → kind 自动分发到三个 Tab）
    pub knowledge_nodes: Vec<KnowledgeNodeMatch>,
    /// 匹配成功的核心素养标签列表（tags 表 core_competence）
    pub competency_tags: Vec<TagMatch>,
    /// 匹配成功的解题方法标签列表（tags 表 method，作为树维度的补充）
    pub method_tags: Vec<TagMatch>,
    /// AI 推断的难度（1-5）
    pub difficulty: Option<i16>,
    /// AI 推断的题型
    pub question_type: Option<String>,
    /// AI 推断的年级
    pub grade_level: Option<String>,
    /// AI 推断的认知层次
    pub cognitive_level: Option<String>,
    /// 未匹配名称（两阶段收敛后恒为空，保留字段兼容前端）
    pub unmatched_knowledge_points: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct KnowledgeNodeMatch {
    /// AI 原始名称（收敛选择阶段的候选名）
    pub ai_name: String,
    /// 匹配到的知识点节点 UUID
    pub node_id: Uuid,
    /// 匹配到的知识点名称
    pub node_name: String,
    /// 所属知识树 ID
    pub tree_id: Uuid,
    /// 物化路径（前端可用于展示层级）
    pub path: String,
    /// 节点深度
    pub depth: i16,
    /// 匹配置信度（0.0-1.0，来自阶段二召回相似度）
    pub score: f32,
    /// 匹配类型：exact / alias / fuzzy / selected
    pub match_type: String,
}

/// AI 打标标签匹配结果（核心素养 / 解题方法，tags 表）
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TagMatch {
    /// AI 返回的原始名称
    pub ai_name: String,
    /// 匹配到的标签 UUID
    pub tag_id: Uuid,
    /// 匹配到的标签名称
    pub tag_name: String,
    /// 标签类别（core_competence / method）
    pub category: String,
    /// 匹配置信度（0.0-1.0）
    pub score: f32,
    /// 匹配类型：exact / alias / fuzzy
    pub match_type: String,
}

// ===========================================================================
// 两阶段内部数据结构
// ===========================================================================

/// 阶段一：LLM 发散提取结果（三维关键词 + 元数据）
#[derive(Debug, Deserialize)]
struct AiExtractResult {
    #[serde(default)]
    chapter_keys: Vec<String>,
    #[serde(default)]
    knowledge_keys: Vec<String>,
    #[serde(default)]
    method_keys: Vec<String>,
    #[serde(default)]
    core_competencies: Vec<String>,
    difficulty: Option<i16>,
    question_type: Option<String>,
    grade_level: Option<String>,
    cognitive_level: Option<String>,
}

/// 阶段三：LLM 收敛选择结果（只能含候选列表中的原名）
#[derive(Debug, Deserialize)]
struct AiConvergeResult {
    #[serde(default)]
    chapter_names: Vec<String>,
    #[serde(default)]
    knowledge_names: Vec<String>,
    #[serde(default)]
    method_names: Vec<String>,
}

/// 阶段二：数据库召回候选节点
#[derive(Debug, Clone)]
struct NodeCandidate {
    id: Uuid,
    name: String,
    tree_id: Uuid,
    path: String,
    depth: i16,
    score: f32,
    match_type: String,
}

// ===========================================================================
// Handler（三步走流水线）
// ===========================================================================

/// POST /api/v1/questions/ai-tagging — AI 智能打标（两阶段 RAG 分类）
pub async fn ai_tagging(
    Extension(auth): Extension<AuthUser>,
    State(state): State<AppState>,
    Json(req): Json<AiTaggingRequest>,
) -> Result<Json<AiTaggingResponse>, (StatusCode, Json<serde_json::Value>)> {
    if req.content.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "题目文本不能为空"})),
        ));
    }

    // 权限：space_id 提供时必须是当前用户可访问的空间
    // （防枚举他人空间的知识树节点名称，轻微只读元数据泄露）
    if let Some(space_id) = req.space_id {
        let space = get_space(&state.pool, space_id)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": format!("查询空间失败: {e}")})),
                )
            })?
            .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;
        if !can_access_space(&state.pool, &auth, &space)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": format!("权限检查失败: {e}")})),
                )
            })?
        {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({"error": "无权访问该空间"})),
            ));
        }
    }

    // ── 0. 解析 AI 配置（用户个人 Key > 平台默认 Key） ─────────────────
    let (api_key, provider_name, model, base_url) =
        resolve_ai_config(&auth, &state, ModelKind::Text)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({"error": e}))))?;
    let provider = create_provider(&provider_name, &api_key, &base_url);

    // ── 阶段一：发散提词（第一次 LLM 调用，轻量） ──────────────────────
    let raw1 = provider
        .parse_text_with_prompt(&req.content, AI_EXTRACT_KEYS_PROMPT, model.as_deref())
        .await
        .map_err(map_ai_error)?;
    let extract: AiExtractResult = clean_and_parse(&raw1).map_err(|e| {
        tracing::warn!("AI 打标-阶段一解析失败: {e}");
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": format!("AI 返回格式损坏: {e}")})),
        )
    })?;

    tracing::info!(
        "AI 打标-阶段一: 章节 {:?}, 知识点 {:?}, 方法 {:?}, 难度 {:?}",
        extract.chapter_keys,
        extract.knowledge_keys,
        extract.method_keys,
        extract.difficulty
    );

    // ── 阶段二：三维并发召回（pg_trgm 隔离检索，互不干扰） ────────────
    // 维度 → knowledge_trees.kind 映射：章节 chapter / 知识点 knowledge /
    // 解题方法 ability（math_method_* 树）；各维度 Top N：10 / 20 / 15
    let (chapters, knowledges, methods) = tokio::join!(
        recall_candidates(&state.pool, &extract.chapter_keys, "chapter", 10, req.space_id),
        recall_candidates(&state.pool, &extract.knowledge_keys, "knowledge", 20, req.space_id),
        recall_candidates(&state.pool, &extract.method_keys, "ability", 15, req.space_id),
    );
    let (chapters, knowledges, methods) = (chapters?, knowledges?, methods?);

    tracing::info!(
        "AI 打标-阶段二: 章节候选 {} 个, 知识点候选 {} 个, 方法候选 {} 个",
        chapters.len(),
        knowledges.len(),
        methods.len()
    );

    // ── 阶段三：精准收敛（第二次 LLM 调用，做选择题） ──────────────────
    let menu = build_candidate_menu(&chapters, &knowledges, &methods);
    let converge_payload = format!("【题目内容】\n{}\n\n{}", req.content, menu);
    let raw2 = provider
        .parse_text_with_prompt(&converge_payload, AI_CONVERGE_PROMPT, model.as_deref())
        .await
        .map_err(map_ai_error)?;

    // 收敛结果解析：失败则降级为每维度召回 Top1（保底不中断打标）
    let converge: AiConvergeResult = match clean_and_parse(&raw2) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("AI 打标-阶段三解析失败，降级为召回 Top1: {e}");
            AiConvergeResult {
                chapter_names: chapters.first().map(|c| c.name.clone()).into_iter().collect(),
                knowledge_names: knowledges
                    .first()
                    .map(|c| c.name.clone())
                    .into_iter()
                    .collect(),
                method_names: methods.first().map(|c| c.name.clone()).into_iter().collect(),
            }
        }
    };

    // 把选择结果映射回候选节点（名称精确匹配候选，找不到即幻觉 → 丢弃）
    let mut knowledge_nodes: Vec<KnowledgeNodeMatch> = Vec::new();
    for c in resolve_selection(&chapters, &converge.chapter_names)
        .into_iter()
        .chain(resolve_selection(&knowledges, &converge.knowledge_names))
        .chain(resolve_selection(&methods, &converge.method_names))
    {
        knowledge_nodes.push(KnowledgeNodeMatch {
            ai_name: c.name.clone(),
            node_id: c.id,
            node_name: c.name.clone(),
            tree_id: c.tree_id,
            path: c.path.clone(),
            depth: c.depth,
            score: c.score,
            match_type: c.match_type.clone(),
        });
    }

    // 核心素养（tags 表 core_competence）与解题方法补充标签（tags 表 method）
    let (competency_tags, _) = match_tags(
        &state.pool,
        &extract.core_competencies,
        "core_competence",
        req.space_id,
    )
    .await?;
    let (method_tags, _) =
        match_tags(&state.pool, &extract.method_keys, "method", req.space_id).await?;

    Ok(Json(AiTaggingResponse {
        knowledge_nodes,
        competency_tags,
        method_tags,
        difficulty: extract.difficulty,
        question_type: extract.question_type,
        grade_level: extract.grade_level,
        cognitive_level: extract.cognitive_level,
        unmatched_knowledge_points: vec![],
    }))
}

// ===========================================================================
// 阶段二：三维隔离召回
// ===========================================================================

/// 对一组关键词在指定维度（knowledge_trees.kind）的节点库中做 pg_trgm
/// 模糊召回：逐关键词查询（exact > alias > fuzzy，相似度底线 0.3），
/// 合并去重（同节点保留最高分），按 score 降序截取 Top `limit`。
///
/// 维度隔离：`kt.kind::text = $3` —— chapter / knowledge / ability
/// （解题方法树 math_method_* 的 kind 为 ability）。
async fn recall_candidates(
    pool: &sqlx::PgPool,
    keys: &[String],
    kind: &str,
    limit: usize,
    space_id: Option<Uuid>,
) -> Result<Vec<NodeCandidate>, (StatusCode, Json<serde_json::Value>)> {
    if keys.is_empty() {
        return Ok(vec![]);
    }

    // id → 候选（合并去重，保留最高分）
    let mut merged: std::collections::HashMap<Uuid, NodeCandidate> = std::collections::HashMap::new();

    for key in keys {
        let key = key.trim();
        if key.is_empty() {
            continue;
        }

        // 单条 SQL：exact / alias / fuzzy 三优先级，维度严格隔离 + 相似度底线
        let rows: Vec<NodeCandidate> = sqlx::query_as::<_, (Uuid, String, Uuid, String, i16, f32, String)>(
            r#"
            SELECT
              kn.id,
              kn.name,
              kn.tree_id,
              kn.path::text,
              kn.depth,
              CASE
                WHEN kn.name = $1 THEN 1.0::real
                WHEN EXISTS (
                  SELECT 1 FROM jsonb_array_elements(kn.aliases) AS a
                  WHERE a->>'alias' = $1
                ) THEN 0.95::real
                ELSE similarity(kn.name::text, $1)
              END AS score,
              CASE
                WHEN kn.name = $1 THEN 'exact'
                WHEN EXISTS (
                  SELECT 1 FROM jsonb_array_elements(kn.aliases) AS a
                  WHERE a->>'alias' = $1
                ) THEN 'alias'
                ELSE 'fuzzy'
              END AS match_type
            FROM knowledge_nodes kn
            JOIN knowledge_trees kt ON kt.id = kn.tree_id
            WHERE kn.is_active = TRUE
              AND kt.is_active = TRUE
              AND (kt.space_id IS NULL OR kt.space_id = $2)
              AND kt.kind::text = $3
              -- 叶子节点约束：题库打标必须最细粒度，召回 100% 无子节点的底层节点
              AND NOT EXISTS (
                SELECT 1 FROM knowledge_nodes child
                WHERE child.parent_id = kn.id AND child.is_active = TRUE
              )
              AND (
                kn.name = $1
                OR EXISTS (
                  SELECT 1 FROM jsonb_array_elements(kn.aliases) AS a
                  WHERE a->>'alias' = $1
                )
                OR (kn.name::text % $1 AND similarity(kn.name::text, $1) >= 0.3)
              )
            ORDER BY score DESC, kn.depth ASC
            LIMIT 30
            "#,
        )
        .bind(key)
        .bind(space_id)
        .bind(kind)
        .fetch_all(pool)
        .await
        .map_err(|e| {
            tracing::warn!("维度「{}」候选召回失败: {e}", kind);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("候选召回失败: {e}")})),
            )
        })?
        .into_iter()
        .map(
            |(id, name, tree_id, path, depth, score, match_type)| NodeCandidate {
                id,
                name,
                tree_id,
                path,
                depth,
                score,
                match_type,
            },
        )
        .collect();

        for c in rows {
            match merged.get(&c.id) {
                Some(existing) if existing.score >= c.score => {}
                _ => {
                    merged.insert(c.id, c);
                }
            }
        }
    }

    // 按 score 降序截取 Top N
    let mut all: Vec<NodeCandidate> = merged.into_values().collect();
    all.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    all.truncate(limit);
    Ok(all)
}

// ===========================================================================
// 阶段三：候选菜单组装 + 选择解析
// ===========================================================================

/// 把三份候选列表组装成菜单文本（供阶段三 LLM 收敛选择）
fn build_candidate_menu(
    chapters: &[NodeCandidate],
    knowledges: &[NodeCandidate],
    methods: &[NodeCandidate],
) -> String {
    fn section(title: &str, candidates: &[NodeCandidate]) -> String {
        if candidates.is_empty() {
            return format!("【{}候选列表】（无候选）\n", title);
        }
        let lines: Vec<String> = candidates
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{}. {}（相似度 {:.2}）", i + 1, c.name, c.score))
            .collect();
        format!("【{}候选列表】\n{}\n", title, lines.join("\n"))
    }

    format!(
        "{}{}{}",
        section("章节", chapters),
        section("知识点", knowledges),
        section("解题方法", methods)
    )
}

/// 把 LLM 收敛选择的名称映射回候选节点（名称精确匹配；候选外名称 = 幻觉 → 丢弃）
fn resolve_selection<'a>(
    candidates: &'a [NodeCandidate],
    names: &[String],
) -> Vec<&'a NodeCandidate> {
    let mut out = Vec::new();
    for name in names {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        if let Some(c) = candidates.iter().find(|c| c.name == name) {
            if !out.iter().any(|x: &&NodeCandidate| x.id == c.id) {
                out.push(c);
            }
        } else {
            tracing::debug!("收敛选择「{}」不在候选列表，视为幻觉丢弃", name);
        }
    }
    out
}

// ===========================================================================
// 保留函数（供 AI 解析流程 / 兼容复用）
// ===========================================================================

/// 对 AI 返回的每个知识点名称，在 knowledge_nodes 表中找最佳匹配
/// （供 ai.rs 的 AI 智能录题后处理链路使用，不受两阶段重构影响）
/// 在指定维度的知识树中匹配 AI 提取的标签名
///
/// - `tree_kind`：knowledge_trees.kind（'chapter' 章节 / 'knowledge' 知识点 /
///   'ability' 解题方法）。按维度隔离检索，杜绝跨树错配
///   （如知识点「集合的交集」模糊命中章节树「集合的分类」）
/// - `space_id`：None 仅搜全局树；Some(sid) 同时搜全局 + 该空间树
///   （worker 落库应传题目所属空间，个人空间自建节点才能被召回）
pub(crate) async fn match_knowledge_nodes(
    pool: &sqlx::PgPool,
    ai_names: &[String],
    space_id: Option<Uuid>,
    tree_kind: &str,
) -> Result<
    (Vec<KnowledgeNodeMatch>, Vec<String>),
    (StatusCode, Json<serde_json::Value>),
> {
    if ai_names.is_empty() {
        return Ok((vec![], vec![]));
    }

    let mut matched = Vec::with_capacity(ai_names.len());
    let mut unmatched = Vec::new();

    for name in ai_names {
        if name.trim().is_empty() {
            continue;
        }

        let result: Option<KnowledgeNodeMatch> = sqlx::query_as(
            r#"
            SELECT
              $1::text AS ai_name,
              kn.id AS node_id,
              kn.name AS node_name,
              kn.tree_id AS tree_id,
              kn.path::text AS path,
              kn.depth AS depth,
              CASE
                WHEN kn.name = $1 THEN 1.0::real
                WHEN EXISTS (
                  SELECT 1 FROM jsonb_array_elements(kn.aliases) AS a
                  WHERE a->>'alias' = $1
                ) THEN 0.95::real
                ELSE similarity(kn.name::text, $1)
              END AS score,
              CASE
                WHEN kn.name = $1 THEN 'exact'
                WHEN EXISTS (
                  SELECT 1 FROM jsonb_array_elements(kn.aliases) AS a
                  WHERE a->>'alias' = $1
                ) THEN 'alias'
                ELSE 'fuzzy'
              END AS match_type
            FROM knowledge_nodes kn
            JOIN knowledge_trees kt ON kt.id = kn.tree_id
            WHERE kn.is_active = TRUE
              AND kt.is_active = TRUE
              AND kt.kind = $3::knowledge_tree_kind
              AND (kt.space_id IS NULL OR kt.space_id = $2)
              -- 叶子节点约束：与 recall_candidates 保持一致，防止选中父节点导致前端级联冲突
              AND NOT EXISTS (
                SELECT 1 FROM knowledge_nodes child
                WHERE child.parent_id = kn.id AND child.is_active = TRUE
              )
              AND (
                kn.name = $1
                OR EXISTS (
                  SELECT 1 FROM jsonb_array_elements(kn.aliases) AS a
                  WHERE a->>'alias' = $1
                )
                OR kn.name::text % $1
              )
            ORDER BY score DESC
            LIMIT 1
            "#,
        )
        .bind(name)
        .bind(space_id)
        .bind(tree_kind)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            tracing::warn!("[{tree_kind}] 标签匹配查询失败: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("标签匹配查询失败: {e}")})),
            )
        })?;

        match result {
            Some(m) if m.score >= 0.3 => {
                tracing::debug!(
                    "[{tree_kind}] 「{}」匹配到「{}」(score={:.2}, type={})",
                    m.ai_name,
                    m.node_name,
                    m.score,
                    m.match_type
                );
                matched.push(m);
            }
            _ => {
                tracing::debug!("[{tree_kind}] 「{}」未找到匹配", name);
                unmatched.push(name.clone());
            }
        }
    }

    Ok((matched, unmatched))
}

/// 对 AI 返回的每个标签名称（核心素养 / 解题方法），在 tags 表中找最佳匹配
/// （匹配策略与 match_knowledge_nodes 一致：exact > alias > fuzzy）
pub(crate) async fn match_tags(
    pool: &sqlx::PgPool,
    ai_names: &[String],
    category: &str,
    space_id: Option<Uuid>,
) -> Result<
    (Vec<TagMatch>, Vec<String>),
    (StatusCode, Json<serde_json::Value>),
> {
    if ai_names.is_empty() {
        return Ok((vec![], vec![]));
    }

    let mut matched = Vec::with_capacity(ai_names.len());

    for name in ai_names {
        if name.trim().is_empty() {
            continue;
        }

        let result: Option<TagMatch> = sqlx::query_as(
            r#"
            SELECT
              $1::text AS ai_name,
              t.id AS tag_id,
              t.name AS tag_name,
              t.category::text AS category,
              CASE
                WHEN t.name = $1 THEN 1.0::real
                WHEN EXISTS (
                  SELECT 1 FROM jsonb_array_elements(t.aliases) AS a
                  WHERE a->>'alias' = $1
                ) THEN 0.95::real
                ELSE similarity(t.name::text, $1)
              END AS score,
              CASE
                WHEN t.name = $1 THEN 'exact'
                WHEN EXISTS (
                  SELECT 1 FROM jsonb_array_elements(t.aliases) AS a
                  WHERE a->>'alias' = $1
                ) THEN 'alias'
                ELSE 'fuzzy'
              END AS match_type
            FROM tags t
            WHERE t.is_active = TRUE
              AND t.category::text = $2
              AND (t.space_id IS NULL OR t.space_id = $3)
              AND (
                t.name = $1
                OR EXISTS (
                  SELECT 1 FROM jsonb_array_elements(t.aliases) AS a
                  WHERE a->>'alias' = $1
                )
                OR t.name::text % $1
              )
            ORDER BY score DESC
            LIMIT 1
            "#,
        )
        .bind(name)
        .bind(category)
        .bind(space_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            tracing::warn!("标签匹配查询失败: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("标签匹配查询失败: {e}")})),
            )
        })?;

        match result {
            Some(m) if m.score >= 0.3 => {
                tracing::debug!(
                    "标签「{}」匹配到「{}」(score={:.2}, type={})",
                    m.ai_name,
                    m.tag_name,
                    m.score,
                    m.match_type
                );
                matched.push(m);
            }
            _ => {
                tracing::debug!("标签「{}」未找到匹配（category={}）", name, category);
            }
        }
    }

    Ok((matched, vec![]))
}

// ===========================================================================
// 单元测试
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_prompt_contains_three_dimension_keys() {
        assert!(AI_EXTRACT_KEYS_PROMPT.contains("chapter_keys"));
        assert!(AI_EXTRACT_KEYS_PROMPT.contains("knowledge_keys"));
        assert!(AI_EXTRACT_KEYS_PROMPT.contains("method_keys"));
        assert!(AI_EXTRACT_KEYS_PROMPT.contains("difficulty"));
        assert!(AI_EXTRACT_KEYS_PROMPT.contains("question_type"));
        assert!(AI_EXTRACT_KEYS_PROMPT.contains("grade_level"));
        assert!(AI_EXTRACT_KEYS_PROMPT.contains("cognitive_level"));
        // 示例验证
        assert!(AI_EXTRACT_KEYS_PROMPT.contains("导数及其应用"));
    }

    #[test]
    fn test_converge_prompt_contains_strict_rules() {
        // 关键防幻觉规则必须存在
        assert!(AI_CONVERGE_PROMPT.contains("严禁输出任何候选列表之外的词汇"));
        assert!(AI_CONVERGE_PROMPT.contains("空数组"));
        assert!(AI_CONVERGE_PROMPT.contains("chapter_names"));
        assert!(AI_CONVERGE_PROMPT.contains("knowledge_names"));
        assert!(AI_CONVERGE_PROMPT.contains("method_names"));
    }

    #[test]
    fn test_extract_result_deserialize() {
        let json_str = r#"{
            "chapter_keys": ["函数", "导数及其应用"],
            "knowledge_keys": ["二次函数最值"],
            "method_keys": ["数形结合"],
            "core_competencies": ["数学运算"],
            "difficulty": 2,
            "question_type": "solution",
            "grade_level": "grade_9",
            "cognitive_level": "apply"
        }"#;
        let r: AiExtractResult = serde_json::from_str(json_str).unwrap();
        assert_eq!(r.chapter_keys.len(), 2);
        assert_eq!(r.knowledge_keys, vec!["二次函数最值".to_string()]);
        assert_eq!(r.method_keys, vec!["数形结合".to_string()]);
        assert_eq!(r.difficulty, Some(2));
    }

    #[test]
    fn test_extract_result_missing_fields_default_empty() {
        // 阶段一 LLM 可能缺字段，serde default 必须兜底为空数组
        let json_str = r#"{"difficulty": 3}"#;
        let r: AiExtractResult = serde_json::from_str(json_str).unwrap();
        assert!(r.chapter_keys.is_empty());
        assert!(r.knowledge_keys.is_empty());
        assert!(r.method_keys.is_empty());
        assert!(r.core_competencies.is_empty());
    }

    #[test]
    fn test_converge_result_deserialize() {
        let json_str = r#"{
            "chapter_names": ["函数"],
            "knowledge_names": ["二次函数最值"],
            "method_names": []
        }"#;
        let r: AiConvergeResult = serde_json::from_str(json_str).unwrap();
        assert_eq!(r.chapter_names, vec!["函数".to_string()]);
        assert!(r.method_names.is_empty());
    }

    fn fake_candidate(name: &str, score: f32) -> NodeCandidate {
        NodeCandidate {
            id: Uuid::new_v4(),
            name: name.to_string(),
            tree_id: Uuid::new_v4(),
            path: "a.b".into(),
            depth: 1,
            score,
            match_type: "fuzzy".into(),
        }
    }

    #[test]
    fn test_build_candidate_menu_format() {
        let chapters = vec![fake_candidate("函数", 0.85), fake_candidate("集合", 0.72)];
        let menu = build_candidate_menu(&chapters, &[], &[]);
        assert!(menu.contains("【章节候选列表】"));
        assert!(menu.contains("1. 函数（相似度 0.85）"));
        assert!(menu.contains("【知识点候选列表】（无候选）"));
        assert!(menu.contains("【解题方法候选列表】（无候选）"));
    }

    #[test]
    fn test_resolve_selection_only_accepts_candidates() {
        let chapters = vec![fake_candidate("函数", 0.85), fake_candidate("集合", 0.72)];
        // 幻觉名称必须被丢弃
        let picked = resolve_selection(&chapters, &["函数".to_string(), "不存在的知识点".to_string()]);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].name, "函数");
        // 空选择
        assert!(resolve_selection(&chapters, &[]).is_empty());
        // 去重
        let picked2 = resolve_selection(&chapters, &["函数".to_string(), " 函数 ".to_string()]);
        assert_eq!(picked2.len(), 1);
    }
}
