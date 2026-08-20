//! V2.1.1 P0-C：AI 解析 Worker（计划书 §七）
//!
//! 全链路：Document → 容器（Paper / QuestionCollection）→ 逐页 OCR →
//! 跨页组装（可选）→ 逐题（hash 去重 → 建/复用 Question → 关联 + 知识点）→
//! 终态（cancelled > failed > partial_success > success）。
//!
//! 可靠性：
//! - Stage 0 原子认领（SKIP LOCKED + locked_at/worker_id/heartbeat_at）
//! - 租约 60s / 心跳 20s；僵尸任务（120s 无心跳）重新入队或 failed（§7.3）
//! - 幂等：progress.idempotency_map（question_index → question_id）+
//!   (paper_id,question_id)/(collection_id,question_id) 唯一索引 + 容器幂等键
//! - 取消：题间检查 cancel_requested_at，终态 cancelled，已落库题目保留（§6.4）
//! - 错误分类：不可重试（NoApiKey/数据缺失）→ failed；
//!   可重试（上游/超时/JSON）→ retrying（retry_count+1，≤2 次）

use std::time::Duration;
use uuid::Uuid;

use base64::Engine as _;

use crate::ai::cleaner::clean_and_parse;
use crate::ai::ocr::{
    create_ocr_provider, parse_percent_value, should_fallback, OcrError, OcrProvider,
    PdfProgressCallback,
};
use crate::ai::prompt::{BATCH_IMAGE_OCR_FULL_PROMPT, STAGE2_PARSE_FULL_PROMPT};
use crate::ai::provider::{create_provider, AiError, AiProvider};
use crate::ai::tagging::{
    run_tagging, tagging_content_from_parsed, TaggingContext, TaggingInput, TaggingPolicy,
};
use crate::ai::types::ParsedQuestion;
use crate::auth::middleware::AuthUser;
use crate::auth::permissions::ensure_personal_space;
use crate::handlers::ai::{post_process_batch, resolve_ai_config, resolve_ocr_config, ModelKind};
use crate::handlers::collections::{get_or_create_collection, link_question_to_collection};
use crate::models::ai_task::{AiParseTask, AiTaskSourceType, AiTaskStatus};
use crate::models::document::is_paper_type;
use crate::util::normalize::compute_normalized_content_hash;
use crate::AppState;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 僵尸任务判定：心跳超时（120s）
const HEARTBEAT_TIMEOUT: &str = "120 seconds";
/// 可重试次数上限
const MAX_RETRIES: i32 = 2;
/// 跨页组装默认开启（环境变量 AI_TASK_ASSEMBLE=0 可关闭）
const ASSEMBLE_ENABLED_BY_DEFAULT: bool = true;

const TASK_COLUMNS: &str = "id, creator_id, raw_text, source_type, image_b64, pdf_bytes, \
     ocr_provider_override, status, question_id, question_ids, error_message, \
     created_at, updated_at, document_id, paper_meta, total_count, processed_count, \
     success_count, failed_count, retry_count, current_page, total_pages, \
     current_question_no, started_at, completed_at, last_error, progress, \
     locked_at, worker_id, heartbeat_at, cancel_requested_at";

/// 任务执行结果：直接写库的终态
enum TaskOutcome {
    Terminal(AiTaskStatus),
}

/// 任务失败：retryable=true 时走 retrying 重试
struct TaskFailure {
    retryable: bool,
    message: String,
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 启动 AI 解析 worker 后台协程（main.rs tokio::spawn）
pub async fn start_worker(state: AppState) {
    let worker_id = format!("worker-{}", Uuid::new_v4().simple());
    tracing::info!("🤖 AI 解析 worker 已启动（{worker_id}），每 2s 轮询一次任务");

    loop {
        recover_stale_tasks(&state, &worker_id).await;
        match process_one_task(&state, &worker_id).await {
            Ok(true) => {}
            Ok(false) => {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Err(e) => {
                tracing::error!("Worker 循环异常: {e}，5 秒后重试");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

/// 恢复僵尸任务（计划书 §7.3）：只有"超时且无心跳"才允许重新入队
async fn recover_stale_tasks(state: &AppState, worker_id: &str) {
    let requeued = sqlx::query(&format!(
        r#"
        UPDATE ai_parse_tasks
        SET status = 'pending', retry_count = retry_count + 1,
            locked_at = NULL, worker_id = NULL, updated_at = NOW()
        WHERE status = 'processing'
          AND heartbeat_at < NOW() - INTERVAL '{HEARTBEAT_TIMEOUT}'
          AND retry_count < {MAX_RETRIES}
        "#
    ))
    .execute(&state.pool)
    .await
    .map(|r| r.rows_affected())
    .unwrap_or(0);

    let failed = sqlx::query(&format!(
        r#"
        UPDATE ai_parse_tasks
        SET status = 'failed', last_error = '任务处理超时（租约过期）',
            completed_at = NOW(), updated_at = NOW()
        WHERE status = 'processing'
          AND heartbeat_at < NOW() - INTERVAL '{HEARTBEAT_TIMEOUT}'
          AND retry_count >= {MAX_RETRIES}
        "#
    ))
    .execute(&state.pool)
    .await
    .map(|r| r.rows_affected())
    .unwrap_or(0);

    if requeued > 0 || failed > 0 {
        tracing::warn!("Worker {worker_id} 恢复僵尸任务：requeue={requeued} failed={failed}");
    }
}

// ---------------------------------------------------------------------------
// 单任务处理
// ---------------------------------------------------------------------------

/// 拾取并处理一个任务；Ok(true)=处理了任务，Ok(false)=队列为空
async fn process_one_task(state: &AppState, worker_id: &str) -> Result<bool, String> {
    let task: Option<AiParseTask> = sqlx::query_as::<_, AiParseTask>(&format!(
        r#"
        UPDATE ai_parse_tasks
        SET status = 'processing', locked_at = NOW(), worker_id = $1,
            heartbeat_at = NOW(), started_at = COALESCE(started_at, NOW()), updated_at = NOW()
        WHERE id = (
            SELECT id FROM ai_parse_tasks
            WHERE status IN ('pending', 'retrying')
            ORDER BY created_at ASC
            LIMIT 1
            FOR UPDATE SKIP LOCKED
        )
        RETURNING {TASK_COLUMNS}
        "#
    ))
    .bind(worker_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| format!("拾取任务失败: {e}"))?;

    let Some(task) = task else {
        return Ok(false);
    };

    let task_id = task.id;
    tracing::info!(
        "Worker {worker_id} 拾取任务 {task_id}（document={:?}，第 {} 次执行）",
        task.document_id,
        task.retry_count + 1
    );

    match execute_task(state, &task).await {
        Ok(TaskOutcome::Terminal(status)) => {
            // ⚠️ 必须参数化 bind（Rust enum → PG enum）。
            // 曾用 serde_json::to_string 拼 SQL，产生 "failed"（双引号）被 PG 当作
            // 标识符列名解析 → "字段 failed 不存在"，导致所有任务无法落终态。
            sqlx::query(
                r#"
                UPDATE ai_parse_tasks
                SET status = $1::ai_task_status, completed_at = NOW(), updated_at = NOW()
                WHERE id = $2
                "#,
            )
            .bind(&status)
            .bind(task_id)
            .execute(&state.pool)
            .await
            .map_err(|e| format!("任务 {task_id} 标记终态失败: {e}"))?;
            tracing::info!("✅ 任务 {task_id} 终态: {status:?}");
        }
        Err(failure) => {
            let short: String = if failure.message.chars().count() > 1000 {
                format!("{}...", failure.message.chars().take(1000).collect::<String>())
            } else {
                failure.message.clone()
            };

            if failure.retryable && task.retry_count < MAX_RETRIES {
                sqlx::query(
                    r#"
                    UPDATE ai_parse_tasks
                    SET status = 'retrying', retry_count = retry_count + 1,
                        last_error = $1, updated_at = NOW()
                    WHERE id = $2
                    "#,
                )
                .bind(&short)
                .bind(task_id)
                .execute(&state.pool)
                .await
                .map_err(|e| format!("任务 {task_id} 标记 retrying 失败: {e}"))?;
                tracing::warn!("♻️ 任务 {task_id} 可重试失败（第 {} 次）: {short}", task.retry_count + 1);
            } else {
                sqlx::query(
                    r#"
                    UPDATE ai_parse_tasks
                    SET status = 'failed', last_error = $1, completed_at = NOW(), updated_at = NOW()
                    WHERE id = $2
                    "#,
                )
                .bind(&short)
                .bind(task_id)
                .execute(&state.pool)
                .await
                .map_err(|e| format!("任务 {task_id} 标记 failed 失败: {e}"))?;
                tracing::error!("❌ 任务 {task_id} 失败（不可重试或重试耗尽）: {short}");
            }
        }
    }

    Ok(true)
}

// ---------------------------------------------------------------------------
// 任务执行核心
// ---------------------------------------------------------------------------

/// 执行单个解析任务（阶段 1-4）
async fn execute_task(state: &AppState, task: &AiParseTask) -> Result<TaskOutcome, TaskFailure> {
    let task_id = task.id;

    // ── Stage 1：加载 Document 与输入快照 ──────────────────────────
    let doc_id = task
        .document_id
        .ok_or_else(|| TaskFailure { retryable: false, message: "任务缺少 Document 关联".into() })?;

    let doc: (String, Option<String>, Option<String>, serde_json::Value) = sqlx::query_as(
        "SELECT status, document_type, title, metadata FROM documents WHERE id = $1",
    )
    .bind(doc_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| TaskFailure { retryable: false, message: format!("查询 Document 失败: {e}") })?
    .ok_or_else(|| TaskFailure { retryable: false, message: "Document 不存在".into() })?;

    let (doc_status, doc_type, _doc_title, doc_metadata) = doc;
    if doc_status != "confirmed" {
        return Err(TaskFailure {
            retryable: false,
            message: "Document 尚未确认资料类型".into(),
        });
    }

    let page_files: Vec<String> = doc_metadata
        .get("pages")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    if page_files.is_empty() {
        return Err(TaskFailure {
            retryable: false,
            message: "Document 没有页面文件".into(),
        });
    }

    // 用户与 AI 配置
    let user_row: Option<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT username, role::text, global_role::text, display_name FROM users WHERE id = $1",
    )
    .bind(task.creator_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| TaskFailure { retryable: false, message: format!("查询用户失败: {e}") })?;
    let Some((username, role, global_role, display_name)) = user_row else {
        return Err(TaskFailure {
            retryable: false,
            message: format!("用户 {} 不存在", task.creator_id),
        });
    };
    let auth = AuthUser {
        id: task.creator_id,
        username,
        role,
        global_role,
    };

    let (vision_key, vision_provider_name, vision_model, vision_base) =
        resolve_ai_config(&auth, state, ModelKind::Vision)
            .await
            .map_err(|e| TaskFailure { retryable: false, message: e })?;
    let vision_provider = create_provider(&vision_provider_name, &vision_key, &vision_base);

    // OCR 引擎路由（任务 override > 用户偏好 > auto）：
    // - doc2x / mineru：两阶段（引擎 OCR → Markdown → 文本模型 Stage 2 结构化）
    // - qwen_vl / auto / 未配置：单阶段视觉批量识别（V2.1.1 原路径，行为不变）
    let ocr_cfg = resolve_ocr_config(&auth, state, task.ocr_provider_override.as_deref())
        .await
        .map_err(|e| TaskFailure { retryable: false, message: e })?;
    let ocr_engine = create_ocr_provider(&ocr_cfg);
    let two_stage = ocr_engine.id() != "qwen_vl";
    tracing::info!(
        "任务 {task_id} OCR 引擎: {}（{}阶段）",
        ocr_engine.id(),
        if two_stage { "两" } else { "单" }
    );

    // 跨页组装（文本模型，可选）；两阶段 OCR 的 Stage 2 也需要文本模型
    let assemble_enabled = std::env::var("AI_TASK_ASSEMBLE")
        .ok()
        .map(|v| v != "0")
        .unwrap_or(ASSEMBLE_ENABLED_BY_DEFAULT);
    let text_provider = if assemble_enabled || two_stage {
        let resolved = resolve_ai_config(&auth, state, ModelKind::Text)
            .await
            .ok()
            .map(|(key, name, model, base)| (create_provider(&name, &key, &base), model));
        if two_stage && resolved.is_none() {
            return Err(TaskFailure {
                retryable: false,
                message: "两阶段 OCR 解析需要文本模型 API Key，请在设置页配置".into(),
            });
        }
        resolved
    } else {
        None
    };

    // 个人空间
    let space_id = ensure_personal_space(
        &state.pool,
        task.creator_id,
        display_name.as_deref().unwrap_or("用户"),
    )
    .await
    .map_err(|e| TaskFailure { retryable: false, message: format!("创建个人空间失败: {e}") })?;

    // ── Stage 2：容器（Paper / QuestionCollection） ────────────────
    let pm = &task.paper_meta;
    let document_type = pm
        .get("document_type")
        .and_then(|v| v.as_str())
        .unwrap_or(doc_type.as_deref().unwrap_or("unknown"))
        .to_string();
    let is_paper = is_paper_type(&document_type);
    let is_mixed = document_type == "mixed";

    let (paper_id, collection_ids): (Option<Uuid>, Vec<Uuid>) = if is_paper {
        // Paper：显式关联 > document_id 幂等复用 > 新建
        let explicit_paper: Option<Uuid> = pm
            .get("paper_meta")
            .and_then(|m| m.get("paper_id"))
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok());
        if let Some(pid) = explicit_paper {
            let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM papers WHERE id = $1")
                .bind(pid)
                .fetch_optional(&state.pool)
                .await
                .map_err(|e| TaskFailure { retryable: false, message: format!("查询试卷失败: {e}") })?;
            if exists.is_none() {
                return Err(TaskFailure {
                    retryable: false,
                    message: "要关联的试卷不存在".into(),
                });
            }
            (Some(pid), vec![])
        } else {
            let existing: Option<Uuid> =
                sqlx::query_scalar("SELECT id FROM papers WHERE document_id = $1 LIMIT 1")
                    .bind(doc_id)
                    .fetch_optional(&state.pool)
                    .await
                    .map_err(|e| TaskFailure { retryable: false, message: format!("查询试卷失败: {e}") })?;
            match existing {
                Some(pid) => (Some(pid), vec![]),
                None => {
                    let pid = create_paper_from_meta(state, &auth, doc_id, pm).await?;
                    (Some(pid), vec![])
                }
            }
        }
    } else {
        // QuestionCollection：复用键 (document_id, title)；无快照 → 默认单集合
        let mut ids = Vec::new();
        let collections = pm
            .get("collections")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();
        if collections.is_empty() {
            let default_title = pm
                .get("title")
                .and_then(|t| t.as_str())
                .filter(|t| !t.trim().is_empty())
                .unwrap_or("默认题目集合");
            let col = get_or_create_collection(
                &state.pool,
                task.creator_id,
                doc_id,
                default_title,
                &document_type,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .map_err(|e| TaskFailure { retryable: false, message: e })?;
            ids.push(col.id);
        } else {
            for c in &collections {
                let title = c.get("title").and_then(|t| t.as_str()).unwrap_or("未命名集合");
                let ctype = c.get("collection_type").and_then(|t| t.as_str()).unwrap_or("other");
                let type_label = c.get("type_label").and_then(|t| t.as_str());
                let source_type = c.get("source_type").and_then(|t| t.as_str());
                let subject = c.get("subject").and_then(|t| t.as_str());
                let stage = c.get("stage").and_then(|t| t.as_str());
                let grade = c.get("grade").and_then(|t| t.as_str());
                let semester = c.get("semester").and_then(|t| t.as_str());
                let chapter_id = c
                    .get("chapter_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok());
                let col = get_or_create_collection(
                    &state.pool,
                    task.creator_id,
                    doc_id,
                    title,
                    ctype,
                    type_label,
                    source_type,
                    subject,
                    stage,
                    grade,
                    semester,
                    chapter_id,
                )
                .await
                .map_err(|e| TaskFailure { retryable: false, message: e })?;
                ids.push(col.id);
            }
        }
        (None, ids)
    };

    // ── Stage 3：OCR 解析 → 逐题落库 ────────────────────────────────
    // PDF 直传快速路径：引擎原生支持整档 OCR（Doc2X/MinerU）且保留有原始 PDF 时，
    // 一次调用完成全部页面识别（N 次 OCR 请求 → 1 次）。
    //
    // parse_mode（随 paper_meta 快照入库）控制降级策略：
    // - 缺省 "auto"：直连失败自动降级逐页路径（V2.1.1 原行为）
    // - "pdf_direct"：仅走直连，失败即任务失败（PDF_DIRECT_FAILED 前缀 →
    //   前端引导用户选择是否拆页 OCR 回退）；引擎不支持/未保留 PDF 同样视为直连失败
    // - "page"：跳过直连，直接逐页路径（用户确认回退后的重跑模式）
    let parse_mode = task
        .paper_meta
        .get("parse_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("auto")
        .to_string();
    let mut all_questions: Vec<(String, ParsedQuestion)> = Vec::new();
    let mut success_count: i32 = 0;
    let mut failed_count: i32 = 0;
    let mut processed_count: i32 = 0;
    let mut cancelled = false;
    // 直连成功后跳过逐页循环（pdf_direct 成功 / auto 成功两种情况）
    let mut fast_path_done = false;

    let pdf_file: Option<String> = doc_metadata
        .get("pdf_file")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let fast_path_available = pdf_file.is_some()
        && text_provider.is_some()
        && ocr_engine.supports_pdf();
    if parse_mode == "pdf_direct" {
        if !fast_path_available {
            let reason = if !ocr_engine.supports_pdf() {
                format!("OCR 引擎 {} 不支持 PDF 直传", ocr_engine.id())
            } else if pdf_file.is_none() {
                "未保留原始 PDF 文件".to_string()
            } else {
                "两阶段解析需要文本模型 API Key，请在设置页配置".to_string()
            };
            return Err(TaskFailure {
                retryable: false,
                message: format!("PDF_DIRECT_FAILED: {reason}"),
            });
        }
        let pf = pdf_file.as_deref().expect("fast_path_available 已校验");
        let (tp, tm) = text_provider.as_ref().expect("fast_path_available 已校验");
        let pdf_path = std::path::Path::new(&state.upload_dir)
            .join("documents")
            .join(doc_id.to_string())
            .join(pf);
        let total_pages = task.total_pages.unwrap_or(page_files.len() as i32);
        match run_pdf_fast_path(
            state,
            task,
            &pdf_path,
            ocr_engine.as_ref(),
            tp.as_ref(),
            tm.as_deref(),
            total_pages,
            paper_id,
            collection_ids.first().copied(),
            is_mixed,
            space_id,
            &mut all_questions,
        )
        .await
        {
            Ok(outcome) => {
                success_count = outcome.success_count;
                failed_count = outcome.failed_count;
                processed_count = outcome.processed_count;
                cancelled = outcome.cancelled;
                fast_path_done = true;
                tracing::info!(
                    "任务 {task_id} PDF 直传快速路径完成：成功 {} / 失败 {} 题",
                    outcome.success_count,
                    outcome.failed_count
                );
            }
            Err(msg) => {
                // pdf_direct 模式不自动降级：任务直接失败，
                // 前端凭 PDF_DIRECT_FAILED 前缀引导用户选择拆页 OCR 回退
                return Err(TaskFailure {
                    retryable: false,
                    message: format!("PDF_DIRECT_FAILED: {msg}"),
                });
            }
        }
    } else if parse_mode != "page" {
        if let (Some(pf), Some((tp, tm))) = (pdf_file.as_deref(), text_provider.as_ref()) {
            if ocr_engine.supports_pdf() {
                let pdf_path = std::path::Path::new(&state.upload_dir)
                    .join("documents")
                    .join(doc_id.to_string())
                    .join(pf);
                let total_pages = task.total_pages.unwrap_or(page_files.len() as i32);
                match run_pdf_fast_path(
                    state,
                    task,
                    &pdf_path,
                    ocr_engine.as_ref(),
                    tp.as_ref(),
                    tm.as_deref(),
                    total_pages,
                    paper_id,
                    collection_ids.first().copied(),
                    is_mixed,
                    space_id,
                    &mut all_questions,
                )
                .await
                {
                    Ok(outcome) => {
                        success_count = outcome.success_count;
                        failed_count = outcome.failed_count;
                        processed_count = outcome.processed_count;
                        cancelled = outcome.cancelled;
                        fast_path_done = true;
                        tracing::info!(
                            "任务 {task_id} PDF 直传快速路径完成：成功 {} / 失败 {} 题",
                            outcome.success_count,
                            outcome.failed_count
                        );
                    }
                    Err(msg) => {
                        tracing::warn!(
                            target: "ocr::engine_select",
                            task_id = %task_id,
                            from = ocr_engine.id(),
                            to = "page_by_page",
                            "任务 {task_id} PDF 直传快速路径失败，降级逐页路径: {msg}"
                        );
                        set_last_error(state, task_id, &msg).await;
                    }
                }
            }
        }
    }

    for (page_idx, page_file) in page_files.iter().enumerate() {
        if fast_path_done {
            break;
        }
        let page_no = (page_idx + 1) as i32;

        refresh_heartbeat(state, task_id).await;
        if is_cancel_requested(state, task_id).await {
            cancelled = true;
            break;
        }

        let page_path = std::path::Path::new(&state.upload_dir)
            .join("documents")
            .join(doc_id.to_string())
            .join(page_file);
        let bytes = match tokio::fs::read(&page_path).await {
            Ok(b) => b,
            Err(e) => {
                let msg = format!("读取第 {page_no} 页失败: {e}");
                tracing::warn!("任务 {task_id} {msg}");
                failed_count += 1;
                processed_count += 1;
                set_last_error(state, task_id, &msg).await;
                update_progress(state, task_id, page_no, processed_count, success_count, failed_count).await;
                continue;
            }
        };
        let image_b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

        let raw_json = match ocr_page_to_json(
            task_id,
            &image_b64,
            ocr_engine.as_ref(),
            two_stage,
            vision_provider.as_ref(),
            vision_model.as_deref(),
            text_provider
                .as_ref()
                .map(|(p, m)| (p.as_ref() as &dyn AiProvider, m.as_deref())),
        )
        .await
        {
            Ok(raw) => raw,
            Err(msg) => {
                // 页面级失败不消耗重试（计入 failed_count，走 partial_success）
                tracing::warn!("任务 {task_id} 第 {page_no} 页 OCR 失败: {msg}");
                failed_count += 1;
                processed_count += 1;
                set_last_error(state, task_id, &msg).await;
                update_progress(state, task_id, page_no, processed_count, success_count, failed_count).await;
                continue;
            }
        };

        let page_questions = match post_process_batch(&raw_json, &state.pool).await {
            Ok(qs) => qs,
            Err((_, err)) => {
                let msg = format!("第 {page_no} 页解析失败: {}", err["error"]);
                tracing::warn!("任务 {task_id} {msg}");
                failed_count += 1;
                processed_count += 1;
                set_last_error(state, task_id, &msg).await;
                update_progress(state, task_id, page_no, processed_count, success_count, failed_count).await;
                continue;
            }
        };

        for (idx, q) in page_questions.into_iter().enumerate() {
            if idx % 5 == 0 {
                refresh_heartbeat(state, task_id).await;
                if is_cancel_requested(state, task_id).await {
                    cancelled = true;
                    break;
                }
            }

            let question_index = format!("p{page_no}_i{idx}");
            let qno = q.question_no.clone();
            processed_count += 1;
            all_questions.push((question_index.clone(), q.clone()));

            // 题内插图占位符 → 本页页面图（静态服务 /uploads → documents/{doc_id}/page_N）
            let page_image_url = format!("/uploads/documents/{doc_id}/{page_file}");
            match stage_question(
                state,
                task,
                &question_index,
                q,
                Some(&page_image_url),
                paper_id,
                collection_ids.first().copied(),
                is_mixed,
                space_id,
            )
            .await
            {
                Ok(()) => {
                    success_count += 1;
                    if let Some(no) = qno {
                        set_current_question_no(state, task_id, &no).await;
                    }
                    update_progress(
                        state,
                        task_id,
                        page_no,
                        processed_count,
                        success_count,
                        failed_count,
                    )
                    .await;
                }
                Err(e) => {
                    tracing::warn!("任务 {task_id} 第 {question_index} 题暂存失败: {e}");
                    failed_count += 1;
                    update_progress(state, task_id, page_no, processed_count, success_count, failed_count).await;
                }
            }
        }

        if cancelled {
            break;
        }
    }

    // ── Stage 3b：跨页组装（题号/顺序重排，失败降级） ──────────────
    if assemble_enabled {
        if let Some((text_provider, text_model)) = &text_provider {
            if !cancelled && all_questions.len() > 1 {
                match assemble_question_order(text_provider.as_ref(), text_model.as_deref(), &all_questions).await {
                    Ok(mapping) => {
                        apply_question_order(state, task_id, &mapping).await;
                    }
                    Err(e) => {
                        tracing::warn!("任务 {task_id} 跨页组装失败，使用原顺序: {e}");
                    }
                }
            }
        }
    }

    // ── Stage 4：终态（cancelled > failed > partial_success > success） ──
    let total_count = processed_count;
    let final_status = if cancelled {
        AiTaskStatus::Cancelled
    } else if total_count == 0 {
        AiTaskStatus::Failed
    } else if failed_count > 0 && success_count == 0 {
        AiTaskStatus::Failed
    } else if failed_count > 0 {
        AiTaskStatus::PartialSuccess
    } else {
        AiTaskStatus::Success
    };

    sqlx::query(
        "UPDATE ai_parse_tasks SET total_count = $1, processed_count = $2, success_count = $3, failed_count = $4, updated_at = NOW() WHERE id = $5",
    )
    .bind(total_count)
    .bind(processed_count)
    .bind(success_count)
    .bind(failed_count)
    .bind(task_id)
    .execute(&state.pool)
    .await
    .map_err(|e| TaskFailure { retryable: false, message: format!("回写计数失败: {e}") })?;

    Ok(TaskOutcome::Terminal(final_status))
}

// ---------------------------------------------------------------------------
// Stage 2 辅助：创建试卷
// ---------------------------------------------------------------------------

async fn create_paper_from_meta(
    state: &AppState,
    auth: &AuthUser,
    doc_id: Uuid,
    pm: &serde_json::Value,
) -> Result<Uuid, TaskFailure> {
    let meta = pm.get("paper_meta").cloned().unwrap_or(serde_json::Value::Null);
    let title = meta
        .get("title")
        .and_then(|t| t.as_str())
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| TaskFailure {
            retryable: false,
            message: "试卷元数据缺少 title".into(),
        })?;

    let id = Uuid::new_v4();
    let now = chrono::Utc::now();
    let g = |k: &str| meta.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
    let year = meta.get("year").and_then(|v| v.as_i64()).map(|v| v as i32);

    sqlx::query(
        r#"
        INSERT INTO papers (id, title, description, subject, grade, total_score, duration_minutes,
            status, creator_id, created_at, updated_at, version,
            year, stage, semester, region_province, region_city, school_name,
            source_type, sub_source_type, document_id, metadata)
        VALUES ($1, $2, NULL, $3, $4, 0, NULL, 'draft', $5, $6, $6, 1,
            $7, $8, $9, $10, $11, $12, $13, $14, $15, '{}')
        "#,
    )
    .bind(id)
    .bind(title)
    .bind(g("subject").unwrap_or_else(|| "数学".into()))
    .bind(g("grade"))
    .bind(auth.id)
    .bind(now)
    .bind(year)
    .bind(g("stage"))
    .bind(g("semester"))
    .bind(g("region_province"))
    .bind(g("region_city"))
    .bind(g("school_name"))
    .bind(g("source_type"))
    .bind(g("sub_source_type"))
    .bind(doc_id)
    .execute(&state.pool)
    .await
    .map_err(|e| TaskFailure { retryable: false, message: format!("创建试卷失败: {e}") })?;

    tracing::info!("Worker 为 Document {doc_id} 创建试卷 {id}（{title}）");
    Ok(id)
}

// ---------------------------------------------------------------------------
// Stage 3 辅助：单题持久化（幂等 + 去重 + 关联 + 知识点）
// ---------------------------------------------------------------------------

/// 外链图片本地化：`![x](http://...)` → 下载转存 `/uploads/questions/{uuid}.ext`
///
/// Doc2X 等 OCR 引擎返回的 Markdown 引用引擎侧的临时/外链图片 URL，
/// 会过期失效且不可控。统一转存为本站静态资源（与手动上传插图、
/// MinerU zip 图片转存同目录同格式），编辑器差集清理逻辑才能正确管理生命周期。
///
/// 行为：
/// - 仅处理 http/https 外链；`/uploads/...`、`data:` 等本地/内联形式原样保留
/// - 同一 URL 只下载一次（含 query 去重），替换所有出现位置
/// - 单图上限 10MB，扩展名白名单 png/jpg/jpeg/gif/webp（URL 无扩展名时按
///   Content-Type 推导，仍未知则放弃保留原链）
/// - 下载失败仅 warn，保留原 URL，不阻塞题目落库
async fn localize_external_images(parsed: &mut ParsedQuestion, upload_dir: &str) {
    // 1. 收集所有外链 URL（stem + options + analysis）
    let re =
        regex::Regex::new(r"!\[[^\]]*\]\((https?://[^)\s]+)\)").expect("外链图片正则必然合法");
    let mut externals: Vec<String> = Vec::new();
    {
        let mut push = |text: &str| {
            for cap in re.captures_iter(text) {
                let url = cap[1].to_string();
                if !externals.contains(&url) {
                    externals.push(url);
                }
            }
        };
        push(&parsed.stem);
        if let Some(opts) = parsed.options.as_ref() {
            for o in opts {
                push(&o.content);
            }
        }
        for a in &parsed.analysis {
            push(&a.content);
        }
    }
    if externals.is_empty() {
        return;
    }

    // 2. 下载转存 → URL 映射
    const MAX_BYTES: usize = 10 * 1024 * 1024;
    let questions_dir = std::path::Path::new(upload_dir).join("questions");
    let mut url_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Err(e) = tokio::fs::create_dir_all(&questions_dir).await {
        tracing::warn!("创建图片目录失败（跳过外链本地化）: {e}");
        return;
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_default();
    for url in &externals {
        let resp = match client.get(url.clone()).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("下载外链图片失败（保留原 URL）: {url} -> {e}");
                continue;
            }
        };
        if !resp.status().is_success() {
            tracing::warn!("下载外链图片失败（保留原 URL）: {url} -> HTTP {}", resp.status());
            continue;
        }
        // 扩展名：URL 路径 > Content-Type 白名单推导
        let ext_from_url = reqwest::Url::parse(url)
            .ok()
            .and_then(|u| {
                u.path_segments()?
                    .last()?
                    .rsplit_once('.')
                    .map(|(_, e)| e.to_ascii_lowercase())
            })
            .filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp"));
        let ext = ext_from_url.or_else(|| {
            let ct = resp.headers().get(reqwest::header::CONTENT_TYPE)?.to_str().ok()?;
            let e = match ct.split(';').next()?.trim().to_ascii_lowercase().as_str() {
                "image/png" => "png",
                "image/jpeg" => "jpg",
                "image/gif" => "gif",
                "image/webp" => "webp",
                _ => return None,
            };
            Some(e.to_string())
        });
        let Some(ext) = ext else {
            tracing::warn!("外链图片非白名单格式（保留原 URL）: {url}");
            continue;
        };
        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("读取外链图片内容失败（保留原 URL）: {url} -> {e}");
                continue;
            }
        };
        if bytes.len() > MAX_BYTES {
            tracing::warn!("外链图片超过 10MB 上限（保留原 URL）: {url}");
            continue;
        }
        let new_name = format!("{}.{ext}", Uuid::new_v4());
        let path = questions_dir.join(&new_name);
        if let Err(e) = tokio::fs::write(&path, &bytes).await {
            tracing::warn!("写入图片失败（保留原 URL）: {} -> {e}", path.display());
            continue;
        }
        url_map.insert(url.clone(), format!("/uploads/questions/{new_name}"));
    }
    if url_map.is_empty() {
        return;
    }

    // 3. 替换所有字段中的外链 URL（含 image_urls 汇总数组）
    let rewrite = |s: &mut String| {
        if !re.is_match(s) {
            return;
        }
        *s = re
            .replace_all(s, |caps: &regex::Captures| {
                match url_map.get(&caps[1]) {
                    Some(new_url) => format!("![配图]({new_url})"),
                    None => caps[0].to_string(),
                }
            })
            .into_owned();
    };
    rewrite(&mut parsed.stem);
    if let Some(opts) = parsed.options.as_mut() {
        for o in opts {
            rewrite(&mut o.content);
        }
    }
    for a in parsed.analysis.iter_mut() {
        rewrite(&mut a.content);
    }
    for u in parsed.image_urls.iter_mut() {
        if let Some(new_url) = url_map.get(u) {
            *u = new_url.clone();
        }
    }
    tracing::info!(
        "外链图片本地化完成：{} 张转存 / {} 张外链",
        url_map.len(),
        externals.len() - url_map.len()
    );
}

/// Markdown / HTML 中的真实图片 URL（排除 IMAGE_PLACEHOLDER）。
fn harvest_markdown_image_urls(md: &str) -> Vec<(usize, String)> {
    let md_re = regex::Regex::new(r"!\[[^\]]*\]\(([^)\s]+)\)").expect("配图正则必然合法");
    let html_re = regex::Regex::new(r#"(?i)<img[^>]+src=["']([^"']+)["']"#).expect("img 正则必然合法");
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut push = |pos: usize, url: String| {
        if url.starts_with("IMAGE_PLACEHOLDER") || url.trim().is_empty() {
            return;
        }
        if !out.iter().any(|(_, u)| u == &url) {
            out.push((pos, url));
        }
    };
    for cap in md_re.captures_iter(md) {
        let m = cap.get(1).unwrap();
        push(m.start(), cap[1].to_string());
    }
    for cap in html_re.captures_iter(md) {
        let m = cap.get(1).unwrap();
        push(m.start(), cap[1].to_string());
    }
    out.sort_by_key(|(pos, _)| *pos);
    out
}

fn question_body_text(parsed: &ParsedQuestion) -> String {
    let mut s = parsed.stem.clone();
    if let Some(opts) = &parsed.options {
        for o in opts {
            s.push('\n');
            s.push_str(&o.content);
        }
    }
    for a in &parsed.analysis {
        s.push('\n');
        s.push_str(&a.content);
    }
    s
}

fn mentions_figure(parsed: &ParsedQuestion) -> bool {
    const HINTS: &[&str] = &[
        "如图",
        "见图",
        "下图",
        "上图",
        "右图",
        "左图",
        "图中",
        "图示",
        "附图",
        "图象如下",
        "图像如下",
        "图如下",
        "阴影部分",
    ];
    let hay = question_body_text(parsed);
    HINTS.iter().any(|h| hay.contains(h))
}

fn has_real_md_image(text: &str) -> bool {
    !harvest_markdown_image_urls(text).is_empty()
}

fn question_has_inline_image(parsed: &ParsedQuestion) -> bool {
    has_real_md_image(&question_body_text(parsed))
}

fn find_question_offset(markdown: &str, q: &ParsedQuestion) -> Option<usize> {
    let no_img = regex::Regex::new(r"!\[[^\]]*\]\([^)]+\)")
        .expect("剥离配图正则必然合法")
        .replace_all(&q.stem, "");
    let first_line = no_img.lines().next().unwrap_or("").trim();
    if first_line.chars().count() >= 8 {
        let prefix: String = first_line.chars().take(18).collect();
        if let Some(pos) = markdown.find(&prefix) {
            return Some(pos);
        }
        let no_tex = regex::Regex::new(r"\$[^$]*\$")
            .expect("剥离公式正则必然合法")
            .replace_all(&prefix, "");
        let compact = no_tex.trim();
        if compact.chars().count() >= 8 {
            if let Some(pos) = markdown.find(compact) {
                return Some(pos);
            }
        }
    }
    q.question_no.as_ref().and_then(|no| {
        let needles = [format!("{no}."), format!("{no}、"), format!("{no}．")];
        needles.iter().find_map(|n| markdown.find(n.as_str()))
    })
}

/// 把 OCR 原文块中的配图划给各题：按题干在原文中的位置归属；
/// 提到「如图」但 Stage2 丢掉图片的题目，再从尚未占用的图里补一张。
fn assign_chunk_images(markdown: &str, questions: &mut [ParsedQuestion]) {
    let images = harvest_markdown_image_urls(markdown);
    if images.is_empty() {
        return;
    }

    let mut offsets: Vec<(usize, usize)> = questions
        .iter()
        .enumerate()
        .filter_map(|(i, q)| find_question_offset(markdown, q).map(|pos| (pos, i)))
        .collect();
    offsets.sort_by_key(|(pos, _)| *pos);

    if !offsets.is_empty() {
        for (k, &(start, qi)) in offsets.iter().enumerate() {
            let end = offsets.get(k + 1).map(|(p, _)| *p).unwrap_or(markdown.len());
            let q = &mut questions[qi];
            for (pos, url) in &images {
                if *pos >= start && *pos < end && !q.image_urls.contains(url) {
                    q.image_urls.push(url.clone());
                }
            }
        }
    }

    let used: std::collections::HashSet<String> = questions
        .iter()
        .flat_map(|q| q.image_urls.iter().cloned())
        .collect();
    let unused: Vec<String> = images
        .iter()
        .map(|(_, u)| u.clone())
        .filter(|u| !used.contains(u))
        .collect();
    let mut unused = unused.into_iter();
    for q in questions.iter_mut() {
        if !mentions_figure(q) {
            continue;
        }
        if question_has_inline_image(q) || !q.image_urls.is_empty() {
            continue;
        }
        if let Some(url) = unused.next() {
            q.image_urls.push(url);
        } else if let Some((_, url)) = images.first() {
            q.image_urls.push(url.clone());
        }
    }
}

fn replace_placeholders_with_url(parsed: &mut ParsedQuestion, url: &str) {
    let re = regex::Regex::new(r"\n*!\[[^\]]*\]\(IMAGE_PLACEHOLDER_\d+\)\n*").expect("占位符正则必然合法");
    let img_line = format!("![配图]({url})");
    let re_before = regex::Regex::new(r"\n+(!\[配图\]\()").expect("图前换行正则必然合法");
    let re_after = regex::Regex::new(r"(!\[配图\]\([^\n]*\))\n+").expect("图后换行正则必然合法");
    let sub = |s: &mut String| {
        if !re.is_match(s) {
            return;
        }
        let t = re.replace_all(s, format!("\n{img_line}\n").as_str());
        let t = re_after.replace_all(&t, "$1\n");
        let t = re_before.replace_all(&t, "\n$1");
        *s = t.into_owned();
    };

    sub(&mut parsed.stem);
    if let Some(opts) = parsed.options.as_mut() {
        for o in opts {
            sub(&mut o.content);
        }
    }
    for a in parsed.analysis.iter_mut() {
        sub(&mut a.content);
    }
}

fn inject_block_image(text: &str, url: &str) -> String {
    if text.contains(&format!("]({url})")) {
        return text.to_string();
    }
    let img_line = format!("![配图]({url})");
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        format!("{img_line}\n")
    } else {
        format!("{trimmed}\n{img_line}\n")
    }
}

fn has_placeholder(parsed: &ParsedQuestion) -> bool {
    parsed.stem.contains("IMAGE_PLACEHOLDER")
        || parsed.options.as_ref().is_some_and(|opts| {
            opts.iter().any(|o| o.content.contains("IMAGE_PLACEHOLDER"))
        })
        || parsed
            .analysis
            .iter()
            .any(|a| a.content.contains("IMAGE_PLACEHOLDER"))
}

/// 图片占位符解析：`![配图](IMAGE_PLACEHOLDER_N)` → 真实可访问 URL
///
/// 视觉模型无法给出题内插图的位置/裁剪信息，唯一真实图源是该题所在页的
/// 页面截图，因此所有占位符统一映射为页面图 URL（编辑器中可再人工裁剪）。
/// Stage 1（Doc2X/MinerU）提取的真实 URL 已在先行的 localize_external_images
/// 中本地化为 /uploads/questions/*，此处直接汇总进 images 列。
///
/// 布局格式（与既有题库系统的图片渲染约定保持一致）：
/// - 占位符替换为**独立成行**的 `![配图](url)`——LatexRender 后处理将
///   独立成行的无配置图片归为 `img-block`（块级居中、可点击调宽）；
///   若留在行中会被判为 `img-inline`（max-height 1.5em 的小图标），
///   几何图形将不可读。
/// - 多个占位符逐行独立成块（AI 无法判断并排关系；
///   用户可在编辑器中用 `:::img-row` 围栏重组并排图组）。
///
/// 另：Stage2 常把「如图」留下却丢掉 `![...](url)`。若题面提到配图但没有任何
/// 内联图片，则把 `image_urls` 或页面图注入题干，避免预览空白。
///
/// 返回值：写入 questions.images 的去重 URL 列表（可能为空）。
fn resolve_question_images(parsed: &mut ParsedQuestion, page_image_url: Option<&str>) -> Vec<String> {
    let mut urls: Vec<String> = parsed.image_urls.iter().cloned().collect();
    for (_, url) in harvest_markdown_image_urls(&question_body_text(parsed)) {
        if !urls.contains(&url) {
            urls.push(url);
        }
    }

    let placeholder = has_placeholder(parsed);

    if placeholder {
        if let Some(url) = page_image_url
            .map(|s| s.to_string())
            .or_else(|| urls.first().cloned())
        {
            replace_placeholders_with_url(parsed, &url);
            if !urls.contains(&url) {
                urls.push(url);
            }
        }
    }

    if !question_has_inline_image(parsed) {
        let inject = urls.first().cloned().or_else(|| {
            if mentions_figure(parsed) {
                page_image_url.map(|s| s.to_string())
            } else {
                None
            }
        });
        if let Some(url) = inject {
            parsed.stem = inject_block_image(&parsed.stem, &url);
            if !urls.contains(&url) {
                urls.push(url);
            }
        }
    }

    urls.dedup();
    urls
}

/// 录入快照中的学段：`paper_meta.stage` 或首个集合 `stage`。
/// 与编辑页默认 `senior` 对齐，缺省时返回 senior，避免 OCR 打标跨学段召回。
fn tagging_stage_from_paper_meta(pm: &serde_json::Value) -> String {
    let normalize = |s: &str| -> Option<String> {
        match s.trim() {
            "junior" | "初中" => Some("junior".into()),
            "senior" | "high" | "高中" => Some("senior".into()),
            _ => None,
        }
    };
    pm.pointer("/paper_meta/stage")
        .and_then(|v| v.as_str())
        .and_then(normalize)
        .or_else(|| {
            pm.get("collections")
                .and_then(|c| c.as_array())
                .and_then(|arr| {
                    arr.iter().find_map(|c| {
                        c.get("stage").and_then(|v| v.as_str()).and_then(normalize)
                    })
                })
        })
        .or_else(|| pm.get("stage").and_then(|v| v.as_str()).and_then(normalize))
        .unwrap_or_else(|| "senior".into())
}

#[allow(clippy::too_many_arguments)]
/// 暂存单题（V2.2：确认后才入库）
///
/// 解析阶段**不写 questions 表**：题目数据（含图片本地化后的 URL）、三维标签
/// 匹配结果、hash 去重命中信息一起暂存到 `ai_parse_tasks.progress.staged_questions`。
/// 用户在工作台确认保存时由 `POST /questions`（ai_meta）真正落库并关联容器；
/// 丢弃/永不保存的暂存项由 GC（72h）清理。
///
/// 暂存项结构：
/// ```json
/// {
///   "index": "p1_i0", "parsed": { ...ParsedQuestion... },
///   "images": [...], "page_image_url": null,
///   "space_id": "...", "existing_question_id": null,
///   "suggestion_id": "...", "engine_version": "tagging-v4",
///   "suggestion": { ...TaggingSuggestion... },
///   "matched": [{node_id, node_name, ai_name, score, match_type, kind}],
///   "unmatched": {"chapter": [], "knowledge": [], "method": []},
///   "saved": false
/// }
/// ```
#[allow(clippy::too_many_arguments)]
async fn stage_question(
    state: &AppState,
    task: &AiParseTask,
    question_index: &str,
    mut parsed: ParsedQuestion,
    page_image_url: Option<&str>,
    paper_id: Option<Uuid>,
    collection_id: Option<Uuid>,
    is_mixed: bool,
    space_id: Uuid,
) -> Result<(), String> {
    // 幂等：同任务重跑时 index 已暂存 → 跳过
    let already: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM jsonb_array_elements(
                COALESCE(progress->'staged_questions', '[]'::jsonb)
            ) elem
            WHERE elem->>'index' = $2
        ) FROM ai_parse_tasks WHERE id = $1
        "#,
    )
    .bind(task.id)
    .bind(question_index)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| format!("暂存幂等检查失败: {e}"))?;
    if already {
        return Ok(());
    }

    // 先把「如图」缺失的配图写回题干（可能仍是外链），再下载转存，
    // 最后再解析一次以收集本地化后的 URL。
    let _ = resolve_question_images(&mut parsed, page_image_url);
    localize_external_images(&mut parsed, &state.upload_dir).await;
    let image_urls = resolve_question_images(&mut parsed, page_image_url);

    let options_json = parsed
        .options
        .as_ref()
        .map(|opts| serde_json::to_value(opts).unwrap_or(serde_json::Value::Null));
    let correct_answer_json = serde_json::to_value(&parsed.correct_answer)
        .map_err(|e| format!("序列化 correct_answer 失败: {e}"))?;

    // hash 去重（只读查询）：命中已有题目 → 暂存 existing_question_id，
    // 保存时前端提示"复用已有题目"而非重复创建
    let normalized_hash = compute_normalized_content_hash(
        &parsed.stem,
        options_json.as_ref(),
        &correct_answer_json,
    );
    let existing_question_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM questions WHERE normalized_content_hash = $1 LIMIT 1",
    )
    .bind(&normalized_hash)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| format!("题目查重失败: {e}"))?;
    if existing_question_id.is_some() {
        tracing::info!(
            "任务 {} 题目「{}」hash 命中已有题目，暂存复用标记",
            task.id,
            parsed.stem.chars().take(20).collect::<String>()
        );
    }

    // 五维标签：与编辑页「AI 智能打标」同一套 Content 提取 + 收敛。
    // 无文本模型时降级 Parsed 适配（测试环境）；打标失败不阻断暂存。
    let auth = AuthUser {
        id: task.creator_id,
        username: String::new(),
        role: "user".into(),
        global_role: "teacher".into(),
    };
    let text_resolved = resolve_ai_config(&auth, state, ModelKind::Text).await.ok();
    let text_provider = text_resolved
        .as_ref()
        .map(|(key, name, _, base)| create_provider(name, key, base));
    let text_model = text_resolved.as_ref().and_then(|(_, _, m, _)| m.clone());
    let has_text_model = text_provider.is_some();

    let mut policy = TaggingPolicy::default();
    policy.fail_on_persist = false;
    if !has_text_model {
        policy.run_llm_extract = false;
        policy.run_llm_converge = false;
    }
    let tagging_stage = tagging_stage_from_paper_meta(&task.paper_meta);
    let ctx = TaggingContext {
        user_id: task.creator_id,
        space_id: Some(space_id),
        question_id: None,
        source_task_id: Some(task.id),
        source_index: Some(question_index.to_string()),
        stage: Some(tagging_stage.clone()),
    };

    let tagging_input = if has_text_model {
        TaggingInput::Content {
            content: tagging_content_from_parsed(&parsed),
        }
    } else {
        TaggingInput::Parsed(Box::new(parsed.clone()))
    };

    let suggestion = match run_tagging(
        &state.pool,
        text_provider.as_deref(),
        text_model.as_deref(),
        tagging_input,
        &ctx,
        &policy,
    )
    .await
    {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!("任务 {} 打标失败（不影响暂存）: {:?}", task.id, e);
            None
        }
    };

    let (matched, unmatched, suggestion_id, engine_version, suggestion_value) =
        if let Some(s) = suggestion {
            let matched = s.compat_matched_nodes();
            let unmatched = serde_json::Value::Object(s.compat_unmatched_map());
            let sid = s.suggestion_id;
            let ver = s.engine_version.clone();
            let val = serde_json::to_value(&s).unwrap_or(serde_json::Value::Null);
            (matched, unmatched, sid, Some(ver), val)
        } else {
            (
                Vec::new(),
                serde_json::json!({}),
                None,
                None,
                serde_json::Value::Null,
            )
        };

    let staged_item = serde_json::json!({
        "index": question_index,
        "parsed": serde_json::to_value(&parsed).map_err(|e| format!("序列化暂存题目失败: {e}"))?,
        "images": image_urls,
        "page_image_url": page_image_url,
        "space_id": space_id,
        // 容器关联信息随暂存项保存，确认保存时由后端直接使用（不信任前端回传）
        "paper_id": paper_id,
        "collection_id": collection_id,
        "is_mixed": is_mixed,
        "existing_question_id": existing_question_id,
        "suggestion_id": suggestion_id,
        "engine_version": engine_version,
        "suggestion": suggestion_value,
        "tagging_stage": tagging_stage,
        "matched": matched,
        "unmatched": unmatched,
        "saved": false,
    });

    // 追加写入 progress.staged_questions（jsonb_set 增量更新，不碰 idempotency_map）
    sqlx::query(
        r#"
        UPDATE ai_parse_tasks
        SET progress = jsonb_set(
              progress,
              '{staged_questions}',
              COALESCE(progress->'staged_questions', '[]'::jsonb) || $2::jsonb
            ),
            heartbeat_at = NOW(), updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(task.id)
    .bind(staged_item)
    .execute(&state.pool)
    .await
    .map_err(|e| format!("写入暂存失败: {e}"))?;

    Ok(())
}

#[allow(dead_code)]
/// 未匹配标签 → tag_candidates（幂等，不阻塞题目落库；kind: chapter/knowledge/method/pattern）
/// 确认保存已改走 Finalizer；本函数仅保留给需要手工回填的路径。
pub(crate) async fn create_tag_candidates(
    state: &AppState,
    task_id: Uuid,
    question_id: Uuid,
    ai_confidence: f32,
    kind: &str,
    unmatched: &[String],
) {
    for name in unmatched {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let normalized = crate::util::normalize::normalize_text(name);
        let confidence = rust_decimal::Decimal::from_f32_retain(ai_confidence)
            .map(|d| d.max(rust_decimal::Decimal::ZERO))
            .unwrap_or(rust_decimal::Decimal::ZERO);
        let target_type = match kind {
            "method" | "core_competence" => "tag",
            _ => "knowledge_node",
        };
        let result = sqlx::query(
            r#"
            INSERT INTO tag_candidates (kind, target_type, raw_name, normalized_name, ai_confidence, match_score, source_task_id, source_question_id)
            VALUES ($1, $2, $3, $4, $5, 0, $6, $7)
            ON CONFLICT (source_task_id, source_question_id, normalized_name, kind)
                WHERE source_task_id IS NOT NULL
            DO NOTHING
            "#,
        )
        .bind(kind)
        .bind(target_type)
        .bind(name)
        .bind(&normalized)
        .bind(confidence)
        .bind(task_id)
        .bind(question_id)
        .execute(&state.pool)
        .await;

        match result {
            Ok(r) if r.rows_affected() > 0 => {
                tracing::info!("任务 {task_id} [{kind}]「{name}」未匹配 → 进入候选队列");
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("任务 {task_id} 写 tag_candidates 失败（不阻塞）: {e}");
            }
        }
    }
}

/// 关联容器（Paper / Collection）；Mixed 文档不自动关联（前端分组）
pub(crate) async fn link_to_container(
    state: &AppState,
    question_id: Uuid,
    paper_id: Option<Uuid>,
    collection_id: Option<Uuid>,
    is_mixed: bool,
    parsed: &ParsedQuestion,
) {
    let display_order = parsed.display_order.unwrap_or(0);
    if let Some(pid) = paper_id {
        if let Err(e) = sqlx::query(
            r#"
            INSERT INTO paper_questions (id, paper_id, question_id, sort_order, score, section, question_no, display_order, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
            ON CONFLICT (paper_id, question_id) DO NOTHING
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(pid)
        .bind(question_id)
        .bind(display_order)
        .bind(parsed.score.unwrap_or(0))
        .bind(Option::<&str>::None)
        .bind(parsed.question_no.as_deref())
        .bind(display_order)
        .execute(&state.pool)
        .await
        {
            tracing::warn!("关联 PaperQuestion 失败: {e}");
        }
    }
    if !is_mixed {
        if let Some(cid) = collection_id {
            if let Err(e) = link_question_to_collection(
                &state.pool,
                cid,
                question_id,
                parsed.question_no.as_deref(),
                display_order,
                parsed.score,
                None,
            )
            .await
            {
                tracing::warn!("关联 CollectionQuestion 失败: {e}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 进度 / 心跳 / 取消
// ---------------------------------------------------------------------------

async fn refresh_heartbeat(state: &AppState, task_id: Uuid) {
    let _ = sqlx::query(
        "UPDATE ai_parse_tasks SET heartbeat_at = NOW(), updated_at = NOW() WHERE id = $1",
    )
    .bind(task_id)
    .execute(&state.pool)
    .await;
}

async fn is_cancel_requested(state: &AppState, task_id: Uuid) -> bool {
    sqlx::query_scalar::<_, Option<chrono::DateTime<chrono::Utc>>>(
        "SELECT cancel_requested_at FROM ai_parse_tasks WHERE id = $1",
    )
    .bind(task_id)
    .fetch_one(&state.pool)
    .await
    .map(|v| v.is_some())
    .unwrap_or(false)
}

async fn set_current_question_no(state: &AppState, task_id: Uuid, question_no: &str) {
    let _ = sqlx::query(
        "UPDATE ai_parse_tasks SET current_question_no = $2, updated_at = NOW() WHERE id = $1",
    )
    .bind(task_id)
    .bind(question_no)
    .execute(&state.pool)
    .await;
}

/// 记录最近一次失败原因（页面级失败也写入，便于排查 failed 任务）
async fn set_last_error(state: &AppState, task_id: Uuid, message: &str) {
    let short: String = if message.chars().count() > 800 {
        format!("{}...", message.chars().take(800).collect::<String>())
    } else {
        message.to_string()
    };
    let _ = sqlx::query(
        "UPDATE ai_parse_tasks SET last_error = $2, updated_at = NOW() WHERE id = $1",
    )
    .bind(task_id)
    .bind(&short)
    .execute(&state.pool)
    .await;
}

/// 更新进度计数器（幂等信息已由 progress.staged_questions 的 index 承载）
async fn update_progress(
    state: &AppState,
    task_id: Uuid,
    page_no: i32,
    processed: i32,
    success: i32,
    failed: i32,
) {
    let _ = sqlx::query(
        r#"
        UPDATE ai_parse_tasks
        SET processed_count = $3, success_count = $4, failed_count = $5,
            current_page = $6, heartbeat_at = NOW(), updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(task_id)
    .bind(processed)
    .bind(success)
    .bind(failed)
    .bind(page_no)
    .execute(&state.pool)
    .await;
}

// ---------------------------------------------------------------------------
// PDF 直传路径：轮询进度映射（纯函数，供 ocr_pdf_async 接入后使用）
// ---------------------------------------------------------------------------

/// PDF 直传任务进度快照（映射自引擎轮询响应）
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PdfOcrProgress {
    /// 正在处理的页（1-based，≤ total_pages）
    pub current_page: i32,
    /// 已完成页数
    pub processed_count: i32,
}

/// 引擎轮询进度 → 任务进度字段的纯映射
///
/// 输入契约（与引擎层轮询响应对齐）：
/// - Doc2X `/api/v2/parse/status`：`data.progress` 为 0~100，类型不固定
///   （可能为数字或数字字符串），仅 processing 阶段有意义
/// - MinerU 云端 `/v4/extract-results/batch/{id}`：无逐页数值进度 → 调用方传 None
///
/// 映射规则：
/// - done = floor(percent × total_pages / 100)；processed_count = done
/// - current_page = min(done + 1, total_pages)（100% 时收敛到 total_pages）
/// - percent 越界 clamp 到 [0, 100]
/// - 单调不回退：轮询可能拿到过期值，任一字段低于上一轮 → 保持上一轮
/// - 无有效数值 / total_pages ≤ 0 → None（调用方仅刷心跳，不更新进度）
pub(crate) fn map_pdf_poll_progress(
    engine_progress: Option<&serde_json::Value>,
    total_pages: i32,
    prev: Option<PdfOcrProgress>,
) -> Option<PdfOcrProgress> {
    if total_pages <= 0 {
        return None;
    }
    let percent = parse_percent_value(engine_progress?)?.clamp(0.0, 100.0);

    let done = (percent * total_pages as f64 / 100.0).floor() as i32;
    let mapped = PdfOcrProgress {
        current_page: (done + 1).min(total_pages),
        processed_count: done,
    };

    // 单调性：任一字段回退 → 沿用上一轮
    match prev {
        Some(p) if mapped.current_page < p.current_page || mapped.processed_count < p.processed_count => Some(p),
        _ => Some(mapped),
    }
}

// ---------------------------------------------------------------------------
// Stage 3：PDF 直传快速路径（ocr_pdf_async 整档 → 切块 Stage2 → 逐题落库）
// ---------------------------------------------------------------------------

/// Stage 2 切块上限（字符数）：约 2~4 页一块，控制 LLM 上下文与输出截断风险
const STAGE2_CHUNK_MAX_CHARS: usize = 6000;

/// 快速路径心跳周期（租约 60s，页循环原路径每页心跳；此处固定 20s）
const FAST_PATH_HEARTBEAT_SECS: u64 = 20;

/// PDF 直传快速路径执行结果（计数语义与逐页路径一致）
struct FastPathOutcome {
    cancelled: bool,
    success_count: i32,
    failed_count: i32,
    processed_count: i32,
}

/// Stage 3 PDF 直传快速路径
///
/// Phase 1 整档 OCR：`ocr_pdf_async_with_progress` + select 轮转
/// （进度回调 → map_pdf_poll_progress 写库 / 定时心跳+取消检查 / 引擎完成）。
/// Phase 2 切块解析：全文 Markdown 按段落切块 → 文本模型 Stage2 → post_process_batch
/// → 逐题落库（幂等键 `c{chunk}_i{idx}`，跨块题目由 Stage 3b 组装合并）。
///
/// 失败语义：
/// - OCR 阶段失败 → Err（调用方降级逐页路径；此时尚未落任何题目，可安全重跑）
/// - 解析/落库阶段失败 → 计 failed 不降级（与逐页路径同语义，走 partial_success）
#[allow(clippy::too_many_arguments)]
async fn run_pdf_fast_path(
    state: &AppState,
    task: &AiParseTask,
    pdf_path: &std::path::Path,
    engine: &dyn OcrProvider,
    text_provider: &dyn AiProvider,
    text_model: Option<&str>,
    total_pages: i32,
    paper_id: Option<Uuid>,
    collection_id: Option<Uuid>,
    is_mixed: bool,
    space_id: Uuid,
    all_questions: &mut Vec<(String, ParsedQuestion)>,
) -> Result<FastPathOutcome, String> {
    let task_id = task.id;
    let pdf_bytes =
        tokio::fs::read(pdf_path).await.map_err(|e| format!("读取原始 PDF 失败: {e}"))?;
    tracing::info!(
        "任务 {task_id} 走 PDF 直传快速路径（{} 字节，引擎 {}）",
        pdf_bytes.len(),
        engine.id()
    );

    // ── Phase 1：整档 OCR（pin 引擎 future，select 并行处理进度/心跳/取消） ──
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u8>();
    let on_progress: PdfProgressCallback = std::sync::Arc::new(move |p| {
        let _ = tx.send(p);
    });
    let engine_fut = engine.ocr_pdf_async_with_progress(&pdf_bytes, &on_progress);
    tokio::pin!(engine_fut);

    let mut prev: Option<PdfOcrProgress> = None;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(FAST_PATH_HEARTBEAT_SECS));
    let markdown = loop {
        tokio::select! {
            // 引擎进度 → 单调映射 → 写库（update_progress 自带 heartbeat_at 刷新）
            Some(pct) = rx.recv() => {
                if let Some(mapped) =
                    map_pdf_poll_progress(Some(&serde_json::json!(pct)), total_pages, prev.take())
                {
                    prev = Some(mapped.clone());
                    update_progress(state, task_id, mapped.current_page, mapped.processed_count, 0, 0).await;
                }
            }
            // 无进度引擎（MinerU 云端）由定时分支保活；取消即返回（future drop 中断上传）
            _ = heartbeat.tick() => {
                refresh_heartbeat(state, task_id).await;
                if is_cancel_requested(state, task_id).await {
                    return Ok(FastPathOutcome {
                        cancelled: true,
                        success_count: 0,
                        failed_count: 0,
                        processed_count: 0,
                    });
                }
            }
            // OCR 完成 / 失败
            res = &mut engine_fut => {
                break res.map_err(|e| format!("PDF 直传 OCR 失败: {}", format_ocr_error(&e)))?;
            }
        }
    };

    // OCR 100%：页进度收敛到满页
    update_progress(state, task_id, total_pages, total_pages, 0, 0).await;

    // ── Phase 2：切块 Stage2 解析 → 逐题落库 ──
    let chunks = split_markdown_chunks(&markdown, STAGE2_CHUNK_MAX_CHARS);
    if chunks.is_empty() {
        return Err("PDF 直传 OCR 结果为空".into());
    }
    tracing::info!(
        "任务 {task_id} 全文 Markdown {} 字符 → {} 块解析",
        markdown.chars().count(),
        chunks.len()
    );

    let mut outcome = FastPathOutcome {
        cancelled: false,
        success_count: 0,
        failed_count: 0,
        processed_count: 0,
    };

    for (ci, chunk) in chunks.iter().enumerate() {
        refresh_heartbeat(state, task_id).await;
        if is_cancel_requested(state, task_id).await {
            outcome.cancelled = true;
            return Ok(outcome);
        }

        let raw_json = match text_provider
            .parse_text_with_prompt(chunk, &STAGE2_PARSE_FULL_PROMPT, text_model)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("第 {} 块解析失败: {}", ci + 1, map_ai_error_msg(&e));
                tracing::warn!("任务 {task_id} {msg}");
                outcome.failed_count += 1;
                outcome.processed_count += 1;
                set_last_error(state, task_id, &msg).await;
                continue;
            }
        };

        let mut chunk_questions = match post_process_batch(&raw_json, &state.pool).await {
            Ok(qs) => qs,
            Err((_, err)) => {
                let msg = format!("第 {} 块后处理失败: {}", ci + 1, err["error"]);
                tracing::warn!("任务 {task_id} {msg}");
                outcome.failed_count += 1;
                outcome.processed_count += 1;
                set_last_error(state, task_id, &msg).await;
                continue;
            }
        };
        // Stage2 常丢掉 ![图](url)；从本块 OCR Markdown 把配图划回对应题目
        assign_chunk_images(chunk, &mut chunk_questions);

        for (idx, q) in chunk_questions.into_iter().enumerate() {
            if idx % 5 == 0 {
                refresh_heartbeat(state, task_id).await;
                if is_cancel_requested(state, task_id).await {
                    outcome.cancelled = true;
                    return Ok(outcome);
                }
            }

            let question_index = format!("c{ci}_i{idx}");
            let qno = q.question_no.clone();
            outcome.processed_count += 1;
            all_questions.push((question_index.clone(), q.clone()));

            match stage_question(
                state,
                task,
                &question_index,
                q,
                None, // 快速路径无逐页图源；Doc2X/MinerU 真实图片 URL 走 image_urls
                paper_id,
                collection_id,
                is_mixed,
                space_id,
            )
            .await
            {
                Ok(()) => {
                    outcome.success_count += 1;
                    if let Some(no) = qno {
                        set_current_question_no(state, task_id, &no).await;
                    }
                    update_progress(
                        state,
                        task_id,
                        total_pages,
                        outcome.processed_count,
                        outcome.success_count,
                        outcome.failed_count,
                    )
                    .await;
                }
                Err(e) => {
                    tracing::warn!("任务 {task_id} 第 {question_index} 题暂存失败: {e}");
                    outcome.failed_count += 1;
                    update_progress(
                        state,
                        task_id,
                        total_pages,
                        outcome.processed_count,
                        outcome.success_count,
                        outcome.failed_count,
                    )
                    .await;
                }
            }
        }
    }

    Ok(outcome)
}

/// 全文 Markdown → Stage2 切块（纯函数）
///
/// 按 `\n\n` 段落聚合至 `max_chars`（字符数，CJK 安全）；单段超长时按字符边界硬切。
/// 切块边界截断的题目由 Stage 3b 跨页组装（merge_into）合并，语义与逐页路径一致。
fn split_markdown_chunks(md: &str, max_chars: usize) -> Vec<String> {
    if md.trim().is_empty() {
        return Vec::new();
    }
    if max_chars == 0 {
        return vec![md.trim().to_string()];
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut cur = String::new();
    for para in md.split("\n\n") {
        let plen = para.chars().count();
        if plen > max_chars {
            // 超长单段：先冲刷聚合块，再按字符边界硬切
            if !cur.trim().is_empty() {
                chunks.push(std::mem::take(&mut cur));
            }
            for seg in para.chars().collect::<Vec<char>>().chunks(max_chars) {
                chunks.push(seg.iter().collect());
            }
            continue;
        }
        let cur_len = cur.chars().count();
        if !cur.is_empty() && cur_len + plen + 2 > max_chars {
            chunks.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push_str("\n\n");
        }
        cur.push_str(para);
    }
    if !cur.trim().is_empty() {
        chunks.push(cur);
    }
    chunks
}

// ---------------------------------------------------------------------------
// Stage 3b：跨页组装（题号/顺序重排）
// ---------------------------------------------------------------------------

const ASSEMBLE_PROMPT: &str = r#"你是一个试卷题号整理助手。以下是整份资料按页解析出的题目清单（index 为页内标识）。部分题目可能是跨页拆分的同一道题（题干开头相同、内容被截断），需要合并；题号可能需要按试卷习惯重排。

输出严格 JSON（不要 markdown）：
{
  "items": [
    {"index": "p1_i0", "question_no": "1", "display_order": 1, "score": 8, "merge_into": null},
    {"index": "p2_i3", "question_no": "17(1)", "display_order": 17, "score": null, "merge_into": "p1_i2"}
  ]
}

规则：
1. items 数组包含全部 index，一个不多一个不少
2. question_no 为最终题号（无题号资料按顺序编号 1,2,3...）；display_order 为最终展示顺序
3. 跨页拆分的同一道题：保留先出现的 index，后出现的 merge_into 填先出现者的 index，并省略该 index 的 question_no/display_order
4. 无法判断时保持原题号与顺序，不要臆造"#;

/// 跨页组装：返回 (index → item) 映射
async fn assemble_question_order(
    provider: &dyn crate::ai::provider::AiProvider,
    model: Option<&str>,
    questions: &[(String, ParsedQuestion)],
) -> Result<Vec<(String, serde_json::Value)>, String> {
    let summaries: Vec<serde_json::Value> = questions
        .iter()
        .map(|(idx, q)| {
            serde_json::json!({
                "index": idx,
                "question_no": q.question_no,
                "stem_head": q.stem.chars().take(120).collect::<String>(),
            })
        })
        .collect();
    let payload = serde_json::json!({ "questions": summaries }).to_string();

    let raw = provider
        .parse_text_with_prompt(&payload, ASSEMBLE_PROMPT, model)
        .await
        .map_err(|e| map_ai_error_msg(&e))?;

    let value: serde_json::Value = clean_and_parse(&raw).map_err(|e| e.to_string())?;
    let items = value
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .ok_or_else(|| "组装结果缺少 items 数组".to_string())?;

    Ok(items
        .iter()
        .filter_map(|item| {
            item.get("index")
                .and_then(|i| i.as_str())
                .map(|idx| (idx.to_string(), item.clone()))
        })
        .collect())
}

/// 应用组装结果：更新暂存项的题号/顺序（确认保存时随落库写入容器关联表）
///
/// - 普通项：写入 `order = {question_no, display_order}`
/// - 跨页合并项（merge_into 非空）：写入 `merged_into = <目标 index>`，
///   前端将其内容并入目标题；保存接口跳过 merged_into 项不重复创建
async fn apply_question_order(
    state: &AppState,
    task_id: Uuid,
    mapping: &[(String, serde_json::Value)],
) {
    for (index, item) in mapping {
        let patch = if item
            .get("merge_into")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty())
        {
            let target = item.get("merge_into").and_then(|v| v.as_str()).unwrap_or("");
            serde_json::json!({ "merged_into": target })
        } else {
            serde_json::json!({
                "order": {
                    "question_no": item.get("question_no").and_then(|v| v.as_str()),
                    "display_order": item.get("display_order").and_then(|v| v.as_i64()),
                }
            })
        };

        let res = sqlx::query(
            r#"
            UPDATE ai_parse_tasks
            SET progress = jsonb_set(
                  progress,
                  '{staged_questions}',
                  (
                    SELECT COALESCE(jsonb_agg(
                      CASE WHEN elem->>'index' = $2 THEN elem || $3::jsonb ELSE elem END
                      ORDER BY ord
                    ), '[]'::jsonb)
                    FROM jsonb_array_elements(progress->'staged_questions')
                      WITH ORDINALITY AS t(elem, ord)
                  )
                ),
                updated_at = NOW()
            WHERE id = $1
              AND progress->'staged_questions' IS NOT NULL
            "#,
        )
        .bind(task_id)
        .bind(index)
        .bind(&patch)
        .execute(&state.pool)
        .await;

        if let Err(e) = res {
            tracing::warn!("任务 {task_id} 暂存项 {index} 应用组装结果失败: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// 错误映射
// ---------------------------------------------------------------------------

fn map_ai_error_msg(e: &AiError) -> String {
    match e {
        AiError::NoApiKey => "未配置 AI API Key".to_string(),
        AiError::Upstream(status, msg) => {
            let short = if msg.chars().count() > 300 {
                format!("{}...", msg.chars().take(300).collect::<String>())
            } else {
                msg.clone()
            };
            format!("AI 上游错误 (HTTP {status}): {short}")
        }
        AiError::Timeout => "AI 调用超时（120s）".to_string(),
    }
}

/// OcrError → 用户可读消息（OcrError 仅实现 Debug）
fn format_ocr_error(e: &OcrError) -> String {
    match e {
        OcrError::UnsupportedPdf => "OCR 引擎不支持 PDF 直传".to_string(),
        OcrError::NoApiKey => "未配置 OCR 引擎 API Key".to_string(),
        OcrError::Timeout => "OCR 引擎请求超时".to_string(),
        OcrError::Upstream(code, msg) => format!("OCR 引擎上游错误 (HTTP {code}): {msg}"),
    }
}

/// 单页识别 → 待 post_process_batch 的原始 JSON
///
/// - 单阶段（qwen_vl / auto / 未配置）：视觉模型 + 批量 OCR Prompt 直出 JSON（V2.1.1 原路径）
/// - 两阶段（doc2x / mineru）：引擎 OCR 出 Markdown → 文本模型 Stage 2 结构化；
///   引擎可恢复故障（NoApiKey / 超时 / 429 / 401 / 403 / 网络）时本页降级单阶段视觉路径，
///   不可恢复错误透传（计入 failed_count → partial_success）
async fn ocr_page_to_json(
    task_id: Uuid,
    image_b64: &str,
    engine: &dyn OcrProvider,
    two_stage: bool,
    vision_provider: &dyn AiProvider,
    vision_model: Option<&str>,
    text_provider: Option<(&dyn AiProvider, Option<&str>)>,
) -> Result<String, String> {
    if !two_stage {
        return vision_provider
            .parse_image_with_prompt(image_b64, &BATCH_IMAGE_OCR_FULL_PROMPT, vision_model)
            .await
            .map_err(|e| map_ai_error_msg(&e));
    }

    let markdown = match engine.ocr_image(image_b64).await {
        Ok(md) => md,
        Err(e) if should_fallback(&e) => {
                    tracing::warn!(
                        target: "ocr::engine_select",
                        task_id = %task_id,
                        from = engine.id(),
                        to = "qwen_vl",
                        "OCR 引擎 {} 可恢复故障（{}），本页降级视觉模型识别",
                        engine.id(),
                        format_ocr_error(&e)
                    );
            return vision_provider
                .parse_image_with_prompt(image_b64, &BATCH_IMAGE_OCR_FULL_PROMPT, vision_model)
                .await
                .map_err(|e| map_ai_error_msg(&e));
        }
        Err(e) => return Err(format_ocr_error(&e)),
    };

    let (tp, tm) =
        text_provider.ok_or_else(|| "文本模型未配置（两阶段解析需要）".to_string())?;
    tp.parse_text_with_prompt(&markdown, &STAGE2_PARSE_FULL_PROMPT, tm)
        .await
        .map_err(|e| map_ai_error_msg(&e))
}

// ---------------------------------------------------------------------------
// 测试（真实 DB，不依赖 LLM：stage_question / recover_stale_tasks 直测）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::types::{AnalysisMethod, ParsedAnswer};
    use crate::db;
    use serde_json::json;

    async fn test_state() -> Option<(AppState, Uuid)> {
        let database_url = crate::testing::database_url()?;
        let pool = db::create_pool(&database_url, 5).await;
        db::run_migrations(&pool).await;
        let state = AppState::new(
            pool,
            "test-secret".into(),
            24,
            crate::config::AiConfig::from_env(),
            "./uploads".to_string(),
        );
        Some((state, Uuid::new_v4()))
    }

    fn fake_task(user_id: Uuid, doc_id: Uuid) -> AiParseTask {
        let now = chrono::Utc::now();
        AiParseTask {
            id: Uuid::new_v4(),
            creator_id: user_id,
            raw_text: Some(String::new()),
            source_type: AiTaskSourceType::Text,
            image_b64: None,
            pdf_bytes: None,
            ocr_provider_override: None,
            status: AiTaskStatus::Pending,
            question_id: None,
            question_ids: None,
            error_message: None,
            created_at: now,
            updated_at: now,
            document_id: Some(doc_id),
            paper_meta: json!({ "document_type": "class_exercise" }),
            total_count: 0,
            processed_count: 0,
            success_count: 0,
            failed_count: 0,
            retry_count: 0,
            current_page: None,
            total_pages: Some(1),
            current_question_no: None,
            started_at: None,
            completed_at: None,
            last_error: None,
            progress: json!({ "idempotency_map": {} }),
            locked_at: None,
            worker_id: None,
            heartbeat_at: None,
            cancel_requested_at: None,
        }
    }

    fn fake_parsed(question_no: Option<&str>, stem: &str) -> ParsedQuestion {
        ParsedQuestion {
            question_type: "solution".into(),
            sub_type: None,
            difficulty: Some("medium".into()),
            stem: stem.into(),
            options: None,
            correct_answer: Some(ParsedAnswer::Solution {
                subs: vec![crate::ai::types::SubAnswer {
                    sub_id: 1,
                    content: "解：x=2".into(),
                }],
            }),
            analysis: vec![AnalysisMethod {
                title: "解法一".into(),
                content: "求导。".into(),
            }],
            knowledge_points: vec![],
            confidence: 0.9,
            warnings: vec![],
            image_placeholders: vec![],
            image_urls: vec![],
            kp_matches: vec![],
            question_no: question_no.map(|s| s.to_string()),
            display_order: Some(1),
            score: Some(8),
            chapter_path: vec![],
            solution_methods: vec![],
        }
    }

    // -----------------------------------------------------------------------
    // resolve_question_images：占位符 → 页面图 URL（图片不丢的关键链路）
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_images_replaces_stem_placeholders() {
        let mut p = fake_parsed(None, "函数图象如下：![配图](IMAGE_PLACEHOLDER_0)，求 f(2)。");
        p.image_placeholders = vec!["IMAGE_PLACEHOLDER_0".into()];
        let urls = resolve_question_images(&mut p, Some("/uploads/documents/d1/page_1.webp"));
        // 独立成行 → img-block 块级渲染（与既有题库图片布局一致）
        assert_eq!(
            p.stem,
            "函数图象如下：\n![配图](/uploads/documents/d1/page_1.webp)\n，求 f(2)。"
        );
        assert_eq!(urls, vec!["/uploads/documents/d1/page_1.webp".to_string()]);
    }

    #[test]
    fn test_resolve_images_placeholder_already_on_own_line() {
        // 占位符原本已独立成行：不产生多余空行
        let mut p = fake_parsed(None, "如图\n![配图](IMAGE_PLACEHOLDER_0)\n求 f(2)。");
        p.image_placeholders = vec!["IMAGE_PLACEHOLDER_0".into()];
        let _ = resolve_question_images(&mut p, Some("/uploads/documents/d1/page_1.webp"));
        assert_eq!(
            p.stem,
            "如图\n![配图](/uploads/documents/d1/page_1.webp)\n求 f(2)。"
        );
    }

    #[test]
    fn test_resolve_images_replaces_all_fields_multiple_placeholders() {
        let mut p = fake_parsed(None, "图1：![配图](IMAGE_PLACEHOLDER_0) 图2：![图](IMAGE_PLACEHOLDER_1)");
        p.options = Some(vec![crate::ai::types::ParsedOption {
            label: "A".into(),
            content: "选项图 ![o](IMAGE_PLACEHOLDER_2)".into(),
        }]);
        p.analysis[0].content = "解析图 ![a](IMAGE_PLACEHOLDER_3)".into();
        let url = "/uploads/documents/d2/page_3.webp";
        let urls = resolve_question_images(&mut p, Some(url));
        assert_eq!(p.stem.matches("IMAGE_PLACEHOLDER").count(), 0);
        assert_eq!(p.options.as_ref().unwrap()[0].content, format!("选项图 \n![配图]({url})\n"));
        assert_eq!(p.analysis[0].content, format!("解析图 \n![配图]({url})\n"));
        // 多个占位符 → 同一页面 URL 只记一次
        assert_eq!(urls, vec![url.to_string()]);
    }

    #[test]
    fn test_resolve_images_no_placeholder_keeps_text() {
        let mut p = fake_parsed(None, "纯文本题干 $x+1$");
        let urls = resolve_question_images(&mut p, Some("/uploads/documents/d1/page_1.webp"));
        assert_eq!(p.stem, "纯文本题干 $x+1$");
        assert!(urls.is_empty(), "无占位符不应写入页面图");
    }

    #[test]
    fn test_resolve_images_merges_real_urls_and_page() {
        // 两阶段路径：Stage2 已收集真实 URL（内联保留）+ 视觉占位符（替换）
        let mut p = fake_parsed(None, "如图 ![配图](IMAGE_PLACEHOLDER_0) 与 ![原图](https://cdn.example.com/fig1.png)");
        p.image_urls = vec!["https://cdn.example.com/fig1.png".into()];
        p.image_placeholders = vec!["IMAGE_PLACEHOLDER_0".into()];
        let urls = resolve_question_images(&mut p, Some("/uploads/documents/d9/page_2.webp"));
        // 真实 URL 内联不动
        assert!(p.stem.contains("https://cdn.example.com/fig1.png"));
        // images 列 = 真实 URL + 页面图，顺序稳定
        assert_eq!(
            urls,
            vec![
                "https://cdn.example.com/fig1.png".to_string(),
                "/uploads/documents/d9/page_2.webp".to_string()
            ]
        );
    }

    #[test]
    fn test_resolve_images_placeholder_without_page_source() {
        // 占位符但无页面图源：用已收集的真实 URL 替换占位符，避免预览丢图
        let mut p = fake_parsed(None, "如图 ![配图](IMAGE_PLACEHOLDER_0)");
        p.image_urls = vec!["https://cdn.example.com/x.png".into()];
        let urls = resolve_question_images(&mut p, None);
        assert!(!p.stem.contains("IMAGE_PLACEHOLDER_0"));
        assert!(p.stem.contains("https://cdn.example.com/x.png"));
        assert_eq!(urls, vec!["https://cdn.example.com/x.png".to_string()]);
    }

    #[test]
    fn test_resolve_images_injects_when_figure_mentioned_without_markdown() {
        let mut p = fake_parsed(
            None,
            "已知全集 $U=\\mathbb{R}$，如图阴影部分表示的集合是 ( )",
        );
        let url = "/uploads/documents/d1/page_1.webp";
        let urls = resolve_question_images(&mut p, Some(url));
        assert!(p.stem.contains(&format!("![配图]({url})")), "题干应注入页面配图: {}", p.stem);
        assert_eq!(urls, vec![url.to_string()]);
    }

    #[test]
    fn test_resolve_images_injects_harvested_url_on_pdf_path() {
        let mut p = fake_parsed(None, "如图所示，求阴影部分面积。");
        p.image_urls = vec!["https://cdn.example.com/venn.png".into()];
        let urls = resolve_question_images(&mut p, None);
        assert!(p.stem.contains("![配图](https://cdn.example.com/venn.png)"));
        assert_eq!(urls, vec!["https://cdn.example.com/venn.png".to_string()]);
    }

    #[test]
    fn test_assign_chunk_images_recovers_dropped_figure() {
        let md = "1. 已知全集，如图阴影\n![fig](https://cdn.example.com/a.png)\n2. 化简 $\\sqrt{12}$";
        let mut qs = vec![
            fake_parsed(Some("1"), "已知全集，如图阴影部分表示的集合是"),
            fake_parsed(Some("2"), "化简 $\\sqrt{12}-\\sqrt{3}$"),
        ];
        assign_chunk_images(md, &mut qs);
        assert_eq!(qs[0].image_urls, vec!["https://cdn.example.com/a.png".to_string()]);
        assert!(qs[1].image_urls.is_empty(), "纯文本题不应分到配图");
    }

    #[test]
    fn test_tagging_stage_from_paper_meta() {
        let senior = serde_json::json!({"paper_meta": {"stage": "高中"}});
        assert_eq!(tagging_stage_from_paper_meta(&senior), "senior");
        let junior = serde_json::json!({"collections": [{"stage": "junior"}]});
        assert_eq!(tagging_stage_from_paper_meta(&junior), "junior");
        assert_eq!(tagging_stage_from_paper_meta(&serde_json::json!({})), "senior");
    }

    #[tokio::test]
    async fn test_stage_question_stages_without_db_write() {
        let Some((state, user_id)) = test_state().await else {
            eprintln!("跳过：未配置 DATABASE_URL_TEST");
            return;
        };
        let pool = state.pool.clone();

        // 准备：用户 + 文档 + 集合 + 任务行（stage_question 仅 UPDATE ai_parse_tasks）
        let username = format!("wkr_{}", Uuid::new_v4().simple().to_string().get(..8).unwrap_or("x"));
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, email, role, global_role, display_name) VALUES ($1, $2, 'x', $3, 'user', 'teacher', '测试用户')",
        )
        .bind(user_id)
        .bind(&username)
        .bind(format!("{username}@test.com"))
        .execute(&pool)
        .await
        .expect("插入用户失败");

        let doc_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO documents (id, creator_id, file_name, page_count, status, document_type, title) VALUES ($1, $2, 't.pdf', 1, 'confirmed', 'class_exercise', '课堂练习')",
        )
        .bind(doc_id)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("插入文档失败");

        let collection_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO question_collections (id, document_id, creator_id, title, collection_type) VALUES ($1, $2, $3, '课堂练习', 'class_exercise')",
        )
        .bind(collection_id)
        .bind(doc_id)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("插入集合失败");

        let space_id = ensure_personal_space(&pool, user_id, "测试用户")
            .await
            .expect("创建个人空间失败");

        let task = fake_task(user_id, doc_id);
        sqlx::query(
            "INSERT INTO ai_parse_tasks (id, creator_id, raw_text, status, document_id, progress, created_at, updated_at) VALUES ($1, $2, '', 'pending', $3, '{}', NOW(), NOW())",
        )
        .bind(task.id)
        .bind(user_id)
        .bind(doc_id)
        .execute(&pool)
        .await
        .expect("插入任务失败");

        // 唯一题干标记（避免与库中历史数据串扰）
        let uid = Uuid::new_v4().simple().to_string();
        let stem = format!("已知 $f(x)=x^2$，求极值。{uid}");

        // 1. 暂存 → 不写 questions 表，只追加 progress.staged_questions
        stage_question(
            &state,
            &task,
            "p1_i0",
            fake_parsed(Some("1"), &stem),
            None, // 无页面图（单测）
            None,
            Some(collection_id),
            false,
            space_id,
        )
        .await
        .expect("stage 失败");

        let question_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM questions WHERE stem = $1")
            .bind(&stem)
            .fetch_one(&pool)
            .await
            .expect("统计题目失败");
        assert_eq!(question_rows, 0, "暂存阶段不应写 questions 表");

        let staged: serde_json::Value = sqlx::query_scalar(
            "SELECT progress->'staged_questions' FROM ai_parse_tasks WHERE id = $1",
        )
        .bind(task.id)
        .fetch_one(&pool)
        .await
        .expect("查询暂存失败");
        let arr = staged.as_array().expect("staged_questions 应为数组");
        assert_eq!(arr.len(), 1, "暂存应恰好 1 项");
        assert_eq!(arr[0]["index"], json!("p1_i0"));
        assert_eq!(arr[0]["collection_id"], json!(collection_id));
        assert_eq!(arr[0]["space_id"], json!(space_id));
        assert_eq!(arr[0]["saved"], json!(false));
        assert!(arr[0]["parsed"]["stem"].as_str().unwrap().contains(&uid));

        // 2. 同 index 重跑 → 幂等跳过（仍 1 项）
        stage_question(
            &state,
            &task,
            "p1_i0",
            fake_parsed(Some("1"), &stem),
            None,
            None,
            Some(collection_id),
            false,
            space_id,
        )
        .await
        .expect("幂等 stage 失败");
        let len: i32 = sqlx::query_scalar(
            "SELECT jsonb_array_length(progress->'staged_questions') FROM ai_parse_tasks WHERE id = $1",
        )
        .bind(task.id)
        .fetch_one(&pool)
        .await
        .expect("统计暂存失败");
        assert_eq!(len, 1, "同 index 重跑应幂等跳过");

        // 3. 不同 index 同内容 → 新增暂存项（不查重拦截，确认保存时再按 hash 去重）
        stage_question(
            &state,
            &task,
            "p1_i1",
            fake_parsed(Some("2"), &stem),
            None,
            None,
            Some(collection_id),
            false,
            space_id,
        )
        .await
        .expect("追加 stage 失败");
        let len2: i32 = sqlx::query_scalar(
            "SELECT jsonb_array_length(progress->'staged_questions') FROM ai_parse_tasks WHERE id = $1",
        )
        .bind(task.id)
        .fetch_one(&pool)
        .await
        .expect("统计暂存失败");
        assert_eq!(len2, 2, "不同 index 应各自暂存");
    }

    #[tokio::test]
    async fn test_recover_stale_tasks() {
        let Some((state, user_id)) = test_state().await else {
            eprintln!("跳过：未配置 DATABASE_URL_TEST");
            return;
        };
        let pool = state.pool.clone();

        // 准备用户（满足 creator_id FK）
        let user_id = Uuid::new_v4();
        let username = format!("wkr_{}", Uuid::new_v4().simple().to_string().get(..8).unwrap_or("x"));
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, email, role, global_role, display_name) VALUES ($1, $2, 'x', $3, 'user', 'teacher', '测试用户')",
        )
        .bind(user_id)
        .bind(&username)
        .bind(format!("{username}@test.com"))
        .execute(&pool)
        .await
        .expect("插入用户失败");

        let insert_task = async |retry_count: i32| -> Uuid {
            let id = Uuid::new_v4();
            sqlx::query(
                r#"
                INSERT INTO ai_parse_tasks (id, creator_id, raw_text, status, document_id, retry_count, heartbeat_at, created_at, updated_at, progress)
                VALUES ($1, $2, '', 'processing', NULL, $3, NOW() - INTERVAL '10 minutes', NOW(), NOW(), '{"idempotency_map": {}}')
                "#,
            )
            .bind(id)
            .bind(user_id)
            .bind(retry_count)
            .execute(&pool)
            .await
            .expect("插入任务失败");
            id
        };

        // 重试未耗尽（0）→ 重新入队 pending + retry_count+1
        let t1 = insert_task(0).await;
        recover_stale_tasks(&state, "test-worker").await;
        let (status, retry): (String, i32) = sqlx::query_as(
            "SELECT status::text, retry_count FROM ai_parse_tasks WHERE id = $1",
        )
        .bind(t1)
        .fetch_one(&pool)
        .await
        .expect("查询任务失败");
        assert_eq!(status, "pending", "僵尸任务应重新入队");
        assert_eq!(retry, 1);

        // 重试耗尽（2）→ failed
        let t2 = insert_task(2).await;
        recover_stale_tasks(&state, "test-worker").await;
        let (status, last_error): (String, Option<String>) = sqlx::query_as(
            "SELECT status::text, last_error FROM ai_parse_tasks WHERE id = $1",
        )
        .bind(t2)
        .fetch_one(&pool)
        .await
        .expect("查询任务失败");
        assert_eq!(status, "failed", "重试耗尽应标记失败");
        assert!(last_error.unwrap_or_default().contains("超时"));

        // 心跳正常（未超时）→ 不受影响
        let t3 = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO ai_parse_tasks (id, creator_id, raw_text, status, document_id, retry_count, heartbeat_at, created_at, updated_at, progress)
            VALUES ($1, $2, '', 'processing', NULL, 0, NOW(), NOW(), NOW(), '{"idempotency_map": {}}')
            "#,
        )
        .bind(t3)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("插入任务失败");
        recover_stale_tasks(&state, "test-worker").await;
        let status: String = sqlx::query_scalar("SELECT status::text FROM ai_parse_tasks WHERE id = $1")
            .bind(t3)
            .fetch_one(&pool)
            .await
            .expect("查询任务失败");
        assert_eq!(status, "processing", "心跳正常的任务不应被恢复");
    }

    #[tokio::test]
    async fn test_stage_question_captures_unmatched_labels() {
        let Some((state, user_id)) = test_state().await else {
            eprintln!("跳过：未配置 DATABASE_URL_TEST");
            return;
        };
        let pool = state.pool.clone();

        let username = format!("wkr_{}", Uuid::new_v4().simple().to_string().get(..8).unwrap_or("x"));
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, email, role, global_role, display_name) VALUES ($1, $2, 'x', $3, 'user', 'teacher', '测试用户')",
        )
        .bind(user_id)
        .bind(&username)
        .bind(format!("{username}@test.com"))
        .execute(&pool)
        .await
        .expect("插入用户失败");

        let space_id = ensure_personal_space(&pool, user_id, "测试用户")
            .await
            .expect("创建个人空间失败");
        let doc_id = Uuid::new_v4();
        let task = fake_task(user_id, doc_id);
        let collection_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO documents (id, creator_id, file_name, page_count, status, document_type, title) VALUES ($1, $2, 't.pdf', 1, 'confirmed', 'class_exercise', '课堂练习')",
        )
        .bind(doc_id)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("插入文档失败");
        // 任务行必须真实存在（暂存写入目标）
        sqlx::query(
            "INSERT INTO ai_parse_tasks (id, creator_id, raw_text, status, document_id, progress, created_at, updated_at) VALUES ($1, $2, '', 'pending', $3, '{}', NOW(), NOW())",
        )
        .bind(task.id)
        .bind(user_id)
        .bind(doc_id)
        .execute(&pool)
        .await
        .expect("插入任务失败");
        sqlx::query(
            "INSERT INTO question_collections (id, document_id, creator_id, title, collection_type) VALUES ($1, $2, $3, '课堂练习', 'class_exercise')",
        )
        .bind(collection_id)
        .bind(doc_id)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("插入集合失败");

        // 带未匹配知识点（保证知识库中不存在该名称）的题目
        let uid = Uuid::new_v4().simple().to_string();
        let mut parsed = fake_parsed(Some("1"), &format!("带未知知识点的题目题干{uid}"));
        parsed.knowledge_points = vec![format!("完全不存在的知识点XYZ_{uid}")];

        stage_question(
            &state,
            &task,
            "p1_i0",
            parsed,
            None, // 无页面图（单测）
            None,
            Some(collection_id),
            false,
            space_id,
        )
        .await
        .expect("stage 失败");

        // 未匹配标签随暂存项保存（确认保存时才写 tag_candidates）
        let staged: serde_json::Value = sqlx::query_scalar(
            "SELECT progress->'staged_questions'->0 FROM ai_parse_tasks WHERE id = $1",
        )
        .bind(task.id)
        .fetch_one(&pool)
        .await
        .expect("查询暂存失败");
        let unmatched_knowledge = staged
            .get("unmatched")
            .and_then(|u| u.get("knowledge"))
            .and_then(|k| k.as_array())
            .cloned()
            .unwrap_or_default();
        let planted = format!("完全不存在的知识点XYZ_{uid}");
        let captured_parsed_key = unmatched_knowledge
            .iter()
            .any(|n| n.as_str() == Some(planted.as_str()));
        if !captured_parsed_key {
            // 配置了文本模型时走与编辑页相同的 Content 提取，不再沿用 OCR knowledge_points
            assert!(
                staged.get("suggestion").is_some() && !staged.get("suggestion").unwrap().is_null(),
                "打标建议应写入暂存项：{staged}"
            );
        }

        // 暂存阶段不产生候选（延迟到确认保存）
        let candidates: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tag_candidates WHERE source_task_id = $1",
        )
        .bind(task.id)
        .fetch_one(&pool)
        .await
        .expect("查询候选失败");
        assert_eq!(candidates, 0, "暂存阶段不应写 tag_candidates");
    }

    // -----------------------------------------------------------------------
    // PDF 直传路径：map_pdf_poll_progress 纯映射（无需数据库）
    // -----------------------------------------------------------------------

    #[test]
    fn test_map_pdf_progress_basic_percent() {
        // 42% × 20 页 → floor(8.4)=8 页完成，正在第 9 页
        let p = map_pdf_poll_progress(Some(&json!(42)), 20, None).unwrap();
        assert_eq!((p.current_page, p.processed_count), (9, 8));
    }

    #[test]
    fn test_map_pdf_progress_number_string_value() {
        // Doc2X progress 为 serde_json::Value，实测可能返回数字字符串
        let p = map_pdf_poll_progress(Some(&json!("42")), 20, None).unwrap();
        assert_eq!((p.current_page, p.processed_count), (9, 8));
        // 带空白的字符串同样可解析
        let p2 = map_pdf_poll_progress(Some(&json!(" 55 ")), 20, None).unwrap();
        assert_eq!((p2.current_page, p2.processed_count), (12, 11));
    }

    #[test]
    fn test_map_pdf_progress_float_floor() {
        // 浮点进度向下取整：42.9% × 20 → 8.58 → 8
        let p = map_pdf_poll_progress(Some(&json!(42.9)), 20, None).unwrap();
        assert_eq!((p.current_page, p.processed_count), (9, 8));
    }

    #[test]
    fn test_map_pdf_progress_boundaries() {
        // 0% → 尚无完成页，正在第 1 页
        let zero = map_pdf_poll_progress(Some(&json!(0)), 20, None).unwrap();
        assert_eq!((zero.current_page, zero.processed_count), (1, 0));
        // 100% → 收敛到最后一页且全部完成
        let full = map_pdf_poll_progress(Some(&json!(100)), 20, None).unwrap();
        assert_eq!((full.current_page, full.processed_count), (20, 20));
    }

    #[test]
    fn test_map_pdf_progress_clamp_out_of_range() {
        // 越界百分比 clamp 到 [0,100]
        let over = map_pdf_poll_progress(Some(&json!(150)), 20, None).unwrap();
        assert_eq!((over.current_page, over.processed_count), (20, 20));
        let under = map_pdf_poll_progress(Some(&json!(-5)), 20, None).unwrap();
        assert_eq!((under.current_page, under.processed_count), (1, 0));
    }

    #[test]
    fn test_map_pdf_progress_no_numeric_progress_returns_none() {
        // MinerU 云端无数值进度 / Doc2X 未携带 progress 字段 → 本轮不更新（调用方仅刷心跳）
        assert!(map_pdf_poll_progress(None, 20, None).is_none());
        assert!(map_pdf_poll_progress(Some(&serde_json::Value::Null), 20, None).is_none());
    }

    #[test]
    fn test_map_pdf_progress_unparseable_value_returns_none() {
        // 布尔 / 对象 / 非数字字符串 → 无法解析，不更新
        assert!(map_pdf_poll_progress(Some(&json!(true)), 20, None).is_none());
        assert!(map_pdf_poll_progress(Some(&json!({"pages": 3})), 20, None).is_none());
        assert!(map_pdf_poll_progress(Some(&json!("processing")), 20, None).is_none());
    }

    #[test]
    fn test_map_pdf_progress_monotonic_no_regression() {
        // 轮询可能返回过期值：新映射 (9,8) 低于上一轮 (12,11) → 保持上一轮
        let prev = PdfOcrProgress { current_page: 12, processed_count: 11 };
        let p = map_pdf_poll_progress(Some(&json!(30)), 20, Some(prev.clone())).unwrap();
        assert_eq!((p.current_page, p.processed_count), (12, 11));
    }

    #[test]
    fn test_map_pdf_progress_monotonic_forward_ok() {
        // 正常前进：上一轮 (12,11)，75% → (16,15)
        let prev = PdfOcrProgress { current_page: 12, processed_count: 11 };
        let p = map_pdf_poll_progress(Some(&json!(75)), 20, Some(prev)).unwrap();
        assert_eq!((p.current_page, p.processed_count), (16, 15));
    }

    #[test]
    fn test_map_pdf_progress_equal_prev_accepted() {
        // 与上一轮相同（如引擎长时间停在 60%）→ 接受，不视为回退
        let prev = PdfOcrProgress { current_page: 13, processed_count: 12 };
        let p = map_pdf_poll_progress(Some(&json!(60)), 20, Some(prev)).unwrap();
        assert_eq!((p.current_page, p.processed_count), (13, 12));
    }

    #[test]
    fn test_map_pdf_progress_invalid_total_pages() {
        // total_pages ≤ 0 无法映射 → None
        assert!(map_pdf_poll_progress(Some(&json!(50)), 0, None).is_none());
        assert!(map_pdf_poll_progress(Some(&json!(50)), -1, None).is_none());
    }

    #[test]
    fn test_map_pdf_progress_single_page_document() {
        // 单页文档：done 只能为 0 或 1，current_page 恒为 1
        let early = map_pdf_poll_progress(Some(&json!(1)), 1, None).unwrap();
        assert_eq!((early.current_page, early.processed_count), (1, 0));
        let mid = map_pdf_poll_progress(Some(&json!(99)), 1, None).unwrap();
        assert_eq!((mid.current_page, mid.processed_count), (1, 0));
        let full = map_pdf_poll_progress(Some(&json!(100)), 1, None).unwrap();
        assert_eq!((full.current_page, full.processed_count), (1, 1));
    }

    // -----------------------------------------------------------------------
    // PDF 直传路径：split_markdown_chunks 纯切块（无需数据库）
    // -----------------------------------------------------------------------

    #[test]
    fn test_split_chunks_empty_input() {
        assert!(split_markdown_chunks("", 100).is_empty());
        assert!(split_markdown_chunks("   \n  ", 100).is_empty());
    }

    #[test]
    fn test_split_chunks_short_input_single_chunk() {
        let md = "第一题题干\n\nA. 选项\n\nB. 选项";
        let chunks = split_markdown_chunks(md, 100);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], md);
    }

    #[test]
    fn test_split_chunks_respects_limit_at_paragraph_boundary() {
        // 3 个 50 字符段落，上限 60 → 每块最多 1 个段落（50+2+50 > 60）
        let paras: Vec<String> = (0..3).map(|i| format!("段落{}：{}", i, "题".repeat(45))).collect();
        let md = paras.join("\n\n");
        let chunks = split_markdown_chunks(&md, 60);
        assert_eq!(chunks.len(), 3, "每段落应独立成块");
        for (i, c) in chunks.iter().enumerate() {
            assert!(c.chars().count() <= 60, "块 {i} 超出上限");
            assert!(c.contains(&format!("段落{i}")));
        }
        // 相邻段落可合并的场合（50+2+30 <= 82）
        let md2 = format!("{}\n\n{}", "题".repeat(50), "解".repeat(30));
        let chunks2 = split_markdown_chunks(&md2, 82);
        assert_eq!(chunks2.len(), 1, "段落合计未超限应合并为一块");
    }

    #[test]
    fn test_split_chunks_oversized_paragraph_hard_split() {
        // 单段 250 字符超过上限 100 → 按 100/100/50 硬切（字符边界，不 panic）
        let md = "题".repeat(250);
        let chunks = split_markdown_chunks(&md, 100);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].chars().count(), 100);
        assert_eq!(chunks[1].chars().count(), 100);
        assert_eq!(chunks[2].chars().count(), 50);
    }

    #[test]
    fn test_split_chunks_cjk_no_panics_and_content_preserved() {
        // 中英混排：无硬切时内容按字符计数无损重组（块间以 \n\n 还原）
        let md = "已知抛物线 $y=ax^2+bx+c$。\n\n求证：$a+b>c$。\n\nEnglish paragraph with formulas $x^2$.";
        let chunks = split_markdown_chunks(md, 40);
        assert!(chunks.len() >= 2);
        let rejoined = chunks.join("\n\n");
        // 所有段落均 ≤ 40，不应触发硬切 → 内容应完整保留
        assert_eq!(rejoined.replace("\n\n", "|"), md.replace("\n\n", "|"));
    }

    #[test]
    fn test_split_chunks_zero_limit_returns_whole() {
        // max_chars=0 视为不切块（防御，正常调用不会传 0）
        let chunks = split_markdown_chunks("任意内容", 0);
        assert_eq!(chunks, vec!["任意内容".to_string()]);
    }
}
