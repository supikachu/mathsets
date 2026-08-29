//! `structure_chunk`：纯脚本把 OCR markdown 块做成 `ScriptDraft`。

use std::sync::LazyLock;

use regex::Regex;

use crate::ai::cleaner::sanitize_question_markup;
use crate::ai::layout::{is_instruction_numbered_line, question_start_regex};
use crate::ai::types::{AnalysisMethod, ParsedAnswer, ParsedQuestion};

use super::analysis::{count_method_headings, split_body_and_tail, split_chunk_analysis};
use super::looks_like_choice_stem;
use super::options::extract_choice_options;
use super::{Confidence, ScriptDraft};

static MD_IMAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!\[[^\]]*\]\(([^)\s]+)\)").expect("md image"));
static LINE_START_A: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*A\s*[\.．、\)]").expect("choice option"));

pub fn structure_chunk(chunk: &str) -> ScriptDraft {
    let majors = major_question_starts(chunk);
    let major_count = majors.len();
    let slice = first_question_slice(chunk, &majors);
    let image_urls_in_chunk = collect_markdown_image_urls(slice);

    let (body, _tail) = split_body_and_tail(slice);
    let question_type = guess_chunk_question_type(slice);
    let question_no = extract_chunk_question_no(slice);

    let (stem, options) = if matches!(question_type.as_str(), "choice" | "multiple") {
        match extract_choice_options(body) {
            Some((stem, opts)) => (stem, Some(opts)),
            None => (body.trim().to_string(), None),
        }
    } else {
        (body.trim().to_string(), None)
    };

    let analysis = split_chunk_analysis(slice);
    let method_heading_count = count_method_headings(slice);

    let mut question = blank_question(&question_type);
    question.stem = sanitize_question_markup(&stem);
    if let Some(mut opts) = options {
        for o in &mut opts {
            o.content = sanitize_question_markup(&o.content);
        }
        question.options = Some(opts);
    }
    question.analysis = analysis
        .into_iter()
        .map(|a| AnalysisMethod {
            title: a.title,
            content: sanitize_question_markup(&a.content),
        })
        .collect();
    question.question_no = question_no;
    question.image_urls = image_urls_in_chunk.clone();
    super::choice::fill_choice_answers(&mut question);

    let mut draft = ScriptDraft {
        question,
        confidence: Confidence::Low,
        reasons: Vec::new(),
        method_heading_count,
        image_urls_in_chunk,
    };
    super::confidence::evaluate(&mut draft, slice, major_count);
    draft
}

pub fn guess_chunk_question_type(chunk: &str) -> String {
    let (body, _) = split_body_and_tail(chunk);
    if extract_choice_options(body).is_some() || looks_like_choice_stem(body) || LINE_START_A.is_match(body)
    {
        "choice".into()
    } else if chunk.contains("____") || chunk.contains("填空") {
        "fill".into()
    } else {
        "solution".into()
    }
}

pub fn extract_chunk_question_no(chunk: &str) -> Option<String> {
    major_question_starts(chunk)
        .into_iter()
        .next()
        .map(|(_, n)| n)
}

fn first_question_slice<'a>(chunk: &'a str, majors: &[(usize, String)]) -> &'a str {
    match majors {
        [] => chunk,
        [(start, _)] => &chunk[*start..],
        [(start, _), (end, _), ..] => &chunk[*start..*end],
    }
}

fn major_question_starts(chunk: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    for line in chunk.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        let trimmed = content.trim();
        if !is_instruction_numbered_line(trimmed) {
            if let Some(caps) = question_start_regex().captures(trimmed) {
                if let Some(n) = caps.get(1) {
                    out.push((offset, n.as_str().to_string()));
                }
            }
        }
        offset += line.len();
    }
    out
}

fn collect_markdown_image_urls(md: &str) -> Vec<String> {
    let mut out = Vec::new();
    for cap in MD_IMAGE.captures_iter(md) {
        let url = cap[1].to_string();
        if url.starts_with("IMAGE_PLACEHOLDER") || url.trim().is_empty() {
            continue;
        }
        if !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

fn blank_question(question_type: &str) -> ParsedQuestion {
    ParsedQuestion {
        question_type: question_type.to_string(),
        sub_type: None,
        difficulty: None,
        stem: String::new(),
        options: None,
        correct_answer: Some(ParsedAnswer::empty_for_type(question_type)),
        analysis: vec![],
        knowledge_points: vec![],
        confidence: 0.4,
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

/// Low 置信补丁：只送题干+选项，解法由规则从原文回填。
pub fn stage2_patch_user_input(chunk: &str, draft: &ScriptDraft) -> String {
    let stem_only = super::analysis::stage2_llm_input(chunk);
    let hint = serde_json::json!({
        "question_type": draft.question.question_type,
        "question_no": draft.question.question_no,
        "stem": draft.question.stem,
        "options": draft.question.options,
        "confidence": format!("{:?}", draft.confidence),
        "reasons": draft.reasons,
    });
    format!(
        "{}\n\n---\n规则草稿（题干/选项，解法由系统回填）：\n{}",
        stem_only.trim(),
        hint
    )
}

/// 3～8 题一批：按顺序拼接 slim 题干。
pub fn stage2_batch_user_input(items: &[(&str, &ScriptDraft)]) -> String {
    items
        .iter()
        .enumerate()
        .map(|(i, (chunk, draft))| {
            format!("### 第{}题\n{}", i + 1, stage2_patch_user_input(chunk, draft))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::structure::confidence::MARKDOWN_FALLBACK_LOW_CHARS;

    #[test]
    fn choice_ad_extracted_and_stripped_from_stem() {
        let md = "\
8. 下列结论正确的是\n\
A. $1$\n\
B. $2$\n\
C. $3$\n\
D. $4$\n";
        let draft = structure_chunk(md);
        assert_eq!(draft.question.question_type, "choice");
        assert_eq!(draft.question.question_no.as_deref(), Some("8"));
        assert!(
            draft.question.stem.contains("下列结论正确的是"),
            "{}",
            draft.question.stem
        );
        assert!(
            !draft.question.stem.contains("A."),
            "stem 不得残留 A.: {}",
            draft.question.stem
        );
        let opts = draft.question.options.as_ref().expect("options");
        assert_eq!(opts.len(), 4);
        assert_eq!(opts[0].label, "A");
        assert_eq!(opts[0].content, "$1$");
        assert_eq!(opts[3].label, "D");
        assert_eq!(opts[3].content, "$4$");
        assert_eq!(draft.confidence, Confidence::High, "{:?}", draft.reasons);
    }

    #[test]
    fn choice_with_figure_keeps_image_in_stem_and_urls() {
        let md = "\
8. 下列结论正确的是\n\
![](/uploads/q1.png)\n\
A. 甲\n\
B. 乙\n\
C. 丙\n\
D. 丁\n";
        let draft = structure_chunk(md);
        assert!(
            draft.question.stem.contains("/uploads/q1.png"),
            "配图应留在题干: {}",
            draft.question.stem
        );
        assert!(
            draft
                .image_urls_in_chunk
                .iter()
                .any(|u| u.contains("/uploads/q1.png")),
            "{:?}",
            draft.image_urls_in_chunk
        );
        assert_eq!(draft.confidence, Confidence::High, "{:?}", draft.reasons);
    }

    #[test]
    fn analysis_paper_splits_three_methods_uncut() {
        let md = "\
16. 已知椭圆 $C$。\n\
【解析】\n\
法一：平移直线，设斜率 $k$。\n\
法二：点差法，由切点弦公式。\n\
另解：参数方程 $x=2\\cos\\theta$。\n";
        let draft = structure_chunk(md);
        assert_eq!(
            draft.question.analysis.len(),
            3,
            "{:?}",
            draft
                .question
                .analysis
                .iter()
                .map(|a| (&a.title, &a.content))
                .collect::<Vec<_>>()
        );
        assert_eq!(draft.method_heading_count, 3);
        assert!(draft.question.analysis[0].content.contains("平移直线"));
        assert!(draft.question.analysis[1].content.contains("点差法"));
        assert!(draft.question.analysis[2].content.contains("参数方程"));
        assert!(!draft.question.analysis[0].content.contains("点差法"));
        assert!(!draft.question.analysis[1].content.contains("参数方程"));
        assert_eq!(draft.confidence, Confidence::High, "{:?}", draft.reasons);
    }

    #[test]
    fn subquestions_stay_in_stem_not_analysis() {
        let md = "\
16. 已知椭圆。\n\
（1）求离心率；\n\
（2）求直线 $l$ 的方程。\n\
【解析】\n\
法一：由 $a=2$ 得 $e$。\n\
法二：点差法。\n";
        let draft = structure_chunk(md);
        assert!(
            draft.question.stem.contains("（1）") && draft.question.stem.contains("（2）"),
            "{}",
            draft.question.stem
        );
        assert!(
            !draft
                .question
                .analysis
                .iter()
                .any(|a| a.content.contains("求离心率") || a.content.contains("求直线")),
            "{:?}",
            draft.question.analysis
        );
        assert_eq!(draft.question.analysis.len(), 2);
        assert_eq!(draft.confidence, Confidence::High, "{:?}", draft.reasons);
    }

    #[test]
    fn instruction_line_is_not_a_second_question() {
        let md = "\
1.答卷前考生务必填涂准考证号。\n\
8. 下列结论正确的是\n\
A. 1\n\
B. 2\n\
C. 3\n\
D. 4\n";
        let draft = structure_chunk(md);
        assert_eq!(draft.question.question_no.as_deref(), Some("8"));
        assert!(draft.question.stem.contains("下列结论正确的是"));
        assert!(
            !draft.question.stem.contains("答卷前"),
            "不得把说明行臆造成题干: {}",
            draft.question.stem
        );
        assert_eq!(draft.confidence, Confidence::High, "{:?}", draft.reasons);
    }

    #[test]
    fn analysis_blob_without_method_headings_is_low() {
        let md = "\
8. 已知 $x=1$。\n\
【解析】\n\
因为 $x=1$，所以选 A。几种想法糊在一起没有法一法二。\n";
        let draft = structure_chunk(md);
        assert_eq!(draft.confidence, Confidence::Low, "{:?}", draft.reasons);
        assert_eq!(draft.question.analysis.len(), 1);
        assert_eq!(draft.question.analysis[0].title, "解析");
    }

    #[test]
    fn two_question_numbers_is_low_and_does_not_merge_stems() {
        let md = "\
8. 下列结论正确的是\n\
A. 1\n\
B. 2\n\
C. 3\n\
D. 4\n\
9. 已知函数 $f(x)=x$。\n";
        let draft = structure_chunk(md);
        assert_eq!(draft.confidence, Confidence::Low, "{:?}", draft.reasons);
        assert!(
            !draft.question.stem.contains("已知函数"),
            "不得并成一道题干: {}",
            draft.question.stem
        );
        assert!(draft.question.stem.contains("下列结论正确的是"));
    }

    #[test]
    fn three_options_is_low() {
        let md = "\
8. 下列结论正确的是\n\
A. 1\n\
B. 2\n\
C. 3\n";
        let draft = structure_chunk(md);
        assert_eq!(draft.question.options.as_ref().map(|o| o.len()), Some(3));
        assert_eq!(draft.confidence, Confidence::Low, "{:?}", draft.reasons);
    }

    #[test]
    fn huge_chunk_is_low() {
        let mut md = "8. 下列结论正确的是\nA. 1\nB. 2\nC. 3\nD. 4\n".to_string();
        md.push_str(&"余".repeat(MARKDOWN_FALLBACK_LOW_CHARS));
        let draft = structure_chunk(&md);
        assert_eq!(draft.confidence, Confidence::Low, "{:?}", draft.reasons);
    }

    #[test]
    fn pi_literal_is_not_a_second_question() {
        let md = "\
8. 下列结论正确的是\n\
3.14 约为圆周率。\n\
A. 甲\n\
B. 乙\n\
C. 丙\n\
D. 丁\n";
        let draft = structure_chunk(md);
        assert_eq!(draft.question.question_no.as_deref(), Some("8"));
        assert!(draft.question.stem.contains("3.14"));
        assert_eq!(draft.confidence, Confidence::High, "{:?}", draft.reasons);
    }

    #[test]
    fn high_choice_does_not_need_provider() {
        let md = "\
8. 下列结论正确的是\n\
A. 1\n\
B. 2\n\
C. 3\n\
D. 4\n";
        let draft = structure_chunk(md);
        assert_eq!(draft.confidence, Confidence::High, "{:?}", draft.reasons);
        assert!(!crate::ai::structure::should_call_llm_with(&draft, false));
        assert!(crate::ai::structure::script_skip_accepted_with(&draft, false));
    }

    #[test]
    fn low_analysis_blob_needs_llm() {
        let md = "\
8. 已知 $x=1$。\n\
【解析】\n\
因为 $x=1$，所以选 A。几种想法糊在一起没有法一法二。\n";
        let draft = structure_chunk(md);
        assert_eq!(draft.confidence, Confidence::Low, "{:?}", draft.reasons);
        assert!(crate::ai::structure::should_call_llm_with(&draft, false));
        assert!(!crate::ai::structure::script_skip_accepted_with(&draft, false));
    }

    #[test]
    fn high_validation_failure_forces_llm() {
        let md = "\
8. 下列结论正确的是\n\
A. 1\n\
B. 2\n\
C. 3\n\
D. 4\n";
        let mut draft = structure_chunk(md);
        assert!(crate::ai::structure::script_skip_accepted_with(&draft, false));
        draft.question.stem.clear();
        draft.question.options = None;
        assert!(
            !crate::ai::structure::script_skip_accepted_with(&draft, false),
            "校验失败的 High 必须回退 LLM"
        );
        assert!(crate::ai::structure::should_call_llm_with(&draft, true),
            "MATHSET_ALWAYS_STAGE2=1 强制走 LLM"
        );
    }

    #[test]
    fn analysis_choice_without_fa_n_is_high_if_answer_printed() {
        let md = "\
8. 已知向量 $\\vec{a}=(0,1)$，则 $x=(\\quad)$\n\
A. $-2$\n\
B. $-1$\n\
C. $1$\n\
D. $2$\n\
【答案】$\\mathrm{B}$\n\
【解析】\n\
【分析】根据向量垂直可求 $x$。\n\
【详解】因为垂直，所以 $x=-1$。故选：B。\n";
        let draft = structure_chunk(md);
        assert_eq!(draft.question.question_type, "choice");
        assert_eq!(draft.question.options.as_ref().map(|o| o.len()), Some(4));
        assert_eq!(draft.confidence, Confidence::High, "{:?}", draft.reasons);
        assert!(crate::ai::structure::script_skip_accepted_with(&draft, false));
    }

    #[test]
    fn analysis_solution_subs_without_fa_n_is_high() {
        let md = "\
16. 已知椭圆 $C$。\n\
（1）求离心率；\n\
（2）求直线 $l$ 的方程。\n\
【答案】（1）$e=\\frac12$（2）$x=1$\n\
【解析】\n\
由 $a=2,b=1$ 得离心率，再求直线。\n";
        let draft = structure_chunk(md);
        assert_eq!(draft.question.question_type, "solution");
        assert_eq!(draft.confidence, Confidence::High, "{:?}", draft.reasons);
        assert!(crate::ai::structure::script_skip_accepted_with(&draft, false));
    }

    #[test]
    fn patch_input_sends_stem_not_jiexi() {
        let md = "\
8. 下列结论正确的是\n\
A. 1\n\
B. 2\n\
C. 3\n\
D. 4\n\
【答案】$\\mathrm{B}$\n\
【解析】\n\
【详解】很长的演算过程不该送给模型。\n";
        let draft = structure_chunk(md);
        let input = stage2_patch_user_input(md, &draft);
        assert!(input.contains("下列结论正确的是"), "{input}");
        assert!(!input.contains("很长的演算过程"), "{input}");
        let batch = stage2_batch_user_input(&[(md, &draft)]);
        assert!(batch.contains("### 第1题"));
        assert!(!batch.contains("很长的演算过程"));
    }
}
