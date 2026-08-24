//! V2.1.1 P0-B：QuestionCollection 管理
//!
//! - `GET /collections`：集合列表（可按 document_id 过滤）
//! - `GET /collections/{id}`：集合详情（来源链路 + 题目列表）
//! - `POST /collections/{id}/questions/batch`：批量添加题目（Mixed 人工分组）
//! - `DELETE /collections/{id}/questions/{question_id}`：移除题目
//! - `get_or_create_collection`：Worker 阶段 2 幂等复用键 (document_id, title)

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::auth::permissions::is_admin_user;
use crate::models::collection::{
    BatchAddQuestionsRequest, CollectionDetail, CollectionQuestionItem, QuestionCollection,
};
use crate::models::document::is_valid_collection_type;
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

fn can_manage(c: &QuestionCollection, auth: &AuthUser) -> bool {
    c.creator_id == auth.id || is_admin_user(auth)
}

const COLLECTION_COLUMNS: &str = "id, document_id, creator_id, title, collection_type, type_label, \
     source_type, subject, stage, grade, semester, chapter_id, metadata, created_at, updated_at";

async fn load_collection(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> Result<Option<QuestionCollection>, (StatusCode, Json<serde_json::Value>)> {
    sqlx::query_as::<_, QuestionCollection>(&format!(
        "SELECT {COLLECTION_COLUMNS} FROM question_collections WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err(format!("查询集合失败: {e}")))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CollectionListQuery {
    pub document_id: Option<Uuid>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

/// GET /api/v1/collections — 集合列表
pub async fn list_collections(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Query(query): Query<CollectionListQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).min(100);
    let offset = (page - 1) * page_size;

    let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(&format!(
        "SELECT {COLLECTION_COLUMNS} FROM question_collections WHERE creator_id = "
    ));
    builder.push_bind(auth.id);
    if let Some(doc_id) = query.document_id {
        builder.push(" AND document_id = ").push_bind(doc_id);
    }
    builder
        .push(" ORDER BY created_at DESC LIMIT ")
        .push_bind(page_size)
        .push(" OFFSET ")
        .push_bind(offset);

    let items: Vec<QuestionCollection> = builder
        .build_query_as()
        .fetch_all(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询集合列表失败: {e}")))?;

    let mut count_builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
        "SELECT COUNT(*) FROM question_collections WHERE creator_id = ",
    );
    count_builder.push_bind(auth.id);
    if let Some(doc_id) = query.document_id {
        count_builder.push(" AND document_id = ").push_bind(doc_id);
    }
    let total: i64 = count_builder
        .build_query_scalar()
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("统计集合总数失败: {e}")))?;

    Ok(Json(json!({
        "items": items,
        "total": total,
        "page": page,
        "page_size": page_size
    })))
}

/// GET /api/v1/collections/{id} — 集合详情（来源链路 + 题目）
pub async fn get_collection(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<CollectionDetail>, (StatusCode, Json<serde_json::Value>)> {
    let collection = load_collection(&state.pool, id).await?.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "集合不存在"})),
        )
    })?;
    if !can_manage(&collection, &auth) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "集合不存在"})),
        ));
    }

    // 来源 Document 摘要
    let (document_title, document_type): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT title, document_type FROM documents WHERE id = $1")
            .bind(collection.document_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| db_err(format!("查询来源 Document 失败: {e}")))?
            .unwrap_or((None, None));

    // 题目列表
    let questions = sqlx::query_as::<_, CollectionQuestionItem>(
        r#"
        SELECT cq.id, cq.question_id, cq.question_no, cq.display_order, cq.score,
               q.stem, q.question_type::text, q.difficulty::text
        FROM collection_questions cq
        JOIN questions q ON q.id = cq.question_id
        WHERE cq.collection_id = $1
        ORDER BY cq.display_order, cq.created_at
        "#,
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| db_err(format!("查询集合题目失败: {e}")))?;

    Ok(Json(CollectionDetail {
        id: collection.id,
        document_id: collection.document_id,
        creator_id: collection.creator_id,
        title: collection.title,
        collection_type: collection.collection_type,
        type_label: collection.type_label,
        source_type: collection.source_type,
        subject: collection.subject,
        stage: collection.stage,
        grade: collection.grade,
        semester: collection.semester,
        chapter_id: collection.chapter_id,
        metadata: collection.metadata,
        created_at: collection.created_at,
        updated_at: collection.updated_at,
        document_title,
        document_type,
        questions,
    }))
}

/// POST /api/v1/collections/{id}/questions/batch — 批量添加题目（人工分组）
pub async fn batch_add_questions(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<BatchAddQuestionsRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    if req.questions.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "题目列表不能为空"})),
        ));
    }
    if req.questions.len() > 500 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "单次最多添加 500 道题"})),
        ));
    }

    let collection = load_collection(&state.pool, id).await?.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "集合不存在"})),
        )
    })?;
    if !can_manage(&collection, &auth) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "集合不存在"})),
        ));
    }

    // 当前最大 display_order（未指定时自动编号）
    let max_order: Option<i32> = sqlx::query_scalar(
        "SELECT MAX(display_order) FROM collection_questions WHERE collection_id = $1",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| db_err(format!("查询集合最大顺序失败: {e}")))?;
    let mut next_order = max_order.unwrap_or(0) + 1;

    // 校验题目存在性并批量插入（(collection_id, question_id) 冲突跳过）
    let mut inserted: usize = 0;
    let mut skipped: usize = 0;
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| db_err(format!("开启事务失败: {e}")))?;

    for q in &req.questions {
        let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM questions WHERE id = $1")
            .bind(q.question_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| db_err(format!("校验题目失败: {e}")))?;
        if exists == 0 {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"error": format!("题目 {} 不存在", q.question_id)})),
            ));
        }

        let display_order = q.display_order.unwrap_or(next_order);
        let result = sqlx::query(
            r#"
            INSERT INTO collection_questions (id, collection_id, question_id, question_no, display_order, section, score, metadata, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, '{}', NOW())
            ON CONFLICT (collection_id, question_id) DO NOTHING
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(id)
        .bind(q.question_id)
        .bind(&q.question_no)
        .bind(display_order)
        .bind(&q.section)
        .bind(q.score)
        .execute(&mut *tx)
        .await
        .map_err(|e| db_err(format!("添加题目到集合失败: {e}")))?;

        if result.rows_affected() > 0 {
            inserted += 1;
            if q.display_order.is_none() {
                next_order += 1;
            }
        } else {
            skipped += 1;
        }
    }

    tx.commit()
        .await
        .map_err(|e| db_err(format!("提交事务失败: {e}")))?;

    Ok((
        StatusCode::CREATED,
        Json(json!({ "inserted": inserted, "skipped": skipped })),
    ))
}

/// DELETE /api/v1/collections/{id}/questions/{question_id} — 从集合移除题目
pub async fn remove_collection_question(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path((id, question_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let collection = load_collection(&state.pool, id).await?.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "集合不存在"})),
        )
    })?;
    if !can_manage(&collection, &auth) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "集合不存在"})),
        ));
    }

    let result = sqlx::query(
        "DELETE FROM collection_questions WHERE collection_id = $1 AND question_id = $2",
    )
    .bind(id)
    .bind(question_id)
    .execute(&state.pool)
    .await
    .map_err(|e| db_err(format!("移除集合题目失败: {e}")))?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "集合中不存在该题目"})),
        ));
    }

    Ok(Json(json!({ "message": "已移除" })))
}

// ---------------------------------------------------------------------------
// Worker 阶段 2 专用：幂等创建/复用集合
// ---------------------------------------------------------------------------

/// 按 (document_id, title) 幂等创建/复用集合（计划书 §6.1）
///
/// 供 Worker 使用：同一文档重跑时复用已建集合，不重复创建；
/// 跨文档同名资料一律新建（不在此函数内做跨文档复用）。
pub(crate) async fn get_or_create_collection(
    pool: &sqlx::PgPool,
    creator_id: Uuid,
    document_id: Uuid,
    title: &str,
    collection_type: &str,
    type_label: Option<&str>,
    source_type: Option<&str>,
    subject: Option<&str>,
    stage: Option<&str>,
    grade: Option<&str>,
    semester: Option<&str>,
    chapter_id: Option<Uuid>,
) -> Result<QuestionCollection, String> {
    if !is_valid_collection_type(collection_type) {
        return Err(format!("未知集合类型: {collection_type}"));
    }

    // 复用键：同文档同标题
    if let Some(existing) = sqlx::query_as::<_, QuestionCollection>(&format!(
        "SELECT {COLLECTION_COLUMNS} FROM question_collections WHERE document_id = $1 AND title = $2"
    ))
    .bind(document_id)
    .bind(title)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("查询集合失败: {e}"))?
    {
        return Ok(existing);
    }

    let id = Uuid::new_v4();
    sqlx::query_as::<_, QuestionCollection>(&format!(
        r#"
        INSERT INTO question_collections (id, document_id, creator_id, title, collection_type,
            type_label, source_type, subject, stage, grade, semester, chapter_id, metadata, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, '{{}}', NOW(), NOW())
        ON CONFLICT (document_id, title) DO UPDATE SET updated_at = NOW()
        RETURNING {COLLECTION_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(document_id)
    .bind(creator_id)
    .bind(title)
    .bind(collection_type)
    .bind(type_label)
    .bind(source_type)
    .bind(subject)
    .bind(stage)
    .bind(grade)
    .bind(semester)
    .bind(chapter_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("创建集合失败: {e}"))
}

/// 校验题目 hash 命中后，按集合添加题目（Worker 阶段 3 用，幂等）
pub(crate) async fn link_question_to_collection(
    pool: &sqlx::PgPool,
    collection_id: Uuid,
    question_id: Uuid,
    question_no: Option<&str>,
    display_order: i32,
    score: Option<i32>,
    section: Option<&str>,
) -> Result<bool, String> {
    let result = sqlx::query(
        r#"
        INSERT INTO collection_questions (id, collection_id, question_id, question_no, display_order, section, score, metadata, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, '{}', NOW())
        ON CONFLICT (collection_id, question_id) DO NOTHING
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(collection_id)
    .bind(question_id)
    .bind(question_no)
    .bind(display_order)
    .bind(section)
    .bind(score)
    .execute(pool)
    .await
    .map_err(|e| format!("关联集合题目失败: {e}"))?;

    Ok(result.rows_affected() > 0)
}
