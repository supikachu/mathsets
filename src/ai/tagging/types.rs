//! 统一智能打标领域类型（五维契约）

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ai::types::ParsedQuestion;

/// 引擎版本：召回/收敛规则变更时递增，写入 suggestion.engine_version
pub const ENGINE_VERSION: &str = "tagging-v4";

/// 打标维度。节点三维进 knowledge_nodes，方法/素养进 tags。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaggingDimension {
    Chapter,
    Knowledge,
    Pattern,
    Method,
    CoreCompetence,
}

impl TaggingDimension {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chapter => "chapter",
            Self::Knowledge => "knowledge",
            Self::Pattern => "pattern",
            Self::Method => "method",
            Self::CoreCompetence => "core_competence",
        }
    }

    pub fn target_type(self) -> TaggingTargetType {
        match self {
            Self::Chapter | Self::Knowledge | Self::Pattern => TaggingTargetType::KnowledgeNode,
            Self::Method | Self::CoreCompetence => TaggingTargetType::Tag,
        }
    }

    /// 知识树 kind；pattern 对应历史枚举 `ability`
    pub fn tree_kind(self) -> Option<&'static str> {
        match self {
            Self::Chapter => Some("chapter"),
            Self::Knowledge => Some("knowledge"),
            Self::Pattern => Some("ability"),
            Self::Method | Self::CoreCompetence => None,
        }
    }

    pub fn tag_category(self) -> Option<&'static str> {
        match self {
            Self::Method => Some("method"),
            Self::CoreCompetence => Some("core_competence"),
            _ => None,
        }
    }

    /// 知识点 / 题型专题只打叶子；章节允许父节点
    pub fn leaf_only(self) -> bool {
        matches!(self, Self::Knowledge | Self::Pattern)
    }

    pub fn from_tree_kind(kind: &str) -> Self {
        match kind {
            "chapter" => Self::Chapter,
            "ability" => Self::Pattern,
            _ => Self::Knowledge,
        }
    }

    pub fn from_kind_str(kind: &str) -> Option<Self> {
        match kind {
            "chapter" => Some(Self::Chapter),
            "knowledge" => Some(Self::Knowledge),
            "pattern" | "ability" => Some(Self::Pattern),
            "method" => Some(Self::Method),
            "core_competence" => Some(Self::CoreCompetence),
            _ => None,
        }
    }
}

/// 匹配目标落点
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaggingTargetType {
    KnowledgeNode,
    Tag,
}

impl TaggingTargetType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::KnowledgeNode => "knowledge_node",
            Self::Tag => "tag",
        }
    }
}

/// 匹配类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaggingMatchType {
    Exact,
    Alias,
    Fuzzy,
}

impl TaggingMatchType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Alias => "alias",
            Self::Fuzzy => "fuzzy",
        }
    }

    pub fn from_db(s: &str) -> Self {
        match s {
            "exact" => Self::Exact,
            "alias" => Self::Alias,
            _ => Self::Fuzzy,
        }
    }

    pub fn is_deterministic(self) -> bool {
        matches!(self, Self::Exact | Self::Alias)
    }
}

/// 打标输入：编辑页题文 或 录题已解析结构
#[derive(Debug, Clone)]
pub enum TaggingInput {
    Content {
        content: String,
    },
    Parsed(Box<ParsedQuestion>),
    /// 题文 + 解析阶段已产出的信号。
    ///
    /// 解析阶段（Stage2）已让 LLM 给出知识点 / 章节 / 解法，打标再对同一段题文重抽一遍
    /// 关键词纯属重复劳动。信号足够时直接复用，省掉一次 LLM 往返；过弱则回退到对
    /// `content` 做 LLM 提取，与 `Content` 路径完全一致。
    ContentWithSignals {
        content: String,
        signals: Box<TaggingSignals>,
    },
}

impl TaggingInput {
    pub fn content_preview(&self) -> &str {
        match self {
            Self::Content { content } | Self::ContentWithSignals { content, .. } => content,
            Self::Parsed(q) => q.stem.as_str(),
        }
    }
}

/// 调用上下文（不把完整题文写入 suggestion 表）
#[derive(Debug, Clone, Default)]
pub struct TaggingContext {
    pub user_id: Uuid,
    pub space_id: Option<Uuid>,
    pub question_id: Option<Uuid>,
    pub source_task_id: Option<Uuid>,
    pub source_index: Option<String>,
    /// 学段约束：`junior` | `senior`（高中树 code 后缀为 `_high`）。
    /// 有值时召回只命中对应学段知识树，避免高中题挂上初中章节。
    pub stage: Option<String>,
}

/// 召回 / 收敛策略
#[derive(Debug, Clone)]
pub struct TaggingPolicy {
    pub fuzzy_threshold: f32,
    pub recall_limit_chapter: usize,
    pub recall_limit_knowledge: usize,
    pub recall_limit_pattern: usize,
    pub max_chapter: usize,
    pub max_knowledge: usize,
    pub max_pattern: usize,
    pub max_method: usize,
    pub max_competence: usize,
    pub run_llm_extract: bool,
    pub run_llm_converge: bool,
    /// 建议落库失败时是否让整个打标失败。Worker 暂存应设为 false。
    pub fail_on_persist: bool,
}

impl Default for TaggingPolicy {
    fn default() -> Self {
        Self {
            fuzzy_threshold: 0.3,
            recall_limit_chapter: 20,
            recall_limit_knowledge: 30,
            recall_limit_pattern: 15,
            max_chapter: 3,
            max_knowledge: 3,
            max_pattern: 3,
            max_method: 5,
            max_competence: 3,
            run_llm_extract: true,
            run_llm_converge: true,
            fail_on_persist: true,
        }
    }
}

impl TaggingPolicy {
    pub fn recall_limit(&self, dim: TaggingDimension) -> usize {
        match dim {
            TaggingDimension::Chapter => self.recall_limit_chapter,
            TaggingDimension::Knowledge => self.recall_limit_knowledge,
            TaggingDimension::Pattern => self.recall_limit_pattern,
            TaggingDimension::Method => self.max_method.saturating_mul(4),
            TaggingDimension::CoreCompetence => self.max_competence.saturating_mul(4),
        }
    }

    pub fn max_selected(&self, dim: TaggingDimension) -> usize {
        match dim {
            TaggingDimension::Chapter => self.max_chapter,
            TaggingDimension::Knowledge => self.max_knowledge,
            TaggingDimension::Pattern => self.max_pattern,
            TaggingDimension::Method => self.max_method,
            TaggingDimension::CoreCompetence => self.max_competence,
        }
    }
}

/// 阶段一提取出的关键词与元数据
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaggingSignals {
    #[serde(default)]
    pub chapter_keys: Vec<String>,
    #[serde(default)]
    pub knowledge_keys: Vec<String>,
    #[serde(default)]
    pub pattern_keys: Vec<String>,
    #[serde(default)]
    pub method_keys: Vec<String>,
    #[serde(default)]
    pub core_competencies: Vec<String>,
    pub difficulty: Option<i16>,
    pub question_type: Option<String>,
    pub grade_level: Option<String>,
    pub cognitive_level: Option<String>,
}

impl TaggingSignals {
    pub fn keys(&self, dim: TaggingDimension) -> &[String] {
        match dim {
            TaggingDimension::Chapter => &self.chapter_keys,
            TaggingDimension::Knowledge => &self.knowledge_keys,
            TaggingDimension::Pattern => &self.pattern_keys,
            TaggingDimension::Method => &self.method_keys,
            TaggingDimension::CoreCompetence => &self.core_competencies,
        }
    }
}

/// 单条匹配
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaggingMatch {
    pub dimension: TaggingDimension,
    pub target_type: TaggingTargetType,
    pub ai_name: String,
    pub target_id: Uuid,
    pub target_name: String,
    pub tree_id: Option<Uuid>,
    pub path: Option<String>,
    pub depth: Option<i16>,
    pub category: Option<String>,
    pub score: f32,
    pub match_type: TaggingMatchType,
}

/// 未匹配项（确认保存后可进候选队列）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaggingUnmatched {
    pub id: String,
    pub dimension: TaggingDimension,
    pub target_type: TaggingTargetType,
    pub raw_name: String,
    pub normalized_name: String,
    pub confidence: Option<f32>,
    pub reason: String,
    pub eligible_for_candidate: bool,
}

/// fuzzy 对齐成功后的别名提案（确认保存才写入 tag_candidates）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaggingAliasProposal {
    pub dimension: TaggingDimension,
    pub raw_name: String,
    pub normalized_name: String,
    pub target_id: Uuid,
    pub target_type: TaggingTargetType,
    pub score: f32,
}

/// 统一建议结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaggingSuggestion {
    pub suggestion_id: Option<Uuid>,
    pub engine_version: String,
    pub input_hash: String,
    pub needs_review: bool,
    pub matches: Vec<TaggingMatch>,
    pub unmatched: Vec<TaggingUnmatched>,
    /// 确认保存时写入 suggested_node_id / suggested_tag_id
    #[serde(default)]
    pub alias_proposals: Vec<TaggingAliasProposal>,
    pub difficulty: Option<i16>,
    pub question_type: Option<String>,
    pub grade_level: Option<String>,
    pub cognitive_level: Option<String>,
}

impl TaggingSuggestion {
    pub fn matches_for(&self, dim: TaggingDimension) -> Vec<&TaggingMatch> {
        self.matches.iter().filter(|m| m.dimension == dim).collect()
    }

    /// 兼容旧暂存 `matched[]`（仅知识树节点；kind 用 tree_kind，pattern → ability）
    pub fn compat_matched_nodes(&self) -> Vec<serde_json::Value> {
        self.matches
            .iter()
            .filter_map(|m| {
                let n = m.to_knowledge_node_match()?;
                Some(serde_json::json!({
                    "node_id": n.node_id,
                    "node_name": n.node_name,
                    "ai_name": n.ai_name,
                    "tree_id": n.tree_id,
                    "path": n.path,
                    "depth": n.depth,
                    "score": n.score,
                    "match_type": n.match_type,
                    "kind": m.dimension.tree_kind().unwrap_or("knowledge"),
                }))
            })
            .collect()
    }

    /// 兼容旧暂存 `unmatched.{chapter|knowledge|...}: string[]`
    pub fn compat_unmatched_map(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut map = serde_json::Map::new();
        for u in &self.unmatched {
            let key = u.dimension.as_str().to_string();
            let arr = map
                .entry(key)
                .or_insert_with(|| serde_json::json!([]));
            if let Some(a) = arr.as_array_mut() {
                a.push(serde_json::json!(u.raw_name));
            }
        }
        map
    }
}

/// 兼容旧 API / ParsedQuestion.kp_matches 的节点视图
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct KnowledgeNodeMatch {
    pub ai_name: String,
    pub node_id: Uuid,
    pub node_name: String,
    pub tree_id: Uuid,
    pub path: String,
    pub depth: i16,
    pub score: f32,
    pub match_type: String,
}

/// 兼容旧 API 的扁平标签视图
#[derive(Debug, Serialize, sqlx::FromRow, Clone)]
pub struct TagMatch {
    pub ai_name: String,
    pub tag_id: Uuid,
    pub tag_name: String,
    pub category: String,
    pub score: f32,
    pub match_type: String,
}

impl TaggingMatch {
    pub fn to_knowledge_node_match(&self) -> Option<KnowledgeNodeMatch> {
        if self.target_type != TaggingTargetType::KnowledgeNode {
            return None;
        }
        Some(KnowledgeNodeMatch {
            ai_name: self.ai_name.clone(),
            node_id: self.target_id,
            node_name: self.target_name.clone(),
            tree_id: self.tree_id?,
            path: self.path.clone().unwrap_or_default(),
            depth: self.depth.unwrap_or(0),
            score: self.score,
            match_type: self.match_type.as_str().to_string(),
        })
    }

    pub fn to_tag_match(&self) -> Option<TagMatch> {
        if self.target_type != TaggingTargetType::Tag {
            return None;
        }
        Some(TagMatch {
            ai_name: self.ai_name.clone(),
            tag_id: self.target_id,
            tag_name: self.target_name.clone(),
            category: self.category.clone().unwrap_or_default(),
            score: self.score,
            match_type: self.match_type.as_str().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimension_maps_to_target_and_tree() {
        assert_eq!(
            TaggingDimension::Chapter.target_type(),
            TaggingTargetType::KnowledgeNode
        );
        assert_eq!(TaggingDimension::Pattern.tree_kind(), Some("ability"));
        assert!(TaggingDimension::Pattern.leaf_only());
        assert!(!TaggingDimension::Chapter.leaf_only());
        assert_eq!(
            TaggingDimension::Method.target_type(),
            TaggingTargetType::Tag
        );
        assert_eq!(
            TaggingDimension::from_tree_kind("ability"),
            TaggingDimension::Pattern
        );
    }

    #[test]
    fn policy_limits_match_product_spec() {
        let p = TaggingPolicy::default();
        assert_eq!(p.max_selected(TaggingDimension::Chapter), 3);
        assert_eq!(p.max_selected(TaggingDimension::Knowledge), 3);
        assert_eq!(p.max_selected(TaggingDimension::Pattern), 3);
        assert_eq!(p.max_selected(TaggingDimension::Method), 5);
        assert_eq!(p.max_selected(TaggingDimension::CoreCompetence), 3);
        assert_eq!(p.fuzzy_threshold, 0.3);
        assert!(p.fail_on_persist);
        assert!(p.run_llm_extract);
    }

    #[test]
    fn signals_missing_fields_default_empty() {
        let r: TaggingSignals = serde_json::from_str(r#"{"difficulty":3}"#).unwrap();
        assert!(r.chapter_keys.is_empty());
        assert!(r.pattern_keys.is_empty());
        assert_eq!(r.difficulty, Some(3));
    }

    #[test]
    fn five_dim_enums_roundtrip() {
        let dims = [
            TaggingDimension::Chapter,
            TaggingDimension::Knowledge,
            TaggingDimension::Pattern,
            TaggingDimension::Method,
            TaggingDimension::CoreCompetence,
        ];
        for dim in dims {
            let raw = serde_json::to_string(&dim).unwrap();
            let back: TaggingDimension = serde_json::from_str(&raw).unwrap();
            assert_eq!(back, dim);
            assert_eq!(back.as_str(), dim.as_str());
        }
        let m = TaggingMatch {
            dimension: TaggingDimension::CoreCompetence,
            target_type: TaggingTargetType::Tag,
            ai_name: "逻辑推理".into(),
            target_id: Uuid::new_v4(),
            target_name: "逻辑推理".into(),
            tree_id: None,
            path: None,
            depth: None,
            category: Some("core_competence".into()),
            score: 1.0,
            match_type: TaggingMatchType::Exact,
        };
        let v = serde_json::to_value(&m).unwrap();
        let back: TaggingMatch = serde_json::from_value(v).unwrap();
        assert_eq!(back.dimension, TaggingDimension::CoreCompetence);
        assert_eq!(back.match_type, TaggingMatchType::Exact);
    }

    #[test]
    fn compat_unmatched_groups_by_dimension() {
        let s = TaggingSuggestion {
            suggestion_id: None,
            engine_version: ENGINE_VERSION.to_string(),
            input_hash: "x".into(),
            needs_review: true,
            matches: vec![],
            unmatched: vec![TaggingUnmatched {
                id: "1".into(),
                dimension: TaggingDimension::Knowledge,
                target_type: TaggingTargetType::KnowledgeNode,
                raw_name: "未知知识点".into(),
                normalized_name: "未知知识点".into(),
                confidence: None,
                reason: "no_deterministic_match".into(),
                eligible_for_candidate: true,
            }],
            alias_proposals: vec![],
            difficulty: None,
            question_type: None,
            grade_level: None,
            cognitive_level: None,
        };
        let map = s.compat_unmatched_map();
        assert_eq!(
            map.get("knowledge").and_then(|v| v.as_array()).unwrap()[0],
            "未知知识点"
        );
    }
}
