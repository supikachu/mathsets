use std::time::Duration;
use uuid::Uuid;

use crate::ai::cleaner::clean_and_parse;
use crate::ai::provider::{create_provider, AiError};
use crate::ai::types::ParsedQuestion;
use crate::auth::middleware::AuthUser;
use crate::auth::permissions::ensure_personal_space;
use crate::handlers::ai::{resolve_ai_config, ModelKind};
use crate::handlers::questions::save_version;
use crate::models::ai_task::AiParseTask;
use crate::models::question::{Difficulty, QuestionStatus, QuestionType};
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
        RETURNING id, creator_id, raw_text, status, question_id, error_message, created_at, updated_at
        "#,
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| format!("拾取任务失败: {e}"))?;

    let Some(task) = task else {
        return Ok(false);
    };

    let task_id = task.id;
    tracing::info!("Worker 拾取任务 {task_id}（creator={}）", task.creator_id);

    // 2. 执行任务 — 任何错误都捕获并标记为 failed
    match execute_task(state, &task).await {
        Ok(question_id) => {
            // 标记为 completed
            if let Err(e) = sqlx::query(
                r#"
                UPDATE ai_parse_tasks
                SET status = 'completed', question_id = $1, updated_at = NOW()
                WHERE id = $2
                "#,
            )
            .bind(question_id)
            .bind(task_id)
            .execute(&state.pool)
            .await
            {
                tracing::error!("任务 {task_id} 标记 completed 失败: {e}（题目已生成: {question_id}）");
            } else {
                tracing::info!("✅ 任务 {task_id} 完成，生成题目 {question_id}");
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
                SET status = 'failed', error_message = $1, updated_at = NOW()
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
// 任务执行核心
// ---------------------------------------------------------------------------

/// 执行单个解析任务：调用 LLM → 清洗 JSON → 落库为新题目（草稿）
///
/// 成功返回新题目的 ID；失败返回错误信息字符串。
async fn execute_task(state: &AppState, task: &AiParseTask) -> Result<Uuid, String> {
    // 1. 加载 creator 信息（resolve_ai_config 需要 AuthUser）
    let user_row: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT username, role, display_name FROM users WHERE id = $1",
    )
    .bind(task.creator_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| format!("查询用户失败: {e}"))?;

    let Some((username, role, display_name)) = user_row else {
        return Err(format!("用户 {} 不存在", task.creator_id));
    };

    let auth = AuthUser {
        id: task.creator_id,
        username,
        role,
    };

    // 2. 解析 AI 配置（用户个人 Key 优先，否则平台默认）
    let (api_key, provider_name, model, base_url) =
        resolve_ai_config(&auth, state, ModelKind::Text).await?;

    // 3. 调用 LLM
    let provider = create_provider(&provider_name, &api_key, &base_url);
    let raw_json = provider
        .parse_text(&task.raw_text, model.as_deref())
        .await
        .map_err(map_ai_error)?;

    // 4. 清洗 & 反序列化
    let parsed: ParsedQuestion = clean_and_parse(&raw_json)
        .map_err(|e| format!("AI 返回 JSON 解析失败: {e}"))?;

    // 5. 转换为题目字段
    let question_type = match parsed.question_type.as_str() {
        "choice" => QuestionType::Choice,
        "fill" => QuestionType::Fill,
        "solution" => QuestionType::Solution,
        other => return Err(format!("未知题型: {other}")),
    };

    let difficulty = match parsed.difficulty.as_deref() {
        Some("easy") => Difficulty::Easy,
        Some("hard") => Difficulty::Hard,
        _ => Difficulty::Medium,
    };

    let options_json = parsed
        .options
        .as_ref()
        .map(|opts| serde_json::to_value(opts).unwrap_or(serde_json::Value::Null));
    let correct_answer_json = serde_json::to_value(&parsed.correct_answer)
        .map_err(|e| format!("序列化 correct_answer 失败: {e}"))?;

    // analysis: 按项目约定用 \n\n---\n\n 拼接多解法（前端反向 split）
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

    // 6. 确保个人空间存在（落题用）
    let space_id = ensure_personal_space(
        &state.pool,
        task.creator_id,
        display_name.as_deref().unwrap_or("用户"),
    )
    .await
    .map_err(|e| format!("创建个人空间失败: {e}"))?;

    // 7. 事务：插入题目 + 生成版本快照
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
        INSERT INTO questions (id, stem, question_type, difficulty, default_score, status,
            options, correct_answer, analysis, grading_criteria, grade, semester, source,
            academic_year, grade_semester, exam_type, exam_region,
            grade_level, semester_new, cognitive_level, difficulty_score, estimated_minutes,
            images, parent_id, sub_order,
            creator_id, created_at, updated_at, version, space_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
            $14, $15, $16, $17,
            $18, $19, $20, $21, $22,
            $23, $24, $25,
            $26, $27, $28, $29, $30)
        "#,
    )
    .bind(id)
    .bind(&parsed.stem)
    .bind(question_type)
    .bind(difficulty)
    .bind(5)
    .bind(QuestionStatus::Draft)
    .bind(&options_json)
    .bind(&correct_answer_json)
    .bind(&analysis_str)
    .bind(None::<serde_json::Value>)
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(None::<crate::models::question::GradeLevel>)
    .bind(None::<crate::models::question::SemesterType>)
    .bind(None::<crate::models::question::CognitiveLevel>)
    .bind(None::<i16>)
    .bind(None::<i16>)
    .bind(None::<serde_json::Value>)
    .bind(None::<Uuid>)
    .bind(None::<i16>)
    .bind(task.creator_id)
    .bind(now)
    .bind(now)
    .bind(version)
    .bind(space_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("插入题目失败: {e}"))?;

    save_version(&mut tx, id, version, Some(task.creator_id))
        .await
        .map_err(|e| format!("保存版本快照失败: {e}"))?;

    tx.commit().await.map_err(|e| format!("提交事务失败: {e}"))?;

    Ok(id)
}

// ---------------------------------------------------------------------------
// 错误映射
// ---------------------------------------------------------------------------

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
