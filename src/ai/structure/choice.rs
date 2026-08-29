//! 选择题兜底：从题干抽出 A–D、回填【答案】/故选、把卷头说明移出解析。

use std::sync::LazyLock;

use regex::Regex;

use crate::ai::layout::{exam_section_heading, split_trailing_exam_section};
use crate::ai::types::{AnalysisMethod, ParsedAnswer, ParsedOption, ParsedPart, ParsedQuestion};

static OPTIONS_TAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?is)(?:^|\n|[。；;！？$）)])\s*\$?\s*A[\.、．\)）]\s*(.*?)\s*B[\.、．\)）]\s*(.*?)\s*C[\.、．\)）]\s*(.*?)\s*D[\.、．\)）]\s*(.*)$",
    )
    .expect("options tail")
});

static GU_XUAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"故选[:：]?\s*([A-Da-d,，、\s]{1,12})").expect("故选"));

static ANS_BRACKET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"【\s*答案\s*】\s*[:：]?\s*\$?\s*([A-Da-d,，、\s]{1,12})").expect("【答案】")
});

static ANS_SQUARE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[\s*答案\s*\]\s*[:：]?\s*\$?\s*([A-Da-d,，、\s]{1,12})").expect("[答案]")
});

static ANS_COLON: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"答案[:：]\s*\$?\s*([A-Da-d,，、\s]{1,12})").expect("答案："));

/// MinerU 常写成 `【答案】$\mathrm{B}$` / `故选：$\mathrm{B}$`，字母不在 $ 后直接出现。
static ANS_MATHRM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:【\s*答案\s*】|\[\s*答案\s*\]|故选|答案)[:：]?\s*\$?\\mathrm\s*\{([A-Da-d]+)\}",
    )
    .expect("答案 mathrm")
});

static MATHRM_BARE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\\mathrm\s*\{([A-Da-d]+)\}").expect("mathrm letters")
});

pub(crate) fn looks_like_choice_stem(stem: &str) -> bool {
    extract_options_from_stem(stem).is_some()
}

/// 题干末尾的 A–D（含同一行 `A.3 B.4 C.6 D.8`）。
pub(crate) fn extract_options_from_stem(stem: &str) -> Option<(String, Vec<ParsedOption>)> {
    let mut last: Option<regex::Captures<'_>> = None;
    for cap in OPTIONS_TAIL.captures_iter(stem) {
        last = Some(cap);
    }
    let cap = last?;
    let m = cap.get(0)?;
    let mut contents = vec![
        cap.get(1).map(|g| g.as_str()).unwrap_or(""),
        cap.get(2).map(|g| g.as_str()).unwrap_or(""),
        cap.get(3).map(|g| g.as_str()).unwrap_or(""),
        cap.get(4).map(|g| g.as_str()).unwrap_or(""),
    ];
    contents[3] = trim_option_tail(contents[3]);
    let opts = vec![
        opt("A", contents[0]),
        opt("B", contents[1]),
        opt("C", contents[2]),
        opt("D", contents[3]),
    ];
    if opts.iter().any(|o| o.content.trim().is_empty()) {
        return None;
    }
    let mut cut = m.start();
    if let Some(ch) = stem[cut..].chars().next() {
        if matches!(ch, '$' | '）' | ')') {
            cut += ch.len_utf8();
            if ch == ')' && stem[cut..].starts_with('$') {
                cut += 1;
            }
        }
    }
    let mut new_stem = stem[..cut].trim_end().to_string();
    if new_stem.ends_with('$') && new_stem.matches('$').count() % 2 == 1 {
        new_stem.pop();
        new_stem = new_stem.trim_end().to_string();
    }
    if new_stem.trim().is_empty() {
        return None;
    }
    Some((new_stem, opts))
}

fn opt(label: &str, content: &str) -> ParsedOption {
    ParsedOption {
        label: label.to_string(),
        content: content.trim().to_string(),
    }
}

fn trim_option_tail(s: &str) -> &str {
    let mut end = s.len();
    for m in ["【", "[答案]", "[ 答案", "故选", "##"] {
        if let Some(i) = s.find(m) {
            end = end.min(i);
        }
    }
    s[..end].trim_end()
}

pub(crate) fn salvage_choice_structure(q: &mut ParsedQuestion) {
    if q.question_type == "fill" {
        return;
    }
    let has_opts = q.options.as_ref().is_some_and(|o| o.len() >= 4);
    if let Some((new_stem, opts)) = extract_options_from_stem(&q.stem) {
        if !has_opts {
            q.options = Some(opts);
        }
        q.stem = new_stem;
        convert_to_choice_if_needed(q);
        return;
    }
    if has_opts {
        convert_to_choice_if_needed(q);
    }
}

fn convert_to_choice_if_needed(q: &mut ParsedQuestion) {
    if matches!(q.question_type.as_str(), "choice" | "multiple") {
        return;
    }
    let letters = collect_choice_letters_from_fields(q);
    let analyses = flatten_part_analyses(&q.parts);
    if q.analysis.iter().all(|a| a.content.trim().is_empty()) && !analyses.is_empty() {
        q.analysis = analyses;
    }
    q.parts.clear();
    q.question_type = "choice".into();
    if !letters.is_empty() {
        q.correct_answer = Some(ParsedAnswer::Choice {
            options: letters.clone(),
        });
        if letters.len() >= 2 {
            mark_as_multiple(q);
        }
    } else {
        q.correct_answer = Some(ParsedAnswer::empty_for_type("choice"));
    }
}

pub(crate) fn has_printed_choice_answer(text: &str) -> bool {
    !parse_choice_letters_ex(text, false).is_empty()
}

pub(crate) fn fill_choice_answers(q: &mut ParsedQuestion) {
    if !matches!(q.question_type.as_str(), "choice" | "multiple") {
        return;
    }
    if !choice_answer_is_empty(q) {
        maybe_mark_multi_from_answer(q);
        return;
    }
    let mut blob = String::new();
    blob.push_str(&q.stem);
    blob.push('\n');
    for a in &q.analysis {
        blob.push_str(&a.content);
        blob.push('\n');
    }
    collect_part_text(&q.parts, &mut blob);
    let letters = parse_choice_letters_ex(&blob, false);
    if letters.is_empty() {
        return;
    }
    q.correct_answer = Some(ParsedAnswer::Choice {
        options: letters.clone(),
    });
    if letters.len() >= 2 {
        mark_as_multiple(q);
    }
}

pub(crate) fn apply_choice_answers_if_empty(
    q: &mut ParsedQuestion,
    answers: &[(Option<u32>, String)],
) {
    if !matches!(q.question_type.as_str(), "choice" | "multiple") {
        return;
    }
    if !choice_answer_is_empty(q) {
        maybe_mark_multi_from_answer(q);
        return;
    }
    let mut letters = Vec::new();
    for (_, t) in answers {
        for l in parse_choice_letters_ex(t, true) {
            if !letters.contains(&l) {
                letters.push(l);
            }
        }
    }
    if letters.is_empty() {
        return;
    }
    q.correct_answer = Some(ParsedAnswer::Choice {
        options: letters.clone(),
    });
    if letters.len() >= 2 {
        mark_as_multiple(q);
    }
}

pub(crate) fn strip_exam_sections_from_question(q: &mut ParsedQuestion) {
    q.stem = split_trailing_exam_section(&q.stem).0.to_string();
    for a in &mut q.analysis {
        a.content = split_trailing_exam_section(&a.content).0.to_string();
    }
    strip_trailing_in_parts(&mut q.parts);

    let mut mark_multi = false;
    q.stem = strip_leading_exam_heading(&q.stem, &mut mark_multi);
    if let Some(first) = q.analysis.first_mut() {
        first.content = strip_leading_exam_heading(&first.content, &mut mark_multi);
    }
    if mark_multi {
        mark_as_multiple(q);
    }
}

fn strip_leading_exam_heading(text: &str, mark_multi: &mut bool) -> String {
    let t = text.trim();
    let first = t.lines().next().unwrap_or("");
    if !exam_section_heading(first) {
        return t.to_string();
    }
    if heading_is_multi(first) {
        *mark_multi = true;
    }
    t.find('\n')
        .map(|i| t[i + 1..].trim_start().to_string())
        .unwrap_or_default()
}

fn heading_is_multi(line: &str) -> bool {
    line.contains("多项") || line.contains("多选")
}

fn mark_as_multiple(q: &mut ParsedQuestion) {
    if matches!(q.question_type.as_str(), "choice" | "multiple") {
        q.question_type = "multiple".into();
        q.sub_type = Some("multi".into());
    }
}

fn maybe_mark_multi_from_answer(q: &mut ParsedQuestion) {
    if let Some(ParsedAnswer::Choice { options }) = &q.correct_answer {
        if options.len() >= 2 {
            mark_as_multiple(q);
        }
    }
}

fn choice_answer_is_empty(q: &ParsedQuestion) -> bool {
    match &q.correct_answer {
        Some(ParsedAnswer::Choice { options }) => options.iter().all(|s| s.trim().is_empty()),
        _ => true,
    }
}

fn parse_choice_letters_ex(text: &str, allow_bare: bool) -> Vec<String> {
    for re in [
        &*ANS_MATHRM,
        &*ANS_BRACKET,
        &*ANS_SQUARE,
        &*GU_XUAN,
        &*ANS_COLON,
    ] {
        if let Some(c) = re.captures(text) {
            let letters = letters_from(c.get(1).map(|m| m.as_str()).unwrap_or(""));
            if !letters.is_empty() {
                return letters;
            }
        }
    }
    if allow_bare {
        let t = text.trim();
        if t.chars().count() <= 24 {
            if let Some(c) = MATHRM_BARE.captures(t) {
                let letters = letters_from(c.get(1).map(|m| m.as_str()).unwrap_or(""));
                if !letters.is_empty() {
                    return letters;
                }
            }
            if let Some(v) = compact_letters(t) {
                return v;
            }
        }
    }
    Vec::new()
}

fn letters_from(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for c in s.chars() {
        let u = c.to_ascii_uppercase();
        if ('A'..='D').contains(&u) {
            let t = u.to_string();
            if !out.contains(&t) {
                out.push(t);
            }
        }
    }
    out
}

fn compact_letters(s: &str) -> Option<Vec<String>> {
    let v: String = s.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    if v.is_empty() {
        return None;
    }
    if !v
        .chars()
        .all(|c| matches!(c.to_ascii_uppercase(), 'A'..='D'))
    {
        return None;
    }
    Some(letters_from(&v))
}

fn collect_choice_letters_from_fields(q: &ParsedQuestion) -> Vec<String> {
    if let Some(ParsedAnswer::Choice { options }) = &q.correct_answer {
        if options.iter().any(|s| !s.trim().is_empty()) {
            return options.clone();
        }
    }
    let mut texts = Vec::new();
    if let Some(ParsedAnswer::Solution { subs }) = &q.correct_answer {
        for s in subs {
            texts.push(s.content.clone());
        }
    }
    collect_part_answers(&q.parts, &mut texts);
    for t in texts {
        let letters = parse_choice_letters_ex(&t, true);
        if !letters.is_empty() {
            return letters;
        }
    }
    Vec::new()
}

fn collect_part_answers(parts: &[ParsedPart], texts: &mut Vec<String>) {
    for p in parts {
        if let Some(a) = &p.answer {
            texts.push(a.clone());
        }
        collect_part_answers(&p.children, texts);
    }
}

fn collect_part_text(parts: &[ParsedPart], blob: &mut String) {
    for p in parts {
        if let Some(a) = &p.answer {
            blob.push_str(a);
            blob.push('\n');
        }
        for a in &p.analyses {
            blob.push_str(&a.content);
            blob.push('\n');
        }
        collect_part_text(&p.children, blob);
    }
}

fn flatten_part_analyses(parts: &[ParsedPart]) -> Vec<AnalysisMethod> {
    let mut out = Vec::new();
    fn walk(parts: &[ParsedPart], out: &mut Vec<AnalysisMethod>) {
        for p in parts {
            out.extend(p.analyses.iter().cloned());
            walk(&p.children, out);
        }
    }
    walk(parts, &mut out);
    out
}

fn strip_trailing_in_parts(parts: &mut [ParsedPart]) {
    for p in parts {
        p.stem = split_trailing_exam_section(&p.stem).0.to_string();
        if let Some(a) = p.answer.as_mut() {
            *a = split_trailing_exam_section(a).0.to_string();
        }
        for an in &mut p.analyses {
            an.content = split_trailing_exam_section(&an.content).0.to_string();
        }
        strip_trailing_in_parts(&mut p.children);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::structure::finalize_parsed_question;
    use serde_json::json;

    fn parse_q(v: serde_json::Value) -> ParsedQuestion {
        serde_json::from_value(v).expect("ParsedQuestion")
    }

    fn choice_letters(q: &ParsedQuestion) -> Vec<String> {
        match &q.correct_answer {
            Some(ParsedAnswer::Choice { options }) => options.clone(),
            _ => vec![],
        }
    }

    #[test]
    fn compact_options_in_stem_become_choice_not_solution() {
        let mut q = parse_q(json!({
            "question_type": "solution",
            "stem": "7. 当 $x \\in [0, 2\\pi]$ 时，曲线 $y = \\sin x$ 与 $y = 2\\sin(3x)$ 的交点个数为 ( ) A.3 B.4 C.6 D.8",
            "correct_answer": {"kind": "solution", "value": {"subs": [{"sub_id": 1, "content": "C"}]}},
            "analysis": [{"title": "解法一", "content": "数形结合。故选：C"}],
            "parts": []
        }));
        finalize_parsed_question(&mut q);
        assert_eq!(q.question_type, "choice", "{}", q.question_type);
        let opts = q.options.as_ref().expect("options");
        assert_eq!(opts.len(), 4);
        assert_eq!(opts[0].content, "3");
        assert_eq!(opts[3].content, "8");
        assert!(!q.stem.contains("A.3"), "选项不得留在题干: {}", q.stem);
        assert!(q.stem.contains("交点个数"));
        assert_eq!(choice_letters(&q), vec!["C".to_string()]);
        assert!(q.parts.is_empty());
    }

    #[test]
    fn bracket_answer_bc_fills_and_marks_multiple() {
        let mut q = parse_q(json!({
            "question_type": "choice",
            "stem": "9. 已知随机变量服从正态分布。",
            "options": [
                {"label": "A", "content": "甲"},
                {"label": "B", "content": "乙"},
                {"label": "C", "content": "丙"},
                {"label": "D", "content": "丁"}
            ],
            "correct_answer": {"kind": "choice", "value": {"options": []}},
            "analysis": [{"title": "解法一", "content": "【答案】BC\n由对称性知选 B、C。"}],
            "parts": []
        }));
        finalize_parsed_question(&mut q);
        assert_eq!(choice_letters(&q), vec!["B".to_string(), "C".to_string()]);
        assert_eq!(q.question_type, "multiple");
        assert_eq!(q.sub_type.as_deref(), Some("multi"));
    }

    #[test]
    fn guxuan_abd_fills_multiple() {
        let mut q = parse_q(json!({
            "question_type": "choice",
            "stem": "10. 设函数 $f(x)$。",
            "options": [
                {"label": "A", "content": "a"},
                {"label": "B", "content": "b"},
                {"label": "C", "content": "c"},
                {"label": "D", "content": "d"}
            ],
            "correct_answer": {"kind": "choice", "value": {"options": []}},
            "analysis": [{"title": "解法一", "content": "逐项判断。故选：ABD."}],
            "parts": []
        }));
        finalize_parsed_question(&mut q);
        assert_eq!(
            choice_letters(&q),
            vec!["A".to_string(), "B".to_string(), "D".to_string()]
        );
        assert_eq!(q.question_type, "multiple");
    }

    #[test]
    fn trailing_section_heading_not_kept_and_does_not_flip_single_choice() {
        let mut q = parse_q(json!({
            "question_type": "choice",
            "stem": "8. 已知函数 $f(x)$。",
            "options": [
                {"label": "A", "content": "a"},
                {"label": "B", "content": "b"},
                {"label": "C", "content": "c"},
                {"label": "D", "content": "d"}
            ],
            "correct_answer": {"kind": "choice", "value": {"options": []}},
            "analysis": [{"title": "解法一", "content": "=======f(10)>100。\n故选：B\n\n## 二、选择题：本题共3小题，每小题6分，共18分。在每小题给出的选项中，有多项符合题目要求。全部选对得6分。"}],
            "parts": []
        }));
        finalize_parsed_question(&mut q);
        assert_eq!(choice_letters(&q), vec!["B".to_string()]);
        assert_eq!(q.question_type, "choice", "上一题不得因下一大题「多项」变成多选");
        let analysis = &q.analysis[0].content;
        assert!(
            !analysis.contains("二、选择题") && !analysis.contains("多项符合"),
            "卷头不得留在解析: {analysis}"
        );
        assert!(analysis.contains("故选：B"));
    }

    #[test]
    fn leading_multi_section_marks_multiple_and_is_stripped() {
        let mut q = parse_q(json!({
            "question_type": "choice",
            "stem": "## 二、选择题：本题共3小题，每小题6分，共18分。在每小题给出的选项中，有多项符合题目要求。\n9. 已知随机变量。",
            "options": [
                {"label": "A", "content": "a"},
                {"label": "B", "content": "b"},
                {"label": "C", "content": "c"},
                {"label": "D", "content": "d"}
            ],
            "correct_answer": {"kind": "choice", "value": {"options": []}},
            "analysis": [{"title": "解法一", "content": "故选：BC"}],
            "parts": []
        }));
        finalize_parsed_question(&mut q);
        assert!(!q.stem.contains("二、选择题"), "{}", q.stem);
        assert!(q.stem.contains("已知随机变量"));
        assert_eq!(q.question_type, "multiple");
        assert_eq!(choice_letters(&q), vec!["B".to_string(), "C".to_string()]);
    }

    #[test]
    fn square_bracket_answer_in_analysis() {
        let mut q = parse_q(json!({
            "question_type": "choice",
            "stem": "9. 正态。",
            "options": [
                {"label": "A", "content": "a"},
                {"label": "B", "content": "b"},
                {"label": "C", "content": "c"},
                {"label": "D", "content": "d"}
            ],
            "correct_answer": {"kind": "choice", "value": {"options": []}},
            "analysis": [{"title": "解法一", "content": "[答案] ACD\n逐项判断。"}],
            "parts": []
        }));
        finalize_parsed_question(&mut q);
        assert_eq!(
            choice_letters(&q),
            vec!["A".to_string(), "C".to_string(), "D".to_string()]
        );
        assert_eq!(q.question_type, "multiple");
    }

    #[test]
    fn mathrm_answer_from_bracket_and_bare_latex() {
        let mut q = parse_q(json!({
            "question_type": "choice",
            "stem": "8. 已知向量。",
            "options": [
                {"label": "A", "content": "-2"},
                {"label": "B", "content": "-1"},
                {"label": "C", "content": "1"},
                {"label": "D", "content": "2"}
            ],
            "correct_answer": {"kind": "choice", "value": {"options": []}},
            "analysis": [{"title": "解法一", "content": "【答案】$\\mathrm{B}$\n因为垂直，所以 $x=2$."}],
            "parts": []
        }));
        finalize_parsed_question(&mut q);
        assert_eq!(choice_letters(&q), vec!["B".to_string()]);
    }
}
