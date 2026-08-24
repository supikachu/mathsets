//! 知识树 / 扁平标签召回（exact/alias + 词级/包含/分词，供 LLM 语义收敛）

use axum::{http::StatusCode, Json};
use regex::Regex;
use serde_json::json;
use std::sync::OnceLock;
use uuid::Uuid;

use super::types::{
    KnowledgeNodeMatch, TagMatch, TaggingDimension, TaggingMatch, TaggingMatchType,
    TaggingPolicy, TaggingTargetType,
};
use super::vector::{
    merge_node_candidate, recall_nodes_vector, vector_match_tag_top1, VectorRecallStats,
};

#[derive(Debug, Clone)]
pub struct NodeCandidate {
    pub id: Uuid,
    pub name: String,
    pub tree_id: Uuid,
    pub path: String,
    pub depth: i16,
    pub score: f32,
    pub match_type: TaggingMatchType,
    /// 召回该节点的全部查询词（同一节点可被多个关键词命中）
    pub source_keys: Vec<String>,
    /// 其中以 exact/alias 命中的查询词（确定采纳时只消解这些，避免泛化词根误清未匹配）
    pub deterministic_keys: Vec<String>,
    /// 祖先名路径，供收敛 Prompt 理解层级（如 集合 / 交集 / 交集的概念及运算）
    pub name_path: String,
}

impl NodeCandidate {
    pub fn primary_key(&self) -> &str {
        self.source_keys.first().map(String::as_str).unwrap_or("")
    }

    pub fn to_tagging_match(&self, dim: TaggingDimension) -> TaggingMatch {
        TaggingMatch {
            dimension: dim,
            target_type: TaggingTargetType::KnowledgeNode,
            ai_name: self.primary_key().to_string(),
            target_id: self.id,
            target_name: self.name.clone(),
            tree_id: Some(self.tree_id),
            path: Some(self.path.clone()),
            depth: Some(self.depth),
            category: None,
            score: self.score,
            match_type: self.match_type,
        }
    }

    pub fn to_tagging_match_for_key(&self, dim: TaggingDimension, key: &str) -> TaggingMatch {
        let mut m = self.to_tagging_match(dim);
        m.ai_name = key.to_string();
        m
    }
}

type DbErr = (StatusCode, Json<serde_json::Value>);

fn db_err(msg: impl Into<String>) -> DbErr {
    let msg = msg.into();
    tracing::warn!("{msg}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": msg})),
    )
}

fn chapter_prefix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^第[一二三四五六七八九十百零〇0-9]+章\s*").unwrap())
}

fn leading_number_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[0-9]+(\.[0-9]+)*\s*").unwrap())
}

/// 去掉「第一章」「1.3」等教材序号，便于「集合」对上「第一章 集合与常用逻辑用语」
pub(crate) fn strip_textbook_prefix(name: &str) -> String {
    let s = chapter_prefix_re().replace(name.trim(), "");
    leading_number_re().replace(s.trim(), "").trim().to_string()
}

/// 泛化词根：禁止单独作为检索 token，避免「运算」召回函数/导数整支。
pub(crate) fn is_generic_token(tok: &str) -> bool {
    matches!(
        tok,
        "运算"
            | "概念"
            | "应用"
            | "基本"
            | "问题"
            | "方法"
            | "性质"
            | "及其"
            | "常用"
            | "用语"
            | "知识"
            | "专题"
            | "题型"
            | "内容"
            | "关系"
            | "函数" // 过宽：单独召回会命中指数/对数/三角等整支
            | "方程"
            | "不等式"
    )
}

/// 章节关键词若仅为过宽词且同时有更具体词，则丢弃，避免「函数」召回「正弦函数」。
pub(crate) fn is_overly_broad_chapter_key(key: &str) -> bool {
    matches!(
        key.trim(),
        "函数" | "方程" | "不等式" | "代数" | "几何" | "数学"
    )
}

/// 强主题标记：候选名含这些且题干/关键词完全未出现时，视为离题噪声。
const STRONG_TOPIC_MARKERS: &[&str] = &[
    "正弦", "余弦", "正切", "余切", "三角", "弧度", "诱导公式",
    "椭圆", "双曲线", "抛物线", "圆锥曲线",
    "向量", "复数",
    "概率", "统计", "排列", "组合",
    "导数", "定积分", "不定积分",
    "立体几何", "空间向量", "球",
    "数列", "等差", "等比",
    "解析几何",
    "指数", "对数",
    "集合", "充要",
    "线性规划",
];

/// 剔除与题目主题明显冲突的候选（如指数题召回「求正弦函数…」章节）。
/// 仅当题干/关键词已出现至少一个强主题词时才过滤，避免无证据时误杀正确章节。
pub(crate) fn filter_offtopic_candidates(
    candidates: Vec<NodeCandidate>,
    question: &str,
    keys: &[String],
) -> Vec<NodeCandidate> {
    let mut hay = question.to_string();
    for k in keys {
        hay.push(' ');
        hay.push_str(k);
    }
    let has_topic_evidence = STRONG_TOPIC_MARKERS.iter().any(|m| hay.contains(m));
    if !has_topic_evidence {
        return candidates;
    }
    candidates
        .into_iter()
        .filter(|c| {
            // 只看节点自身名称：祖先路径常含并列主题（如「指数函数与对数函数」）
            for marker in STRONG_TOPIC_MARKERS {
                if c.name.contains(marker) && !hay.contains(marker) {
                    return false;
                }
            }
            true
        })
        .collect()
}

fn push_token(tokens: &mut Vec<String>, tok: String) {
    if tok.chars().count() < 2 || is_generic_token(&tok) {
        return;
    }
    if !tokens.iter().any(|t| t == &tok) {
        tokens.push(tok);
    }
}

/// 从查询词抽出可检索词根：按虚词切开，保留较长片段的**首**双字（不取尾双字）。
/// 「集合的交集运算」→ 集合、交集运算、交集（不含「运算」）
pub(crate) fn recall_tokens(key: &str) -> Vec<String> {
    let stripped = strip_textbook_prefix(key);
    let mut tokens = Vec::new();
    if stripped.chars().count() >= 2 {
        push_token(&mut tokens, stripped.clone());
    }
    for run in stripped.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
        if run.len() >= 4 {
            push_token(&mut tokens, run.to_string());
        }
    }
    for part in stripped.split(|c: char| "的与及和、，, /（）()【】[]".contains(c)) {
        let p = part.trim();
        let n = p.chars().count();
        if n < 2 {
            continue;
        }
        push_token(&mut tokens, p.to_string());
        if n >= 4 {
            let chars: Vec<char> = p.chars().collect();
            let head: String = chars[..2].iter().collect();
            push_token(&mut tokens, head);
        }
    }
    tokens.truncate(12);
    tokens
}

/// 章节主题词给知识点/专题候选加分，抑制跨主题噪声。
pub(crate) fn rerank_by_topic(mut candidates: Vec<NodeCandidate>, hints: &[String]) -> Vec<NodeCandidate> {
    let hint_tokens: Vec<String> = hints
        .iter()
        .flat_map(|h| recall_tokens(h))
        .filter(|t| t.chars().count() >= 2 && !is_generic_token(t))
        .collect();
    if hint_tokens.is_empty() {
        return candidates;
    }
    for c in &mut candidates {
        let hay = format!("{} {}", c.name, c.name_path);
        if hint_tokens.iter().any(|t| hay.contains(t.as_str())) {
            c.score = (c.score + 0.12).min(1.0);
        }
    }
    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates
}

/// 将前端/任务学段规范为树 code 后缀：`junior` | `high`；无法识别则不过滤。
pub fn tree_stage_code_suffix(stage: Option<&str>) -> Option<&'static str> {
    match stage.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("junior") | Some("初中") => Some("junior"),
        Some("senior") | Some("high") | Some("高中") => Some("high"),
        _ => None,
    }
}

/// 按维度召回：exact/alias 可直接确定；包含、分词、word_similarity 一律视为 fuzzy，交给 LLM 语义收敛。
/// `stage` 为 `junior`/`senior` 时只召回对应学段树（code 后缀 `_junior` / `_high`）。
pub async fn recall_nodes(
    pool: &sqlx::PgPool,
    keys: &[String],
    dim: TaggingDimension,
    policy: &TaggingPolicy,
    space_id: Option<Uuid>,
    stage: Option<&str>,
) -> Result<Vec<NodeCandidate>, DbErr> {
    let (candidates, _stats) =
        recall_nodes_with_stats(pool, keys, dim, policy, space_id, stage).await?;
    Ok(candidates)
}

/// 与 `recall_nodes` 相同，额外返回向量召回观测字段。
pub async fn recall_nodes_with_stats(
    pool: &sqlx::PgPool,
    keys: &[String],
    dim: TaggingDimension,
    policy: &TaggingPolicy,
    space_id: Option<Uuid>,
    stage: Option<&str>,
) -> Result<(Vec<NodeCandidate>, VectorRecallStats), DbErr> {
    let empty_stats = VectorRecallStats {
        hits: 0,
        elapsed_ms: 0,
    };
    let Some(tree_kind) = dim.tree_kind() else {
        return Ok((vec![], empty_stats));
    };
    if keys.is_empty() {
        return Ok((vec![], empty_stats));
    }

    let mut merged: std::collections::HashMap<Uuid, NodeCandidate> =
        std::collections::HashMap::new();
    let leaf_only = dim.leaf_only();
    let threshold = policy.fuzzy_threshold;
    let per_key_limit: i64 = (policy.recall_limit(dim).max(40)) as i64;
    let stage_suffix = tree_stage_code_suffix(stage);
    // 仅在指定学段时收紧 code 形态（避免 knowledge 误召 math_method_*）；
    // 未指定学段时保持旧行为，兼容测试树与解析 Worker。
    let code_infix: Option<&str> = if stage_suffix.is_some() {
        match dim {
            TaggingDimension::Chapter => Some("chapter"),
            TaggingDimension::Knowledge => Some("knowledge"),
            TaggingDimension::Pattern => Some("method"),
            _ => None,
        }
    } else {
        None
    };

    for key in keys {
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let tokens = recall_tokens(key);

        let rows: Vec<(Uuid, String, Uuid, String, i16, f32, String, String)> = sqlx::query_as(
            r#"
            SELECT
              kn.id,
              kn.name,
              kn.tree_id,
              kn.path::text,
              kn.depth,
              CASE
                WHEN kn.name = $1 THEN 1.0::real
                WHEN EXISTS (
                  SELECT 1 FROM jsonb_array_elements(kn.aliases) AS a
                  WHERE a->>'alias' = $1
                ) THEN 0.95::real
                WHEN char_length($1) >= 2 AND (
                  position($1 in kn.name) > 0 OR position($1 in s.stripped) > 0
                ) THEN GREATEST(
                  0.72::real,
                  similarity(kn.name::text, $1),
                  word_similarity($1, kn.name::text),
                  word_similarity($1, s.stripped)
                )
                WHEN char_length($1) >= 2
                     AND char_length(kn.name) >= 2
                     AND position(kn.name in $1) > 0 THEN GREATEST(
                  0.70::real,
                  similarity(kn.name::text, $1),
                  word_similarity($1, kn.name::text)
                )
                WHEN EXISTS (
                  SELECT 1 FROM unnest($7::text[]) AS tok
                  WHERE char_length(tok) >= 2 AND position(tok in kn.name) > 0
                ) THEN GREATEST(
                  0.65::real,
                  similarity(kn.name::text, $1),
                  word_similarity($1, kn.name::text),
                  word_similarity($1, s.stripped)
                )
                ELSE GREATEST(
                  similarity(kn.name::text, $1),
                  word_similarity($1, kn.name::text),
                  word_similarity($1, s.stripped)
                )
              END AS score,
              CASE
                WHEN kn.name = $1 THEN 'exact'
                WHEN EXISTS (
                  SELECT 1 FROM jsonb_array_elements(kn.aliases) AS a
                  WHERE a->>'alias' = $1
                ) THEN 'alias'
                ELSE 'fuzzy'
              END AS match_type,
              COALESCE((
                SELECT string_agg(anc.name, ' / ' ORDER BY anc.depth)
                FROM knowledge_nodes anc
                WHERE anc.tree_id = kn.tree_id
                  AND kn.path <@ anc.path
                  AND anc.is_active = TRUE
              ), kn.name) AS name_path
            FROM knowledge_nodes kn
            JOIN knowledge_trees kt ON kt.id = kn.tree_id
            CROSS JOIN LATERAL (
              SELECT regexp_replace(
                       regexp_replace(kn.name, '^第[一二三四五六七八九十百零〇0-9]+章\s*', ''),
                       '^[0-9]+(\.[0-9]+)*\s*',
                       ''
                     ) AS stripped
            ) s
            WHERE kn.is_active = TRUE
              AND kn.status = 'active'
              AND kn.canonical_id IS NULL
              AND kt.is_active = TRUE
              AND (kt.space_id IS NULL OR kt.space_id = $2)
              AND (
                -- pattern 维度：库内题型专题树 kind 可能是 ability 或 knowledge（math_method_*）
                ($3 = 'ability' AND (
                  kt.kind::text = 'ability'
                  OR (kt.kind::text = 'knowledge' AND kt.code LIKE '%_method_%')
                ))
                OR ($3 <> 'ability' AND kt.kind::text = $3)
              )
              AND ($8::text IS NULL OR kt.code LIKE '%_' || $8)
              AND ($9::text IS NULL OR kt.code LIKE '%_' || $9 || '_%')
              AND (
                NOT $4 OR NOT EXISTS (
                  SELECT 1 FROM knowledge_nodes child
                  WHERE child.parent_id = kn.id AND child.is_active = TRUE
                )
              )
              AND (
                kn.name = $1
                OR EXISTS (
                  SELECT 1 FROM jsonb_array_elements(kn.aliases) AS a
                  WHERE a->>'alias' = $1
                )
                OR (kn.name::text % $1 AND similarity(kn.name::text, $1) >= $5)
                OR word_similarity($1, kn.name::text) >= $5
                OR word_similarity($1, s.stripped) >= $5
                OR (
                  char_length($1) >= 2 AND (
                    position($1 in kn.name) > 0 OR position($1 in s.stripped) > 0
                  )
                )
                OR (
                  char_length($1) >= 2
                  AND char_length(kn.name) >= 2
                  AND position(kn.name in $1) > 0
                )
                OR EXISTS (
                  SELECT 1 FROM unnest($7::text[]) AS tok
                  WHERE char_length(tok) >= 2 AND position(tok in kn.name) > 0
                )
              )
            ORDER BY score DESC, kn.depth ASC
            LIMIT $6
            "#,
        )
        .bind(key)
        .bind(space_id)
        .bind(tree_kind)
        .bind(leaf_only)
        .bind(threshold)
        .bind(per_key_limit)
        .bind(&tokens)
        .bind(stage_suffix)
        .bind(code_infix)
        .fetch_all(pool)
        .await
        .map_err(|e| db_err(format!("维度「{}」候选召回失败: {e}", dim.as_str())))?;

        for (id, name, tree_id, path, depth, score, match_type, name_path) in rows {
            let mt = TaggingMatchType::from_db(&match_type);
            let c = NodeCandidate {
                id,
                name,
                tree_id,
                path,
                depth,
                score,
                match_type: mt,
                source_keys: vec![key.to_string()],
                deterministic_keys: if mt.is_deterministic() {
                    vec![key.to_string()]
                } else {
                    vec![]
                },
                name_path,
            };
            merge_node_candidate(&mut merged, c);
        }
    }

    let mut vector_stats = VectorRecallStats {
        hits: 0,
        elapsed_ms: 0,
    };
    match recall_nodes_vector(pool, keys, dim, policy, space_id, stage).await {
        Ok((hits, stats)) => {
            vector_stats = stats;
            for c in hits {
                merge_node_candidate(&mut merged, c);
            }
        }
        Err(e) => {
            tracing::debug!(dimension = dim.as_str(), "向量召回跳过: {e}");
        }
    }

    let mut all: Vec<NodeCandidate> = merged.into_values().collect();
    all.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    all.truncate(policy.recall_limit(dim));
    Ok((all, vector_stats))
}

/// 单点最佳匹配（录题 Worker 兼容路径）
pub async fn match_nodes(
    pool: &sqlx::PgPool,
    ai_names: &[String],
    space_id: Option<Uuid>,
    tree_kind: &str,
) -> Result<(Vec<KnowledgeNodeMatch>, Vec<String>), DbErr> {
    let dim = TaggingDimension::from_tree_kind(tree_kind);
    let policy = TaggingPolicy::default();
    let mut matched = Vec::new();
    let mut unmatched = Vec::new();

    for name in ai_names {
        if name.trim().is_empty() {
            continue;
        }
        let recs = recall_nodes(pool, &[name.clone()], dim, &policy, space_id, None).await?;
        match recs.into_iter().next() {
            Some(c) if c.score >= policy.fuzzy_threshold => {
                let mut m = c.to_tagging_match(dim);
                m.ai_name = name.clone();
                if let Some(km) = m.to_knowledge_node_match() {
                    matched.push(km);
                }
            }
            _ => unmatched.push(name.clone()),
        }
    }
    Ok((matched, unmatched))
}

pub async fn match_tags(
    pool: &sqlx::PgPool,
    ai_names: &[String],
    category: &str,
    space_id: Option<Uuid>,
) -> Result<(Vec<TagMatch>, Vec<String>), DbErr> {
    if ai_names.is_empty() {
        return Ok((vec![], vec![]));
    }

    let dim = if category == "method" {
        TaggingDimension::Method
    } else {
        TaggingDimension::CoreCompetence
    };
    let threshold = TaggingPolicy::default().fuzzy_threshold;
    let mut matched = Vec::new();
    let mut unmatched = Vec::new();

    for name in ai_names {
        if name.trim().is_empty() {
            continue;
        }
        let result: Option<(Uuid, String, String, f32, String)> = sqlx::query_as(
            r#"
            SELECT
              t.id,
              t.name,
              t.category::text,
              CASE
                WHEN t.name = $1 THEN 1.0::real
                WHEN EXISTS (
                  SELECT 1 FROM jsonb_array_elements(t.aliases) AS a
                  WHERE a->>'alias' = $1
                ) THEN 0.95::real
                ELSE similarity(t.name::text, $1)
              END AS score,
              CASE
                WHEN t.name = $1 THEN 'exact'
                WHEN EXISTS (
                  SELECT 1 FROM jsonb_array_elements(t.aliases) AS a
                  WHERE a->>'alias' = $1
                ) THEN 'alias'
                ELSE 'fuzzy'
              END AS match_type
            FROM tags t
            WHERE t.is_active = TRUE
              AND t.category::text = $2
              AND (t.space_id IS NULL OR t.space_id = $3)
              AND (
                t.name = $1
                OR EXISTS (
                  SELECT 1 FROM jsonb_array_elements(t.aliases) AS a
                  WHERE a->>'alias' = $1
                )
                OR (t.name::text % $1 AND similarity(t.name::text, $1) >= $4)
              )
            ORDER BY score DESC
            LIMIT 1
            "#,
        )
        .bind(name)
        .bind(category)
        .bind(space_id)
        .bind(threshold)
        .fetch_optional(pool)
        .await
        .map_err(|e| db_err(format!("标签匹配查询失败: {e}")))?;

        match result {
            Some((tag_id, tag_name, cat, score, match_type)) if score >= threshold => {
                matched.push(TagMatch {
                    ai_name: name.clone(),
                    tag_id,
                    tag_name,
                    category: cat,
                    score,
                    match_type,
                });
            }
            _ => {
                match vector_match_tag_top1(pool, name, category, space_id, threshold).await {
                    Ok(Some((tag_id, tag_name, cat, score))) => {
                        matched.push(TagMatch {
                            ai_name: name.clone(),
                            tag_id,
                            tag_name,
                            category: cat,
                            score,
                            match_type: "fuzzy".to_string(),
                        });
                    }
                    Ok(None) => unmatched.push(name.clone()),
                    Err(e) => {
                        tracing::debug!("标签向量召回跳过: {e}");
                        unmatched.push(name.clone());
                    }
                }
            }
        }
    }

    let _ = dim;
    Ok((matched, unmatched))
}

pub fn tagging_match_from_tag(m: &TagMatch, dim: TaggingDimension) -> TaggingMatch {
    TaggingMatch {
        dimension: dim,
        target_type: TaggingTargetType::Tag,
        ai_name: m.ai_name.clone(),
        target_id: m.tag_id,
        target_name: m.tag_name.clone(),
        tree_id: None,
        path: None,
        depth: None,
        category: Some(m.category.clone()),
        score: m.score,
        match_type: TaggingMatchType::from_db(&m.match_type),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_textbook_prefix_drops_chapter_and_section_numbers() {
        assert_eq!(
            strip_textbook_prefix("第一章 集合与常用逻辑用语"),
            "集合与常用逻辑用语"
        );
        assert_eq!(strip_textbook_prefix("1.3 集合的基本运算"), "集合的基本运算");
        assert_eq!(strip_textbook_prefix("集合"), "集合");
    }

    #[test]
    fn recall_tokens_splits_function_words_and_keeps_stems() {
        let tokens = recall_tokens("集合的交集运算");
        assert!(tokens.contains(&"集合".to_string()));
        assert!(tokens.contains(&"交集运算".to_string()));
        assert!(tokens.contains(&"交集".to_string()));
        assert!(
            !tokens.contains(&"运算".to_string()),
            "泛化尾词「运算」不得作为检索词根: {tokens:?}"
        );
        assert!(tokens.contains(&"集合的交集运算".to_string()));
    }

    #[test]
    fn generic_token_is_filtered() {
        assert!(is_generic_token("运算"));
        assert!(is_generic_token("函数"));
        assert!(!is_generic_token("交集"));
        assert!(!is_generic_token("集合"));
        assert!(is_overly_broad_chapter_key("函数"));
        assert!(!is_overly_broad_chapter_key("指数函数"));
    }

    fn fake_cand(name: &str, path: &str) -> NodeCandidate {
        NodeCandidate {
            id: Uuid::new_v4(),
            name: name.to_string(),
            tree_id: Uuid::new_v4(),
            path: "a".into(),
            depth: 1,
            score: 0.8,
            match_type: TaggingMatchType::Fuzzy,
            source_keys: vec!["函数".into()],
            deterministic_keys: vec![],
            name_path: path.to_string(),
        }
    }

    #[test]
    fn filter_offtopic_drops_sine_when_question_is_exponential() {
        let kept = filter_offtopic_candidates(
            vec![
                fake_cand("4.2 指数函数", "必修一 / 指数函数与对数函数 / 4.2 指数函数"),
                fake_cand("求正弦（型）函数的最值", "必修一 / 三角函数 / 求正弦（型）函数的最值"),
            ],
            "已知函数 f(x)=(2^x+a)/(2^x+1) 为奇函数",
            &["指数函数".into(), "奇函数".into()],
        );
        assert_eq!(kept.len(), 1, "{:?}", kept.iter().map(|c| &c.name).collect::<Vec<_>>());
        assert!(kept[0].name.contains("指数"));
    }

    #[test]
    fn filter_offtopic_noop_without_strong_topic_evidence() {
        let kept = filter_offtopic_candidates(
            vec![
                fake_cand("4.2 指数函数", "指数函数"),
                fake_cand("求正弦函数的最值", "三角函数"),
            ],
            "已知函数 f(x)=x+1",
            &["函数".into()],
        );
        assert_eq!(kept.len(), 2);
    }
}
