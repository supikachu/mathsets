//! V2.1.1 P0-C：AI 解析任务 API
//!
//! - `POST /ai/parse-task`：按已确认 Document 创建解析任务（1:N；存在未终态任务 → 409）
//! - `GET /ai/parse-task/{id}`：任务进度（计数/当前页/结果关联）
//! - `POST /ai/parse-task/{id}/cancel`：取消（已落库题目保留，计划书 §6.4）

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::auth::permissions::is_admin_user;
use crate::models::ai_task::{AiParseTask, AiTaskStatus, TaskStatusResponse};
use crate::AppState;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 每日解析任务配额（原 parse-image 额度迁移至此）
const DAILY_TASK_QUOTA: i64 = 50;

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

const TASK_COLUMNS: &str = "id, creator_id, raw_text, status, question_id, error_message, \
     created_at, updated_at, document_id, paper_meta, total_count, processed_count, \
     success_count, failed_count, retry_count, current_page, total_pages, \
     current_question_no, started_at, completed_at, last_error, progress, \
     locked_at, worker_id, heartbeat_at, cancel_requested_at";

async fn load_task(
    pool: &sqlx::PgPool,
    task_id: Uuid,
) -> Result<Option<AiParseTask>, (StatusCode, Json<serde_json::Value>)> {
    sqlx::query_as::<_, AiParseTask>(&format!(
        "SELECT {TASK_COLUMNS} FROM ai_parse_tasks WHERE id = $1"
    ))
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err(format!("查询任务失败: {e}")))
}

fn can_manage_task(task: &AiParseTask, auth: &AuthUser) -> bool {
    task.creator_id == auth.id || is_admin_user(auth)
}

/// 任务产出题目 ID（按 progress.idempotency_map 键排序，键如 p1_i2）
fn task_question_ids(task: &AiParseTask) -> Vec<Uuid> {
    let mut pairs: Vec<(String, Uuid)> = task
        .progress
        .get("idempotency_map")
        .and_then(|m| m.as_object())
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| {
                    v.as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .map(|id| (k.clone(), id))
                })
                .collect()
        })
        .unwrap_or_default();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs.into_iter().map(|(_, id)| id).collect()
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/ai/parse-task — 创建解析任务
#[derive(Debug, Deserialize)]
pub struct SubmitParseTaskRequest {
    pub document_id: Uuid,
}

pub async fn submit_parse_task(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Json(req): Json<SubmitParseTaskRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    // 1. Document 必须存在且已确认
    let doc = sqlx::query_as::<_, (Uuid, String, Option<String>, serde_json::Value)>(
        "SELECT id, status, document_type, metadata FROM documents WHERE id = $1",
    )
    .bind(req.document_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| db_err(format!("查询 Document 失败: {e}")))?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "资料不存在"}))))?;

    let (doc_id, doc_status, doc_type, doc_metadata) = doc;
    if doc_status != "confirmed" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "资料尚未确认类型，请先完成资料类型确认",
                "code": "ERR_DOCUMENT_NOT_CONFIRMED"
            })),
        ));
    }
    // 文档归属校验（管理员可代跑）
    let doc_creator: Uuid = sqlx::query_scalar("SELECT creator_id FROM documents WHERE id = $1")
        .bind(doc_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询 Document 归属失败: {e}")))?;
    if doc_creator != auth.id && !is_admin_user(&auth) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "资料不存在"})),
        ));
    }

    // 2. 幂等：同 Document 存在未终态任务 → 409（不静默复用）
    let existing: Option<Uuid> = sqlx::query_scalar(
        r#"
        SELECT id FROM ai_parse_tasks
        WHERE document_id = $1 AND status IN ('pending', 'processing', 'retrying')
        LIMIT 1
        "#,
    )
    .bind(doc_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| db_err(format!("查询进行中任务失败: {e}")))?;
    if let Some(task_id) = existing {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "该资料已有进行中的解析任务",
                "code": "ERR_TASK_ACTIVE",
                "existing_task_id": task_id
            })),
        ));
    }

    // 3. 配额：日 50 次（原子抢占，防 TOCTOU）
    let quota_ok = sqlx::query(
        r#"
        INSERT INTO ai_usage_log (user_id, endpoint, created_at)
        SELECT $1, $2, NOW()
        WHERE (SELECT COUNT(*) FROM ai_usage_log WHERE user_id = $1 AND created_at >= CURRENT_DATE) < $3
        RETURNING id
        "#,
    )
    .bind(auth.id)
    .bind("parse_task")
    .bind(DAILY_TASK_QUOTA)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| db_err(format!("配额校验失败: {e}")))?
    .is_some();

    if !quota_ok {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "今日解析任务额度已耗尽",
                "code": "ERR_QUOTA_EXCEEDED"
            })),
        ));
    }

    // 4. paper_meta 输入快照：从 documents.metadata 复制（计划书 §六 输入快照原则）
    let paper_meta_snapshot = json!({
        "document_type": doc_type,
        "title": doc_metadata.get("title").cloned().unwrap_or(json!(null)),
        "paper_meta": doc_metadata.get("paper_meta").cloned().unwrap_or(json!(null)),
        "collections": doc_metadata.get("collections").cloned().unwrap_or(json!([])),
    });

    let page_count: i32 = sqlx::query_scalar("SELECT page_count FROM documents WHERE id = $1")
        .bind(doc_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询页数失败: {e}")))?;

    // 5. 创建任务
    let task_id = Uuid::new_v4();
    let task: AiParseTask = sqlx::query_as::<_, AiParseTask>(&format!(
        r#"
        INSERT INTO ai_parse_tasks (id, creator_id, raw_text, status, created_at, updated_at,
            document_id, paper_meta, total_pages, progress)
        VALUES ($1, $2, '', 'pending', NOW(), NOW(), $3, $4, $5, '{{"idempotency_map": {{}}}}')
        RETURNING {TASK_COLUMNS}
        "#
    ))
    .bind(task_id)
    .bind(auth.id)
    .bind(doc_id)
    .bind(&paper_meta_snapshot)
    .bind(page_count)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| db_err(format!("创建解析任务失败: {e}")))?;

    tracing::info!(
        "用户 {} 创建解析任务 {}（document={}）",
        auth.id,
        task_id,
        doc_id
    );

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "task_id": task_id,
            "status": task.status,
            "created_at": task.created_at
        })),
    ))
}

/// GET /api/v1/ai/parse-task/{id} — 任务进度与结果
pub async fn get_task_status(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<TaskStatusResponse>, (StatusCode, Json<serde_json::Value>)> {
    let task = load_task(&state.pool, task_id).await?.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "任务不存在"})),
        )
    })?;
    if !can_manage_task(&task, &auth) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "任务不存在"})),
        ));
    }

    // 结果关联（懒查询）
    let paper_id: Option<Uuid> = match task.document_id {
        Some(doc_id) => sqlx::query_scalar(
            "SELECT id FROM papers WHERE document_id = $1 AND creator_id = $2 LIMIT 1",
        )
        .bind(doc_id)
        .bind(task.creator_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询试卷关联失败: {e}")))?,
        None => None,
    };
    let collection_ids: Vec<Uuid> = match task.document_id {
        Some(doc_id) => sqlx::query_scalar(
            "SELECT id FROM question_collections WHERE document_id = $1 ORDER BY created_at",
        )
        .bind(doc_id)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询集合关联失败: {e}")))?,
        None => vec![],
    };

    let question_ids = task_question_ids(&task);

    Ok(Json(TaskStatusResponse {
        id: task.id,
        status: task.status.to_view(),
        error_message: task.error_message.clone(),
        created_at: task.created_at,
        updated_at: task.updated_at,
        total_count: task.total_count,
        processed_count: task.processed_count,
        success_count: task.success_count,
        failed_count: task.failed_count,
        retry_count: task.retry_count,
        current_page: task.current_page,
        total_pages: task.total_pages,
        current_question_no: task.current_question_no,
        started_at: task.started_at,
        completed_at: task.completed_at,
        paper_id,
        collection_ids,
        question_ids,
    }))
}

/// POST /api/v1/ai/parse-task/{id}/cancel — 取消任务
///
/// 语义（计划书 §6.4）：置 cancel_requested_at；worker 题间检查后落 cancelled；
/// 已成功落库的题目全部保留，success_count 如实反映。
pub async fn cancel_task(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let task = load_task(&state.pool, task_id).await?.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "任务不存在"})),
        )
    })?;
    if !can_manage_task(&task, &auth) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "任务不存在"})),
        ));
    }

    if task.status.is_terminal() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "任务已结束，无法取消",
                "status": task.status.to_view()
            })),
        ));
    }

    sqlx::query(
        "UPDATE ai_parse_tasks SET cancel_requested_at = NOW(), updated_at = NOW() WHERE id = $1",
    )
    .bind(task_id)
    .execute(&state.pool)
    .await
    .map_err(|e| db_err(format!("取消任务失败: {e}")))?;

    Ok(Json(json!({ "message": "已请求取消，正在停止解析" })))
}
