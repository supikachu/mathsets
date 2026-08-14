use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use axum_extra::extract::Query;
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::auth::permissions::{
    can_access_space, can_edit_question, can_publish_question, can_review_question,
    can_write_in_space, ensure_personal_space, ensure_public_space, get_member_meta, get_space,
    is_admin_user, list_reviewers, PermissionError,
};
use crate::models::question::{
    CreateQuestionRequest, KnowledgeNodeSummary, Question, QuestionDetail,
    QuestionQuery, QuestionStatus, QuestionSummary, RejectRequest, SubmitReviewRequest,
    TagSummary, TransferQuestionRequest, UpdateQuestionRequest,
};
use crate::models::space::SpaceKind;
use crate::models::user::{GlobalRole, User};
use crate::models::PageResult;
use crate::AppState;

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

pub(crate) async fn save_version(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    question_id: Uuid,
    version: i32,
    created_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    let question = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(question_id)
        .fetch_one(&mut **tx)
        .await?;

    let snapshot = serde_json::to_value(&question).unwrap_or_default();

    sqlx::query(
        r#"
        INSERT INTO question_versions (id, question_id, version, snapshot, created_by, created_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(question_id)
    .bind(version)
    .bind(&snapshot)
    .bind(created_by)
    .bind(chrono::Utc::now())
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub(crate) async fn update_knowledge_nodes(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    question_id: Uuid,
    node_ids: &[Uuid],
    primary_node_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    // 1. 查询当前已有关联（node_id, is_primary, source, ai_confidence）
    let existing: Vec<(Uuid, bool, String, Option<rust_decimal::Decimal>)> = sqlx::query_as(
        r#"SELECT node_id, is_primary, source::text, ai_confidence
           FROM question_knowledge_nodes WHERE question_id = $1"#,
    )
    .bind(question_id)
    .fetch_all(&mut **tx)
    .await?;

    let existing_map: std::collections::HashMap<Uuid, (bool, String, Option<rust_decimal::Decimal>)> =
        existing
            .into_iter()
            .map(|(id, is_p, src, conf)| (id, (is_p, src, conf)))
            .collect();

    let new_set: std::collections::HashSet<Uuid> = node_ids.iter().copied().collect();

    // 2. 计算差分集合
    let removed: Vec<Uuid> = existing_map
        .keys()
        .filter(|id| !new_set.contains(id))
        .copied()
        .collect();
    let added: Vec<Uuid> = node_ids
        .iter()
        .filter(|id| !existing_map.contains_key(id))
        .copied()
        .collect();

    // 3. 删除被移除的关联
    if !removed.is_empty() {
        sqlx::query(
            "DELETE FROM question_knowledge_nodes WHERE question_id = $1 AND node_id = ANY($2)",
        )
        .bind(question_id)
        .bind(&removed)
        .execute(&mut **tx)
        .await?;
    }

    // 4. 插入新增关联（统一标记为 manual；AI 来源只能由 upsert_ai_knowledge_nodes 写入）
    //    保留 retained 关联的 source 与 ai_confidence，避免覆盖 AI 审计数据
    for node_id in &added {
        let is_primary = primary_node_id == Some(*node_id);
        sqlx::query(
            r#"
            INSERT INTO question_knowledge_nodes
              (question_id, node_id, is_primary, source, created_at)
            VALUES ($1, $2, $3, 'manual', NOW())
            ON CONFLICT (question_id, node_id) DO NOTHING
            "#,
        )
        .bind(question_id)
        .bind(node_id)
        .bind(is_primary)
        .execute(&mut **tx)
        .await?;
    }

    // 5. 主知识点切换：先把当前题所有 is_primary=true 清零，再设置目标
    //    用 ON CONFLICT 统一处理 added/retained 两种情况，仅更新 is_primary，不破坏 source/ai_confidence
    sqlx::query(
        "UPDATE question_knowledge_nodes SET is_primary = FALSE WHERE question_id = $1 AND is_primary = TRUE",
    )
    .bind(question_id)
    .execute(&mut **tx)
    .await?;

    if let Some(primary_id) = primary_node_id {
        sqlx::query(
            r#"
            INSERT INTO question_knowledge_nodes
              (question_id, node_id, is_primary, source, created_at)
            VALUES ($1, $2, TRUE, 'manual', NOW())
            ON CONFLICT (question_id, node_id) DO UPDATE SET is_primary = TRUE
            "#,
        )
        .bind(question_id)
        .bind(primary_id)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

/// AI 专用的知识点关联 Upsert（B3 新增）
///
/// 与 `update_knowledge_nodes` 的差异：
/// - `source = 'ai'`（审计追溯，区分人工/AI 标注）
/// - 将 `KnowledgeNodeMatch.score`（f32）写入 `ai_confidence`（NUMERIC(5,4)）
/// - `ON CONFLICT DO UPDATE`（覆盖已有 manual 关联的 source 与置信度）
pub(crate) async fn upsert_ai_knowledge_nodes(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    question_id: Uuid,
    matches: &[crate::handlers::ai_tagging::KnowledgeNodeMatch],
    primary_node_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM question_knowledge_nodes WHERE question_id = $1")
        .bind(question_id)
        .execute(&mut **tx)
        .await?;

    for m in matches {
        let is_primary = primary_node_id == Some(m.node_id);
        // f32 → rust_decimal::Decimal（ai_confidence 列为 NUMERIC(5,4)）
        let ai_confidence = {
            use rust_decimal::prelude::FromPrimitive;
            rust_decimal::Decimal::from_f32(m.score)
        };

        sqlx::query(
            r#"
            INSERT INTO question_knowledge_nodes
              (question_id, node_id, is_primary, source, ai_confidence, created_at)
            VALUES ($1, $2, $3, 'ai', $4, NOW())
            ON CONFLICT (question_id, node_id) DO UPDATE SET
              is_primary = EXCLUDED.is_primary,
              source = 'ai',
              ai_confidence = EXCLUDED.ai_confidence
            "#,
        )
        .bind(question_id)
        .bind(m.node_id)
        .bind(is_primary)
        .bind(ai_confidence)
        .execute(&mut **tx)
        .await?;
    }

    // 确保 primary 唯一性：若 primary_node_id 不在 matches 中，单独插入
    if let Some(primary_id) = primary_node_id {
        if !matches.iter().any(|m| m.node_id == primary_id) {
            sqlx::query(
                r#"
                INSERT INTO question_knowledge_nodes
                  (question_id, node_id, is_primary, source, created_at)
                VALUES ($1, $2, TRUE, 'ai', NOW())
                ON CONFLICT (question_id, node_id) DO UPDATE SET
                  is_primary = TRUE,
                  source = 'ai'
                "#,
            )
            .bind(question_id)
            .bind(primary_id)
            .execute(&mut **tx)
            .await?;
        }
    }

    Ok(())
}

async fn get_question_knowledge_nodes(
    pool: &sqlx::PgPool,
    question_id: Uuid,
) -> Result<Vec<KnowledgeNodeSummary>, sqlx::Error> {
    sqlx::query_as::<_, KnowledgeNodeSummary>(
        r#"
        SELECT kn.id, kn.tree_id, kn.name,
               kn.path::text AS path, kn.depth,
               kt.kind::text AS kind,
               qkn.is_primary, qkn.ai_confidence, qkn.source
        FROM knowledge_nodes kn
        JOIN knowledge_trees kt ON kt.id = kn.tree_id
        JOIN question_knowledge_nodes qkn ON qkn.node_id = kn.id
        WHERE qkn.question_id = $1
        ORDER BY qkn.is_primary DESC, kn.sort_order, kn.name
        "#,
    )
    .bind(question_id)
    .fetch_all(pool)
    .await
}

/// 同步题目标签关联：增量更新 — 仅对新增关联递增 use_count，仅对移除关联递减
async fn update_question_tags(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    question_id: Uuid,
    tag_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    // 1. 查询当前已有关联
    let existing: Vec<Uuid> = sqlx::query_scalar(
        "SELECT tag_id FROM question_tags_relation WHERE question_id = $1",
    )
    .bind(question_id)
    .fetch_all(&mut **tx)
    .await?;

    let existing_set: std::collections::HashSet<Uuid> = existing.into_iter().collect();

    // 2. 删除被移除的关联，递减 use_count
    let removed: Vec<Uuid> = existing_set
        .iter()
        .filter(|id| !tag_ids.contains(id))
        .copied()
        .collect();
    if !removed.is_empty() {
        sqlx::query("DELETE FROM question_tags_relation WHERE question_id = $1 AND tag_id = ANY($2)")
            .bind(question_id)
            .bind(&removed)
            .execute(&mut **tx)
            .await?;
        sqlx::query("UPDATE tags SET use_count = GREATEST(use_count - 1, 0) WHERE id = ANY($1)")
            .bind(&removed)
            .execute(&mut **tx)
            .await?;
    }

    // 3. 插入新增关联，仅对新关联递增 use_count
    let added: Vec<Uuid> = tag_ids
        .iter()
        .filter(|id| !existing_set.contains(id))
        .copied()
        .collect();
    for tag_id in &added {
        sqlx::query(
            r#"
            INSERT INTO question_tags_relation (question_id, tag_id)
            VALUES ($1, $2)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(question_id)
        .bind(tag_id)
        .execute(&mut **tx)
        .await?;

        sqlx::query("UPDATE tags SET use_count = use_count + 1 WHERE id = $1")
            .bind(tag_id)
            .execute(&mut **tx)
            .await?;
    }

    Ok(())
}

/// 同步题目 ↔ 试卷关联（全量覆盖：删除多余的，插入新增的）
/// 在题目创建/更新事务内调用，保证原子性。
pub(crate) async fn sync_question_papers(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    question_id: Uuid,
    paper_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    // 1. 查询当前已关联的 paper_id
    let existing: Vec<Uuid> =
        sqlx::query_scalar("SELECT paper_id FROM paper_questions WHERE question_id = $1")
            .bind(question_id)
            .fetch_all(&mut **tx)
            .await?;

    let existing_set: std::collections::HashSet<Uuid> = existing.into_iter().collect();

    // 2. 删除被移除的关联
    let removed: Vec<Uuid> = existing_set
        .iter()
        .filter(|id| !paper_ids.contains(id))
        .copied()
        .collect();
    if !removed.is_empty() {
        sqlx::query(
            "DELETE FROM paper_questions WHERE question_id = $1 AND paper_id = ANY($2)",
        )
        .bind(question_id)
        .bind(&removed)
        .execute(&mut **tx)
        .await?;
    }

    // 3. 插入新增关联（去重，默认 sort_order=0, score=0）
    let added: Vec<Uuid> = paper_ids
        .iter()
        .filter(|id| !existing_set.contains(id))
        .copied()
        .collect();
    for paper_id in &added {
        let pq_id = Uuid::new_v4();
        let now = chrono::Utc::now();
        sqlx::query(
            r#"
            INSERT INTO paper_questions (id, paper_id, question_id, sort_order, score, section, created_at)
            VALUES ($1, $2, $3, 0, 0, NULL, $4)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(pq_id)
        .bind(paper_id)
        .bind(question_id)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }

    // 4. 同步 questions.paper_count 缓存字段
    let cnt: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM paper_questions WHERE question_id = $1")
        .bind(question_id)
        .fetch_one(&mut **tx)
        .await?;
    sqlx::query("UPDATE questions SET paper_count = $1 WHERE id = $2")
        .bind(cnt as i32)
        .bind(question_id)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

/// 获取题目的关联标签
async fn get_question_tags(
    pool: &sqlx::PgPool,
    question_id: Uuid,
) -> Result<Vec<TagSummary>, sqlx::Error> {
    sqlx::query_as::<_, TagSummary>(
        r#"
        SELECT t.id, t.name, t.category
        FROM tags t
        JOIN question_tags_relation qtr ON qtr.tag_id = t.id
        WHERE qtr.question_id = $1
        ORDER BY t.category, t.name
        "#,
    )
    .bind(question_id)
    .fetch_all(pool)
    .await
}

/// 自建标签原子化 Upsert：名称未入库则创建（use_count=1），已存在则递增 use_count
/// 返回所有标签的 ID（含已存在 + 新建），供合并到 tag_ids
async fn upsert_new_tags(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    new_tags: &[crate::models::question::NewTagInput],
    space_id: Option<Uuid>,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let mut ids = Vec::with_capacity(new_tags.len());
    for nt in new_tags {
        let name = nt.name.trim();
        if name.is_empty() {
            continue;
        }
        let id: Uuid = sqlx::query_scalar(
            r#"
            INSERT INTO tags (id, name, category, space_id, use_count, created_at)
            VALUES ($1, $2, $3, $4, 1, NOW())
            ON CONFLICT DO UPDATE SET use_count = tags.use_count + 1
            RETURNING id
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(name)
        .bind(&nt.category)
        .bind(space_id)
        .fetch_one(&mut **tx)
        .await?;
        ids.push(id);
    }
    Ok(ids)
}

pub(crate) async fn build_detail(
    pool: &sqlx::PgPool,
    auth: &AuthUser,
    question: Question,
    creator_name: Option<String>,
) -> Result<QuestionDetail, sqlx::Error> {
    // 关联数据查询失败时记录日志而非静默吞错（fix：旧实现 unwrap_or_default
    // 会把 SQL 错误吞成空数组，导致 tags/knowledge_nodes 在接口里"神秘消失"）
    let kns = match get_question_knowledge_nodes(pool, question.id).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                "build_detail: 查询题目 {} 的知识节点失败（返回空数组兜底）: {}",
                question.id,
                e
            );
            Vec::new()
        }
    };
    let tags = match get_question_tags(pool, question.id).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                "build_detail: 查询题目 {} 的标签失败（返回空数组兜底）: {}",
                question.id,
                e
            );
            Vec::new()
        }
    };
    let reviewer_ids = match list_reviewers(pool, question.id).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                "build_detail: 查询题目 {} 的审题人失败（返回空数组兜底）: {}",
                question.id,
                e
            );
            Vec::new()
        }
    };

    let mut detail = QuestionDetail::from((question.clone(), kns));
    detail.tags = tags;
    detail.creator_name = creator_name;
    detail.reviewer_ids = reviewer_ids;

    if let Ok(Some(space)) = get_space(pool, question.space_id).await {
        detail.can_review = can_review_question(
            pool,
            auth,
            &space,
            question.creator_id.into(),
            &question.status,
            question.id,
        )
        .await
        .unwrap_or(false);
    }

    Ok(detail)
}

pub(crate) fn db_err(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
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

// ---------------------------------------------------------------------------
// 统计
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct QuestionStats {
    pub total: i64,
    pub draft: i64,
    pub pending: i64,
    pub rejected: i64,
    pub published: i64,
    pub disabled: i64,
}

/// GET /api/v1/questions/stats
pub async fn question_stats(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Query(query): Query<QuestionQuery>,
) -> Result<Json<QuestionStats>, (StatusCode, Json<serde_json::Value>)> {
    let mut builder = sqlx::QueryBuilder::new(
        "SELECT status::text, COUNT(*) FROM questions q WHERE 1=1",
    );
    apply_access_filters(&mut builder, &auth, &query);
    builder.push(" GROUP BY status");

    let rows = builder
        .build_query_as::<(String, i64)>()
        .fetch_all(&state.pool)
        .await
        .map_err(|e| db_err(format!("统计失败: {}", e)))?;

    let mut stats = QuestionStats {
        total: 0,
        draft: 0,
        pending: 0,
        rejected: 0,
        published: 0,
        disabled: 0,
    };

    for (status, count) in rows {
        stats.total += count;
        match status.as_str() {
            "draft" => stats.draft = count,
            "pending" => stats.pending = count,
            "rejected" => stats.rejected = count,
            "published" => stats.published = count,
            "disabled" => stats.disabled = count,
            _ => {}
        }
    }

    Ok(Json(stats))
}

// ---------------------------------------------------------------------------
// 题目 CRUD
// ---------------------------------------------------------------------------

/// GET /api/v1/questions
pub async fn list_questions(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Query(query): Query<QuestionQuery>,
) -> Result<Json<PageResult<QuestionSummary>>, (StatusCode, Json<serde_json::Value>)> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).min(100);
    let offset = (page - 1) * page_size;

    if let Some(space_id) = query.space_id {
        let space = get_space(&state.pool, space_id)
            .await
            .map_err(|e| db_err(format!("查询空间失败: {}", e)))?
            .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;
        if !can_access_space(&state.pool, &auth, &space)
            .await
            .map_err(|e| db_err(format!("权限检查失败: {}", e)))?
        {
            return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权访问该空间"}))));
        }
    }

    let mut count_builder = sqlx::QueryBuilder::new("SELECT COUNT(*) FROM questions q WHERE 1=1");
    apply_access_filters(&mut count_builder, &auth, &query);
    apply_question_filters(&mut count_builder, &query);

    let total: i64 = count_builder
        .build_query_scalar::<i64>()
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("统计题目总数失败: {}", e)))?;

    let mut builder = sqlx::QueryBuilder::new(
        "SELECT q.id, q.stem, q.question_type, q.difficulty, q.status, \
         q.creator_id, u.display_name AS creator_name, q.created_at, q.updated_at, q.version, q.space_id \
         FROM questions q LEFT JOIN users u ON u.id = q.creator_id WHERE 1=1",
    );
    apply_access_filters(&mut builder, &auth, &query);
    apply_question_filters(&mut builder, &query);

    builder.push(" ORDER BY q.updated_at DESC LIMIT ");
    builder.push_bind(page_size as i64);
    builder.push(" OFFSET ");
    builder.push_bind(offset as i64);

    let questions = builder
        .build_query_as::<QuestionSummary>()
        .fetch_all(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?;

    Ok(Json(PageResult {
        items: questions,
        total,
        page,
        page_size,
    }))
}

/// 可见范围：指定 space / 我可访问的全部 / 我可审的待审
fn apply_access_filters<'a>(
    builder: &mut sqlx::QueryBuilder<'a, sqlx::Postgres>,
    auth: &'a AuthUser,
    query: &'a QuestionQuery,
) {
    if query.reviewable_by_me.unwrap_or(false) {
        // 【空间隔离】强制按 space_id 过滤，无论是否管理员，审核队列必须严格按空间隔离
        if let Some(space_id) = query.space_id {
            builder.push(" AND q.space_id = ");
            builder.push_bind(space_id);
        }
        // 待审 + （指定审题人含我 OR 无指定且空间可审）
        builder.push(" AND q.status = 'pending'");
        builder.push(" AND (");
        // 指定审题人
        builder.push(
            " EXISTS (SELECT 1 FROM question_reviewers qr WHERE qr.question_id = q.id AND qr.user_id = ",
        );
        builder.push_bind(auth.id);
        builder.push(")");
        // 或无指定审题人，且在可访问空间内
        builder.push(" OR (");
        builder.push(" NOT EXISTS (SELECT 1 FROM question_reviewers qr WHERE qr.question_id = q.id)");
        builder.push(" AND (");
        // 个人空间自审
        builder.push(
            " EXISTS (SELECT 1 FROM spaces s WHERE s.id = q.space_id AND s.kind = 'personal' AND s.owner_user_id = ",
        );
        builder.push_bind(auth.id);
        builder.push(" AND q.creator_id = ");
        builder.push_bind(auth.id);
        builder.push(")");
        // 或团队成员
        builder.push(
            " OR EXISTS (SELECT 1 FROM space_members sm WHERE sm.space_id = q.space_id AND sm.user_id = ",
        );
        builder.push_bind(auth.id);
        builder.push(")");
        // 管理员也不能跨空间审核 —— 移除 OR TRUE，强制 space_id 隔离
        builder.push("))");
        builder.push(")");
        return;
    }

    if let Some(space_id) = query.space_id {
        builder.push(" AND q.space_id = ");
        builder.push_bind(space_id);
        return;
    }

    // 默认可见：公共已发布 + 个人 + 团队成员 + Admin 全部
    if is_admin_user(&auth) {
        return;
    }

    builder.push(" AND (");
    builder.push(
        " EXISTS (SELECT 1 FROM spaces s WHERE s.id = q.space_id AND s.kind = 'public' AND q.status = 'published')",
    );
    builder.push(
        " OR EXISTS (SELECT 1 FROM spaces s WHERE s.id = q.space_id AND s.kind = 'personal' AND s.owner_user_id = ",
    );
    builder.push_bind(auth.id);
    builder.push(")");
    builder.push(
        " OR EXISTS (SELECT 1 FROM space_members sm WHERE sm.space_id = q.space_id AND sm.user_id = ",
    );
    builder.push_bind(auth.id);
    builder.push(")");
    builder.push(")");
}

fn apply_question_filters<'a>(
    builder: &mut sqlx::QueryBuilder<'a, sqlx::Postgres>,
    query: &'a QuestionQuery,
) {
    // 学段 / 学科过滤：匹配 metadata JSONB 中的 stage / subject 字段
    if let Some(ref stage) = query.stage {
        builder.push(" AND q.metadata->>'stage' = ");
        builder.push_bind(stage);
    }
    if let Some(ref subject) = query.subject {
        builder.push(" AND q.metadata->>'subject' = ");
        builder.push_bind(subject);
    }
    if let Some(ref status) = query.status {
        // reviewable_by_me 已强制 pending
        if !query.reviewable_by_me.unwrap_or(false) {
            builder.push(" AND q.status = ");
            builder.push_bind(status);
        }
    }
    if let Some(ref qt) = query.question_type {
        builder.push(" AND q.question_type = ");
        builder.push_bind(qt);
    }
    // 难度过滤：优先用精确 difficulty，否则用 difficulty_min/max 范围
    if let Some(diff) = query.difficulty {
        builder.push(" AND q.difficulty = ");
        builder.push_bind(diff);
    } else if query.difficulty_min.is_some() || query.difficulty_max.is_some() {
        if let Some(min) = query.difficulty_min {
            builder.push(" AND q.difficulty >= ");
            builder.push_bind(min);
        }
        if let Some(max) = query.difficulty_max {
            builder.push(" AND q.difficulty <= ");
            builder.push_bind(max);
        }
    }
    // 知识点节点多选过滤：支持 LTREE 子树包含（include_descendants=true）
    if let Some(ref node_ids) = query.knowledge_node_ids {
        if !node_ids.is_empty() {
            if query.include_descendants {
                // LTREE 子树查询：命中任一选中节点或其子孙节点
                // EXISTS 写法：题目关联的某个节点 kn，存在选中节点 root 使 kn.path <@ root.path
                builder.push(" AND EXISTS (SELECT 1 FROM question_knowledge_nodes qkn \
                              JOIN knowledge_nodes kn ON kn.id = qkn.node_id \
                              WHERE qkn.question_id = q.id \
                              AND EXISTS (SELECT 1 FROM knowledge_nodes root \
                                          WHERE root.id = ANY(");
                builder.push_bind(node_ids.clone());
                builder.push(") AND kn.path <@ root.path))");
            } else {
                // 精确匹配：题目关联的节点在 node_ids 中
                builder.push(" AND EXISTS (SELECT 1 FROM question_knowledge_nodes qkn \
                              WHERE qkn.question_id = q.id AND qkn.node_id = ANY(");
                builder.push_bind(node_ids.clone());
                builder.push("))");
            }
        }
    }
    // 标签多选过滤（OR 关系）
    if let Some(ref tag_ids) = query.tag_ids {
        if !tag_ids.is_empty() {
            builder.push(" AND EXISTS (SELECT 1 FROM question_tags_relation qtr \
                          WHERE qtr.question_id = q.id AND qtr.tag_id = ANY(");
            builder.push_bind(tag_ids.clone());
            builder.push("))");
        }
    }
    if let Some(ref creator) = query.creator_id {
        builder.push(" AND q.creator_id = ");
        builder.push_bind(creator);
    }
    if let Some(ref keyword) = query.keyword {
        builder.push(" AND q.stem ILIKE ");
        builder.push_bind(format!("%{}%", keyword));
    }
    // ── V2.1.1 来源/试卷元数据过滤（P1 检索） ──
    if let Some(year) = query.year {
        builder.push(" AND EXISTS (SELECT 1 FROM paper_questions pq \
                      JOIN papers p ON p.id = pq.paper_id \
                      WHERE pq.question_id = q.id AND p.year = ");
        builder.push_bind(year);
        builder.push(")");
    }
    if let Some(ref semester) = query.semester {
        builder.push(" AND EXISTS (SELECT 1 FROM paper_questions pq \
                      JOIN papers p ON p.id = pq.paper_id \
                      WHERE pq.question_id = q.id AND p.semester = ");
        builder.push_bind(semester);
        builder.push(")");
    }
    if let Some(ref region) = query.region {
        builder.push(" AND EXISTS (SELECT 1 FROM paper_questions pq \
                      JOIN papers p ON p.id = pq.paper_id \
                      WHERE pq.question_id = q.id AND (p.region_province = ");
        builder.push_bind(region);
        builder.push(" OR p.region_city = ");
        builder.push_bind(region);
        builder.push("))");
    }
    if let Some(ref source_type) = query.source_type {
        builder.push(" AND EXISTS (SELECT 1 FROM paper_questions pq \
                      JOIN papers p ON p.id = pq.paper_id \
                      WHERE pq.question_id = q.id AND p.source_type = ");
        builder.push_bind(source_type);
        builder.push(")");
    }
    if let Some(ref document_type) = query.document_type {
        builder.push(" AND (EXISTS (SELECT 1 FROM paper_questions pq \
                      JOIN papers p ON p.id = pq.paper_id \
                      JOIN documents d ON d.id = p.document_id \
                      WHERE pq.question_id = q.id AND d.document_type = ");
        builder.push_bind(document_type);
        builder.push(") OR EXISTS (SELECT 1 FROM collection_questions cq \
                      JOIN question_collections c ON c.id = cq.collection_id \
                      JOIN documents d ON d.id = c.document_id \
                      WHERE cq.question_id = q.id AND d.document_type = ");
        builder.push_bind(document_type);
        builder.push("))");
    }
    if let Some(ref collection_id) = query.collection_id {
        builder.push(" AND EXISTS (SELECT 1 FROM collection_questions cq \
                      WHERE cq.question_id = q.id AND cq.collection_id = ");
        builder.push_bind(collection_id);
        builder.push(")");
    }
}

/// POST /api/v1/questions — 创建草稿
pub async fn create_question(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateQuestionRequest>,
) -> Result<(StatusCode, Json<QuestionDetail>), (StatusCode, Json<serde_json::Value>)> {
    let space_id = if let Some(sid) = req.space_id {
        sid
    } else {
        // 默认个人空间
        let display = sqlx::query_scalar::<_, String>(
            "SELECT display_name FROM users WHERE id = $1",
        )
        .bind(auth_user.id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询用户失败: {}", e)))?
        .unwrap_or_else(|| "用户".into());

        ensure_personal_space(&state.pool, auth_user.id, &display)
            .await
            .map_err(|e| db_err(format!("创建个人空间失败: {}", e)))?
    };

    let space = get_space(&state.pool, space_id)
        .await
        .map_err(|e| db_err(format!("查询空间失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    if !can_write_in_space(&state.pool, &auth_user, &space)
        .await
        .map_err(|e| db_err(format!("权限检查失败: {}", e)))?
    {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权在该空间创建题目"}))));
    }

    let id = Uuid::new_v4();
    let now = chrono::Utc::now();
    let creator_id = auth_user.id;
    let version = 1;

    let mut tx = state.pool.begin().await.map_err(|e| db_err(format!("开启事务失败: {}", e)))?;

    // ── OCR 配额扣减与跨日重置（事务内，与题目写入保证绝对原子性） ──
    // 仅当录入方式为 "ocr" 时触发；manual / ai_parse / 缺省均跳过
    if req.input_method.as_deref() == Some("ocr") {
        // FOR UPDATE 行锁 — 防止并发请求超额扣减
        let user_row = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1 FOR UPDATE")
            .bind(auth_user.id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| db_err(format!("查询用户配额失败: {}", e)))?;

        let now_utc = chrono::Utc::now();
        // 跨日重置：当前时间已过重置点 → used 清零，reset_at 顺延至明天
        let (used, reset_at) = if now_utc > user_row.ocr_quota_reset_at {
            (0i32, now_utc + chrono::Duration::days(1))
        } else {
            (user_row.ocr_quota_used, user_row.ocr_quota_reset_at)
        };

        // 配额不足 — 403 拦截（tx 在 drop 时自动回滚，不污染任何数据）
        if used >= user_row.ocr_quota_daily {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "今日 OCR 配额已用尽，请明日重置后再试",
                    "code": "ERR_OCR_QUOTA_EXCEEDED",
                    "quota_daily": user_row.ocr_quota_daily,
                    "quota_used": used,
                    "reset_at": reset_at
                })),
            ));
        }

        // 配额充足 — 扣减 1 并写回（同事务内）
        let new_used = used + 1;
        sqlx::query(
            "UPDATE users SET ocr_quota_used = $1, ocr_quota_reset_at = $2 WHERE id = $3",
        )
        .bind(new_used)
        .bind(reset_at)
        .bind(auth_user.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| db_err(format!("更新 OCR 配额失败: {}", e)))?;
    }

    // ── V2.1.1 去重 hash（创建接口即时计算，计划书 §八） ──
    let content_hash = crate::util::normalize::compute_content_hash(
        &req.stem,
        req.options.as_ref(),
        &req.correct_answer,
        req.analysis.as_deref(),
    );
    let normalized_content_hash = crate::util::normalize::compute_normalized_content_hash(
        &req.stem,
        req.options.as_ref(),
        &req.correct_answer,
    );

    sqlx::query(
        r#"
        INSERT INTO questions (id, stem, question_type, difficulty, status,
            options, correct_answer, analysis, metadata,
            images, parent_id, sub_order,
            creator_id, created_at, updated_at, version, space_id,
            content_hash, normalized_content_hash)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, COALESCE($9, '{}'::jsonb),
            $10, $11, $12,
            $13, $14, $15, $16, $17, $18, $19)
        "#,
    )
    .bind(id)
    .bind(&req.stem)
    .bind(&req.question_type)
    .bind(&req.difficulty)
    .bind(QuestionStatus::Draft)
    .bind(&req.options)
    .bind(&req.correct_answer)
    .bind(&req.analysis)
    .bind(&req.metadata)
    .bind(&req.images)
    .bind(&req.parent_id)
    .bind(req.sub_order)
    .bind(creator_id)
    .bind(now)
    .bind(now)
    .bind(version)
    .bind(space_id)
    .bind(&content_hash)
    .bind(&normalized_content_hash)
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err(format!("创建题目失败: {}", e)))?;

    if let Some(ref node_ids) = req.knowledge_node_ids {
        update_knowledge_nodes(&mut tx, id, node_ids, req.primary_knowledge_node_id)
            .await
            .map_err(|e| db_err(format!("关联知识点失败: {}", e)))?;
    } else if let Some(primary_id) = req.primary_knowledge_node_id {
        // 只指定主知识点，无其他关联
        update_knowledge_nodes(&mut tx, id, &[], Some(primary_id))
            .await
            .map_err(|e| db_err(format!("关联主知识点失败: {}", e)))?;
    }

    // 合并已有 tag_ids + 自建 new_tags（Upsert 后取 ID）
    let tag_ids_provided = req.tag_ids.is_some();
    let new_tags_provided = req
        .new_tags
        .as_ref()
        .map_or(false, |nt| !nt.is_empty());
    let mut all_tag_ids: Vec<Uuid> = req.tag_ids.clone().unwrap_or_default();
    if new_tags_provided {
        let new_ids = upsert_new_tags(&mut tx, req.new_tags.as_ref().unwrap(), Some(space_id))
            .await
            .map_err(|e| db_err(format!("自建标签入库失败: {}", e)))?;
        all_tag_ids.extend(new_ids);
    }

    // tag_ids 或 new_tags 被显式提供时同步关联（含清空场景）
    if tag_ids_provided || new_tags_provided {
        update_question_tags(&mut tx, id, &all_tag_ids)
            .await
            .map_err(|e| db_err(format!("关联标签失败: {}", e)))?;
    }

    // 同步题目 ↔ 试卷关联（全量覆盖）
    if let Some(ref paper_ids) = req.paper_ids {
        sync_question_papers(&mut tx, id, paper_ids)
            .await
            .map_err(|e| db_err(format!("关联试卷失败: {}", e)))?;
    }

    save_version(&mut tx, id, version, Some(creator_id))
        .await
        .map_err(|e| db_err(format!("保存版本失败: {}", e)))?;

    tx.commit()
        .await
        .map_err(|e| db_err(format!("提交事务失败: {}", e)))?;

    let question = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?;

    let detail = build_detail(&state.pool, &auth_user, question, None)
        .await
        .map_err(|e| db_err(format!("构建详情失败: {}", e)))?;

    Ok((StatusCode::CREATED, Json(detail)))
}

/// GET /api/v1/questions/:id
pub async fn get_question(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<QuestionDetail>, (StatusCode, Json<serde_json::Value>)> {
    let question = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "题目不存在"}))))?;

    let space = get_space(&state.pool, question.space_id)
        .await
        .map_err(|e| db_err(format!("查询空间失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    // 公共未发布非成员不可见
    let can_see = can_access_space(&state.pool, &auth, &space)
        .await
        .map_err(|e| db_err(format!("权限检查失败: {}", e)))?
        && (space.kind != SpaceKind::Public || question.status == QuestionStatus::Published || is_admin_user(&auth));

    if !can_see {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权查看该题目"}))));
    }

    let creator_name = sqlx::query_scalar::<_, String>("SELECT display_name FROM users WHERE id = $1")
        .bind(question.creator_id)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten();

    let detail = build_detail(&state.pool, &auth, question, creator_name)
        .await
        .map_err(|e| db_err(format!("构建详情失败: {}", e)))?;

    Ok(Json(detail))
}

/// PUT /api/v1/questions/:id
pub async fn update_question(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateQuestionRequest>,
) -> Result<Json<QuestionDetail>, (StatusCode, Json<serde_json::Value>)> {
    let existing = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "题目不存在"}))))?;

    let space = get_space(&state.pool, existing.space_id)
        .await
        .map_err(|e| db_err(format!("查询空间失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    // 状态机保护：仅 Draft / Rejected 可编辑（已发布/Pending 需走审核流程）
    if existing.status != QuestionStatus::Draft && existing.status != QuestionStatus::Rejected {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "当前状态不允许编辑"})),
        ));
    }

    if !can_edit_question(
        &state.pool,
        &auth_user,
        &space,
        existing.creator_id.into(),
        &existing.status,
    )
    .await
    .map_err(|e| db_err(format!("权限检查失败: {}", e)))?
    {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权编辑该题目"}))));
    }

    let now = chrono::Utc::now();
    let old_version = existing.version;
    let new_version = old_version + 1;

    let mut tx = state.pool.begin().await.map_err(|e| db_err(format!("开启事务失败: {}", e)))?;

    let query_result = sqlx::query(
        r#"
        UPDATE questions SET
            stem = COALESCE($1, stem),
            question_type = COALESCE($2, question_type),
            difficulty = COALESCE($3, difficulty),
            options = COALESCE($4, options),
            correct_answer = COALESCE($5, correct_answer),
            analysis = COALESCE($6, analysis),
            metadata = COALESCE($7, metadata),
            images = COALESCE($8, images),
            parent_id = COALESCE($9, parent_id),
            sub_order = COALESCE($10, sub_order),
            status = 'draft'::question_status,
            updated_by = $11,
            updated_at = $12,
            version = $13
        WHERE id = $14 AND version = $15
        "#,
    )
    .bind(&req.stem)
    .bind(&req.question_type)
    .bind(&req.difficulty)
    .bind(&req.options)
    .bind(&req.correct_answer)
    .bind(&req.analysis)
    .bind(&req.metadata)
    .bind(&req.images)
    .bind(&req.parent_id)
    .bind(req.sub_order)
    .bind(auth_user.id)
    .bind(now)
    .bind(new_version)
    .bind(id)
    .bind(old_version)
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err(format!("更新题目失败: {}", e)))?;

    if query_result.rows_affected() == 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "题目已被他人修改，请刷新页面重新编辑",
                "code": "ERR_CONCURRENT_UPDATE"
            })),
        ));
    }

    // ── 状态降级：已发布题目被编辑后强制回到草稿，清除旧审题人记录 ──
    // 确保修改后的题目必须重新提交审核
    if existing.status != QuestionStatus::Draft {
        sqlx::query("DELETE FROM question_reviewers WHERE question_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| db_err(format!("清除审题人记录失败: {}", e)))?;
    }

    if let Some(ref node_ids) = req.knowledge_node_ids {
        update_knowledge_nodes(&mut tx, id, node_ids, req.primary_knowledge_node_id)
            .await
            .map_err(|e| db_err(format!("更新知识点关联失败: {}", e)))?;
    } else if let Some(primary_id) = req.primary_knowledge_node_id {
        update_knowledge_nodes(&mut tx, id, &[], Some(primary_id))
            .await
            .map_err(|e| db_err(format!("更新主知识点关联失败: {}", e)))?;
    }

    // 合并已有 tag_ids + 自建 new_tags（Upsert 后取 ID）
    let tag_ids_provided = req.tag_ids.is_some();
    let new_tags_provided = req
        .new_tags
        .as_ref()
        .map_or(false, |nt| !nt.is_empty());
    let mut all_tag_ids: Vec<Uuid> = req.tag_ids.clone().unwrap_or_default();
    if new_tags_provided {
        let new_ids = upsert_new_tags(&mut tx, req.new_tags.as_ref().unwrap(), Some(existing.space_id))
            .await
            .map_err(|e| db_err(format!("自建标签入库失败: {}", e)))?;
        all_tag_ids.extend(new_ids);
    }

    // tag_ids 或 new_tags 被显式提供时同步关联（含清空场景）
    if tag_ids_provided || new_tags_provided {
        update_question_tags(&mut tx, id, &all_tag_ids)
            .await
            .map_err(|e| db_err(format!("更新标签关联失败: {}", e)))?;
    }

    // 同步题目 ↔ 试卷关联（全量覆盖）
    if let Some(ref paper_ids) = req.paper_ids {
        sync_question_papers(&mut tx, id, paper_ids)
            .await
            .map_err(|e| db_err(format!("更新试卷关联失败: {}", e)))?;
    }

    save_version(&mut tx, id, new_version, Some(auth_user.id))
        .await
        .map_err(|e| db_err(format!("保存版本失败: {}", e)))?;

    tx.commit()
        .await
        .map_err(|e| db_err(format!("提交事务失败: {}", e)))?;

    let question = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?;

    let detail = build_detail(&state.pool, &auth_user, question, None)
        .await
        .map_err(|e| db_err(format!("构建详情失败: {}", e)))?;

    Ok(Json(detail))
}

/// DELETE /api/v1/questions/:id
pub async fn delete_question(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let existing = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "题目不存在"}))))?;

    if existing.status != QuestionStatus::Draft && existing.status != QuestionStatus::Published {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "只能删除草稿或已发布状态的题目"})),
        ));
    }

    let space = get_space(&state.pool, existing.space_id)
        .await
        .map_err(|e| db_err(format!("查询空间失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    // ── 删除权限：按空间类型分流 ──
    // 个人空间：创建者或管理员可删除
    // 团队/公共空间：仅超级管理员或空间 Owner 可删除
    let can_delete = match space.kind {
        SpaceKind::Personal => existing.creator_id == auth_user.id || is_admin_user(&auth_user),
        SpaceKind::Team | SpaceKind::Public => {
            is_admin_user(&auth_user) || space.owner_user_id == Some(auth_user.id)
        }
    };

    if !can_delete {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权删除该题目，仅空间 Owner 或管理员可删除"}))));
    }

    sqlx::query("DELETE FROM questions WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| db_err(format!("删除题目失败: {}", e)))?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// 教研状态机（Draft → Pending → Published / Rejected → Draft）
// ---------------------------------------------------------------------------

/// POST /api/v1/questions/:id/submit — 提交审核（draft → pending）
///
/// 状态校验：仅 draft 可提交（rejected 不可直接重提，需先回到 draft）
/// 权限：题目创建者，或空间 owner/editor/reviewer
pub async fn submit_for_review(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<SubmitReviewRequest>,
) -> Result<Json<QuestionDetail>, (StatusCode, Json<serde_json::Value>)> {
    let existing = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "题目不存在"}))))?;

    // ── 状态校验：仅 draft 可提交 ──
    if existing.status != QuestionStatus::Draft {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": format!("状态流转违规：当前状态 {:?} 不可提交审核，仅 draft 可提交", existing.status),
                "code": "ERR_INVALID_STATE_TRANSITION",
                "current_status": format!("{:?}", existing.status),
                "expected_status": "draft"
            })),
        ));
    }

    // ── 权限校验：creator / admin / 空间 owner/member ──
    let space = get_space(&state.pool, existing.space_id)
        .await
        .map_err(|e| db_err(format!("查询空间失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    let can_submit = if existing.creator_id == auth_user.id || is_admin_user(&auth_user) {
        true
    } else {
        match get_member_meta(&state.pool, space.id, auth_user.id)
            .await
            .map_err(|e| db_err(format!("查询成员角色失败: {}", e)))?
        {
            Some((role, _)) => matches!(role.as_str(), "owner" | "member"),
            None => false,
        }
    };

    if !can_submit {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "无权提交审核：仅题目创建者或空间 owner/member 可提交",
                "code": "ERR_FORBIDDEN"
            })),
        ));
    }

    // ── 审题人校验：按空间类型分流 ──
    let reviewer_id = match space.kind {
        SpaceKind::Personal => {
            // 个人空间：自审自发，reviewer_id = creator_id
            existing.creator_id
        }
        SpaceKind::Team => {
            // 团队空间：强制交叉审核，reviewer_id 必须由前端传入且 != creator_id
            let rid = req.reviewer_id.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "团队空间提交审核必须指定审题人"})),
                )
            })?;
            if rid == existing.creator_id {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(json!({"error": "团队空间不允许自审，请选择其他成员作为审题人"})),
                ));
            }
            // 校验审题人是否为该空间成员且有权审题（owner/member）
            let reviewer_role = get_member_meta(&state.pool, space.id, rid)
                .await
                .map_err(|e| db_err(format!("查询审题人角色失败: {}", e)))?;
            match reviewer_role {
                Some((role, _)) if matches!(role.as_str(), "owner" | "member") => rid,
                _ => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(json!({"error": "指定的审题人不存在或无审核权限"})),
                    ));
                }
            }
        }
        SpaceKind::Public => {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({"error": "公共空间题目无需提交审核"})),
            ));
        }
    };

    // ── 事务内：FOR UPDATE 锁定 + 状态流转 + 审题人写入（GAP-4 修复） ──
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| db_err(format!("开启事务失败: {}", e)))?;

    // FOR UPDATE 锁定 — 防止并发提交竞态
    let locked = sqlx::query_as::<_, Question>(
        "SELECT * FROM questions WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| db_err(format!("锁定题目失败: {}", e)))?;

    // 二次校验状态（防止并发窗口内状态已变）
    if locked.status != QuestionStatus::Draft {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "题目状态已被并发修改，请刷新后重试",
                "code": "ERR_CONCURRENT_STATE_CHANGE"
            })),
        ));
    }

    // 状态流转：draft → pending
    sqlx::query(
        "UPDATE questions SET status = 'pending'::question_status, updated_at = $1 WHERE id = $2",
    )
    .bind(chrono::Utc::now())
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err(format!("提交审核失败: {}", e)))?;

    // 写入审题人记录（覆盖旧记录，同事务内）
    sqlx::query("DELETE FROM question_reviewers WHERE question_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| db_err(format!("清理旧审题人失败: {}", e)))?;

    sqlx::query("INSERT INTO question_reviewers (question_id, user_id) VALUES ($1, $2)")
        .bind(id)
        .bind(reviewer_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| db_err(format!("写入审题人失败: {}", e)))?;

    tx.commit()
        .await
        .map_err(|e| db_err(format!("提交事务失败: {}", e)))?;

    // ── 通知审题人 ──
    if reviewer_id != existing.creator_id {
        if let Err(e) = crate::handlers::notifications::send_notification(
            &state.pool,
            &state.notify_tx,
            crate::models::notification::CreateNotification {
                user_id: reviewer_id,
                kind: "workflow".into(),
                title: "新题目待审核".into(),
                body: Some(format!(
                    "您所在的团队空间「{}」有新题目等待您的审核",
                    space.name
                )),
                resource_type: Some("question".into()),
                resource_id: Some(id),
            },
        )
        .await
        {
            tracing::warn!("发送提交通知失败, reviewer_id={}, err={}", reviewer_id, e);
        }
    }

    let question = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?;

    let detail = build_detail(&state.pool, &auth_user, question, None)
        .await
        .map_err(|e| db_err(format!("构建详情失败: {}", e)))?;

    Ok(Json(detail))
}

/// POST /api/v1/questions/:id/approve — 审核通过（pending → published）
///
/// 前置拦截：必须通过 can_publish_question（Maker-Checker 引擎）
/// 事务一致性：状态更新 + version+1 + save_version 在同一 tx 内
pub async fn approve_question(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<QuestionDetail>, (StatusCode, Json<serde_json::Value>)> {
    // ── 1. 事务外加载所有数据 ──
    let existing = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "题目不存在"}))))?;

    let space = get_space(&state.pool, existing.space_id)
        .await
        .map_err(|e| db_err(format!("查询空间失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    // can_publish_question 需要 GlobalRole，必须加载完整 User 行
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(auth_user.id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询用户失败: {}", e)))?
        .ok_or_else(|| (StatusCode::FORBIDDEN, Json(json!({"error": "用户不存在"}))))?;

    // ── 2. 状态校验：仅 pending 可审核通过 ──
    if existing.status != QuestionStatus::Pending {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": format!("状态流转违规：当前状态 {:?} 不可审核通过，仅 pending 可审核", existing.status),
                "code": "ERR_INVALID_STATE_TRANSITION",
                "current_status": format!("{:?}", existing.status),
                "expected_status": "pending"
            })),
        ));
    }

    // ── 3. Maker-Checker 鉴权（can_publish_question 内部处理所有法则） ──
    can_publish_question(&state.pool, &user, &existing, &space)
        .await
        .map_err(|e| match e {
            PermissionError::MakerCheckerViolation => (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "录审分离违规：创建者不能审核自己录入的题目",
                    "code": "ERR_MAKER_CHECKER_VIOLATION"
                })),
            ),
            PermissionError::MissingPrivilege(msg) => (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": format!("无权审核：{}", msg),
                    "code": "ERR_FORBIDDEN"
                })),
            ),
            PermissionError::Database(e) => db_err(format!("权限检查失败: {}", e)),
        })?;

    // ── 3b. 指定审核人校验（GAP-1 修复）──
    // 如果题目已指定审核人，仅指定审核人可审核通过（SuperAdmin 豁免）
    if user.global_role != GlobalRole::SuperAdmin {
        let designated = list_reviewers(&state.pool, id)
            .await
            .map_err(|e| db_err(format!("查询审题人失败: {}", e)))?;
        if !designated.is_empty() && !designated.contains(&auth_user.id) {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "无权审核：您不是该题目的指定审核人",
                    "code": "ERR_NOT_DESIGNATED_REVIEWER"
                })),
            ));
        }
    }

    // ── 4. 事务内：FOR UPDATE 锁定 + 状态更新 + version+1 + save_version ──
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| db_err(format!("开启事务失败: {}", e)))?;

    let locked = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| db_err(format!("锁定题目失败: {}", e)))?;

    // 二次校验：防止并发审核导致状态已变
    if locked.status != QuestionStatus::Pending {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "题目状态已被并发修改，请刷新后重试",
                "code": "ERR_CONCURRENT_STATE_CHANGE"
            })),
        ));
    }

    let new_version = locked.version + 1;
    let now = chrono::Utc::now();

    sqlx::query(
        r#"
        UPDATE questions
        SET status = 'published'::question_status,
            version = $1,
            updated_at = $2,
            updated_by = $3
        WHERE id = $4
        "#,
    )
    .bind(new_version)
    .bind(now)
    .bind(auth_user.id)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err(format!("更新题目状态失败: {}", e)))?;

    // 写入新版本快照（同事务，保证原子性）
    save_version(&mut tx, id, new_version, Some(auth_user.id))
        .await
        .map_err(|e| db_err(format!("保存版本快照失败: {}", e)))?;

    tx.commit()
        .await
        .map_err(|e| db_err(format!("提交事务失败: {}", e)))?;

    // ── 通知题目创建者：审核通过 ──
    // Maker-Checker 已确保团队空间中审核者 ≠ 创建者；
    // 个人空间允许自审，此时不通知自己
    if existing.creator_id != auth_user.id {
        if let Err(e) = crate::handlers::notifications::send_notification(
            &state.pool,
            &state.notify_tx,
            crate::models::notification::CreateNotification {
                user_id: existing.creator_id,
                kind: "workflow".into(),
                title: "题目审核通过".into(),
                body: Some("您提交的题目已通过审核，正式发布".into()),
                resource_type: Some("question".into()),
                resource_id: Some(id),
            },
        )
        .await
        {
            tracing::warn!(
                "发送审核通过通知失败, creator_id={}, err={}",
                existing.creator_id,
                e
            );
        }
    }

    // ── 5. 返回详情 ──
    let question = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?;

    let detail = build_detail(&state.pool, &auth_user, question, None)
        .await
        .map_err(|e| db_err(format!("构建详情失败: {}", e)))?;

    Ok(Json(detail))
}

/// POST /api/v1/questions/:id/reject — 打回重做（pending → draft）
///
/// 状态校验：仅 pending 可驳回
/// 权限：团队空间 owner/reviewer（允许自审退回，不触发 Maker-Checker）
///       个人空间 creator 自审退回；SuperAdmin 一票通过
pub async fn reject_question(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<RejectRequest>,
) -> Result<Json<QuestionDetail>, (StatusCode, Json<serde_json::Value>)> {
    let existing = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "题目不存在"}))))?;

    let space = get_space(&state.pool, existing.space_id)
        .await
        .map_err(|e| db_err(format!("查询空间失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    // ── 状态校验：仅 pending 可驳回 ──
    if existing.status != QuestionStatus::Pending {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": format!("状态流转违规：当前状态 {:?} 不可驳回，仅 pending 可驳回", existing.status),
                "code": "ERR_INVALID_STATE_TRANSITION",
                "current_status": format!("{:?}", existing.status),
                "expected_status": "pending"
            })),
        ));
    }

    // ── 权限校验：reject 允许自审退回（与 approve 的 Maker-Checker 不同） ──
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(auth_user.id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询用户失败: {}", e)))?
        .ok_or_else(|| (StatusCode::FORBIDDEN, Json(json!({"error": "用户不存在"}))))?;

    let can_reject = if user.global_role == GlobalRole::SuperAdmin {
        true
    } else {
        match space.kind {
            SpaceKind::Personal => {
                // 个人空间：creator 自审退回
                existing.creator_id == auth_user.id
            }
            SpaceKind::Team => {
                // 团队空间（GAP-2 修复）：
                // 1. creator 可以撤回自己的提交（不受指定审核人约束）
                // 2. 非 creator 需要 owner/member 角色 + 是指定审核人（如果有指定）
                if existing.creator_id == auth_user.id {
                    true
                } else {
                    let has_role = match get_member_meta(&state.pool, space.id, auth_user.id)
                        .await
                        .map_err(|e| db_err(format!("查询成员角色失败: {}", e)))?
                    {
                        Some((role, _)) => matches!(role.as_str(), "owner" | "member"),
                        None => false,
                    };
                    if !has_role {
                        false
                    } else {
                        // 指定审核人校验：有指定审核人时，仅指定人可驳回
                        let designated = list_reviewers(&state.pool, id)
                            .await
                            .map_err(|e| db_err(format!("查询审题人失败: {}", e)))?;
                        designated.is_empty() || designated.contains(&auth_user.id)
                    }
                }
            }
            SpaceKind::Public => false,
        }
    };

    if !can_reject {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "无权驳回：仅空间 owner/reviewer 可驳回（个人空间允许自审退回）",
                "code": "ERR_FORBIDDEN"
            })),
        ));
    }

    // ── 记录驳回原因到日志（可选字段，便于审计追踪） ──
    if let Some(ref reason) = req.reject_reason {
        tracing::info!(
            question_id = %id,
            reviewer_id = %auth_user.id,
            reject_reason = %reason,
            "题目被驳回"
        );
    }

    // ── 状态流转：pending → draft ──
    sqlx::query(
        r#"
        UPDATE questions
        SET status = 'draft'::question_status,
            updated_at = $1,
            updated_by = $2
        WHERE id = $3
        "#,
    )
    .bind(chrono::Utc::now())
    .bind(auth_user.id)
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(|e| db_err(format!("驳回题目失败: {}", e)))?;

    // ── 通知题目创建者：题目被驳回 ──
    // 个人空间允许自审退回，此时不通知自己
    // resource_type 使用 question_edit，前端点击后直接跳转编辑页
    if existing.creator_id != auth_user.id {
        if let Err(e) = crate::handlers::notifications::send_notification(
            &state.pool,
            &state.notify_tx,
            crate::models::notification::CreateNotification {
                user_id: existing.creator_id,
                kind: "workflow".into(),
                title: "题目被驳回".into(),
                body: req
                    .reject_reason
                    .as_ref()
                    .map(|r| format!("驳回理由：{}", r)),
                resource_type: Some("question_edit".into()),
                resource_id: Some(id),
            },
        )
        .await
        {
            tracing::warn!(
                "发送驳回通知失败, creator_id={}, err={}",
                existing.creator_id,
                e
            );
        }
    }

    let question = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?;

    let detail = build_detail(&state.pool, &auth_user, question, None)
        .await
        .map_err(|e| db_err(format!("构建详情失败: {}", e)))?;

    Ok(Json(detail))
}

// ---------------------------------------------------------------------------
// 公共库双向流通（复制）
// ---------------------------------------------------------------------------

/// POST /api/v1/questions/:id/contribute — 贡献到公共库
pub async fn contribute_to_public(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<QuestionDetail>), (StatusCode, Json<serde_json::Value>)> {
    let src = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "题目不存在"}))))?;

    if src.status != QuestionStatus::Published {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "仅已发布题目可贡献到公共库"})),
        ));
    }

    let space = get_space(&state.pool, src.space_id)
        .await
        .map_err(|e| db_err(format!("查询空间失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    if !can_access_space(&state.pool, &auth, &space)
        .await
        .map_err(|e| db_err(format!("权限检查失败: {}", e)))?
    {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权操作该题目"}))));
    }

    let public_id = ensure_public_space(&state.pool)
        .await
        .map_err(|e| db_err(format!("初始化公共空间失败: {}", e)))?;

    let new_id = copy_question(&state.pool, &src, public_id, auth.id, Some(src.id))
        .await
        .map_err(|e| db_err(format!("复制到公共库失败: {}", e)))?;

    let question = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(new_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?;

    let detail = build_detail(&state.pool, &auth, question, None)
        .await
        .map_err(|e| db_err(format!("构建详情失败: {}", e)))?;

    Ok((StatusCode::CREATED, Json(detail)))
}

/// POST /api/v1/questions/:id/import — 从公共库（或任意已发布可见题）导入到目标空间
pub async fn import_question(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<TransferQuestionRequest>,
) -> Result<(StatusCode, Json<QuestionDetail>), (StatusCode, Json<serde_json::Value>)> {
    let src = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "题目不存在"}))))?;

    if src.status != QuestionStatus::Published {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "仅已发布题目可导入"})),
        ));
    }

    let src_space = get_space(&state.pool, src.space_id)
        .await
        .map_err(|e| db_err(format!("查询空间失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    // 来源：公共库 或 有访问权限的已发布题
    if src_space.kind != SpaceKind::Public
        && !can_access_space(&state.pool, &auth, &src_space)
            .await
            .map_err(|e| db_err(format!("权限检查失败: {}", e)))?
    {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权导入该题目"}))));
    }

    let target_space_id = if let Some(tid) = req.target_space_id {
        tid
    } else {
        let display = sqlx::query_scalar::<_, String>(
            "SELECT display_name FROM users WHERE id = $1",
        )
        .bind(auth.id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询用户失败: {}", e)))?
        .unwrap_or_else(|| "用户".into());
        ensure_personal_space(&state.pool, auth.id, &display)
            .await
            .map_err(|e| db_err(format!("创建个人空间失败: {}", e)))?
    };

    let target = get_space(&state.pool, target_space_id)
        .await
        .map_err(|e| db_err(format!("查询空间失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "目标空间不存在"}))))?;

    if !can_write_in_space(&state.pool, &auth, &target)
        .await
        .map_err(|e| db_err(format!("权限检查失败: {}", e)))?
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "无权写入目标空间"})),
        ));
    }

    let new_id = copy_question(&state.pool, &src, target_space_id, auth.id, Some(src.id))
        .await
        .map_err(|e| db_err(format!("导入失败: {}", e)))?;

    let question = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(new_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?;

    let detail = build_detail(&state.pool, &auth, question, None)
        .await
        .map_err(|e| db_err(format!("构建详情失败: {}", e)))?;

    Ok((StatusCode::CREATED, Json(detail)))
}

async fn copy_question(
    pool: &sqlx::PgPool,
    src: &Question,
    target_space_id: Uuid,
    creator_id: Uuid,
    origin_id: Option<Uuid>,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    let now = chrono::Utc::now();

    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        INSERT INTO questions (
            id, stem, stem_text, images, question_type, difficulty, status,
            options, correct_answer, analysis, metadata,
            parent_id, sub_order,
            creator_id, created_at, updated_at, version, space_id, origin_question_id,
            content_hash, normalized_content_hash
        )
        VALUES (
            $1, $2, $3, $4, $5, $6, 'published'::question_status,
            $7, $8, $9, COALESCE($10, '{}'::jsonb),
            $11, $12,
            $13, $14, $15, 1, $16, $17,
            $18, $19
        )
        "#,
    )
    .bind(id)
    .bind(&src.stem)
    .bind(&src.stem_text)
    .bind(&src.images)
    .bind(&src.question_type)
    .bind(&src.difficulty)
    .bind(&src.options)
    .bind(&src.correct_answer)
    .bind(&src.analysis)
    .bind(&src.metadata)
    .bind(&src.parent_id)
    .bind(src.sub_order)
    .bind(creator_id)
    .bind(now)
    .bind(now)
    .bind(target_space_id)
    .bind(origin_id)
    .bind(&src.content_hash)
    .bind(&src.normalized_content_hash)
    .execute(&mut *tx)
    .await?;

    // 复制知识点节点关联（保留 is_primary 与 source）
    sqlx::query(
        r#"
        INSERT INTO question_knowledge_nodes (question_id, node_id, is_primary, ai_confidence, source, created_at)
        SELECT $1, node_id, is_primary, ai_confidence, source, NOW()
        FROM question_knowledge_nodes
        WHERE question_id = $2
        "#,
    )
    .bind(id)
    .bind(src.id)
    .execute(&mut *tx)
    .await?;

    save_version(&mut tx, id, 1, Some(creator_id)).await?;
    tx.commit().await?;
    Ok(id)
}
