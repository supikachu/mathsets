//! 异步智能打标 Worker
//!
//! 拾取 `ai_tagging_tasks` → 调用统一 TaggingEngine → 回写 suggestion_id。
//! 候选仍只在用户确认保存后由 Finalizer 产生；唯一例外是题目已先落库时，
//! 完成后会把建议认领到该题（否则打标晚于保存就等于标签丢失）。

use std::time::Duration;
use uuid::Uuid;

use crate::ai::provider::{create_provider, is_rate_limit_message, is_transient_openrouter_error, AiError, RATE_LIMIT_USER_MESSAGE, OPENROUTER_PROVIDER_ERROR_USER_MESSAGE};
use crate::ai::tagging::engine::TaggingError;
use crate::ai::tagging::{
    claim_suggestion_for_saved_question, run_tagging, TaggingContext, TaggingInput, TaggingPolicy,
    TaggingSignals, TaggingSuggestion,
};
use crate::auth::middleware::AuthUser;
use crate::handlers::ai::{resolve_ai_config, ModelKind};
use crate::models::ai_tagging_task::{AiTaggingTask, TAGGING_TASK_COLUMNS};
use crate::AppState;
use serde_json::json;

const HEARTBEAT_TIMEOUT: &str = "120 seconds";
const MAX_RETRIES: i32 = 2;

struct TaskFailure {
    retryable: bool,
    message: String,
}

const DEFAULT_WORKER_CONCURRENCY: usize = 4;

/// 并发数；`TAGGING_WORKER_CONCURRENCY` 可覆盖，上游频繁 429 时下调。
fn worker_concurrency() -> usize {
    std::env::var("TAGGING_WORKER_CONCURRENCY")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_WORKER_CONCURRENCY)
        .clamp(1, 32)
}

/// 启动打标 worker 组。
///
/// 打标耗时几乎全在 LLM 往返上（实测单题 ~185s，数据库与向量召回合计仅 2s），
/// 串行拾取会让整卷时间等于题数乘以单题耗时。拾取 SQL 用 `FOR UPDATE SKIP LOCKED`，
/// 并发拾取本身安全。
///
/// `abandon_orphaned_processing` 会取消所有 processing 行，因此只能在拉起任何轮询
/// 循环之前跑一次：若每个 worker 各跑一次，后启动的会把先启动的在跑任务取消掉。
pub async fn start_worker(state: AppState) {
    abandon_orphaned_processing(&state, "tag-worker-boot").await;

    let n = worker_concurrency();
    tracing::info!("🏷️ AI 打标 worker 组已启动（并发 {n}），每 2s 轮询一次任务");

    let mut handles = Vec::with_capacity(n);
    for _ in 0..n {
        let state = state.clone();
        handles.push(tokio::spawn(run_worker_loop(state)));
    }
    for h in handles {
        let _ = h.await;
    }
}

async fn run_worker_loop(state: AppState) {
    let worker_id = format!("tag-worker-{}", Uuid::new_v4().simple());
    loop {
        recover_stale_tasks(&state, &worker_id).await;
        match process_one_task(&state, &worker_id).await {
            Ok(true) => {}
            Ok(false) => {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Err(e) => {
                tracing::error!("打标 Worker {worker_id} 循环异常: {e}，5 秒后重试");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

async fn abandon_orphaned_processing(state: &AppState, worker_id: &str) {
    let n = sqlx::query(
        r#"
        UPDATE ai_tagging_tasks
        SET status = 'cancelled',
            error_message = '后台进程已停止，任务已终止（不会自动续跑）',
            completed_at = NOW(), locked_at = NULL, worker_id = NULL, updated_at = NOW()
        WHERE status = 'processing'
        "#,
    )
    .execute(&state.pool)
    .await
    .map(|r| r.rows_affected())
    .unwrap_or(0);
    if n > 0 {
        tracing::warn!("打标 Worker {worker_id} 终止上一次遗留 processing 任务 {n} 个");
    }
}

async fn recover_stale_tasks(state: &AppState, worker_id: &str) {
    let cancelled = sqlx::query(
        r#"
        UPDATE ai_tagging_tasks
        SET status = 'cancelled',
            error_message = COALESCE(error_message, '任务已取消'),
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
        UPDATE ai_tagging_tasks
        SET status = 'cancelled',
            error_message = '任务处理中断（超时或进程退出），未自动续跑',
            completed_at = NOW(), updated_at = NOW(),
            locked_at = NULL, worker_id = NULL
        WHERE status = 'processing'
          AND heartbeat_at < NOW() - INTERVAL '{HEARTBEAT_TIMEOUT}'
        "#
    ))
    .execute(&state.pool)
    .await
    .map(|r| r.rows_affected())
    .unwrap_or(0);

    if cancelled > 0 || stale > 0 {
        tracing::warn!("打标 Worker {worker_id} 清理任务：cancelled={cancelled} stale={stale}");
    }
}

async fn process_one_task(state: &AppState, worker_id: &str) -> Result<bool, String> {
    let task: Option<AiTaggingTask> = sqlx::query_as::<_, AiTaggingTask>(&format!(
        r#"
        UPDATE ai_tagging_tasks
        SET status = 'processing', locked_at = NOW(), worker_id = $1,
            heartbeat_at = NOW(), started_at = COALESCE(started_at, NOW()), updated_at = NOW()
        WHERE id = (
            SELECT id FROM ai_tagging_tasks
            WHERE status IN ('pending', 'retrying')
              AND cancel_requested_at IS NULL
            ORDER BY created_at ASC
            LIMIT 1
            FOR UPDATE SKIP LOCKED
        )
        RETURNING {TAGGING_TASK_COLUMNS}
        "#
    ))
    .bind(worker_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| format!("拾取打标任务失败: {e}"))?;

    let Some(task) = task else {
        return Ok(false);
    };

    tracing::info!(
        "打标 Worker {worker_id} 拾取任务 {}（第 {} 次执行）",
        task.id,
        task.retry_count + 1
    );

    if task.cancel_requested_at.is_some() {
        mark_cancelled(&state.pool, task.id, task.suggestion_id).await?;
        return Ok(true);
    }

    let heartbeat = spawn_heartbeat(state.pool.clone(), task.id);
    let outcome = tokio::select! {
        r = execute_task(state, &task) => r,
        _ = wait_until_tag_cancel(&state.pool, task.id) => {
            heartbeat.abort();
            mark_cancelled(&state.pool, task.id, None).await?;
            return Ok(true);
        }
    };
    heartbeat.abort();

    match outcome {
        Ok(suggestion_id) => {
            if cancel_requested(&state.pool, task.id).await? {
                mark_cancelled(&state.pool, task.id, Some(suggestion_id)).await?;
            } else {
                sqlx::query(
                    r#"
                    UPDATE ai_tagging_tasks
                    SET status = 'success', suggestion_id = $2, error_message = NULL,
                        completed_at = NOW(), updated_at = NOW(),
                        locked_at = NULL, worker_id = NULL
                    WHERE id = $1
                    "#,
                )
                .bind(task.id)
                .bind(suggestion_id)
                .execute(&state.pool)
                .await
                .map_err(|e| format!("标记打标成功失败: {e}"))?;
                tracing::info!("✅ 打标任务 {} 成功", task.id);
                write_back_parse_staging(&state.pool, &task, Some(suggestion_id), "done").await;
                claim_if_question_already_saved(&state.pool, &task, suggestion_id).await;
            }
        }
        Err(failure) => {
            if cancel_requested(&state.pool, task.id).await.unwrap_or(false) {
                mark_cancelled(&state.pool, task.id, None).await?;
                return Ok(true);
            }

            let short: String = if failure.message.chars().count() > 1000 {
                format!(
                    "{}...",
                    failure.message.chars().take(1000).collect::<String>()
                )
            } else {
                failure.message.clone()
            };

            if failure.retryable && task.retry_count < MAX_RETRIES {
                sqlx::query(
                    r#"
                    UPDATE ai_tagging_tasks
                    SET status = 'retrying', retry_count = retry_count + 1,
                        error_message = $1, updated_at = NOW(),
                        locked_at = NULL, worker_id = NULL
                    WHERE id = $2
                    "#,
                )
                .bind(&short)
                .bind(task.id)
                .execute(&state.pool)
                .await
                .map_err(|e| format!("标记打标重试失败: {e}"))?;
                tracing::warn!(
                    task_id = %task.id,
                    retryable = true,
                    retry_count = task.retry_count + 1,
                    "打标任务将重试: {short}"
                );
            } else {
                sqlx::query(
                    r#"
                    UPDATE ai_tagging_tasks
                    SET status = 'failed', error_message = $1,
                        completed_at = NOW(), updated_at = NOW(),
                        locked_at = NULL, worker_id = NULL
                    WHERE id = $2
                    "#,
                )
                .bind(&short)
                .bind(task.id)
                .execute(&state.pool)
                .await
                .map_err(|e| format!("标记打标失败失败: {e}"))?;
                tracing::warn!(
                    task_id = %task.id,
                    retryable = failure.retryable,
                    retry_count = task.retry_count,
                    "打标任务失败: {short}"
                );
                write_back_parse_staging(&state.pool, &task, None, "failed").await;
            }
        }
    }

    Ok(true)
}

async fn write_back_parse_staging(
    pool: &sqlx::PgPool,
    task: &AiTaggingTask,
    suggestion_id: Option<Uuid>,
    tagging_status: &str,
) {
    let Some(parse_id) = task.parse_task_id else {
        return;
    };
    let Some(index) = task.source_index.as_deref() else {
        return;
    };

    let mut patch = json!({ "tagging_status": tagging_status });
    if let Some(sid) = suggestion_id {
        let result: Option<serde_json::Value> =
            sqlx::query_scalar("SELECT result FROM ai_tagging_suggestions WHERE id = $1")
                .bind(sid)
                .fetch_optional(pool)
                .await
                .ok()
                .flatten();
        if let Some(result) = result {
            if let Ok(s) = serde_json::from_value::<TaggingSuggestion>(result.clone()) {
                patch = json!({
                    "tagging_status": tagging_status,
                    "suggestion_id": sid,
                    "engine_version": s.engine_version,
                    "suggestion": result,
                    "matched": s.compat_matched_nodes(),
                    "unmatched": serde_json::Value::Object(s.compat_unmatched_map()),
                });
            }
        }
    }

    if let Err(e) = sqlx::query(
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
    {
        tracing::warn!(
            parse_task_id = %parse_id,
            index,
            "回写解析暂存打标结果失败: {e}"
        );
    }
}

/// 打标完成时题目可能已经确认保存（用户等不到标签就点了保存）。
/// 此时建议不会有任何落点，需要主动挂到已保存的题目上，否则标签永久丢失。
async fn claim_if_question_already_saved(
    pool: &sqlx::PgPool,
    task: &AiTaggingTask,
    suggestion_id: Uuid,
) {
    // 编辑页手动打标：任务自带 question_id，前端会带 confirmation 保存，无需认领
    if task.question_id.is_some() {
        return;
    }
    let Some(parse_id) = task.parse_task_id else {
        return;
    };
    let Some(index) = task.source_index.as_deref() else {
        return;
    };

    let saved: Option<String> = sqlx::query_scalar(
        r#"
        SELECT elem->>'saved_question_id'
        FROM ai_parse_tasks t,
             jsonb_array_elements(COALESCE(t.progress->'staged_questions', '[]'::jsonb)) AS elem
        WHERE t.id = $1
          AND elem->>'index' = $2
          AND elem->>'saved_question_id' IS NOT NULL
        LIMIT 1
        "#,
    )
    .bind(parse_id)
    .bind(index)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let Some(question_id) = saved.as_deref().and_then(|s| Uuid::parse_str(s).ok()) else {
        return;
    };

    if let Err(e) = claim_suggestion_for_saved_question(pool, suggestion_id, question_id).await {
        tracing::warn!(
            suggestion_id = %suggestion_id,
            question_id = %question_id,
            "认领打标建议到已保存题目失败: {e}"
        );
    }
}

fn spawn_heartbeat(pool: sqlx::PgPool, task_id: Uuid) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(20)).await;
            let _ = sqlx::query(
                "UPDATE ai_tagging_tasks SET heartbeat_at = NOW() \
                 WHERE id = $1 AND status = 'processing' AND cancel_requested_at IS NULL",
            )
            .bind(task_id)
            .execute(&pool)
            .await;
        }
    })
}

async fn cancel_requested(pool: &sqlx::PgPool, task_id: Uuid) -> Result<bool, String> {
    let flag: bool = sqlx::query_scalar(
        "SELECT cancel_requested_at IS NOT NULL FROM ai_tagging_tasks WHERE id = $1",
    )
    .bind(task_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("查询取消标记失败: {e}"))?;
    Ok(flag)
}

async fn wait_until_tag_cancel(pool: &sqlx::PgPool, task_id: Uuid) {
    loop {
        if cancel_requested(pool, task_id).await.unwrap_or(false) {
            return;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn mark_cancelled(
    pool: &sqlx::PgPool,
    task_id: Uuid,
    suggestion_id: Option<Uuid>,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE ai_tagging_tasks
        SET status = 'cancelled', suggestion_id = COALESCE($2, suggestion_id),
            completed_at = NOW(), updated_at = NOW(),
            locked_at = NULL, worker_id = NULL
        WHERE id = $1
        "#,
    )
    .bind(task_id)
    .bind(suggestion_id)
    .execute(pool)
    .await
    .map_err(|e| format!("标记打标取消失败: {e}"))?;
    tracing::info!("打标任务 {task_id} 已取消");
    Ok(())
}

async fn execute_task(state: &AppState, task: &AiTaggingTask) -> Result<Uuid, TaskFailure> {
    if task.content.trim().is_empty() {
        return Err(TaskFailure {
            retryable: false,
            message: "题目文本不能为空".into(),
        });
    }

    let user_row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT username, role::text, global_role::text FROM users WHERE id = $1",
    )
    .bind(task.creator_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| TaskFailure {
        retryable: true,
        message: format!("查询用户失败: {e}"),
    })?;
    let Some((username, role, global_role)) = user_row else {
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

    let (api_key, provider_name, model, base_url) =
        resolve_ai_config(&auth, state, ModelKind::Tagging)
            .await
            .map_err(|e| TaskFailure {
                retryable: false,
                message: e,
            })?;
    let provider = create_provider(&provider_name, &api_key, &base_url);

    if cancel_requested(&state.pool, task.id)
        .await
        .map_err(|e| TaskFailure {
            retryable: true,
            message: e,
        })?
    {
        return Err(TaskFailure {
            retryable: false,
            message: "已取消".into(),
        });
    }

    let ctx = TaggingContext {
        user_id: task.creator_id,
        space_id: task.space_id,
        question_id: task.question_id,
        source_task_id: task.parse_task_id,
        source_index: task.source_index.clone(),
        stage: task.stage.clone(),
    };

    // 解析阶段带来的信号足够时，引擎会跳过 LLM 提取；反序列化失败则退回纯题文提取
    let input = match task
        .parsed_signals
        .clone()
        .and_then(|v| serde_json::from_value::<TaggingSignals>(v).ok())
    {
        Some(signals) => TaggingInput::ContentWithSignals {
            content: task.content.clone(),
            signals: Box::new(signals),
        },
        None => TaggingInput::Content {
            content: task.content.clone(),
        },
    };

    let suggestion = run_tagging(
        &state.pool,
        Some(provider.as_ref()),
        model.as_deref(),
        input,
        &ctx,
        &TaggingPolicy::default(),
    )
    .await
    .map_err(map_tagging_failure)?;

    suggestion.suggestion_id.ok_or_else(|| TaskFailure {
        retryable: true,
        message: "打标建议未写入".into(),
    })
}

fn map_tagging_failure(e: TaggingError) -> TaskFailure {
    let retryable = is_retryable(&e);
    let message = match &e {
        TaggingError::EmptyContent => "题目文本不能为空".into(),
        TaggingError::ExtractParse(msg) => format!("AI 返回格式损坏: {msg}"),
        TaggingError::Ai(AiError::NoApiKey) => "未配置 AI API Key".into(),
        TaggingError::Ai(AiError::Timeout) => "AI 服务响应超时".into(),
        TaggingError::Ai(AiError::Upstream(code, msg)) => {
            if msg.contains("免费档不可用") {
                crate::ai::gemini_limit::GEMINI_UNAVAILABLE_USER_MESSAGE.into()
            } else if is_rate_limit_message(*code, msg) {
                if msg.contains("RPD") || msg.contains("太平洋时间") {
                    crate::ai::gemini_limit::GEMINI_RPD_USER_MESSAGE.into()
                } else {
                    RATE_LIMIT_USER_MESSAGE.into()
                }
            } else if is_transient_openrouter_error(*code, msg) {
                OPENROUTER_PROVIDER_ERROR_USER_MESSAGE.into()
            } else {
                format!("上游错误 {code}: {msg}")
            }
        }
        TaggingError::Persist(e) => format!("保存打标建议失败: {e}"),
        TaggingError::Db(_) => "打标召回失败".into(),
    };
    TaskFailure { retryable, message }
}

fn is_retryable(err: &TaggingError) -> bool {
    match err {
        TaggingError::EmptyContent => false,
        TaggingError::ExtractParse(_) | TaggingError::Persist(_) | TaggingError::Db(_) => true,
        TaggingError::Ai(AiError::NoApiKey) => false,
        TaggingError::Ai(AiError::Timeout) => true,
        TaggingError::Ai(e) => e.is_rate_limited() || matches!(e, AiError::Upstream(code, _) if *code == 0 || *code >= 500),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_timeout_and_429() {
        assert!(is_retryable(&TaggingError::Ai(AiError::Timeout)));
        assert!(is_retryable(&TaggingError::Ai(AiError::Upstream(
            429,
            "rate".into()
        ))));
        assert!(is_retryable(&TaggingError::Ai(AiError::Upstream(
            503,
            "busy".into()
        ))));
        assert!(is_retryable(&TaggingError::ExtractParse("bad json".into())));
        assert!(!is_retryable(&TaggingError::Ai(AiError::NoApiKey)));
        assert!(!is_retryable(&TaggingError::Ai(AiError::Upstream(
            400,
            "bad req".into()
        ))));
        assert!(!is_retryable(&TaggingError::EmptyContent));
    }
}
