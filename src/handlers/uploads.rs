// ============================================================
// 通用图片上传 Handler 模块
// ------------------------------------------------------------
// 提供：
//   POST /api/v1/uploads/images — 上传题目配图等通用图片
//
// 设计差异（对比 /users/avatar）：
//   - 题目配图常含坐标系/几何大图，限制放宽至 10 MB
//   - 无 DB 关联，不绑定到具体用户记录（题目图片可在多题间复用）
//   - 不自动清理旧文件（无单一所有者，由独立 GC 机制处理）
//   - 复用 users.rs 的零信任校验：Magic Bytes + MIME 白名单
// ============================================================

use axum::{
    extract::{Extension, Multipart, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::handlers::users::{validate_image_type, ALLOWED_MIME_TYPES};
use crate::AppState;

/// 题目配图文件最大字节数：10 MB（几何大图/坐标系需更高上限）
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// Multipart 表单字段名（前端必须用这个名字）
const IMAGE_FIELD_NAME: &str = "image";

/// 题目图片 URL 前缀（与 lib.rs 中 ServeDir 挂载点保持一致）
const IMAGE_URL_PREFIX: &str = "/uploads/questions";

/// 图片上传成功响应
#[derive(Debug, Serialize)]
pub struct ImageUploadResponse {
    pub url: String,
}

/// POST /api/v1/uploads/images — 上传题目配图等通用图片
///
/// 流程：
///   1. 解析 multipart，提取 `image` 字段二进制
///   2. 零信任校验：大小 ≤ 10MB + MIME 白名单 + Magic Bytes
///   3. 生成安全文件名：`{user_id}_{timestamp_ms}_{rand8}.{ext}`
///   4. 写入 `{upload_dir}/questions/{filename}`
///   5. 返回持久化 URL（如 `/uploads/questions/xxx.png`）
///
/// 不做的事：
///   - 不写入 DB（图片可被多题复用，无单一所有者）
///   - 不清理旧文件（无 GC 上下文，留待后续定期清理任务）
pub async fn upload_image(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<ImageUploadResponse>), (StatusCode, Json<serde_json::Value>)> {
    // 1. 解析 multipart — 提取 image 字段
    let mut image_bytes: Option<Vec<u8>> = None;
    let mut client_mime: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| {
            tracing::error!("upload_image multipart 解析失败: {:?}", e);
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("multipart 解析失败: {}", e)})),
            )
        })?
    {
        let name = field.name().unwrap_or("").to_string();
        if name != IMAGE_FIELD_NAME {
            continue; // 忽略未知字段
        }

        client_mime = field.content_type().map(|s| s.to_string());
        let bytes = field
            .bytes()
            .await
            .map_err(|e| {
                tracing::error!("upload_image 读取字段字节失败: {:?}", e);
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": format!("读取文件数据失败: {}", e)})),
                )
            })?
            .to_vec();
        image_bytes = Some(bytes);
        break;
    }

    let image_bytes = image_bytes.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("未接收到 {} 字段", IMAGE_FIELD_NAME)})),
        )
    })?;

    // 2. 大小校验
    if image_bytes.len() > MAX_IMAGE_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({
                "error": format!("图片文件过大，最大允许 {} 字节", MAX_IMAGE_BYTES),
                "limit": MAX_IMAGE_BYTES,
            })),
        ));
    }

    // 3. 零信任类型校验：MIME 白名单 + Magic Bytes
    let ext = validate_image_type(&image_bytes, client_mime.as_deref()).ok_or_else(|| {
        (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(json!({
                "error": "仅支持 jpg / png / webp 格式",
                "allowed": ALLOWED_MIME_TYPES,
            })),
        )
    })?;

    // 4. 生成安全文件名 — 不使用客户端原文件名（防路径穿越）
    let timestamp = chrono::Utc::now().timestamp_millis();
    let rand_suffix = Uuid::new_v4().simple().to_string();
    let rand_suffix = &rand_suffix[..8];
    let filename = format!("{}_{}_{}.{}", auth.id, timestamp, rand_suffix, ext);

    // 5. 写入文件系统
    let upload_dir = PathBuf::from(&state.upload_dir);
    let questions_dir = upload_dir.join("questions");
    let file_path = questions_dir.join(&filename);

    if let Err(e) = tokio::fs::create_dir_all(&questions_dir).await {
        tracing::error!("upload_image 创建目录失败 {:?}: {:?}", questions_dir, e);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("创建目录失败: {}", e)})),
        ));
    }

    if let Err(e) = tokio::fs::write(&file_path, &image_bytes).await {
        tracing::error!("upload_image 写入文件失败 {:?}: {:?}", file_path, e);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("写入文件失败: {}", e)})),
        ));
    }

    let url = format!("{}/{}", IMAGE_URL_PREFIX, filename);

    tracing::info!(
        "图片上传成功: user_id={}, url={}, size={}bytes",
        auth.id,
        url,
        image_bytes.len()
    );

    Ok((StatusCode::OK, Json(ImageUploadResponse { url })))
}
