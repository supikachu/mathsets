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
use crate::models::question::{CreateTagRequest, Tag, TagCategory, TagQuery, UpdateTagRequest};
use crate::AppState;

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

fn db_err(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    let msg_str = msg.into();
    tracing::error!("数据库错误: {}", msg_str);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "服务器内部错误，请稍后重试",
            "code": "ERR_INTERNAL_SERVER"
        })),
    )
}

/// 将 UUID 转为 LTREE 兼容的路径段（去掉横杠，避免 LTREE 标签非法字符）
fn uuid_to_ltree_segment(id: Uuid) -> String {
    id.to_string().replace('-', "_")
}

/// 计算子标签的 LTREE path，并校验父节点存在且 category 一致
///
/// 一次查询同时取 parent.path 和 parent.category：
/// - 父节点不存在 → 400
/// - 父子 category 不一致 → 400（防止 core_competence 下挂 school 子标签等脏数据）
async fn compute_tag_path(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    parent_id: Option<Uuid>,
    child_category: &TagCategory,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let segment = uuid_to_ltree_segment(id);
    if let Some(pid) = parent_id {
        // 一次查询同时取 path::text 和 category，避免二次查询父节点
        let row: Option<(String, TagCategory)> =
            sqlx::query_as("SELECT path::text, category FROM tags WHERE id = $1")
                .bind(pid)
                .fetch_optional(&mut **tx)
                .await
                .map_err(|e| db_err(format!("查询父标签失败: {}", e)))?;

        let (parent_path, parent_category) = row.ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "指定的父标签不存在"})),
            )
        })?;

        // 强制校验：父子标签 category 必须一致
        if &parent_category != child_category {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "父子标签的类别必须一致"})),
            ));
        }

        Ok(format!("{}.{}", parent_path, segment))
    } else {
        Ok(segment)
    }
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
    // 注意：tags.path 是 LTREE 类型，必须用 path::text AS path 才能解码为 String
    let tags = match (&query.category, &query.space_id) {
        (Some(cat), Some(sid)) => {
            sqlx::query_as::<_, Tag>(
                r#"
                SELECT id, parent_id, name, category, path::text AS path,
                       aliases, description, space_id, use_count, is_active, created_at
                FROM tags
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
                SELECT id, parent_id, name, category, path::text AS path,
                       aliases, description, space_id, use_count, is_active, created_at
                FROM tags
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
                SELECT id, parent_id, name, category, path::text AS path,
                       aliases, description, space_id, use_count, is_active, created_at
                FROM tags
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
                SELECT id, parent_id, name, category, path::text AS path,
                       aliases, description, space_id, use_count, is_active, created_at
                FROM tags
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
    /// 标签类别（TagCategory 枚举，反序列化时自动校验合法性）
    pub category: Option<TagCategory>,
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

    // 注意：tags.path 是 LTREE，SELECT 时必须 path::text AS path
    let tags = match (&query.category, &query.space_id) {
        (Some(cat), Some(sid)) => {
            sqlx::query_as::<_, Tag>(
                r#"
                SELECT id, parent_id, name, category, path::text AS path,
                       aliases, description, space_id, use_count, is_active, created_at
                FROM tags
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
                SELECT id, parent_id, name, category, path::text AS path,
                       aliases, description, space_id, use_count, is_active, created_at
                FROM tags
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
                SELECT id, parent_id, name, category, path::text AS path,
                       aliases, description, space_id, use_count, is_active, created_at
                FROM tags
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
                SELECT id, parent_id, name, category, path::text AS path,
                       aliases, description, space_id, use_count, is_active, created_at
                FROM tags
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
///
/// B3 重构：
/// - 移除手写 `valid_categories` 校验（TagCategory 枚举反序列化时已自动校验）
/// - 新增 parent_id / aliases / description 字段
/// - LTREE path 在 handler 层计算：根 = uuid_segment；子 = parent.path || '.' || uuid_segment
/// - aliases 缺省 `'[]'::jsonb`，is_active 缺省 TRUE
pub async fn create_tag(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateTagRequest>,
) -> Result<(StatusCode, Json<Tag>), (StatusCode, Json<serde_json::Value>)> {
    // 全局标签（space_id = NULL）仅管理员可创建
    if req.space_id.is_none() && !is_admin(&auth_user.role) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "仅管理员可创建全局预置标签"})),
        ));
    }

    let id = Uuid::new_v4();
    let now = chrono::Utc::now();
    let aliases = req.aliases.unwrap_or_else(|| serde_json::json!([]));

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| db_err(format!("开启事务失败: {}", e)))?;

    // 计算 LTREE path 并校验父节点合法性（存在性 + category 一致性）
    let path = compute_tag_path(&mut tx, id, req.parent_id, &req.category).await?;

    let tag = sqlx::query_as::<_, Tag>(
        r#"
        INSERT INTO tags (id, parent_id, name, category, path, aliases, description,
                          space_id, use_count, is_active, created_at)
        VALUES ($1, $2, $3, $4, text2ltree($5), $6, $7, $8, 0, TRUE, $9)
        RETURNING id, parent_id, name, category, path::text AS path,
                  aliases, description, space_id, use_count, is_active, created_at
        "#,
    )
    .bind(id)
    .bind(req.parent_id)
    .bind(&req.name)
    .bind(&req.category)
    .bind(&path)
    .bind(&aliases)
    .bind(&req.description)
    .bind(req.space_id)
    .bind(now)
    .fetch_one(&mut *tx)
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

    tx.commit()
        .await
        .map_err(|e| db_err(format!("提交事务失败: {}", e)))?;

    Ok((StatusCode::CREATED, Json(tag)))
}

/// PUT /api/v1/tags/:id — 更新标签（name / aliases / description / is_active）
///
/// B3 重构：
/// - 不允许修改 category（保证树一致性，B2 已从 UpdateTagRequest 移除 category 字段）
/// - 移除手写 `valid_categories` 校验
/// - SELECT/RETURNING 使用 path::text AS path 适配 LTREE
pub async fn update_tag(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateTagRequest>,
) -> Result<Json<Tag>, (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    let existing = sqlx::query_as::<_, Tag>(
        r#"
        SELECT id, parent_id, name, category, path::text AS path,
               aliases, description, space_id, use_count, is_active, created_at
        FROM tags WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| db_err(format!("查询标签失败: {}", e)))?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "标签不存在"}))))?;

    let new_name = req.name.unwrap_or(existing.name);
    let new_aliases = req.aliases.unwrap_or(existing.aliases);
    let new_description = req.description.or(existing.description);
    let new_is_active = req.is_active.unwrap_or(existing.is_active);

    let updated = sqlx::query_as::<_, Tag>(
        r#"
        UPDATE tags SET name = $1, aliases = $2, description = $3, is_active = $4
        WHERE id = $5
        RETURNING id, parent_id, name, category, path::text AS path,
                  aliases, description, space_id, use_count, is_active, created_at
        "#,
    )
    .bind(&new_name)
    .bind(&new_aliases)
    .bind(&new_description)
    .bind(new_is_active)
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
    let source: Option<Tag> = sqlx::query_as::<_, Tag>(
        r#"
        SELECT id, parent_id, name, category, path::text AS path,
               aliases, description, space_id, use_count, is_active, created_at
        FROM tags WHERE id = $1
        "#,
    )
    .bind(source_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| db_err(format!("查询源标签失败: {}", e)))?;
    let target: Option<Tag> = sqlx::query_as::<_, Tag>(
        r#"
        SELECT id, parent_id, name, category, path::text AS path,
               aliases, description, space_id, use_count, is_active, created_at
        FROM tags WHERE id = $1
        "#,
    )
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
