//! 打标建议持久化

use sqlx::PgPool;
use uuid::Uuid;

use super::types::{TaggingContext, TaggingSuggestion};

pub async fn persist_suggestion(
    pool: &PgPool,
    ctx: &TaggingContext,
    suggestion: &mut TaggingSuggestion,
) -> Result<Uuid, sqlx::Error> {
    let result = serde_json::to_value(&suggestion).unwrap_or_else(|_| serde_json::json!({}));

    let row: (Uuid,) = if ctx.source_task_id.is_some() && ctx.source_index.is_some() {
        sqlx::query_as(
            r#"
            INSERT INTO ai_tagging_suggestions (
                creator_id, space_id, question_id, source_task_id, source_index,
                input_hash, engine_version, status, result
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', $8)
            ON CONFLICT (source_task_id, source_index)
                WHERE source_task_id IS NOT NULL AND source_index IS NOT NULL
            DO UPDATE SET
                result = EXCLUDED.result,
                input_hash = EXCLUDED.input_hash,
                engine_version = EXCLUDED.engine_version,
                status = 'pending',
                question_id = COALESCE(EXCLUDED.question_id, ai_tagging_suggestions.question_id),
                applied_at = NULL
            RETURNING id
            "#,
        )
        .bind(ctx.user_id)
        .bind(ctx.space_id)
        .bind(ctx.question_id)
        .bind(ctx.source_task_id)
        .bind(ctx.source_index.as_deref())
        .bind(&suggestion.input_hash)
        .bind(&suggestion.engine_version)
        .bind(&result)
        .fetch_one(pool)
        .await?
    } else {
        sqlx::query_as(
            r#"
            INSERT INTO ai_tagging_suggestions (
                creator_id, space_id, question_id, source_task_id, source_index,
                input_hash, engine_version, status, result
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', $8)
            RETURNING id
            "#,
        )
        .bind(ctx.user_id)
        .bind(ctx.space_id)
        .bind(ctx.question_id)
        .bind(ctx.source_task_id)
        .bind(ctx.source_index.as_deref())
        .bind(&suggestion.input_hash)
        .bind(&suggestion.engine_version)
        .bind(&result)
        .fetch_one(pool)
        .await?
    };

    suggestion.suggestion_id = Some(row.0);
    Ok(row.0)
}
