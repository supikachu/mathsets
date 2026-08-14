//! V2.1.1 P1：标签治理闭环（计划书 §六.3 / 文档 17-19 节）
//!
//! - `GET /admin/tag-candidates`：候选队列（status/kind 过滤，分页）
//! - `GET /admin/tag-candidates/{id}`：候选详情
//! - `POST /admin/tag-candidates/{id}/approve`：四分支审核
//!   （new_node 接受为新标签 / alias 加为已有标签别名 / merge 并入已有标签）
//! - `POST /admin/tag-candidates/{id}/reject`：拒绝
//! - `POST /knowledge-nodes/{id}/merge`：canonical 合并（环检测 + 审计，不物理删除）
//! - `GET /tags/{id}/usage`：标签使用情况
//!
//! 原则（文档 15/19 节）：合并不物理删除；A→B 后 A.status=merged + A.canonical_id=B.id；
//! canonical 环 / 自指必须拒绝；每次合并写 tag_merge_records。

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde_json::json;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::auth::permissions::is_admin_user;
use crate::models::tag_governance::{
    ApproveCandidateRequest, MergeKnowledgeNodeRequest, RejectCandidateRequest, TagCandidate,
    TagCandidateQuery,
};
use crate::AppState;

// ---------------------------------------------------------------------------
// 辅助
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

fn forbidden() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(json!({"error": "仅管理员可执行标签治理操作"})),
    )
}

const CANDIDATE_COLUMNS: &str = "id, kind, raw_name, normalized_name, suggested_node_id, \
     ai_confidence, match_score, source_task_id, source_question_id, status, \
     reviewed_by, reviewed_at, created_at";

async fn load_candidate(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> Result<Option<TagCandidate>, (StatusCode, Json<serde_json::Value>)> {
    sqlx::query_as::<_, TagCandidate>(&format!(
        "SELECT {CANDIDATE_COLUMNS} FROM tag_candidates WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err(format!("查询候选失败: {e}")))
}

/// 写合并审计记录
async fn record_merge(
    pool: &sqlx::PgPool,
    source_tag_id: Uuid,
    target_tag_id: Uuid,
    operator_id: Uuid,
    reason: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO tag_merge_records (target_type, source_tag_id, target_tag_id, operator_id, operator_type, reason)
        VALUES ('knowledge_node', $1, $2, $3, 'admin', $4)
        "#,
    )
    .bind(source_tag_id)
    .bind(target_tag_id)
    .bind(operator_id)
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(())
}

/// canonical 环检测：从 target 沿 canonical_id 链走，若到达 source → 环
async fn would_form_cycle(
    pool: &sqlx::PgPool,
    source_id: Uuid,
    target_id: Uuid,
) -> Result<bool, (StatusCode, Json<serde_json::Value>)> {
    let cyclic: bool = sqlx::query_scalar(
        r#"
        WITH RECURSIVE chain AS (
            SELECT canonical_id FROM knowledge_nodes WHERE id = $2
            UNION ALL
            SELECT kn.canonical_id FROM knowledge_nodes kn
            JOIN chain c ON kn.id = c.canonical_id
            WHERE kn.canonical_id IS NOT NULL
        )
        SELECT EXISTS (SELECT 1 FROM chain WHERE canonical_id = $1)
        "#,
    )
    .bind(source_id)
    .bind(target_id)
    .fetch_one(pool)
    .await
    .map_err(|e| db_err(format!("环检测失败: {e}")))?;
    Ok(cyclic)
}

/// 迁移 source 节点的题目关联到 target（去重），返回迁移数
async fn migrate_question_relations(
    pool: &sqlx::PgPool,
    source_id: Uuid,
    target_id: Uuid,
) -> Result<i64, (StatusCode, Json<serde_json::Value>)> {
    // 先按题目去重计数（目标已有关联的题目不计入）
    let migrated: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM question_knowledge_nodes src
        WHERE src.node_id = $1
          AND NOT EXISTS (SELECT 1 FROM question_knowledge_nodes tgt
                          WHERE tgt.question_id = src.question_id AND tgt.node_id = $2)
        "#,
    )
    .bind(source_id)
    .bind(target_id)
    .fetch_one(pool)
    .await
    .map_err(|e| db_err(format!("统计迁移数量失败: {e}")))?;

    // 迁移（保留 is_primary 与 source/confidence；冲突跳过）
    sqlx::query(
        r#"
        INSERT INTO question_knowledge_nodes (question_id, node_id, is_primary, relevance_score, ai_confidence, source, created_at)
        SELECT question_id, $2, is_primary, relevance_score, ai_confidence, source, NOW()
        FROM question_knowledge_nodes
        WHERE node_id = $1
        ON CONFLICT (question_id, node_id) DO NOTHING
        "#,
    )
    .bind(source_id)
    .bind(target_id)
    .execute(pool)
    .await
    .map_err(|e| db_err(format!("迁移题目关联失败: {e}")))?;

    // 删除源节点关联
    sqlx::query("DELETE FROM question_knowledge_nodes WHERE node_id = $1")
        .bind(source_id)
        .execute(pool)
        .await
        .map_err(|e| db_err(format!("清理源节点关联失败: {e}")))?;

    Ok(migrated)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/admin/tag-candidates — 候选队列
pub async fn list_tag_candidates(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Query(query): Query<TagCandidateQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_admin_user(&auth) {
        return Err(forbidden());
    }

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).min(100);
    let offset = (page - 1) * page_size;

    let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(&format!(
        "SELECT {CANDIDATE_COLUMNS} FROM tag_candidates WHERE 1=1"
    ));
    if let Some(status) = query.status.as_deref() {
        builder.push(" AND status = ").push_bind(status);
    }
    if let Some(kind) = query.kind.as_deref() {
        builder.push(" AND kind = ").push_bind(kind);
    }
    builder
        .push(" ORDER BY created_at DESC LIMIT ")
        .push_bind(page_size)
        .push(" OFFSET ")
        .push_bind(offset);
    let items: Vec<TagCandidate> = builder
        .build_query_as()
        .fetch_all(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询候选列表失败: {e}")))?;

    let mut count_builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
        "SELECT COUNT(*) FROM tag_candidates WHERE 1=1",
    );
    if let Some(status) = query.status.as_deref() {
        count_builder.push(" AND status = ").push_bind(status);
    }
    if let Some(kind) = query.kind.as_deref() {
        count_builder.push(" AND kind = ").push_bind(kind);
    }
    let total: i64 = count_builder
        .build_query_scalar()
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("统计候选总数失败: {e}")))?;

    Ok(Json(json!({ "items": items, "total": total, "page": page, "page_size": page_size })))
}

/// GET /api/v1/admin/tag-candidates/{id} — 候选详情（含来源题目题干）
pub async fn get_tag_candidate(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_admin_user(&auth) {
        return Err(forbidden());
    }
    let candidate = load_candidate(&state.pool, id).await?.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "候选不存在"})),
        )
    })?;

    // 来源题目题干（便于审核时对照上下文）
    let stem: Option<String> = match candidate.source_question_id {
        Some(qid) => sqlx::query_scalar("SELECT stem FROM questions WHERE id = $1")
            .bind(qid)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| db_err(format!("查询来源题目失败: {e}")))?
            .and_then(|s: String| Some(s.chars().take(300).collect())),
        None => None,
    };
    let task_id = candidate.source_task_id;

    Ok(Json(json!({
        "candidate": candidate,
        "source_stem": stem,
        "source_task_id": task_id,
    })))
}

/// POST /api/v1/admin/tag-candidates/{id}/approve — 审核四分支
pub async fn approve_tag_candidate(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<ApproveCandidateRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_admin_user(&auth) {
        return Err(forbidden());
    }
    let candidate = load_candidate(&state.pool, id).await?.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "候选不存在"})),
        )
    })?;
    if candidate.status != "pending" {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": format!("候选已处理（status={}）", candidate.status)})),
        ));
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| db_err(format!("开启事务失败: {e}")))?;

    // 处理后的目标节点（alias/merge 指向 target；new_node 为新建节点）
    let target_node_id: Uuid = match req.action.as_str() {
        // ── 分支 1：接受为新标签 ──
        "new_node" => {
            let tree_id = req.tree_id.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "new_node 分支必须提供 tree_id"})),
                )
            })?;
            let name = req
                .name
                .clone()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| candidate.raw_name.clone());

            // 复用 knowledge_nodes 的插入模式：path = 父节点 path + 新段
            let (parent_path, parent_depth): (Option<String>, i16) = match req.parent_id {
                Some(pid) => {
                    let row: Option<(String, i16)> = sqlx::query_as(
                        "SELECT path::text, depth FROM knowledge_nodes WHERE id = $1 AND is_active = TRUE",
                    )
                    .bind(pid)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| db_err(format!("查询父节点失败: {e}")))?;
                    row.map(|(p, d)| (Some(p), d))
                        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json(json!({"error": "父节点不存在"}))))?
                }
                None => (None, -1),
            };

            let new_id = Uuid::new_v4();
            let path_seg = new_id.to_string().replace('-', "_");
            let new_path = match &parent_path {
                Some(p) => format!("{p}.{path_seg}"),
                None => path_seg,
            };
            let new_depth = parent_depth + 1;

            sqlx::query(
                r#"
                INSERT INTO knowledge_nodes (id, tree_id, parent_id, path, depth, name, aliases,
                    description, sort_order, question_count, is_active, status, source, created_at, updated_at)
                VALUES ($1, $2, $3, $4::ltree, $5, $6, '[]', NULL, 0, 0, TRUE, 'active', 'admin', NOW(), NOW())
                "#,
            )
            .bind(new_id)
            .bind(tree_id)
            .bind(req.parent_id)
            .bind(&new_path)
            .bind(new_depth)
            .bind(&name)
            .execute(&mut *tx)
            .await
            .map_err(|e| db_err(format!("创建新标签失败: {e}")))?;

            new_id
        }
        // ── 分支 2：添加为已有标签的别名 ──
        "alias" | "merge" => {
            let target_id = req.target_node_id.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "alias/merge 分支必须提供 target_node_id"})),
                )
            })?;
            let exists: Option<Uuid> = sqlx::query_scalar(
                "SELECT id FROM knowledge_nodes WHERE id = $1 AND is_active = TRUE",
            )
            .bind(target_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| db_err(format!("查询目标标签失败: {e}")))?;
            if exists.is_none() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "目标标签不存在或已停用"})),
                ));
            }

            if req.action == "merge" {
                // 环检测（源候选不是节点，环只可能由未来 target 链引起 → 校验 target 链无自引用）
                let self_ref: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM knowledge_nodes WHERE id = $1 AND canonical_id = $1)",
                )
                .bind(target_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| db_err(format!("环检测失败: {e}")))?;
                if self_ref {
                    return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "目标标签存在自引用，拒绝合并"}))));
                }
                record_merge(
                    &state.pool,
                    candidate.id,
                    target_id,
                    auth.id,
                    req.reason.as_deref(),
                )
                .await
                .map_err(|e| db_err(format!("写合并审计失败: {e}")))?;
            }

            // 追加别名（去重）
            sqlx::query(
                r#"
                UPDATE knowledge_nodes
                SET aliases = CASE
                        WHEN EXISTS (SELECT 1 FROM jsonb_array_elements(aliases) a WHERE a->>'alias' = $2)
                        THEN aliases
                        ELSE aliases || jsonb_build_array(jsonb_build_object('alias', $2))
                    END,
                    updated_at = NOW()
                WHERE id = $1
                "#,
            )
            .bind(target_id)
            .bind(&candidate.raw_name)
            .execute(&mut *tx)
            .await
            .map_err(|e| db_err(format!("追加别名失败: {e}")))?;

            target_id
        }
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("未知审核动作: {other}")})),
            ))
        }
    };

    // 关联来源题目到目标节点（AI 关联，保留置信度；冲突跳过）
    if let Some(question_id) = candidate.source_question_id {
        let confidence = candidate.ai_confidence;
        sqlx::query(
            r#"
            INSERT INTO question_knowledge_nodes (question_id, node_id, is_primary, relevance_score, ai_confidence, source, created_at)
            VALUES ($1, $2, FALSE, NULL, $3, 'ai', NOW())
            ON CONFLICT (question_id, node_id) DO NOTHING
            "#,
        )
        .bind(question_id)
        .bind(target_node_id)
        .bind(confidence)
        .execute(&mut *tx)
        .await
        .map_err(|e| db_err(format!("关联题目到标签失败: {e}")))?;
    }

    // 更新候选状态 + 目标节点 question_count
    sqlx::query(
        r#"
        UPDATE tag_candidates
        SET status = 'approved', reviewed_by = $2, reviewed_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(candidate.id)
    .bind(auth.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err(format!("更新候选状态失败: {e}")))?;

    sqlx::query(
        "UPDATE knowledge_nodes SET question_count = question_count + 1, updated_at = NOW() WHERE id = $1",
    )
    .bind(target_node_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err(format!("更新标签计数失败: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| db_err(format!("提交事务失败: {e}")))?;

    Ok(Json(json!({
        "message": "已审核",
        "action": req.action,
        "target_node_id": target_node_id
    })))
}

/// POST /api/v1/admin/tag-candidates/{id}/reject — 拒绝候选
pub async fn reject_tag_candidate(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<RejectCandidateRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_admin_user(&auth) {
        return Err(forbidden());
    }
    let candidate = load_candidate(&state.pool, id).await?.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "候选不存在"})),
        )
    })?;
    if candidate.status != "pending" {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": format!("候选已处理（status={}）", candidate.status)})),
        ));
    }

    sqlx::query(
        r#"
        UPDATE tag_candidates
        SET status = 'rejected', reviewed_by = $2, reviewed_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(auth.id)
    .execute(&state.pool)
    .await
    .map_err(|e| db_err(format!("拒绝候选失败: {e}")))?;

    let _ = req.reason;
    Ok(Json(json!({ "message": "已拒绝" })))
}

/// POST /api/v1/knowledge-nodes/{id}/merge — canonical 合并（文档 19 节）
///
/// :id = 源节点（被合并，不物理删除）：status=merged + canonical_id=target；
/// 题目关联迁移到 target（去重）；写 tag_merge_records；环/自指拒绝。
pub async fn merge_knowledge_node(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(source_id): Path<Uuid>,
    Json(req): Json<MergeKnowledgeNodeRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_admin_user(&auth) {
        return Err(forbidden());
    }
    if source_id == req.target_id {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "不能将标签合并到自身"})),
        ));
    }

    // 源/目标都必须存在且 active
    let source_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM knowledge_nodes WHERE id = $1)",
    )
    .bind(source_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| db_err(format!("查询源标签失败: {e}")))?;
    let target_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM knowledge_nodes WHERE id = $1 AND is_active = TRUE)",
    )
    .bind(req.target_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| db_err(format!("查询目标标签失败: {e}")))?;
    if !source_exists || !target_exists {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "源标签或目标标签不存在"})),
        ));
    }

    // 目标必须是"最终标签"（未被合并过），否则合并会形成链/环
    let target_is_merged: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM knowledge_nodes WHERE id = $1 AND canonical_id IS NOT NULL)",
    )
    .bind(req.target_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| db_err(format!("查询目标标签状态失败: {e}")))?;
    if target_is_merged {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "目标标签已是合并标签（merged），不能作为合并目标"})),
        ));
    }

    // 环检测（A→B 且 B 链上可达 A → 拒绝）
    if would_form_cycle(&state.pool, source_id, req.target_id).await? {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "合并将形成 canonical 环，已拒绝"})),
        ));
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| db_err(format!("开启事务失败: {e}")))?;

    // 迁移题目关联
    let migrated = migrate_question_relations(&state.pool, source_id, req.target_id).await?;

    // 源节点 → merged + canonical_id（不物理删除）
    sqlx::query(
        r#"
        UPDATE knowledge_nodes
        SET status = 'merged', canonical_id = $2, question_count = 0, updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(source_id)
    .bind(req.target_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err(format!("标记合并失败: {e}")))?;

    // 目标节点计数
    sqlx::query(
        "UPDATE knowledge_nodes SET question_count = question_count + $2, updated_at = NOW() WHERE id = $1",
    )
    .bind(req.target_id)
    .bind(migrated as i32)
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err(format!("更新目标计数失败: {e}")))?;

    // 审计
    sqlx::query(
        r#"
        INSERT INTO tag_merge_records (target_type, source_tag_id, target_tag_id, operator_id, operator_type, reason)
        VALUES ('knowledge_node', $1, $2, $3, 'admin', $4)
        "#,
    )
    .bind(source_id)
    .bind(req.target_id)
    .bind(auth.id)
    .bind(&req.reason)
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err(format!("写合并审计失败: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| db_err(format!("提交事务失败: {e}")))?;

    Ok(Json(json!({
        "message": "已合并",
        "source_id": source_id,
        "target_id": req.target_id,
        "migrated_relations": migrated
    })))
}

/// GET /api/v1/tags/{id}/usage — 标签使用情况（题目数 + 自建/引用分布）
pub async fn get_tag_usage(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let tag: Option<(String, String, i32)> =
        sqlx::query_as("SELECT name, category::text, use_count FROM tags WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| db_err(format!("查询标签失败: {e}")))?;
    let Some((name, category, use_count)) = tag else {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error": "标签不存在"}))));
    };

    let question_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM question_tags_relation WHERE tag_id = $1",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| db_err(format!("统计标签使用失败: {e}")))?;

    Ok(Json(json!({
        "tag_id": id,
        "name": name,
        "category": category,
        "use_count": use_count,
        "question_count": question_count
    })))
}
