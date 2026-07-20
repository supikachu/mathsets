//! 知识树元数据 CRUD（B3 新增）
//!
//! 知识树（KnowledgeTree）是知识点的容器，支持多棵树：
//! - 数学知识树（knowledge）
//! - 数学能力树（ability）
//! - 教材章节树（chapter，按版本细分：人教版/北师大版）

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::models::question::{
    CreateKnowledgeTreeRequest, KnowledgeTree, KnowledgeTreeKind, UpdateKnowledgeTreeRequest,
};
use crate::AppState;

/// 知识树查询参数
#[derive(Debug, Deserialize)]
pub struct TreeQuery {
    /// 按类型过滤
    pub kind: Option<KnowledgeTreeKind>,
    /// 按空间过滤（NULL = 全局预置树）
    pub space_id: Option<Uuid>,
    /// 是否包含 inactive 的树（仅管理员）
    #[serde(default)]
    pub include_inactive: bool,
}

/// GET /api/v1/knowledge-trees — 列出知识树
///
/// 默认返回：当前用户空间的全局树（space_id IS NULL）+ 空间专属树
pub async fn list_knowledge_trees(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Query(query): Query<TreeQuery>,
) -> Result<Json<Vec<KnowledgeTree>>, (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    let trees = sqlx::query_as::<_, KnowledgeTree>(
        r#"
        SELECT * FROM knowledge_trees
        WHERE ($1::bool OR is_active = TRUE)
          AND ($2::knowledge_tree_kind IS NULL OR kind = $2)
          AND (
            $3::uuid IS NULL
            OR space_id IS NULL
            OR space_id = $3
          )
        ORDER BY kind, code
        "#,
    )
    .bind(query.include_inactive)
    .bind(query.kind)
    .bind(query.space_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询知识树失败: {e}")})),
        )
    })?;

    Ok(Json(trees))
}

/// POST /api/v1/knowledge-trees — 新建知识树
///
/// 注意：code 在同空间内唯一（partial unique index）
pub async fn create_knowledge_tree(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateKnowledgeTreeRequest>,
) -> Result<(StatusCode, Json<KnowledgeTree>), (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    let kind = req.kind.unwrap_or(KnowledgeTreeKind::Knowledge);

    let tree = sqlx::query_as::<_, KnowledgeTree>(
        r#"
        INSERT INTO knowledge_trees (code, name, kind, space_id, description)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING *
        "#,
    )
    .bind(&req.code)
    .bind(&req.name)
    .bind(kind)
    .bind(req.space_id)
    .bind(&req.description)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        let msg = format!("{e}");
        let status = if msg.contains("duplicate") || msg.contains("unique") {
            StatusCode::CONFLICT
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (status, Json(json!({"error": format!("创建知识树失败: {msg}")})))
    })?;

    Ok((StatusCode::CREATED, Json(tree)))
}

/// PUT /api/v1/knowledge-trees/{id} — 更新知识树元数据
pub async fn update_knowledge_tree(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateKnowledgeTreeRequest>,
) -> Result<Json<KnowledgeTree>, (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    let tree = sqlx::query_as::<_, KnowledgeTree>(
        r#"
        UPDATE knowledge_trees
        SET name        = COALESCE($1, name),
            description = COALESCE($2, description),
            is_active   = COALESCE($3, is_active),
            updated_at  = NOW()
        WHERE id = $4
        RETURNING *
        "#,
    )
    .bind(req.name)
    .bind(req.description)
    .bind(req.is_active)
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("更新知识树失败: {e}")})),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "知识树不存在"})),
        )
    })?;

    Ok(Json(tree))
}

/// DELETE /api/v1/knowledge-trees/{id} — 删除知识树
///
/// 级联删除：knowledge_trees ON DELETE CASCADE 会自动删除所有 knowledge_nodes
/// （但 question_knowledge_nodes 也会因 knowledge_nodes ON DELETE CASCADE 被清理）
pub async fn delete_knowledge_tree(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    // 预检查：如果有题目关联，拒绝删除（避免误删）
    let linked_questions = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*) 
        FROM question_knowledge_nodes qkn
        JOIN knowledge_nodes kn ON kn.id = qkn.node_id
        WHERE kn.tree_id = $1
        "#,
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询关联题目失败: {e}")})),
        )
    })?;

    if linked_questions > 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": format!("该知识树下有 {} 个题目关联，请先解除关联后再删除", linked_questions)
            })),
        ));
    }

    let result = sqlx::query("DELETE FROM knowledge_trees WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("删除知识树失败: {e}")})),
            )
        })?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "知识树不存在"})),
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}
