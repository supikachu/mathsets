use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use axum_extra::extract::Query;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use uuid::Uuid;

use crate::ai::tagging::{
    apply_tagging_suggestion, confirmation_or_legacy, insert_confirmed_candidates,
};
use crate::auth::middleware::AuthUser;
use crate::auth::permissions::{
    can_access_space, can_edit_question, can_publish_question, can_review_question,
    can_write_in_space, ensure_personal_space, ensure_public_space, get_member_meta, get_space,
    is_admin_user, list_reviewers, PermissionError,
};
use crate::models::question::{
    AiCreateMeta, CreateQuestionRequest, KnowledgeNodeSummary, Question, QuestionDetail,
    QuestionQuery, QuestionStatus, QuestionSummary, QuestionType, RejectRequest,
    SubmitReviewRequest, TagSummary, TransferQuestionRequest, UpdateQuestionRequest,
    is_answer_empty, refresh_system_flags,
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

    // 4. 插入新增关联（统一标记为 manual；AI 来源由 TaggingFinalizer 在保存后回写）
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

/// 提交审核前的完整性校验（T2-1 ~ T2-4）
///
/// 三道校验门，任一失败返回 422 + 结构化错误：
/// 1. 答案校验：`is_answer_empty` 覆盖 None/Null/空数组/纯空格
/// 2. 选择题选项校验：question_type ∈ {Choice, Multiple} 时 options.options 非空
/// 3. 解析校验：analysis 为空且 `no_analysis_needed != true` 时拒绝
///
/// 返回的 `missing` 数组用于前端逐项高亮未补全字段。
fn validate_question_completeness(
    question: &Question,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    // 1. 答案校验
    if is_answer_empty(&question.correct_answer) {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "error": "题目尚未补全答案，无法提交审核",
                "code": "ERR_ANSWER_INCOMPLETE",
                "missing": ["correct_answer"]
            })),
        ));
    }

    // 2. 选择题选项校验
    // options 的规范存储格式是数组 [{label, content}]（创建接口原样落库、
    // worker 序列化 Vec<ParsedOption>、前端 API 类型一致）；
    // 防御性兼容历史对象包裹格式 {"options": [...]}
    if matches!(question.question_type, QuestionType::Choice | QuestionType::Multiple) {
        let options_empty = question.options.as_ref().map_or(true, |o| match o {
            serde_json::Value::Array(a) => a.is_empty(),
            serde_json::Value::Object(_) => o
                .get("options")
                .and_then(|arr| arr.as_array())
                .map_or(true, |a| a.is_empty()),
            _ => true,
        });
        if options_empty {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({
                    "error": "选择题尚未填写选项内容",
                    "code": "ERR_OPTIONS_INCOMPLETE",
                    "missing": ["options"]
                })),
            ));
        }
    }

    // 3. 解析校验（no_analysis_needed=true 时豁免）
    let no_analysis_needed = question
        .metadata
        .get("system_flags")
        .and_then(|f| f.get("no_analysis_needed"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let analysis_empty = question
        .analysis
        .as_ref()
        .map_or(true, |s| s.trim().is_empty());
    if analysis_empty && !no_analysis_needed {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "error": "题目尚未补全解析，无法提交审核",
                "code": "ERR_ANALYSIS_INCOMPLETE",
                "missing": ["analysis"]
            })),
        ));
    }

    Ok(())
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
    // 待补全筛选（T2-6）— 必须用 `@>` 包含操作符命中 GIN 索引
    // `->>` 会将 JSONB 转为 text，绕过 GIN 索引退化为 Seq Scan
    if let Some(ref flag) = query.system_flag {
        match flag.as_str() {
            "pending_answer" => {
                builder.push(" AND q.metadata->'system_flags' @> '{\"pending_answer\": true}'::jsonb");
            }
            "missing_analysis" => {
                builder.push(" AND q.metadata->'system_flags' @> '{\"missing_analysis\": true}'::jsonb");
            }
            "incomplete" => {
                // pending_answer OR missing_analysis 并集
                // 与 incomplete_count 接口的 total 逻辑保持一致
                builder.push(" AND (q.metadata->'system_flags' @> '{\"pending_answer\": true}'::jsonb");
                builder.push(" OR q.metadata->'system_flags' @> '{\"missing_analysis\": true}'::jsonb)");
            }
            _ => {} // 未知 flag 忽略，避免 SQL 注入
        }
    }
}

/// 加载并校验 AI 智能录入的暂存项（确认保存时使用）
///
/// - 校验任务归属（本人或管理员）
/// - 按 `staged_index` 从 `progress.staged_questions` 定位暂存项
/// - 已保存的暂存项拒绝重复提交
async fn load_ai_staged_item(
    pool: &sqlx::PgPool,
    meta: &AiCreateMeta,
    auth: &AuthUser,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let task_row: Option<(Uuid, serde_json::Value)> = sqlx::query_as(
        "SELECT creator_id, progress FROM ai_parse_tasks WHERE id = $1",
    )
    .bind(meta.task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err(format!("查询解析任务失败: {e}")))?;

    let (creator_id, progress) = task_row
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "解析任务不存在"}))))?;
    if creator_id != auth.id && !is_admin_user(auth) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "解析任务不存在"})),
        ));
    }

    let staged = progress
        .get("staged_questions")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter().find(|item| {
                item.get("index").and_then(|i| i.as_str()) == Some(meta.staged_index.as_str())
            })
        })
        .cloned()
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "暂存题目不存在，可能已被保存或丢弃"})),
            )
        })?;

    if staged.get("saved").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "该题目已保存，请勿重复提交"})),
        ));
    }

    Ok(staged)
}

/// 标记 AI 暂存项已保存：写 saved/saved_question_id + 幂等映射（index → 题目 ID）
async fn mark_ai_staged_saved(
    pool: &sqlx::PgPool,
    task_id: Uuid,
    staged_index: &str,
    question_id: Uuid,
) {
    // 1. 暂存项标记 saved + saved_question_id（供前端防重复提交）
    let _ = sqlx::query(
        r#"
        UPDATE ai_parse_tasks
        SET progress = jsonb_set(
              progress,
              '{staged_questions}',
              (
                SELECT COALESCE(jsonb_agg(
                  CASE WHEN elem->>'index' = $3
                       THEN elem || jsonb_build_object('saved', true, 'saved_question_id', $2::text)
                       ELSE elem END
                  ORDER BY ord
                ), '[]'::jsonb)
                FROM jsonb_array_elements(progress->'staged_questions')
                  WITH ORDINALITY AS t(elem, ord)
              )
            ),
            updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(task_id)
    .bind(question_id.to_string())
    .bind(staged_index)
    .execute(pool)
    .await;

    // 2. 幂等映射（供 GET parse-task 返回 question_ids；值为 uuid 字符串）
    let _ = sqlx::query(
        r#"
        UPDATE ai_parse_tasks
        SET progress = jsonb_set(
              progress,
              '{idempotency_map}',
              COALESCE(progress->'idempotency_map', '{}'::jsonb) || jsonb_build_object($3::text, $2::text)
            ),
            updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(task_id)
    .bind(question_id.to_string())
    .bind(staged_index)
    .execute(pool)
    .await;
}

fn merge_primary(mut node_ids: Vec<Uuid>, primary: Option<Uuid>) -> Vec<Uuid> {
    if let Some(p) = primary {
        if !node_ids.contains(&p) {
            node_ids.push(p);
        }
    }
    node_ids
}

/// 旧录题路径：把暂存 matched 节点标为 AI 来源（无 suggestion 时使用）
async fn apply_legacy_staged_matches(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    question_id: Uuid,
    staged: &serde_json::Value,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let Some(matched) = staged.get("matched").and_then(|m| m.as_array()) else {
        return Ok(());
    };
    use rust_decimal::prelude::FromPrimitive;
    for m in matched {
        let Some(node_id) = m
            .get("node_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
        else {
            continue;
        };
        let score = m.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let ai_confidence = rust_decimal::Decimal::from_f32(score);
        sqlx::query(
            r#"
            INSERT INTO question_knowledge_nodes
              (question_id, node_id, is_primary, source, ai_confidence, created_at)
            VALUES ($1, $2, FALSE, 'ai', $3, NOW())
            ON CONFLICT (question_id, node_id) DO UPDATE SET
              source = 'ai',
              ai_confidence = EXCLUDED.ai_confidence
            "#,
        )
        .bind(question_id)
        .bind(node_id)
        .bind(ai_confidence)
        .execute(&mut **tx)
        .await
        .map_err(|e| db_err(format!("写入 AI 知识树标签失败: {e}")))?;
    }
    Ok(())
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

    // ── AI 智能录入：加载暂存项（校验归属 + 未保存），容器关联/候选/标记据此完成 ──
    let ai_staged: Option<serde_json::Value> = match &req.ai_meta {
        Some(meta) => Some(load_ai_staged_item(&state.pool, meta, &auth_user).await?),
        None => None,
    };

    let id = Uuid::new_v4();
    let now = chrono::Utc::now();
    let creator_id = auth_user.id;
    let version = 1;

    let mut tx = state.pool.begin().await.map_err(|e| db_err(format!("开启事务失败: {}", e)))?;

    // 注：配额已统一由 ai_usage_log 计量（任务创建时原子抢占，见 ai_tasks.rs），
    // 题目落库本身不再单独扣减配额。

    // ── V2.1.1 去重 hash（创建接口即时计算，计划书 §八） ──
    // 空答案统一按 JSON null 参与 hash（与下方 INSERT 写入值一致）
    let answer_for_hash = req.correct_answer.as_ref().unwrap_or(&serde_json::Value::Null);
    let content_hash = crate::util::normalize::compute_content_hash(
        &req.stem,
        req.options.as_ref(),
        answer_for_hash,
        req.analysis.as_deref(),
    );
    let normalized_content_hash = crate::util::normalize::compute_normalized_content_hash(
        &req.stem,
        req.options.as_ref(),
        answer_for_hash,
    );

    // ── 异步补全：刷新 metadata.system_flags（pending_answer / missing_analysis） ──
    let mut metadata = req.metadata.clone().unwrap_or_else(|| serde_json::json!({}));
    refresh_system_flags(&mut metadata, &req.correct_answer, &req.analysis);

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
    // 空答案统一写入 JSON null（非 SQL NULL，不违反 NOT NULL 约束）
    .bind(req.correct_answer.as_ref().unwrap_or(&serde_json::Value::Null))
    .bind(&req.analysis)
    .bind(&metadata)
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

    let confirmation = confirmation_or_legacy(
        req.ai_tagging_confirmation.clone(),
        ai_staged.as_ref(),
    );
    let mut pending_candidates = Vec::new();
    if let Some(ref conf) = confirmation {
        let node_ids = merge_primary(
            req.knowledge_node_ids.clone().unwrap_or_default(),
            req.primary_knowledge_node_id,
        );
        pending_candidates = apply_tagging_suggestion(
            &mut tx,
            auth_user.id,
            id,
            conf,
            &node_ids,
            &all_tag_ids,
        )
        .await?;
    } else if let Some(ref staged) = ai_staged {
        apply_legacy_staged_matches(&mut tx, id, staged).await?;
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

    insert_confirmed_candidates(&state.pool, id, &pending_candidates).await;

    // ── AI 智能录入后处理（尽力而为，不阻塞题目已成功创建） ──
    if let (Some(meta), Some(staged)) = (&req.ai_meta, &ai_staged) {
        let parsed: Option<crate::ai::types::ParsedQuestion> = staged
            .get("parsed")
            .and_then(|p| serde_json::from_value(p.clone()).ok());

        let paper_id = staged
            .get("paper_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok());
        let collection_id = staged
            .get("collection_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok());
        let is_mixed = staged.get("is_mixed").and_then(|v| v.as_bool()).unwrap_or(false);

        // 容器关联（试卷/集合），题号/顺序取自暂存项的解析结果
        if let Some(ref p) = parsed {
            crate::workers::ai_parse_worker::link_to_container(
                &state,
                id,
                paper_id,
                collection_id,
                is_mixed,
                p,
            )
            .await;
        }

        // 标记暂存项已保存 + 写幂等映射（供 GET parse-task 返回 question_ids）
        mark_ai_staged_saved(&state.pool, meta.task_id, &meta.staged_index, id).await;
    }

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

    // 状态机保护：
    //   - Draft / Rejected → 直接编辑保存
    //   - Published → 允许纠错：保存后状态降级为 Pending，重新进入审核流程
    //   - Pending → 禁止编辑（正在审核中）
    if existing.status == QuestionStatus::Pending {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "题目正在审核中，无法编辑"})),
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

    // ── 图片差集计算：找出被用户删除/替换的旧图片 ──
    // 正则匹配 /uploads/questions/xxx.png 格式的文件名
    let re = Regex::new(r"/uploads/questions/([a-zA-Z0-9_\-\.]+\.(?:png|jpg|jpeg|gif|webp))")
        .map_err(|e| db_err(format!("正则编译失败: {}", e)))?;

    // 提取旧文本中的图片集合
    let mut old_images: HashSet<String> = HashSet::new();
    for cap in re.captures_iter(&existing.stem) {
        if let Some(f) = cap.get(1) { old_images.insert(f.as_str().to_string()); }
    }
    if let Some(ref analysis) = existing.analysis {
        for cap in re.captures_iter(analysis) {
            if let Some(f) = cap.get(1) { old_images.insert(f.as_str().to_string()); }
        }
    }
    if let Some(ref options) = existing.options {
        for cap in re.captures_iter(&options.to_string()) {
            if let Some(f) = cap.get(1) { old_images.insert(f.as_str().to_string()); }
        }
    }

    // 提取新文本中的图片集合（COALESCE 语义：未提供的字段保留旧值）
    let new_stem = req.stem.as_deref().unwrap_or(&existing.stem);
    let mut new_images: HashSet<String> = HashSet::new();
    for cap in re.captures_iter(new_stem) {
        if let Some(f) = cap.get(1) { new_images.insert(f.as_str().to_string()); }
    }
    let new_analysis = req.analysis.as_deref().or(existing.analysis.as_deref());
    if let Some(analysis) = new_analysis {
        for cap in re.captures_iter(analysis) {
            if let Some(f) = cap.get(1) { new_images.insert(f.as_str().to_string()); }
        }
    }
    let new_options = req.options.as_ref().or(existing.options.as_ref());
    if let Some(options) = new_options {
        for cap in re.captures_iter(&options.to_string()) {
            if let Some(f) = cap.get(1) { new_images.insert(f.as_str().to_string()); }
        }
    }

    // 差集：存在于旧文本中但已不存在于新文本中的图片 = 被遗弃的旧图片
    let orphaned_images: Vec<String> = old_images.difference(&new_images).cloned().collect();

    let mut tx = state.pool.begin().await.map_err(|e| db_err(format!("开启事务失败: {}", e)))?;

    // 纠错降级：Published 题目修改后状态转为 Pending 重新审核；Draft/Rejected 保持 Draft
    let new_status = if existing.status == QuestionStatus::Published {
        QuestionStatus::Pending
    } else {
        QuestionStatus::Draft
    };

    // ── 异步补全：基于「更新后」的答案/解析刷新 system_flags ──
    // COALESCE 语义：未提供的字段保留旧值，故基于 req.X.or(existing.X) 计算最终值
    let mut effective_metadata = req
        .metadata
        .clone()
        .unwrap_or_else(|| existing.metadata.clone());
    let effective_answer = req
        .correct_answer
        .clone()
        .or_else(|| existing.correct_answer.clone());
    let effective_analysis = req.analysis.clone().or(existing.analysis.clone());
    refresh_system_flags(
        &mut effective_metadata,
        &effective_answer,
        &effective_analysis,
    );

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
            status = $16::question_status,
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
    .bind(&effective_metadata)
    .bind(&req.images)
    .bind(&req.parent_id)
    .bind(req.sub_order)
    .bind(auth_user.id)
    .bind(now)
    .bind(new_version)
    .bind(id)
    .bind(old_version)
    .bind(new_status)
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

    let mut pending_candidates = Vec::new();
    if let Some(ref conf) = req.ai_tagging_confirmation {
        let node_ids = if let Some(ref ids) = req.knowledge_node_ids {
            merge_primary(ids.clone(), req.primary_knowledge_node_id)
        } else {
            sqlx::query_scalar::<_, Uuid>(
                "SELECT node_id FROM question_knowledge_nodes WHERE question_id = $1",
            )
            .bind(id)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| db_err(format!("查询知识点关联失败: {e}")))?
        };
        let tag_ids = if tag_ids_provided || new_tags_provided {
            all_tag_ids.clone()
        } else {
            sqlx::query_scalar::<_, Uuid>(
                "SELECT tag_id FROM question_tags_relation WHERE question_id = $1",
            )
            .bind(id)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| db_err(format!("查询标签关联失败: {e}")))?
        };
        pending_candidates = apply_tagging_suggestion(
            &mut tx,
            auth_user.id,
            id,
            conf,
            &node_ids,
            &tag_ids,
        )
        .await?;
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

    insert_confirmed_candidates(&state.pool, id, &pending_candidates).await;

    // ── 删后：异步清理被遗弃的旧图片文件 ──
    //    DB 更新已成功提交，物理文件删除失败不应阻断 API 响应
    if !orphaned_images.is_empty() {
        cleanup_question_images(&state.upload_dir, &orphaned_images);
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
    // 团队/公共空间：
    //   - Draft（未提交草稿）：创建者可丢弃自己的（AI 录题工作台"丢弃未确认题目"依赖，
    //     未提交草稿无协作价值）；管理员/Owner 亦可
    //   - 已发布题目：仅超级管理员或空间 Owner 可删除（保持协作保护）
    let can_delete = match space.kind {
        SpaceKind::Personal => existing.creator_id == auth_user.id || is_admin_user(&auth_user),
        SpaceKind::Team | SpaceKind::Public => {
            (existing.status == QuestionStatus::Draft && existing.creator_id == auth_user.id)
                || is_admin_user(&auth_user)
                || space.owner_user_id == Some(auth_user.id)
        }
    };

    if !can_delete {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权删除该题目，仅空间 Owner 或管理员可删除"}))));
    }

    // ── 删前：提取所有图片文件名（含子题目） ──
    //    必须在 DELETE 之前查询，否则数据已被删除无法获取
    let image_filenames = extract_image_filenames(&state.pool, id).await?;

    // 先删子题目（复合题结构），再删主题目
    sqlx::query("DELETE FROM questions WHERE parent_id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| db_err(format!("删除子题目失败: {}", e)))?;

    sqlx::query("DELETE FROM questions WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| db_err(format!("删除题目失败: {}", e)))?;

    // ── 删后：异步清理物理图片文件（孤儿文件 GC） ──
    //    DB 删除已成功提交，物理文件删除失败不应阻断 API 响应
    cleanup_question_images(&state.upload_dir, &image_filenames);

    Ok(StatusCode::NO_CONTENT)
}

/// 提取题目文本中所有引用 `/uploads/questions/` 的图片文件名。
///
/// 扫描字段：stem / analysis / options(JSON 序列化后扫描)
/// 兼容复合题：同时扫描 parent_id = question_id 的所有子题目
fn extract_image_filenames(
    pool: &sqlx::PgPool,
    question_id: Uuid,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<String>, (StatusCode, Json<serde_json::Value>)>> + Send + '_>> {
    Box::pin(async move {
        // 匹配 /uploads/questions/xxx.png 格式的图片文件名
        // 安全：文件名字符集限制 [a-zA-Z0-9_\-\.]，防止路径穿越
        let re = Regex::new(r"/uploads/questions/([a-zA-Z0-9_\-\.]+\.(?:png|jpg|jpeg|gif|webp))")
            .map_err(|e| db_err(format!("正则编译失败: {}", e)))?;

        let mut filenames: Vec<String> = Vec::new();

        // 收集主题目 + 所有子题目
        let rows = sqlx::query_as::<_, Question>(
            "SELECT * FROM questions WHERE id = $1 OR parent_id = $1 ORDER BY sub_order NULLS FIRST",
        )
        .bind(question_id)
        .fetch_all(pool)
        .await
        .map_err(|e| db_err(format!("查询题目及子题目失败: {}", e)))?;

        for q in &rows {
            // stem
            for cap in re.captures_iter(&q.stem) {
                if let Some(f) = cap.get(1) {
                    filenames.push(f.as_str().to_string());
                }
            }
            // analysis
            if let Some(ref analysis) = q.analysis {
                for cap in re.captures_iter(analysis) {
                    if let Some(f) = cap.get(1) {
                        filenames.push(f.as_str().to_string());
                    }
                }
            }
            // options (JSONB → 序列化后扫描)
            if let Some(ref options) = q.options {
                let options_str = options.to_string();
                for cap in re.captures_iter(&options_str) {
                    if let Some(f) = cap.get(1) {
                        filenames.push(f.as_str().to_string());
                    }
                }
            }
        }

        // 去重（同一张图可能被多次引用）
        filenames.sort();
        filenames.dedup();

        Ok(filenames)
    })
}

/// 异步清理题目图片文件 —— spawn 隔离，失败仅 log，不阻断 API 响应。
///
/// 路径拼接：`{upload_dir}/questions/{filename}`
/// 容错：文件不存在 → 静默忽略；其他 IO 错误 → warn 日志
fn cleanup_question_images(upload_dir: &str, filenames: &[String]) {
    if filenames.is_empty() {
        return;
    }
    let dir = std::path::PathBuf::from(upload_dir).join("questions");
    let to_delete: Vec<String> = filenames.to_vec();
    tokio::spawn(async move {
        for filename in &to_delete {
            let file_path = dir.join(filename);
            match tokio::fs::remove_file(&file_path).await {
                Ok(()) => {
                    tracing::info!("已清理题目图片: {}", filename);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // 文件已不存在 — 静默忽略
                }
                Err(e) => {
                    tracing::warn!("清理题目图片失败 {}: {}", filename, e);
                }
            }
        }
    });
}

/// GC：清理 AI 录题孤儿草稿（用户从未确认保存的 worker 落库题目）
///
/// 兜底场景：用户关闭浏览器/崩溃，前端"丢弃"通道未触达，草稿永久残留。
/// 判定（同时满足）：
/// - `metadata->>'source' = 'ai_parse'`（worker 落库时打标）
/// - `status = 'draft'`（从未提交审核）
/// - `version = 1`（用户保存 = update 会递增 version；=1 即从未保存过）
/// - `created_at < 72h 前`（保留恢复窗口，用户可从批量快照恢复）
/// 手动创建的草稿经过显式"保存"动作，不受本 GC 影响。
/// 图片清理复用 delete_question 逻辑，防止物理文件泄漏。
pub async fn gc_abandoned_ai_drafts(pool: &sqlx::PgPool, upload_dir: &str) {
    let ids: Vec<Uuid> = match sqlx::query_scalar(
        r#"
        SELECT id FROM questions
        WHERE status = 'draft'
          AND version = 1
          AND metadata->>'source' = 'ai_parse'
          AND created_at < NOW() - INTERVAL '72 hours'
        LIMIT 500
        "#,
    )
    .fetch_all(pool)
    .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("[gc] AI 孤儿草稿查询失败: {e}");
            return;
        }
    };
    if ids.is_empty() {
        return;
    }

    let mut deleted = 0usize;
    for id in ids {
        let filenames = match extract_image_filenames(pool, id).await {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("[gc] 题目 {id} 图片扫描失败，跳过: {:?}", e.1);
                continue;
            }
        };
        // 与 delete_question 一致：先删子题目再删主题目（parent_id 外键无级联）；
        // question_knowledge_nodes / paper_questions / collection_questions 为 CASCADE
        let delete_result: Result<(), sqlx::Error> = async {
            sqlx::query("DELETE FROM questions WHERE parent_id = $1")
                .bind(id)
                .execute(pool)
                .await?;
            sqlx::query("DELETE FROM questions WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await?;
            Ok(())
        }
        .await;
        match delete_result {
            Ok(()) => {
                cleanup_question_images(upload_dir, &filenames);
                deleted += 1;
            }
            Err(e) => tracing::warn!("[gc] 题目 {id} 删除失败: {e}"),
        }
    }
    tracing::info!("[gc] AI 孤儿草稿清理完成：删除 {deleted} 题");
}

// ---------------------------------------------------------------------------
// 教研状态机（Draft → Pending → Published / Rejected → Draft）
// ---------------------------------------------------------------------------

/// POST /api/v1/questions/:id/submit — 提交审核（draft → pending）
///
/// 状态校验：仅 draft 可提交（rejected 不可直接重提，需先回到 draft）
/// 权限：题目创建者，或空间 owner/editor/reviewer
/// 完整性校验（T2-1~T2-4）：答案 / 选项 / 解析 三道校验门
pub async fn submit_for_review(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<SubmitReviewRequest>,
) -> Result<Json<QuestionDetail>, (StatusCode, Json<serde_json::Value>)> {
    // 预检：NotFound + Pending 幂等短路（保留 creator_name 优化）
    let pre_existing =
        sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| db_err(format!("查询题目失败: {}", e)))?
            .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "题目不存在"}))))?;

    if pre_existing.status == QuestionStatus::Pending {
        let creator_name =
            sqlx::query_scalar::<_, String>("SELECT display_name FROM users WHERE id = $1")
                .bind(pre_existing.creator_id)
                .fetch_optional(&state.pool)
                .await
                .ok()
                .flatten();
        let detail = build_detail(&state.pool, &auth_user, pre_existing, creator_name)
            .await
            .map_err(|e| db_err(format!("构建详情失败: {}", e)))?;
        return Ok(Json(detail));
    }

    let question = submit_question_for_review(&state, &auth_user, id, req.reviewer_id).await?;
    let detail = build_detail(&state.pool, &auth_user, question, None)
        .await
        .map_err(|e| db_err(format!("构建详情失败: {}", e)))?;
    Ok(Json(detail))
}

/// 提交审核核心逻辑（被 `submit_for_review` 与 `batch_submit_questions` 复用）
///
/// 流程：加载题目 → Pending 幂等短路 → Draft 校验 → 完整性校验 → 权限校验
///      → 审题人校验 → 事务内 FOR UPDATE + 状态流转 → 通知审题人 → 重新加载
///
/// 失败时返回结构化错误（含 `code` 字段），便于批量接口逐题记录失败原因。
async fn submit_question_for_review(
    state: &AppState,
    auth: &AuthUser,
    id: Uuid,
    reviewer_id_opt: Option<Uuid>,
) -> Result<Question, (StatusCode, Json<serde_json::Value>)> {
    let existing = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "题目不存在", "code": "ERR_NOT_FOUND"})),
            )
        })?;

    // Pending 视为幂等：已提交过，直接返回当前题目，不重复流转
    if existing.status == QuestionStatus::Pending {
        return Ok(existing);
    }

    // 仅 Draft 可提交审核
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

    // ── 完整性校验（T2-1 ~ T2-4）──
    validate_question_completeness(&existing)?;

    // ── 权限校验：creator / admin / 空间 owner/member ──
    let space = get_space(&state.pool, existing.space_id)
        .await
        .map_err(|e| db_err(format!("查询空间失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    let can_submit = if existing.creator_id == auth.id || is_admin_user(auth) {
        true
    } else {
        match get_member_meta(&state.pool, space.id, auth.id)
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
            let rid = reviewer_id_opt.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "团队空间提交审核必须指定审题人",
                        "code": "ERR_REVIEWER_REQUIRED"
                    })),
                )
            })?;
            if rid == existing.creator_id {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "error": "团队空间不允许自审，请选择其他成员作为审题人",
                        "code": "ERR_SELF_REVIEW_FORBIDDEN"
                    })),
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
                        Json(json!({
                            "error": "指定的审题人不存在或无审核权限",
                            "code": "ERR_INVALID_REVIEWER"
                        })),
                    ));
                }
            }
        }
        SpaceKind::Public => {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "公共空间题目无需提交审核",
                    "code": "ERR_PUBLIC_NOT_SUBMITTABLE"
                })),
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

    Ok(question)
}

/// POST /api/v1/questions/batch-submit — 批量提交审核（T2-5）
///
/// 每题独立调用 `submit_question_for_review`，单题失败不影响其他题。
/// 返回逐题明细：成功 `{"status":"success"}`，失败 `{"status":"failed", code, missing}`。
///
/// 注意：批量接口不支持指定 reviewer_id，因此 Team 空间题目会因
/// `ERR_REVIEWER_REQUIRED` 失败（设计取舍：批量场景多用于个人空间草稿）。
#[derive(Debug, Deserialize)]
pub struct BatchSubmitRequest {
    pub question_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct BatchSubmitResultItem {
    id: Uuid,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    missing: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct BatchSubmitResponse {
    total: usize,
    succeeded: usize,
    failed: usize,
    results: Vec<BatchSubmitResultItem>,
}

pub async fn batch_submit_questions(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<BatchSubmitRequest>,
) -> Result<Json<BatchSubmitResponse>, (StatusCode, Json<serde_json::Value>)> {
    let total = req.question_ids.len();
    let mut results = Vec::with_capacity(total);
    let mut succeeded = 0usize;
    let mut failed = 0usize;

    for id in req.question_ids {
        match submit_question_for_review(&state, &auth_user, id, None).await {
            Ok(_) => {
                results.push(BatchSubmitResultItem {
                    id,
                    status: "success",
                    code: None,
                    missing: None,
                });
                succeeded += 1;
            }
            Err((_, body)) => {
                let value = body.0; // 提取 serde_json::Value
                let code = value
                    .get("code")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let missing = value
                    .get("missing")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    });
                results.push(BatchSubmitResultItem {
                    id,
                    status: "failed",
                    code,
                    missing,
                });
                failed += 1;
            }
        }
    }

    Ok(Json(BatchSubmitResponse {
        total,
        succeeded,
        failed,
        results,
    }))
}

/// GET /api/v1/questions/incomplete-count — 待补全计数（T2-8）
///
/// 返回当前用户可见范围内的待补全统计：
/// - `pending_answer`：system_flags.pending_answer=true 的题目数
/// - `missing_analysis`：system_flags.missing_analysis=true 的题目数
/// - `total`：上述两者的并集（待补全总数）
///
/// SQL 使用 `@>` 包含操作符命中 GIN 索引（避免 `->>` 退化为 Seq Scan）。
#[derive(Debug, Serialize)]
pub struct IncompleteCountResponse {
    pending_answer: i64,
    missing_analysis: i64,
    total: i64,
}

pub async fn incomplete_count(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Query(query): Query<QuestionQuery>,
) -> Result<Json<IncompleteCountResponse>, (StatusCode, Json<serde_json::Value>)> {
    // pending_answer 计数
    let mut b1 = sqlx::QueryBuilder::new("SELECT COUNT(*) FROM questions q WHERE 1=1");
    apply_access_filters(&mut b1, &auth, &query);
    b1.push(" AND q.metadata->'system_flags' @> '{\"pending_answer\": true}'::jsonb");
    let pending_answer: i64 = b1
        .build_query_scalar::<i64>()
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询待补全答案数失败: {}", e)))?;

    // missing_analysis 计数
    let mut b2 = sqlx::QueryBuilder::new("SELECT COUNT(*) FROM questions q WHERE 1=1");
    apply_access_filters(&mut b2, &auth, &query);
    b2.push(" AND q.metadata->'system_flags' @> '{\"missing_analysis\": true}'::jsonb");
    let missing_analysis: i64 = b2
        .build_query_scalar::<i64>()
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询缺失解析数失败: {}", e)))?;

    // 总待补全（OR 并集）
    let mut b3 = sqlx::QueryBuilder::new("SELECT COUNT(*) FROM questions q WHERE 1=1");
    apply_access_filters(&mut b3, &auth, &query);
    b3.push(" AND (q.metadata->'system_flags' @> '{\"pending_answer\": true}'::jsonb");
    b3.push(" OR q.metadata->'system_flags' @> '{\"missing_analysis\": true}'::jsonb)");
    let total: i64 = b3
        .build_query_scalar::<i64>()
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询总待补全数失败: {}", e)))?;

    Ok(Json(IncompleteCountResponse {
        pending_answer,
        missing_analysis,
        total,
    }))
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
