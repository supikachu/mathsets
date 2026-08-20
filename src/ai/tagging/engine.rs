//! 统一 TaggingEngine：提取 → 召回 → 确定性匹配 / 模糊收敛 → 建议

use sha2::{Digest, Sha256};
use std::time::Instant;
use uuid::Uuid;

use crate::ai::cleaner::clean_and_parse;
use crate::ai::provider::AiProvider;
use crate::ai::types::{ParsedAnswer, ParsedQuestion};
use crate::util::normalize::normalize_text;

use super::persist::persist_suggestion;
use super::prompts::{AI_CONVERGE_PROMPT, AI_EXTRACT_KEYS_PROMPT};
use super::repository::{
    match_tags, recall_nodes_with_stats, rerank_by_topic, filter_offtopic_candidates,
    is_generic_token, is_overly_broad_chapter_key, tagging_match_from_tag, NodeCandidate,
};
use super::types::{
    TaggingAliasProposal, TaggingContext, TaggingDimension, TaggingInput, TaggingPolicy,
    TaggingSignals, TaggingSuggestion, TaggingUnmatched, ENGINE_VERSION,
};

#[derive(Debug)]
pub enum TaggingError {
    EmptyContent,
    ExtractParse(String),
    Ai(crate::ai::provider::AiError),
    Db((axum::http::StatusCode, axum::Json<serde_json::Value>)),
    Persist(sqlx::Error),
}

#[derive(Debug, serde::Deserialize)]
struct AiConvergePick {
    #[serde(default)]
    key: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct AiConvergeResult {
    #[serde(default)]
    chapter: Vec<AiConvergePick>,
    #[serde(default)]
    knowledge: Vec<AiConvergePick>,
    #[serde(default)]
    pattern: Vec<AiConvergePick>,
    #[serde(default)]
    chapter_names: Vec<String>,
    #[serde(default)]
    knowledge_names: Vec<String>,
    #[serde(default)]
    pattern_names: Vec<String>,
}

const NODE_DIMS: [TaggingDimension; 3] = [
    TaggingDimension::Chapter,
    TaggingDimension::Knowledge,
    TaggingDimension::Pattern,
];

pub async fn run_tagging(
    pool: &sqlx::PgPool,
    provider: Option<&dyn AiProvider>,
    model: Option<&str>,
    input: TaggingInput,
    ctx: &TaggingContext,
    policy: &TaggingPolicy,
) -> Result<TaggingSuggestion, TaggingError> {
    let content_for_hash = match &input {
        TaggingInput::Content { content } => {
            if content.trim().is_empty() {
                return Err(TaggingError::EmptyContent);
            }
            content.clone()
        }
        TaggingInput::Parsed(q) => format!(
            "{}|{}|{}",
            q.stem,
            q.knowledge_points.join(","),
            q.chapter_path.join("/")
        ),
    };
    let input_hash = sha256_hex(&content_for_hash);
    let input_kind = match &input {
        TaggingInput::Content { .. } => "content",
        TaggingInput::Parsed(_) => "parsed",
    };

    let extract_started = Instant::now();
    let signals = match &input {
        TaggingInput::Content { content } => {
            if !policy.run_llm_extract {
                TaggingSignals::default()
            } else {
                let Some(provider) = provider else {
                    return Err(TaggingError::ExtractParse("未配置文本模型".into()));
                };
                extract_signals(provider, content, model).await?
            }
        }
        TaggingInput::Parsed(q) => signals_from_parsed(q),
    };
    let mut signals = signals;
    cap_extract_keys(&mut signals);
    let extract_ms = extract_started.elapsed().as_millis() as u64;

    let question_content = match &input {
        TaggingInput::Content { content } => content.clone(),
        TaggingInput::Parsed(q) => q.stem.clone(),
    };

    let mut matches = Vec::new();
    let mut unmatched = Vec::new();
    let mut alias_proposals = Vec::new();
    let mut needs_review = false;
    let mut fuzzy_by_dim: Vec<(TaggingDimension, Vec<NodeCandidate>)> = Vec::new();
    let mut recalled = [0usize; 3];
    let mut topic_hints: Vec<String> = signals.chapter_keys.clone();
    let mut vector_recalled = 0usize;
    let mut vector_ms = 0u64;

    let recall_started = Instant::now();
    for (idx, dim) in NODE_DIMS.iter().enumerate() {
        let mut keys = signals.keys(*dim).to_vec();
        // 章节：有更具体关键词时丢掉「函数」等过宽词，避免误召三角/导数等章节
        if *dim == TaggingDimension::Chapter {
            let specific: Vec<String> = keys
                .iter()
                .filter(|k| !is_overly_broad_chapter_key(k))
                .cloned()
                .collect();
            if !specific.is_empty() {
                keys = specific;
            }
        }
        let (mut candidates, vstats) = recall_nodes_with_stats(
            pool,
            &keys,
            *dim,
            policy,
            ctx.space_id,
            ctx.stage.as_deref(),
        )
            .await
            .map_err(TaggingError::Db)?;
        vector_recalled += vstats.hits;
        vector_ms += vstats.elapsed_ms;
        candidates = filter_offtopic_candidates(candidates, &question_content, &keys);
        // 各维均按章节主题词重排，压低跨主题噪声
        candidates = rerank_by_topic(candidates, &topic_hints);
        candidates.truncate(policy.recall_limit(*dim));
        recalled[idx] = candidates.len();

        let mut determined = Vec::new();
        let mut fuzzy = Vec::new();
        for c in candidates {
            if c.match_type.is_deterministic() {
                determined.push(c);
            } else {
                fuzzy.push(c);
            }
        }

        let mut accepted_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
        determined.truncate(policy.max_selected(*dim));
        for c in &determined {
            for k in &c.deterministic_keys {
                accepted_keys.insert(k.clone());
            }
            accepted_keys.insert(c.name.clone());
            matches.push(c.to_tagging_match(*dim));
            if *dim == TaggingDimension::Chapter {
                topic_hints.push(c.name.clone());
            }
        }

        let pending_keys: Vec<String> = keys
            .iter()
            .filter(|k| {
                let t = k.trim();
                !t.is_empty() && !accepted_keys.contains(*k) && !accepted_keys.contains(t)
            })
            .cloned()
            .collect();

        if !pending_keys.is_empty() && !fuzzy.is_empty() && policy.run_llm_converge {
            fuzzy_by_dim.push((*dim, fuzzy));
            unmatched.extend(pending_keys.into_iter().map(|raw| pending_unmatched(*dim, raw)));
        } else {
            if !pending_keys.is_empty() && !fuzzy.is_empty() {
                needs_review = true;
            }
            unmatched.extend(pending_keys.into_iter().map(|raw| pending_unmatched(*dim, raw)));
        }
    }
    let recall_ms = recall_started.elapsed().as_millis() as u64;

    let converge_started = Instant::now();
    if !fuzzy_by_dim.is_empty() && policy.run_llm_converge {
        if let Some(provider) = provider {
            let menu = build_candidate_menu(&fuzzy_by_dim);
            let payload = format!(
                "{}\n【题目内容】\n{}\n\n{}",
                build_query_keys_block(&signals),
                question_content,
                menu
            );
            match provider
                .parse_text_with_prompt(&payload, AI_CONVERGE_PROMPT, model)
                .await
            {
                Ok(raw) => match clean_and_parse::<AiConvergeResult>(&raw) {
                    Ok(converge) => {
                        apply_converge(
                            &mut matches,
                            &mut unmatched,
                            &mut alias_proposals,
                            &fuzzy_by_dim,
                            &converge,
                            policy,
                        );
                    }
                    Err(e) => {
                        tracing::warn!("AI 打标-阶段三解析失败，保留未匹配待确认: {e}");
                        needs_review = true;
                    }
                },
                Err(e) => {
                    tracing::warn!("AI 打标-阶段三调用失败，保留未匹配待确认: {:?}", e);
                    needs_review = true;
                }
            }
        } else {
            needs_review = true;
        }
    }
    let converge_ms = converge_started.elapsed().as_millis() as u64;

    // 扁平标签：exact/alias/fuzzy Top1，不走第二次 LLM
    let (comp_matched, comp_unmatched) = match_tags(
        pool,
        &signals.core_competencies,
        "core_competence",
        ctx.space_id,
    )
    .await
    .map_err(TaggingError::Db)?;
    for m in comp_matched.into_iter().take(policy.max_competence) {
        matches.push(tagging_match_from_tag(&m, TaggingDimension::CoreCompetence));
    }
    unmatched.extend(
        comp_unmatched
            .into_iter()
            .map(|raw| pending_unmatched(TaggingDimension::CoreCompetence, raw)),
    );

    let (method_matched, method_unmatched) =
        match_tags(pool, &signals.method_keys, "method", ctx.space_id)
            .await
            .map_err(TaggingError::Db)?;
    for m in method_matched.into_iter().take(policy.max_method) {
        matches.push(tagging_match_from_tag(&m, TaggingDimension::Method));
    }
    unmatched.extend(
        method_unmatched
            .into_iter()
            .map(|raw| pending_unmatched(TaggingDimension::Method, raw)),
    );

    refine_unmatched(&mut unmatched, &matches);

    if !unmatched.is_empty() {
        needs_review = true;
    }

    let chapter_matched = matches
        .iter()
        .filter(|m| m.dimension == TaggingDimension::Chapter)
        .count();
    let knowledge_matched = matches
        .iter()
        .filter(|m| m.dimension == TaggingDimension::Knowledge)
        .count();
    let pattern_matched = matches
        .iter()
        .filter(|m| m.dimension == TaggingDimension::Pattern)
        .count();
    let method_matched = matches
        .iter()
        .filter(|m| m.dimension == TaggingDimension::Method)
        .count();
    let competence_matched = matches
        .iter()
        .filter(|m| m.dimension == TaggingDimension::CoreCompetence)
        .count();
    let chapter_unmatched = unmatched
        .iter()
        .filter(|u| u.dimension == TaggingDimension::Chapter)
        .count();
    let knowledge_unmatched = unmatched
        .iter()
        .filter(|u| u.dimension == TaggingDimension::Knowledge)
        .count();
    let pattern_unmatched = unmatched
        .iter()
        .filter(|u| u.dimension == TaggingDimension::Pattern)
        .count();
    let chapter_recalled = recalled[0];
    let knowledge_recalled = recalled[1];
    let pattern_recalled = recalled[2];

    let mut suggestion = TaggingSuggestion {
        suggestion_id: None,
        engine_version: ENGINE_VERSION.to_string(),
        input_hash,
        needs_review,
        matches,
        unmatched,
        alias_proposals,
        difficulty: signals.difficulty,
        question_type: signals.question_type,
        grade_level: signals.grade_level,
        cognitive_level: signals.cognitive_level,
    };

    let persist_started = Instant::now();
    if !ctx.user_id.is_nil() {
        if let Err(e) = persist_suggestion(pool, ctx, &mut suggestion).await {
            if policy.fail_on_persist {
                return Err(TaggingError::Persist(e));
            }
            tracing::warn!("写入打标建议失败（忽略，不影响主流程）: {e}");
        }
    }
    let persist_ms = persist_started.elapsed().as_millis() as u64;

    tracing::info!(
        engine_version = ENGINE_VERSION,
        input_kind,
        extract_ms,
        recall_ms,
        vector_recalled,
        vector_ms,
        converge_ms,
        persist_ms,
        chapter_recalled,
        chapter_matched,
        chapter_unmatched,
        knowledge_recalled,
        knowledge_matched,
        knowledge_unmatched,
        pattern_recalled,
        pattern_matched,
        pattern_unmatched,
        method_matched,
        competence_matched,
        unmatched_total = suggestion.unmatched.len(),
        needs_review = suggestion.needs_review,
        suggestion_id = ?suggestion.suggestion_id,
        "TaggingEngine 完成"
    );

    Ok(suggestion)
}

async fn extract_signals(
    provider: &dyn AiProvider,
    content: &str,
    model: Option<&str>,
) -> Result<TaggingSignals, TaggingError> {
    let raw = provider
        .parse_text_with_prompt(content, AI_EXTRACT_KEYS_PROMPT, model)
        .await
        .map_err(TaggingError::Ai)?;
    clean_and_parse(&raw).map_err(|e| {
        tracing::warn!("AI 打标-阶段一解析失败: {e}");
        TaggingError::ExtractParse(e)
    })
}

/// 与编辑页 `buildTaggingContent` 对齐：题干 + 选项 + 答案 + 解析。
pub fn tagging_content_from_parsed(q: &ParsedQuestion) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !q.stem.trim().is_empty() {
        parts.push(q.stem.clone());
    }
    if let Some(opts) = &q.options {
        let line = opts
            .iter()
            .map(|o| format!("{}. {}", o.label, o.content))
            .collect::<Vec<_>>()
            .join("\n");
        if !line.trim().is_empty() {
            parts.push(line);
        }
    }
    if let Some(ans) = answer_text_from_parsed(q) {
        parts.push(format!("参考答案：{ans}"));
    }
    let analysis: Vec<&str> = q
        .analysis
        .iter()
        .map(|a| a.content.as_str())
        .filter(|s| !s.trim().is_empty())
        .collect();
    if !analysis.is_empty() {
        parts.push(format!("解析：{}", analysis.join("\n")));
    }
    parts.join("\n\n")
}

fn answer_text_from_parsed(q: &ParsedQuestion) -> Option<String> {
    match q.correct_answer.as_ref()? {
        ParsedAnswer::Choice { options } => {
            let t = options
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("；");
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        }
        ParsedAnswer::Fill { blanks } => {
            let t = blanks
                .iter()
                .map(|b| b.answer.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("；");
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        }
        ParsedAnswer::Solution { subs } => {
            let t = subs
                .iter()
                .map(|s| s.content.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("；");
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        }
    }
}

pub fn signals_from_parsed(q: &ParsedQuestion) -> TaggingSignals {
    let difficulty = q.difficulty.as_deref().map(|d| match d {
        "easy" => 2,
        "hard" => 4,
        _ => 3,
    });
    TaggingSignals {
        chapter_keys: q
            .chapter_path
            .last()
            .cloned()
            .map(|leaf| vec![leaf])
            .unwrap_or_default(),
        knowledge_keys: q.knowledge_points.clone(),
        pattern_keys: vec![],
        method_keys: q
            .solution_methods
            .iter()
            .map(|m| m.name.clone())
            .filter(|n| !n.trim().is_empty())
            .collect(),
        core_competencies: vec![],
        difficulty,
        question_type: Some(q.question_type.clone()),
        grade_level: None,
        cognitive_level: None,
    }
}

fn cap_extract_keys(signals: &mut TaggingSignals) {
    signals.chapter_keys.truncate(2);
    signals.knowledge_keys.truncate(3);
    signals.pattern_keys.truncate(2);
    signals.method_keys.truncate(2);
    signals.core_competencies.truncate(2);
}

fn is_eligible_new_label(raw: &str, dim: TaggingDimension) -> bool {
    let t = raw.trim();
    let n = t.chars().count();
    if n < 3 {
        return false;
    }
    if is_generic_token(t) {
        return false;
    }
    if dim == TaggingDimension::Chapter && is_overly_broad_chapter_key(t) {
        return false;
    }
    true
}

fn pending_unmatched(dim: TaggingDimension, raw: String) -> TaggingUnmatched {
    let raw_name = raw.trim().to_string();
    let eligible = is_eligible_new_label(&raw_name, dim);
    TaggingUnmatched {
        id: Uuid::new_v4().to_string(),
        dimension: dim,
        target_type: dim.target_type(),
        normalized_name: normalize_text(&raw_name),
        raw_name,
        confidence: None,
        reason: if eligible {
            "no_deterministic_match".into()
        } else {
            "low_value_key".into()
        },
        eligible_for_candidate: eligible,
    }
}

/// 去掉已命中项，限制每维最多 2 条未匹配（展示用）；短词/泛化词不可「提交为新」。
fn refine_unmatched(unmatched: &mut Vec<TaggingUnmatched>, matches: &[super::types::TaggingMatch]) {
    let hit: std::collections::HashSet<String> = matches
        .iter()
        .flat_map(|m| {
            [
                normalize_text(&m.target_name),
                normalize_text(&m.ai_name),
            ]
        })
        .collect();
    unmatched.retain(|u| {
        let n = if u.normalized_name.is_empty() {
            normalize_text(&u.raw_name)
        } else {
            u.normalized_name.clone()
        };
        !hit.contains(&n)
    });
    let mut seen_norm: std::collections::HashSet<String> = std::collections::HashSet::new();
    unmatched.retain(|u| {
        let n = if u.normalized_name.is_empty() {
            normalize_text(&u.raw_name)
        } else {
            u.normalized_name.clone()
        };
        seen_norm.insert(n)
    });
    let mut per_dim: std::collections::HashMap<TaggingDimension, usize> =
        std::collections::HashMap::new();
    unmatched.retain(|u| {
        let c = per_dim.entry(u.dimension).or_insert(0);
        *c += 1;
        *c <= 2
    });
}

fn build_query_keys_block(signals: &TaggingSignals) -> String {
    fn join_keys(keys: &[String]) -> String {
        let items: Vec<&str> = keys
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if items.is_empty() {
            "（无）".into()
        } else {
            items.join("、")
        }
    }
    format!(
        "【待对齐关键词】（按语义对齐到候选原名；输出必须与候选原名逐字一致）\n章节：{}\n知识点：{}\n题型专题：{}\n",
        join_keys(&signals.chapter_keys),
        join_keys(&signals.knowledge_keys),
        join_keys(&signals.pattern_keys)
    )
}

fn build_candidate_menu(fuzzy_by_dim: &[(TaggingDimension, Vec<NodeCandidate>)]) -> String {
    fn section(title: &str, candidates: &[NodeCandidate]) -> String {
        if candidates.is_empty() {
            return format!("【{}候选列表】（无候选）\n", title);
        }
        let lines: Vec<String> = candidates
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let path = if c.name_path.is_empty() {
                    c.name.as_str()
                } else {
                    c.name_path.as_str()
                };
                format!(
                    "{}. {}（路径：{}；查询词：{}；相关度 {:.2}）",
                    i + 1,
                    c.name,
                    path,
                    c.source_keys.join("、"),
                    c.score
                )
            })
            .collect();
        format!("【{}候选列表】\n{}\n", title, lines.join("\n"))
    }

    let mut chapters = vec![];
    let mut knowledges = vec![];
    let mut patterns = vec![];
    for (dim, cs) in fuzzy_by_dim {
        match dim {
            TaggingDimension::Chapter => chapters = cs.clone(),
            TaggingDimension::Knowledge => knowledges = cs.clone(),
            TaggingDimension::Pattern => patterns = cs.clone(),
            _ => {}
        }
    }
    format!(
        "{}{}{}",
        section("章节", &chapters),
        section("知识点", &knowledges),
        section("题型专题", &patterns)
    )
}

fn apply_converge(
    matches: &mut Vec<super::types::TaggingMatch>,
    unmatched: &mut Vec<TaggingUnmatched>,
    alias_proposals: &mut Vec<TaggingAliasProposal>,
    fuzzy_by_dim: &[(TaggingDimension, Vec<NodeCandidate>)],
    converge: &AiConvergeResult,
    policy: &TaggingPolicy,
) {
    for (dim, candidates) in fuzzy_by_dim {
        let picks: &[AiConvergePick] = match dim {
            TaggingDimension::Chapter => &converge.chapter,
            TaggingDimension::Knowledge => &converge.knowledge,
            TaggingDimension::Pattern => &converge.pattern,
            _ => &[],
        };
        let legacy_names: &[String] = match dim {
            TaggingDimension::Chapter => &converge.chapter_names,
            TaggingDimension::Knowledge => &converge.knowledge_names,
            TaggingDimension::Pattern => &converge.pattern_names,
            _ => &[],
        };

        let mut accepted_keys = std::collections::HashSet::new();
        let mut accepted_ids = std::collections::HashSet::new();
        let max_n = policy.max_selected(*dim);

        let mut apply_candidate = |c: &NodeCandidate, mapped_key: &str| {
            if accepted_ids.contains(&c.id) {
                for k in &c.source_keys {
                    accepted_keys.insert(k.clone());
                }
                if !mapped_key.is_empty() {
                    accepted_keys.insert(mapped_key.to_string());
                }
                accepted_keys.insert(c.name.clone());
                return;
            }
            if accepted_ids.len() >= max_n {
                return;
            }
            accepted_ids.insert(c.id);
            for k in &c.source_keys {
                accepted_keys.insert(k.clone());
            }
            if !mapped_key.is_empty() {
                accepted_keys.insert(mapped_key.to_string());
            }
            accepted_keys.insert(c.name.clone());
            let key_for_match = if mapped_key.is_empty() {
                c.primary_key()
            } else {
                mapped_key
            };
            matches.push(c.to_tagging_match_for_key(*dim, key_for_match));
            if c.match_type == super::types::TaggingMatchType::Fuzzy {
                let raw = if mapped_key.is_empty() {
                    c.primary_key().to_string()
                } else {
                    mapped_key.to_string()
                };
                if raw != c.name {
                    alias_proposals.push(TaggingAliasProposal {
                        dimension: *dim,
                        raw_name: raw.clone(),
                        normalized_name: normalize_text(&raw),
                        target_id: c.id,
                        target_type: super::types::TaggingTargetType::KnowledgeNode,
                        score: c.score,
                    });
                }
            }
        };

        for pick in picks {
            let name = pick
                .name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let Some(name) = name else {
                continue;
            };
            if let Some(c) = candidates.iter().find(|c| c.name == name) {
                apply_candidate(c, pick.key.trim());
            } else {
                tracing::debug!("收敛选择「{}」不在候选列表，视为幻觉丢弃", name);
            }
        }

        if picks.is_empty() && !legacy_names.is_empty() {
            for c in resolve_selection(candidates, legacy_names)
                .into_iter()
                .take(max_n)
            {
                apply_candidate(c, "");
            }
        }

        unmatched.retain(|u| {
            if u.dimension != *dim {
                return true;
            }
            !accepted_keys.contains(&u.raw_name)
        });
    }
}

fn resolve_selection<'a>(
    candidates: &'a [NodeCandidate],
    names: &[String],
) -> Vec<&'a NodeCandidate> {
    let mut out = Vec::new();
    for name in names {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        if let Some(c) = candidates.iter().find(|c| c.name == name) {
            if !out.iter().any(|x: &&NodeCandidate| x.id == c.id) {
                out.push(c);
            }
        } else {
            tracing::debug!("收敛选择「{}」不在候选列表，视为幻觉丢弃", name);
        }
    }
    out
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// 与 `TaggingInput::Content` 使用同一哈希，供异步打标任务幂等复用。
/// 学段参与哈希，避免高中/初中同题文复用到错误学段的结果。
pub fn content_input_hash(content: &str) -> String {
    sha256_hex(content)
}

pub fn content_input_hash_with_stage(content: &str, stage: Option<&str>) -> String {
    let versioned = format!("{content}\nengine:{ENGINE_VERSION}");
    match stage.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => sha256_hex(&format!("{versioned}\nstage:{s}")),
        None => sha256_hex(&versioned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::TaggingMatchType;

    fn fake_candidate(name: &str, score: f32) -> NodeCandidate {
        NodeCandidate {
            id: Uuid::new_v4(),
            name: name.to_string(),
            tree_id: Uuid::new_v4(),
            path: "a.b".into(),
            depth: 1,
            score,
            match_type: TaggingMatchType::Fuzzy,
            source_keys: vec![name.to_string()],
            deterministic_keys: vec![],
            name_path: name.to_string(),
        }
    }

    fn fake_candidate_keys(name: &str, keys: &[&str], score: f32) -> NodeCandidate {
        let mut c = fake_candidate(name, score);
        c.source_keys = keys.iter().map(|s| (*s).to_string()).collect();
        c
    }

    #[test]
    fn resolve_selection_drops_hallucination() {
        let chapters = vec![fake_candidate("函数", 0.85), fake_candidate("集合", 0.72)];
        let picked =
            resolve_selection(&chapters, &["函数".to_string(), "不存在的知识点".to_string()]);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].name, "函数");
        assert!(resolve_selection(&chapters, &[]).is_empty());
        let picked2 = resolve_selection(&chapters, &["函数".to_string(), " 函数 ".to_string()]);
        assert_eq!(picked2.len(), 1);
    }

    #[test]
    fn content_hash_includes_engine_version() {
        let a = content_input_hash_with_stage("题干", Some("senior"));
        let b = content_input_hash_with_stage("题干", Some("junior"));
        assert_ne!(a, b);
        assert_ne!(a, content_input_hash("题干"));
        assert!(ENGINE_VERSION.contains("v4"));
    }

    #[test]
    fn apply_converge_drops_hallucination_and_respects_limit() {
        let c1 = fake_candidate("函数", 0.8);
        let c2 = fake_candidate("集合", 0.7);
        let c3 = fake_candidate("导数", 0.6);
        let fuzzy = vec![(
            TaggingDimension::Chapter,
            vec![c1.clone(), c2.clone(), c3.clone()],
        )];
        let mut matches = Vec::new();
        let mut unmatched = vec![
            pending_unmatched(TaggingDimension::Chapter, "函数".into()),
            pending_unmatched(TaggingDimension::Chapter, "集合".into()),
            pending_unmatched(TaggingDimension::Chapter, "导数".into()),
        ];
        let mut proposals = Vec::new();
        let converge = AiConvergeResult {
            chapter: vec![
                AiConvergePick {
                    key: "函数".into(),
                    name: Some("函数".into()),
                },
                AiConvergePick {
                    key: "不存在".into(),
                    name: Some("不存在".into()),
                },
                AiConvergePick {
                    key: "集合".into(),
                    name: Some("集合".into()),
                },
                AiConvergePick {
                    key: "导数".into(),
                    name: Some("导数".into()),
                },
            ],
            ..AiConvergeResult::default()
        };
        let mut policy = TaggingPolicy::default();
        policy.max_chapter = 2;
        apply_converge(
            &mut matches,
            &mut unmatched,
            &mut proposals,
            &fuzzy,
            &converge,
            &policy,
        );
        assert_eq!(matches.len(), 2);
        assert!(matches.iter().all(|m| m.target_name == "函数" || m.target_name == "集合"));
        assert_eq!(unmatched.len(), 1);
        assert_eq!(unmatched[0].raw_name, "导数");
    }

    #[test]
    fn apply_converge_clears_all_source_keys_of_mapped_node() {
        let leaf = fake_candidate_keys(
            "交集的概念及运算",
            &["集合的交集运算", "交集"],
            0.7,
        );
        let fuzzy = vec![(TaggingDimension::Knowledge, vec![leaf])];
        let mut matches = Vec::new();
        let mut unmatched = vec![
            pending_unmatched(TaggingDimension::Knowledge, "集合的交集运算".into()),
            pending_unmatched(TaggingDimension::Knowledge, "交集".into()),
        ];
        let mut proposals = Vec::new();
        let converge = AiConvergeResult {
            knowledge: vec![AiConvergePick {
                key: "集合的交集运算".into(),
                name: Some("交集的概念及运算".into()),
            }],
            ..AiConvergeResult::default()
        };
        apply_converge(
            &mut matches,
            &mut unmatched,
            &mut proposals,
            &fuzzy,
            &converge,
            &TaggingPolicy::default(),
        );
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].ai_name, "集合的交集运算");
        assert!(unmatched.is_empty(), "同一节点的全部来源关键词都应消解: {unmatched:?}");
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].raw_name, "集合的交集运算");
    }

    #[test]
    fn menu_marks_empty_sections() {
        let menu = build_candidate_menu(&[(
            TaggingDimension::Chapter,
            vec![fake_candidate("函数", 0.85)],
        )]);
        assert!(menu.contains("【章节候选列表】"));
        assert!(menu.contains("1. 函数（路径：函数；查询词：函数；相关度 0.85）"));
        assert!(menu.contains("【知识点候选列表】（无候选）"));
    }

    #[test]
    fn query_keys_block_lists_extract_terms() {
        let signals = TaggingSignals {
            chapter_keys: vec!["集合".into()],
            knowledge_keys: vec!["集合的交集运算".into()],
            ..TaggingSignals::default()
        };
        let block = build_query_keys_block(&signals);
        assert!(block.contains("【待对齐关键词】"));
        assert!(block.contains("章节：集合"));
        assert!(block.contains("知识点：集合的交集运算"));
    }

    #[test]
    fn parsed_adapter_uses_chapter_leaf_and_methods() {
        let q = ParsedQuestion {
            question_type: "solution".into(),
            sub_type: None,
            difficulty: Some("easy".into()),
            stem: "题干".into(),
            options: None,
            correct_answer: None,
            analysis: vec![],
            knowledge_points: vec!["二次函数最值".into()],
            confidence: 0.8,
            warnings: vec![],
            image_placeholders: vec![],
            image_urls: vec![],
            kp_matches: vec![],
            question_no: None,
            display_order: None,
            score: None,
            chapter_path: vec!["函数".into(), "二次函数".into()],
            solution_methods: vec![crate::ai::types::SolutionMethod {
                name: "数形结合".into(),
                confidence: Some(0.9),
            }],
        };
        let s = signals_from_parsed(&q);
        assert_eq!(s.chapter_keys, vec!["二次函数".to_string()]);
        assert_eq!(s.knowledge_keys, vec!["二次函数最值".to_string()]);
        assert_eq!(s.method_keys, vec!["数形结合".to_string()]);
        assert!(s.pattern_keys.is_empty());
        assert_eq!(s.difficulty, Some(2));
    }

    #[test]
    fn short_or_generic_keys_cannot_submit_as_new() {
        let u = pending_unmatched(TaggingDimension::Chapter, "函数".into());
        assert!(!u.eligible_for_candidate);
        let u = pending_unmatched(TaggingDimension::Knowledge, "集合".into());
        assert!(!u.eligible_for_candidate);
        let u = pending_unmatched(TaggingDimension::Knowledge, "交集的概念及运算".into());
        assert!(u.eligible_for_candidate);
    }

    #[test]
    fn refine_unmatched_drops_hits_and_caps_per_dim() {
        let matches = vec![crate::ai::tagging::types::TaggingMatch {
            dimension: TaggingDimension::Knowledge,
            target_type: crate::ai::tagging::types::TaggingTargetType::KnowledgeNode,
            ai_name: "交集运算".into(),
            target_id: Uuid::new_v4(),
            target_name: "交集的概念及运算".into(),
            tree_id: None,
            path: None,
            depth: None,
            category: None,
            score: 0.9,
            match_type: crate::ai::tagging::types::TaggingMatchType::Exact,
        }];
        let mut unmatched = vec![
            pending_unmatched(TaggingDimension::Knowledge, "交集的概念及运算".into()),
            pending_unmatched(TaggingDimension::Knowledge, "完全不存在的知识点甲".into()),
            pending_unmatched(TaggingDimension::Knowledge, "完全不存在的知识点乙".into()),
            pending_unmatched(TaggingDimension::Knowledge, "完全不存在的知识点丙".into()),
        ];
        refine_unmatched(&mut unmatched, &matches);
        assert!(unmatched.iter().all(|u| u.raw_name != "交集的概念及运算"));
        assert_eq!(unmatched.len(), 2);
    }

    #[test]
    fn tagging_content_joins_stem_options_answer_analysis() {
        let q = ParsedQuestion {
            question_type: "choice".into(),
            sub_type: None,
            difficulty: Some("medium".into()),
            stem: "如图阴影部分表示的集合是".into(),
            options: Some(vec![crate::ai::types::ParsedOption {
                label: "A".into(),
                content: "$A \\cap B$".into(),
            }]),
            correct_answer: Some(ParsedAnswer::Choice {
                options: vec!["B".into()],
            }),
            analysis: vec![crate::ai::types::AnalysisMethod {
                title: "解法一".into(),
                content: "数形结合。".into(),
            }],
            knowledge_points: vec![],
            confidence: 0.9,
            warnings: vec![],
            image_placeholders: vec![],
            image_urls: vec![],
            kp_matches: vec![],
            question_no: None,
            display_order: None,
            score: None,
            chapter_path: vec![],
            solution_methods: vec![],
        };
        let content = tagging_content_from_parsed(&q);
        assert!(content.contains("如图阴影部分表示的集合是"));
        assert!(content.contains("A. $A \\cap B$"));
        assert!(content.contains("参考答案：B"));
        assert!(content.contains("解析：数形结合。"));
    }
}
