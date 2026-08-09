use std::time::Duration;
use uuid::Uuid;

use crate::ai::cleaner::clean_and_parse;
use crate::ai::ocr::{create_ocr_provider, should_fallback, OcrError, OcrProvider, QwenVlOcrProvider};
use crate::ai::prompt::STAGE2_PARSE_FULL_PROMPT;
use crate::ai::provider::{create_provider, AiError};
use crate::ai::types::ParsedQuestion;
use crate::auth::middleware::AuthUser;
use crate::auth::permissions::ensure_personal_space;
use crate::handlers::ai::{post_process_batch, resolve_ai_config, resolve_ocr_config, ModelKind};
use crate::handlers::ai_tagging::{match_knowledge_nodes, KnowledgeNodeMatch};
use crate::handlers::questions::{save_version, upsert_ai_knowledge_nodes};
use crate::models::ai_task::{AiParseTask, AiTaskSourceType};
use crate::models::question::{refresh_system_flags, Difficulty, QuestionStatus, QuestionType};
use crate::AppState;

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 启动 AI 解析 worker 后台协程
///
/// 在 `main.rs` 中通过 `tokio::spawn(start_worker(state.clone()))` 启动。
/// 该函数永不返回（死循环），仅在协程被取消时退出。
pub async fn start_worker(state: AppState) {
    tracing::info!("🤖 AI 解析 worker 已启动，每 2s 轮询一次 pending 任务");

    loop {
        match process_one_task(&state).await {
            Ok(true) => {
                // 处理了一个任务，立即继续下一轮
            }
            Ok(false) => {
                // 队列为空，休眠 2 秒后重试
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Err(e) => {
                // 循环本身异常（DB 连接断开等），5 秒后重试
                tracing::error!("Worker 循环异常: {e}，5 秒后重试");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 单任务处理
// ---------------------------------------------------------------------------

/// 拾取并处理一个 pending 任务
///
/// 返回值：
/// - `Ok(true)`：处理了一个任务（无论成功或失败，都标记了最终状态）
/// - `Ok(false)`：队列无 pending 任务
/// - `Err(_)`：循环级异常（DB 错误等），应延迟后重试
async fn process_one_task(state: &AppState) -> Result<bool, String> {
    // 1. 原子拾取：UPDATE ... WHERE id = (SELECT ... FOR UPDATE SKIP LOCKED)
    //    避免多 worker 并发时重复消费
    //    M4：RETURNING 包含新增字段 source_type/image_b64/pdf_bytes/ocr_provider_override/question_ids
    let task: Option<AiParseTask> = sqlx::query_as::<_, AiParseTask>(
        r#"
        UPDATE ai_parse_tasks
        SET status = 'processing', updated_at = NOW()
        WHERE id = (
            SELECT id FROM ai_parse_tasks
            WHERE status = 'pending'
            ORDER BY created_at ASC
            LIMIT 1
            FOR UPDATE SKIP LOCKED
        )
        RETURNING id, creator_id, raw_text, source_type, image_b64, pdf_bytes,
                  ocr_provider_override, status, question_id, question_ids,
                  error_message, created_at, updated_at
        "#,
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| format!("拾取任务失败: {e}"))?;

    let Some(task) = task else {
        return Ok(false);
    };

    let task_id = task.id;
    tracing::info!(
        "Worker 拾取任务 {task_id}（creator={}, source_type={:?}）",
        task.creator_id,
        task.source_type
    );

    // 2. 按 source_type 分派执行路径
    let exec_result: Result<Vec<Uuid>, String> = match task.source_type {
        AiTaskSourceType::Text => match execute_text_task(state, &task).await {
            Ok(qid) => Ok(vec![qid]),
            Err(e) => Err(e),
        },
        AiTaskSourceType::Image => execute_image_task(state, &task).await,
        AiTaskSourceType::Pdf => execute_pdf_task(state, &task).await,
    };

    // 3. 根据执行结果标记任务状态
    match exec_result {
        Ok(question_ids) => {
            let primary = question_ids.first().copied();
            let ids_json = serde_json::to_value(&question_ids).ok();
            let count = question_ids.len();

            if let Err(e) = sqlx::query(
                r#"
                UPDATE ai_parse_tasks
                SET status = 'completed',
                    question_id = $1,
                    question_ids = $2,
                    image_b64 = NULL,
                    pdf_bytes = NULL,
                    updated_at = NOW()
                WHERE id = $3
                "#,
            )
            .bind(primary)
            .bind(ids_json)
            .bind(task_id)
            .execute(&state.pool)
            .await
            {
                tracing::error!(
                    "任务 {task_id} 标记 completed 失败: {e}（已生成 {count} 题）"
                );
            } else {
                tracing::info!("✅ 任务 {task_id} 完成，生成 {count} 题");
            }
        }
        Err(e) => {
            tracing::warn!("❌ 任务 {task_id} 失败: {e}");
            // 截断超长错误信息，避免 DB 列溢出
            let short_err: String = if e.chars().count() > 2000 {
                format!("{}...", e.chars().take(2000).collect::<String>())
            } else {
                e.clone()
            };
            if let Err(e2) = sqlx::query(
                r#"
                UPDATE ai_parse_tasks
                SET status = 'failed',
                    error_message = $1,
                    image_b64 = NULL,
                    pdf_bytes = NULL,
                    updated_at = NOW()
                WHERE id = $2
                "#,
            )
            .bind(&short_err)
            .bind(task_id)
            .execute(&state.pool)
            .await
            {
                tracing::error!("任务 {task_id} 标记 failed 失败: {e2}");
            }
        }
    }

    Ok(true)
}

// ---------------------------------------------------------------------------
// 路径 A：文本任务（旧路径，保持等价行为）
// ---------------------------------------------------------------------------

/// 执行文本类解析任务：调用 LLM → 清洗 JSON → 落库为新题目（草稿）
///
/// 返回新题目的 ID；失败返回错误信息字符串。
async fn execute_text_task(state: &AppState, task: &AiParseTask) -> Result<Uuid, String> {
    let raw_text = task
        .raw_text
        .as_ref()
        .ok_or_else(|| "文本任务缺少 raw_text 字段".to_string())?;

    // 1. 加载 creator 信息（resolve_ai_config 需要 AuthUser）
    let auth = load_task_auth(state, task.creator_id).await?;

    // 2. 解析 AI 配置（用户个人 Key 优先，否则平台默认）
    let (api_key, provider_name, model, base_url) =
        resolve_ai_config(&auth, state, ModelKind::Text).await?;

    // 3. 调用 LLM
    let provider = create_provider(&provider_name, &api_key, &base_url);
    let raw_json = provider
        .parse_text(raw_text, model.as_deref())
        .await
        .map_err(map_ai_error)?;

    // 4. 清洗 & 反序列化
    let parsed: ParsedQuestion = clean_and_parse(&raw_json)
        .map_err(|e| format!("AI 返回 JSON 解析失败: {e}"))?;

    // 5. 加载 display_name 与 space_id
    let display_name = load_display_name(state, task.creator_id).await?;
    let space_id = ensure_personal_space(
        &state.pool,
        task.creator_id,
        display_name.as_deref().unwrap_or("用户"),
    )
    .await
    .map_err(|e| format!("创建个人空间失败: {e}"))?;

    // 6. 落库为新题目
    save_parsed_question(state, task.creator_id, space_id, parsed).await
}

// ---------------------------------------------------------------------------
// 路径 B：图片任务（M4 新增）
// ---------------------------------------------------------------------------

/// 执行图片类解析任务：OCR → Stage 2 LLM → 批量后处理 → 落库多题
///
/// 返回所有生成题目的 ID 列表；失败返回错误信息字符串。
async fn execute_image_task(state: &AppState, task: &AiParseTask) -> Result<Vec<Uuid>, String> {
    let image_b64 = task
        .image_b64
        .as_ref()
        .ok_or_else(|| "图片任务缺少 image_b64 字段".to_string())?;

    let auth = load_task_auth(state, task.creator_id).await?;

    // Stage 1：OCR → Markdown（与 parse_image_v2 一致，含 Qwen-VL 兜底）
    let markdown = run_ocr_with_fallback(
        state,
        &auth,
        image_b64,
        None, // image 任务无 PDF bytes
        task.ocr_provider_override.as_deref(),
    )
    .await?;

    // Stage 2 + 后处理 + 批量落库
    run_stage2_and_save(state, &auth, &markdown).await
}

// ---------------------------------------------------------------------------
// 路径 C：PDF 任务（M4 新增）
// ---------------------------------------------------------------------------

/// 执行 PDF 类解析任务：OCR PDF → Stage 2 LLM → 批量后处理 → 落库多题
///
/// 返回所有生成题目的 ID 列表；失败返回错误信息字符串。
async fn execute_pdf_task(state: &AppState, task: &AiParseTask) -> Result<Vec<Uuid>, String> {
    let pdf_bytes = task
        .pdf_bytes
        .as_ref()
        .ok_or_else(|| "PDF 任务缺少 pdf_bytes 字段".to_string())?
        .clone();

    let auth = load_task_auth(state, task.creator_id).await?;

    // Stage 1：OCR PDF → Markdown
    // PDF 任务必须用支持 PDF 的引擎（doc2x / mineru_local），不允许走 qwen_vl
    let markdown = run_ocr_with_fallback(
        state,
        &auth,
        "", // image_b64 留空
        Some(&pdf_bytes),
        task.ocr_provider_override.as_deref(),
    )
    .await?;

    // Stage 2 + 后处理 + 批量落库
    run_stage2_and_save(state, &auth, &markdown).await
}

// ---------------------------------------------------------------------------
// 共享辅助函数
// ---------------------------------------------------------------------------

/// 加载任务发起人的 AuthUser（用于后续 resolve_ai_config / resolve_ocr_config）
///
/// 注意：users.role / global_role 是 enum 类型，必须用 ::text 强制转换。
async fn load_task_auth(
    state: &AppState,
    creator_id: Uuid,
) -> Result<AuthUser, String> {
    let user_row: Option<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT username, role::text, global_role::text, display_name FROM users WHERE id = $1",
    )
    .bind(creator_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| format!("查询用户失败: {e}"))?;

    let Some((username, role, global_role, _display_name)) = user_row else {
        return Err(format!("用户 {creator_id} 不存在"));
    };

    Ok(AuthUser {
        id: creator_id,
        username,
        role,
        global_role,
    })
}

/// 加载用户 display_name（用于 ensure_personal_space 的命名）
async fn load_display_name(
    state: &AppState,
    user_id: Uuid,
) -> Result<Option<String>, String> {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT display_name FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| format!("查询 display_name 失败: {e}"))?;

    Ok(row.and_then(|(d,)| d))
}

/// OCR Stage 1：调用 OCR 引擎 → Markdown（含 Qwen-VL 兜底降级）
///
/// - `image_b64` 非空时走图片路径（ocr_image）
/// - `pdf_bytes` 非空时走 PDF 路径（ocr_pdf_async，仅支持 doc2x/mineru）
/// - 二者不可同时为空
///
/// 失败时按 `should_fallback` 判断是否切换 Qwen-VL 兜底重试。
/// PDF 任务因 Qwen-VL 不支持 PDF，无法兜底，错误直接透传。
async fn run_ocr_with_fallback(
    state: &AppState,
    auth: &AuthUser,
    image_b64: &str,
    pdf_bytes: Option<&[u8]>,
    ocr_provider_override: Option<&str>,
) -> Result<String, String> {
    let ocr_cfg = resolve_ocr_config(auth, state, ocr_provider_override).await?;
    let provider = create_ocr_provider(&ocr_cfg);
    let primary_id = provider.id();

    tracing::info!("Worker OCR Stage1 engine={primary_id}");

    let primary_result = if let Some(pdf) = pdf_bytes {
        // PDF 路径
        provider.ocr_pdf_async(pdf).await
    } else {
        // 图片路径
        provider.ocr_image(image_b64).await
    };

    match primary_result {
        Ok(md) => Ok(md),
        Err(e) if primary_id != "qwen_vl" && should_fallback(&e) => {
            // PDF 任务不允许走 qwen_vl 兜底（不支持 PDF）
            if pdf_bytes.is_some() {
                return Err(format!(
                    "PDF OCR 引擎 {primary_id} 失败（{:?}），且 Qwen-VL 不支持 PDF，无法兜底",
                    e
                ));
            }
            // 图片任务可降级 Qwen-VL
            tracing::warn!(
                "OCR 引擎 {primary_id} 失败（{:?}），自动切换 Qwen-VL 兜底重试",
                e
            );
            let (fb_api_key, _fb_provider, fb_model, fb_base_url) =
                resolve_ai_config(auth, state, ModelKind::Vision).await?;
            let fallback_provider =
                QwenVlOcrProvider::new(fb_api_key, fb_base_url, fb_model);
            fallback_provider
                .ocr_image(image_b64)
                .await
                .map_err(|e| format!("Qwen-VL 兜底失败: {:?}", e))
        }
        Err(e) => Err(format!("OCR 引擎 {primary_id} 失败: {:?}", e)),
    }
}

/// Stage 2：文本 LLM 结构化 → JSON → 批量后处理 → 批量落库
///
/// 返回所有生成题目的 ID 列表。
async fn run_stage2_and_save(
    state: &AppState,
    auth: &AuthUser,
    markdown: &str,
) -> Result<Vec<Uuid>, String> {
    // Stage 2：文本 LLM 结构化
    let (text_api_key, text_provider_name, text_model, text_base_url) =
        resolve_ai_config(auth, state, ModelKind::Text).await?;
    let text_provider = create_provider(&text_provider_name, &text_api_key, &text_base_url);
    let raw_json = text_provider
        .parse_text_with_prompt(markdown, &STAGE2_PARSE_FULL_PROMPT, text_model.as_deref())
        .await
        .map_err(map_ai_error)?;

    // 批量后处理（截断容错 + 逐题隔离 + 知识点匹配）
    let questions = post_process_batch(&raw_json, &state.pool)
        .await
        .map_err(|(_code, msg)| {
            // 从 Json Value 中提取 error 字段
            let err_str = msg
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("批量后处理失败");
            err_str.to_string()
        })?;

    if questions.is_empty() {
        return Err("AI 未识别出有效题目".to_string());
    }

    // 加载 space_id（用于题目落库）
    let display_name = load_display_name(state, auth.id).await?;
    let space_id = ensure_personal_space(
        &state.pool,
        auth.id,
        display_name.as_deref().unwrap_or("用户"),
    )
    .await
    .map_err(|e| format!("创建个人空间失败: {e}"))?;

    // 逐题落库，收集 ID
    let mut question_ids = Vec::with_capacity(questions.len());
    for q in questions {
        match save_parsed_question(state, auth.id, space_id, q).await {
            Ok(qid) => question_ids.push(qid),
            Err(e) => {
                tracing::warn!("批量落库中某题失败（不影响其他题）: {e}");
            }
        }
    }

    if question_ids.is_empty() {
        return Err("所有题目落库均失败".to_string());
    }

    Ok(question_ids)
}

/// 将单个 ParsedQuestion 落库为新题目（草稿）
///
/// 与 `handlers::questions::create_question` 流程一致：
/// - 转换 question_type / difficulty / options / correct_answer / analysis
/// - 知识点模糊匹配 + upsert
/// - 事务：INSERT question + upsert_ai_knowledge_nodes + save_version
///
/// 返回新题目的 UUID。
async fn save_parsed_question(
    state: &AppState,
    creator_id: Uuid,
    space_id: Uuid,
    parsed: ParsedQuestion,
) -> Result<Uuid, String> {
    // 1. 题型映射
    let question_type = match parsed.question_type.as_str() {
        "choice" => QuestionType::Choice,
        "multiple" => QuestionType::Multiple,
        "fill" => QuestionType::Fill,
        "solution" => QuestionType::Solution,
        other => return Err(format!("未知题型: {other}")),
    };

    // 2. 难度映射
    let difficulty = match parsed.difficulty.as_deref() {
        Some("easy") => Difficulty(2),
        Some("hard") => Difficulty(4),
        _ => Difficulty(3),
    };

    // 3. options / correct_answer / analysis 序列化
    let options_json = parsed
        .options
        .as_ref()
        .map(|opts| serde_json::to_value(opts).unwrap_or(serde_json::Value::Null));
    // Option<ParsedAnswer>：None → JSON null 落库（非 SQL NULL），自动触发 pending_answer
    let correct_answer_opt: Option<serde_json::Value> = match &parsed.correct_answer {
        Some(a) => Some(
            serde_json::to_value(a).map_err(|e| format!("序列化 correct_answer 失败: {e}"))?,
        ),
        None => None,
    };

    // analysis 拼接：项目约定用 \n\n---\n\n 分隔多解法（前端反向 split）
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

    // ── AI 异常降级 ──
    // stem 兜底：AI 连题干都未提取出（全黑/乱码图片）时填入占位符，
    // 避免空 stem 影响后续审阅（NOT NULL 不违约，但空 stem 无业务价值）
    let stem = if parsed.stem.trim().is_empty() {
        tracing::warn!("AI 提取题干失败，填入占位符等待人工补充");
        "[AI提取题干失败，请老师人工补充]".to_string()
    } else {
        parsed.stem.clone()
    };

    // ── 异步补全：根据 AI 提取结果刷新 system_flags ──
    // correct_answer 为 None → pending_answer=true；analysis 为空 → missing_analysis=true
    let mut metadata = serde_json::json!({});
    refresh_system_flags(&mut metadata, &correct_answer_opt, &analysis_str);

    // 4. 知识点模糊匹配（B3 修复：批量录题不丢失知识点关联）
    let (ai_matches, primary_node_id): (Vec<KnowledgeNodeMatch>, Option<Uuid>) =
        if !parsed.knowledge_points.is_empty() {
            match match_knowledge_nodes(&state.pool, &parsed.knowledge_points, None).await {
                Ok((matched, _unmatched)) => {
                    for m in &matched {
                        if m.score < 0.95 {
                            tracing::info!(
                                "批量录题知识点「{}」模糊匹配到「{}」(相似度 {:.0}%)",
                                m.ai_name,
                                m.node_name,
                                m.score * 100.0
                            );
                        }
                    }
                    let primary = matched
                        .iter()
                        .max_by(|a, b| {
                            a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .map(|m| m.node_id);
                    (matched, primary)
                }
                Err(e) => {
                    tracing::warn!("批量录题知识点匹配失败（不影响录题）: {:?}", e.1);
                    (vec![], None)
                }
            }
        } else {
            (vec![], None)
        };

    // 5. 事务：插入题目 + 关联知识点 + 生成版本快照
    let id = Uuid::new_v4();
    let now = chrono::Utc::now();
    let version: i32 = 1;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| format!("开启事务失败: {e}"))?;

    sqlx::query(
        r#"
        INSERT INTO questions (id, stem, stem_text, images,
            question_type, options, correct_answer, analysis, grading_criteria,
            difficulty, difficulty_score, default_score, estimated_minutes, cognitive_level,
            grade_level, semester, source, exam_type, metadata,
            parent_id, sub_order,
            status, space_id, origin_question_id,
            creator_id, created_at, updated_by, updated_at, version)
        VALUES ($1, $2, NULL, NULL,
            $3, $4, $5, $6, NULL,
            $7, NULL, $8, NULL, NULL,
            NULL, NULL, NULL, NULL, COALESCE($9, '{}'::jsonb),
            NULL, NULL,
            $10, $11, NULL,
            $12, $13, NULL, $14, $15)
        "#,
    )
    .bind(id)
    .bind(&stem)
    .bind(question_type)
    .bind(&options_json)
    .bind(correct_answer_opt.as_ref().unwrap_or(&serde_json::Value::Null))
    .bind(&analysis_str)
    .bind(difficulty)
    .bind(5)
    .bind(&metadata)
    .bind(QuestionStatus::Draft)
    .bind(space_id)
    .bind(creator_id)
    .bind(now)
    .bind(now)
    .bind(version)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("插入题目失败: {e}"))?;

    if !ai_matches.is_empty() {
        upsert_ai_knowledge_nodes(&mut tx, id, &ai_matches, primary_node_id)
            .await
            .map_err(|e| format!("关联知识点失败: {e}"))?;
    }

    save_version(&mut tx, id, version, Some(creator_id))
        .await
        .map_err(|e| format!("保存版本快照失败: {e}"))?;

    tx.commit().await.map_err(|e| format!("提交事务失败: {e}"))?;

    Ok(id)
}

// ---------------------------------------------------------------------------
// 错误映射
// ---------------------------------------------------------------------------

/// 将 AiError 映射为可读字符串（worker 内部使用，不返回 HTTP 状态）
fn map_ai_error(e: AiError) -> String {
    match e {
        AiError::NoApiKey => "未配置 AI API Key".to_string(),
        AiError::Upstream(status, msg) => {
            let short = if msg.chars().count() > 500 {
                format!("{}...", msg.chars().take(500).collect::<String>())
            } else {
                msg
            };
            format!("AI 上游错误 (HTTP {status}): {short}")
        }
        AiError::Timeout => "AI 调用超时（120s）".to_string(),
    }
}

// Allow unused import warnings for OcrError if not directly referenced
#[allow(dead_code)]
fn _unused_ocr_error_marker(_: OcrError) {}
