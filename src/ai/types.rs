use serde::{Deserialize, Serialize};

use crate::ai::kp_matcher::KpMatch;

/// 多小题答案单元（解答题）
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SubAnswer {
    /// 小题序号，从 1 开始
    pub sub_id: i32,
    /// 该小题答案，含 $...$ 公式与 ![配图](IMAGE_PLACEHOLDER_N)
    pub content: String,
}

/// 多解法解析单元
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AnalysisMethod {
    /// "解法一" / "解法二"
    pub title: String,
    /// 推导过程，含 $...$ 与 ![配图](IMAGE_PLACEHOLDER_N)
    pub content: String,
}

/// 填空题空位
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BlankAnswer {
    pub position: i32,
    pub answer: String,
}

/// 选择题选项
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ParsedOption {
    pub label: String,
    pub content: String,
}

/// 按题型分支的答案结构
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum ParsedAnswer {
    /// 选择题：["A"] 或 ["A", "C"]
    Choice { options: Vec<String> },
    /// 填空题：[{position: 1, answer: "x"}]
    Fill { blanks: Vec<BlankAnswer> },
    /// 解答题：[{sub_id: 1, content: "..."}]
    Solution { subs: Vec<SubAnswer> },
}

/// AI 解析结果（强制数组化）
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ParsedQuestion {
    /// "choice" | "fill" | "solution"
    pub question_type: String,
    /// 选择题多选时为 "multi"
    pub sub_type: Option<String>,
    /// "easy" | "medium" | "hard"
    pub difficulty: Option<String>,
    /// 题干，含 IMAGE_PLACEHOLDER_N
    pub stem: String,
    /// 选择题选项
    pub options: Option<Vec<ParsedOption>>,
    /// 按题型分支
    pub correct_answer: ParsedAnswer,
    /// 多解法数组（至少 1 个）
    pub analysis: Vec<AnalysisMethod>,
    /// 名称列表，后端做模糊匹配
    pub knowledge_points: Vec<String>,
    /// 0.0-1.0
    pub confidence: f32,
    /// AI 自报警告
    pub warnings: Vec<String>,
    /// ["IMAGE_PLACEHOLDER_0", ...] 便于前端批量替换
    pub image_placeholders: Vec<String>,
    /// 后端知识点模糊匹配结果（非 AI 输出，后端填充）
    #[serde(default)]
    pub kp_matches: Vec<KpMatch>,
}
