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
use crate::models::ai_task::{AiParseTask, AiTaskSourceType, AiTaskStatus, TaskStatusResponse};
use crate::models::user::try_consume_quota;
use crate::AppState;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

// 每日解析任务配额统一由 models::user::DAILY_TASK_QUOTA 定义（ai_usage_log 单一计量）

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

const TASK_COLUMNS: &str = "id, creator_id, raw_text, source_type, image_b64, pdf_bytes, \
     ocr_provider_override, status, question_id, question_ids, error_message, \
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

/// 暂存项 index 排序键：`p{页号}_i{序号}` / `c{块号}_i{序号}`
///
/// 必须按页号/块号**数值**排序：字符串排序下 `p10_i0` < `p2_i0`（'1'<'2'），
/// 超过 9 页/块的文档题目顺序会与原文错乱。无法解析的键回退字符串序，保证稳定。
fn index_sort_key(k: &str) -> (i64, i64, &str) {
    let body = k.strip_prefix('p').or_else(|| k.strip_prefix('c')).unwrap_or(k);
    match body.split_once("_i") {
        Some((a, b)) => match (a.parse::<i64>(), b.parse::<i64>()) {
            (Ok(maj), Ok(min)) => (maj, min, k),
            _ => (i64::MAX, i64::MAX, k),
        },
        None => (i64::MAX, i64::MAX, k),
    }
}

/// 任务产出题目 ID（按 progress.idempotency_map 键自然排序）
///
/// 暂存改造后该映射仅在确认保存时写入（index → 已保存题目 ID）。
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
    pairs.sort_by(|a, b| {
        let (ka, kb) = (index_sort_key(&a.0), index_sort_key(&b.0));
        ka.cmp(&kb).then_with(|| a.1.cmp(&b.1))
    });
    pairs.into_iter().map(|(_, id)| id).collect()
}

/// 暂存题目列表（progress.staged_questions，按原文顺序数值排序）
fn task_staged_questions(task: &AiParseTask) -> Vec<serde_json::Value> {
    let mut items: Vec<(String, serde_json::Value)> = task
        .progress
        .get("staged_questions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let key = item.get("index")?.as_str()?.to_string();
                    Some((key, item.clone()))
                })
                .collect()
        })
        .unwrap_or_default();
    items.sort_by(|a, b| index_sort_key(&a.0).cmp(&index_sort_key(&b.0)));
    items.into_iter().map(|(_, v)| v).collect()
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/ai/parse-task — 创建解析任务
#[derive(Debug, Deserialize)]
pub struct SubmitParseTaskRequest {
    pub document_id: Uuid,
    /// 任务级 OCR 引擎覆盖（可选）：doc2x | mineru_local | qwen_vl | auto
    ///
    /// 优先于用户设置页偏好（worker 侧 resolve_ocr_config 决策链第 1 级）；
    /// 透传至 ai_parse_tasks.ocr_provider_override 列（INSERT 绑定待接入）。
    #[serde(default)]
    pub ocr_provider_override: Option<String>,
    /// 解析模式（可选，随 paper_meta 快照入库）：
    /// - `pdf_direct`：仅走 PDF 直连快速路径，失败即任务失败（前端据此引导用户选择回退）
    /// - `page`：跳过直连，直接逐页 OCR（PDF 直连失败后的用户确认回退）
    /// - 缺省：自动策略（直连失败自动降级逐页，V2.1.1 原行为）
    #[serde(default)]
    pub parse_mode: Option<String>,
}

pub async fn submit_parse_task(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Json(req): Json<SubmitParseTaskRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    // 1. Document 必须存在；OCR 先行：uploaded/classifying/classified/confirmed 均可建任务
    let doc = sqlx::query_as::<_, (Uuid, String, Option<String>, serde_json::Value, Option<String>)>(
        "SELECT id, status, document_type, metadata, mime FROM documents WHERE id = $1",
    )
    .bind(req.document_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| db_err(format!("查询 Document 失败: {e}")))?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "资料不存在"}))))?;

    let (doc_id, doc_status, doc_type, doc_metadata, doc_mime) = doc;
    const ALLOWED_PARSE_STATUSES: &[&str] = &["uploaded", "classifying", "classified", "confirmed"];
    if !ALLOWED_PARSE_STATUSES.contains(&doc_status.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("当前资料状态（{doc_status}）不允许开始解析"),
                "code": "ERR_DOCUMENT_STATUS"
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
    let quota_ok = try_consume_quota(&state.pool, auth.id, "parse_task")
        .await
        .map_err(|e| db_err(format!("配额校验失败: {e}")))?;

    if !quota_ok {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "今日解析任务额度已耗尽",
                "code": "ERR_QUOTA_EXCEEDED"
            })),
        ));
    }

    // parse_mode 白名单校验（非法值直接 400，防止脏数据流入 worker）
    if let Some(pm) = req.parse_mode.as_deref() {
        if !matches!(pm, "pdf_direct" | "page") {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "parse_mode 仅支持 pdf_direct | page",
                    "code": "ERR_INVALID_PARSE_MODE"
                })),
            ));
        }
    }

    // 4. 输入快照：来源级联 + 可选建卷（OCR 先行时可能尚未 confirm）
    let paper_meta_snapshot = json!({
        "document_type": doc_type,
        "source_category": doc_metadata.get("source_category").cloned().unwrap_or(json!(null)),
        "source_kind": doc_metadata.get("source_kind").cloned().unwrap_or(json!(null)),
        "create_paper": doc_metadata.get("create_paper").cloned().unwrap_or(json!(false)),
        "title": doc_metadata.get("title").cloned().unwrap_or(json!(null)),
        "paper_meta": doc_metadata.get("paper_meta").cloned().unwrap_or(json!(null)),
        "collections": doc_metadata.get("collections").cloned().unwrap_or(json!([])),
        "parse_mode": req.parse_mode,
    });

    let page_count: i32 = sqlx::query_scalar("SELECT page_count FROM documents WHERE id = $1")
        .bind(doc_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询页数失败: {e}")))?;

    // 5. 创建任务
    // source_type 按 Document 原始 MIME 推导（pdf / image），保持任务来源统计口径准确
    // 注意：绑定类型化枚举（列类型 ai_task_source_type，绑 &str 会被 PG 判为 text 报错）
    let source_type = if doc_mime.as_deref() == Some("application/pdf") {
        AiTaskSourceType::Pdf
    } else {
        AiTaskSourceType::Image
    };
    let task_id = Uuid::new_v4();
    let task: AiParseTask = sqlx::query_as::<_, AiParseTask>(&format!(
        r#"
        INSERT INTO ai_parse_tasks (id, creator_id, raw_text, source_type, status, created_at, updated_at,
            document_id, paper_meta, total_pages, progress)
        VALUES ($1, $2, '', $3, 'pending', NOW(), NOW(), $4, $5, $6, '{{"idempotency_map": {{}}}}')
        RETURNING {TASK_COLUMNS}
        "#
    ))
    .bind(task_id)
    .bind(auth.id)
    .bind(source_type)
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

    // 本任务待审核的标签候选数（成功提示用；查询失败不阻塞状态返回）
    let pending_candidate_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tag_candidates WHERE source_task_id = $1 AND status = 'pending'",
    )
    .bind(task.id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    Ok(Json(TaskStatusResponse {
        id: task.id,
        status: task.status.to_view(),
        // worker 失败路径写入 last_error（分页进度级），error_message 为旧单题任务列；
        // 二者取其一返回，保证失败原因（含 PDF_DIRECT_FAILED 前缀）能触达前端
        error_message: task.error_message.clone().or(task.last_error.clone()),
        created_at: task.created_at,
        updated_at: task.updated_at,
        total_count: task.total_count,
        processed_count: task.processed_count,
        success_count: task.success_count,
        failed_count: task.failed_count,
        retry_count: task.retry_count,
        current_page: task.current_page,
        total_pages: task.total_pages,
        current_question_no: task.current_question_no.clone(),
        started_at: task.started_at,
        completed_at: task.completed_at,
        document_id: task.document_id,
        paper_id,
        collection_ids,
        question_ids,
        pending_candidate_count,
        staged_questions: task_staged_questions(&task),
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

    if matches!(task.status, AiTaskStatus::Pending | AiTaskStatus::Retrying) {
        sqlx::query(
            r#"
            UPDATE ai_parse_tasks
            SET status = 'cancelled', cancel_requested_at = COALESCE(cancel_requested_at, NOW()),
                last_error = '用户取消', completed_at = NOW(), updated_at = NOW(),
                locked_at = NULL, worker_id = NULL
            WHERE id = $1 AND status IN ('pending', 'retrying')
            "#,
        )
        .bind(task_id)
        .execute(&state.pool)
        .await
        .map_err(|e| db_err(format!("取消任务失败: {e}")))?;
        return Ok(Json(json!({ "message": "已取消", "status": "cancelled" })));
    }

    sqlx::query(
        "UPDATE ai_parse_tasks SET cancel_requested_at = NOW(), updated_at = NOW() WHERE id = $1 AND status = 'processing'",
    )
    .bind(task_id)
    .execute(&state.pool)
    .await
    .map_err(|e| db_err(format!("取消任务失败: {e}")))?;

    Ok(Json(json!({ "message": "已请求取消，正在停止解析", "status": "cancelling" })))
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // SubmitParseTaskRequest.ocr_provider_override 反序列化契约（透传入口）
    //
    // 契约：字段可选（#[serde(default)]），旧客户端不带该字段必须兼容；
    // 值原样透传（不做白名单校验——未知引擎由 resolve_ocr_config 降级链处理）。
    // -----------------------------------------------------------------------

    const DOC_ID: &str = "00000000-0000-0000-0000-000000000001";

    #[test]
    fn test_request_override_deserializes() {
        // 显式指定引擎 → Some("doc2x")，原样透传
        let req: SubmitParseTaskRequest =
            serde_json::from_str(&format!(r#"{{"document_id":"{DOC_ID}","ocr_provider_override":"doc2x"}}"#))
                .expect("带 override 的请求应解析成功");
        assert_eq!(req.ocr_provider_override.as_deref(), Some("doc2x"));
        assert_eq!(req.document_id.to_string(), DOC_ID);
    }

    #[test]
    fn test_request_missing_override_defaults_none() {
        // 旧客户端不带该字段 → None（serde(default) 向后兼容）
        let req: SubmitParseTaskRequest =
            serde_json::from_str(&format!(r#"{{"document_id":"{DOC_ID}"}}"#))
                .expect("缺省 override 的请求应解析成功");
        assert_eq!(req.ocr_provider_override, None);
    }

    #[test]
    fn test_request_null_override_is_none() {
        // 显式 null → None（与缺省等价）
        let req: SubmitParseTaskRequest = serde_json::from_str(
            &format!(r#"{{"document_id":"{DOC_ID}","ocr_provider_override":null}}"#),
        )
        .expect("null override 的请求应解析成功");
        assert_eq!(req.ocr_provider_override, None);
    }

    #[test]
    fn test_request_override_value_passthrough_unvalidated() {
        // 任意字符串值原样透传（含未知引擎名），白名单/降级由 resolve_ocr_config 负责
        for v in ["mineru_local", "qwen_vl", "auto", "unknown_engine"] {
            let req: SubmitParseTaskRequest = serde_json::from_str(&format!(
                r#"{{"document_id":"{DOC_ID}","ocr_provider_override":"{v}"}}"#
            ))
            .unwrap_or_else(|e| panic!("引擎值 {v} 应可解析: {e}"));
            assert_eq!(req.ocr_provider_override.as_deref(), Some(v));
        }
    }

    #[test]
    fn test_request_invalid_document_id_rejected() {
        // document_id 非 UUID → 解析失败（请求整体拒绝，不部分透传）
        assert!(serde_json::from_str::<SubmitParseTaskRequest>(
            r#"{"document_id":"not-a-uuid","ocr_provider_override":"doc2x"}"#
        )
        .is_err());
    }

    // -----------------------------------------------------------------------
    // task_question_ids：题目顺序 = 原文档顺序（idempotency_map 键自然排序）
    //
    // 契约：键 p{页}_i{序} / c{块}_i{序} 按数值排序；字符串排序会使
    // p10 < p2（'1'<'2'），导致 9 页以上文档题序错乱。
    // -----------------------------------------------------------------------

    fn ids_task(map: serde_json::Value) -> AiParseTask {
        let mut t = fake_task();
        t.progress = map;
        t
    }

    fn fake_task() -> AiParseTask {
        AiParseTask {
            id: Uuid::new_v4(),
            creator_id: Uuid::new_v4(),
            raw_text: None,
            source_type: crate::models::ai_task::AiTaskSourceType::Image,
            image_b64: None,
            pdf_bytes: None,
            ocr_provider_override: None,
            status: crate::models::ai_task::AiTaskStatus::Success,
            question_id: None,
            question_ids: None,
            error_message: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            document_id: None,
            paper_meta: serde_json::json!({}),
            total_count: 0,
            processed_count: 0,
            success_count: 0,
            failed_count: 0,
            retry_count: 0,
            current_page: None,
            total_pages: None,
            current_question_no: None,
            started_at: None,
            completed_at: None,
            last_error: None,
            progress: serde_json::json!({}),
            locked_at: None,
            worker_id: None,
            heartbeat_at: None,
            cancel_requested_at: None,
        }
    }

    fn uuid_from(seed: u8) -> Uuid {
        // 稳定 UUID 便于断言顺序（uuid v4 无 seed；用简单拼装）
        Uuid::parse_str(&format!(
            "00000000-0000-0000-0000-{:012x}",
            seed as u64
        ))
        .unwrap()
    }

    #[test]
    fn test_question_ids_natural_sort_across_ten_pages() {
        // 11 页文档：字符串序 p10_i0 会排到 p2_i0 前 → 错乱；自然序应保持页号递增
        let mut map = serde_json::Map::new();
        let expect: Vec<Uuid> = (1..=11)
            .map(|p| {
                let id = uuid_from(p as u8);
                map.insert(format!("p{p}_i0"), serde_json::json!(id.to_string()));
                id
            })
            .collect();
        let task = ids_task(serde_json::json!({ "idempotency_map": map }));
        assert_eq!(task_question_ids(&task), expect, "跨 10 页题序必须按页号数值排序");
    }

    #[test]
    fn test_question_ids_sort_within_page() {
        // 同页内按题内序号 i 排序（i2 < i10）
        let mut map = serde_json::Map::new();
        let id2 = uuid_from(2);
        let id10 = uuid_from(10);
        map.insert("p1_i2".into(), serde_json::json!(id2.to_string()));
        map.insert("p1_i10".into(), serde_json::json!(id10.to_string()));
        let task = ids_task(serde_json::json!({ "idempotency_map": map }));
        assert_eq!(task_question_ids(&task), vec![id2, id10]);
    }

    #[test]
    fn test_question_ids_mixed_page_and_chunk_keys() {
        // 混合键（不应出现在单任务中，但需稳定不 panic）：可解析键在前按数值序，
        // 不可解析键回退字符串序排后
        let mut map = serde_json::Map::new();
        let idc2 = uuid_from(21);
        let idp1 = uuid_from(22);
        let idx = uuid_from(23);
        map.insert("c2_i0".into(), serde_json::json!(idc2.to_string()));
        map.insert("p1_i0".into(), serde_json::json!(idp1.to_string()));
        map.insert("legacy_key".into(), serde_json::json!(idx.to_string()));
        let task = ids_task(serde_json::json!({ "idempotency_map": map }));
        // p1/c2 均可解析：p1(1) < c2(2)；legacy_key 不可解析排最后
        assert_eq!(task_question_ids(&task), vec![idp1, idc2, idx]);
    }

    #[test]
    fn test_question_ids_empty_map() {
        let task = ids_task(serde_json::json!({ "idempotency_map": {} }));
        assert!(task_question_ids(&task).is_empty());
    }
}

