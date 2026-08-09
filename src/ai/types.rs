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

impl ParsedAnswer {
    /// 按题型生成空答案默认值（当 LLM 输出 `null` 或缺失答案时使用）
    ///
    /// - `choice` / `multiple` → `Choice { options: [] }`
    /// - `fill` → `Fill { blanks: [] }`
    /// - 其他 → `Solution { subs: [] }`
    pub fn empty_for_type(question_type: &str) -> Self {
        match question_type {
            "choice" | "multiple" => ParsedAnswer::Choice { options: vec![] },
            "fill" => ParsedAnswer::Fill { blanks: vec![] },
            _ => ParsedAnswer::Solution { subs: vec![] },
        }
    }
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
    /// 按题型分支（`Option` 容错：LLM 可能输出 `null` 表示无答案，后端补默认空结构）
    #[serde(default)]
    pub correct_answer: Option<ParsedAnswer>,
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
    /// v1.1：从 Markdown 中提取的所有图片 URL（去重）
    ///
    /// 当 Stage 1（Doc2X / MinerU）输出含 `![...](url)` 真实链接时，
    /// Stage 2 收集去重后填入此数组，前端据内联标记绑定原图，避免几何题丢图。
    /// Qwen-VL 路径仅有 IMAGE_PLACEHOLDER_N，此数组为空（向后兼容）。
    #[serde(default)]
    pub image_urls: Vec<String>,
    /// 后端知识点模糊匹配结果（非 AI 输出，后端填充）
    #[serde(default)]
    pub kp_matches: Vec<KpMatch>,
}

/// 批量解析响应 — LLM 批量输出用 {"questions": [...]} 包裹
/// 注意：实际批量解析走 serde_json::Value 逐题隔离（补丁十防连坐），
/// 此结构仅用于类型参考和文档。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BatchParseResponse {
    pub questions: Vec<ParsedQuestion>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_correct_answer_null_deserializes_to_none() {
        // LLM 输出 "correct_answer": null 时不应崩溃，应反序列化为 None
        let raw = serde_json::json!({
            "question_type": "solution",
            "stem": "求 x",
            "correct_answer": null,
            "analysis": [],
            "knowledge_points": [],
            "confidence": 0.6,
            "warnings": [],
            "image_placeholders": []
        });
        let q: ParsedQuestion = serde_json::from_value(raw).unwrap();
        assert!(q.correct_answer.is_none());
    }

    #[test]
    fn test_correct_answer_missing_defaults_to_none() {
        // correct_answer 字段缺失时也应反序列化为 None（#[serde(default)]）
        let raw = serde_json::json!({
            "question_type": "choice",
            "stem": "1+1=?",
            "analysis": [],
            "knowledge_points": [],
            "confidence": 0.5,
            "warnings": [],
            "image_placeholders": []
        });
        let q: ParsedQuestion = serde_json::from_value(raw).unwrap();
        assert!(q.correct_answer.is_none());
    }

    #[test]
    fn test_correct_answer_present_deserializes_to_some() {
        let raw = serde_json::json!({
            "question_type": "choice",
            "stem": "1+1=?",
            "correct_answer": {"kind": "choice", "value": {"options": ["B"]}},
            "analysis": [],
            "knowledge_points": [],
            "confidence": 0.9,
            "warnings": [],
            "image_placeholders": []
        });
        let q: ParsedQuestion = serde_json::from_value(raw).unwrap();
        let ans = q.correct_answer.unwrap();
        assert!(matches!(ans, ParsedAnswer::Choice { options } if options == vec!["B".to_string()]));
    }

    #[test]
    fn test_empty_for_type_choice() {
        let a = ParsedAnswer::empty_for_type("choice");
        assert!(matches!(a, ParsedAnswer::Choice { options } if options.is_empty()));
    }

    #[test]
    fn test_empty_for_type_multiple() {
        let a = ParsedAnswer::empty_for_type("multiple");
        assert!(matches!(a, ParsedAnswer::Choice { options } if options.is_empty()));
    }

    #[test]
    fn test_empty_for_type_fill() {
        let a = ParsedAnswer::empty_for_type("fill");
        assert!(matches!(a, ParsedAnswer::Fill { blanks } if blanks.is_empty()));
    }

    #[test]
    fn test_empty_for_type_solution() {
        let a = ParsedAnswer::empty_for_type("solution");
        assert!(matches!(a, ParsedAnswer::Solution { subs } if subs.is_empty()));
    }
}
