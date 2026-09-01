//! 解答题题内嵌套结构（`questions.structure` JSONB）
//!
//! 分支节点：`children` 非空，只存局部题干。
//! 叶子节点：`children` 为空，可有答案与多种解法。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 一种解法（导出引擎复用；TS 绑定随 exam.ts 一并导出）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub struct AnalysisBlock {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub content: String,
}

/// 问树节点（递归）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub struct QuestionPart {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub stem: String,
    #[serde(default)]
    pub children: Vec<QuestionPart>,
    #[serde(default)]
    pub answer: Option<String>,
    #[serde(default)]
    pub analyses: Vec<AnalysisBlock>,
    #[serde(default)]
    pub no_analysis_needed: bool,
    /// 为 true 时编号重排不覆盖 label
    #[serde(default, alias = "labelDirty")]
    pub label_dirty: bool,
}

/// 解答题问树
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuestionStructure {
    #[serde(default = "structure_version")]
    pub version: i32,
    #[serde(default)]
    pub parts: Vec<QuestionPart>,
}

fn structure_version() -> i32 {
    1
}

impl QuestionStructure {
    pub fn empty() -> Self {
        Self {
            version: 1,
            parts: vec![],
        }
    }
}

/// 深度优先收集叶子
pub fn walk_leaves(parts: &[QuestionPart]) -> Vec<&QuestionPart> {
    let mut out = Vec::new();
    collect_leaves(parts, &mut out);
    out
}

fn collect_leaves<'a>(parts: &'a [QuestionPart], out: &mut Vec<&'a QuestionPart>) {
    for p in parts {
        if p.children.is_empty() {
            out.push(p);
        } else {
            collect_leaves(&p.children, out);
        }
    }
}

pub fn parse_structure(value: Option<&serde_json::Value>) -> Option<QuestionStructure> {
    let v = value?;
    if v.is_null() {
        return None;
    }
    serde_json::from_value(v.clone()).ok()
}

fn answer_blank(s: Option<&str>) -> bool {
    s.map_or(true, |t| t.trim().is_empty())
}

fn analyses_blank(part: &QuestionPart) -> bool {
    part.analyses
        .iter()
        .all(|a| a.content.trim().is_empty())
}

/// 无叶子，或任一叶子答案为空
pub fn is_solution_answer_empty(structure: Option<&QuestionStructure>) -> bool {
    let Some(s) = structure else {
        return true;
    };
    let leaves = walk_leaves(&s.parts);
    if leaves.is_empty() {
        return true;
    }
    leaves.iter().any(|p| answer_blank(p.answer.as_deref()))
}

/// 任一叶子未勾选「无需解析」且解法全空
pub fn is_solution_analysis_missing(structure: Option<&QuestionStructure>) -> bool {
    let Some(s) = structure else {
        return true;
    };
    let leaves = walk_leaves(&s.parts);
    if leaves.is_empty() {
        return true;
    }
    leaves
        .iter()
        .any(|p| !p.no_analysis_needed && analyses_blank(p))
}

/// 全部叶子都勾选「无需解析」（且至少一叶）
pub fn all_leaves_skip_analysis(structure: Option<&QuestionStructure>) -> bool {
    let Some(s) = structure else {
        return false;
    };
    let leaves = walk_leaves(&s.parts);
    !leaves.is_empty() && leaves.iter().all(|p| p.no_analysis_needed)
}

/// 把 structure JSON 中所有字符串拼起来，供配图 URL 扫描
pub fn structure_text_blobs(value: Option<&serde_json::Value>) -> String {
    let mut out = String::new();
    if let Some(v) = value {
        collect_strings(v, &mut out);
    }
    out
}

fn collect_strings(v: &serde_json::Value, out: &mut String) {
    match v {
        serde_json::Value::String(s) => {
            out.push_str(s);
            out.push('\n');
        }
        serde_json::Value::Array(arr) => {
            for x in arr {
                collect_strings(x, out);
            }
        }
        serde_json::Value::Object(map) => {
            for x in map.values() {
                collect_strings(x, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(answer: &str, analysis: &str, skip: bool) -> QuestionPart {
        QuestionPart {
            id: uuid::Uuid::new_v4().to_string(),
            label: "(1)".into(),
            stem: String::new(),
            children: vec![],
            answer: Some(answer.into()),
            analyses: if analysis.is_empty() {
                vec![]
            } else {
                vec![AnalysisBlock {
                    id: "a1".into(),
                    title: "解法一".into(),
                    content: analysis.into(),
                }]
            },
            no_analysis_needed: skip,
            label_dirty: false,
        }
    }

    #[test]
    fn walk_nested_leaves() {
        let tree = QuestionStructure {
            version: 1,
            parts: vec![
                QuestionPart {
                    id: "I".into(),
                    label: "I".into(),
                    stem: "若为奇函数".into(),
                    children: vec![leaf("m=-1", "证奇", false), leaf("a>0", "不等式", false)],
                    answer: None,
                    analyses: vec![],
                    no_analysis_needed: false,
                    label_dirty: false,
                },
                leaf("p(m)=...", "绝对值", false),
            ],
        };
        assert_eq!(walk_leaves(&tree.parts).len(), 3);
        assert!(!is_solution_answer_empty(Some(&tree)));
        assert!(!is_solution_analysis_missing(Some(&tree)));
    }

    #[test]
    fn empty_structure_pending() {
        assert!(is_solution_answer_empty(None));
        assert!(is_solution_answer_empty(Some(&QuestionStructure::empty())));
        assert!(is_solution_analysis_missing(None));
    }

    #[test]
    fn skip_analysis_on_leaf() {
        let s = QuestionStructure {
            version: 1,
            parts: vec![leaf("2", "", true)],
        };
        assert!(!is_solution_answer_empty(Some(&s)));
        assert!(!is_solution_analysis_missing(Some(&s)));
        assert!(all_leaves_skip_analysis(Some(&s)));
    }
}
