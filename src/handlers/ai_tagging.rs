//! AI 智能打标 API（B3 新增）
//!
//! 核心流程：
//! 1. 接收题目文本 content
//! 2. 调用 LLM 提取 knowledge_points 名称、difficulty、question_type、grade_level、cognitive_level
//! 3. 用 PostgreSQL pg_trgm + JSONB aliases 模糊匹配 knowledge_nodes 表中真实的 UUID
//!    匹配优先级：exact（精确）> alias（同义词）> fuzzy（trgm 相似度）
//! 4. 返回 node_ids + 难度 + 题型给前端，前端自动选中
//!
//! 关键 SQL：
//! ```sql
//! SELECT
//!   CASE
//!     WHEN name = $1 THEN 1.0
//!     WHEN EXISTS (aliases @> '[{"alias":"..."}]'::jsonb) THEN 0.95
//!     ELSE similarity(name, $1)
//!   END AS score
//! FROM knowledge_nodes
//! WHERE name = $1 OR aliases @> ... OR name % $1
//! ```

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
use crate::handlers::ai::{map_ai_error, resolve_ai_config, ModelKind};
use crate::AppState;

// ===========================================================================
// AI Prompt
// ===========================================================================

const AI_TAGGING_SYSTEM_PROMPT: &str = r#"你是一名数学教研专家。请分析题目文本，提取教研标签信息。

**输出格式（严格 JSON，不要 markdown 代码块，不要任何解释文字）**：
{
  "knowledge_points": ["知识点1", "知识点2"],
  "difficulty": 1到5的整数,
  "question_type": "choice" | "multiple" | "fill" | "solution",
  "grade_level": "grade_7" | "grade_8" | "grade_9" | "grade_10" | "grade_11" | "grade_12" | "other",
  "cognitive_level": "remember" | "understand" | "apply" | "analyze" | "evaluate" | "create"
}

**字段规则**：
1. knowledge_points: 从题目中识别 1-5 个核心知识点，用中文名称，按重要度排序（最重要的在前）
2. difficulty: 1=极易, 2=容易, 3=中等, 4=较难, 5=极难
3. question_type:
   - choice = 单选题
   - multiple = 多选题
   - fill = 填空题
   - solution = 解答题（含证明题、计算题）
4. grade_level:
   - grade_7 = 初一, grade_8 = 初二, grade_9 = 初三
   - grade_10 = 高一, grade_11 = 高二, grade_12 = 高三
   - other = 跨年级或不明确
5. cognitive_level: 布鲁姆认知层次
   - remember = 记忆, understand = 理解, apply = 应用
   - analyze = 分析, evaluate = 评价, create = 创造

**示例**：
输入："求二次函数 y=x²-2x-3 的顶点坐标"
输出：{"knowledge_points":["二次函数","抛物线"],"difficulty":2,"question_type":"solution","grade_level":"grade_9","cognitive_level":"apply"}

输入："下列说法正确的是（ ） A. ... B. ... C. ... D. ..."
输出：{"knowledge_points":[],"difficulty":3,"question_type":"choice","grade_level":"other","cognitive_level":"understand"}

**重要**：
- 如果无法识别知识点，knowledge_points 返回空数组 []
- 只输出 JSON，不要任何 markdown 标记或额外文字
- 知识点名称用简洁的中文术语（如"二次函数"而非"二次函数的概念与性质"）"#;

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
    /// 匹配成功的知识点节点列表（含 AI 原始名称 + 匹配的 UUID + 分数）
    pub knowledge_nodes: Vec<KnowledgeNodeMatch>,
    /// AI 推断的难度（1-5）
    pub difficulty: Option<i16>,
    /// AI 推断的题型
    pub question_type: Option<String>,
    /// AI 推断的年级
    pub grade_level: Option<String>,
    /// AI 推断的认知层次
    pub cognitive_level: Option<String>,
    /// AI 返回但未匹配上的知识点名称（前端可提示用户手动选择）
    pub unmatched_knowledge_points: Vec<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct KnowledgeNodeMatch {
    /// AI 返回的原始名称
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
    /// 匹配置信度（0.0-1.0）
    pub score: f32,
    /// 匹配类型：exact（精确）/ alias（同义词）/ fuzzy（模糊）
    pub match_type: String,
}

/// AI 返回的原始结构
#[derive(Debug, Deserialize)]
struct AiTaggingResult {
    knowledge_points: Vec<String>,
    difficulty: Option<i16>,
    question_type: Option<String>,
    grade_level: Option<String>,
    cognitive_level: Option<String>,
}

// ===========================================================================
// Handler
// ===========================================================================

/// POST /api/v1/questions/ai-tagging — AI 智能打标
///
/// 流程：
/// 1. 调用 LLM 提取知识点名称 + 难度 + 题型 + 年级 + 认知层次
/// 2. 对每个 AI 返回的知识点名称，在 knowledge_nodes 表中匹配
///    优先级：精确匹配 name = $1 > aliases 包含 > pg_trgm 相似度
/// 3. 返回匹配结果，前端自动选中 node_ids
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

    // ── 1. 调用 LLM 提取标签 ───────────────────────────────────────────
    let (api_key, provider_name, model, base_url) =
        resolve_ai_config(&auth, &state, ModelKind::Text)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({"error": e}))))?;

    let provider = create_provider(&provider_name, &api_key, &base_url);
    let raw_json = provider
        .parse_text_with_prompt(&req.content, AI_TAGGING_SYSTEM_PROMPT, model.as_deref())
        .await
        .map_err(map_ai_error)?;

    // ── 2. 清洗 AI 返回的 JSON ────────────────────────────────────────
    let ai_result: AiTaggingResult = clean_and_parse(&raw_json).map_err(|e| {
        tracing::warn!("AI 打标 JSON 解析失败: {e}");
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": format!("AI 返回格式损坏: {e}")})),
        )
    })?;

    tracing::info!(
        "AI 打标结果: 知识点 {:?}, 难度 {:?}, 题型 {:?}",
        ai_result.knowledge_points,
        ai_result.difficulty,
        ai_result.question_type
    );

    // ── 3. 数据库匹配知识点 ───────────────────────────────────────────
    let (matched, unmatched) =
        match_knowledge_nodes(&state.pool, &ai_result.knowledge_points, req.space_id).await?;

    Ok(Json(AiTaggingResponse {
        knowledge_nodes: matched,
        difficulty: ai_result.difficulty,
        question_type: ai_result.question_type,
        grade_level: ai_result.grade_level,
        cognitive_level: ai_result.cognitive_level,
        unmatched_knowledge_points: unmatched,
    }))
}

// ===========================================================================
// 知识点模糊匹配核心逻辑
// ===========================================================================

/// 对 AI 返回的每个知识点名称，在 knowledge_nodes 表中找最佳匹配
///
/// 匹配策略（按优先级）：
/// 1. **exact**: `name = $1` → score = 1.0
/// 2. **alias**: `EXISTS (SELECT 1 FROM jsonb_array_elements(aliases) WHERE ->>'alias' = $1)` → score = 0.95
/// 3. **fuzzy**: `name % $1`（pg_trgm 相似度，threshold 0.3）→ score = similarity(name, $1)
///
/// 同时返回未匹配的名称列表，便于前端提示用户手动选择
pub(crate) async fn match_knowledge_nodes(
    pool: &sqlx::PgPool,
    ai_names: &[String],
    space_id: Option<Uuid>,
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

        // 单条 SQL 同时支持三种匹配方式，并按 score 降序取第一个
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
                WHEN kn.name = $1 THEN 1.0
                WHEN EXISTS (
                  SELECT 1 FROM jsonb_array_elements(kn.aliases) AS a
                  WHERE a->>'alias' = $1
                ) THEN 0.95
                ELSE similarity(kn.name, $1)
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
              AND (
                kn.name = $1
                OR EXISTS (
                  SELECT 1 FROM jsonb_array_elements(kn.aliases) AS a
                  WHERE a->>'alias' = $1
                )
                OR kn.name % $1
              )
            ORDER BY score DESC
            LIMIT 1
            "#,
        )
        .bind(name)
        .bind(space_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            tracing::warn!("知识点匹配查询失败: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("知识点匹配查询失败: {e}")})),
            )
        })?;

        match result {
            Some(m) if m.score >= 0.3 => {
                tracing::debug!(
                    "知识点「{}」匹配到「{}」(score={:.2}, type={})",
                    m.ai_name,
                    m.node_name,
                    m.score,
                    m.match_type
                );
                matched.push(m);
            }
            _ => {
                tracing::debug!("知识点「{}」未找到匹配", name);
                unmatched.push(name.clone());
            }
        }
    }

    Ok((matched, unmatched))
}

// ===========================================================================
// 单元测试
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tagging_prompt_contains_essential_rules() {
        // 验证 prompt 包含关键规则
        assert!(AI_TAGGING_SYSTEM_PROMPT.contains("knowledge_points"));
        assert!(AI_TAGGING_SYSTEM_PROMPT.contains("difficulty"));
        assert!(AI_TAGGING_SYSTEM_PROMPT.contains("question_type"));
        assert!(AI_TAGGING_SYSTEM_PROMPT.contains("multiple"));
        assert!(AI_TAGGING_SYSTEM_PROMPT.contains("grade_level"));
        assert!(AI_TAGGING_SYSTEM_PROMPT.contains("cognitive_level"));
        // 验证示例
        assert!(AI_TAGGING_SYSTEM_PROMPT.contains("二次函数"));
        assert!(AI_TAGGING_SYSTEM_PROMPT.contains("抛物线"));
    }

    #[test]
    fn test_ai_tagging_result_deserialize() {
        let json_str = r#"{
            "knowledge_points": ["二次函数", "抛物线"],
            "difficulty": 2,
            "question_type": "solution",
            "grade_level": "grade_9",
            "cognitive_level": "apply"
        }"#;
        let result: AiTaggingResult = serde_json::from_str(json_str).unwrap();
        assert_eq!(result.knowledge_points.len(), 2);
        assert_eq!(result.difficulty, Some(2));
        assert_eq!(result.question_type.as_deref(), Some("solution"));
    }

    #[test]
    fn test_ai_tagging_result_empty_kp() {
        let json_str = r#"{
            "knowledge_points": [],
            "difficulty": null,
            "question_type": null,
            "grade_level": null,
            "cognitive_level": null
        }"#;
        let result: AiTaggingResult = serde_json::from_str(json_str).unwrap();
        assert!(result.knowledge_points.is_empty());
        assert!(result.difficulty.is_none());
    }
}
