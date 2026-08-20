//! 异步智能打标 Worker
//!
//! 拾取 `ai_tagging_tasks` → 调用统一 TaggingEngine → 回写 suggestion_id。
//! 不写入 `questions`，候选仍只在用户确认保存后由 Finalizer 产生。

use std::time::Duration;
use uuid::Uuid;

use crate::ai::provider::{create_provider, AiError};
use crate::ai::tagging::engine::TaggingError;
use crate::ai::tagging::{run_tagging, TaggingContext, TaggingInput, TaggingPolicy};
use crate::auth::middleware::AuthUser;
use crate::handlers::ai::{resolve_ai_config, ModelKind};
use crate::models::ai_tagging_task::{AiTaggingTask, TAGGING_TASK_COLUMNS};
use crate::AppState;

const HEARTBEAT_TIMEOUT: &str = "120 seconds";
const MAX_RETRIES: i32 = 2;

struct TaskFailure {
    retryable: bool,
    message: String,
}

pub async fn start_worker(state: AppState) {
    let worker_id = format!("tag-worker-{}", Uuid::new_v4().simple());
    tracing::info!("🏷️ AI 打标 worker 已启动（{worker_id}），每 2s 轮询一次任务");

    loop {
        recover_stale_tasks(&state, &worker_id).await;
        match process_one_task(&state, &worker_id).await {
            Ok(true) => {}
            Ok(false) => {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Err(e) => {
                tracing::error!("打标 Worker 循环异常: {e}，5 秒后重试");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

async fn recover_stale_tasks(state: &AppState, worker_id: &str) {
    let requeued = sqlx::query(&format!(
        r#"
        UPDATE ai_tagging_tasks
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
        UPDATE ai_tagging_tasks
        SET status = 'failed', error_message = '任务处理超时（租约过期）',
            completed_at = NOW(), updated_at = NOW(),
            locked_at = NULL, worker_id = NULL
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
        tracing::warn!("打标 Worker {worker_id} 恢复僵尸任务：requeue={requeued} failed={failed}");
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
    let outcome = execute_task(state, &task).await;
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
            }
        }
    }

    Ok(true)
}

fn spawn_heartbeat(pool: sqlx::PgPool, task_id: Uuid) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(20)).await;
            let _ = sqlx::query(
                "UPDATE ai_tagging_tasks SET heartbeat_at = NOW() WHERE id = $1 AND status = 'processing'",
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
        resolve_ai_config(&auth, state, ModelKind::Text)
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
        source_task_id: None,
        source_index: None,
        stage: task.stage.clone(),
    };

    let suggestion = run_tagging(
        &state.pool,
        Some(provider.as_ref()),
        model.as_deref(),
        TaggingInput::Content {
            content: task.content.clone(),
        },
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
        TaggingError::Ai(AiError::Upstream(code, msg)) => format!("上游错误 {code}: {msg}"),
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
        TaggingError::Ai(AiError::Upstream(code, _)) => *code == 0 || *code == 429 || *code >= 500,
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
