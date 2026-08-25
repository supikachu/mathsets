//! 脚本草稿与 LLM 结果合并。阶段 2 仍每块调用 LLM；脚本是解法/选项安全网。

use crate::ai::types::ParsedQuestion;

use super::validate::{llm_core_ok, strip_options_residue_from_stem};
use super::ScriptDraft;

pub fn script_usable(draft: &ScriptDraft) -> bool {
    draft
        .question
        .options
        .as_ref()
        .is_some_and(|o| o.len() >= 3)
        || draft
            .question
            .analysis
            .iter()
            .any(|a| !a.content.trim().is_empty())
}

pub fn merge_script_and_llm(
    _chunk: &str,
    draft: &ScriptDraft,
    llm_qs: Vec<ParsedQuestion>,
) -> Vec<ParsedQuestion> {
    if llm_qs.is_empty() {
        if script_usable(draft) {
            return vec![tagged_script(draft)];
        }
        return Vec::new();
    }

    let mut out = llm_qs;
    {
        let primary = &mut out[0];
        if !llm_core_ok(primary) {
            *primary = tagged_script(draft);
        } else {
            tag_llm(primary);
            merge_script_warnings(primary, draft);
            restore_script_analysis_on(primary, draft);
        }
        strip_options_residue_from_stem(primary);
    }
    out
}

pub fn restore_script_analysis_if_needed(qs: &mut [ParsedQuestion], draft: &ScriptDraft) {
    if let Some(q) = qs.first_mut() {
        restore_script_analysis_on(q, draft);
    }
}

fn restore_script_analysis_on(q: &mut ParsedQuestion, draft: &ScriptDraft) {
    let llm_n = nonempty_analysis_len(q);
    if draft.method_heading_count > llm_n && !draft.question.analysis.is_empty() {
        q.analysis = draft.question.analysis.clone();
        if !q.warnings.iter().any(|w| w.contains("回填解法")) {
            q.warnings.push("规则结构化：回填解法".into());
        }
    }
}

fn nonempty_analysis_len(q: &ParsedQuestion) -> usize {
    q.analysis
        .iter()
        .filter(|a| !a.content.trim().is_empty())
        .count()
}

fn tagged_script(draft: &ScriptDraft) -> ParsedQuestion {
    let mut q = draft.question.clone();
    push_unique(&mut q.warnings, "规则结构化");
    q
}

fn tag_llm(q: &mut ParsedQuestion) {
    push_unique(&mut q.warnings, "模型补全");
}

fn merge_script_warnings(q: &mut ParsedQuestion, draft: &ScriptDraft) {
    for w in &draft.question.warnings {
        push_unique(&mut q.warnings, w);
    }
    push_unique(&mut q.warnings, "规则结构化");
}

fn push_unique(warnings: &mut Vec<String>, w: &str) {
    if !warnings.iter().any(|x| x == w) {
        warnings.push(w.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::structure::structure_chunk;
    use crate::ai::types::AnalysisMethod;
    use serde_json::json;

    fn parse_q(v: serde_json::Value) -> ParsedQuestion {
        serde_json::from_value(v).expect("ParsedQuestion")
    }

    #[test]
    fn restores_second_method_when_llm_drops_it() {
        let md = "\
16. 已知椭圆 $C$。\n\
【解析】\n\
法一：平移直线，设斜率 $k$。\n\
法二：点差法，由切点弦公式。\n";
        let draft = structure_chunk(md);
        assert_eq!(draft.method_heading_count, 2);
        let llm = parse_q(json!({
            "question_type": "solution",
            "stem": "16. 已知椭圆 $C$。",
            "analysis": [{"title": "解法一", "content": "平移残片"}],
            "parts": []
        }));
        let out = merge_script_and_llm(md, &draft, vec![llm]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].analysis.len(), 2, "{:?}", titles(&out[0].analysis));
        assert!(out[0].analysis[1].content.contains("点差法"));
        assert!(out[0].warnings.iter().any(|w| w.contains("回填解法")));
    }

    #[test]
    fn strips_choice_options_left_in_llm_stem() {
        let md = "\
8. 下列结论正确的是\n\
A. 1\n\
B. 2\n\
C. 3\n\
D. 4\n";
        let draft = structure_chunk(md);
        let llm = parse_q(json!({
            "question_type": "choice",
            "stem": "下列结论正确的是\nA. 1\nB. 2\nC. 3\nD. 4",
            "options": [
                {"label": "A", "content": "1"},
                {"label": "B", "content": "2"},
                {"label": "C", "content": "3"},
                {"label": "D", "content": "4"}
            ],
            "correct_answer": {"kind": "choice", "value": {"options": ["A"]}},
            "analysis": [{"title": "解法一", "content": "故选 A"}]
        }));
        let out = merge_script_and_llm(md, &draft, vec![llm]);
        assert!(!out[0].stem.contains("A."), "残留应被剥掉: {}", out[0].stem);
        assert!(out[0].stem.contains("下列结论正确的是"));
        assert_eq!(out[0].options.as_ref().map(|o| o.len()), Some(4));
        assert!(out[0].warnings.iter().any(|w| w.contains("模型补全")));
    }

    #[test]
    fn prefers_script_when_llm_stem_empty() {
        let md = "\
8. 下列结论正确的是\n\
A. 1\n\
B. 2\n\
C. 3\n\
D. 4\n";
        let draft = structure_chunk(md);
        let llm = parse_q(json!({
            "question_type": "choice",
            "stem": "",
            "options": [],
            "analysis": [{"title": "解法一", "content": ""}]
        }));
        let out = merge_script_and_llm(md, &draft, vec![llm]);
        assert!(out[0].stem.contains("下列结论正确的是"));
        assert_eq!(out[0].options.as_ref().map(|o| o.len()), Some(4));
        assert!(out[0].warnings.iter().any(|w| w.contains("规则结构化")));
    }

    #[test]
    fn empty_llm_uses_script_when_usable() {
        let md = "\
16. 已知椭圆。\n\
【解析】\n\
法一：平移。\n\
法二：点差。\n";
        let draft = structure_chunk(md);
        assert!(script_usable(&draft));
        let out = merge_script_and_llm(md, &draft, vec![]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].analysis.len(), 2);
    }

    fn titles(methods: &[AnalysisMethod]) -> Vec<&str> {
        methods.iter().map(|a| a.title.as_str()).collect()
    }
}
