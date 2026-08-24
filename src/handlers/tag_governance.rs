//! V2.1.1 P1：标签治理闭环（计划书 §六.3 / 文档 17-19 节）
//!
//! - `GET /admin/tag-candidates`：候选队列（status/kind/target_type 过滤，分页）
//! - `GET /admin/tag-candidates/{id}`：候选详情
//! - `POST /admin/tag-candidates/{id}/approve`：按 knowledge_node / tag 分支审核
//!   （new_node 接受为新标签 / alias 加为已有标签别名 / merge 并入已有标签）
//! - `POST /admin/tag-candidates/{id}/reject`：拒绝（写入 review_note）
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
use crate::models::question::TagCategory;
use crate::models::tag_governance::{
    ApproveCandidateRequest, MergeKnowledgeNodeRequest, RejectCandidateRequest, TagCandidate,
    TagCandidateQuery,
};
use crate::AppState;

type ApiErr = (StatusCode, Json<serde_json::Value>);
type Tx<'a> = sqlx::Transaction<'a, sqlx::Postgres>;

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

fn db_err(msg: impl Into<String>) -> ApiErr {
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

fn forbidden() -> ApiErr {
    (
        StatusCode::FORBIDDEN,
        Json(json!({"error": "仅管理员可执行标签治理操作"})),
    )
}

fn bad_request(msg: impl Into<String>) -> ApiErr {
    (StatusCode::BAD_REQUEST, Json(json!({"error": msg.into()})))
}

const CANDIDATE_COLUMNS: &str = "id, kind, target_type, raw_name, normalized_name, \
     suggested_node_id, suggested_tag_id, ai_confidence, match_score, source_task_id, \
     source_question_id, status, reviewed_by, reviewed_at, review_note, created_at";

async fn load_candidate(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> Result<Option<TagCandidate>, ApiErr> {
    sqlx::query_as::<_, TagCandidate>(&format!(
        "SELECT {CANDIDATE_COLUMNS} FROM tag_candidates WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err(format!("查询候选失败: {e}")))
}

fn expected_target_type(kind: &str) -> Result<&'static str, ApiErr> {
    match kind {
        "chapter" | "knowledge" | "pattern" => Ok("knowledge_node"),
        "method" | "core_competence" => Ok("tag"),
        other => Err(bad_request(format!("未知候选维度: {other}"))),
    }
}

fn expected_tree_kind(kind: &str) -> Result<&'static str, ApiErr> {
    match kind {
        "chapter" => Ok("chapter"),
        "knowledge" => Ok("knowledge"),
        "pattern" => Ok("ability"),
        other => Err(bad_request(format!("维度 {other} 不能挂到知识树"))),
    }
}

fn expected_tag_category(kind: &str) -> Result<TagCategory, ApiErr> {
    match kind {
        "method" => Ok(TagCategory::Method),
        "core_competence" => Ok(TagCategory::CoreCompetence),
        other => Err(bad_request(format!("维度 {other} 不能创建扁平标签"))),
    }
}

fn kind_tree_mismatch_msg(kind: &str) -> String {
    match kind {
        "chapter" => "章节候选只能挂到章节树".into(),
        "knowledge" => "知识点候选只能挂到知识树".into(),
        "pattern" => "题型专题候选只能挂到专题树".into(),
        other => format!("维度 {other} 与目标树类型不匹配"),
    }
}

fn uuid_to_ltree_segment(id: Uuid) -> String {
    id.to_string().replace('-', "_")
}

async fn record_merge_tx(
    tx: &mut Tx<'_>,
    target_type: &str,
    source_id: Uuid,
    target_id: Uuid,
    operator_id: Uuid,
    reason: Option<&str>,
) -> Result<(), ApiErr> {
    sqlx::query(
        r#"
        INSERT INTO tag_merge_records (target_type, source_tag_id, target_tag_id, operator_id, operator_type, reason)
        VALUES ($1, $2, $3, $4, 'admin', $5)
        "#,
    )
    .bind(target_type)
    .bind(source_id)
    .bind(target_id)
    .bind(operator_id)
    .bind(reason)
    .execute(&mut **tx)
    .await
    .map_err(|e| db_err(format!("写合并审计失败: {e}")))?;
    Ok(())
}

async fn finish_candidate(
    tx: &mut Tx<'_>,
    candidate_id: Uuid,
    reviewer_id: Uuid,
    status: &str,
    review_note: Option<&str>,
) -> Result<(), ApiErr> {
    sqlx::query(
        r#"
        UPDATE tag_candidates
        SET status = $2, reviewed_by = $3, reviewed_at = NOW(), review_note = $4
        WHERE id = $1
        "#,
    )
    .bind(candidate_id)
    .bind(status)
    .bind(reviewer_id)
    .bind(review_note)
    .execute(&mut **tx)
    .await
    .map_err(|e| db_err(format!("更新候选状态失败: {e}")))?;
    Ok(())
}

async fn append_node_alias(tx: &mut Tx<'_>, node_id: Uuid, alias: &str) -> Result<(), ApiErr> {
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
    .bind(node_id)
    .bind(alias)
    .execute(&mut **tx)
    .await
    .map_err(|e| db_err(format!("追加节点别名失败: {e}")))?;
    Ok(())
}

async fn append_tag_alias(tx: &mut Tx<'_>, tag_id: Uuid, alias: &str) -> Result<(), ApiErr> {
    sqlx::query(
        r#"
        UPDATE tags
        SET aliases = CASE
                WHEN EXISTS (
                    SELECT 1 FROM jsonb_array_elements(aliases) a
                    WHERE a->>'alias' = $2 OR a #>> '{}' = $2
                )
                THEN aliases
                ELSE aliases || jsonb_build_array(jsonb_build_object('alias', $2))
            END
        WHERE id = $1
        "#,
    )
    .bind(tag_id)
    .bind(alias)
    .execute(&mut **tx)
    .await
    .map_err(|e| db_err(format!("追加标签别名失败: {e}")))?;
    Ok(())
}

async fn link_question_node(
    tx: &mut Tx<'_>,
    question_id: Uuid,
    node_id: Uuid,
    confidence: Option<rust_decimal::Decimal>,
) -> Result<bool, ApiErr> {
    let result = sqlx::query(
        r#"
        INSERT INTO question_knowledge_nodes (question_id, node_id, is_primary, relevance_score, ai_confidence, source, created_at)
        VALUES ($1, $2, FALSE, NULL, $3, 'ai', NOW())
        ON CONFLICT (question_id, node_id) DO NOTHING
        "#,
    )
    .bind(question_id)
    .bind(node_id)
    .bind(confidence)
    .execute(&mut **tx)
    .await
    .map_err(|e| db_err(format!("关联题目到节点失败: {e}")))?;
    Ok(result.rows_affected() > 0)
}

async fn link_question_tag(
    tx: &mut Tx<'_>,
    question_id: Uuid,
    tag_id: Uuid,
    confidence: Option<rust_decimal::Decimal>,
) -> Result<bool, ApiErr> {
    let result = sqlx::query(
        r#"
        INSERT INTO question_tags_relation (question_id, tag_id, source, ai_confidence)
        VALUES ($1, $2, 'ai', $3)
        ON CONFLICT (question_id, tag_id) DO NOTHING
        "#,
    )
    .bind(question_id)
    .bind(tag_id)
    .bind(confidence)
    .execute(&mut **tx)
    .await
    .map_err(|e| db_err(format!("关联题目到标签失败: {e}")))?;
    Ok(result.rows_affected() > 0)
}

async fn maybe_link_and_count_node(
    tx: &mut Tx<'_>,
    candidate: &TagCandidate,
    node_id: Uuid,
) -> Result<(), ApiErr> {
    let Some(question_id) = candidate.source_question_id else {
        return Ok(());
    };
    // question_count 由 trg_qkn_sync_count 在 INSERT/DELETE 时维护，此处只写关联
    let _ = link_question_node(tx, question_id, node_id, candidate.ai_confidence).await?;
    Ok(())
}

async fn maybe_link_and_count_tag(
    tx: &mut Tx<'_>,
    candidate: &TagCandidate,
    tag_id: Uuid,
) -> Result<(), ApiErr> {
    let Some(question_id) = candidate.source_question_id else {
        return Ok(());
    };
    if link_question_tag(tx, question_id, tag_id, candidate.ai_confidence).await? {
        sqlx::query("UPDATE tags SET use_count = use_count + 1 WHERE id = $1")
            .bind(tag_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| db_err(format!("更新标签计数失败: {e}")))?;
    }
    Ok(())
}

async fn load_tree_kind(tx: &mut Tx<'_>, tree_id: Uuid) -> Result<String, ApiErr> {
    let row: Option<(String, bool)> = sqlx::query_as(
        "SELECT kind::text, is_active FROM knowledge_trees WHERE id = $1",
    )
    .bind(tree_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| db_err(format!("查询知识树失败: {e}")))?;
    match row {
        Some((kind, true)) => Ok(kind),
        Some((_, false)) => Err(bad_request("目标知识树已停用")),
        None => Err(bad_request("目标知识树不存在")),
    }
}

async fn create_knowledge_node(
    tx: &mut Tx<'_>,
    tree_id: Uuid,
    parent_id: Option<Uuid>,
    name: &str,
) -> Result<Uuid, ApiErr> {
    let (parent_path, parent_depth): (Option<String>, i16) = match parent_id {
        Some(pid) => {
            let row: Option<(String, i16, Uuid, bool)> = sqlx::query_as(
                "SELECT path::text, depth, tree_id, is_active FROM knowledge_nodes WHERE id = $1",
            )
            .bind(pid)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| db_err(format!("查询父节点失败: {e}")))?;
            let Some((path, depth, parent_tree, active)) = row else {
                return Err(bad_request("父节点不存在"));
            };
            if !active {
                return Err(bad_request("父节点不存在或已停用"));
            }
            if parent_tree != tree_id {
                return Err(bad_request("父节点不属于所选知识树"));
            }
            (Some(path), depth)
        }
        None => (None, -1),
    };

    let new_id = Uuid::new_v4();
    let path_seg = uuid_to_ltree_segment(new_id);
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
    .bind(parent_id)
    .bind(&new_path)
    .bind(new_depth)
    .bind(name)
    .execute(&mut **tx)
    .await
    .map_err(|e| db_err(format!("创建新节点失败: {e}")))?;

    Ok(new_id)
}

async fn resolve_active_node(
    tx: &mut Tx<'_>,
    node_id: Uuid,
    expected_kind: &str,
    candidate_kind: &str,
) -> Result<Uuid, ApiErr> {
    let row: Option<(Uuid, bool, String, Option<Uuid>, String)> = sqlx::query_as(
        r#"
        SELECT kn.id, kn.is_active, kn.status, kn.canonical_id, kt.kind::text
        FROM knowledge_nodes kn
        JOIN knowledge_trees kt ON kt.id = kn.tree_id
        WHERE kn.id = $1
        "#,
    )
    .bind(node_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| db_err(format!("查询目标节点失败: {e}")))?;

    let Some((id, active, status, canonical_id, tree_kind)) = row else {
        return Err(bad_request("目标节点不存在"));
    };
    if !active || status != "active" {
        return Err(bad_request("目标节点不存在或已停用"));
    }
    if canonical_id.is_some() {
        return Err(bad_request("目标节点已是合并标签，不能作为审核目标"));
    }
    if tree_kind != expected_kind {
        return Err(bad_request(kind_tree_mismatch_msg(candidate_kind)));
    }
    Ok(id)
}

async fn resolve_active_tag(
    tx: &mut Tx<'_>,
    tag_id: Uuid,
    expected: &TagCategory,
) -> Result<Uuid, ApiErr> {
    let row: Option<(Uuid, bool, TagCategory)> =
        sqlx::query_as("SELECT id, is_active, category FROM tags WHERE id = $1")
            .bind(tag_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| db_err(format!("查询目标标签失败: {e}")))?;
    let Some((id, active, category)) = row else {
        return Err(bad_request("目标标签不存在"));
    };
    if !active {
        return Err(bad_request("目标标签不存在或已停用"));
    }
    if &category != expected {
        return Err(bad_request("候选维度与目标标签类别不一致"));
    }
    Ok(id)
}

async fn create_tag(
    tx: &mut Tx<'_>,
    name: &str,
    category: &TagCategory,
) -> Result<Uuid, ApiErr> {
    let id = Uuid::new_v4();
    let path = uuid_to_ltree_segment(id);
    let now = chrono::Utc::now();
    sqlx::query(
        r#"
        INSERT INTO tags (id, parent_id, name, category, path, aliases, description,
                          space_id, use_count, is_active, created_at)
        VALUES ($1, NULL, $2, $3, text2ltree($4), '[]'::jsonb, NULL, NULL, 0, TRUE, $5)
        "#,
    )
    .bind(id)
    .bind(name)
    .bind(category)
    .bind(&path)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("unique") || msg.contains("duplicate") {
            (
                StatusCode::CONFLICT,
                Json(json!({"error": "同名标签已存在，请改用别名或并入已有标签"})),
            )
        } else {
            db_err(format!("创建新标签失败: {e}"))
        }
    })?;
    Ok(id)
}

/// canonical 环检测：从 target 沿 canonical_id 链走，若到达 source → 环
async fn would_form_cycle(
    pool: &sqlx::PgPool,
    source_id: Uuid,
    target_id: Uuid,
) -> Result<bool, ApiErr> {
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
) -> Result<i64, ApiErr> {
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

    sqlx::query("DELETE FROM question_knowledge_nodes WHERE node_id = $1")
        .bind(source_id)
        .execute(pool)
        .await
        .map_err(|e| db_err(format!("清理源节点关联失败: {e}")))?;

    Ok(migrated)
}

fn apply_filters(builder: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>, query: &TagCandidateQuery) {
    if let Some(status) = query.status.clone() {
        builder.push(" AND status = ").push_bind(status);
    }
    if let Some(kind) = query.kind.clone() {
        builder.push(" AND kind = ").push_bind(kind);
    }
    if let Some(target_type) = query.target_type.clone() {
        builder.push(" AND target_type = ").push_bind(target_type);
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/admin/tag-candidates — 候选队列
pub async fn list_tag_candidates(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Query(query): Query<TagCandidateQuery>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    if !is_admin_user(&auth) {
        return Err(forbidden());
    }

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).min(100);
    let offset = (page - 1) * page_size;

    let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(&format!(
        "SELECT {CANDIDATE_COLUMNS} FROM tag_candidates WHERE 1=1"
    ));
    apply_filters(&mut builder, &query);
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
    apply_filters(&mut count_builder, &query);
    let total: i64 = count_builder
        .build_query_scalar()
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("统计候选总数失败: {e}")))?;

    Ok(Json(json!({ "items": items, "total": total, "page": page, "page_size": page_size })))
}

/// GET /api/v1/admin/tag-candidates/{id} — 候选详情（含来源题目题干、建议节点/标签摘要）
pub async fn get_tag_candidate(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    if !is_admin_user(&auth) {
        return Err(forbidden());
    }
    let candidate = load_candidate(&state.pool, id).await?.ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(json!({"error": "候选不存在"})))
    })?;

    let source_question: Option<serde_json::Value> = match candidate.source_question_id {
        Some(qid) => {
            let row: Option<(String, String, Option<serde_json::Value>)> = sqlx::query_as(
                r#"
                SELECT stem, question_type::text, options
                FROM questions
                WHERE id = $1
                "#,
            )
            .bind(qid)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| db_err(format!("查询来源题目失败: {e}")))?;
            row.map(|(stem, question_type, options)| {
                json!({
                    "id": qid,
                    "stem": stem,
                    "question_type": question_type,
                    "options": options,
                })
            })
        }
        None => None,
    };
    let stem = source_question
        .as_ref()
        .and_then(|q| q.get("stem"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    let task_id = candidate.source_task_id;

    let suggested_node = match candidate.suggested_node_id {
        Some(nid) => {
            let row: Option<(Uuid, String, String, String, String)> = sqlx::query_as(
                r#"
                SELECT
                  kn.id,
                  kn.name,
                  COALESCE((
                    SELECT string_agg(anc.name, ' / ' ORDER BY anc.depth)
                    FROM knowledge_nodes anc
                    WHERE anc.tree_id = kn.tree_id
                      AND kn.path <@ anc.path
                      AND anc.is_active = TRUE
                  ), kn.name) AS name_path,
                  kt.name AS tree_name,
                  kt.kind::text AS tree_kind
                FROM knowledge_nodes kn
                JOIN knowledge_trees kt ON kt.id = kn.tree_id
                WHERE kn.id = $1
                "#,
            )
            .bind(nid)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| db_err(format!("查询建议节点失败: {e}")))?;
            row.map(|(id, name, name_path, tree_name, tree_kind)| {
                json!({
                    "id": id,
                    "name": name,
                    "name_path": name_path,
                    "tree_name": tree_name,
                    "tree_kind": tree_kind,
                })
            })
        }
        None => None,
    };

    let suggested_tag = match candidate.suggested_tag_id {
        Some(tid) => {
            let row: Option<(Uuid, String, String)> = sqlx::query_as(
                "SELECT id, name, category::text FROM tags WHERE id = $1",
            )
            .bind(tid)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| db_err(format!("查询建议标签失败: {e}")))?;
            row.map(|(id, name, category)| {
                json!({
                    "id": id,
                    "name": name,
                    "category": category,
                })
            })
        }
        None => None,
    };

    Ok(Json(json!({
        "candidate": candidate,
        "source_stem": stem,
        "source_question": source_question,
        "source_task_id": task_id,
        "suggested_node": suggested_node,
        "suggested_tag": suggested_tag,
    })))
}

/// POST /api/v1/admin/tag-candidates/{id}/approve — 按目标类型审核
pub async fn approve_tag_candidate(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<ApproveCandidateRequest>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    if !is_admin_user(&auth) {
        return Err(forbidden());
    }
    let candidate = load_candidate(&state.pool, id).await?.ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(json!({"error": "候选不存在"})))
    })?;
    if candidate.status != "pending" {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": format!("候选已处理（status={}）", candidate.status)})),
        ));
    }

    let expected_tt = expected_target_type(&candidate.kind)?;
    if candidate.target_type != expected_tt {
        return Err(bad_request("候选目标类型与维度不一致"));
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| db_err(format!("开启事务失败: {e}")))?;

    let status = if req.action == "merge" {
        "merged"
    } else {
        "approved"
    };
    let reason = req.reason.as_deref().filter(|s| !s.trim().is_empty());

    let mut target_node_id: Option<Uuid> = None;
    let mut target_tag_id: Option<Uuid> = None;

    match expected_tt {
        "knowledge_node" => {
            let tree_kind = expected_tree_kind(&candidate.kind)?;
            let node_id = match req.action.as_str() {
                "new_node" => {
                    let tree_id = req.tree_id.ok_or_else(|| bad_request("new_node 分支必须提供 tree_id"))?;
                    let actual_kind = load_tree_kind(&mut tx, tree_id).await?;
                    if actual_kind != tree_kind {
                        return Err(bad_request(kind_tree_mismatch_msg(&candidate.kind)));
                    }
                    let name = req
                        .name
                        .clone()
                        .filter(|n| !n.trim().is_empty())
                        .unwrap_or_else(|| candidate.raw_name.clone());
                    create_knowledge_node(&mut tx, tree_id, req.parent_id, &name).await?
                }
                "alias" | "merge" => {
                    let target_id = req
                        .target_node_id
                        .or(req.target_tag_id)
                        .ok_or_else(|| bad_request("alias/merge 分支必须提供 target_node_id"))?;
                    let node_id =
                        resolve_active_node(&mut tx, target_id, tree_kind, &candidate.kind).await?;
                    if req.action == "merge" {
                        record_merge_tx(
                            &mut tx,
                            "knowledge_node",
                            candidate.id,
                            node_id,
                            auth.id,
                            reason,
                        )
                        .await?;
                    }
                    append_node_alias(&mut tx, node_id, &candidate.raw_name).await?;
                    node_id
                }
                other => return Err(bad_request(format!("未知审核动作: {other}"))),
            };
            maybe_link_and_count_node(&mut tx, &candidate, node_id).await?;
            target_node_id = Some(node_id);
        }
        "tag" => {
            let category = expected_tag_category(&candidate.kind)?;
            let tag_id = match req.action.as_str() {
                "new_node" => {
                    let name = req
                        .name
                        .clone()
                        .filter(|n| !n.trim().is_empty())
                        .unwrap_or_else(|| candidate.raw_name.clone());
                    create_tag(&mut tx, &name, &category).await?
                }
                "alias" | "merge" => {
                    let target_id = req
                        .target_tag_id
                        .or(req.target_node_id)
                        .ok_or_else(|| bad_request("alias/merge 分支必须提供 target_tag_id"))?;
                    let tag_id = resolve_active_tag(&mut tx, target_id, &category).await?;
                    if req.action == "merge" {
                        record_merge_tx(&mut tx, "tag", candidate.id, tag_id, auth.id, reason)
                            .await?;
                    }
                    append_tag_alias(&mut tx, tag_id, &candidate.raw_name).await?;
                    tag_id
                }
                other => return Err(bad_request(format!("未知审核动作: {other}"))),
            };
            maybe_link_and_count_tag(&mut tx, &candidate, tag_id).await?;
            target_tag_id = Some(tag_id);
        }
        _ => return Err(bad_request("未知候选目标类型")),
    }

    finish_candidate(&mut tx, candidate.id, auth.id, status, reason).await?;

    tx.commit()
        .await
        .map_err(|e| db_err(format!("提交事务失败: {e}")))?;

    if let Some(id) = target_node_id {
        crate::ai::embedding::spawn_refresh_node_embedding(state.pool.clone(), id);
    }
    if let Some(id) = target_tag_id {
        crate::ai::embedding::spawn_refresh_tag_embedding(state.pool.clone(), id);
    }

    Ok(Json(json!({
        "message": "已审核",
        "action": req.action,
        "status": status,
        "target_node_id": target_node_id,
        "target_tag_id": target_tag_id,
    })))
}

/// POST /api/v1/admin/tag-candidates/{id}/reject — 拒绝候选
pub async fn reject_tag_candidate(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<RejectCandidateRequest>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    if !is_admin_user(&auth) {
        return Err(forbidden());
    }
    let candidate = load_candidate(&state.pool, id).await?.ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(json!({"error": "候选不存在"})))
    })?;
    if candidate.status != "pending" {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": format!("候选已处理（status={}）", candidate.status)})),
        ));
    }

    let reason = req.reason.as_deref().filter(|s| !s.trim().is_empty());
    sqlx::query(
        r#"
        UPDATE tag_candidates
        SET status = 'rejected', reviewed_by = $2, reviewed_at = NOW(), review_note = $3
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(auth.id)
    .bind(reason)
    .execute(&state.pool)
    .await
    .map_err(|e| db_err(format!("拒绝候选失败: {e}")))?;

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
) -> Result<Json<serde_json::Value>, ApiErr> {
    if !is_admin_user(&auth) {
        return Err(forbidden());
    }
    if source_id == req.target_id {
        return Err(bad_request("不能将标签合并到自身"));
    }

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
        return Err(bad_request("源标签或目标标签不存在"));
    }

    let target_is_merged: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM knowledge_nodes WHERE id = $1 AND canonical_id IS NOT NULL)",
    )
    .bind(req.target_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| db_err(format!("查询目标标签状态失败: {e}")))?;
    if target_is_merged {
        return Err(bad_request("目标标签已是合并标签（merged），不能作为合并目标"));
    }

    if would_form_cycle(&state.pool, source_id, req.target_id).await? {
        return Err(bad_request("合并将形成 canonical 环，已拒绝"));
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| db_err(format!("开启事务失败: {e}")))?;

    let migrated = migrate_question_relations(&state.pool, source_id, req.target_id).await?;

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

    sqlx::query(
        "UPDATE knowledge_nodes SET question_count = question_count + $2, updated_at = NOW() WHERE id = $1",
    )
    .bind(req.target_id)
    .bind(migrated as i32)
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err(format!("更新目标计数失败: {e}")))?;

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
) -> Result<Json<serde_json::Value>, ApiErr> {
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
