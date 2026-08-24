use serde::{Deserialize, Deserializer, Serialize};

use crate::ai::kp_matcher::KpMatch;

/// LLM 常把字符串/数组写成 JSON `null`；按空值接收，避免整题被丢弃。
mod llm_null {
    use super::*;

    pub fn as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de> + Default,
    {
        Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
    }

    pub fn vec_skip_null<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        let raw: Option<Vec<Option<T>>> = Option::deserialize(deserializer)?;
        Ok(raw
            .unwrap_or_default()
            .into_iter()
            .flatten()
            .collect())
    }

    pub fn opt_vec_skip_null<'de, D, T>(deserializer: D) -> Result<Option<Vec<T>>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        let raw: Option<Vec<Option<T>>> = Option::deserialize(deserializer)?;
        Ok(raw.map(|arr| arr.into_iter().flatten().collect()))
    }

    /// 题号可能是 `"14"` 或 JSON 数字 `14`。
    pub fn opt_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = Option::<serde_json::Value>::deserialize(deserializer)?;
        Ok(match v {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(s)) => {
                let t = s.trim();
                if t.is_empty() {
                    None
                } else {
                    Some(t.to_string())
                }
            }
            Some(serde_json::Value::Number(n)) => Some(n.to_string()),
            Some(other) => other.as_str().map(str::to_string),
        })
    }
}

/// 多小题答案单元（解答题）
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SubAnswer {
    /// 小题序号，从 1 开始
    #[serde(default, alias = "id")]
    pub sub_id: i32,
    /// 该小题答案，含 $...$ 公式与 ![配图](IMAGE_PLACEHOLDER_N)
    #[serde(default, deserialize_with = "llm_null::as_default")]
    pub content: String,
}

/// 多解法解析单元
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AnalysisMethod {
    /// "解法一" / "解法二"
    #[serde(default, deserialize_with = "llm_null::as_default")]
    pub title: String,
    /// 推导过程，含 $...$ 与 ![配图](IMAGE_PLACEHOLDER_N)
    #[serde(default, deserialize_with = "llm_null::as_default")]
    pub content: String,
}

/// V2.1.1 通用解题方法（AI 输出；匹配 tags.category=method，不匹配专题树）
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SolutionMethod {
    #[serde(default, deserialize_with = "llm_null::as_default")]
    pub name: String,
    #[serde(default)]
    pub confidence: Option<f32>,
}

/// 填空题空位
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BlankAnswer {
    #[serde(default)]
    pub position: i32,
    #[serde(default, deserialize_with = "llm_null::as_default")]
    pub answer: String,
}

/// 选择题选项
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ParsedOption {
    #[serde(default, deserialize_with = "llm_null::as_default")]
    pub label: String,
    #[serde(default, deserialize_with = "llm_null::as_default")]
    pub content: String,
}

/// 按题型分支的答案结构
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum ParsedAnswer {
    /// 选择题：["A"] 或 ["A", "C"]
    Choice {
        #[serde(default, deserialize_with = "llm_null::vec_skip_null")]
        options: Vec<String>,
    },
    /// 填空题：[{position: 1, answer: "x"}]
    Fill {
        #[serde(default, deserialize_with = "llm_null::vec_skip_null")]
        blanks: Vec<BlankAnswer>,
    },
    /// 解答题：[{sub_id: 1, content: "..."}]
    Solution {
        #[serde(default, deserialize_with = "llm_null::vec_skip_null")]
        subs: Vec<SubAnswer>,
    },
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

/// 解答题问树节点（AI 输出；与入库 `questions.structure.parts` 对齐）
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ParsedPart {
    #[serde(default, deserialize_with = "llm_null::as_default")]
    pub id: String,
    #[serde(default, deserialize_with = "llm_null::as_default")]
    pub label: String,
    #[serde(default, deserialize_with = "llm_null::as_default")]
    pub stem: String,
    #[serde(default, deserialize_with = "llm_null::vec_skip_null")]
    pub children: Vec<ParsedPart>,
    #[serde(default)]
    pub answer: Option<String>,
    #[serde(default, deserialize_with = "llm_null::vec_skip_null")]
    pub analyses: Vec<AnalysisMethod>,
    #[serde(default)]
    pub no_analysis_needed: bool,
}

/// AI 解析结果（强制数组化）
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ParsedQuestion {
    /// "choice" | "fill" | "solution"
    #[serde(default, alias = "type", deserialize_with = "llm_null::as_default")]
    pub question_type: String,
    /// 选择题多选时为 "multi"
    pub sub_type: Option<String>,
    /// "easy" | "medium" | "hard"
    pub difficulty: Option<String>,
    /// 题干，含 IMAGE_PLACEHOLDER_N
    #[serde(default, deserialize_with = "llm_null::as_default")]
    pub stem: String,
    /// 选择题选项
    #[serde(default, deserialize_with = "llm_null::opt_vec_skip_null")]
    pub options: Option<Vec<ParsedOption>>,
    /// 按题型分支（`Option` 容错：LLM 可能输出 `null` 表示无答案，后端补默认空结构）
    #[serde(default)]
    pub correct_answer: Option<ParsedAnswer>,
    /// 多解法数组（选择题/填空题整题解析；解答题应放到叶子 `parts[].analyses`）
    #[serde(default, deserialize_with = "llm_null::vec_skip_null")]
    pub analysis: Vec<AnalysisMethod>,
    /// 解答题问树。其它题型为空。缺失时 worker 从 subs+analysis 合成一层叶子。
    #[serde(default, deserialize_with = "llm_null::vec_skip_null")]
    pub parts: Vec<ParsedPart>,
    /// 名称列表，后端做模糊匹配
    #[serde(default, deserialize_with = "llm_null::vec_skip_null")]
    pub knowledge_points: Vec<String>,
    /// 0.0-1.0
    #[serde(default, deserialize_with = "llm_null::as_default")]
    pub confidence: f32,
    /// AI 自报警告
    #[serde(default, deserialize_with = "llm_null::vec_skip_null")]
    pub warnings: Vec<String>,
    /// ["IMAGE_PLACEHOLDER_0", ...] 便于前端批量替换
    #[serde(default, deserialize_with = "llm_null::vec_skip_null")]
    pub image_placeholders: Vec<String>,
    /// v1.1：从 Markdown 中提取的所有图片 URL（去重）
    ///
    /// 当 Stage 1（Doc2X / MinerU）输出含 `![...](url)` 真实链接时，
    /// Stage 2 收集去重后填入此数组，前端据内联标记绑定原图，避免几何题丢图。
    /// Qwen-VL 路径仅有 IMAGE_PLACEHOLDER_N，此数组为空（向后兼容）。
    #[serde(default, deserialize_with = "llm_null::vec_skip_null")]
    pub image_urls: Vec<String>,
    /// 后端知识点模糊匹配结果（非 AI 输出，后端填充）
    #[serde(default)]
    pub kp_matches: Vec<KpMatch>,

    // ── V2.1.1 批量录题扩展（全部可选，向后兼容） ──
    /// 题号（如 "17(2)" / "1" / "一、1"），只属于 PaperQuestion/CollectionQuestion
    #[serde(default, deserialize_with = "llm_null::opt_string")]
    pub question_no: Option<String>,
    /// 展示顺序（缺省由 Worker 按序编号）
    #[serde(default)]
    pub display_order: Option<i32>,
    /// 分值（如 8）
    #[serde(default)]
    pub score: Option<i32>,
    /// 章节路径（如 ["高中数学", "函数", "导数"]）
    #[serde(default, deserialize_with = "llm_null::vec_skip_null")]
    pub chapter_path: Vec<String>,
    /// 解题方法（通用方法/数学思想，如 [{"name": "数形结合", "confidence": 0.91}]）
    /// 不匹配题型专题树；编辑页由 tags.category=method 承载
    #[serde(default, deserialize_with = "llm_null::vec_skip_null")]
    pub solution_methods: Vec<SolutionMethod>,
}

impl ParsedQuestion {
    /// 对 stem / options / analysis / parts 调用 `close_unclosed_img_row_fences`
    pub fn sanitize_img_row_fences(&mut self) {
        self.visit_strings_mut(|s| {
            *s = crate::ai::cleaner::close_unclosed_img_row_fences(s);
        });
    }

    /// 清洗字面量 `\n` 与误输出的 HTML 表格
    pub fn sanitize_text_markup(&mut self) {
        self.visit_strings_mut(|s| {
            *s = crate::ai::cleaner::sanitize_question_markup(s);
        });
    }

    pub fn visit_strings_mut<F: FnMut(&mut String)>(&mut self, mut f: F) {
        f(&mut self.stem);
        if let Some(opts) = self.options.as_mut() {
            for o in opts {
                f(&mut o.content);
            }
        }
        for a in &mut self.analysis {
            f(&mut a.content);
        }
        fn walk_parts<F: FnMut(&mut String)>(nodes: &mut [ParsedPart], f: &mut F) {
            for p in nodes {
                f(&mut p.stem);
                if let Some(ans) = p.answer.as_mut() {
                    f(ans);
                }
                for a in &mut p.analyses {
                    f(&mut a.content);
                }
                walk_parts(&mut p.children, f);
            }
        }
        walk_parts(&mut self.parts, &mut f);
    }

    pub fn visit_strings<F: FnMut(&str)>(&self, mut f: F) {
        f(&self.stem);
        if let Some(opts) = &self.options {
            for o in opts {
                f(&o.content);
            }
        }
        for a in &self.analysis {
            f(&a.content);
        }
        fn walk_parts<F: FnMut(&str)>(nodes: &[ParsedPart], f: &mut F) {
            for p in nodes {
                f(&p.stem);
                if let Some(ans) = &p.answer {
                    f(ans);
                }
                for a in &p.analyses {
                    f(&a.content);
                }
                walk_parts(&p.children, f);
            }
        }
        walk_parts(&self.parts, &mut f);
    }

    /// 题干或问树里是否有可展示正文（解答题总前提可空，小问在 parts 里）
    pub fn has_visible_body(&self) -> bool {
        if !self.stem.trim().is_empty() {
            return true;
        }
        fn walk(parts: &[ParsedPart]) -> bool {
            parts.iter().any(|p| {
                !p.stem.trim().is_empty()
                    || p.answer.as_deref().is_some_and(|s| !s.trim().is_empty())
                    || p.analyses.iter().any(|a| !a.content.trim().is_empty())
                    || walk(&p.children)
            })
        }
        walk(&self.parts)
    }

    /// 解答题若未输出 parts，从扁平 subs + 整题 analysis 合成一层叶子。
    pub fn ensure_solution_parts(&mut self) {
        if self.question_type != "solution" {
            return;
        }
        if !self.parts.is_empty() {
            assign_part_ids(&mut self.parts);
            return;
        }
        let subs = match &self.correct_answer {
            Some(ParsedAnswer::Solution { subs }) if !subs.is_empty() => subs.clone(),
            _ => vec![SubAnswer {
                sub_id: 1,
                content: String::new(),
            }],
        };
        let analyses = self.analysis.clone();
        self.parts = subs
            .into_iter()
            .enumerate()
            .map(|(i, sub)| ParsedPart {
                id: uuid::Uuid::new_v4().to_string(),
                label: format!("({})", if sub.sub_id > 0 { sub.sub_id } else { (i + 1) as i32 }),
                stem: String::new(),
                children: vec![],
                answer: Some(sub.content),
                analyses: if i == 0 { analyses.clone() } else { vec![] },
                no_analysis_needed: false,
            })
            .collect();
    }

    /// 解答题问树 → 入库 JSON（version + parts）
    pub fn structure_json(&self) -> Option<serde_json::Value> {
        if self.question_type != "solution" || self.parts.is_empty() {
            return None;
        }
        serde_json::to_value(serde_json::json!({
            "version": 1,
            "parts": self.parts,
        }))
        .ok()
    }
}

fn assign_part_ids(parts: &mut [ParsedPart]) {
    for p in parts {
        if p.id.trim().is_empty() {
            p.id = uuid::Uuid::new_v4().to_string();
        }
        assign_part_ids(&mut p.children);
    }
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

    #[test]
    fn test_null_strings_in_analysis_and_knowledge_do_not_drop_question() {
        let raw = serde_json::json!({
            "question_type": "solution",
            "stem": "已知椭圆",
            "analysis": [{"title": "解法一", "content": null}],
            "knowledge_points": ["椭圆", null],
            "confidence": null,
            "warnings": null,
            "image_placeholders": null
        });
        let q: ParsedQuestion = serde_json::from_value(raw).unwrap();
        assert_eq!(q.stem, "已知椭圆");
        assert_eq!(q.analysis.len(), 1);
        assert_eq!(q.analysis[0].title, "解法一");
        assert_eq!(q.analysis[0].content, "");
        assert_eq!(q.knowledge_points, vec!["椭圆".to_string()]);
        assert_eq!(q.confidence, 0.0);
        assert!(q.warnings.is_empty());
    }

    #[test]
    fn test_null_option_content_deserializes() {
        let raw = serde_json::json!({
            "question_type": "choice",
            "stem": "下列正确的是",
            "options": [{"label": "A", "content": null}, null],
            "analysis": [],
            "knowledge_points": [],
            "confidence": 0.5,
            "warnings": [],
            "image_placeholders": []
        });
        let q: ParsedQuestion = serde_json::from_value(raw).unwrap();
        let opts = q.options.unwrap();
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].label, "A");
        assert_eq!(opts[0].content, "");
    }

    #[test]
    fn test_doubao_shaped_question_deserializes() {
        let raw = serde_json::json!({
            "question_type": "choice",
            "sub_type": "multi",
            "difficulty": null,
            "stem": "曲线 C 过原点",
            "options": [
                {"label": "A", "content": "$a=-2$"},
                {"label": "B", "content": "点在 C 上"}
            ],
            "correct_answer": {"kind": "choice", "value": {"options": ["A", "B"]}},
            "analysis": [{"title": "解析", "content": "代入原点"}],
            "knowledge_points": ["曲线与方程"],
            "confidence": 1.0,
            "warnings": [],
            "image_placeholders": [],
            "image_urls": [],
            "question_no": "11",
            "display_order": 1,
            "chapter_path": ["解析几何"],
            "solution_methods": [{"name": "特例法", "confidence": 0.8}]
        });
        let q: ParsedQuestion = serde_json::from_value(raw).expect("豆包 Stage2 JSON 应能反序列化");
        assert_eq!(q.question_type, "choice");
        assert_eq!(q.sub_type.as_deref(), Some("multi"));
        assert_eq!(q.question_no.as_deref(), Some("11"));
        assert!(matches!(
            q.correct_answer,
            Some(ParsedAnswer::Choice { ref options }) if options == &["A".to_string(), "B".to_string()]
        ));
    }

    #[test]
    fn test_question_no_accepts_json_number() {
        let raw = serde_json::json!({
            "question_type": "fill",
            "stem": "填空",
            "analysis": [],
            "question_no": 14
        });
        let q: ParsedQuestion = serde_json::from_value(raw).expect("题号数字应能反序列化");
        assert_eq!(q.question_no.as_deref(), Some("14"));
    }

    #[test]
    fn test_parts_deserialize_and_ensure_solution_parts() {
        let raw = serde_json::json!({
            "question_type": "solution",
            "stem": "总前提",
            "parts": [{
                "label": "(1)",
                "stem": "求 x",
                "answer": "1",
                "analyses": [{"title": "解法一", "content": "代入"}]
            }]
        });
        let q: ParsedQuestion = serde_json::from_value(raw).unwrap();
        assert_eq!(q.parts.len(), 1);
        assert_eq!(q.parts[0].answer.as_deref(), Some("1"));

        let mut q2 = ParsedQuestion {
            question_type: "solution".into(),
            sub_type: None,
            difficulty: None,
            stem: "总前提".into(),
            options: None,
            correct_answer: Some(ParsedAnswer::Solution {
                subs: vec![SubAnswer { sub_id: 1, content: "2".into() }],
            }),
            analysis: vec![AnalysisMethod { title: "解法一".into(), content: "算".into() }],
            parts: vec![],
            knowledge_points: vec![],
            confidence: 0.8,
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
        q2.ensure_solution_parts();
        assert_eq!(q2.parts.len(), 1);
        assert_eq!(q2.parts[0].answer.as_deref(), Some("2"));
        assert_eq!(q2.parts[0].analyses[0].content, "算");
        assert!(q2.structure_json().is_some());
    }

    #[test]
    fn test_has_visible_body_allows_empty_stem_with_parts() {
        let q: ParsedQuestion = serde_json::from_value(serde_json::json!({
            "question_type": "solution",
            "stem": "",
            "parts": [{
                "label": "(1)",
                "stem": "求最小值",
                "children": [],
                "answer": "",
                "analyses": []
            }]
        }))
        .unwrap();
        assert!(q.has_visible_body());
        let empty: ParsedQuestion = serde_json::from_value(serde_json::json!({
            "question_type": "solution",
            "stem": "   ",
            "parts": []
        }))
        .unwrap();
        assert!(!empty.has_visible_body());
    }
}
