use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde_json::json;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::auth::permissions::is_admin_user;
use crate::models::paper::{
    AddQuestionRequest, CreatePaperRequest, PaperBrief, PaperDetail, PaperQuestionItem,
    PaperStatus, PaperSummary, QuestionPaperItem, UpdatePaperQuestionRequest, UpdatePaperRequest,
};
use crate::models::PageResult;
use crate::AppState;

// ---------------------------------------------------------------------------
// 试卷 CRUD
// ---------------------------------------------------------------------------

/// GET /api/v1/papers — 试卷列表
pub async fn list_papers(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthUser>,
    Query(query): Query<PaperListQuery>,
) -> Result<Json<PageResult<PaperSummary>>, (StatusCode, Json<serde_json::Value>)> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).min(100);
    let offset = (page - 1) * page_size;

    // 统计总数（参数绑定，避免 SQL 注入）
    let mut count_sql = String::from("SELECT COUNT(*) FROM papers p WHERE 1=1");
    let mut count_bind_status: Option<String> = None;
    let mut count_bind_subject: Option<String> = None;
    if let Some(ref status) = query.status {
        count_sql.push_str(" AND p.status = $1");
        count_bind_status = Some(status.clone());
    }
    if let Some(ref subject) = query.subject {
        let idx = if count_bind_status.is_some() { 2 } else { 1 };
        count_sql.push_str(&format!(" AND p.subject = ${}", idx));
        count_bind_subject = Some(subject.clone());
    }

    let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
    if let Some(s) = count_bind_status {
        count_query = count_query.bind(s);
    }
    if let Some(s) = count_bind_subject {
        count_query = count_query.bind(s);
    }
    let total: i64 = count_query
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("统计试卷总数失败: {}", e)})),
            )
        })?;

    let mut query_builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
        r#"
        SELECT p.*, u.display_name AS creator_name,
               (SELECT COUNT(*) FROM paper_questions pq WHERE pq.paper_id = p.id) AS question_count
        FROM papers p
        LEFT JOIN users u ON u.id = p.creator_id
        WHERE 1=1
        "#,
    );

    // 筛选（使用参数绑定，避免 SQL 注入）
    if let Some(ref status) = query.status {
        query_builder.push(" AND p.status = ").push_bind(status.clone());
    }
    if let Some(ref subject) = query.subject {
        query_builder.push(" AND p.subject = ").push_bind(subject.clone());
    }

    query_builder
        .push(" ORDER BY p.updated_at DESC LIMIT ")
        .push_bind(page_size)
        .push(" OFFSET ")
        .push_bind(offset);

    let papers = query_builder
        .build_query_as::<PaperSummary>()
        .fetch_all(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询试卷失败: {}", e)})),
            )
        })?;

    Ok(Json(PageResult {
        items: papers,
        total,
        page: page as u32,
        page_size: page_size as u32,
    }))
}

/// GET /api/v1/papers/brief — 试卷轻量列表（仅 id + title，供下拉选择）
pub async fn list_papers_brief(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthUser>,
) -> Result<Json<Vec<PaperBrief>>, (StatusCode, Json<serde_json::Value>)> {
    let papers = sqlx::query_as::<_, PaperBrief>(
        r#"
        SELECT id, title
        FROM papers
        ORDER BY updated_at DESC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询试卷简报失败: {}", e)})),
        )
    })?;

    Ok(Json(papers))
}

/// GET /api/v1/questions/:id/papers — 反向查询引用该题目的试卷列表
pub async fn get_question_papers(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<QuestionPaperItem>>, (StatusCode, Json<serde_json::Value>)> {
    let papers = sqlx::query_as::<_, QuestionPaperItem>(
        r#"
        SELECT pq.paper_id, p.title, pq.sort_order, pq.score, pq.section, pq.created_at
        FROM paper_questions pq
        JOIN papers p ON p.id = pq.paper_id
        WHERE pq.question_id = $1
        ORDER BY pq.sort_order, pq.created_at
        "#,
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询题目关联试卷失败: {}", e)})),
        )
    })?;

    Ok(Json(papers))
}

/// GET /api/v1/papers/:id — 试卷详情
pub async fn get_paper(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<PaperDetail>, (StatusCode, Json<serde_json::Value>)> {
    let paper = sqlx::query_as::<_, PaperSummary>(
        r#"
        SELECT p.*, u.display_name AS creator_name,
               (SELECT COUNT(*) FROM paper_questions pq WHERE pq.paper_id = p.id) AS question_count
        FROM papers p
        LEFT JOIN users u ON u.id = p.creator_id
        WHERE p.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询试卷失败: {}", e)})),
        )
    })?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "试卷不存在"}))))?;

    // 获取题目列表
    let questions = sqlx::query_as::<_, PaperQuestionRow>(
        r#"
        SELECT pq.id, pq.paper_id, pq.question_id, pq.sort_order, pq.score, pq.section, pq.created_at,
               q.stem, q.question_type::text, q.difficulty::text
        FROM paper_questions pq
        JOIN questions q ON q.id = pq.question_id
        WHERE pq.paper_id = $1
        ORDER BY pq.sort_order, pq.created_at
        "#,
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询试卷题目失败: {}", e)})),
        )
    })?;

    Ok(Json(PaperDetail {
        id: paper.id,
        title: paper.title,
        description: paper.description,
        subject: paper.subject,
        grade: paper.grade,
        total_score: paper.total_score,
        duration_minutes: paper.duration_minutes,
        status: paper.status,
        creator_id: paper.creator_id,
        creator_name: paper.creator_name,
        created_at: paper.created_at,
        updated_at: paper.updated_at,
        version: paper.version,
        questions: questions
            .into_iter()
            .map(|q| PaperQuestionItem {
                id: q.id,
                question_id: q.question_id,
                sort_order: q.sort_order,
                score: q.score,
                section: q.section,
                stem: q.stem,
                question_type: q.question_type,
                difficulty: q.difficulty,
            })
            .collect(),
    }))
}

/// POST /api/v1/papers — 创建试卷
pub async fn create_paper(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Json(req): Json<CreatePaperRequest>,

) -> Result<(StatusCode, Json<PaperDetail>), (StatusCode, Json<serde_json::Value>)> {
    if req.title.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "试卷标题不能为空"})),
        ));
    }

    let id = Uuid::new_v4();
    let now = chrono::Utc::now();

    sqlx::query(
        r#"
        INSERT INTO papers (id, title, description, subject, grade, total_score, duration_minutes, status, creator_id, created_at, updated_at, version)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        "#,
    )
    .bind(id)
    .bind(&req.title)
    .bind(&req.description)
    .bind(req.subject.as_deref().unwrap_or("数学"))
    .bind(&req.grade)
    .bind(req.total_score.unwrap_or(0))
    .bind(req.duration_minutes)
    .bind(PaperStatus::Draft)
    .bind(auth.id)
    .bind(now)
    .bind(now)
    .bind(1)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("创建试卷失败: {}", e)})),
        )
    })?;

    // 返回详情
    let detail = get_paper_internal(&state.pool, id).await?;

    Ok((StatusCode::CREATED, Json(detail)))
}

/// PUT /api/v1/papers/:id — 更新试卷
pub async fn update_paper(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdatePaperRequest>,
) -> Result<Json<PaperDetail>, (StatusCode, Json<serde_json::Value>)> {
    // 检查是否存在并获取创建者
    let creator_id: Option<Uuid> = sqlx::query_scalar("SELECT creator_id FROM papers WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询试卷失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "试卷不存在"}))))?;

    // 所有权检查：Admin 可操作所有试卷，其他用户仅能操作自己创建的
    if !is_admin_user(&auth) && creator_id != Some(auth.id) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权操作该试卷"}))));
    }

    let now = chrono::Utc::now();
    sqlx::query(
        r#"
        UPDATE papers SET
            title = COALESCE($1, title),
            description = COALESCE($2, description),
            subject = COALESCE($3, subject),
            grade = COALESCE($4, grade),
            total_score = COALESCE($5, total_score),
            duration_minutes = COALESCE($6, duration_minutes),
            updated_at = $7,
            version = version + 1
        WHERE id = $8
        "#,
    )
    .bind(&req.title)
    .bind(&req.description)
    .bind(&req.subject)
    .bind(&req.grade)
    .bind(req.total_score)
    .bind(req.duration_minutes)
    .bind(now)
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("更新试卷失败: {}", e)})),
        )
    })?;

    let detail = get_paper_internal(&state.pool, id).await?;

    Ok(Json(detail))
}

/// DELETE /api/v1/papers/:id — 删除试卷
pub async fn delete_paper(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    // 检查是否存在并获取创建者
    let creator_id: Option<Uuid> = sqlx::query_scalar("SELECT creator_id FROM papers WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询试卷失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "试卷不存在"}))))?;

    // 所有权检查：Admin 可操作所有试卷，其他用户仅能操作自己创建的
    if !is_admin_user(&auth) && creator_id != Some(auth.id) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权操作该试卷"}))));
    }

    sqlx::query("DELETE FROM papers WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("删除试卷失败: {}", e)})),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// 试卷题目管理
// ---------------------------------------------------------------------------

/// POST /api/v1/papers/:id/questions — 添加题目到试卷
pub async fn add_question_to_paper(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(paper_id): Path<Uuid>,
    Json(req): Json<AddQuestionRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    // 检查试卷存在并获取创建者
    let creator_id: Option<Uuid> = sqlx::query_scalar("SELECT creator_id FROM papers WHERE id = $1")
        .bind(paper_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询试卷失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "试卷不存在"}))))?;

    // 所有权检查：Admin 可操作所有试卷，其他用户仅能操作自己创建的
    if !is_admin_user(&auth) && creator_id != Some(auth.id) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权操作该试卷"}))));
    }

    // 检查题目存在
    let q_exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM questions WHERE id = $1")
        .bind(req.question_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0)
        > 0;

    if !q_exists {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error": "题目不存在"}))));
    }

    // 检查是否已添加
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM paper_questions WHERE paper_id = $1 AND question_id = $2",
    )
    .bind(paper_id)
    .bind(req.question_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0)
        > 0;

    if exists {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "该题目已在试卷中"})),
        ));
    }

    let id = Uuid::new_v4();
    let sort_order = req.sort_order.unwrap_or(0);
    let score = req.score.unwrap_or(0);

    sqlx::query(
        r#"
        INSERT INTO paper_questions (id, paper_id, question_id, sort_order, score, section, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(id)
    .bind(paper_id)
    .bind(req.question_id)
    .bind(sort_order)
    .bind(score)
    .bind(&req.section)
    .bind(chrono::Utc::now())
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("添加题目到试卷失败: {}", e)})),
        )
    })?;

    // 更新试卷总分
    update_paper_total_score(&state.pool, paper_id).await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({"id": id, "message": "题目已添加到试卷"})),
    ))
}

/// PUT /api/v1/papers/:paper_id/questions/:question_id — 更新试卷题目
pub async fn update_paper_question(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path((paper_id, question_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdatePaperQuestionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // 检查试卷存在并获取创建者
    let creator_id: Option<Uuid> = sqlx::query_scalar("SELECT creator_id FROM papers WHERE id = $1")
        .bind(paper_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询试卷失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "试卷不存在"}))))?;

    // 所有权检查：Admin 可操作所有试卷，其他用户仅能操作自己创建的
    if !is_admin_user(&auth) && creator_id != Some(auth.id) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权操作该试卷"}))));
    }

    let result = sqlx::query(
        r#"
        UPDATE paper_questions SET
            score = COALESCE($1, score),
            sort_order = COALESCE($2, sort_order),
            section = COALESCE($3, section)
        WHERE paper_id = $4 AND question_id = $5
        "#,
    )
    .bind(req.score)
    .bind(req.sort_order)
    .bind(&req.section)
    .bind(paper_id)
    .bind(question_id)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("更新试卷题目失败: {}", e)})),
        )
    })?;

    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error": "试卷题目不存在"}))));
    }

    update_paper_total_score(&state.pool, paper_id).await?;

    Ok(Json(json!({"message": "已更新"})))
}

/// DELETE /api/v1/papers/:paper_id/questions/:question_id — 从试卷移除题目
pub async fn remove_question_from_paper(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path((paper_id, question_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // 检查试卷存在并获取创建者
    let creator_id: Option<Uuid> = sqlx::query_scalar("SELECT creator_id FROM papers WHERE id = $1")
        .bind(paper_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询试卷失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "试卷不存在"}))))?;

    // 所有权检查：Admin 可操作所有试卷，其他用户仅能操作自己创建的
    if !is_admin_user(&auth) && creator_id != Some(auth.id) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权操作该试卷"}))));
    }

    let result = sqlx::query("DELETE FROM paper_questions WHERE paper_id = $1 AND question_id = $2")
        .bind(paper_id)
        .bind(question_id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("移除题目失败: {}", e)})),
            )
        })?;

    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error": "试卷题目不存在"}))));
    }

    update_paper_total_score(&state.pool, paper_id).await?;

    Ok(Json(json!({"message": "已移除"})))
}

/// POST /api/v1/papers/:id/publish — 发布试卷
pub async fn publish_paper(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // 检查试卷存在并获取创建者
    let creator_id: Option<Uuid> = sqlx::query_scalar("SELECT creator_id FROM papers WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "试卷不存在"}))))?;

    // 所有权检查：Admin 可操作所有试卷，其他用户仅能操作自己创建的
    if !is_admin_user(&auth) && creator_id != Some(auth.id) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权操作该试卷"}))));
    }

    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM paper_questions WHERE paper_id = $1",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询失败: {}", e)})),
        )
    })?;

    if count == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "试卷中没有题目，无法发布"})),
        ));
    }

    let now = chrono::Utc::now();
    sqlx::query("UPDATE papers SET status = $3, updated_at = $1, version = version + 1 WHERE id = $2")
        .bind(now)
        .bind(id)
        .bind(PaperStatus::Published)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("发布失败: {}", e)})),
            )
        })?;

    Ok(Json(json!({"message": "试卷已发布"})))
}

// ---------------------------------------------------------------------------
// 内部辅助
// ---------------------------------------------------------------------------

#[derive(Debug, sqlx::FromRow)]
struct PaperQuestionRow {
    id: Uuid,
    paper_id: Uuid,
    question_id: Uuid,
    sort_order: i32,
    score: i32,
    section: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    stem: String,
    question_type: String,
    difficulty: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct PaperListQuery {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub status: Option<String>,
    pub subject: Option<String>,
}

/// 获取试卷详情（内部使用）
async fn get_paper_internal(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> Result<PaperDetail, (StatusCode, Json<serde_json::Value>)> {
    let paper = sqlx::query_as::<_, PaperSummary>(
        r#"
        SELECT p.*, u.display_name AS creator_name,
               (SELECT COUNT(*) FROM paper_questions pq WHERE pq.paper_id = p.id) AS question_count
        FROM papers p
        LEFT JOIN users u ON u.id = p.creator_id
        WHERE p.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询试卷失败: {}", e)})),
        )
    })?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "试卷不存在"}))))?;

    let questions = sqlx::query_as::<_, PaperQuestionRow>(
        r#"
        SELECT pq.id, pq.paper_id, pq.question_id, pq.sort_order, pq.score, pq.section, pq.created_at,
               q.stem, q.question_type::text, q.difficulty::text
        FROM paper_questions pq
        JOIN questions q ON q.id = pq.question_id
        WHERE pq.paper_id = $1
        ORDER BY pq.sort_order, pq.created_at
        "#,
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询试卷题目失败: {}", e)})),
        )
    })?;

    Ok(PaperDetail {
        id: paper.id,
        title: paper.title,
        description: paper.description,
        subject: paper.subject,
        grade: paper.grade,
        total_score: paper.total_score,
        duration_minutes: paper.duration_minutes,
        status: paper.status,
        creator_id: paper.creator_id,
        creator_name: paper.creator_name,
        created_at: paper.created_at,
        updated_at: paper.updated_at,
        version: paper.version,
        questions: questions
            .into_iter()
            .map(|q| PaperQuestionItem {
                id: q.id,
                question_id: q.question_id,
                sort_order: q.sort_order,
                score: q.score,
                section: q.section,
                stem: q.stem,
                question_type: q.question_type,
                difficulty: q.difficulty,
            })
            .collect(),
    })
}

/// 重新计算试卷总分
async fn update_paper_total_score(
    pool: &sqlx::PgPool,
    paper_id: Uuid,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let total: Option<i32> = sqlx::query_scalar(
        "SELECT COALESCE(SUM(score), 0) FROM paper_questions WHERE paper_id = $1",
    )
    .bind(paper_id)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("计算总分失败: {}", e)})),
        )
    })?;

    sqlx::query("UPDATE papers SET total_score = $1 WHERE id = $2")
        .bind(total.unwrap_or(0))
        .bind(paper_id)
        .execute(pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("更新总分失败: {}", e)})),
            )
        })?;

    Ok(())
}
