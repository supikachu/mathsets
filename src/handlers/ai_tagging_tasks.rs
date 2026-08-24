//! 编辑页异步打标任务 API
//!
//! - `POST /questions/ai-tagging-tasks` → 202 + task id（进行中任务幂等复用，不重复扣配额）
//! - `GET /questions/ai-tagging-tasks/{id}` → 状态 + suggestion（不含题文）
//! - `POST /questions/ai-tagging-tasks/{id}/cancel`
//! - `POST /ai/parse-task/{id}/start-tagging` → 为暂存题入队打标（站外结构化需用户点击）
//! - `POST /ai/parse-task/{id}/cancel-tagging` → 批量终止某解析任务下未完成的打标任务

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::ai::tagging::{content_input_hash_with_stage, TaggingSuggestion};
use crate::auth::middleware::AuthUser;
use crate::auth::permissions::{can_access_space, get_space, is_admin_user};
use crate::handlers::ai_tagging::{legacy_response, AiTaggingRequest, AiTaggingResponse};
use crate::models::ai_tagging_task::{AiTaggingTask, AiTaggingTaskStatus, TAGGING_TASK_COLUMNS};
use crate::models::user::try_consume_quota;
use crate::AppState;

const MAX_CONTENT_CHARS: usize = 200_000;
const NIL_SPACE: Uuid = Uuid::nil();

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

fn quota_exceeded() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": "今日 AI 额度已耗尽",
            "code": "ERR_QUOTA_EXCEEDED"
        })),
    )
}

fn can_manage(task: &AiTaggingTask, auth: &AuthUser) -> bool {
    task.creator_id == auth.id || is_admin_user(auth)
}

async fn load_task(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> Result<Option<AiTaggingTask>, (StatusCode, Json<serde_json::Value>)> {
    sqlx::query_as::<_, AiTaggingTask>(&format!(
        "SELECT {TAGGING_TASK_COLUMNS} FROM ai_tagging_tasks WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err(format!("查询打标任务失败: {e}")))
}

async fn find_inflight(
    pool: &sqlx::PgPool,
    creator_id: Uuid,
    space_id: Option<Uuid>,
    input_hash: &str,
) -> Result<Option<(Uuid, String)>, (StatusCode, Json<serde_json::Value>)> {
    sqlx::query_as::<_, (Uuid, String)>(
        r#"
        SELECT id, status FROM ai_tagging_tasks
        WHERE creator_id = $1
          AND COALESCE(space_id, $4) = COALESCE($2, $4)
          AND input_hash = $3
          AND status IN ('pending', 'processing', 'retrying')
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(creator_id)
    .bind(space_id)
    .bind(input_hash)
    .bind(NIL_SPACE)
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err(format!("查询进行中打标任务失败: {e}")))
}

async fn ensure_space_access(
    state: &AppState,
    auth: &AuthUser,
    space_id: Option<Uuid>,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let Some(space_id) = space_id else {
        return Ok(());
    };
    let space = get_space(&state.pool, space_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询空间失败: {e}")})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;
    if !can_access_space(&state.pool, auth, &space)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("权限检查失败: {e}")})),
            )
        })?
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "无权访问该空间"})),
        ));
    }
    Ok(())
}

async fn suggestion_response(
    pool: &sqlx::PgPool,
    suggestion_id: Option<Uuid>,
) -> Option<AiTaggingResponse> {
    let sid = suggestion_id?;
    let result: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT result FROM ai_tagging_suggestions WHERE id = $1")
            .bind(sid)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    let result = result?;
    match serde_json::from_value::<TaggingSuggestion>(result) {
        Ok(mut s) => {
            if s.suggestion_id.is_none() {
                s.suggestion_id = Some(sid);
            }
            Some(legacy_response(s))
        }
        Err(e) => {
            tracing::warn!("打标建议反序列化失败 suggestion_id={sid}: {e}");
            None
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CreateTaskResponse {
    pub id: Uuid,
    pub status: String,
    pub reused: bool,
}

/// POST /api/v1/questions/ai-tagging-tasks
pub async fn create_tagging_task(
    Extension(auth): Extension<AuthUser>,
    State(state): State<AppState>,
    Json(req): Json<AiTaggingRequest>,
) -> Result<(StatusCode, Json<CreateTaskResponse>), (StatusCode, Json<serde_json::Value>)> {
    let content = req.content.trim().to_string();
    if content.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "题目文本不能为空"})),
        ));
    }
    if content.chars().count() > MAX_CONTENT_CHARS {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "题目文本过长"})),
        ));
    }

    ensure_space_access(&state, &auth, req.space_id).await?;

    let stage = req
        .stage
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase());
    let input_hash = content_input_hash_with_stage(&content, stage.as_deref());

    if let Some((id, status)) = find_inflight(&state.pool, auth.id, req.space_id, &input_hash).await?
    {
        return Ok((
            StatusCode::ACCEPTED,
            Json(CreateTaskResponse {
                id,
                status,
                reused: true,
            }),
        ));
    }

    let quota_ok = try_consume_quota(&state.pool, auth.id, "tagging_task")
        .await
        .map_err(|e| db_err(format!("配额校验失败: {e}")))?;
    if !quota_ok {
        return Err(quota_exceeded());
    }

    let inserted: Result<(Uuid, String), sqlx::Error> = sqlx::query_as(
        r#"
        INSERT INTO ai_tagging_tasks (creator_id, space_id, question_id, input_hash, content, stage, status)
        VALUES ($1, $2, $3, $4, $5, $6, 'pending')
        RETURNING id, status
        "#,
    )
    .bind(auth.id)
    .bind(req.space_id)
    .bind(req.question_id)
    .bind(&input_hash)
    .bind(&content)
    .bind(&stage)
    .fetch_one(&state.pool)
    .await;

    match inserted {
        Ok((id, status)) => Ok((
            StatusCode::ACCEPTED,
            Json(CreateTaskResponse {
                id,
                status,
                reused: false,
            }),
        )),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") || msg.contains("idx_ai_tagging_tasks_inflight")
            {
                if let Some((id, status)) =
                    find_inflight(&state.pool, auth.id, req.space_id, &input_hash).await?
                {
                    return Ok((
                        StatusCode::ACCEPTED,
                        Json(CreateTaskResponse {
                            id,
                            status,
                            reused: true,
                        }),
                    ));
                }
            }
            Err(db_err(format!("创建打标任务失败: {e}")))
        }
    }
}

/// GET /api/v1/questions/ai-tagging-tasks/{id}
pub async fn get_tagging_task(
    Extension(auth): Extension<AuthUser>,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let task = load_task(&state.pool, id).await?.ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(json!({"error": "打标任务不存在"})))
    })?;
    if !can_manage(&task, &auth) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "无权查看该打标任务"})),
        ));
    }

    let suggestion = suggestion_response(&state.pool, task.suggestion_id).await;
    let cancelling = task.cancel_requested_at.is_some()
        && !AiTaggingTaskStatus::from_db(&task.status).is_terminal();

    Ok(Json(json!({
        "id": task.id,
        "status": task.status,
        "retry_count": task.retry_count,
        "error_message": task.error_message,
        "suggestion_id": task.suggestion_id,
        "suggestion": suggestion,
        "cancelling": cancelling,
        "created_at": task.created_at,
        "started_at": task.started_at,
        "completed_at": task.completed_at,
        "updated_at": task.updated_at,
    })))
}

/// POST /api/v1/questions/ai-tagging-tasks/{id}/cancel
pub async fn cancel_tagging_task(
    Extension(auth): Extension<AuthUser>,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let task = load_task(&state.pool, id).await?.ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(json!({"error": "打标任务不存在"})))
    })?;
    if !can_manage(&task, &auth) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "无权取消该打标任务"})),
        ));
    }

    let status = AiTaggingTaskStatus::from_db(&task.status);
    if status.is_terminal() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": format!("任务已结束（status={}）", task.status)})),
        ));
    }

    if matches!(status, AiTaggingTaskStatus::Pending | AiTaggingTaskStatus::Retrying) {
        sqlx::query(
            r#"
            UPDATE ai_tagging_tasks
            SET status = 'cancelled', completed_at = NOW(), updated_at = NOW(),
                locked_at = NULL, worker_id = NULL
            WHERE id = $1 AND status IN ('pending', 'retrying')
            "#,
        )
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| db_err(format!("取消打标任务失败: {e}")))?;
        return Ok(Json(json!({ "id": id, "status": "cancelled" })));
    }

    sqlx::query(
        r#"
        UPDATE ai_tagging_tasks
        SET cancel_requested_at = COALESCE(cancel_requested_at, NOW()), updated_at = NOW()
        WHERE id = $1 AND status = 'processing'
        "#,
    )
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(|e| db_err(format!("请求取消打标任务失败: {e}")))?;

    Ok(Json(json!({ "id": id, "status": "cancelling" })))
}

/// POST /api/v1/ai/parse-task/{id}/start-tagging
///
/// 站外结构化导入后题目为 idle，需用户点击才入队。全自动路径一般已是 pending/done，此时为 0。
pub async fn start_parse_tagging_tasks(
    Extension(auth): Extension<AuthUser>,
    State(state): State<AppState>,
    Path(parse_task_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT creator_id FROM ai_parse_tasks WHERE id = $1",
    )
    .bind(parse_task_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| db_err(format!("查询解析任务失败: {e}")))?;
    let Some((creator_id,)) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "解析任务不存在"})),
        ));
    };
    if creator_id != auth.id && !is_admin_user(&auth) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "无权启动该任务的打标"})),
        ));
    }

    let (started, skipped) = crate::workers::ai_parse_worker::start_staged_tagging(&state, parse_task_id)
        .await
        .map_err(|e| db_err(e))?;

    let message = if started == 0 {
        "没有待打标的题目".to_string()
    } else {
        format!("已开始打标 {started} 道题")
    };

    Ok(Json(json!({
        "started": started,
        "skipped": skipped,
        "message": message,
    })))
}

/// POST /api/v1/ai/parse-task/{id}/cancel-tagging
///
/// 用户停止打标、离开录入，或题目已全部确认保存后调用。
/// pending 立即终止；processing 打取消标记由 worker 收敛。
/// 未保存暂存项的 tagging_status 从 pending 回到 idle，便于再次开始。
pub async fn cancel_parse_tagging_tasks(
    Extension(auth): Extension<AuthUser>,
    State(state): State<AppState>,
    Path(parse_task_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let is_admin = is_admin_user(&auth);

    let owner: Option<Uuid> = sqlx::query_scalar(
        "SELECT creator_id FROM ai_parse_tasks WHERE id = $1",
    )
    .bind(parse_task_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| db_err(format!("查询解析任务失败: {e}")))?;
    let can_manage_parse = owner
        .map(|id| id == auth.id || is_admin)
        .unwrap_or(false);

    let cancelled = sqlx::query(
        r#"
        UPDATE ai_tagging_tasks
        SET status = 'cancelled',
            error_message = COALESCE(error_message, '打标任务已终止'),
            completed_at = NOW(), updated_at = NOW(),
            locked_at = NULL, worker_id = NULL
        WHERE parse_task_id = $1
          AND (creator_id = $2 OR $3::boolean)
          AND status IN ('pending', 'retrying')
        "#,
    )
    .bind(parse_task_id)
    .bind(auth.id)
    .bind(is_admin)
    .execute(&state.pool)
    .await
    .map_err(|e| db_err(format!("取消解析打标任务失败: {e}")))?
    .rows_affected();

    // processing 只能打标记，由 worker 在下一次轮询时收敛为 cancelled
    let cancelling = sqlx::query(
        r#"
        UPDATE ai_tagging_tasks
        SET cancel_requested_at = NOW(), updated_at = NOW()
        WHERE parse_task_id = $1
          AND (creator_id = $2 OR $3::boolean)
          AND status = 'processing'
          AND cancel_requested_at IS NULL
        "#,
    )
    .bind(parse_task_id)
    .bind(auth.id)
    .bind(is_admin)
    .execute(&state.pool)
    .await
    .map_err(|e| db_err(format!("请求取消解析打标任务失败: {e}")))?
    .rows_affected();

    if cancelled > 0 || cancelling > 0 {
        tracing::info!(
            parse_task_id = %parse_task_id,
            cancelled,
            cancelling,
            "终止该解析任务下未完成的打标任务"
        );
    }

    if can_manage_parse {
        if let Err(e) = crate::workers::ai_parse_worker::reset_pending_staged_tagging(
            &state.pool,
            parse_task_id,
        )
        .await
        {
            tracing::warn!(
                parse_task_id = %parse_task_id,
                "重置暂存打标状态失败: {e}"
            );
        }
    }

    Ok(Json(json!({
        "cancelled": cancelled,
        "cancelling": cancelling,
    })))
}
