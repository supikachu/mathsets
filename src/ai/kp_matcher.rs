//! 知识点匹配结果类型（B3 重构）
//!
//! 旧的 `match_knowledge_points` 函数已删除，
//! 新的匹配逻辑在 `handlers::ai_tagging::match_knowledge_nodes` 中实现，
//! 基于 PostgreSQL pg_trgm + JSONB aliases 做三级模糊匹配（exact/alias/fuzzy），
//! 性能和准确度均优于旧的 Rust 端 Levenshtein 实现。
//!
//! 本文件仅保留 `KpMatch` 结构体，作为 `ParsedQuestion.kp_matches` 字段类型，
//! 供 AI 解析后处理流程填充匹配结果。

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 知识点匹配结果（兼容 ParsedQuestion.kp_matches 字段）
///
/// 注意：新匹配逻辑返回的 `KnowledgeNodeMatch` 字段更丰富（含 tree_id/path/depth/match_type），
/// 此结构仅用于 AI 解析响应的简化视图。完整匹配信息请直接调用 AI 打标 API。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KpMatch {
    /// AI 返回的原始名称
    pub ai_name: String,
    /// 匹配到的知识点节点 ID
    pub matched_id: Option<Uuid>,
    /// 匹配到的知识点名称
    pub matched_name: Option<String>,
    /// 0.0-1.0 相似度
    pub score: f32,
}
