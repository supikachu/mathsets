//! 统一智能打标引擎
//!
//! 编辑页题文与 AI 录题解析结果共用同一套五维召回 / 收敛 / 建议协议。
//! 建议只在用户确认保存后才会写入题目关联和 tag_candidates；唯一例外是
//! 打标晚于保存完成时的兜底认领（`claim_suggestion_for_saved_question`），
//! 它只补写匹配项，未匹配项仍需教师确认。

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
    apply_tagging_suggestion, claim_suggestion_for_saved_question, confirmation_or_legacy,
    insert_confirmed_candidates, repair_applied_suggestion_links,
    AiTaggingConfirmation, AliasMapItem, PendingCandidate,
};
pub use types::{
    KnowledgeNodeMatch, TagMatch, TaggingContext, TaggingDimension, TaggingInput,
    TaggingMatch, TaggingPolicy, TaggingSignals, TaggingSuggestion, TaggingTargetType,
    ENGINE_VERSION,
};
pub use vector::{cap_vector_score, merge_node_candidate, VECTOR_SCORE_CAP};
