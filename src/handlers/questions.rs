use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::auth::permissions::{
    can_access_space, can_edit_question, can_review_question, can_write_in_space,
    ensure_personal_space, ensure_public_space, get_space, is_admin, list_reviewers,
};
use crate::models::question::{
    CreateQuestionRequest, KnowledgePointSummary, Question, QuestionDetail, QuestionQuery,
    QuestionStatus, QuestionSummary, ReviewActionRequest, SubmitReviewRequest,
    TransferQuestionRequest, UpdateQuestionRequest,
};
use crate::models::space::SpaceKind;
use crate::models::PageResult;
use crate::AppState;

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

async fn save_version(
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

async fn update_knowledge_points(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    question_id: Uuid,
    kp_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM question_knowledge_points WHERE question_id = $1")
        .bind(question_id)
        .execute(&mut **tx)
        .await?;

    for kp_id in kp_ids {
        sqlx::query(
            "INSERT INTO question_knowledge_points (question_id, knowledge_point_id) VALUES ($1, $2)",
        )
        .bind(question_id)
        .bind(kp_id)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

async fn get_question_knowledge_points(
    pool: &sqlx::PgPool,
    question_id: Uuid,
) -> Result<Vec<KnowledgePointSummary>, sqlx::Error> {
    sqlx::query_as::<_, KnowledgePointSummary>(
        r#"
        SELECT kp.id, kp.name
        FROM knowledge_points kp
        JOIN question_knowledge_points qkp ON qkp.knowledge_point_id = kp.id
        WHERE qkp.question_id = $1
        ORDER BY kp.sort_order, kp.name
        "#,
    )
    .bind(question_id)
    .fetch_all(pool)
    .await
}

async fn replace_reviewers(
    pool: &sqlx::PgPool,
    question_id: Uuid,
    reviewer_ids: &[Uuid],
    assigned_by: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM question_reviewers WHERE question_id = $1")
        .bind(question_id)
        .execute(pool)
        .await?;

    let now = chrono::Utc::now();
    for uid in reviewer_ids {
        sqlx::query(
            r#"
            INSERT INTO question_reviewers (question_id, user_id, assigned_by, assigned_at)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(question_id)
        .bind(uid)
        .bind(assigned_by)
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn build_detail(
    pool: &sqlx::PgPool,
    auth: &AuthUser,
    question: Question,
    creator_name: Option<String>,
) -> Result<QuestionDetail, sqlx::Error> {
    let kps = get_question_knowledge_points(pool, question.id)
        .await
        .unwrap_or_default();
    let reviewer_ids = list_reviewers(pool, question.id).await.unwrap_or_default();

    let mut detail = QuestionDetail::from((question.clone(), kps));
    detail.creator_name = creator_name;
    detail.reviewer_ids = reviewer_ids;

    if let Ok(Some(space)) = get_space(pool, question.space_id).await {
        detail.can_review = can_review_question(
            pool,
            auth,
            &space,
            question.creator_id,
            &question.status,
            question.id,
        )
        .await
        .unwrap_or(false);
    }

    Ok(detail)
}

fn db_err(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": msg.into()})),
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
        "SELECT q.id, q.stem, q.question_type, q.difficulty, q.default_score, q.status, q.grade, \
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
        if is_admin(&auth.role) {
            builder.push(" OR TRUE");
        }
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
    if is_admin(&auth.role) {
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
    if let Some(ref diff) = query.difficulty {
        builder.push(" AND q.difficulty = ");
        builder.push_bind(diff);
    }
    if let Some(ref grade) = query.grade {
        builder.push(" AND q.grade = ");
        builder.push_bind(grade);
    }
    if let Some(ref kp_id) = query.knowledge_point_id {
        builder.push(
            " AND q.id IN (SELECT question_id FROM question_knowledge_points WHERE knowledge_point_id = ",
        );
        builder.push_bind(kp_id);
        builder.push(")");
    }
    if let Some(ref creator) = query.creator_id {
        builder.push(" AND q.creator_id = ");
        builder.push_bind(creator);
    }
    if let Some(ref keyword) = query.keyword {
        builder.push(" AND q.stem ILIKE ");
        builder.push_bind(format!("%{}%", keyword));
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

    sqlx::query(
        r#"
        INSERT INTO questions (id, stem, question_type, difficulty, default_score, status,
            options, correct_answer, analysis, grading_criteria, grade, semester, source,
            creator_id, created_at, updated_at, version, space_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
        "#,
    )
    .bind(id)
    .bind(&req.stem)
    .bind(&req.question_type)
    .bind(&req.difficulty)
    .bind(req.default_score.unwrap_or(5))
    .bind(QuestionStatus::Draft)
    .bind(&req.options)
    .bind(&req.correct_answer)
    .bind(&req.analysis)
    .bind(&req.grading_criteria)
    .bind(&req.grade)
    .bind(&req.semester)
    .bind(&req.source)
    .bind(creator_id)
    .bind(now)
    .bind(now)
    .bind(version)
    .bind(space_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err(format!("创建题目失败: {}", e)))?;

    if let Some(ref kp_ids) = req.knowledge_point_ids {
        update_knowledge_points(&mut tx, id, kp_ids)
            .await
            .map_err(|e| db_err(format!("关联知识点失败: {}", e)))?;
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
        && (space.kind != SpaceKind::Public || question.status == QuestionStatus::Published || is_admin(&auth.role));

    if !can_see {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权查看该题目"}))));
    }

    let creator_name = if let Some(cid) = question.creator_id {
        sqlx::query_scalar::<_, String>("SELECT display_name FROM users WHERE id = $1")
            .bind(cid)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten()
    } else {
        None
    };

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

    if !can_edit_question(
        &state.pool,
        &auth_user,
        &space,
        existing.creator_id,
        &existing.status,
    )
    .await
    .map_err(|e| db_err(format!("权限检查失败: {}", e)))?
    {
        if existing.status != QuestionStatus::Draft && existing.status != QuestionStatus::Rejected {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({"error": "只有草稿或驳回状态的题目可以编辑"})),
            ));
        }
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权编辑该题目"}))));
    }

    let now = chrono::Utc::now();
    let new_version = existing.version + 1;

    let mut tx = state.pool.begin().await.map_err(|e| db_err(format!("开启事务失败: {}", e)))?;

    sqlx::query(
        r#"
        UPDATE questions SET
            stem = COALESCE($1, stem),
            question_type = COALESCE($2, question_type),
            difficulty = COALESCE($3, difficulty),
            default_score = COALESCE($4, default_score),
            options = COALESCE($5, options),
            correct_answer = COALESCE($6, correct_answer),
            analysis = COALESCE($7, analysis),
            grading_criteria = COALESCE($8, grading_criteria),
            grade = COALESCE($9, grade),
            semester = COALESCE($10, semester),
            source = COALESCE($11, source),
            status = 'draft'::question_status,
            updated_by = $12,
            updated_at = $13,
            version = $14
        WHERE id = $15
        "#,
    )
    .bind(&req.stem)
    .bind(&req.question_type)
    .bind(&req.difficulty)
    .bind(req.default_score.map(|s| s as i32))
    .bind(&req.options)
    .bind(&req.correct_answer)
    .bind(&req.analysis)
    .bind(&req.grading_criteria)
    .bind(&req.grade)
    .bind(&req.semester)
    .bind(&req.source)
    .bind(auth_user.id)
    .bind(now)
    .bind(new_version)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err(format!("更新题目失败: {}", e)))?;

    if let Some(ref kp_ids) = req.knowledge_point_ids {
        update_knowledge_points(&mut tx, id, kp_ids)
            .await
            .map_err(|e| db_err(format!("更新知识点关联失败: {}", e)))?;
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

    if existing.status != QuestionStatus::Draft {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "只有草稿状态的题目可以删除"})),
        ));
    }

    let space = get_space(&state.pool, existing.space_id)
        .await
        .map_err(|e| db_err(format!("查询空间失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    if !can_edit_question(
        &state.pool,
        &auth_user,
        &space,
        existing.creator_id,
        &existing.status,
    )
    .await
    .map_err(|e| db_err(format!("权限检查失败: {}", e)))?
    {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权删除该题目"}))));
    }

    sqlx::query("DELETE FROM questions WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| db_err(format!("删除题目失败: {}", e)))?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// 审核流程
// ---------------------------------------------------------------------------

/// POST /api/v1/questions/:id/submit
pub async fn submit_question(
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

    if existing.creator_id.map(|uid| uid != auth_user.id).unwrap_or(true) && !is_admin(&auth_user.role)
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "只有题目的创建者可以提交审核"})),
        ));
    }

    if existing.status != QuestionStatus::Draft && existing.status != QuestionStatus::Rejected {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "只有草稿或驳回状态的题目可以提交审核"})),
        ));
    }

    if let Some(ref ids) = req.reviewer_ids {
        replace_reviewers(&state.pool, id, ids, auth_user.id)
            .await
            .map_err(|e| db_err(format!("设置审题人失败: {}", e)))?;
    } else {
        // 清空旧指定，走空间默认规则
        sqlx::query("DELETE FROM question_reviewers WHERE question_id = $1")
            .bind(id)
            .execute(&state.pool)
            .await
            .map_err(|e| db_err(format!("清除审题人失败: {}", e)))?;
    }

    sqlx::query(
        "UPDATE questions SET status = 'pending'::question_status, updated_at = $1 WHERE id = $2",
    )
    .bind(chrono::Utc::now())
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(|e| db_err(format!("提交审核失败: {}", e)))?;

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

/// POST /api/v1/questions/:id/review
pub async fn review_question(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<ReviewActionRequest>,
) -> Result<Json<QuestionDetail>, (StatusCode, Json<serde_json::Value>)> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| db_err(format!("开启事务失败: {}", e)))?;

    let existing = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| db_err(format!("查询题目失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "题目不存在"}))))?;

    let space = get_space(&state.pool, existing.space_id)
        .await
        .map_err(|e| db_err(format!("查询空间失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    if !can_review_question(
        &state.pool,
        &auth_user,
        &space,
        existing.creator_id,
        &existing.status,
        existing.id,
    )
    .await
    .map_err(|e| db_err(format!("权限检查失败: {}", e)))?
    {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权审核该题目"}))));
    }

    if existing.status != QuestionStatus::Pending {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "只有待审核状态的题目可以审核"})),
        ));
    }

    let new_status = match req.action.as_str() {
        "approved" => QuestionStatus::Published,
        "rejected" => QuestionStatus::Rejected,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "审核操作必须是 approved 或 rejected"})),
            ));
        }
    };

    sqlx::query("UPDATE questions SET status = $1, updated_at = $2 WHERE id = $3")
        .bind(&new_status)
        .bind(chrono::Utc::now())
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| db_err(format!("审核操作失败: {}", e)))?;

    sqlx::query(
        r#"
        INSERT INTO review_records (id, question_id, reviewer_id, action, comment, created_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(id)
    .bind(auth_user.id)
    .bind(&req.action)
    .bind(&req.comment)
    .bind(chrono::Utc::now())
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err(format!("记录审核失败: {}", e)))?;

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
            id, stem, question_type, difficulty, default_score, status,
            options, correct_answer, analysis, grading_criteria, grade, semester, source,
            creator_id, created_at, updated_at, version, space_id, origin_question_id
        )
        VALUES (
            $1, $2, $3, $4, $5, 'published'::question_status,
            $6, $7, $8, $9, $10, $11, $12,
            $13, $14, $15, 1, $16, $17
        )
        "#,
    )
    .bind(id)
    .bind(&src.stem)
    .bind(&src.question_type)
    .bind(&src.difficulty)
    .bind(src.default_score)
    .bind(&src.options)
    .bind(&src.correct_answer)
    .bind(&src.analysis)
    .bind(&src.grading_criteria)
    .bind(&src.grade)
    .bind(&src.semester)
    .bind(&src.source)
    .bind(creator_id)
    .bind(now)
    .bind(now)
    .bind(target_space_id)
    .bind(origin_id)
    .execute(&mut *tx)
    .await?;

    // 复制知识点关联
    let kp_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT knowledge_point_id FROM question_knowledge_points WHERE question_id = $1",
    )
    .bind(src.id)
    .fetch_all(&mut *tx)
    .await?;

    for kp_id in kp_ids {
        sqlx::query(
            "INSERT INTO question_knowledge_points (question_id, knowledge_point_id) VALUES ($1, $2)",
        )
        .bind(id)
        .bind(kp_id)
        .execute(&mut *tx)
        .await?;
    }

    save_version(&mut tx, id, 1, Some(creator_id)).await?;
    tx.commit().await?;
    Ok(id)
}
