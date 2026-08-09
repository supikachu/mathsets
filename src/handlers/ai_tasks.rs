use axum::{
    extract::{Extension, Multipart, Path, State},
    http::StatusCode,
    Json,
};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::auth::permissions::is_admin_user;
use crate::models::ai_task::{AiParseTask, AiTaskSourceType, AiTaskStatus};
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
    /// 任务来源类型（text/image/pdf），前端据此决定加载单题还是多题
    pub source_type: AiTaskSourceType,
    pub status: AiTaskStatus,
    /// 首题 ID（向后兼容旧前端）
    pub question_id: Option<Uuid>,
    /// M4：所有生成题目的 UUID 数组（多题批处理场景，image/pdf 任务优先读取此字段）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question_ids: Option<serde_json::Value>,
    pub error_message: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<AiParseTask> for TaskStatusResponse {
    fn from(t: AiParseTask) -> Self {
        Self {
            id: t.id,
            source_type: t.source_type,
            status: t.status,
            question_id: t.question_id,
            question_ids: t.question_ids,
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
        SELECT id, creator_id, raw_text, source_type, image_b64, pdf_bytes,
               ocr_provider_override, status, question_id, question_ids,
               error_message, created_at, updated_at
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
    if task.creator_id != auth.id && !is_admin_user(&auth) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "任务不存在"})),
        ));
    }

    Ok(Json(TaskStatusResponse::from(task)))
}

// ---------------------------------------------------------------------------
// M4：图片 / PDF 异步任务提交（Multipart）
// ---------------------------------------------------------------------------

/// POST /api/v1/ai/parse-task
///
/// 提交图片或 PDF 异步解析任务（Multipart）。
/// - 字段 `image` 或 `file`：二进制文件（JPEG/PNG/WebP/PDF）
/// - 字段 `ocr_provider`：可选引擎覆盖（doc2x / mineru_local / qwen_vl）
///
/// 通过 Magic Number 零信任校验自动判定 source_type：
/// - JPEG/PNG/WebP → source_type=image，存 base64 到 image_b64
/// - PDF           → source_type=pdf，存原始字节到 pdf_bytes
///
/// 任务以 `pending` 状态入队，返回 202 + 任务 ID。
/// 客户端凭 ID 轮询 `GET /ai/parse/:id` 获取结果（`question_ids` 字段含全部生成题目）。
pub async fn submit_parse_task_media(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<SubmitParseResponse>), (StatusCode, Json<serde_json::Value>)> {
    // 1. 流式读取 multipart 字段
    let mut file_bytes: Vec<u8> = Vec::new();
    let mut ocr_provider: Option<String> = None;
    let mut magic_checked = false;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Multipart 解析失败: {e}")})),
            )
        })?
    {
        match field.name() {
            Some("image") | Some("file") => {
                // 流式分块读取，避免一次性 bytes().await 导致大内存峰值
                loop {
                    let chunk = field.chunk().await.map_err(|e| {
                        (
                            StatusCode::BAD_REQUEST,
                            Json(json!({"error": format!("读取流失败: {e}")})),
                        )
                    })?;
                    match chunk {
                        Some(bytes) => {
                            file_bytes.extend_from_slice(&bytes);

                            // 第一个块累计满 12 字节时进行 Magic Number 零信任校验
                            if !magic_checked && file_bytes.len() >= 12 {
                                let kind = infer::get(&file_bytes[..12]);
                                let is_valid = match kind {
                                    Some(t) => {
                                        t.mime_type() == "image/jpeg"
                                            || t.mime_type() == "image/png"
                                            || t.mime_type() == "image/webp"
                                            || t.mime_type() == "application/pdf"
                                    }
                                    None => false,
                                };
                                if !is_valid {
                                    return Err((
                                        StatusCode::BAD_REQUEST,
                                        Json(json!({
                                            "error": "非法的文件格式，仅支持 JPEG/PNG/WebP 图片或 PDF"
                                        })),
                                    ));
                                }
                                magic_checked = true;
                            }
                        }
                        None => break,
                    }
                }
            }
            Some("ocr_provider") => {
                if let Ok(text) = field.text().await {
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        ocr_provider = Some(text);
                    }
                }
            }
            _ => {
                // 忽略未知字段
            }
        }
    }

    if file_bytes.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "未接收到文件数据"})),
        ));
    }

    // 2. 根据 Magic Number 判定 source_type，分别填充 image_b64 / pdf_bytes
    let kind = infer::get(&file_bytes[..12]).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "无法识别的文件类型"})),
        )
    })?;

    let (source_type, image_b64, pdf_bytes): (AiTaskSourceType, Option<String>, Option<Vec<u8>>) =
        match kind.mime_type() {
            "image/jpeg" | "image/png" | "image/webp" => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&file_bytes);
                // 立即释放原始二进制，防止 base64 + 原始 bytes 同时驻留
                drop(file_bytes);
                (AiTaskSourceType::Image, Some(b64), None)
            }
            "application/pdf" => {
                (AiTaskSourceType::Pdf, None, Some(file_bytes))
            }
            other => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": format!("不支持的文件类型: {other}")})),
                ));
            }
        };

    tracing::info!(
        "用户 {} 提交异步 {source_type:?} 解析任务（ocr_provider={:?}）",
        auth.id,
        ocr_provider
    );

    // 3. 插入任务记录（status 默认为 pending）
    let task: AiParseTask = sqlx::query_as::<_, AiParseTask>(
        r#"
        INSERT INTO ai_parse_tasks (id, creator_id, raw_text, source_type,
                                    image_b64, pdf_bytes, ocr_provider_override,
                                    status, created_at, updated_at)
        VALUES (gen_random_uuid(), $1, NULL, $2,
                $3, $4, $5,
                'pending', NOW(), NOW())
        RETURNING id, creator_id, raw_text, source_type, image_b64, pdf_bytes,
                  ocr_provider_override, status, question_id, question_ids,
                  error_message, created_at, updated_at
        "#,
    )
    .bind(auth.id)
    .bind(source_type)
    .bind(&image_b64)
    .bind(&pdf_bytes)
    .bind(&ocr_provider)
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
