//! 保守门控：误判 High 比多打一次 LLM 更糟。

use crate::ai::types::ParsedQuestion;

use super::analysis::{count_method_headings, looks_like_analysis_chunk};
use super::options::{extract_choice_options, stem_has_option_residue};
use super::validate::validate_structured;
use super::{Confidence, ScriptDraft};

pub(crate) const MARKDOWN_FALLBACK_LOW_CHARS: usize = 6000;

pub(crate) fn evaluate(
    draft: &mut ScriptDraft,
    source_chunk: &str,
    major_count: usize,
) {
    let mut reasons = Vec::new();
    let mut high = true;

    if major_count != 1 {
        high = false;
        reasons.push(if major_count == 0 {
            "块内没有大题题号".into()
        } else {
            format!("块内有 {major_count} 个大题题号")
        });
    }

    if source_chunk.chars().count() >= MARKDOWN_FALLBACK_LOW_CHARS {
        high = false;
        reasons.push("块过大，疑似 markdown 大段回退".into());
    }

    let blob = fields_blob(&draft.question);
    for url in &draft.image_urls_in_chunk {
        if !blob.contains(url.as_str()) {
            high = false;
            reasons.push(format!("配图未进入题干/选项/解析: {url}"));
        }
    }

    match draft.question.question_type.as_str() {
        "choice" | "multiple" => {
            let n = draft.question.options.as_ref().map(|o| o.len()).unwrap_or(0);
            if n != 4 {
                high = false;
                reasons.push(format!("选择题选项数为 {n}，不是连续 A–D"));
            }
            if stem_has_option_residue(&draft.question.stem) {
                high = false;
                reasons.push("切开后题干仍残留 A./A、".into());
            }
        }
        _ => {
            if extract_choice_options(&draft.question.stem).is_some() {
                high = false;
                reasons.push("填空/解答题行首仍有 A–D 选项表".into());
            }
        }
    }

    let heading_n = draft.method_heading_count;
    let analysis_n = draft.question.analysis.len();
    if heading_n > 0 && heading_n != analysis_n {
        high = false;
        reasons.push(format!(
            "解法标题 {heading_n} 项，analysis {analysis_n} 项"
        ));
    }
    if heading_n == 0 && looks_like_analysis_chunk(source_chunk) {
        if !analysis_paper_script_complete(&draft.question, source_chunk) {
            high = false;
            reasons.push("解析卷结构不完整（缺选项/答案/小问或解析）".into());
        }
    }

    // 与 ScriptDraft.method_heading_count 交叉核对原文
    let counted = count_method_headings(source_chunk);
    if counted != heading_n && major_count <= 1 && heading_n > 0 {
        high = false;
        reasons.push(format!(
            "块内解法标题计数 {counted} 与草稿 {heading_n} 不一致"
        ));
    }

    draft.confidence = if high {
        Confidence::High
    } else {
        Confidence::Low
    };
    draft.reasons = reasons;
    for r in &draft.reasons {
        if !draft.question.warnings.iter().any(|w| w == r) {
            draft.question.warnings.push(r.clone());
        }
    }
    draft.question.warnings.push("规则结构化".into());
    draft.question.confidence = if high { 0.9 } else { 0.4 };
}

pub fn always_stage2() -> bool {
    matches!(std::env::var("MATHSET_ALWAYS_STAGE2").as_deref(), Ok("1"))
}

pub fn should_call_llm(draft: &ScriptDraft) -> bool {
    should_call_llm_with(draft, always_stage2())
}

pub fn should_call_llm_with(draft: &ScriptDraft, force: bool) -> bool {
    force || draft.confidence == Confidence::Low
}

pub fn script_skip_accepted(draft: &ScriptDraft) -> bool {
    script_skip_accepted_with(draft, always_stage2())
}

pub fn script_skip_accepted_with(draft: &ScriptDraft, force: bool) -> bool {
    if should_call_llm_with(draft, force) {
        return false;
    }
    let report = validate_structured(
        &draft.question,
        draft.method_heading_count,
        draft.confidence,
    );
    report.schema_ok && !report.method_count_mismatch
}

/// 解析卷没有「法N」时，结构足够完整也可以跳过 Stage2。
fn analysis_paper_script_complete(q: &ParsedQuestion, chunk: &str) -> bool {
    let has_jiexi = chunk.contains("【解析】") || chunk.contains("【详解】");
    let has_answer_heading = chunk.contains("【答案】") || chunk.contains("[答案]");
    match q.question_type.as_str() {
        "choice" | "multiple" => {
            let n = q.options.as_ref().map(|o| o.len()).unwrap_or(0);
            n == 4
                && !stem_has_option_residue(&q.stem)
                && super::choice::has_printed_choice_answer(chunk)
        }
        "solution" => has_answer_heading && has_jiexi && stem_has_subquestions(&q.stem),
        "fill" => has_answer_heading && has_jiexi,
        _ => false,
    }
}

fn stem_has_subquestions(stem: &str) -> bool {
    let has1 = stem.contains("（1）") || stem.contains("(1)");
    let has2 = stem.contains("（2）") || stem.contains("(2)");
    has1 && has2
}

fn fields_blob(q: &ParsedQuestion) -> String {
    let mut s = q.stem.clone();
    if let Some(opts) = &q.options {
        for o in opts {
            s.push('\n');
            s.push_str(&o.content);
        }
    }
    for a in &q.analysis {
        s.push('\n');
        s.push_str(&a.title);
        s.push('\n');
        s.push_str(&a.content);
    }
    s
}
