//! AI 智能打标 API
//!
//! 业务逻辑在 `crate::ai::tagging`。本文件负责鉴权、HTTP 映射；
//! `match_knowledge_nodes` 仅用于解析预览（kp_matches），权威打标走 TaggingEngine。

use axum::{
    extract::{Extension, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::ai::provider::create_provider;
use crate::ai::tagging::engine::TaggingError;
use crate::ai::tagging::types::TaggingDimension;
use crate::ai::tagging::{
    run_tagging, TaggingContext, TaggingInput, TaggingPolicy, TaggingSuggestion,
};
use crate::auth::middleware::AuthUser;
use crate::auth::permissions::{can_access_space, get_space};
use crate::handlers::ai::{map_ai_error, resolve_ai_config, ModelKind};
use crate::AppState;

pub use crate::ai::tagging::types::{KnowledgeNodeMatch, TagMatch};

#[derive(Debug, Deserialize)]
pub struct AiTaggingRequest {
    pub content: String,
    pub space_id: Option<Uuid>,
    pub question_id: Option<Uuid>,
    /// 学段：`junior` | `senior`，约束知识树召回，避免高中题命中初中树
    pub stage: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AiTaggingResponse {
    pub knowledge_nodes: Vec<KnowledgeNodeMatch>,
    pub competency_tags: Vec<TagMatch>,
    pub method_tags: Vec<TagMatch>,
    pub difficulty: Option<i16>,
    pub question_type: Option<String>,
    pub grade_level: Option<String>,
    pub cognitive_level: Option<String>,
    pub unmatched_knowledge_points: Vec<String>,
    /// 统一建议 ID（确认保存时回传）；兼容期可选
    pub suggestion_id: Option<Uuid>,
    pub engine_version: String,
    pub needs_review: bool,
    pub unmatched: Vec<crate::ai::tagging::types::TaggingUnmatched>,
    /// 五维匹配（前端按 dimension 分发）；兼容期与 knowledge_nodes 并行
    pub matches: Vec<crate::ai::tagging::types::TaggingMatch>,
}

/// POST /api/v1/questions/ai-tagging
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

    let (api_key, provider_name, model, base_url) =
        resolve_ai_config(&auth, &state, ModelKind::Text)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({"error": e}))))?;
    let provider = create_provider(&provider_name, &api_key, &base_url);

    let ctx = TaggingContext {
        user_id: auth.id,
        space_id: req.space_id,
        question_id: req.question_id,
        source_task_id: None,
        source_index: None,
        stage: req.stage.clone(),
    };

    let suggestion = run_tagging(
        &state.pool,
        Some(provider.as_ref()),
        model.as_deref(),
        TaggingInput::Content {
            content: req.content,
        },
        &ctx,
        &TaggingPolicy::default(),
    )
    .await
    .map_err(map_tagging_error)?;

    Ok(Json(legacy_response(suggestion)))
}

pub fn legacy_response(suggestion: TaggingSuggestion) -> AiTaggingResponse {
    let mut knowledge_nodes = Vec::new();
    let mut competency_tags = Vec::new();
    let mut method_tags = Vec::new();
    for m in &suggestion.matches {
        match m.dimension {
            TaggingDimension::Chapter | TaggingDimension::Knowledge | TaggingDimension::Pattern => {
                if let Some(n) = m.to_knowledge_node_match() {
                    knowledge_nodes.push(n);
                }
            }
            TaggingDimension::CoreCompetence => {
                if let Some(t) = m.to_tag_match() {
                    competency_tags.push(t);
                }
            }
            TaggingDimension::Method => {
                if let Some(t) = m.to_tag_match() {
                    method_tags.push(t);
                }
            }
        }
    }

    let unmatched_knowledge_points = suggestion
        .unmatched
        .iter()
        .filter(|u| {
            matches!(
                u.dimension,
                TaggingDimension::Chapter | TaggingDimension::Knowledge | TaggingDimension::Pattern
            )
        })
        .map(|u| u.raw_name.clone())
        .collect();

    AiTaggingResponse {
        knowledge_nodes,
        competency_tags,
        method_tags,
        difficulty: suggestion.difficulty,
        question_type: suggestion.question_type,
        grade_level: suggestion.grade_level,
        cognitive_level: suggestion.cognitive_level,
        unmatched_knowledge_points,
        suggestion_id: suggestion.suggestion_id,
        engine_version: suggestion.engine_version,
        needs_review: suggestion.needs_review,
        unmatched: suggestion.unmatched,
        matches: suggestion.matches,
    }
}

fn map_tagging_error(e: TaggingError) -> (StatusCode, Json<serde_json::Value>) {
    match e {
        TaggingError::EmptyContent => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "题目文本不能为空"})),
        ),
        TaggingError::ExtractParse(msg) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": format!("AI 返回格式损坏: {msg}")})),
        ),
        TaggingError::Ai(e) => map_ai_error(e),
        TaggingError::Db(e) => e,
        TaggingError::Persist(e) => {
            tracing::error!("写入打标建议失败: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "保存打标建议失败，请稍后重试"})),
            )
        }
    }
}

pub(crate) async fn match_knowledge_nodes(
    pool: &sqlx::PgPool,
    ai_names: &[String],
    space_id: Option<Uuid>,
    tree_kind: &str,
) -> Result<
    (Vec<KnowledgeNodeMatch>, Vec<String>),
    (StatusCode, Json<serde_json::Value>),
> {
    crate::ai::tagging::repository::match_nodes(pool, ai_names, space_id, tree_kind).await
}
