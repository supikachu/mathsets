use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::auth::permissions::is_admin;
use crate::models::question::{CreateTagRequest, Tag, TagQuery, UpdateTagRequest};
use crate::AppState;

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

fn db_err(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": msg.into()})),
    )
}

// ---------------------------------------------------------------------------
// CRUD
// ---------------------------------------------------------------------------

/// GET /api/v1/tags — 获取标签列表
/// 支持按 category 和 space_id 过滤；默认返回全局预置标签（space_id IS NULL）
pub async fn list_tags(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Query(query): Query<TagQuery>,
) -> Result<Json<Vec<Tag>>, (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    // 构建动态查询：全局标签始终可见，指定空间时额外包含该空间标签
    let tags = match (&query.category, &query.space_id) {
        (Some(cat), Some(sid)) => {
            sqlx::query_as::<_, Tag>(
                r#"
                SELECT * FROM tags
                WHERE category = $1 AND (space_id IS NULL OR space_id = $2)
                ORDER BY use_count DESC, name
                "#,
            )
            .bind(cat)
            .bind(sid)
            .fetch_all(&state.pool)
            .await
        }
        (Some(cat), None) => {
            sqlx::query_as::<_, Tag>(
                r#"
                SELECT * FROM tags
                WHERE category = $1 AND space_id IS NULL
                ORDER BY use_count DESC, name
                "#,
            )
            .bind(cat)
            .fetch_all(&state.pool)
            .await
        }
        (None, Some(sid)) => {
            sqlx::query_as::<_, Tag>(
                r#"
                SELECT * FROM tags
                WHERE space_id IS NULL OR space_id = $1
                ORDER BY category, use_count DESC, name
                "#,
            )
            .bind(sid)
            .fetch_all(&state.pool)
            .await
        }
        (None, None) => {
            sqlx::query_as::<_, Tag>(
                r#"
                SELECT * FROM tags
                WHERE space_id IS NULL
                ORDER BY category, use_count DESC, name
                "#,
            )
            .fetch_all(&state.pool)
            .await
        }
    }
    .map_err(|e| db_err(format!("查询标签失败: {}", e)))?;

    Ok(Json(tags))
}

/// 模糊联想查询参数
#[derive(Debug, Deserialize)]
pub struct SuggestQuery {
    pub q: String,
    pub category: Option<String>,
    pub space_id: Option<Uuid>,
}

/// GET /api/v1/tags/suggest — 模糊联想
/// LIKE '%q%'，按 use_count DESC 排序，LIMIT 10
pub async fn suggest_tags(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Query(query): Query<SuggestQuery>,
) -> Result<Json<Vec<Tag>>, (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    let q_trimmed = query.q.trim();
    if q_trimmed.is_empty() {
        return Ok(Json(vec![]));
    }
    let pattern = format!("%{}%", q_trimmed);

    let tags = match (&query.category, &query.space_id) {
        (Some(cat), Some(sid)) => {
            sqlx::query_as::<_, Tag>(
                r#"
                SELECT * FROM tags
                WHERE name ILIKE $1 AND category = $2 AND (space_id IS NULL OR space_id = $3)
                ORDER BY use_count DESC, name
                LIMIT 10
                "#,
            )
            .bind(&pattern)
            .bind(cat)
            .bind(sid)
            .fetch_all(&state.pool)
            .await
        }
        (Some(cat), None) => {
            sqlx::query_as::<_, Tag>(
                r#"
                SELECT * FROM tags
                WHERE name ILIKE $1 AND category = $2 AND space_id IS NULL
                ORDER BY use_count DESC, name
                LIMIT 10
                "#,
            )
            .bind(&pattern)
            .bind(cat)
            .fetch_all(&state.pool)
            .await
        }
        (None, Some(sid)) => {
            sqlx::query_as::<_, Tag>(
                r#"
                SELECT * FROM tags
                WHERE name ILIKE $1 AND (space_id IS NULL OR space_id = $2)
                ORDER BY category, use_count DESC, name
                LIMIT 10
                "#,
            )
            .bind(&pattern)
            .bind(sid)
            .fetch_all(&state.pool)
            .await
        }
        (None, None) => {
            sqlx::query_as::<_, Tag>(
                r#"
                SELECT * FROM tags
                WHERE name ILIKE $1 AND space_id IS NULL
                ORDER BY category, use_count DESC, name
                LIMIT 10
                "#,
            )
            .bind(&pattern)
            .fetch_all(&state.pool)
            .await
        }
    }
    .map_err(|e| db_err(format!("联想查询失败: {}", e)))?;

    Ok(Json(tags))
}

/// POST /api/v1/tags — 新建标签
/// 任意登录用户可创建自定义标签；全局预置标签（space_id = NULL）仅管理员可建
pub async fn create_tag(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateTagRequest>,
) -> Result<(StatusCode, Json<Tag>), (StatusCode, Json<serde_json::Value>)> {
    // 校验 category 合法性
    let valid_categories = ["core_competence", "method", "school"];
    if !valid_categories.contains(&req.category.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("无效的标签类别: {}，合法值: core_competence | method | school", req.category)})),
        ));
    }

    // 全局标签（space_id = NULL）仅管理员可创建
    if req.space_id.is_none() && !is_admin(&auth_user.role) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "仅管理员可创建全局预置标签"})),
        ));
    }

    let id = Uuid::new_v4();
    let now = chrono::Utc::now();

    let tag = sqlx::query_as::<_, Tag>(
        r#"
        INSERT INTO tags (id, name, category, space_id, use_count, created_at)
        VALUES ($1, $2, $3, $4, 0, $5)
        RETURNING id, name, category, space_id, use_count, created_at
        "#,
    )
    .bind(id)
    .bind(&req.name)
    .bind(&req.category)
    .bind(req.space_id)
    .bind(now)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        // 唯一约束冲突
        if e.to_string().contains("unique") || e.to_string().contains("duplicate") {
            (
                StatusCode::CONFLICT,
                Json(json!({"error": "同名标签已存在"})),
            )
        } else {
            db_err(format!("创建标签失败: {}", e))
        }
    })?;

    Ok((StatusCode::CREATED, Json(tag)))
}

/// PUT /api/v1/tags/:id — 更新标签名称或类别
pub async fn update_tag(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateTagRequest>,
) -> Result<Json<Tag>, (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    let existing = sqlx::query_as::<_, Tag>("SELECT * FROM tags WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询标签失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "标签不存在"}))))?;

    let new_name = req.name.unwrap_or(existing.name);
    let new_category = req.category.unwrap_or(existing.category);

    // 校验 category 合法性
    let valid_categories = ["core_competence", "method", "school"];
    if !valid_categories.contains(&new_category.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("无效的标签类别: {}", new_category)})),
        ));
    }

    let updated = sqlx::query_as::<_, Tag>(
        r#"
        UPDATE tags SET name = $1, category = $2
        WHERE id = $3
        RETURNING id, name, category, space_id, use_count, created_at
        "#,
    )
    .bind(&new_name)
    .bind(&new_category)
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("unique") || e.to_string().contains("duplicate") {
            (
                StatusCode::CONFLICT,
                Json(json!({"error": "同名标签已存在"})),
            )
        } else {
            db_err(format!("更新标签失败: {}", e))
        }
    })?;

    Ok(Json(updated))
}

/// DELETE /api/v1/tags/:id — 删除标签
/// 关联表 question_tags_relation 通过 ON DELETE CASCADE 自动清理
/// 仅管理员可执行
pub async fn delete_tag(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    if !is_admin(&auth_user.role) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "仅管理员可删除标签"})),
        ));
    }

    let result = sqlx::query("DELETE FROM tags WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| db_err(format!("删除标签失败: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error": "标签不存在"}))));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// 合并标签请求体
#[derive(Debug, Deserialize)]
pub struct MergeTagRequest {
    /// 目标标签 ID（保留此标签，将源标签的关联全部迁移过来）
    pub target_id: Uuid,
}

/// POST /api/v1/tags/:id/merge — 合并同义词标签
/// :id = 源标签（被合并、将被删除），target_id = 目标标签（保留）
/// 操作：将源标签的所有题目关联迁移到目标标签（去重），合并 use_count，删除源标签
/// 仅管理员可执行
pub async fn merge_tag(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(source_id): Path<Uuid>,
    Json(req): Json<MergeTagRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_admin(&auth_user.role) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "仅管理员可合并标签"})),
        ));
    }

    if source_id == req.target_id {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "不能将标签合并到自身"})),
        ));
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| db_err(format!("开启事务失败: {}", e)))?;

    // 1. 验证两个标签都存在且同类别
    let source: Option<Tag> = sqlx::query_as::<_, Tag>("SELECT * FROM tags WHERE id = $1")
        .bind(source_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| db_err(format!("查询源标签失败: {}", e)))?;
    let target: Option<Tag> = sqlx::query_as::<_, Tag>("SELECT * FROM tags WHERE id = $1")
        .bind(req.target_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| db_err(format!("查询目标标签失败: {}", e)))?;

    let source = source.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "源标签不存在"})),
        )
    })?;
    let target = target.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "目标标签不存在"})),
        )
    })?;

    if source.category != target.category {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "只能合并同类别的标签"})),
        ));
    }

    // 2. 将源标签的题目关联迁移到目标标签（冲突时跳过——题目已有目标标签则不重复）
    sqlx::query(
        r#"
        INSERT INTO question_tags_relation (question_id, tag_id)
        SELECT question_id, $1 FROM question_tags_relation WHERE tag_id = $2
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(req.target_id)
    .bind(source_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err(format!("迁移关联失败: {}", e)))?;

    // 3. 统计实际迁移的关联数（目标已有关联的题目不重复计入）
    let migrated_count: i64 = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*) FROM question_tags_relation
        WHERE tag_id = $1 AND question_id IN (
            SELECT question_id FROM question_tags_relation WHERE tag_id = $2
        )
        "#,
    )
    .bind(req.target_id)
    .bind(source_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| db_err(format!("统计迁移数失败: {}", e)))?;

    // 4. 合并 use_count（目标 = 目标已有 + 源标签全部，因迁移后目标可能因去重而少于源+目）
    let merged_use_count: i64 = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(DISTINCT question_id) FROM question_tags_relation
        WHERE tag_id IN ($1, $2)
        "#,
    )
    .bind(req.target_id)
    .bind(source_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| db_err(format!("统计合并后 use_count 失败: {}", e)))?;

    sqlx::query("UPDATE tags SET use_count = $1 WHERE id = $2")
        .bind(merged_use_count)
        .bind(req.target_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| db_err(format!("更新 use_count 失败: {}", e)))?;

    // 5. 删除源标签（关联表通过 ON DELETE CASCADE 自动清理源标签的残余关联）
    sqlx::query("DELETE FROM tags WHERE id = $1")
        .bind(source_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| db_err(format!("删除源标签失败: {}", e)))?;

    tx.commit()
        .await
        .map_err(|e| db_err(format!("提交事务失败: {}", e)))?;

    Ok(Json(json!({
        "message": format!("已将「{}」合并到「{}」", source.name, target.name),
        "migrated_count": migrated_count,
        "merged_use_count": merged_use_count,
        "target_tag": target,
    })))
}
