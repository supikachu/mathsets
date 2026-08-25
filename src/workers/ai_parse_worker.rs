//! V2.1.1 P0-C：AI 解析 Worker（计划书 §七）
//!
//! 全链路：Document → 容器（Paper / QuestionCollection）→ 逐页 OCR →
//! 跨页组装（可选）→ 逐题（hash 去重 → 建/复用 Question → 关联 + 知识点）→
//! 终态（cancelled > failed > partial_success > success）。
//!
//! 可靠性：
//! - Stage 0 原子认领（SKIP LOCKED + locked_at/worker_id/heartbeat_at）
//! - 租约 60s / 心跳 20s；僵尸任务（240s 无心跳）重新入队或 failed（须大于单次 LLM 超时）
//! - 幂等：progress.idempotency_map（question_index → question_id）+
//!   (paper_id,question_id)/(collection_id,question_id) 唯一索引 + 容器幂等键
//! - 取消：题间检查 cancel_requested_at，终态 cancelled，已落库题目保留（§6.4）
//! - 错误分类：不可重试（NoApiKey/数据缺失）→ failed；
//!   可重试（上游/超时/JSON）→ retrying（retry_count+1，≤2 次）

use std::collections::HashSet;
use std::time::Duration;
use uuid::Uuid;

use base64::Engine as _;

use crate::ai::cleaner::clean_and_parse;
use crate::ai::continuation::merge_split_questions;
use crate::ai::structure::{
    finalize_parsed_question, recover_chunk_questions, recover_parsed_questions,
    recover_question_sections, stage2_llm_input,
};
use crate::ai::layout::{
    exam_section_heading, is_implausible_major_no_drop, layout_sidecar_path, load_layout_sidecar,
    question_major_no, question_start_regex, rehome_trailing_exam_sections, split_question_chunks,
    LayoutDocument, LayoutSource,
};
use crate::ai::ocr::{
    create_ocr_provider, parse_percent_value, should_fallback, OcrError, OcrProvider,
    PdfProgressCallback,
};
use crate::ai::prompt::{BATCH_IMAGE_OCR_FULL_PROMPT, STAGE2_PARSE_FULL_PROMPT, STAGE2_PARSE_SLIM_PROMPT};
use crate::ai::provider::{
    create_provider, is_transient_openrouter_error, AiError, AiProvider,
    OPENROUTER_PROVIDER_ERROR_USER_MESSAGE, RATE_LIMIT_USER_MESSAGE,
};
use crate::ai::tagging::{
    content_input_hash_with_stage, run_tagging, tagging_content_from_parsed, TaggingContext,
    TaggingInput, TaggingPolicy,
};
use crate::ai::paper_order::{
    cmp_paper_order, infer_question_no_from_stem, parse_question_no_key, paper_order_key,
};
use crate::ai::types::{ParsedAnswer, ParsedQuestion};
use crate::auth::middleware::AuthUser;
use crate::auth::permissions::ensure_personal_space;
use crate::handlers::ai::{post_process_batch, resolve_ai_config, resolve_ocr_config, ModelKind};
use crate::handlers::collections::{get_or_create_collection, link_question_to_collection};
use crate::models::ai_task::{AiParseTask, AiTaskSourceType, AiTaskStatus};
use crate::models::document::is_paper_type;
use crate::util::normalize::compute_normalized_content_hash_ex;
use crate::AppState;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 僵尸任务判定：须大于 DeepSeekProvider 单次超时（180s），避免切块解析中被误回收
const HEARTBEAT_TIMEOUT: &str = "240 seconds";
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
    abandon_orphaned_processing(&state, &worker_id).await;

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

/// 进程启动时：上一次崩溃/关停留下的 processing 不再续跑
async fn abandon_orphaned_processing(state: &AppState, worker_id: &str) {
    let n = sqlx::query(
        r#"
        UPDATE ai_parse_tasks
        SET status = 'cancelled',
            last_error = '后台进程已停止，任务已终止（不会自动续跑）',
            completed_at = NOW(), locked_at = NULL, worker_id = NULL, updated_at = NOW()
        WHERE status = 'processing'
        "#,
    )
    .execute(&state.pool)
    .await
    .map(|r| r.rows_affected())
    .unwrap_or(0);
    if n > 0 {
        tracing::warn!("Worker {worker_id} 终止上一次遗留 processing 任务 {n} 个");
    }
}

/// 取消请求立即落 cancelled（未拾取的任务）；卡住的 processing 终止而不是重新入队
async fn recover_stale_tasks(state: &AppState, worker_id: &str) {
    let cancelled = sqlx::query(
        r#"
        UPDATE ai_parse_tasks
        SET status = 'cancelled',
            last_error = COALESCE(last_error, '任务已取消'),
            completed_at = NOW(), locked_at = NULL, worker_id = NULL, updated_at = NOW()
        WHERE cancel_requested_at IS NOT NULL
          AND status IN ('pending', 'retrying')
        "#,
    )
    .execute(&state.pool)
    .await
    .map(|r| r.rows_affected())
    .unwrap_or(0);

    let stale = sqlx::query(&format!(
        r#"
        UPDATE ai_parse_tasks
        SET status = 'cancelled',
            last_error = '任务处理中断（超时或进程退出），未自动续跑',
            completed_at = NOW(), locked_at = NULL, worker_id = NULL, updated_at = NOW()
        WHERE status = 'processing'
          AND heartbeat_at < NOW() - INTERVAL '{HEARTBEAT_TIMEOUT}'
        "#
    ))
    .execute(&state.pool)
    .await
    .map(|r| r.rows_affected())
    .unwrap_or(0);

    if cancelled > 0 || stale > 0 {
        tracing::warn!("Worker {worker_id} 清理任务：cancelled={cancelled} stale={stale}");
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
              AND cancel_requested_at IS NULL
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

    let heartbeat = spawn_parse_heartbeat(state.pool.clone(), task_id);
    let outcome = execute_task(state, &task).await;
    heartbeat.abort();

    match outcome {
        Ok(TaskOutcome::Terminal(status)) => {
            // ⚠️ 必须参数化 bind（Rust enum → PG enum）。
            // 曾用 serde_json::to_string 拼 SQL，产生 "failed"（双引号）被 PG 当作
            // 标识符列名解析 → "字段 failed 不存在"，导致所有任务无法落终态。
            sqlx::query(
                r#"
                UPDATE ai_parse_tasks
                SET status = $1::ai_task_status, completed_at = NOW(), updated_at = NOW()
                WHERE id = $2 AND status = 'processing'
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
                    WHERE id = $2 AND status = 'processing'
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
                    WHERE id = $2 AND status = 'processing'
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
    if is_cancel_requested(state, task_id).await {
        return Ok(TaskOutcome::Terminal(AiTaskStatus::Cancelled));
    }

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
    // OCR 先行：uploaded/classifying/classified/confirmed 均可执行
    const ALLOWED: &[&str] = &["uploaded", "classifying", "classified", "confirmed", "parsing"];
    if !ALLOWED.contains(&doc_status.as_str()) {
        return Err(TaskFailure {
            retryable: false,
            message: format!("Document 状态不允许解析: {doc_status}"),
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
    let mut ocr_cfg = resolve_ocr_config(&auth, state, task.ocr_provider_override.as_deref())
        .await
        .map_err(|e| TaskFailure { retryable: false, message: e })?;
    ocr_cfg.task_id = Some(task.id);
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
    let stage2_n = {
        let tm = text_provider.as_ref().and_then(|(_, m)| m.as_deref());
        let user_n = load_user_stage2_concurrency(&state.pool, task.creator_id).await;
        stage2_concurrency(tm, user_n)
    };

    // 个人空间
    let space_id = ensure_personal_space(
        &state.pool,
        task.creator_id,
        display_name.as_deref().unwrap_or("用户"),
    )
    .await
    .map_err(|e| TaskFailure { retryable: false, message: format!("创建个人空间失败: {e}") })?;

    // ── Stage 2：容器（仅 create_paper=true 时建 Paper；默认独立题不建集合） ──
    let pm = &task.paper_meta;
    // 优先读文档最新 metadata（confirm 可能在解析中后置）
    let live_create = doc_metadata
        .get("create_paper")
        .and_then(|v| v.as_bool())
        .or_else(|| pm.get("create_paper").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    let source_category = doc_metadata
        .get("source_category")
        .and_then(|v| v.as_str())
        .or_else(|| pm.get("source_category").and_then(|v| v.as_str()))
        .unwrap_or("practice");
    let document_type = pm
        .get("document_type")
        .and_then(|v| v.as_str())
        .or(doc_type.as_deref())
        .unwrap_or("practice:in_class")
        .to_string();
    let create_paper = crate::models::document::should_create_paper(source_category, live_create)
        || (live_create && is_paper_type(&document_type));
    let is_mixed = false; // 方案 A：不再支持 mixed 多集合

    let (paper_id, collection_ids): (Option<Uuid>, Vec<Uuid>) = if create_paper {
        // Paper：显式关联 > document_id 幂等复用 > 新建
        let paper_meta_src = doc_metadata
            .get("paper_meta")
            .cloned()
            .or_else(|| pm.get("paper_meta").cloned())
            .unwrap_or(serde_json::Value::Null);
        let explicit_paper: Option<Uuid> = paper_meta_src
            .get("paper_id")
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
                Some(pid) => {
                    // 已有卷：用最新表单补齐空字段（confirm 可能后置）
                    let mut snap = pm.clone();
                    if let Some(obj) = snap.as_object_mut() {
                        obj.insert("paper_meta".into(), paper_meta_src);
                    }
                    let _ = create_paper_from_meta(state, &auth, doc_id, &snap).await;
                    (Some(pid), vec![])
                }
                None => {
                    // 合并 live paper_meta 到快照
                    let mut snap = pm.clone();
                    if let Some(obj) = snap.as_object_mut() {
                        obj.insert("paper_meta".into(), paper_meta_src);
                    }
                    let pid = create_paper_from_meta(state, &auth, doc_id, &snap).await?;
                    (Some(pid), vec![])
                }
            }
        }
    } else {
        // 独立题：不建 Paper / Collection
        (None, vec![])
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
    let mut ocr_export_done = false;

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
            stage2_n,
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
                ocr_export_done = outcome.ocr_export;
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
                    stage2_n,
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
                        ocr_export_done = outcome.ocr_export;
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

    if is_ocr_export(task) && !fast_path_done {
        match run_page_ocr_export(
            state,
            task,
            &page_files,
            doc_id,
            ocr_engine.as_ref(),
            space_id,
            paper_id,
            collection_ids.first().copied(),
            is_mixed,
        )
        .await
        {
            Ok(outcome) => {
                cancelled = outcome.cancelled;
                ocr_export_done = !outcome.cancelled;
                fast_path_done = true;
            }
            Err(msg) => {
                return Err(TaskFailure {
                    retryable: false,
                    message: msg,
                });
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

        let raw_json = match tokio::select! {
            r = ocr_page_to_json(
                task_id,
                &image_b64,
                ocr_engine.as_ref(),
                two_stage,
                vision_provider.as_ref(),
                vision_model.as_deref(),
                text_provider
                    .as_ref()
                    .map(|(p, m)| (p.as_ref() as &dyn AiProvider, m.as_deref())),
            ) => r,
            _ = wait_until_cancel(state, task_id) => {
                cancelled = true;
                break;
            }
        } {
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

        let mut page_questions = match post_process_batch(&raw_json, &state.pool, true).await {
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

        for q in &mut page_questions {
            let own = q.stem.clone();
            recover_question_sections(q, &own);
        }

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
    } else if ocr_export_done {
        AiTaskStatus::Success
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
    let meta_val = pm.get("paper_meta").cloned().unwrap_or(serde_json::Value::Null);
    let mut meta: crate::models::document::PaperMetaInput = serde_json::from_value(meta_val)
        .map_err(|_| TaskFailure {
            retryable: false,
            message: "试卷元数据缺少 title".into(),
        })?;
    if meta.title.trim().is_empty() {
        return Err(TaskFailure {
            retryable: false,
            message: "试卷元数据缺少 title".into(),
        });
    }
    let source_kind = pm
        .get("source_kind")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if meta.source_type.as_deref().map(str::trim).unwrap_or("").is_empty() {
        meta.source_type = if source_kind.is_empty() {
            None
        } else {
            Some(source_kind.to_string())
        };
    }

    crate::handlers::documents::sync_paper_for_document(
        &state.pool,
        auth.id,
        doc_id,
        &meta,
        source_kind,
    )
    .await
    .map_err(|e| TaskFailure {
        retryable: false,
        message: format!("创建试卷失败: {e}"),
    })
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
    parsed.visit_strings(|t| {
        for cap in re.captures_iter(t) {
            let url = cap[1].to_string();
            if !externals.contains(&url) {
                externals.push(url);
            }
        }
    });
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
    parsed.visit_strings_mut(|s| {
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
    });
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
    let mut s = String::new();
    parsed.visit_strings(|t| {
        if !t.is_empty() {
            s.push_str(t);
            s.push('\n');
        }
    });
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

    parsed.visit_strings_mut(sub);
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
    let mut hit = false;
    parsed.visit_strings(|t| {
        if t.contains("IMAGE_PLACEHOLDER") {
            hit = true;
        }
    });
    hit
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
/// 显式「丢弃」走 `POST /ai/parse-task/{id}/clear-staged`（只删未保存项）；
/// 从未丢弃的暂存项由 GC（72h）兜底清理。
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

    finalize_parsed_question(&mut parsed);
    parsed.ensure_solution_parts();
    let options_json = parsed
        .options
        .as_ref()
        .map(|opts| serde_json::to_value(opts).unwrap_or(serde_json::Value::Null));
    let correct_answer_json = serde_json::to_value(&parsed.correct_answer)
        .map_err(|e| format!("序列化 correct_answer 失败: {e}"))?;
    let structure_json = parsed.structure_json();

    // hash 去重（只读查询）：命中已有题目 → 暂存 existing_question_id，
    // 保存时前端提示"复用已有题目"而非重复创建
    let normalized_hash = compute_normalized_content_hash_ex(
        &parsed.stem,
        options_json.as_ref(),
        &correct_answer_json,
        structure_json.as_ref(),
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
    let text_resolved = resolve_ai_config(&auth, state, ModelKind::Tagging).await.ok();
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

    // 站外结构化：导入后暂不入队，等用户在「智能打标」里点开始。
    // 无文本模型时仍走同步 Parsed 适配（测试环境 / 未配置打标模型）。
    let defer_async_tagging = is_ocr_export(task);
    let (matched, unmatched, suggestion_id, engine_version, suggestion_value, tagging_status) =
        if has_text_model && defer_async_tagging {
            (
                Vec::new(),
                serde_json::json!({}),
                None,
                None,
                serde_json::Value::Null,
                "idle",
            )
        } else if has_text_model {
            let content = tagging_content_from_parsed(&parsed);
            // 解析阶段已产出知识点 / 章节 / 解法，一并带上让打标复用，省掉重复的 LLM 提取
            let signals = serde_json::to_value(crate::ai::tagging::signals_from_parsed(&parsed)).ok();
            enqueue_staged_tagging(
                state,
                task,
                question_index,
                &content,
                space_id,
                &tagging_stage,
                signals,
            )
            .await;
            (
                Vec::new(),
                serde_json::json!({}),
                None,
                None,
                serde_json::Value::Null,
                "pending",
            )
        } else {
            let tagging_input = TaggingInput::Parsed(Box::new(parsed.clone()));
            match run_tagging(
                &state.pool,
                text_provider.as_deref(),
                text_model.as_deref(),
                tagging_input,
                &ctx,
                &policy,
            )
            .await
            {
                Ok(s) => {
                    let matched = s.compat_matched_nodes();
                    let unmatched = serde_json::Value::Object(s.compat_unmatched_map());
                    let sid = s.suggestion_id;
                    let ver = s.engine_version.clone();
                    let val = serde_json::to_value(&s).unwrap_or(serde_json::Value::Null);
                    (matched, unmatched, sid, Some(ver), val, "done")
                }
                Err(e) => {
                    tracing::warn!("任务 {} 打标失败（不影响暂存）: {:?}", task.id, e);
                    (
                        Vec::new(),
                        serde_json::json!({}),
                        None,
                        None,
                        serde_json::Value::Null,
                        "failed",
                    )
                }
            }
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
        "tagging_status": tagging_status,
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
              COALESCE(progress->'staged_questions', '[]'::jsonb) || jsonb_build_array($2::jsonb)
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

/// 解析完成入队打标（不占打标日额度；进行中任务按 hash 幂等复用）
///
/// 返回是否已在队列中（新插入或 inflight 幂等命中）。
async fn enqueue_staged_tagging(
    state: &AppState,
    task: &AiParseTask,
    question_index: &str,
    content: &str,
    space_id: Uuid,
    stage: &str,
    parsed_signals: Option<serde_json::Value>,
) -> bool {
    let content = content.trim();
    if content.is_empty() {
        return false;
    }
    let input_hash = content_input_hash_with_stage(content, Some(stage));
    let inserted = sqlx::query(
        r#"
        INSERT INTO ai_tagging_tasks (
            creator_id, space_id, question_id, input_hash, content, stage, status,
            parse_task_id, source_index, parsed_signals
        )
        VALUES ($1, $2, NULL, $3, $4, $5, 'pending', $6, $7, $8)
        "#,
    )
    .bind(task.creator_id)
    .bind((!space_id.is_nil()).then_some(space_id))
    .bind(&input_hash)
    .bind(content)
    .bind(stage)
    .bind(task.id)
    .bind(question_index)
    .bind(parsed_signals)
    .execute(&state.pool)
    .await;

    match inserted {
        Ok(_) => {
            tracing::info!(
                "任务 {} 题目 {question_index} 已入队异步打标",
                task.id
            );
            true
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("unique")
                || msg.contains("duplicate")
                || msg.contains("idx_ai_tagging_tasks_inflight")
            {
                tracing::debug!("任务 {} 题目 {question_index} 打标任务已在队列中", task.id);
                true
            } else {
                tracing::warn!("任务 {} 题目 {question_index} 入队打标失败（不阻断暂存）: {e}", task.id);
                false
            }
        }
    }
}

fn staged_space_id(item: &serde_json::Value, task: &AiParseTask) -> Uuid {
    item.get("space_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .or_else(|| {
            task.progress
                .get("ocr_export_ctx")
                .and_then(|c| c.get("space_id"))
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())
        })
        .unwrap_or(Uuid::nil())
}

fn staged_item_saved(item: &serde_json::Value) -> bool {
    item.get("saved")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

async fn patch_staged_tagging_status(
    pool: &sqlx::PgPool,
    parse_id: Uuid,
    index: &str,
    status: &str,
) -> Result<(), String> {
    let patch = serde_json::json!({ "tagging_status": status });
    sqlx::query(
        r#"
        UPDATE ai_parse_tasks
        SET progress = jsonb_set(
              progress,
              '{staged_questions}',
              COALESCE((
                SELECT jsonb_agg(
                    elem || CASE WHEN elem->>'index' = $2 THEN $3::jsonb ELSE '{}'::jsonb END
                    ORDER BY ord
                )
                FROM jsonb_array_elements(COALESCE(progress->'staged_questions', '[]'::jsonb))
                    WITH ORDINALITY AS t(elem, ord)
              ), '[]'::jsonb)
            ),
            updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(parse_id)
    .bind(index)
    .bind(&patch)
    .execute(pool)
    .await
    .map_err(|e| format!("回写暂存打标状态失败: {e}"))?;
    Ok(())
}

/// 用户停止打标 / 离开录入：未完成的暂存项从 pending 回到 idle，便于再次开始。
pub(crate) async fn reset_pending_staged_tagging(
    pool: &sqlx::PgPool,
    parse_task_id: Uuid,
) -> Result<u64, String> {
    let result = sqlx::query(
        r#"
        UPDATE ai_parse_tasks
        SET progress = jsonb_set(
              progress,
              '{staged_questions}',
              COALESCE((
                SELECT jsonb_agg(
                    CASE
                      WHEN elem->>'tagging_status' = 'pending'
                       AND COALESCE(elem->>'saved', 'false') <> 'true'
                      THEN elem || '{"tagging_status":"idle"}'::jsonb
                      ELSE elem
                    END
                    ORDER BY ord
                )
                FROM jsonb_array_elements(COALESCE(progress->'staged_questions', '[]'::jsonb))
                    WITH ORDINALITY AS t(elem, ord)
              ), '[]'::jsonb)
            ),
            updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(parse_task_id)
    .execute(pool)
    .await
    .map_err(|e| format!("重置暂存打标状态失败: {e}"))?;
    Ok(result.rows_affected())
}

/// 丢弃未确认的暂存题：保留 `saved=true` 的项，并终止该解析任务下未完成的打标。
pub(crate) async fn clear_unsaved_staged_questions(
    pool: &sqlx::PgPool,
    parse_task_id: Uuid,
) -> Result<(usize, usize), String> {
    let existing_json: serde_json::Value = sqlx::query_scalar(
        "SELECT COALESCE(progress->'staged_questions', '[]'::jsonb) FROM ai_parse_tasks WHERE id = $1",
    )
    .bind(parse_task_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("读取暂存失败: {e}"))?;
    let existing: Vec<serde_json::Value> = match existing_json {
        serde_json::Value::Array(items) => items,
        _ => Vec::new(),
    };
    let kept: Vec<serde_json::Value> = existing
        .iter()
        .filter(|item| staged_item_saved(item))
        .cloned()
        .collect();
    let kept_count = kept.len();
    let removed = existing.len().saturating_sub(kept_count);

    sqlx::query(
        r#"
        UPDATE ai_parse_tasks
        SET progress = jsonb_set(
              COALESCE(progress, '{}'::jsonb),
              '{staged_questions}',
              $2::jsonb
            ),
            updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(parse_task_id)
    .bind(serde_json::Value::Array(kept))
    .execute(pool)
    .await
    .map_err(|e| format!("清空暂存失败: {e}"))?;

    if kept_count == 0 {
        let row: Option<(Option<Uuid>, Uuid)> = sqlx::query_as(
            "SELECT document_id, creator_id FROM ai_parse_tasks WHERE id = $1",
        )
        .bind(parse_task_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("查询解析任务失败: {e}"))?;
        if let Some((Some(doc_id), creator_id)) = row {
            crate::handlers::documents::delete_empty_draft_paper_for_document(
                pool, doc_id, creator_id,
            )
            .await
            .map_err(|e| format!("删除空草稿试卷失败: {e}"))?;
        }
    }

    let _ = sqlx::query(
        r#"
        UPDATE ai_tagging_tasks
        SET status = 'cancelled',
            error_message = COALESCE(error_message, '用户丢弃暂存题目'),
            completed_at = NOW(), updated_at = NOW(),
            locked_at = NULL, worker_id = NULL
        WHERE parse_task_id = $1
          AND status IN ('pending', 'retrying', 'queued')
        "#,
    )
    .bind(parse_task_id)
    .execute(pool)
    .await;

    let _ = sqlx::query(
        r#"
        UPDATE ai_tagging_tasks
        SET cancel_requested_at = NOW(), updated_at = NOW()
        WHERE parse_task_id = $1
          AND status = 'processing'
          AND cancel_requested_at IS NULL
        "#,
    )
    .bind(parse_task_id)
    .execute(pool)
    .await;

    Ok((removed, kept_count))
}

/// 用户点击「开始打标」后，为 idle/failed 的未保存暂存项入队。
///
/// 返回 (started, skipped)。
pub(crate) async fn start_staged_tagging(
    state: &AppState,
    parse_task_id: Uuid,
) -> Result<(u32, u32), String> {
    let task: AiParseTask = sqlx::query_as(&format!(
        "SELECT {TASK_COLUMNS} FROM ai_parse_tasks WHERE id = $1"
    ))
    .bind(parse_task_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| format!("查询解析任务失败: {e}"))?
    .ok_or_else(|| "解析任务不存在".to_string())?;

    let staged_json: serde_json::Value = sqlx::query_scalar(
        "SELECT COALESCE(progress->'staged_questions', '[]'::jsonb) FROM ai_parse_tasks WHERE id = $1",
    )
    .bind(parse_task_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| format!("读取暂存失败: {e}"))?;
    let items = match staged_json {
        serde_json::Value::Array(items) => items,
        _ => Vec::new(),
    };

    let default_stage = tagging_stage_from_paper_meta(&task.paper_meta);
    let mut started = 0u32;
    let mut skipped = 0u32;

    for item in items {
        let index = match item.get("index").and_then(|v| v.as_str()) {
            Some(i) if !i.is_empty() => i.to_string(),
            _ => {
                skipped += 1;
                continue;
            }
        };
        if staged_item_saved(&item) {
            skipped += 1;
            continue;
        }
        let status = item
            .get("tagging_status")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if status == "pending" || status == "done" {
            skipped += 1;
            continue;
        }

        let parsed: ParsedQuestion = match serde_json::from_value(
            item.get("parsed").cloned().unwrap_or(serde_json::Value::Null),
        ) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    parse_task_id = %parse_task_id,
                    index,
                    "开始打标时解析暂存题失败: {e}"
                );
                skipped += 1;
                continue;
            }
        };
        let content = tagging_content_from_parsed(&parsed);
        let signals = serde_json::to_value(crate::ai::tagging::signals_from_parsed(&parsed)).ok();
        let stage = item
            .get("tagging_stage")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(&default_stage);
        let space_id = staged_space_id(&item, &task);
        if enqueue_staged_tagging(state, &task, &index, &content, space_id, stage, signals).await {
            if let Err(e) =
                patch_staged_tagging_status(&state.pool, parse_task_id, &index, "pending").await
            {
                tracing::warn!(
                    parse_task_id = %parse_task_id,
                    index,
                    "入队后回写 pending 失败: {e}"
                );
            }
            started += 1;
        } else {
            skipped += 1;
        }
    }

    Ok((started, skipped))
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

fn spawn_parse_heartbeat(pool: sqlx::PgPool, task_id: Uuid) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(20)).await;
            let _ = sqlx::query(
                "UPDATE ai_parse_tasks SET heartbeat_at = NOW(), updated_at = NOW() \
                 WHERE id = $1 AND status = 'processing' AND cancel_requested_at IS NULL",
            )
            .bind(task_id)
            .execute(&pool)
            .await;
        }
    })
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
/// 只识别出一道大题时不再按字数横切（避免把长解析切成无题干残片）
const STAGE2_SINGLE_QUESTION_MAX_CHARS: usize = 24000;
/// Stage2 同时解析的切块数（暂存仍串行，避免 jsonb 追加丢题）
const STAGE2_CONCURRENCY: usize = 4;

/// 明确已知限流很紧的档位：智谱、Gemini 免费档、OpenRouter `:free` 免费模型。
///
/// 早先把所有含 `/` 的 OpenRouter `vendor/model` ID 一律压到 1，但付费 OpenRouter 模型
/// 并没有那么紧——实测 `stealth/ox-alpha` 连续 50 次调用只撞到 1 次 429，串行反而让
/// 10 块排队 13.8 分钟。改为只对确知的免费档降档。
fn stage2_concurrency_for(text_model: Option<&str>, override_n: Option<usize>) -> usize {
    if let Some(n) = override_n {
        return n.clamp(1, 16);
    }
    let m = text_model.unwrap_or("").to_ascii_lowercase();
    if m.contains("glm") || m.contains("gemini") || m.contains(":free") {
        1
    } else {
        STAGE2_CONCURRENCY
    }
}

fn stage2_concurrency(text_model: Option<&str>, user_n: Option<usize>) -> usize {
    if let Some(n) = user_n {
        return n.clamp(1, 16);
    }
    let override_n = std::env::var("STAGE2_CONCURRENCY")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok());
    stage2_concurrency_for(text_model, override_n)
}

async fn load_user_stage2_concurrency(pool: &sqlx::PgPool, user_id: Uuid) -> Option<usize> {
    sqlx::query_scalar::<_, Option<i16>>(
        "SELECT stage2_concurrency FROM user_ai_settings WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .flatten()
    .map(|v| v as usize)
}

/// 快速路径心跳周期（租约 60s，页循环原路径每页心跳；此处固定 20s）
const FAST_PATH_HEARTBEAT_SECS: u64 = 20;

/// PDF 直传快速路径执行结果（计数语义与逐页路径一致）
struct FastPathOutcome {
    cancelled: bool,
    success_count: i32,
    failed_count: i32,
    processed_count: i32,
    /// OCR 导出流水线：已落 Markdown，跳过 Stage2
    ocr_export: bool,
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
    stage2_n: usize,
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

    let markdown = if let Some(cached) = cached_ocr_markdown(task) {
        tracing::info!(
            "任务 {task_id} 复用已落库 OCR Markdown（{} 字符，跳过 OCR）",
            cached.chars().count()
        );
        cached
    } else {
        // ── Phase 1：整档 OCR（pin 引擎 future，select 并行处理进度/心跳/取消） ──
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u8>();
        let on_progress: PdfProgressCallback = std::sync::Arc::new(move |p| {
            let _ = tx.send(p);
        });
        let engine_fut = engine.ocr_pdf_async_with_progress(&pdf_bytes, &on_progress);
        tokio::pin!(engine_fut);

        let mut prev: Option<PdfOcrProgress> = None;
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        let mut hb_ticks: u32 = 0;
        let markdown = loop {
            tokio::select! {
                Some(pct) = rx.recv() => {
                    if let Some(mapped) =
                        map_pdf_poll_progress(Some(&serde_json::json!(pct)), total_pages, prev.take())
                    {
                        prev = Some(mapped.clone());
                        update_progress(state, task_id, mapped.current_page, mapped.processed_count, 0, 0).await;
                    }
                }
                _ = tick.tick() => {
                    hb_ticks += 1;
                    if hb_ticks % FAST_PATH_HEARTBEAT_SECS as u32 == 0 {
                        refresh_heartbeat(state, task_id).await;
                    }
                    if is_cancel_requested(state, task_id).await {
                        return Ok(FastPathOutcome {
                            cancelled: true,
                            success_count: 0,
                            failed_count: 0,
                            processed_count: 0,
                            ocr_export: false,
                        });
                    }
                }
                res = &mut engine_fut => {
                    break res.map_err(|e| format!("PDF 直传 OCR 失败: {}", format_ocr_error(&e)))?;
                }
            }
        };
        persist_ocr_markdown(state, task_id, &markdown, engine.id()).await;
        markdown
    };

    let layout = resolve_ocr_layout(state, task, &markdown);
    persist_ocr_layout(state, task_id, &layout).await;

    // OCR 100%：页进度收敛到满页
    update_progress(state, task_id, total_pages, total_pages, 0, 0).await;

    if is_ocr_export(task) {
        if markdown.trim().is_empty() {
            return Err("OCR 结果为空".into());
        }
        persist_ocr_export_ready(
            state,
            task_id,
            space_id,
            paper_id,
            collection_id,
            is_mixed,
        )
        .await;
        tracing::info!("任务 {task_id} OCR 导出就绪，跳过 Stage2");
        return Ok(FastPathOutcome {
            cancelled: false,
            success_count: 0,
            failed_count: 0,
            processed_count: 0,
            ocr_export: true,
        });
    }

    // ── Phase 2：版面切大题（失败则回退 Markdown 题号切块）→ Stage2 ──
    let analysis_paper = looks_like_analysis_paper(&markdown, &task.paper_meta);
    let (chunks, split_via) = split_stage2_with_layout(&layout, &markdown, &task.paper_meta);
    if chunks.is_empty() {
        return Err("PDF 直传 OCR 结果为空".into());
    }
    let prompt = if analysis_paper {
        STAGE2_PARSE_SLIM_PROMPT.as_str()
    } else {
        STAGE2_PARSE_FULL_PROMPT.as_str()
    };
    tracing::info!(
        "任务 {task_id} 全文 Markdown {} 字符 → {} 块解析（切题={split_via}，版面来源={}，{} 块，解析卷={}，并发={}）",
        markdown.chars().count(),
        chunks.len(),
        layout.source.as_str(),
        layout.blocks.len(),
        analysis_paper,
        stage2_n
    );

    let mut outcome = FastPathOutcome {
        cancelled: false,
        success_count: 0,
        failed_count: 0,
        processed_count: 0,
        ocr_export: false,
    };

    let mut pending: Vec<(usize, String)> = chunks.into_iter().enumerate().collect();
    let mut parsed_by_chunk: std::collections::BTreeMap<usize, Vec<ParsedQuestion>> =
        std::collections::BTreeMap::new();

    while !pending.is_empty() {
        refresh_heartbeat(state, task_id).await;
        if is_cancel_requested(state, task_id).await {
            outcome.cancelled = true;
            return Ok(outcome);
        }

        let take_n = stage2_n.min(pending.len());
        let batch: Vec<(usize, String)> = pending.drain(..take_n).collect();

        // 通用 N 路：必须覆盖整个 batch。batch 已从 pending 里 drain 出来，漏处理任何一块
        // 都会让该块的题目凭空消失且不报错。
        let results: Vec<(usize, Result<Vec<ParsedQuestion>, (bool, String)>)> =
            futures::future::join_all(batch.iter().map(|(ci, chunk)| async move {
                let r = parse_stage2_chunk_cancellable(
                    state, task_id, text_provider, text_model, prompt, *ci, chunk,
                )
                .await;
                (*ci, r)
            }))
            .await;

        let mut fatal = false;
        for (ci, res) in results {
            match res {
                Ok(qs) => {
                    parsed_by_chunk.insert(ci, qs);
                }
                Err((is_fatal, msg)) => {
                    if msg.contains("任务已取消") || is_cancel_requested(state, task_id).await {
                        outcome.cancelled = true;
                        return Ok(outcome);
                    }
                    tracing::warn!("任务 {task_id} {msg}");
                    outcome.failed_count += 1;
                    outcome.processed_count += 1;
                    set_last_error(state, task_id, &msg).await;
                    if is_fatal {
                        tracing::warn!(
                            "任务 {task_id} 遇到不可恢复的 AI 错误，停止后续 {} 块",
                            pending.len()
                        );
                        fatal = true;
                    }
                }
            }
        }
        if fatal {
            break;
        }
    }

    let mut flat: Vec<ParsedQuestion> = Vec::new();
    for (_ci, chunk_questions) in parsed_by_chunk {
        flat.extend(chunk_questions);
    }
    let mut merged = merge_split_questions(flat);
    recover_parsed_questions(&mut merged, &markdown);

    for (idx, q) in merged.into_iter().enumerate() {
        if idx % 5 == 0 {
            refresh_heartbeat(state, task_id).await;
            if is_cancel_requested(state, task_id).await {
                outcome.cancelled = true;
                return Ok(outcome);
            }
        }

        let question_index = format!("m_i{idx}");
        let qno = q.question_no.clone();
        outcome.processed_count += 1;
        all_questions.push((question_index.clone(), q.clone()));

        match stage_question(
            state,
            task,
            &question_index,
            q,
            None,
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

    Ok(outcome)
}

fn cached_ocr_markdown(task: &AiParseTask) -> Option<String> {
    task.progress
        .get("ocr_markdown")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

async fn persist_ocr_markdown(state: &AppState, task_id: Uuid, markdown: &str, engine_id: &str) {
    let zip_rel = format!("ocr/{task_id}/mineru.zip");
    let zip_abs = std::path::Path::new(&state.upload_dir).join(&zip_rel);
    let mut patch = serde_json::json!({
        "ocr_markdown": markdown,
        "ocr_engine": engine_id,
        "ocr_chars": markdown.chars().count(),
    });
    if zip_abs.is_file() {
        patch["ocr_zip"] = serde_json::Value::String(zip_rel.replace('\\', "/"));
    }
    if let Err(e) = sqlx::query(
        r#"
        UPDATE ai_parse_tasks
        SET progress = COALESCE(progress, '{}'::jsonb) || $2::jsonb, updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(task_id)
    .bind(&patch)
    .execute(&state.pool)
    .await
    {
        tracing::warn!("任务 {task_id} 写入 OCR Markdown 失败: {e}");
    }
}

fn is_ocr_export(task: &AiParseTask) -> bool {
    task.paper_meta
        .get("pipeline")
        .and_then(|v| v.as_str())
        .or_else(|| task.progress.get("pipeline").and_then(|v| v.as_str()))
        == Some("ocr_export")
}

async fn persist_ocr_export_ready(
    state: &AppState,
    task_id: Uuid,
    space_id: Uuid,
    paper_id: Option<Uuid>,
    collection_id: Option<Uuid>,
    is_mixed: bool,
) {
    let patch = serde_json::json!({
        "phase": "ocr_ready",
        "pipeline": "ocr_export",
        "ocr_export_ctx": {
            "space_id": space_id,
            "paper_id": paper_id,
            "collection_id": collection_id,
            "is_mixed": is_mixed,
        }
    });
    if let Err(e) = sqlx::query(
        r#"
        UPDATE ai_parse_tasks
        SET progress = COALESCE(progress, '{}'::jsonb) || $2::jsonb, updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(task_id)
    .bind(&patch)
    .execute(&state.pool)
    .await
    {
        tracing::warn!("任务 {task_id} 写入 OCR 导出就绪标记失败: {e}");
    }
}

async fn run_page_ocr_export(
    state: &AppState,
    task: &AiParseTask,
    page_files: &[String],
    doc_id: Uuid,
    engine: &dyn crate::ai::ocr::OcrProvider,
    space_id: Uuid,
    paper_id: Option<Uuid>,
    collection_id: Option<Uuid>,
    is_mixed: bool,
) -> Result<FastPathOutcome, String> {
    let task_id = task.id;
    let mut parts: Vec<String> = Vec::new();
    for (page_idx, page_file) in page_files.iter().enumerate() {
        refresh_heartbeat(state, task_id).await;
        if is_cancel_requested(state, task_id).await {
            return Ok(FastPathOutcome {
                cancelled: true,
                success_count: 0,
                failed_count: 0,
                processed_count: 0,
                ocr_export: false,
            });
        }
        let page_no = (page_idx + 1) as i32;
        let page_path = std::path::Path::new(&state.upload_dir)
            .join("documents")
            .join(doc_id.to_string())
            .join(page_file);
        let bytes = tokio::fs::read(&page_path)
            .await
            .map_err(|e| format!("读取第 {page_no} 页失败: {e}"))?;
        let image_b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        match engine.ocr_image(&image_b64).await {
            Ok(md) => {
                if !md.trim().is_empty() {
                    parts.push(md);
                }
            }
            Err(e) => {
                tracing::warn!(
                    "任务 {task_id} 第 {page_no} 页 OCR 导出失败: {}",
                    format_ocr_error(&e)
                );
            }
        }
        update_progress(state, task_id, page_no, page_no, 0, 0).await;
    }
    let markdown = parts.join("\n\n");
    if markdown.trim().is_empty() {
        return Err("OCR 结果为空".into());
    }
    persist_ocr_markdown(state, task_id, &markdown, engine.id()).await;
    let layout = resolve_ocr_layout(state, task, &markdown);
    persist_ocr_layout(state, task_id, &layout).await;
    persist_ocr_export_ready(
        state,
        task_id,
        space_id,
        paper_id,
        collection_id,
        is_mixed,
    )
    .await;
    Ok(FastPathOutcome {
        cancelled: false,
        success_count: 0,
        failed_count: 0,
        processed_count: 0,
        ocr_export: true,
    })
}

fn normalize_stem_key(stem: &str) -> String {
    stem.chars().filter(|c| !c.is_whitespace()).take(96).collect()
}

fn question_no_identity(no: Option<&str>) -> Option<String> {
    let raw = no.map(str::trim).filter(|s| !s.is_empty())?;
    Some(
        parse_question_no_key(raw)
            .map(|(major, minor)| format!("{major}:{minor}"))
            .unwrap_or_else(|| raw.to_string()),
    )
}

fn staged_question_identity(item: &serde_json::Value) -> Option<String> {
    question_no_identity(staged_question_no(item).as_deref())
}

fn staged_stem_identity(item: &serde_json::Value) -> Option<String> {
    item.get("parsed")
        .and_then(|p| p.get("stem"))
        .and_then(|v| v.as_str())
        .map(normalize_stem_key)
        .filter(|s| !s.is_empty())
}

fn staged_index_str(item: &serde_json::Value) -> Option<&str> {
    item.get("index").and_then(|v| v.as_str())
}

/// 同一份站外 JSON 偶发带重复题；按题号、再按题干指纹去重，保留先出现的一题。
fn dedupe_parsed_questions(questions: Vec<ParsedQuestion>) -> Vec<ParsedQuestion> {
    let mut seen_no = HashSet::new();
    let mut seen_stem = HashSet::new();
    let mut out = Vec::with_capacity(questions.len());
    for q in questions {
        if !q.has_visible_body() {
            continue;
        }
        if let Some(key) = question_no_identity(q.question_no.as_deref()) {
            if !seen_no.insert(key) {
                continue;
            }
        }
        let stem_key = normalize_stem_key(&q.stem);
        if !stem_key.is_empty() && !seen_stem.insert(stem_key) {
            continue;
        }
        out.push(q);
    }
    out
}

fn fill_imported_question(q: &mut ParsedQuestion) {
    if q.correct_answer.is_none() {
        q.correct_answer = Some(ParsedAnswer::empty_for_type(&q.question_type));
    }
    if q.image_urls.is_empty() {
        q.image_urls = harvest_markdown_image_urls(&question_body_text(q))
            .into_iter()
            .map(|(_, u)| u)
            .collect();
    }
    let no = q
        .question_no
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    q.question_no = no.or_else(|| infer_question_no_from_stem(&q.stem));
}

fn json_question_no(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn staged_question_no(item: &serde_json::Value) -> Option<String> {
    item.get("parsed")
        .and_then(|p| p.get("question_no"))
        .and_then(json_question_no)
}

fn staged_paper_order_key(item: &serde_json::Value) -> (u8, i32, i32) {
    let parsed = item.get("parsed");
    let no = parsed
        .and_then(|p| p.get("question_no"))
        .and_then(json_question_no);
    let display_order = parsed
        .and_then(|p| p.get("display_order"))
        .and_then(|v| v.as_i64())
        .map(|n| n as i32);
    let stem = parsed
        .and_then(|p| p.get("stem"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    paper_order_key(no.as_deref(), display_order, stem)
}

async fn resort_staged_questions_by_paper_no(
    state: &AppState,
    task_id: Uuid,
) -> Result<(), String> {
    let raw: serde_json::Value = sqlx::query_scalar(
        "SELECT COALESCE(progress->'staged_questions', '[]'::jsonb) FROM ai_parse_tasks WHERE id = $1",
    )
    .bind(task_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| format!("读取暂存失败: {e}"))?;
    let mut items = match raw {
        serde_json::Value::Array(a) => a,
        _ => return Ok(()),
    };
    if items.len() < 2 {
        return Ok(());
    }
    items.sort_by(|a, b| staged_paper_order_key(a).cmp(&staged_paper_order_key(b)));
    sqlx::query(
        r#"
        UPDATE ai_parse_tasks
        SET progress = jsonb_set(
              COALESCE(progress, '{}'::jsonb),
              '{staged_questions}',
              $2::jsonb
            ),
            updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(task_id)
    .bind(serde_json::Value::Array(items))
    .execute(&state.pool)
    .await
    .map_err(|e| format!("按题号重排暂存失败: {e}"))?;
    Ok(())
}

pub(crate) async fn import_external_questions(
    state: &AppState,
    task: &AiParseTask,
    questions: Vec<ParsedQuestion>,
    replace: bool,
) -> Result<usize, String> {
    let ctx = task.progress.get("ocr_export_ctx").cloned();
    let space_id = ctx
        .as_ref()
        .and_then(|c| c.get("space_id"))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| "缺少 OCR 导出上下文 space_id，请重新识别".to_string())?;
    let paper_id = ctx
        .as_ref()
        .and_then(|c| c.get("paper_id"))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());
    let collection_id = ctx
        .as_ref()
        .and_then(|c| c.get("collection_id"))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());
    let is_mixed = ctx
        .as_ref()
        .and_then(|c| c.get("is_mixed"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if replace {
        sqlx::query(
            r#"
            UPDATE ai_parse_tasks
            SET progress = jsonb_set(COALESCE(progress, '{}'::jsonb), '{staged_questions}', '[]'::jsonb),
                updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(task.id)
        .execute(&state.pool)
        .await
        .map_err(|e| format!("清空暂存失败: {e}"))?;
        let _ = sqlx::query(
            r#"
            UPDATE ai_tagging_tasks
            SET status = 'cancelled', updated_at = NOW()
            WHERE parse_task_id = $1 AND status IN ('pending', 'processing', 'queued')
            "#,
        )
        .bind(task.id)
        .execute(&state.pool)
        .await;
    }

    // progress->'staged_questions' 是 JSONB 数组值，不是 Postgres JSONB[]。
    // 解码成 Vec<Value> 会被 sqlx 当成 JSONB[]，触发 type mismatch。
    let existing_json: serde_json::Value = sqlx::query_scalar(
        "SELECT COALESCE(progress->'staged_questions', '[]'::jsonb) FROM ai_parse_tasks WHERE id = $1",
    )
    .bind(task.id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| format!("读取暂存失败: {e}"))?;
    let mut existing: Vec<serde_json::Value> = match existing_json {
        serde_json::Value::Array(items) => items,
        _ => Vec::new(),
    };

    let mut imported = 0usize;
    let mut next_i = existing.len();
    let mut questions = questions;
    for q in &mut questions {
        fill_imported_question(q);
    }
    questions.sort_by(|a, b| {
        cmp_paper_order(
            a.question_no.as_deref(),
            a.display_order,
            &a.stem,
            b.question_no.as_deref(),
            b.display_order,
            &b.stem,
        )
    });
    questions = dedupe_parsed_questions(questions);
    for q in questions {
        if !q.has_visible_body() {
            continue;
        }
        let no_key = question_no_identity(q.question_no.as_deref());
        let stem_key = normalize_stem_key(&q.stem);
        let reuse_index = existing.iter().find_map(|item| {
            let no_hit = no_key.is_some() && staged_question_identity(item) == no_key;
            let stem_hit = !stem_key.is_empty()
                && staged_stem_identity(item).as_deref() == Some(stem_key.as_str());
            if no_hit || stem_hit {
                staged_index_str(item).map(str::to_string)
            } else {
                None
            }
        });
        let index = if let Some(idx) = reuse_index {
            sqlx::query(
                r#"
                UPDATE ai_parse_tasks
                SET progress = jsonb_set(
                      progress,
                      '{staged_questions}',
                      COALESCE((
                        SELECT jsonb_agg(elem)
                        FROM jsonb_array_elements(COALESCE(progress->'staged_questions', '[]'::jsonb)) elem
                        WHERE elem->>'index' <> $2
                      ), '[]'::jsonb)
                    ),
                    updated_at = NOW()
                WHERE id = $1
                "#,
            )
            .bind(task.id)
            .bind(&idx)
            .execute(&state.pool)
            .await
            .map_err(|e| format!("合并暂存失败: {e}"))?;
            idx
        } else {
            let idx = format!("ext_i{next_i}");
            next_i += 1;
            idx
        };
        let q_no = q.question_no.clone();
        let q_stem = q.stem.clone();
        stage_question(
            state,
            task,
            &index,
            q,
            None,
            paper_id,
            collection_id,
            is_mixed,
            space_id,
        )
        .await?;
        existing.retain(|item| staged_index_str(item) != Some(index.as_str()));
        existing.push(serde_json::json!({
            "index": index,
            "parsed": { "question_no": q_no, "stem": q_stem }
        }));
        imported += 1;
    }
    if imported > 0 {
        resort_staged_questions_by_paper_no(state, task.id).await?;
    }
    Ok(imported)
}

fn merge_layout_progress_fields(
    patch: &mut serde_json::Value,
    task_id: Uuid,
    layout: &LayoutDocument,
) {
    patch["ocr_layout_source"] = serde_json::Value::String(layout.source.as_str().into());
    patch["ocr_layout_blocks"] = serde_json::json!(layout.blocks.len());
    patch["ocr_layout_path"] =
        serde_json::Value::String(format!("ocr/{task_id}/layout.json").replace('\\', "/"));
}

fn resolve_ocr_layout(state: &AppState, task: &AiParseTask, markdown: &str) -> LayoutDocument {
    if let Some(doc) = load_layout_sidecar(&state.upload_dir, task.id) {
        if !doc.blocks.is_empty() {
            return doc;
        }
    }
    if let Some(v) = task.progress.get("ocr_layout") {
        if v.get("blocks").is_some() {
            if let Ok(doc) = serde_json::from_value::<LayoutDocument>(v.clone()) {
                if !doc.blocks.is_empty() {
                    return doc;
                }
            }
        }
    }
    LayoutDocument::from_markdown(markdown, LayoutSource::Markdown)
}

async fn persist_ocr_layout(state: &AppState, task_id: Uuid, layout: &LayoutDocument) {
    let path = layout_sidecar_path(&state.upload_dir, task_id);
    if let Some(parent) = path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            tracing::warn!("任务 {task_id} 创建版面目录失败: {e}");
        } else {
            match serde_json::to_vec(layout) {
                Ok(bytes) => {
                    if let Err(e) = tokio::fs::write(&path, bytes).await {
                        tracing::warn!("任务 {task_id} 写入 layout.json 失败: {e}");
                    }
                }
                Err(e) => tracing::warn!("任务 {task_id} 序列化版面失败: {e}"),
            }
        }
    }

    let mut patch = serde_json::json!({});
    merge_layout_progress_fields(&mut patch, task_id, layout);
    if let Err(e) = sqlx::query(
        r#"
        UPDATE ai_parse_tasks
        SET progress = COALESCE(progress, '{}'::jsonb) || $2::jsonb, updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(task_id)
    .bind(&patch)
    .execute(&state.pool)
    .await
    {
        tracing::warn!("任务 {task_id} 写入 OCR 版面摘要失败: {e}");
    }
}

fn split_stage2_with_layout(
    layout: &LayoutDocument,
    md: &str,
    paper_meta: &serde_json::Value,
) -> (Vec<String>, &'static str) {
    if let Some(chunks) = split_question_chunks(layout) {
        if chunks.len() >= 2 {
            return (chunks, layout.source.as_str());
        }
    }
    (
        rehome_trailing_exam_sections(split_stage2_markdown(md, paper_meta)),
        "markdown_fallback",
    )
}

/// Ok(questions) / Err((fatal, message))
async fn parse_stage2_chunk_cancellable(
    state: &AppState,
    task_id: Uuid,
    text_provider: &dyn AiProvider,
    text_model: Option<&str>,
    prompt: &str,
    ci: usize,
    chunk: &str,
) -> Result<Vec<ParsedQuestion>, (bool, String)> {
    tokio::select! {
        r = parse_stage2_chunk(state, task_id, text_provider, text_model, prompt, ci, chunk) => r,
        _ = wait_until_cancel(state, task_id) => Err((false, "任务已取消".into())),
    }
}

async fn wait_until_cancel(state: &AppState, task_id: Uuid) {
    loop {
        if is_cancel_requested(state, task_id).await {
            return;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// Ok(questions) / Err((fatal, message))
async fn parse_stage2_chunk(
    state: &AppState,
    task_id: Uuid,
    text_provider: &dyn AiProvider,
    text_model: Option<&str>,
    prompt: &str,
    ci: usize,
    chunk: &str,
) -> Result<Vec<ParsedQuestion>, (bool, String)> {
    let draft = crate::ai::structure::structure_chunk(chunk);
    tracing::info!(
        confidence = ?draft.confidence,
        method_heading_count = draft.method_heading_count,
        question_no = draft.question.question_no.as_deref().unwrap_or("-"),
        "任务 {task_id} 第 {} 块规则结构化（仍走 Stage2）",
        ci + 1
    );
    let llm_input = stage2_llm_input(chunk);
    if llm_input.chars().count() != chunk.chars().count() {
        tracing::info!(
            "任务 {task_id} 第 {} 块 Stage2 只送题干 {} 字（原文 {} 字，解析由规则回填）",
            ci + 1,
            llm_input.chars().count(),
            chunk.chars().count()
        );
    }
    let mut last_err: Option<AiError> = None;
    let mut parsed: Option<String> = None;
    for attempt in 0..2u8 {
        match text_provider
            .parse_text_with_prompt(&llm_input, prompt, text_model)
            .await
        {
            Ok(r) => {
                parsed = Some(r);
                break;
            }
            Err(e) if (matches!(e, AiError::Timeout) || e.is_rate_limited()) && attempt == 0 => {
                tracing::warn!(
                    "任务 {task_id} 第 {} 块{}，3s 后重试 1 次",
                    ci + 1,
                    if e.is_rate_limited() { "限流" } else { "超时" }
                );
                tokio::time::sleep(Duration::from_secs(3)).await;
                last_err = Some(e);
            }
            Err(e) => {
                last_err = Some(e);
                break;
            }
        }
    }
    let raw_json = match parsed {
        Some(r) => r,
        None => {
            let e = last_err.expect("解析失败时必有错误");
            let msg = format!("第 {} 块解析失败: {}", ci + 1, map_ai_error_msg(&e));
            if is_fatal_ai_error(&e) {
                return Err((true, msg));
            }
            tracing::warn!("任务 {task_id} {msg}，降级为 OCR 草稿题干（保留配图）");
            let mut qs = vec![draft_question_from_chunk(chunk, &map_ai_error_msg(&e))];
            recover_chunk_questions(&mut qs, chunk);
            assign_chunk_images(chunk, &mut qs);
            return Ok(qs);
        }
    };

    let mut chunk_questions = match post_process_batch(&raw_json, &state.pool, true).await {
        Ok(qs) if !qs.is_empty() => qs,
        other => {
            let detail = match other {
                Ok(_) => "questions 为空".to_string(),
                Err((_, err)) => err["error"].as_str().unwrap_or("后处理失败").to_string(),
            };
            tracing::warn!(
                "任务 {task_id} 第 {} 块后处理失败（{detail}），降级为 OCR 草稿题干（保留配图）",
                ci + 1
            );
            vec![draft_question_from_chunk(chunk, &detail)]
        }
    };
    recover_chunk_questions(&mut chunk_questions, chunk);
    assign_chunk_images(chunk, &mut chunk_questions);
    Ok(chunk_questions)
}

fn draft_question_from_chunk(chunk: &str, reason: &str) -> ParsedQuestion {
    let question_type = guess_chunk_question_type(chunk);
    let mut q = ParsedQuestion {
        question_type: question_type.clone(),
        sub_type: None,
        difficulty: None,
        stem: chunk.trim().to_string(),
        options: None,
        correct_answer: Some(ParsedAnswer::empty_for_type(&question_type)),
        analysis: vec![],
        knowledge_points: vec![],
        confidence: 0.25,
        warnings: vec![format!(
            "Stage2 未能结构化（{reason}），已保留 OCR 原文与配图，请核对"
        )],
        image_placeholders: vec![],
        image_urls: vec![],
        kp_matches: vec![],
        parts: vec![],
        question_no: extract_chunk_question_no(chunk),
        display_order: None,
        score: None,
        chapter_path: vec![],
        solution_methods: vec![],
    };
    recover_question_sections(&mut q, chunk);
    q
}

fn extract_chunk_question_no(chunk: &str) -> Option<String> {
    crate::ai::structure::extract_chunk_question_no(chunk)
}

fn guess_chunk_question_type(chunk: &str) -> String {
    crate::ai::structure::guess_chunk_question_type(chunk)
}

fn looks_like_analysis_paper(md: &str, paper_meta: &serde_json::Value) -> bool {
    let title = paper_meta
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let pm_title = paper_meta
        .pointer("/paper_meta/title")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if title.contains("解析") || pm_title.contains("解析") {
        return true;
    }
    md.matches("【解析】").count() + md.matches("【分析】").count() >= 3
}

fn split_stage2_markdown(md: &str, paper_meta: &serde_json::Value) -> Vec<String> {
    let max_q = if looks_like_analysis_paper(md, paper_meta) {
        1
    } else {
        2
    };
    let by_q = split_markdown_by_question_no(md, max_q);
    if by_q.len() >= 2 {
        return rehome_trailing_exam_sections(by_q);
    }
    if md.chars().count() <= STAGE2_SINGLE_QUESTION_MAX_CHARS {
        let t = md.trim();
        if t.is_empty() {
            return Vec::new();
        }
        return rehome_trailing_exam_sections(vec![t.to_string()]);
    }
    rehome_trailing_exam_sections(split_markdown_chunks(md, STAGE2_CHUNK_MAX_CHARS))
}

fn is_notice_heading(line: &str) -> bool {
    line.contains("注意事项") || line.contains("注意事項")
}

/// 卷头「1.答卷前 / 2.用铅笔涂卡」这类说明，不是大题。
fn is_instruction_numbered_line(line: &str) -> bool {
    const HINTS: &[&str] = &[
        "答卷前",
        "考生务必",
        "准考证",
        "答题卡",
        "用铅笔",
        "用橡皮",
        "本试卷",
        "写在本试卷",
        "考试结束",
        "一并交回",
        "密封线",
        "填涂",
        "选出每小题",
        "回答选择题时",
        "注意事项",
    ];
    HINTS.iter().any(|h| line.contains(h))
}

fn looks_like_math_question_start(line: &str) -> bool {
    const MATH: &[&str] = &[
        "已知",
        "设",
        "若",
        "如图",
        "函数",
        "求证",
        "计算",
        "下列",
        "椭圆",
        "集合",
        "向量",
        "不等式",
        "证明：",
        "证明:",
    ];
    MATH.iter().any(|h| line.contains(h))
}

/// 按行首大题号切段，再按 `max_questions_per_chunk` 打包。
/// 切不出至少两道大题时返回空，由调用方回退字数切块。
/// 卷头「注意事项」序号与考场说明不计入题号。
fn split_markdown_by_question_no(md: &str, max_questions_per_chunk: usize) -> Vec<String> {
    let re = question_start_regex();
    let mut starts: Vec<usize> = Vec::new();
    let mut offset = 0usize;
    let mut in_notice = false;
    let mut last_major: Option<u32> = None;
    for line in md.split_inclusive('\n') {
        let trimmed = line.trim();
        if is_notice_heading(trimmed) {
            in_notice = true;
        }
        if exam_section_heading(trimmed) {
            in_notice = false;
        }
        if re.is_match(trimmed) {
            let instruction = is_instruction_numbered_line(trimmed);
            let math_like = looks_like_math_question_start(trimmed);
            if instruction || (in_notice && !math_like) {
                // 卷头说明序号，跳过
            } else if let (Some(prev), Some(curr)) = (last_major, question_major_no(trimmed)) {
                if is_implausible_major_no_drop(prev, curr) {
                    // OCR 把小问收成「2. 若过…」，不要当成新大题
                } else {
                    in_notice = false;
                    last_major = Some(curr);
                    starts.push(offset);
                }
            } else {
                in_notice = false;
                if let Some(n) = question_major_no(trimmed) {
                    last_major = Some(n);
                }
                starts.push(offset);
            }
        }
        offset += line.len();
    }
    if starts.len() < 2 {
        return Vec::new();
    }

    let mut pieces: Vec<String> = Vec::new();
    for (i, &s) in starts.iter().enumerate() {
        let end = starts.get(i + 1).copied().unwrap_or(md.len());
        let body = &md[s..end];
        if i == 0 && s > 0 {
            pieces.push(format!("{}{}", &md[..s], body));
        } else {
            pieces.push(body.to_string());
        }
    }

    let pack = max_questions_per_chunk.max(1);
    pieces
        .chunks(pack)
        .map(|g| g.join(""))
        .filter(|s| !s.trim().is_empty())
        .collect()
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
            if is_insufficient_balance(*status, msg) {
                return "AI 服务余额不足，请充值后再试".to_string();
            }
            if e.is_rate_limited() {
                if msg.contains("RPD") || msg.contains("太平洋时间") {
                    return crate::ai::gemini_limit::GEMINI_RPD_USER_MESSAGE.to_string();
                }
                return RATE_LIMIT_USER_MESSAGE.to_string();
            }
            if is_transient_openrouter_error(*status, msg) {
                return OPENROUTER_PROVIDER_ERROR_USER_MESSAGE.to_string();
            }
            if msg.contains("免费档不可用") {
                return crate::ai::gemini_limit::GEMINI_UNAVAILABLE_USER_MESSAGE.to_string();
            }
            if *status == 401 {
                return "AI API Key 无效或已过期，请到设置页检查".to_string();
            }
            if *status == 403 {
                return "AI 服务拒绝访问（HTTP 403），请检查密钥权限".to_string();
            }
            let short = if msg.chars().count() > 300 {
                format!("{}...", msg.chars().take(300).collect::<String>())
            } else {
                msg.clone()
            };
            format!("AI 上游错误 (HTTP {status}): {short}")
        }
        AiError::Timeout => "AI 调用超时（180s）".to_string(),
    }
}

fn is_insufficient_balance(status: u16, msg: &str) -> bool {
    if status == 402 {
        return true;
    }
    let lower = msg.to_ascii_lowercase();
    lower.contains("insufficient") && lower.contains("balance")
}

/// 401/402/403 / 未配置 Key：后续切块会同样失败，应立即停掉
fn is_fatal_ai_error(e: &AiError) -> bool {
    match e {
        AiError::NoApiKey => true,
        AiError::Upstream(status, msg) => {
            matches!(*status, 401 | 402 | 403) || is_insufficient_balance(*status, msg)
        }
        AiError::Timeout => false,
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
            parts: vec![],
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
    fn test_draft_from_chunk_keeps_figure_and_question_no() {
        let chunk = "5. 函数 $f(x)$ 的图象可能是（ ）\n\n![A](/uploads/q5a.png)\n\nA.\nB.";
        let q = draft_question_from_chunk(chunk, "缺少 questions 数组");
        assert_eq!(q.question_no.as_deref(), Some("5"));
        assert_eq!(q.question_type, "choice");
        assert!(q.stem.contains("/uploads/q5a.png"));
        assert!(q.warnings.iter().any(|w| w.contains("OCR")));
    }

    #[test]
    fn test_draft_from_chunk_peels_analysis_and_keeps_six_methods() {
        let chunk = "\
16. 已知椭圆\n\
（1）求离心率\n\
【解析】\n\
法一：平移\n\
法二：点差\n\
法三：韦达\n\
法四：参数\n\
法五：斜率不存在\n\
法六：水平宽乘铅垂高\n";
        let q = draft_question_from_chunk(chunk, "JSON 截断");
        assert!(!q.stem.contains("【解析】"), "题干不应再含解析: {}", q.stem);
        assert!(q.stem.contains("已知椭圆"));
        let n = q
            .parts
            .iter()
            .flat_map(|p| p.analyses.iter())
            .chain(q.analysis.iter())
            .filter(|a| a.content.contains("水平宽") || a.title.contains("六"))
            .count();
        assert!(n >= 1, "应回填法六: parts={:?} analysis={:?}", q.parts, q.analysis);
        let total = q.parts.iter().map(|p| p.analyses.len()).sum::<usize>().max(q.analysis.len());
        assert!(total >= 6, "应保留 6 种解法, total={total}, parts={:?}", q.parts);
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

        let insert_stale = async |retry_count: i32| -> Uuid {
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

        // 心跳超时的 processing 一律终止，不自动续跑；剩余重试次数不影响结果
        for retry_count in [0, 2] {
            let id = insert_stale(retry_count).await;
            recover_stale_tasks(&state, "test-worker").await;
            let (status, last_error, retry): (String, Option<String>, i32) = sqlx::query_as(
                "SELECT status::text, last_error, retry_count FROM ai_parse_tasks WHERE id = $1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("查询任务失败");
            assert_eq!(status, "cancelled", "僵尸任务应终止而非重新入队");
            assert!(last_error.unwrap_or_default().contains("中断"));
            assert_eq!(retry, retry_count, "终止不应消耗重试次数");
        }

        // 已请求取消且尚未被拾取 → 立即落 cancelled
        let pending_cancel = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO ai_parse_tasks (id, creator_id, raw_text, status, document_id, retry_count, cancel_requested_at, created_at, updated_at, progress)
            VALUES ($1, $2, '', 'pending', NULL, 0, NOW(), NOW(), NOW(), '{"idempotency_map": {}}')
            "#,
        )
        .bind(pending_cancel)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("插入任务失败");
        recover_stale_tasks(&state, "test-worker").await;
        let status: String = sqlx::query_scalar("SELECT status::text FROM ai_parse_tasks WHERE id = $1")
            .bind(pending_cancel)
            .fetch_one(&pool)
            .await
            .expect("查询任务失败");
        assert_eq!(status, "cancelled", "取消请求应在拾取前生效");

        // 心跳正常（未超时）→ 不受影响
        let healthy = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO ai_parse_tasks (id, creator_id, raw_text, status, document_id, retry_count, heartbeat_at, created_at, updated_at, progress)
            VALUES ($1, $2, '', 'processing', NULL, 0, NOW(), NOW(), NOW(), '{"idempotency_map": {}}')
            "#,
        )
        .bind(healthy)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("插入任务失败");
        recover_stale_tasks(&state, "test-worker").await;
        let status: String = sqlx::query_scalar("SELECT status::text FROM ai_parse_tasks WHERE id = $1")
            .bind(healthy)
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

        let staged: serde_json::Value = sqlx::query_scalar(
            "SELECT progress->'staged_questions'->0 FROM ai_parse_tasks WHERE id = $1",
        )
        .bind(task.id)
        .fetch_one(&pool)
        .await
        .expect("查询暂存失败");

        match staged.get("tagging_status").and_then(|s| s.as_str()) {
            // 配置了文本模型：走与编辑页相同的 Content 提取，暂存阶段只入队，标签由打标 worker 事后回写
            Some("pending") => {
                assert!(
                    staged.get("suggestion").is_none_or(|s| s.is_null()),
                    "异步打标尚未完成时不应写入 suggestion：{staged}"
                );
                let queued: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM ai_tagging_tasks WHERE parse_task_id = $1 AND source_index = $2",
                )
                .bind(task.id)
                .bind("p1_i0")
                .fetch_one(&pool)
                .await
                .expect("查询打标队列失败");
                assert_eq!(queued, 1, "暂存应为该题入队一个打标任务");
            }
            // 无文本模型时降级为同步 Parsed 适配，未匹配名称随暂存项保存
            Some("done") => {
                let planted = format!("完全不存在的知识点XYZ_{uid}");
                let unmatched_knowledge = staged
                    .get("unmatched")
                    .and_then(|u| u.get("knowledge"))
                    .and_then(|k| k.as_array())
                    .cloned()
                    .unwrap_or_default();
                assert!(
                    unmatched_knowledge
                        .iter()
                        .any(|n| n.as_str() == Some(planted.as_str())),
                    "未匹配知识点应随暂存项保存：{staged}"
                );
            }
            other => panic!("暂存项 tagging_status 异常：{other:?}，staged={staged}"),
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

    #[tokio::test]
    async fn test_ocr_export_defers_tagging_until_start() {
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
        let mut task = fake_task(user_id, doc_id);
        task.paper_meta = json!({ "document_type": "class_exercise", "pipeline": "ocr_export" });
        task.progress = json!({ "pipeline": "ocr_export", "idempotency_map": {} });
        sqlx::query(
            "INSERT INTO documents (id, creator_id, file_name, page_count, status, document_type, title) VALUES ($1, $2, 't.pdf', 1, 'confirmed', 'class_exercise', '课堂练习')",
        )
        .bind(doc_id)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("插入文档失败");
        sqlx::query(
            "INSERT INTO ai_parse_tasks (id, creator_id, raw_text, status, document_id, progress, paper_meta, created_at, updated_at) VALUES ($1, $2, '', 'success', $3, $4, $5, NOW(), NOW())",
        )
        .bind(task.id)
        .bind(user_id)
        .bind(doc_id)
        .bind(&task.progress)
        .bind(&task.paper_meta)
        .execute(&pool)
        .await
        .expect("插入任务失败");

        let uid = Uuid::new_v4().simple().to_string();
        let parsed = fake_parsed(Some("1"), &format!("站外导入题干{uid}"));
        stage_question(&state, &task, "p1_i0", parsed, None, None, None, false, space_id)
            .await
            .expect("stage 失败");

        let staged: serde_json::Value = sqlx::query_scalar(
            "SELECT progress->'staged_questions'->0 FROM ai_parse_tasks WHERE id = $1",
        )
        .bind(task.id)
        .fetch_one(&pool)
        .await
        .expect("查询暂存失败");
        let queued: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM ai_tagging_tasks WHERE parse_task_id = $1 AND source_index = $2",
        )
        .bind(task.id)
        .bind("p1_i0")
        .fetch_one(&pool)
        .await
        .expect("查询打标队列失败");
        assert_eq!(queued, 0, "站外导入不应自动入队打标：{staged}");

        match staged.get("tagging_status").and_then(|s| s.as_str()) {
            Some("idle") => {
                let (started, _) = start_staged_tagging(&state, task.id)
                    .await
                    .expect("开始打标失败");
                assert_eq!(started, 1, "idle 题应入队 1 个打标任务");
                let status: String = sqlx::query_scalar(
                    "SELECT progress->'staged_questions'->0->>'tagging_status' FROM ai_parse_tasks WHERE id = $1",
                )
                .bind(task.id)
                .fetch_one(&pool)
                .await
                .expect("查询状态失败");
                assert_eq!(status, "pending");
                reset_pending_staged_tagging(&pool, task.id)
                    .await
                    .expect("重置失败");
                let status: String = sqlx::query_scalar(
                    "SELECT progress->'staged_questions'->0->>'tagging_status' FROM ai_parse_tasks WHERE id = $1",
                )
                .bind(task.id)
                .fetch_one(&pool)
                .await
                .expect("查询状态失败");
                assert_eq!(status, "idle", "停止打标后应回到 idle");
            }
            Some("done") => {
                let (started, skipped) = start_staged_tagging(&state, task.id)
                    .await
                    .expect("开始打标失败");
                assert_eq!(started, 0, "无文本模型时同步打标已完成，不应再入队");
                assert!(skipped >= 1);
            }
            other => panic!("ocr_export 暂存 tagging_status 异常：{other:?}，staged={staged}"),
        }
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

    #[test]
    fn test_split_by_question_no_packs_two() {
        let md = "1. 第一题题干\n\n解答略\n\n2. 第二题题干\n\n3. 第三题\n\n4. 第四题";
        let chunks = split_markdown_by_question_no(md, 2);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].contains("1. 第一题"));
        assert!(chunks[0].contains("2. 第二题"));
        assert!(chunks[1].contains("3. 第三题"));
        assert!(chunks[1].contains("4. 第四题"));
        assert!(!chunks[0].contains("3. 第三题"));
    }

    #[test]
    fn test_split_by_question_no_analysis_one_per_chunk() {
        let md = "16. 椭圆题\n【解析】法一\n\n17. 导数题\n【解析】法二";
        let chunks = split_markdown_by_question_no(md, 1);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].contains("16. 椭圆题"));
        assert!(!chunks[0].contains("17. 导数题"));
    }

    #[test]
    fn test_split_stage2_rehomes_section_heading_on_analysis_paper() {
        let md = "\
8. 已知函数 $f(x)$。\n\
故选：B\n\
\n\
## 二、选择题：本题共3小题，每小题6分，共18分。有多项符合题目要求。\n\
9. 已知随机变量服从正态分布。\n\
故选：BC\n";
        let chunks = split_stage2_markdown(md, &json!({"title": "2024年高考数学试卷（解析卷）"}));
        assert!(chunks.len() >= 2, "{chunks:?}");
        let q8 = chunks.iter().find(|c| c.contains("8. 已知函数")).expect("第8题");
        let q9 = chunks.iter().find(|c| c.contains("9. 已知随机变量")).expect("第9题");
        assert!(
            !q8.contains("二、选择题") && !q8.contains("多项符合"),
            "卷头不得留在上一题: {q8}"
        );
        assert!(
            q9.contains("二、选择题") && q9.contains("9. 已知随机变量"),
            "卷头应交给下一题: {q9}"
        );
    }

    #[test]
    fn test_split_by_question_no_ignores_sub_items_and_decimals() {
        let md = "1. 大题\n（1）小问一\n（2）小问二\n3.14 不是题号\n2. 第二大题";
        let chunks = split_markdown_by_question_no(md, 1);
        assert_eq!(chunks.len(), 2, "只应按 1. / 2. 切，不切（1）和 3.14");
        assert!(chunks[0].contains("（1）小问一"));
        assert!(chunks[0].contains("3.14"));
        assert!(chunks[1].starts_with("2. 第二大题") || chunks[1].contains("2. 第二大题"));
    }

    #[test]
    fn test_split_by_question_no_keeps_item_16_with_ocr_subquestion_two() {
        let md = "\
16. 已知 $A(0,3)$ 为椭圆上两点.\n\
（1）求离心率\n\
2. 若过 $P$ 的直线 $l$ 交 $C$ 于另一点 $B$\n\
【解析】\n\
法五：斜率不存在\n\
法六：水平宽\n";
        let chunks = split_markdown_by_question_no(md, 1);
        assert!(
            chunks.is_empty(),
            "16 后的行首「2. 若过」不应切成第二道大题，chunks={chunks:?}"
        );
        let stage2 = split_stage2_markdown(md, &serde_json::json!({}));
        assert_eq!(stage2.len(), 1, "单题长解析应整题送 Stage2: {stage2:?}");
        assert!(stage2[0].contains("法六"));
    }

    #[test]
    fn test_split_skips_notice_numbered_instructions() {
        let md = "\
宁波市 2025 期末九校联考高一数学试题\n\
注意事项：\n\
1.答卷前，考生务必将自己的姓名、准考证号填写在答题卡上.\n\
2.回答选择题时，选出每小题答案后，用铅笔把答题卡上对应题目的答案标号涂黑.\n\
3.考试结束后，将本试卷和答题卡一并交回.\n\
第 I 卷\n\
一、选择题（本大题共 8 小题）\n\
1. 已知全集 $U=R$，集合 $A$ 如图阴影部分表示的集合是（  ）\n\
A. ${0<x<1}$\n\
2. 已知向量 $\\vec{a}$，则下列等式中成立的是（  ）\n\
3. 已知命题 $p$，则实数 $a$ 的取值范围是（  ）\n\
4. 已知 $a,b,c$ 的大小关系为（  ）\n";
        let chunks = split_markdown_by_question_no(md, 2);
        assert!(chunks.len() >= 2, "应切出真实选择题，chunks={chunks:?}");
        assert!(
            chunks[0].contains("1. 已知全集"),
            "第 1 块应含第 1 题: {}",
            chunks[0]
        );
        assert!(
            chunks[0].contains("2. 已知向量"),
            "每块 2 题，第 1 块还应含第 2 题"
        );
        assert!(
            !chunks[0].contains("3. 已知命题"),
            "第 3 题不应打进第 1 块"
        );
        // 注意事项应作为前文附在第 1 题上，但 1.答卷前 不能单独成块
        assert!(chunks[0].contains("注意事项"));
        assert!(chunks.iter().all(|c| !c.trim_start().starts_with("1.答卷前")));
        assert!(chunks[1].contains("3. 已知命题"));
    }

    #[test]
    fn test_split_skips_notice_without_section_heading() {
        let md = "\
注意事项：\n\
1.考生务必在答题卡上填涂.\n\
2.考试结束后一并交回.\n\
1. 已知函数 $f(x)$ 的值域是\n\
2. 设椭圆 $C$ 的方程为\n";
        let chunks = split_markdown_by_question_no(md, 1);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].contains("1. 已知函数"));
        assert!(chunks[1].contains("2. 设椭圆"));
        assert!(!chunks[1].contains("考生务必"));
    }

    #[test]
    fn test_split_stage2_falls_back_without_numbers() {
        let md = "没有题号的一段文字\n\n另一段";
        let chunks = split_stage2_markdown(md, &json!({}));
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_split_stage2_prefers_layout_spans() {
        use crate::ai::layout::{BBox, BlockKind, LayoutBlock};
        let layout = LayoutDocument {
            source: LayoutSource::Mineru,
            blocks: vec![
                LayoutBlock {
                    page: 0,
                    order: 0,
                    kind: BlockKind::Text,
                    text: "1. 已知集合 A".into(),
                    bbox: Some(BBox {
                        x0: 80.0,
                        y0: 100.0,
                        x1: 400.0,
                        y1: 160.0,
                    }),
                    image_url: None,
                },
                LayoutBlock {
                    page: 0,
                    order: 1,
                    kind: BlockKind::Text,
                    text: "2. 设椭圆 C".into(),
                    bbox: Some(BBox {
                        x0: 80.0,
                        y0: 200.0,
                        x1: 400.0,
                        y1: 260.0,
                    }),
                    image_url: None,
                },
            ],
        };
        let md = "整页糊成一团 1. 已知集合 A 2. 设椭圆 C 不按行切开";
        let (chunks, via) = split_stage2_with_layout(&layout, md, &json!({}));
        assert_eq!(via, "mineru");
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].contains("1. 已知集合"));
        assert!(chunks[1].contains("2. 设椭圆"));
    }

    #[test]
    fn test_split_stage2_layout_too_few_falls_back() {
        let layout = LayoutDocument::from_markdown("没有题号的一段文字", LayoutSource::Markdown);
        let md = "没有题号的一段文字\n\n另一段";
        let (chunks, via) = split_stage2_with_layout(&layout, md, &json!({}));
        assert_eq!(via, "markdown_fallback");
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_looks_like_analysis_paper_by_title() {
        assert!(looks_like_analysis_paper("1. x", &json!({"title": "2024年高考数学试卷（解析卷）"})));
        assert!(!looks_like_analysis_paper("1. x", &json!({"title": "月考"})));
    }

    #[test]
    fn test_map_ai_error_402_is_balance_message() {
        let e = AiError::Upstream(
            402,
            r#"{"error":{"message":"Insufficient Balance","type":"unknown_error"}}"#.into(),
        );
        assert_eq!(map_ai_error_msg(&e), "AI 服务余额不足，请充值后再试");
        assert!(is_fatal_ai_error(&e));
    }

    #[test]
    fn test_map_ai_error_insufficient_balance_text_without_402() {
        let e = AiError::Upstream(400, "Error: Insufficient Balance for this api key".into());
        assert_eq!(map_ai_error_msg(&e), "AI 服务余额不足，请充值后再试");
        assert!(is_fatal_ai_error(&e));
    }

    #[test]
    fn test_map_ai_error_429_glm_rate_limit() {
        let e = AiError::Upstream(
            429,
            r#"{"error":{"code":"1302","message":"您的账户已达到速率限制，请您控制请求频率"}}"#.into(),
        );
        assert_eq!(map_ai_error_msg(&e), RATE_LIMIT_USER_MESSAGE);
        assert!(!is_fatal_ai_error(&e));
        // 确知限流很紧的档位降为 1
        assert_eq!(stage2_concurrency_for(Some("glm-4.7-flash"), None), 1);
        assert_eq!(stage2_concurrency_for(Some("gemini-3.7-flash"), None), 1);
        assert_eq!(stage2_concurrency_for(Some("gemini-2.5-flash"), None), 1);
        assert_eq!(stage2_concurrency_for(Some("qwen/qwen3-8b:free"), None), 1);
        assert_eq!(stage2_concurrency_for(Some("google/gemma-3-27b-it:free"), None), 1);
        // 付费模型（含 OpenRouter vendor/model ID）走默认并发
        assert_eq!(stage2_concurrency_for(Some("deepseek-chat"), None), STAGE2_CONCURRENCY);
        assert_eq!(stage2_concurrency_for(Some("stealth/ox-alpha"), None), STAGE2_CONCURRENCY);
        // 环境变量覆盖优先，且被夹在 [1,16]
        assert_eq!(stage2_concurrency_for(Some("gemini-3.7-flash"), Some(8)), 8);
        assert_eq!(stage2_concurrency_for(Some("deepseek-chat"), Some(0)), 1);
        assert_eq!(stage2_concurrency_for(Some("deepseek-chat"), Some(99)), 16);
        let or_err = AiError::Upstream(
            400,
            r#"{"error":{"message":"Provider returned error","code":400,"metadata":{"raw":"ERROR","provider_name":"Stealth"}}}"#.into(),
        );
        assert_eq!(map_ai_error_msg(&or_err), OPENROUTER_PROVIDER_ERROR_USER_MESSAGE);
    }

    #[test]
    fn test_staged_paper_order_prefers_question_no() {
        let q14 = serde_json::json!({"parsed": {"question_no": "14", "stem": "填空"}});
        let q4 = serde_json::json!({"parsed": {"question_no": 4, "stem": "选择"}});
        let q1 = serde_json::json!({"parsed": {"question_no": "1", "stem": "选择"}});
        let mut items = vec![q14, q4, q1];
        items.sort_by(|a, b| staged_paper_order_key(a).cmp(&staged_paper_order_key(b)));
        let nos: Vec<_> = items
            .iter()
            .map(|v| staged_question_no(v).unwrap())
            .collect();
        assert_eq!(nos, vec!["1", "4", "14"]);
    }

    #[test]
    fn test_dedupe_parsed_questions_by_no_and_stem() {
        let qs = vec![
            fake_parsed(Some("1"), "已知复数 z"),
            fake_parsed(Some("1."), "已知复数 z"),
            fake_parsed(Some("13"), "已知 alpha 为第一象限角"),
            fake_parsed(None, "已知 alpha 为第一象限角"),
            fake_parsed(Some("14"), "第 14 题表格"),
        ];
        let out = dedupe_parsed_questions(qs);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].question_no.as_deref(), Some("1"));
        assert_eq!(out[1].question_no.as_deref(), Some("13"));
        assert_eq!(out[2].question_no.as_deref(), Some("14"));
    }
}
