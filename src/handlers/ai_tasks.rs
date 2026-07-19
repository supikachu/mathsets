use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::auth::permissions::is_admin;
use crate::models::ai_task::{AiParseTask, AiTaskStatus};
use crate::AppState;

// ---------------------------------------------------------------------------
// 请求 / 响应类型
// ---------------------------------------------------------------------------

/// 提交解析任务请求体
#[derive(Deserialize)]
pub struct SubmitParseRequest {
    /// 前端传来的 OCR 原始生肉文本
    pub raw_text: String,
}

/// 提交任务后的 202 响应体
#[derive(Serialize)]
pub struct SubmitParseResponse {
    pub task_id: Uuid,
    pub status: AiTaskStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// 任务详情查询响应体
#[derive(Serialize)]
pub struct TaskStatusResponse {
    pub id: Uuid,
    pub status: AiTaskStatus,
    pub question_id: Option<Uuid>,
    pub error_message: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<AiParseTask> for TaskStatusResponse {
    fn from(t: AiParseTask) -> Self {
        Self {
            id: t.id,
            status: t.status,
            question_id: t.question_id,
            error_message: t.error_message,
            created_at: t.created_at,
            updated_at: t.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// 辅助函数
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

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/ai/parse
///
/// 提交一个 AI 解析任务到队列。任务以 `pending` 状态入队，等待后台 worker 拾取。
/// 返回 202 Accepted + 任务 ID，客户端可凭 ID 轮询 `/ai/parse/:id` 获取结果。
pub async fn submit_parse_task(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Json(req): Json<SubmitParseRequest>,
) -> Result<(StatusCode, Json<SubmitParseResponse>), (StatusCode, Json<serde_json::Value>)> {
    // 1. 输入校验：拒绝空文本
    let raw_text = req.raw_text.trim().to_string();
    if raw_text.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "raw_text 不能为空"})),
        ));
    }

    // 2. 插入任务记录（status 默认为 pending）
    let task: AiParseTask = sqlx::query_as::<_, AiParseTask>(
        r#"
        INSERT INTO ai_parse_tasks (id, creator_id, raw_text, status, created_at, updated_at)
        VALUES (gen_random_uuid(), $1, $2, 'pending', NOW(), NOW())
        RETURNING id, creator_id, raw_text, status, question_id, error_message, created_at, updated_at
        "#,
    )
    .bind(auth.id)
    .bind(&raw_text)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| db_err(format!("创建 AI 解析任务失败: {}", e)))?;

    Ok((
        StatusCode::ACCEPTED,
        Json(SubmitParseResponse {
            task_id: task.id,
            status: task.status,
            created_at: task.created_at,
        }),
    ))
}

/// GET /api/v1/ai/parse/:id
///
/// 查询任务详情。仅允许任务创建者或管理员查看，否则返回 404（不泄露任务存在性）。
pub async fn get_task_status(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<TaskStatusResponse>, (StatusCode, Json<serde_json::Value>)> {
    // 1. 查询任务
    let task: AiParseTask = sqlx::query_as::<_, AiParseTask>(
        r#"
        SELECT id, creator_id, raw_text, status, question_id, error_message, created_at, updated_at
        FROM ai_parse_tasks
        WHERE id = $1
        "#,
    )
    .bind(task_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| db_err(format!("查询 AI 解析任务失败: {}", e)))?
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "任务不存在"})),
        )
    })?;

    // 2. 权限校验：仅创建者或管理员可见
    //    （无权限统一返回 404，避免泄露任务存在性）
    if task.creator_id != auth.id && !is_admin(&auth.role) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "任务不存在"})),
        ));
    }

    Ok(Json(TaskStatusResponse::from(task)))
}
