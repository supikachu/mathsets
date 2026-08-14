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
use crate::ai::prompt::BATCH_IMAGE_OCR_FULL_PROMPT;
use crate::ai::provider::{create_provider, AiError};
use crate::ai::types::ParsedQuestion;
use crate::auth::middleware::AuthUser;
use crate::auth::permissions::ensure_personal_space;
use crate::handlers::ai::{post_process_batch, resolve_ai_config, ModelKind};
use crate::handlers::ai_tagging::{match_knowledge_nodes, KnowledgeNodeMatch};
use crate::handlers::collections::{get_or_create_collection, link_question_to_collection};
use crate::handlers::questions::{save_version, upsert_ai_knowledge_nodes};
use crate::models::ai_task::{AiParseTask, AiTaskSourceType, AiTaskStatus};
use crate::models::document::is_paper_type;
use crate::models::question::{Difficulty, QuestionStatus, QuestionType};
use crate::util::normalize::{compute_content_hash, compute_normalized_content_hash};
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

    // 跨页组装（文本模型，可选）
    let assemble_enabled = std::env::var("AI_TASK_ASSEMBLE")
        .ok()
        .map(|v| v != "0")
        .unwrap_or(ASSEMBLE_ENABLED_BY_DEFAULT);
    let text_provider = if assemble_enabled {
        resolve_ai_config(&auth, state, ModelKind::Text)
            .await
            .ok()
            .map(|(key, name, model, base)| (create_provider(&name, &key, &base), model))
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

    // ── Stage 3：逐页 OCR → 逐题落库 ────────────────────────────────
    let mut all_questions: Vec<(String, ParsedQuestion)> = Vec::new();
    let mut success_count: i32 = 0;
    let mut failed_count: i32 = 0;
    let mut processed_count: i32 = 0;
    let mut cancelled = false;

    for (page_idx, page_file) in page_files.iter().enumerate() {
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
                update_progress(state, task_id, page_no, None, processed_count, success_count, failed_count).await;
                continue;
            }
        };
        let image_b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

        let raw_json = match vision_provider
            .parse_image_with_prompt(&image_b64, &BATCH_IMAGE_OCR_FULL_PROMPT, vision_model.as_deref())
            .await
        {
            Ok(raw) => raw,
            Err(e) => {
                // 页面级失败不消耗重试（计入 failed_count，走 partial_success）
                let msg = map_ai_error_msg(&e);
                tracing::warn!("任务 {task_id} 第 {page_no} 页 OCR 失败: {msg}");
                failed_count += 1;
                processed_count += 1;
                set_last_error(state, task_id, &msg).await;
                update_progress(state, task_id, page_no, None, processed_count, success_count, failed_count).await;
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
                update_progress(state, task_id, page_no, None, processed_count, success_count, failed_count).await;
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

            match persist_question(
                state,
                task,
                &question_index,
                q,
                paper_id,
                collection_ids.first().copied(),
                is_mixed,
                space_id,
                &auth,
            )
            .await
            {
                Ok(question_id) => {
                    success_count += 1;
                    if let Some(no) = qno {
                        set_current_question_no(state, task_id, &no).await;
                    }
                    update_progress(
                        state,
                        task_id,
                        page_no,
                        Some((question_index, question_id)),
                        processed_count,
                        success_count,
                        failed_count,
                    )
                    .await;
                }
                Err(e) => {
                    tracing::warn!("任务 {task_id} 第 {question_index} 题落库失败: {e}");
                    failed_count += 1;
                    update_progress(state, task_id, page_no, None, processed_count, success_count, failed_count).await;
                }
            }
        }

        if cancelled {
            break;
        }
    }

    // ── Stage 3b：跨页组装（题号/顺序重排，失败降级） ──────────────
    if let Some((text_provider, text_model)) = text_provider {
        if !cancelled && all_questions.len() > 1 {
            match assemble_question_order(&text_provider, text_model.as_deref(), &all_questions).await {
                Ok(mapping) => {
                    apply_question_order(state, task_id, &mapping).await;
                }
                Err(e) => {
                    tracing::warn!("任务 {task_id} 跨页组装失败，使用原顺序: {e}");
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

#[allow(clippy::too_many_arguments)]
async fn persist_question(
    state: &AppState,
    task: &AiParseTask,
    question_index: &str,
    parsed: ParsedQuestion,
    paper_id: Option<Uuid>,
    collection_id: Option<Uuid>,
    is_mixed: bool,
    space_id: Uuid,
    auth: &AuthUser,
) -> Result<Uuid, String> {
    // 幂等：同任务重跑命中 idempotency_map → 复用
    if let Some(existing) = task
        .progress
        .get("idempotency_map")
        .and_then(|m| m.get(question_index))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
    {
        return Ok(existing);
    }

    let question_type = match parsed.question_type.as_str() {
        "choice" => QuestionType::Choice,
        "multiple" => QuestionType::Multiple,
        "fill" => QuestionType::Fill,
        "solution" => QuestionType::Solution,
        other => return Err(format!("未知题型: {other}")),
    };
    let difficulty = match parsed.difficulty.as_deref() {
        Some("easy") => Difficulty(2),
        Some("hard") => Difficulty(4),
        _ => Difficulty(3),
    };

    let options_json = parsed
        .options
        .as_ref()
        .map(|opts| serde_json::to_value(opts).unwrap_or(serde_json::Value::Null));
    let correct_answer_json = serde_json::to_value(&parsed.correct_answer)
        .map_err(|e| format!("序列化 correct_answer 失败: {e}"))?;
    let analysis_str: Option<String> = if parsed.analysis.is_empty() {
        None
    } else {
        Some(
            parsed
                .analysis
                .iter()
                .map(|m| m.content.clone())
                .collect::<Vec<_>>()
                .join("\n\n---\n\n"),
        )
    };

    // V2.1.1：hash 去重（normalized 命中 → 复用 Question）
    let content_hash = compute_content_hash(
        &parsed.stem,
        options_json.as_ref(),
        &correct_answer_json,
        analysis_str.as_deref(),
    );
    let normalized_hash = compute_normalized_content_hash(
        &parsed.stem,
        options_json.as_ref(),
        &correct_answer_json,
    );

    if let Some(existing) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM questions WHERE normalized_content_hash = $1 LIMIT 1",
    )
    .bind(&normalized_hash)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| format!("题目查重失败: {e}"))?
    {
        tracing::info!(
            "任务 {} 题目「{}」hash 命中已有题目 {existing}，复用",
            task.id,
            parsed.stem.chars().take(20).collect::<String>()
        );
        link_to_container(state, existing, paper_id, collection_id, is_mixed, &parsed).await;
        return Ok(existing);
    }

    // 新建（单题事务：Question + 知识点 + 版本）
    let id = Uuid::new_v4();
    let now = chrono::Utc::now();
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| format!("开启事务失败: {e}"))?;

    sqlx::query(
        r#"
        INSERT INTO questions (id, stem, stem_text, images,
            question_type, options, correct_answer, analysis,
            difficulty, metadata,
            parent_id, sub_order,
            status, space_id, origin_question_id,
            creator_id, created_at, updated_by, updated_at, version,
            content_hash, normalized_content_hash)
        VALUES ($1, $2, NULL, NULL,
            $3, $4, $5, $6,
            $7, COALESCE($8, '{}'::jsonb),
            NULL, NULL,
            $9, $10, NULL,
            $11, $12, NULL, $13, $14,
            $15, $16)
        "#,
    )
    .bind(id)
    .bind(&parsed.stem)
    .bind(question_type)
    .bind(&options_json)
    .bind(&correct_answer_json)
    .bind(&analysis_str)
    .bind(difficulty)
    .bind(None::<serde_json::Value>)
    .bind(QuestionStatus::Draft)
    .bind(space_id)
    .bind(auth.id)
    .bind(now)
    .bind(now)
    .bind(1)
    .bind(&content_hash)
    .bind(&normalized_hash)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("插入题目失败: {e}"))?;

    // 知识点匹配（未匹配项 → tag_candidates 候选队列，不阻塞落库）
    // 注意：候选创建必须在题目事务提交后进行（source_question_id 外键可见性）
    let mut unmatched_names: Vec<String> = Vec::new();
    let (ai_matches, primary_node_id): (Vec<KnowledgeNodeMatch>, Option<Uuid>) =
        if !parsed.knowledge_points.is_empty() {
            match match_knowledge_nodes(&state.pool, &parsed.knowledge_points, None).await {
                Ok((matched, unmatched)) => {
                    unmatched_names = unmatched;
                    let primary = matched
                        .iter()
                        .max_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal))
                        .map(|m| m.node_id);
                    (matched, primary)
                }
                Err(e) => {
                    tracing::warn!("任务 {} 知识点匹配失败（不影响落库）: {:?}", task.id, e.1);
                    (vec![], None)
                }
            }
        } else {
            (vec![], None)
        };
    if !ai_matches.is_empty() {
        upsert_ai_knowledge_nodes(&mut tx, id, &ai_matches, primary_node_id)
            .await
            .map_err(|e| format!("关联知识点失败: {e}"))?;
    }

    save_version(&mut tx, id, 1, Some(auth.id))
        .await
        .map_err(|e| format!("保存版本快照失败: {e}"))?;

    tx.commit().await.map_err(|e| format!("提交事务失败: {e}"))?;

    if !unmatched_names.is_empty() {
        create_tag_candidates(state, task.id, id, &parsed, &unmatched_names).await;
    }

    link_to_container(state, id, paper_id, collection_id, is_mixed, &parsed).await;

    Ok(id)
}

/// 未匹配知识点 → tag_candidates（幂等，不阻塞题目落库）
async fn create_tag_candidates(
    state: &AppState,
    task_id: Uuid,
    question_id: Uuid,
    parsed: &ParsedQuestion,
    unmatched: &[String],
) {
    for name in unmatched {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let normalized = crate::util::normalize::normalize_text(name);
        let confidence = rust_decimal::Decimal::from_f32_retain(parsed.confidence)
            .map(|d| d.max(rust_decimal::Decimal::ZERO))
            .unwrap_or(rust_decimal::Decimal::ZERO);
        let result = sqlx::query(
            r#"
            INSERT INTO tag_candidates (kind, raw_name, normalized_name, ai_confidence, match_score, source_task_id, source_question_id)
            VALUES ('knowledge', $1, $2, $3, 0, $4, $5)
            ON CONFLICT (source_task_id, source_question_id, normalized_name, kind) DO NOTHING
            "#,
        )
        .bind(name)
        .bind(&normalized)
        .bind(confidence)
        .bind(task_id)
        .bind(question_id)
        .execute(&state.pool)
        .await;

        match result {
            Ok(r) if r.rows_affected() > 0 => {
                tracing::info!("任务 {task_id} 知识点「{name}」未匹配 → 进入候选队列");
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("任务 {task_id} 写 tag_candidates 失败（不阻塞）: {e}");
            }
        }
    }
}

/// 关联容器（Paper / Collection）；Mixed 文档不自动关联（前端分组）
async fn link_to_container(
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

/// 更新进度计数器 + 幂等映射
#[allow(clippy::too_many_arguments)]
async fn update_progress(
    state: &AppState,
    task_id: Uuid,
    page_no: i32,
    idem: Option<(String, Uuid)>,
    processed: i32,
    success: i32,
    failed: i32,
) {
    let qid_opt = idem.as_ref().map(|(_, qid)| *qid);
    let sql = match &idem {
        Some((key, _)) => format!(
            r#"
            UPDATE ai_parse_tasks
            SET progress = jsonb_set(progress, '{{idempotency_map,{key}}}', to_jsonb($2::uuid), true),
                processed_count = $3, success_count = $4, failed_count = $5,
                current_page = $6, heartbeat_at = NOW(), updated_at = NOW()
            WHERE id = $1
            "#
        ),
        None => r#"
            UPDATE ai_parse_tasks
            SET processed_count = $3, success_count = $4, failed_count = $5,
                current_page = $6, heartbeat_at = NOW(), updated_at = NOW()
            WHERE id = $1
        "#
        .to_string(),
    };
    let mut q = sqlx::query(&sql).bind(task_id);
    if let Some(qid) = qid_opt {
        q = q.bind(qid);
    }
    let _ = q
        .bind(processed)
        .bind(success)
        .bind(failed)
        .bind(page_no)
        .execute(&state.pool)
        .await;
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
    provider: &Box<dyn crate::ai::provider::AiProvider>,
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

/// 应用组装结果：更新容器关联表的 question_no / display_order（合并项跳过）
async fn apply_question_order(
    state: &AppState,
    task_id: Uuid,
    mapping: &[(String, serde_json::Value)],
) {
    let doc_id: Option<Uuid> = sqlx::query_scalar("SELECT document_id FROM ai_parse_tasks WHERE id = $1")
        .bind(task_id)
        .fetch_one(&state.pool)
        .await
        .ok()
        .flatten();

    for (index, item) in mapping {
        // 跨页合并项跳过（保留先出现者的题号）
        if item
            .get("merge_into")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty())
        {
            continue;
        }
        let Some(question_id) = question_id_by_index(state, task_id, index).await else {
            continue;
        };
        let question_no = item.get("question_no").and_then(|v| v.as_str());
        let display_order = item.get("display_order").and_then(|v| v.as_i64()).map(|v| v as i32);

        let Some(doc) = doc_id else { continue };
        // 试卷关联
        let _ = sqlx::query(
            r#"
            UPDATE paper_questions
            SET question_no = COALESCE($1, question_no),
                display_order = COALESCE($2, display_order)
            WHERE question_id = $3
              AND paper_id = (SELECT id FROM papers WHERE document_id = $4 LIMIT 1)
            "#,
        )
        .bind(question_no)
        .bind(display_order)
        .bind(question_id)
        .bind(doc)
        .execute(&state.pool)
        .await;
        // 集合关联
        let _ = sqlx::query(
            r#"
            UPDATE collection_questions
            SET question_no = COALESCE($1, question_no),
                display_order = COALESCE($2, display_order)
            WHERE question_id = $3
              AND collection_id IN (SELECT id FROM question_collections WHERE document_id = $4)
            "#,
        )
        .bind(question_no)
        .bind(display_order)
        .bind(question_id)
        .bind(doc)
        .execute(&state.pool)
        .await;
    }
}

async fn question_id_by_index(state: &AppState, task_id: Uuid, index: &str) -> Option<Uuid> {
    sqlx::query_scalar::<_, Option<serde_json::Value>>(
        "SELECT progress->'idempotency_map'->$2 FROM ai_parse_tasks WHERE id = $1",
    )
    .bind(task_id)
    .bind(index)
    .fetch_one(&state.pool)
    .await
    .ok()
    .flatten()
    .and_then(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok()))
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

// ---------------------------------------------------------------------------
// 测试（真实 DB，不依赖 LLM：persist_question / recover_stale_tasks 直测）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::types::{AnalysisMethod, ParsedAnswer};
    use crate::db;
    use serde_json::json;

    async fn test_state() -> Option<(AppState, Uuid)> {
        let _ = dotenvy::dotenv();
        let database_url = std::env::var("DATABASE_URL_TEST")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .ok()?;
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

    #[tokio::test]
    async fn test_persist_question_creates_and_dedups() {
        let Some((state, user_id)) = test_state().await else {
            eprintln!("跳过：未配置 DATABASE_URL");
            return;
        };
        let pool = state.pool.clone();

        // 准备：用户 + 文档 + 集合
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
        let auth = AuthUser {
            id: user_id,
            username: username.clone(),
            role: "teacher".into(),
            global_role: "teacher".into(),
        };
        let task = fake_task(user_id, doc_id);

        // 1. 新建题目 → 落库 + 集合关联 + hash
        let q1 = persist_question(
            &state,
            &task,
            "p1_i0",
            fake_parsed(Some("1"), "已知 $f(x)=x^2$，求极值。"),
            None,
            Some(collection_id),
            false,
            space_id,
            &auth,
        )
        .await
        .expect("persist 失败");

        let row: (String, String, String) = sqlx::query_as(
            "SELECT content_hash, normalized_content_hash, stem FROM questions WHERE id = $1",
        )
        .bind(q1)
        .fetch_one(&pool)
        .await
        .expect("查询题目失败");
        assert_eq!(row.0.len(), 64, "content_hash 缺失");
        assert_eq!(row.1.len(), 64, "normalized_content_hash 缺失");
        assert!(row.2.contains("极值"));

        let linked: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM collection_questions WHERE collection_id = $1 AND question_id = $2",
        )
        .bind(collection_id)
        .bind(q1)
        .fetch_one(&pool)
        .await
        .expect("查询关联失败");
        assert_eq!(linked, 1, "题目应关联到集合");
        let qno: Option<String> = sqlx::query_scalar(
            "SELECT question_no FROM collection_questions WHERE collection_id = $1 AND question_id = $2",
        )
        .bind(collection_id)
        .bind(q1)
        .fetch_one(&pool)
        .await
        .expect("查询题号失败");
        assert_eq!(qno.as_deref(), Some("1"));

        // 2. 同 index 重跑 → 幂等映射命中（返回同一题目）
        let q1_again = persist_question(
            &state,
            &task,
            "p1_i0",
            fake_parsed(Some("1"), "已知 $f(x)=x^2$，求极值。"),
            None,
            Some(collection_id),
            false,
            space_id,
            &auth,
        )
        .await
        .expect("幂等 persist 失败");
        assert_eq!(q1, q1_again, "同 index 重跑应复用同一题目");

        // 3. 不同 index 同内容 → hash 去重复用（不新建）
        let q2 = persist_question(
            &state,
            &task,
            "p1_i1",
            fake_parsed(Some("2"), "已知 $f(x)=x^2$，求极值。"),
            None,
            Some(collection_id),
            false,
            space_id,
            &auth,
        )
        .await
        .expect("去重 persist 失败");
        assert_eq!(q1, q2, "normalized hash 相同应复用题目");

        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM questions WHERE id = $1 OR id = $2")
            .bind(q1)
            .bind(q2)
            .fetch_one(&pool)
            .await
            .expect("统计失败");
        assert_eq!(total, 1, "不应产生重复题目");
    }

    #[tokio::test]
    async fn test_recover_stale_tasks() {
        let Some((state, user_id)) = test_state().await else {
            eprintln!("跳过：未配置 DATABASE_URL");
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
    async fn test_persist_question_creates_tag_candidates_for_unmatched() {
        let Some((state, user_id)) = test_state().await else {
            eprintln!("跳过：未配置 DATABASE_URL");
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
        let auth = AuthUser {
            id: user_id,
            username: username.clone(),
            role: "user".into(),
            global_role: "teacher".into(),
        };
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
        // 任务行必须真实存在（tag_candidates.source_task_id 外键）
        sqlx::query(
            "INSERT INTO ai_parse_tasks (id, creator_id, raw_text, status, document_id, progress, created_at, updated_at) VALUES ($1, $2, '', 'pending', $3, '{\"idempotency_map\": {}}', NOW(), NOW())",
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
        let mut parsed = fake_parsed(Some("1"), "带未知知识点的题目题干");
        parsed.knowledge_points = vec!["完全不存在的知识点XYZ_202608".to_string()];

        let qid = persist_question(
            &state,
            &task,
            "p1_i0",
            parsed,
            None,
            Some(collection_id),
            false,
            space_id,
            &auth,
        )
        .await
        .expect("persist 失败");

        // 候选已创建（不阻塞落库）
        let candidates: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tag_candidates WHERE source_question_id = $1 AND status = 'pending'",
        )
        .bind(qid)
        .fetch_one(&pool)
        .await
        .expect("查询候选失败");
        assert_eq!(candidates, 1, "未匹配知识点应进入候选队列");

        let raw: String = sqlx::query_scalar(
            "SELECT raw_name FROM tag_candidates WHERE source_question_id = $1",
        )
        .bind(qid)
        .fetch_one(&pool)
        .await
        .expect("查询候选名失败");
        assert_eq!(raw, "完全不存在的知识点XYZ_202608");

        // 幂等：同一任务同一题重跑（不同 index 但同内容 hash 复用 → 不重复建候选）
        // （hash 复用路径不会再次进入匹配，候选保持 1 条）
        let parsed2 = {
            let mut p = fake_parsed(Some("1"), "带未知知识点的题目题干");
            p.knowledge_points = vec!["完全不存在的知识点XYZ_202608".to_string()];
            p
        };
        let _ = persist_question(
            &state,
            &task,
            "p1_i1",
            parsed2,
            None,
            Some(collection_id),
            false,
            space_id,
            &auth,
        )
        .await
        .expect("去重 persist 失败");
        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tag_candidates WHERE source_question_id = $1",
        )
        .bind(qid)
        .fetch_one(&pool)
        .await
        .expect("统计候选失败");
        assert_eq!(total, 1, "hash 复用的题目不应重复产生候选");
    }
}
