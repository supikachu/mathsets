//! 解析正文清洗：去掉卷面编辑标记，不把【分析】/【点睛】留在解析里。

use std::sync::LazyLock;

use regex::Regex;

use crate::ai::types::{ParsedPart, ParsedQuestion};

static EDITORIAL_OPEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"【[ \t]*(分析|详解|解析|答案|解答|点睛)[^】]*】").expect("editorial open")
});

static METHOD_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*(?:法|解法|方法)[ \t]*[一二三四五六七八九十0-9]").expect("method line")
});

static MULTI_BLANK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\n{3,}").expect("blank lines"));

static LEADING_CHOICE_ANS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)\A\s*(?:\$\\mathrm\s*\{[A-Da-d]+\}\$|[A-D]{1,4})\s*\n+")
        .expect("leading choice ans")
});

pub fn clean_analysis_text(text: &str) -> String {
    let stripped = strip_editorial_sections(text);
    let mut s = LEADING_CHOICE_ANS.replace(&stripped, "").into_owned();
    s = drop_leading_strategy_paragraph(&s);
    collapse_blank(s.trim())
}

pub fn clean_question_editorial(q: &mut ParsedQuestion) {
    for a in &mut q.analysis {
        a.content = clean_analysis_text(&a.content);
    }
    clean_parts_editorial(&mut q.parts);
}

fn clean_parts_editorial(parts: &mut [ParsedPart]) {
    for p in parts {
        for a in &mut p.analyses {
            a.content = clean_analysis_text(&a.content);
        }
        if let Some(ans) = p.answer.as_mut() {
            *ans = EDITORIAL_OPEN.replace_all(ans, "").trim().to_string();
        }
        clean_parts_editorial(&mut p.children);
    }
}

pub fn strip_empty_analysis(q: &mut ParsedQuestion) {
    q.analysis.retain(|a| !a.content.trim().is_empty());
    strip_empty_in_parts(&mut q.parts);
}

fn strip_empty_in_parts(parts: &mut [ParsedPart]) {
    for p in parts {
        p.analyses.retain(|a| !a.content.trim().is_empty());
        strip_empty_in_parts(&mut p.children);
    }
}

fn strip_editorial_sections(text: &str) -> String {
    let marks: Vec<(usize, usize, String)> = EDITORIAL_OPEN
        .captures_iter(text)
        .filter_map(|c| {
            let full = c.get(0)?;
            let name = c.get(1)?.as_str().to_string();
            Some((full.start(), full.end(), name))
        })
        .collect();
    if marks.is_empty() {
        return text.to_string();
    }

    let mut kept = String::new();
    if marks[0].0 > 0 {
        kept.push_str(&text[..marks[0].0]);
    }
    for (i, &(_, tag_end, ref name)) in marks.iter().enumerate() {
        let next_tag = marks.get(i + 1).map(|(s, _, _)| *s).unwrap_or(text.len());
        let cut = section_body_end(text, tag_end, next_tag);
        let drop_body = matches!(name.as_str(), "分析" | "点睛" | "答案" | "解答");
        if drop_body {
            if cut < next_tag {
                kept.push_str(&text[cut..next_tag]);
            }
            continue;
        }
        kept.push_str(&text[tag_end..next_tag]);
    }
    kept
}

fn section_body_end(text: &str, body_start: usize, next_tag: usize) -> usize {
    let window = &text[body_start..next_tag];
    if let Some(m) = METHOD_LINE.find(window) {
        return body_start + m.start();
    }
    next_tag
}

/// 合并后无标签的短「即可/可求」段若后面已有演算，一并丢掉。
fn drop_leading_strategy_paragraph(text: &str) -> String {
    let t = text.trim();
    if t.is_empty() {
        return String::new();
    }
    for sep in ["\n\n", "；"] {
        if let Some((first, rest)) = t.split_once(sep) {
            if looks_like_strategy_blurb(first) && looks_like_worked_solution(rest) {
                return rest.trim().to_string();
            }
        }
    }
    t.to_string()
}

fn looks_like_strategy_blurb(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > 200 {
        return false;
    }
    if t.contains("故选") || t.contains("故填") {
        return false;
    }
    if t.contains("因为") && t.contains("所以") {
        return false;
    }
    const HINTS: &[&str] = &[
        "即可得",
        "即可求",
        "即可解",
        "即可判断",
        "即可选",
        "即可求解",
        "可求",
        "解出即可",
        "根据图象即可",
        "由递推即可",
    ];
    HINTS.iter().any(|h| t.contains(*h))
}

fn looks_like_worked_solution(text: &str) -> bool {
    let t = text.trim();
    t.contains("因为")
        || t.contains("故选")
        || t.contains("所以")
        || t.matches('$').count() >= 4
}

fn collapse_blank(s: &str) -> String {
    MULTI_BLANK.replace_all(s, "\n\n").trim().to_string()
}
