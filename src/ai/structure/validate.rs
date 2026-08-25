//! 合并后校验：schema、解法项数、题干选项残留。

use std::sync::LazyLock;

use regex::Regex;

use crate::ai::types::ParsedQuestion;

use super::{Confidence, ScriptDraft};

static OPTIONS_RESIDUE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?is)(?:^|\n|[。；;！？$）)])\s*\$?\s*A[\.、．\)]\s*.*?B[\.、．\)]\s*.*?C[\.、．\)]\s*.*?D[\.、．\)]\s*.*$",
    )
    .expect("选项残留正则编译失败")
});

const VALID_TYPES: &[&str] = &["choice", "fill", "solution", "multiple"];

#[derive(Debug, Clone, Default)]
pub struct StructuredValidation {
    pub schema_ok: bool,
    pub method_count_mismatch: bool,
    pub issues: Vec<String>,
}

pub fn validate_structured(
    q: &ParsedQuestion,
    method_heading_count: usize,
    confidence: Confidence,
) -> StructuredValidation {
    let mut issues = Vec::new();

    if !VALID_TYPES.contains(&q.question_type.as_str()) {
        issues.push(format!("题型无效: {}", q.question_type));
    }
    if q.stem.trim().is_empty() && !q.has_visible_body() {
        issues.push("题干为空".into());
    }

    if matches!(confidence, Confidence::High)
        && matches!(q.question_type.as_str(), "choice" | "multiple")
    {
        let n = q.options.as_ref().map(|o| o.len()).unwrap_or(0);
        if n != 4 {
            issues.push(format!("High 选择题必须 4 个选项，实际 {n}"));
        }
    }

    let analysis_n = q.analysis.len();
    let method_count_mismatch = method_heading_count != analysis_n;
    if method_count_mismatch {
        issues.push(format!(
            "解法标题 {method_heading_count} 项，analysis {analysis_n} 项"
        ));
    }

    let schema_ok = !issues.iter().any(|i| {
        i.starts_with("题型无效") || i.starts_with("题干为空") || i.starts_with("High 选择题")
    });

    StructuredValidation {
        schema_ok,
        method_count_mismatch,
        issues,
    }
}

pub fn llm_core_ok(q: &ParsedQuestion) -> bool {
    let report = validate_structured(q, q.analysis.len(), Confidence::Low);
    if !report.schema_ok {
        return false;
    }
    if matches!(q.question_type.as_str(), "choice" | "multiple") {
        return q.options.as_ref().is_some_and(|o| !o.is_empty());
    }
    true
}

/// 第二道防线：剥离选择题题干末尾的 A–D 残留。
pub fn strip_options_residue_from_stem(q: &mut ParsedQuestion) {
    if !matches!(q.question_type.as_str(), "choice" | "multiple") {
        return;
    }
    let has_options = q.options.as_ref().is_some_and(|o| !o.is_empty());
    if !has_options {
        return;
    }
    if let Some(m) = OPTIONS_RESIDUE_RE.find(&q.stem) {
        let mut cut = m.start();
        if let Some(ch) = q.stem[cut..].chars().next() {
            if matches!(ch, '$' | '）' | ')') {
                cut += ch.len_utf8();
                if ch == ')' && q.stem[cut..].starts_with('$') {
                    cut += 1;
                }
            }
        }
        let mut new_stem = q.stem[..cut].trim_end().to_string();
        if new_stem.ends_with('$') && new_stem.matches('$').count() % 2 == 1 {
            new_stem.pop();
            new_stem = new_stem.trim_end().to_string();
        }
        if new_stem != q.stem {
            tracing::info!(
                "剥离题干选项残留：{} 字符 → {} 字符",
                q.stem.chars().count(),
                new_stem.chars().count()
            );
            q.warnings.push("已自动剥离题干中残留的选项文本".into());
            q.stem = new_stem;
        }
    }
}

pub fn append_validation_warnings(q: &mut ParsedQuestion, draft: &ScriptDraft) {
    let report = validate_structured(q, draft.method_heading_count, draft.confidence);
    for issue in report.issues {
        if !q.warnings.iter().any(|w| w == &issue) {
            q.warnings.push(issue);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::types::{AnalysisMethod, ParsedAnswer, ParsedOption};

    fn choice(stem: &str, n_opts: usize) -> ParsedQuestion {
        let opts: Vec<ParsedOption> = (0..n_opts)
            .map(|i| ParsedOption {
                label: ((b'A' + i as u8) as char).to_string(),
                content: (i + 1).to_string(),
            })
            .collect();
        ParsedQuestion {
            question_type: "choice".into(),
            sub_type: None,
            difficulty: None,
            stem: stem.into(),
            options: Some(opts),
            correct_answer: Some(ParsedAnswer::empty_for_type("choice")),
            analysis: vec![AnalysisMethod {
                title: "解法一".into(),
                content: String::new(),
            }],
            knowledge_points: vec![],
            confidence: 0.9,
            warnings: vec![],
            image_placeholders: vec![],
            image_urls: vec![],
            kp_matches: vec![],
            parts: vec![],
            question_no: None,
            display_order: None,
            score: None,
            chapter_path: vec![],
            solution_methods: vec![],
        }
    }

    #[test]
    fn high_choice_requires_four_options() {
        let q = choice("下列结论正确的是", 3);
        let report = validate_structured(&q, 1, Confidence::High);
        assert!(!report.schema_ok);
        let report = validate_structured(&q, 2, Confidence::Low);
        assert!(report.schema_ok);
        assert!(report.method_count_mismatch);
    }

    #[test]
    fn strips_multiline_options_residue() {
        let mut q = choice("下列结论正确的是\nA. 1\nB. 2\nC. 3\nD. 4", 4);
        strip_options_residue_from_stem(&mut q);
        assert_eq!(q.stem, "下列结论正确的是");
        assert!(q.warnings.iter().any(|w| w.contains("剥离")));
    }
}
