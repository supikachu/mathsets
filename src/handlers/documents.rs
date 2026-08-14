//! V2.1.1 P0-A：资料/Document 上传、分类（多级 fallback）、确认
//!
//! 对应计划书 P0-A（B1）：`POST /ai/documents`、`POST /ai/documents/{id}/classify`、
//! `POST /ai/documents/{id}/confirm`、`GET /ai/documents`、`GET /ai/documents/{id}`。
//!
//! 分类多级 fallback（计划书 §7.2）：
//!   Level 1 文件名（text 模型）→ 置信 <0.6 → Level 2 第 1 页图（vision）
//!   → <0.6 → Level 3 前 3 页图（vision 多图）→ <0.6 → unknown（前端强制用户选择）

use axum::{
    extract::{Extension, Multipart, Path, State},
    http::StatusCode,
    Json,
};
use base64::Engine as _;
use serde_json::json;
use uuid::Uuid;

use crate::ai::cleaner::clean_and_parse;
use crate::ai::prompt::{AI_CLASSIFY_FULL_PROMPT_TEXT, AI_CLASSIFY_FULL_PROMPT_VISION};
use crate::ai::provider::create_provider;
use crate::auth::middleware::AuthUser;
use crate::auth::permissions::is_admin_user;
use crate::handlers::ai::{map_ai_error, resolve_ai_config, ModelKind};
use crate::models::document::{
    is_valid_document_type, validate_confirm, AiClassification, AiClassificationRaw,
    ConfirmDocumentRequest, Document,
};
use crate::AppState;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 单页图片大小上限（与 parse-image 保持一致量级）
const MAX_PAGE_BYTES: usize = 10 * 1024 * 1024;
/// 文档页数上限（TD-1 前端 pdfjs 渲染同样限制 30 页）
const MAX_PAGES: usize = 30;

/// 分类置信度门槛（低于则升级检测层级；最终仍低于 → unknown）
const CLASSIFY_CONFIDENCE_THRESHOLD: f32 = 0.6;

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

/// 读取页面文件并 base64 编码（用于视觉分类）
async fn load_page_base64(upload_dir: &str, doc_id: Uuid, page_file: &str) -> Result<String, String> {
    let path = std::path::Path::new(upload_dir)
        .join("documents")
        .join(doc_id.to_string())
        .join(page_file);
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| format!("读取页面文件失败: {e}"))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}

/// 校验图片 magic number，返回扩展名（jpeg/png/webp）
fn detect_image_ext(bytes: &[u8]) -> Option<&'static str> {
    let kind = infer::get(bytes)?;
    match kind.mime_type() {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

/// 视觉模型最小尺寸限制（qwen-vl 等要求宽高均 > 10）
const MIN_IMAGE_DIMENSION: u32 = 10;

/// 解析图片宽高（PNG / JPEG / WebP，纯手写解析避免引入 image crate）
fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    // PNG：IHDR 位于固定偏移 16..24（big-endian width/height）
    if bytes.len() >= 24 && bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some((
            u32::from_be_bytes(bytes[16..20].try_into().ok()?),
            u32::from_be_bytes(bytes[20..24].try_into().ok()?),
        ));
    }
    // JPEG：扫描 SOF0/1/2 等标记
    if bytes.len() >= 4 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        let mut i = 2usize;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            // 无长度段的标记：SOI/EOI/RSTn/TEM
            if marker == 0xD8
                || marker == 0xD9
                || marker == 0x01
                || (0xD0..=0xD7).contains(&marker)
            {
                i += 2;
                continue;
            }
            if i + 3 >= bytes.len() {
                return None;
            }
            let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            if matches!(
                marker,
                0xC0 | 0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB | 0xCD
                    | 0xCE | 0xCF
            ) {
                if i + 8 >= bytes.len() {
                    return None;
                }
                let height = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                let width = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
                return Some((width, height));
            }
            i += 2 + len;
        }
        return None;
    }
    // WebP
    if bytes.len() >= 30 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        let chunk = &bytes[12..];
        if chunk.starts_with(b"VP8X") {
            // VP8X：canvas size 24..30（24-bit little-endian，+1）
            let w = (bytes[24] as u32) | ((bytes[25] as u32) << 8) | ((bytes[26] as u32) << 16);
            let h = (bytes[27] as u32) | ((bytes[28] as u32) << 8) | ((bytes[29] as u32) << 16);
            return Some((w + 1, h + 1));
        }
        if chunk.starts_with(b"VP8L") {
            // VP8L：21..25 位打包
            let (b0, b1, b2, b3) = (
                bytes[21] as u32,
                bytes[22] as u32,
                bytes[23] as u32,
                bytes[24] as u32,
            );
            let w = 1 + (((b1 & 0x3F) << 8) | b0);
            let h = 1 + (((b3 & 0x0F) << 10) | (b2 << 2) | ((b1 & 0xC0) >> 6));
            return Some((w, h));
        }
        if chunk.starts_with(b"VP8 ") {
            // VP8 有损：帧头 23..30（14-bit LE）
            let w = u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3FFF;
            let h = u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3FFF;
            return Some((w as u32, h as u32));
        }
    }
    None
}

/// 检查文档归属：本人或管理员
fn can_manage_document(doc: &Document, auth: &AuthUser) -> bool {
    doc.creator_id == auth.id || is_admin_user(auth)
}

async fn load_document(
    pool: &sqlx::PgPool,
    doc_id: Uuid,
) -> Result<Option<Document>, (StatusCode, Json<serde_json::Value>)> {
    sqlx::query_as::<_, Document>(
        r#"
        SELECT id, creator_id, file_name, file_size, mime, page_count,
               document_type, type_label, title, source_type, sub_source_type,
               status, ai_classification, metadata, conversion_engine,
               created_at, updated_at
        FROM documents WHERE id = $1
        "#,
    )
    .bind(doc_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err(format!("查询 Document 失败: {e}")))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/ai/documents — 上传资料页面图片集（PDF 由前端渲染为页图后上传）
///
/// multipart 字段：
/// - `pages`（重复）：页面图片（jpeg/png/webp，magic number 校验，单页 ≤10MB，≤30 页）
/// - `file_name`：原始文件名（可选）
/// - `file_type`：`image` | `pdf`（可选，pdf 时记录 conversion_engine='pdfjs'）
pub async fn upload_document(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let mut pages: Vec<(Vec<u8>, &'static str)> = Vec::new();
    let mut file_name: Option<String> = None;
    let mut file_type: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Multipart 解析失败: {e}")})),
            )
        })?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "pages" => {
                // 流式分块读取，单页大小熔断
                let mut bytes: Vec<u8> = Vec::new();
                let mut field = field;
                loop {
                    let chunk = field.chunk().await.map_err(|e| {
                        (
                            StatusCode::BAD_REQUEST,
                            Json(json!({"error": format!("读取页面数据失败: {e}")})),
                        )
                    })?;
                    match chunk {
                        Some(c) => {
                            bytes.extend_from_slice(&c);
                            if bytes.len() > MAX_PAGE_BYTES {
                                return Err((
                                    StatusCode::PAYLOAD_TOO_LARGE,
                                    Json(json!({"error": format!("单页图片不能超过 {}MB", MAX_PAGE_BYTES / 1024 / 1024)})),
                                ));
                            }
                        }
                        None => break,
                    }
                }
                if bytes.is_empty() {
                    return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "页面图片为空"}))));
                }
                let ext = detect_image_ext(&bytes).ok_or_else(|| {
                    (
                        StatusCode::UNSUPPORTED_MEDIA_TYPE,
                        Json(json!({"error": "非法的页面图片格式，仅支持 JPEG/PNG/WebP"})),
                    )
                })?;
                // 视觉模型尺寸限制：宽高必须 > 10（qwen-vl 上游 400 防御）
                if let Some((w, h)) = image_dimensions(&bytes) {
                    if w < MIN_IMAGE_DIMENSION || h < MIN_IMAGE_DIMENSION {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "error": format!(
                                    "页面图片尺寸过小（{w}x{h}），视觉模型要求宽高均大于 {}px",
                                    MIN_IMAGE_DIMENSION
                                )
                            })),
                        ));
                    }
                } else {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(json!({"error": "无法解析页面图片尺寸，请重新上传"})),
                    ));
                }
                pages.push((bytes, ext));
                if pages.len() > MAX_PAGES {
                    return Err((
                        StatusCode::PAYLOAD_TOO_LARGE,
                        Json(json!({"error": format!("文档页数不能超过 {} 页", MAX_PAGES)})),
                    ));
                }
            }
            "file_name" => {
                file_name = Some(
                    String::from_utf8_lossy(&field.bytes().await.map_err(|e| {
                        (
                            StatusCode::BAD_REQUEST,
                            Json(json!({"error": format!("读取 file_name 失败: {e}")})),
                        )
                    })?)
                    .trim()
                    .to_string(),
                );
            }
            "file_type" => {
                file_type = Some(
                    String::from_utf8_lossy(&field.bytes().await.map_err(|e| {
                        (
                            StatusCode::BAD_REQUEST,
                            Json(json!({"error": format!("读取 file_type 失败: {e}")})),
                        )
                    })?)
                    .trim()
                    .to_string(),
                );
            }
            _ => { /* 忽略未知字段，容错 */ }
        }
    }

    if pages.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "未接收到任何页面图片（pages 字段不能为空）"})),
        ));
    }

    let doc_id = Uuid::new_v4();
    let page_count = pages.len() as i32;
    let total_size: i64 = pages.iter().map(|(b, _)| b.len() as i64).sum();
    let file_name = file_name.unwrap_or_else(|| "未命名资料".to_string());
    let conversion_engine = if file_type.as_deref() == Some("pdf") {
        Some("pdfjs".to_string())
    } else {
        None
    };
    let mime = match pages[0].1 {
        "jpg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        _ => Some("image/webp"),
    };

    // 落盘：{upload_dir}/documents/{doc_id}/page_{n}.{ext}
    let dir = std::path::Path::new(&state.upload_dir)
        .join("documents")
        .join(doc_id.to_string());
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("创建文档目录失败: {e}")})),
            )
        })?;

    let mut page_files: Vec<String> = Vec::with_capacity(page_count as usize);
    for (i, (bytes, ext)) in pages.iter().enumerate() {
        let file_name = format!("page_{}.{}", i + 1, ext);
        if let Err(e) = tokio::fs::write(dir.join(&file_name), bytes).await {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("写入页面文件失败: {e}")})),
            ));
        }
        page_files.push(file_name);
    }

    let metadata = json!({ "pages": page_files });

    let doc: Document = sqlx::query_as::<_, Document>(
        r#"
        INSERT INTO documents (id, creator_id, file_name, file_size, mime, page_count,
            document_type, type_label, title, source_type, sub_source_type,
            status, ai_classification, metadata, conversion_engine, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, NULL, NULL, NULL, NULL, NULL,
            'uploaded', NULL, $7, $8, NOW(), NOW())
        RETURNING id, creator_id, file_name, file_size, mime, page_count,
            document_type, type_label, title, source_type, sub_source_type,
            status, ai_classification, metadata, conversion_engine, created_at, updated_at
        "#,
    )
    .bind(doc_id)
    .bind(auth.id)
    .bind(&file_name)
    .bind(total_size)
    .bind(mime)
    .bind(page_count)
    .bind(metadata)
    .bind(conversion_engine)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("创建 Document 失败: {e}");
        // 清理已落盘文件
        let _ = tokio::fs::remove_dir_all(&dir);
        db_err(format!("创建 Document 失败: {e}"))
    })?;

    tracing::info!(
        "用户 {} 上传 Document {}（{} 页，{}）",
        auth.id,
        doc_id,
        page_count,
        file_name
    );

    Ok((StatusCode::CREATED, Json(json!({ "data": doc }))))
}

/// GET /api/v1/ai/documents — 当前用户的资料列表
pub async fn list_documents(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let docs: Vec<Document> = sqlx::query_as::<_, Document>(
        r#"
        SELECT id, creator_id, file_name, file_size, mime, page_count,
               document_type, type_label, title, source_type, sub_source_type,
               status, ai_classification, metadata, conversion_engine,
               created_at, updated_at
        FROM documents
        WHERE creator_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(auth.id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| db_err(format!("查询 Document 列表失败: {e}")))?;

    Ok(Json(json!({ "data": docs })))
}

/// GET /api/v1/ai/documents/{id} — 资料详情
pub async fn get_document(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(doc_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let doc = load_document(&state.pool, doc_id).await?.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "资料不存在"})),
        )
    })?;
    if !can_manage_document(&doc, &auth) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "资料不存在"})),
        ));
    }
    Ok(Json(json!({ "data": doc })))
}

/// POST /api/v1/ai/documents/{id}/classify — AI 资料类型识别（多级 fallback）
///
/// 服务端执行 Level 1→2→3 检测；最终置信度仍 <0.6 时输出 unknown，
/// 由前端强制用户选择（计划书 §7.2）。
pub async fn classify_document(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(doc_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let doc = load_document(&state.pool, doc_id).await?.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "资料不存在"})),
        )
    })?;
    if !can_manage_document(&doc, &auth) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "资料不存在"})),
        ));
    }

    let page_files: Vec<String> = doc
        .metadata
        .get("pages")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    if page_files.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "该资料没有可分析的页面"})),
        ));
    }

    let prev_status = doc.status.clone();
    if let Err(e) = sqlx::query("UPDATE documents SET status = 'classifying', updated_at = NOW() WHERE id = $1")
        .bind(doc.id)
        .execute(&state.pool)
        .await
    {
        return Err(db_err(format!("更新 Document 状态失败: {e}")));
    }

    let result = run_classification(&state, &doc, &page_files).await;

    match result {
        Ok(classification) => {
            let ai_classification = serde_json::to_value(&classification)
                .map_err(|e| db_err(format!("序列化分类结果失败: {e}")))?;
            sqlx::query(
                r#"
                UPDATE documents
                SET status = 'classified', ai_classification = $2, updated_at = NOW()
                WHERE id = $1
                "#,
            )
            .bind(doc.id)
            .bind(&ai_classification)
            .execute(&state.pool)
            .await
            .map_err(|e| db_err(format!("保存分类结果失败: {e}")))?;

            tracing::info!(
                "Document {} 分类完成: type={} confidence={:.2} level={} checked_pages={}",
                doc.id,
                classification.document_type,
                classification.confidence,
                classification.level,
                classification.checked_pages
            );

            let updated = load_document(&state.pool, doc.id).await?.unwrap();
            Ok(Json(json!({ "data": updated, "ai_classification": classification })))
        }
        Err(e) => {
            // 分类失败：恢复原状态（uploaded/classified）
            let _ = sqlx::query("UPDATE documents SET status = $2, updated_at = NOW() WHERE id = $1")
                .bind(doc.id)
                .bind(&prev_status)
                .execute(&state.pool)
                .await;
            Err(e)
        }
    }
}

/// 多级 fallback 分类核心（可独立测试）
async fn run_classification(
    state: &AppState,
    doc: &Document,
    page_files: &[String],
) -> Result<AiClassification, (StatusCode, Json<serde_json::Value>)> {
    let (api_key, provider_name, text_model, base_url) =
        resolve_ai_config(&auth_for(doc), state, ModelKind::Text)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({"error": e}))))?;
    let provider = create_provider(&provider_name, &api_key, &base_url);

    let file_name = &doc.file_name;

    // ── Level 1：文件名（文本模型） ─────────────────────────────
    let mut level = 1;
    let mut checked_pages = 0;
    let mut best = classify_text(&provider, file_name, text_model.as_deref()).await?;

    // ── Level 2：第 1 页图（视觉模型） ───────────────────────────
    if !classification_confident(&best) {
        if let Some(page1) = page_files.first() {
            match load_page_base64(&state.upload_dir, doc.id, page1).await {
                Ok(b64) => {
                    let prompt = format!(
                        "{}\n文件名：{}",
                        AI_CLASSIFY_FULL_PROMPT_VISION.as_str(),
                        file_name
                    );
                    if let Ok(raw) = provider
                        .parse_image_with_prompt(&b64, &prompt, None)
                        .await
                    {
                        if let Ok(parsed) = parse_classification(&raw) {
                            level = 2;
                            checked_pages = 1;
                            best = parsed;
                        }
                    }
                }
                Err(e) => tracing::warn!("分类 Level 2 读取首页失败: {e}"),
            }
        }
    }

    // ── Level 3：前 3 页图（视觉模型多图） ────────────────────────
    if !classification_confident(&best) {
        let mut imgs: Vec<String> = Vec::new();
        for page_file in page_files.iter().take(3) {
            match load_page_base64(&state.upload_dir, doc.id, page_file).await {
                Ok(b64) => imgs.push(b64),
                Err(e) => tracing::warn!("分类 Level 3 读取页面失败: {e}"),
            }
        }
        if !imgs.is_empty() {
            let prompt = format!(
                "{}\n文件名：{}",
                AI_CLASSIFY_FULL_PROMPT_VISION.as_str(),
                file_name
            );
            if let Ok(raw) = provider
                .parse_images_with_prompt(&imgs, &prompt, None)
                .await
            {
                if let Ok(parsed) = parse_classification(&raw) {
                    level = 3;
                    checked_pages = imgs.len();
                    best = parsed;
                }
            }
        }
    }

    // ── 归一化：非法类型 / 低置信 → unknown（前端强制用户选择） ──
    let mut document_type = best.document_type;
    if !is_valid_document_type(&document_type) || best.confidence < CLASSIFY_CONFIDENCE_THRESHOLD {
        document_type = "unknown".to_string();
    }

    Ok(AiClassification {
        document_type,
        title: best.title,
        confidence: best.confidence,
        reason: best.reason,
        level,
        checked_pages: checked_pages as i32,
    })
}

/// Level 1 文本分类调用
async fn classify_text(
    provider: &Box<dyn crate::ai::provider::AiProvider>,
    file_name: &str,
    model: Option<&str>,
) -> Result<AiClassificationRaw, (StatusCode, Json<serde_json::Value>)> {
    let raw = provider
        .parse_text_with_prompt(file_name, &AI_CLASSIFY_FULL_PROMPT_TEXT, model)
        .await
        .map_err(map_ai_error)?;
    parse_classification(&raw).map_err(|e| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": format!("AI 分类返回格式损坏: {e}")})),
        )
    })
}

fn parse_classification(raw: &str) -> Result<AiClassificationRaw, String> {
    clean_and_parse(raw).map_err(|e| e.to_string())
}

fn classification_confident(raw: &AiClassificationRaw) -> bool {
    is_valid_document_type(&raw.document_type)
        && raw.document_type != "unknown"
        && raw.confidence >= CLASSIFY_CONFIDENCE_THRESHOLD
}

/// 构造用于 resolve_ai_config 的 AuthUser（分类使用文档创建者的 AI 配置）
fn auth_for(doc: &Document) -> crate::auth::middleware::AuthUser {
    crate::auth::middleware::AuthUser {
        id: doc.creator_id,
        username: String::new(),
        role: "teacher".to_string(),
        global_role: "teacher".to_string(),
    }
}

/// POST /api/v1/ai/documents/{id}/confirm — 用户确认资料类型 + 元数据快照
///
/// 校验规则（计划书 §6.1/§四）：exam/mock_exam 必填 paper_meta.title；
/// mixed 必填 ≥1 个集合；other 必填 type_label；unknown 不允许提交。
/// 非试卷类型未提供 collections 时自动补默认单集合。
pub async fn confirm_document(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(doc_id): Path<Uuid>,
    Json(req): Json<ConfirmDocumentRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let doc = load_document(&state.pool, doc_id).await?.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "资料不存在"})),
        )
    })?;
    if !can_manage_document(&doc, &auth) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "资料不存在"})),
        ));
    }

    // 1. 业务校验（白名单 / 分类型必填 / 集合归一化）
    let normalized = validate_confirm(&req).map_err(|msg| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": msg, "code": "ERR_INVALID_CONFIRM"})),
        )
    })?;

    // 2. 显式关联已有试卷时校验存在性
    if let Some(paper_id) = req.paper_meta.as_ref().and_then(|m| m.paper_id) {
        let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM papers WHERE id = $1")
            .bind(paper_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| db_err(format!("查询试卷失败: {e}")))?;
        if exists.is_none() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "要关联的试卷不存在"})),
            ));
        }
    }

    // 3. 组装元数据快照（Worker 阶段 1 读取）
    let paper_meta = req.paper_meta.clone().map(|m| serde_json::to_value(m).unwrap_or(json!(null)));
    let collections = serde_json::to_value(&normalized.collections).unwrap_or(json!([]));
    let mut metadata = doc.metadata.clone();
    if let Some(obj) = metadata.as_object_mut() {
        if let Some(pm) = paper_meta {
            obj.insert("paper_meta".into(), pm);
        }
        obj.insert("collections".into(), collections);
        obj.insert("user_confirmed".into(), json!(true));
    }

    // 4. 标题：试卷类型取 paper_meta.title；否则取用户 title 或首个集合标题
    let title = if normalized.is_paper {
        req.paper_meta.as_ref().map(|m| m.title.trim().to_string())
    } else {
        req.title
            .clone()
            .filter(|t| !t.trim().is_empty())
            .or_else(|| normalized.collections.first().map(|c| c.title.clone()))
    };

    sqlx::query(
        r#"
        UPDATE documents
        SET document_type = $2, type_label = $3, title = $4,
            source_type = $5, sub_source_type = $6,
            metadata = $7, status = 'confirmed', updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(doc.id)
    .bind(&req.document_type)
    .bind(&req.type_label)
    .bind(&title)
    .bind(&req.source_type)
    .bind(&req.sub_source_type)
    .bind(&metadata)
    .execute(&state.pool)
    .await
    .map_err(|e| db_err(format!("保存确认信息失败: {e}")))?;

    tracing::info!(
        "Document {} 确认: type={} title={:?} collections={}",
        doc.id,
        req.document_type,
        title,
        normalized.collections.len()
    );

    let updated = load_document(&state.pool, doc.id).await?.unwrap();
    Ok(Json(json!({ "data": updated })))
}
