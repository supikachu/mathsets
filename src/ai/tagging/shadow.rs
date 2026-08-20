//! 影子比对：旧 Top1（含 fuzzy）与引擎确定性匹配的差异，不额外调用 LLM。
//!
//! 由 `TAGGING_SHADOW_COMPARE=1` 开启，用于解析后处理观测，默认关闭。

use sqlx::PgPool;

use super::repository::{match_nodes, recall_nodes};
use super::types::{TaggingDimension, TaggingPolicy};

pub async fn maybe_log_knowledge_shadow(pool: &PgPool, names: &[String]) {
    if std::env::var("TAGGING_SHADOW_COMPARE").ok().as_deref() != Some("1") {
        return;
    }
    if names.iter().all(|n| n.trim().is_empty()) {
        return;
    }
    let old = match match_nodes(pool, names, None, "knowledge").await {
        Ok((m, _)) => m,
        Err(e) => {
            tracing::warn!(error = ?e.1, "tagging shadow: 旧 Top1 召回失败");
            return;
        }
    };
    let policy = TaggingPolicy {
        run_llm_extract: false,
        run_llm_converge: false,
        fail_on_persist: false,
        ..TaggingPolicy::default()
    };
    let recalled = match recall_nodes(
        pool,
        names,
        TaggingDimension::Knowledge,
        &policy,
        None,
        None,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = ?e.1, "tagging shadow: 引擎召回失败");
            return;
        }
    };
    let new_ids: Vec<String> = recalled
        .iter()
        .filter(|c| c.match_type.is_deterministic())
        .map(|c| c.id.to_string())
        .collect();
    let old_ids: Vec<String> = old.iter().map(|m| m.node_id.to_string()).collect();
    let extra_in_old: Vec<&String> = old_ids
        .iter()
        .filter(|id| !new_ids.contains(id))
        .collect();
    if extra_in_old.is_empty() && old_ids.len() == new_ids.len() {
        tracing::debug!(
            old = old_ids.len(),
            new_determined = new_ids.len(),
            "tagging shadow: 知识点 Top1 与确定性匹配一致"
        );
        return;
    }
    tracing::info!(
        engine_version = super::types::ENGINE_VERSION,
        keys = names.len(),
        old_top1 = old_ids.len(),
        new_determined = new_ids.len(),
        extra_in_old = extra_in_old.len(),
        "tagging shadow: 旧 Top1 含引擎未自动接受的模糊命中"
    );
}
