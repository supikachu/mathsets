use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use serde_json::json;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::models::question::{
    CreateKnowledgePointRequest, KnowledgePoint, KnowledgePointTreeNode,
    UpdateKnowledgePointRequest,
};
use crate::AppState;

/// 构建知识点树（递归）
fn build_tree(points: &[KnowledgePoint], parent_id: Option<Uuid>) -> Vec<KnowledgePointTreeNode> {
    let mut children: Vec<KnowledgePointTreeNode> = points
        .iter()
        .filter(|p| p.parent_id == parent_id)
        .map(|p| {
            let mut node = KnowledgePointTreeNode::from(p.clone());
            node.children = build_tree(points, Some(p.id));
            node
        })
        .collect();
    children.sort_by_key(|n| n.sort_order);
    children
}

/// GET /api/v1/knowledge-points — 获取知识点树
pub async fn list_knowledge_points(
    State(state): State<AppState>,
) -> Result<Json<Vec<KnowledgePointTreeNode>>, (StatusCode, Json<serde_json::Value>)> {
    let points = sqlx::query_as::<_, KnowledgePoint>(
        "SELECT * FROM knowledge_points ORDER BY sort_order, name",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询知识点失败: {}", e)})),
        )
    })?;

    let tree = build_tree(&points, None);
    Ok(Json(tree))
}

/// POST /api/v1/knowledge-points — 新增知识点
pub async fn create_knowledge_point(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateKnowledgePointRequest>,
) -> Result<(StatusCode, Json<KnowledgePoint>), (StatusCode, Json<serde_json::Value>)> {
    // 任意登录用户可维护知识点（全局树，首期不按空间隔离）
    let _ = auth_user;

    let id = Uuid::new_v4();
    let now = chrono::Utc::now();

    sqlx::query(
        r#"
        INSERT INTO knowledge_points (id, parent_id, name, grade, sort_order, created_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(id)
    .bind(req.parent_id)
    .bind(&req.name)
    .bind(&req.grade)
    .bind(req.sort_order.unwrap_or(0))
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("创建知识点失败: {}", e)})),
        )
    })?;

    let kp = KnowledgePoint {
        id,
        parent_id: req.parent_id,
        name: req.name,
        grade: req.grade,
        sort_order: req.sort_order.unwrap_or(0),
        created_at: now,
    };

    Ok((StatusCode::CREATED, Json(kp)))
}

/// PUT /api/v1/knowledge-points/:id — 更新知识点
pub async fn update_knowledge_point(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateKnowledgePointRequest>,
) -> Result<Json<KnowledgePoint>, (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    let existing = sqlx::query_as::<_, KnowledgePoint>(
        "SELECT * FROM knowledge_points WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询知识点失败: {}", e)})),
        )
    })?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "知识点不存在"}))))?;

    let new_parent_id = req.parent_id.or(existing.parent_id);
    let new_name = req.name.unwrap_or(existing.name);
    let new_grade = req.grade.or(existing.grade);
    let new_sort_order = req.sort_order.unwrap_or(existing.sort_order);

    sqlx::query(
        r#"
        UPDATE knowledge_points
        SET parent_id = $1, name = $2, grade = $3, sort_order = $4
        WHERE id = $5
        "#,
    )
    .bind(new_parent_id)
    .bind(&new_name)
    .bind(&new_grade)
    .bind(new_sort_order)
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("更新知识点失败: {}", e)})),
        )
    })?;

    let updated = sqlx::query_as::<_, KnowledgePoint>(
        "SELECT * FROM knowledge_points WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询知识点失败: {}", e)})),
        )
    })?;

    Ok(Json(updated))
}

/// DELETE /api/v1/knowledge-points/:id — 删除知识点
pub async fn delete_knowledge_point(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    // 检查是否有子节点
    let child_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM knowledge_points WHERE parent_id = $1",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询知识点失败: {}", e)})),
        )
    })?;

    if child_count > 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "该知识点下有子节点，请先删除子节点"})),
        ));
    }

    // 检查是否有题目关联
    let ref_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM question_knowledge_points WHERE knowledge_point_id = $1",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询题目关联失败: {}", e)})),
        )
    })?;

    if ref_count > 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "该知识点下有题目关联，请先解除关联"})),
        ));
    }

    let result = sqlx::query("DELETE FROM knowledge_points WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("删除知识点失败: {}", e)})),
            )
        })?;

    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error": "知识点不存在"}))));
    }

    Ok(StatusCode::NO_CONTENT)
}
