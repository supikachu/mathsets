//! 统一智能打标引擎
//!
//! 编辑页题文与 AI 录题解析结果共用同一套五维召回 / 收敛 / 建议协议。
//! 建议只在用户确认保存后才会写入题目关联和 tag_candidates。

pub mod engine;
pub mod finalize;
pub mod persist;
pub mod prompts;
pub mod repository;
pub mod shadow;
pub mod types;
pub mod vector;

pub use engine::{
    content_input_hash, content_input_hash_with_stage, run_tagging, signals_from_parsed,
    tagging_content_from_parsed, TaggingError,
};
pub use finalize::{
    apply_tagging_suggestion, confirmation_or_legacy, insert_confirmed_candidates,
    AiTaggingConfirmation, AliasMapItem, PendingCandidate,
};
pub use types::{
    KnowledgeNodeMatch, TagMatch, TaggingContext, TaggingDimension, TaggingInput,
    TaggingMatch, TaggingPolicy, TaggingSuggestion, TaggingTargetType, ENGINE_VERSION,
};
pub use vector::{cap_vector_score, merge_node_candidate, VECTOR_SCORE_CAP};
