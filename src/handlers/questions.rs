use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::models::question::{
    CreateQuestionRequest, KnowledgePointSummary, Question, QuestionDetail, QuestionQuery,
    QuestionStatus, QuestionSummary, ReviewActionRequest, SubmitReviewRequest,
    UpdateQuestionRequest,
};
use crate::AppState;

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

/// 保存一个版本快照
async fn save_version(
    pool: &sqlx::PgPool,
    question_id: Uuid,
    version: i32,
    created_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    let question = sqlx::query_as::<_, Question>(
        "SELECT * FROM questions WHERE id = $1",
    )
    .bind(question_id)
    .fetch_one(pool)
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
    .execute(pool)
    .await?;

    Ok(())
}

/// 更新题目关联的知识点
async fn update_knowledge_points(
    pool: &sqlx::PgPool,
    question_id: Uuid,
    kp_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM question_knowledge_points WHERE question_id = $1")
        .bind(question_id)
        .execute(pool)
        .await?;

    for kp_id in kp_ids {
        sqlx::query(
            "INSERT INTO question_knowledge_points (question_id, knowledge_point_id) VALUES ($1, $2)",
        )
        .bind(question_id)
        .bind(kp_id)
        .execute(pool)
        .await?;
    }

    Ok(())
}

/// 获取题目的知识点列表
async fn get_question_knowledge_points(
    pool: &sqlx::PgPool,
    question_id: Uuid,
) -> Result<Vec<KnowledgePointSummary>, sqlx::Error> {
    let rows = sqlx::query_as::<_, KnowledgePointSummary>(
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
    .await?;

    Ok(rows)
}

// ---------------------------------------------------------------------------
// 统计
// ---------------------------------------------------------------------------

/// 题目统计
#[derive(Debug, Serialize)]
pub struct QuestionStats {
    pub total: i64,
    pub draft: i64,
    pub pending: i64,
    pub rejected: i64,
    pub published: i64,
    pub disabled: i64,
}

/// GET /api/v1/questions/stats — 题目统计（各状态数量）
pub async fn question_stats(
    State(state): State<AppState>,
) -> Result<Json<QuestionStats>, (StatusCode, Json<serde_json::Value>)> {
    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT status::text, COUNT(*) FROM questions GROUP BY status",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("统计失败: {}", e)})),
        )
    })?;

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

/// GET /api/v1/questions — 搜索/列表
pub async fn list_questions(
    State(state): State<AppState>,
    Query(query): Query<QuestionQuery>,
) -> Result<Json<Vec<QuestionSummary>>, (StatusCode, Json<serde_json::Value>)> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).min(100);
    let offset = (page - 1) * page_size;

    use sqlx::QueryBuilder;

    let mut builder = QueryBuilder::new(
        "SELECT q.id, q.stem, q.question_type, q.difficulty, q.default_score, q.status, q.grade, q.creator_id, u.display_name AS creator_name, q.created_at, q.updated_at, q.version FROM questions q LEFT JOIN users u ON u.id = q.creator_id WHERE 1=1",
    );

    if let Some(ref status) = query.status {
        builder.push(" AND status = ");
        builder.push_bind(status);
    }
    if let Some(ref qt) = query.question_type {
        builder.push(" AND question_type = ");
        builder.push_bind(qt);
    }
    if let Some(ref diff) = query.difficulty {
        builder.push(" AND difficulty = ");
        builder.push_bind(diff);
    }
    if let Some(ref grade) = query.grade {
        builder.push(" AND grade = ");
        builder.push_bind(grade);
    }
    if let Some(ref kp_id) = query.knowledge_point_id {
        builder.push(" AND id IN (SELECT question_id FROM question_knowledge_points WHERE knowledge_point_id = ");
        builder.push_bind(kp_id);
        builder.push(")");
    }
    if let Some(ref creator) = query.creator_id {
        builder.push(" AND creator_id = ");
        builder.push_bind(creator);
    }
    if let Some(ref keyword) = query.keyword {
        builder.push(" AND stem ILIKE ");
        builder.push_bind(format!("%{}%", keyword));
    }

    builder.push(" ORDER BY updated_at DESC LIMIT ");
    builder.push_bind(page_size as i64);
    builder.push(" OFFSET ");
    builder.push_bind(offset as i64);

    let questions = builder.build_query_as::<QuestionSummary>().fetch_all(&state.pool).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询题目失败: {}", e)})),
        )
    })?;

    Ok(Json(questions))
}

/// POST /api/v1/questions — 创建草稿
pub async fn create_question(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateQuestionRequest>,
) -> Result<(StatusCode, Json<QuestionDetail>), (StatusCode, Json<serde_json::Value>)> {
    let id = Uuid::new_v4();
    let now = chrono::Utc::now();
    let creator_id = auth_user.id;
    let version = 1;

    sqlx::query(
        r#"
        INSERT INTO questions (id, stem, question_type, difficulty, default_score, status,
            options, correct_answer, analysis, grading_criteria, grade, semester, source,
            creator_id, created_at, updated_at, version)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
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
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("创建题目失败: {}", e)})),
        )
    })?;

    // 关联知识点
    if let Some(ref kp_ids) = req.knowledge_point_ids {
        update_knowledge_points(&state.pool, id, kp_ids).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("关联知识点失败: {}", e)})),
            )
        })?;
    }

    // 保存初始版本
    save_version(&state.pool, id, version, Some(creator_id)).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("保存版本失败: {}", e)})),
        )
    })?;

    let question = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询题目失败: {}", e)})),
            )
        })?;

    let kps = get_question_knowledge_points(&state.pool, id).await.unwrap_or_default();

    Ok((StatusCode::CREATED, Json(QuestionDetail::from((question, kps)))))
}

/// GET /api/v1/questions/:id — 题目详情
pub async fn get_question(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<QuestionDetail>, (StatusCode, Json<serde_json::Value>)> {
    // 带创建者名称的题目
    #[derive(sqlx::FromRow)]
    struct QuestionRow {
        id: Uuid,
        stem: String,
        question_type: crate::models::question::QuestionType,
        difficulty: crate::models::question::Difficulty,
        default_score: i32,
        status: crate::models::question::QuestionStatus,
        options: Option<serde_json::Value>,
        correct_answer: serde_json::Value,
        analysis: Option<String>,
        grading_criteria: Option<serde_json::Value>,
        grade: Option<String>,
        semester: Option<String>,
        source: Option<String>,
        creator_id: Option<Uuid>,
        creator_name: Option<String>,
        created_at: chrono::DateTime<chrono::Utc>,
        updated_by: Option<Uuid>,
        updated_at: chrono::DateTime<chrono::Utc>,
        version: i32,
    }

    let row = sqlx::query_as::<_, QuestionRow>(
        r#"
        SELECT q.*, u.display_name AS creator_name
        FROM questions q
        LEFT JOIN users u ON u.id = q.creator_id
        WHERE q.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询题目失败: {}", e)})),
        )
    })?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "题目不存在"}))))?;

    let kps = get_question_knowledge_points(&state.pool, id).await.unwrap_or_default();

    Ok(Json(QuestionDetail {
        id: row.id,
        stem: row.stem,
        question_type: row.question_type,
        difficulty: row.difficulty,
        default_score: row.default_score,
        status: row.status,
        options: row.options,
        correct_answer: row.correct_answer,
        analysis: row.analysis,
        grading_criteria: row.grading_criteria,
        grade: row.grade,
        semester: row.semester,
        source: row.source,
        creator_id: row.creator_id,
        creator_name: row.creator_name,
        created_at: row.created_at,
        updated_by: row.updated_by,
        updated_at: row.updated_at,
        version: row.version,
        knowledge_points: kps,
    }))
}

/// PUT /api/v1/questions/:id — 更新题目（仅草稿/驳回状态可编辑）
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
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询题目失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "题目不存在"}))))?;

    // 仅草稿或驳回状态可编辑
    if existing.status != QuestionStatus::Draft && existing.status != QuestionStatus::Rejected {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "只有草稿或驳回状态的题目可以编辑"})),
        ));
    }

    let now = chrono::Utc::now();
    let new_version = existing.version + 1;

    sqlx::query(
        r#"
        UPDATE questions SET
            stem = COALESCE($1, stem),
            difficulty = COALESCE($2, difficulty),
            default_score = COALESCE($3, default_score),
            options = COALESCE($4, options),
            correct_answer = COALESCE($5, correct_answer),
            analysis = COALESCE($6, analysis),
            grading_criteria = COALESCE($7, grading_criteria),
            grade = COALESCE($8, grade),
            semester = COALESCE($9, semester),
            source = COALESCE($10, source),
            status = 'draft'::question_status,
            updated_by = $11,
            updated_at = $12,
            version = $13
        WHERE id = $14
        "#,
    )
    .bind(&req.stem)
    .bind(&req.difficulty)
    .bind(req.default_score.map(|s| s as i32))
    .bind(&req.options)
    .bind(&req.correct_answer)
    .bind(&req.analysis)
    .bind(&req.grading_criteria)
    .bind(&req.grade)
    .bind(&req.semester)
    .bind(&req.source)
    .bind(auth_user.id) // updated_by
    .bind(now)
    .bind(new_version)
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("更新题目失败: {}", e)})),
        )
    })?;

    // 更新知识点关联
    if let Some(ref kp_ids) = req.knowledge_point_ids {
        update_knowledge_points(&state.pool, id, kp_ids).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("更新知识点关联失败: {}", e)})),
            )
        })?;
    }

    // 保存版本
    save_version(&state.pool, id, new_version, Some(auth_user.id)).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("保存版本失败: {}", e)})),
        )
    })?;

    let question = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询题目失败: {}", e)})),
            )
        })?;

    let kps = get_question_knowledge_points(&state.pool, id).await.unwrap_or_default();

    Ok(Json(QuestionDetail::from((question, kps))))
}

/// DELETE /api/v1/questions/:id — 删除题目（仅草稿状态）
pub async fn delete_question(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let existing = sqlx::query_scalar::<_, QuestionStatus>(
        "SELECT status FROM questions WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询题目失败: {}", e)})),
        )
    })?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "题目不存在"}))))?;

    if existing != QuestionStatus::Draft {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "只有草稿状态的题目可以删除"})),
        ));
    }

    sqlx::query("DELETE FROM questions WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("删除题目失败: {}", e)})),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// 审核流程
// ---------------------------------------------------------------------------

/// POST /api/v1/questions/:id/submit — 提交审核
pub async fn submit_question(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(_req): Json<SubmitReviewRequest>,
) -> Result<Json<QuestionDetail>, (StatusCode, Json<serde_json::Value>)> {
    let existing = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询题目失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "题目不存在"}))))?;

    // 仅创建者可提交审核
    if existing.creator_id.map(|uid| uid != auth_user.id).unwrap_or(true) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "只有题目的创建者可以提交审核"})),
        ));
    }

    // 仅草稿/驳回状态可提交审核
    if existing.status != QuestionStatus::Draft && existing.status != QuestionStatus::Rejected {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "只有草稿或驳回状态的题目可以提交审核"})),
        ));
    }

    sqlx::query("UPDATE questions SET status = 'pending'::question_status, updated_at = $1 WHERE id = $2")
        .bind(chrono::Utc::now())
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("提交审核失败: {}", e)})),
            )
        })?;

    let question = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询题目失败: {}", e)})),
            )
        })?;

    let kps = get_question_knowledge_points(&state.pool, id).await.unwrap_or_default();

    Ok(Json(QuestionDetail::from((question, kps))))
}

/// POST /api/v1/questions/:id/review — 审核通过/驳回
pub async fn review_question(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<ReviewActionRequest>,
) -> Result<Json<QuestionDetail>, (StatusCode, Json<serde_json::Value>)> {
    let existing = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询题目失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "题目不存在"}))))?;

    // 仅组长/管理员可审核
    if auth_user.role != "GroupLeader" && auth_user.role != "Admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "仅教研组长或管理员可以审核题目"})),
        ));
    }

    // 创建者回避：组长不能审核自己的题目
    if existing.creator_id.map(|uid| uid == auth_user.id).unwrap_or(false) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "教研组长不能审核自己创建的题目"})),
        ));
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

    // 更新题目状态
    sqlx::query(
        "UPDATE questions SET status = $1, updated_at = $2 WHERE id = $3",
    )
    .bind(&new_status)
    .bind(chrono::Utc::now())
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("审核操作失败: {}", e)})),
        )
    })?;

    // 记录审核记录
    let review_id = Uuid::new_v4();

    sqlx::query(
        r#"
        INSERT INTO review_records (id, question_id, reviewer_id, action, comment, created_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(review_id)
    .bind(id)
    .bind(auth_user.id) // reviewer_id
    .bind(&req.action)
    .bind(&req.comment)
    .bind(chrono::Utc::now())
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("记录审核失败: {}", e)})),
        )
    })?;

    let question = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询题目失败: {}", e)})),
            )
        })?;

    let kps = get_question_knowledge_points(&state.pool, id).await.unwrap_or_default();

    Ok(Json(QuestionDetail::from((question, kps))))
}
