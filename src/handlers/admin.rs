//! V2.1.1 P0-D：数据质量基础检查（计划书 §九 / 评审意见⑯）
//!
//! `GET /admin/data-quality/summary`：按需计算（不做定时任务，定时进 P2）：
//! - 孤儿关联：paper_questions / collection_questions 指向不存在的行
//! - 无题容器：无题 Paper / Collection / Document
//! - 题号重复：同一容器内 question_no 重复的分组数
//! - 无来源题目：没有任何 Paper/Collection 关联的题目数

use axum::{
    extract::{Extension, State},
    http::StatusCode,
    Json,
};
use serde_json::json;

use crate::auth::middleware::AuthUser;
use crate::auth::permissions::is_admin_user;
use crate::AppState;

/// GET /api/v1/admin/data-quality/summary — 数据一致性概览（仅管理员）
pub async fn data_quality_summary(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_admin_user(&auth) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "仅管理员可查看数据质量报告"})),
        ));
    }

    async fn count(pool: &sqlx::PgPool, sql: &str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(pool).await.unwrap_or(-1)
    }

    let orphan_paper_questions = count(
        &state.pool,
        r#"
        SELECT COUNT(*) FROM paper_questions pq
        LEFT JOIN papers p ON p.id = pq.paper_id
        LEFT JOIN questions q ON q.id = pq.question_id
        WHERE p.id IS NULL OR q.id IS NULL
        "#,
    )
    .await;

    let orphan_collection_questions = count(
        &state.pool,
        r#"
        SELECT COUNT(*) FROM collection_questions cq
        LEFT JOIN question_collections c ON c.id = cq.collection_id
        LEFT JOIN questions q ON q.id = cq.question_id
        WHERE c.id IS NULL OR q.id IS NULL
        "#,
    )
    .await;

    let papers_without_questions = count(
        &state.pool,
        r#"
        SELECT COUNT(*) FROM papers p
        WHERE NOT EXISTS (SELECT 1 FROM paper_questions pq WHERE pq.paper_id = p.id)
        "#,
    )
    .await;

    let collections_without_questions = count(
        &state.pool,
        r#"
        SELECT COUNT(*) FROM question_collections c
        WHERE NOT EXISTS (SELECT 1 FROM collection_questions cq WHERE cq.collection_id = c.id)
        "#,
    )
    .await;

    // 已确认但未产出任何容器的 Document（Worker 失败/取消场景的残留）
    let documents_without_sources = count(
        &state.pool,
        r#"
        SELECT COUNT(*) FROM documents d
        WHERE d.status IN ('confirmed', 'parsing', 'done')
          AND NOT EXISTS (SELECT 1 FROM papers p WHERE p.document_id = d.id)
          AND NOT EXISTS (SELECT 1 FROM question_collections c WHERE c.document_id = d.id)
        "#,
    )
    .await;

    // 容器内题号重复（question_no 非空且重复的分组数）
    let duplicate_paper_question_no = count(
        &state.pool,
        r#"
        SELECT COUNT(*) FROM (
            SELECT paper_id FROM paper_questions
            WHERE question_no IS NOT NULL
            GROUP BY paper_id, question_no HAVING COUNT(*) > 1
        ) t
        "#,
    )
    .await;

    let duplicate_collection_question_no = count(
        &state.pool,
        r#"
        SELECT COUNT(*) FROM (
            SELECT collection_id FROM collection_questions
            WHERE question_no IS NOT NULL
            GROUP BY collection_id, question_no HAVING COUNT(*) > 1
        ) t
        "#,
    )
    .await;

    // 无来源题目（历史遗留）
    let questions_without_sources = count(
        &state.pool,
        r#"
        SELECT COUNT(*) FROM questions q
        WHERE NOT EXISTS (SELECT 1 FROM paper_questions pq WHERE pq.question_id = q.id)
          AND NOT EXISTS (SELECT 1 FROM collection_questions cq WHERE cq.question_id = q.id)
        "#,
    )
    .await;

    // ── V2.1.1 P1 标签治理检查 ──
    // merged 但无 canonical_id（数据一致性要求 4/5：merged 必须指向最终标签）
    let merged_without_canonical = count(
        &state.pool,
        "SELECT COUNT(*) FROM knowledge_nodes WHERE status = 'merged' AND canonical_id IS NULL",
    )
    .await;

    // canonical 链异常：链长 > 1（A→B→C 过度合并）或环（depth 截断防无限递归）
    let canonical_chain_issues = count(
        &state.pool,
        r#"
        WITH RECURSIVE chain AS (
            SELECT id, canonical_id, 1 AS depth FROM knowledge_nodes WHERE canonical_id IS NOT NULL
            UNION ALL
            SELECT kn.id, kn.canonical_id, c.depth + 1
            FROM knowledge_nodes kn JOIN chain c ON kn.id = c.canonical_id
            WHERE c.depth < 20
        )
        SELECT COUNT(*) FROM chain WHERE depth > 1
        "#,
    )
    .await;

    // 长期未审核候选（>7 天）
    let pending_candidates_aging = count(
        &state.pool,
        r#"
        SELECT COUNT(*) FROM tag_candidates
        WHERE status = 'pending' AND created_at < NOW() - INTERVAL '7 days'
        "#,
    )
    .await;

    // 待审核候选总数
    let pending_candidates = count(
        &state.pool,
        "SELECT COUNT(*) FROM tag_candidates WHERE status = 'pending'",
    )
    .await;

    Ok(Json(json!({
        "orphan_paper_questions": orphan_paper_questions,
        "orphan_collection_questions": orphan_collection_questions,
        "papers_without_questions": papers_without_questions,
        "collections_without_questions": collections_without_questions,
        "documents_without_sources": documents_without_sources,
        "duplicate_paper_question_no_groups": duplicate_paper_question_no,
        "duplicate_collection_question_no_groups": duplicate_collection_question_no,
        "questions_without_sources": questions_without_sources,
        "merged_without_canonical": merged_without_canonical,
        "canonical_chain_issues": canonical_chain_issues,
        "pending_candidates": pending_candidates,
        "pending_candidates_aging": pending_candidates_aging,
        "generated_at": chrono::Utc::now(),
    })))
}
