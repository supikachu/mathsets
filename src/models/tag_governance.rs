// ===========================================================================
// V2.1.1 P1：标签治理数据模型（tag_candidates / tag_merge_records）
// ===========================================================================

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// 标签候选（数据库行）
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TagCandidate {
    pub id: Uuid,
    /// chapter / knowledge / method
    pub kind: String,
    /// AI 原始标签
    pub raw_name: String,
    /// 规范化标签（去重键之一）
    pub normalized_name: String,
    pub suggested_node_id: Option<Uuid>,
    pub ai_confidence: Option<rust_decimal::Decimal>,
    pub match_score: Option<rust_decimal::Decimal>,
    pub source_task_id: Option<Uuid>,
    pub source_question_id: Option<Uuid>,
    /// pending / approved / rejected / merged
    pub status: String,
    pub reviewed_by: Option<Uuid>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// 候选列表查询
#[derive(Debug, Deserialize)]
pub struct TagCandidateQuery {
    pub status: Option<String>,
    pub kind: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

/// 候选审核请求（approve 四分支）
#[derive(Debug, Deserialize)]
pub struct ApproveCandidateRequest {
    /// new_node（接受为新标签）/ alias（作为已有标签的别名）/ merge（并入已有标签）
    pub action: String,
    /// new_node 分支：目标树
    pub tree_id: Option<Uuid>,
    /// new_node 分支：父节点（可选，缺省为树根）
    pub parent_id: Option<Uuid>,
    /// new_node 分支：节点名（缺省用 raw_name）
    pub name: Option<String>,
    /// alias / merge 分支：目标已有标签
    pub target_node_id: Option<Uuid>,
    /// 审核备注
    pub reason: Option<String>,
}

/// 候选拒绝请求
#[derive(Debug, Deserialize)]
pub struct RejectCandidateRequest {
    pub reason: Option<String>,
}

/// 知识点合并请求（POST /knowledge-nodes/{id}/merge）
#[derive(Debug, Deserialize)]
pub struct MergeKnowledgeNodeRequest {
    pub target_id: Uuid,
    pub reason: Option<String>,
}
