//! 确认保存：把 TaggingSuggestion 落到题目关联与候选队列

use axum::{http::StatusCode, Json};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use super::types::{TaggingSuggestion, TaggingTargetType};
use crate::util::normalize::normalize_text;

#[derive(Debug, Clone, Deserialize)]
pub struct AliasMapItem {
    pub unmatched_id: String,
    pub node_id: Option<Uuid>,
    pub tag_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AiTaggingConfirmation {
    pub suggestion_id: Uuid,
    /// 用户勾选、允许进入候选审核的 unmatched.id
    #[serde(default)]
    pub unmatched_ids: Vec<String>,
    /// 用户将未匹配指到已有节点/标签（优先于 unmatched_ids）
    #[serde(default)]
    pub alias_maps: Vec<AliasMapItem>,
}

/// 事务提交后再写入的候选（失败不回滚题目）
#[derive(Debug, Clone)]
pub struct PendingCandidate {
    pub kind: String,
    pub target_type: String,
    pub raw_name: String,
    pub normalized_name: String,
    pub confidence: rust_decimal::Decimal,
    pub source_task_id: Option<Uuid>,
    pub suggested_node_id: Option<Uuid>,
    pub suggested_tag_id: Option<Uuid>,
}

fn db_err(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    let msg = msg.into();
    tracing::error!("{msg}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "服务器内部错误，请稍后重试",
            "code": "ERR_INTERNAL_SERVER"
        })),
    )
}

/// 请求显式确认优先；旧前端未带 confirmation 时，仍应用建议到题目，
/// 但 **不** 把未匹配项整批写入审核队列（须教师勾选「提交为新」或「等于已有」）。
pub fn confirmation_or_legacy(
    explicit: Option<AiTaggingConfirmation>,
    staged: Option<&serde_json::Value>,
) -> Option<AiTaggingConfirmation> {
    if explicit.is_some() {
        return explicit;
    }
    let staged = staged?;
    let sid = staged
        .get("suggestion_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())?;
    Some(AiTaggingConfirmation {
        suggestion_id: sid,
        unmatched_ids: vec![],
        alias_maps: vec![],
    })
}

/// 将建议中用户仍保留的匹配标为 AI 来源，返回待写入候选。
///
/// 幂等：同一 suggestion 已应用到同一题目时直接成功（不再重复生成候选）。
pub async fn apply_tagging_suggestion(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    auth_id: Uuid,
    question_id: Uuid,
    confirmation: &AiTaggingConfirmation,
    final_node_ids: &[Uuid],
    final_tag_ids: &[Uuid],
) -> Result<Vec<PendingCandidate>, (StatusCode, Json<serde_json::Value>)> {
    let row: Option<(Uuid, Uuid, String, serde_json::Value, Option<Uuid>, Option<Uuid>)> =
        sqlx::query_as(
            r#"
            SELECT id, creator_id, status, result, question_id, source_task_id
            FROM ai_tagging_suggestions
            WHERE id = $1
            "#,
        )
        .bind(confirmation.suggestion_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| db_err(format!("查询打标建议失败: {e}")))?;

    let Some((sid, creator_id, status, result, applied_qid, source_task_id)) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "打标建议不存在或已过期"})),
        ));
    };

    if creator_id != auth_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "无权应用该打标建议"})),
        ));
    }

    if status == "applied" {
        if applied_qid == Some(question_id) {
            return Ok(Vec::new());
        }
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "该打标建议已应用到其他题目"})),
        ));
    }
    if status != "pending" {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": format!("打标建议不可用（status={status}）")})),
        ));
    }

    let suggestion: TaggingSuggestion = serde_json::from_value(result).map_err(|e| {
        db_err(format!("打标建议内容损坏: {e}"))
    })?;

    let node_set: std::collections::HashSet<Uuid> = final_node_ids.iter().copied().collect();
    let tag_set: std::collections::HashSet<Uuid> = final_tag_ids.iter().copied().collect();

    for m in &suggestion.matches {
        let conf = rust_decimal::Decimal::from_f32_retain(m.score)
            .map(|d| d.max(rust_decimal::Decimal::ZERO))
            .unwrap_or(rust_decimal::Decimal::ZERO);
        match m.target_type {
            TaggingTargetType::KnowledgeNode if node_set.contains(&m.target_id) => {
                sqlx::query(
                    r#"
                    UPDATE question_knowledge_nodes
                    SET source = 'ai',
                        ai_confidence = $3,
                        suggestion_id = $4
                    WHERE question_id = $1 AND node_id = $2
                    "#,
                )
                .bind(question_id)
                .bind(m.target_id)
                .bind(conf)
                .bind(sid)
                .execute(&mut **tx)
                .await
                .map_err(|e| db_err(format!("标记 AI 知识树关联失败: {e}")))?;
            }
            TaggingTargetType::Tag if tag_set.contains(&m.target_id) => {
                sqlx::query(
                    r#"
                    UPDATE question_tags_relation
                    SET source = 'ai',
                        ai_confidence = $3,
                        suggestion_id = $4
                    WHERE question_id = $1 AND tag_id = $2
                    "#,
                )
                .bind(question_id)
                .bind(m.target_id)
                .bind(conf)
                .bind(sid)
                .execute(&mut **tx)
                .await
                .map_err(|e| db_err(format!("标记 AI 标签关联失败: {e}")))?;
            }
            _ => {}
        }
    }

    let selected: std::collections::HashSet<&str> = confirmation
        .unmatched_ids
        .iter()
        .map(|s| s.as_str())
        .collect();
    let mut alias_by_id: std::collections::HashMap<&str, &AliasMapItem> =
        std::collections::HashMap::new();
    for m in &confirmation.alias_maps {
        if m.node_id.is_some() || m.tag_id.is_some() {
            alias_by_id.insert(m.unmatched_id.as_str(), m);
        }
    }

    let mut pending = Vec::new();
    let mut seen_keys: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    let push_pending = |pending: &mut Vec<PendingCandidate>,
                        seen: &mut std::collections::HashSet<(String, String)>,
                        item: PendingCandidate| {
        let key = (item.kind.clone(), item.normalized_name.clone());
        if seen.insert(key) {
            pending.push(item);
        }
    };

    for u in &suggestion.unmatched {
        let mapped = alias_by_id.get(u.id.as_str()).copied();
        if mapped.is_none() {
            if !u.eligible_for_candidate || !selected.contains(u.id.as_str()) {
                continue;
            }
        }
        let normalized = if u.normalized_name.is_empty() {
            normalize_text(&u.raw_name)
        } else {
            u.normalized_name.clone()
        };
        let confidence = u
            .confidence
            .and_then(rust_decimal::Decimal::from_f32_retain)
            .unwrap_or(rust_decimal::Decimal::ZERO);
        push_pending(
            &mut pending,
            &mut seen_keys,
            PendingCandidate {
                kind: u.dimension.as_str().to_string(),
                target_type: u.target_type.as_str().to_string(),
                raw_name: u.raw_name.clone(),
                normalized_name: normalized,
                confidence,
                source_task_id,
                suggested_node_id: mapped.and_then(|m| m.node_id),
                suggested_tag_id: mapped.and_then(|m| m.tag_id),
            },
        );
    }

    // fuzzy 别名提案不再自动进审核队列：只给本题打上已选节点，
    // 别名沉淀改由教师在「等于已有」中显式提交。

    sqlx::query(
        r#"
        UPDATE ai_tagging_suggestions
        SET status = 'applied',
            question_id = $2,
            applied_at = NOW()
        WHERE id = $1 AND status = 'pending'
        "#,
    )
    .bind(sid)
    .bind(question_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| db_err(format!("更新打标建议状态失败: {e}")))?;

    tracing::info!(
        suggestion_id = %sid,
        question_id = %question_id,
        pending_candidates = pending.len(),
        "打标建议已应用"
    );

    Ok(pending)
}

/// 题目事务提交后写入候选；单条失败只记日志。
pub async fn insert_confirmed_candidates(
    pool: &PgPool,
    question_id: Uuid,
    items: &[PendingCandidate],
) {
    for item in items {
        let name = item.raw_name.trim();
        if name.is_empty() {
            continue;
        }
        let result = sqlx::query(
            r#"
            INSERT INTO tag_candidates (
                kind, target_type, raw_name, normalized_name,
                ai_confidence, match_score, source_task_id, source_question_id,
                suggested_node_id, suggested_tag_id
            )
            VALUES ($1, $2, $3, $4, $5, 0, $6, $7, $8, $9)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(&item.kind)
        .bind(&item.target_type)
        .bind(name)
        .bind(&item.normalized_name)
        .bind(item.confidence)
        .bind(item.source_task_id)
        .bind(question_id)
        .bind(item.suggested_node_id)
        .bind(item.suggested_tag_id)
        .execute(pool)
        .await;

        if let Err(e) = result {
            tracing::warn!(
                "写入 tag_candidates 失败（不回滚题目）: question={question_id} name={name} err={e}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn explicit_confirmation_wins_even_when_empty() {
        let sid = Uuid::new_v4();
        let explicit = Some(AiTaggingConfirmation {
            suggestion_id: sid,
            unmatched_ids: vec![],
            alias_maps: vec![],
        });
        let staged = json!({
            "suggestion_id": Uuid::new_v4().to_string(),
            "suggestion": { "unmatched": [{ "id": "u1" }] }
        });
        let got = confirmation_or_legacy(explicit, Some(&staged)).unwrap();
        assert_eq!(got.suggestion_id, sid);
        assert!(got.unmatched_ids.is_empty());
    }

    #[test]
    fn legacy_does_not_dump_unmatched_into_queue() {
        let sid = Uuid::new_v4();
        let staged = json!({
            "suggestion_id": sid.to_string(),
            "suggestion": {
                "unmatched": [{ "id": "a" }, { "id": "b" }]
            }
        });
        let got = confirmation_or_legacy(None, Some(&staged)).unwrap();
        assert_eq!(got.suggestion_id, sid);
        assert!(got.unmatched_ids.is_empty());
        assert!(got.alias_maps.is_empty());
    }

    #[test]
    fn missing_suggestion_id_yields_none() {
        let staged = json!({ "unmatched": { "knowledge": ["x"] } });
        assert!(confirmation_or_legacy(None, Some(&staged)).is_none());
    }

    #[test]
    fn confirmation_deserializes_without_alias_maps() {
        let v = json!({
            "suggestion_id": Uuid::new_v4(),
            "unmatched_ids": ["u1"]
        });
        let c: AiTaggingConfirmation = serde_json::from_value(v).unwrap();
        assert_eq!(c.unmatched_ids, vec!["u1"]);
        assert!(c.alias_maps.is_empty());
    }
}
