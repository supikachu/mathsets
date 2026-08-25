//! 把被切块/误切题号拆开的同一道题重新拼回去。
//!
//! 典型事故：OCR 把「（2）若过…」收成行首「2. 若过…」，或长解析按字数横切，
//! 后一段没有总干，只剩「法五 / 法六」，被 Stage2 当成第二道空题干题。

use super::layout::{is_implausible_major_no_drop, looks_like_math_question_start, question_major_no};
use super::paper_order::parse_question_no_key;
use super::structure::recover_question_sections;
use super::types::{AnalysisMethod, ParsedPart, ParsedQuestion};

/// 按原文顺序合并「无题干的解析残片」到上一题。
pub fn merge_split_questions(questions: Vec<ParsedQuestion>) -> Vec<ParsedQuestion> {
    let mut out: Vec<ParsedQuestion> = Vec::new();
    for mut q in questions {
        let own = q.stem.clone();
        recover_question_sections(&mut q, &own);
        if let Some(prev) = out.last_mut() {
            if is_continuation_fragment(prev, &q) {
                merge_question_into(prev, q);
                continue;
            }
        }
        out.push(q);
    }
    out
}

fn is_continuation_fragment(prev: &ParsedQuestion, curr: &ParsedQuestion) -> bool {
    if !is_fragment_stem(&curr.stem) {
        return false;
    }
    if !has_analysis_body(curr) {
        return false;
    }
    if looks_like_new_major_question(prev, curr) {
        return false;
    }
    true
}

fn is_fragment_stem(stem: &str) -> bool {
    let t = stem.trim();
    if t.is_empty() {
        return true;
    }
    if starts_like_method_or_analysis(t) {
        return true;
    }
    t.chars().count() < 12 && !looks_like_math_question_start(t)
}

fn starts_like_method_or_analysis(text: &str) -> bool {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    static HEAD: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"^(?:【(?:解析|分析|小问)|(?:解法|方法|法)\s*[一二三四五六七八九十百0-9]|另解|别解)",
        )
        .expect("method head")
    });
    HEAD.is_match(line)
}

fn has_analysis_body(q: &ParsedQuestion) -> bool {
    q.analysis.iter().any(|a| !a.content.trim().is_empty()) || parts_have_analysis(&q.parts)
}

fn parts_have_analysis(parts: &[ParsedPart]) -> bool {
    parts.iter().any(|p| {
        p.analyses.iter().any(|a| !a.content.trim().is_empty()) || parts_have_analysis(&p.children)
    })
}

fn looks_like_new_major_question(prev: &ParsedQuestion, curr: &ParsedQuestion) -> bool {
    let line = curr
        .stem
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if line.is_empty() {
        return false;
    }
    if !looks_like_math_question_start(line) {
        return false;
    }
    let Some(curr_no) = question_major_no(line).or_else(|| {
        curr.question_no
            .as_deref()
            .and_then(parse_question_no_key)
            .map(|(n, _)| n as u32)
    }) else {
        return false;
    };
    let Some(prev_no) = prev
        .question_no
        .as_deref()
        .and_then(parse_question_no_key)
        .map(|(n, _)| n as u32)
        .or_else(|| {
            prev.stem
                .lines()
                .map(str::trim)
                .find_map(question_major_no)
        })
    else {
        return curr_no >= 10;
    };
    curr_no != prev_no && !is_implausible_major_no_drop(prev_no, curr_no)
}

fn merge_question_into(dst: &mut ParsedQuestion, mut src: ParsedQuestion) {
    let extra = take_all_analyses(&mut src);
    append_to_primary_analysis(dst, extra);
    if dst.stem.trim().is_empty() && !src.stem.trim().is_empty() && !is_fragment_stem(&src.stem) {
        dst.stem = src.stem;
    }
    merge_part_answers(&mut dst.parts, &src.parts);
    if dst.correct_answer.is_none() {
        dst.correct_answer = src.correct_answer;
    }
    for kp in src.knowledge_points {
        if !dst.knowledge_points.iter().any(|x| x == &kp) {
            dst.knowledge_points.push(kp);
        }
    }
    dst.warnings.extend(src.warnings);
}

fn take_all_analyses(q: &mut ParsedQuestion) -> Vec<AnalysisMethod> {
    let mut out = std::mem::take(&mut q.analysis);
    take_part_analyses(&mut q.parts, &mut out);
    out
}

fn take_part_analyses(parts: &mut [ParsedPart], out: &mut Vec<AnalysisMethod>) {
    for p in parts {
        out.append(&mut p.analyses);
        take_part_analyses(&mut p.children, out);
    }
}

fn append_to_primary_analysis(q: &mut ParsedQuestion, extra: Vec<AnalysisMethod>) {
    if extra.is_empty() {
        return;
    }
    if let Some(leaf) = last_leaf_mut(&mut q.parts) {
        append_analyses(&mut leaf.analyses, extra);
    } else {
        append_analyses(&mut q.analysis, extra);
    }
}

fn last_leaf_mut(parts: &mut [ParsedPart]) -> Option<&mut ParsedPart> {
    let p = parts.last_mut()?;
    if p.children.is_empty() {
        Some(p)
    } else {
        last_leaf_mut(&mut p.children)
    }
}

fn append_analyses(dst: &mut Vec<AnalysisMethod>, extra: Vec<AnalysisMethod>) {
    for method in extra {
        if method.content.trim().is_empty() {
            continue;
        }
        let key = method_title_key(&method.title);
        if let Some(last) = dst.last_mut() {
            if method_title_key(&last.title) == key && !key.is_empty() {
                if !last.content.trim().is_empty() {
                    last.content.push_str("\n\n");
                }
                last.content.push_str(&method.content);
                if last.title.trim().is_empty() {
                    last.title = method.title;
                }
                continue;
            }
        }
        dst.push(method);
    }
}

fn method_title_key(title: &str) -> String {
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?:解法|方法|法)\s*([一二三四五六七八九十百0-9]+)")
            .expect("method title")
    });
    if let Some(c) = RE.captures(title.trim()) {
        return format!("法{}", normalize_method_num(c.get(1).map(|m| m.as_str()).unwrap_or("")));
    }
    title.trim().to_string()
}

fn normalize_method_num(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_digit()) {
        return s.trim_start_matches('0').to_string();
    }
    match s {
        "一" => "1".into(),
        "二" => "2".into(),
        "三" => "3".into(),
        "四" => "4".into(),
        "五" => "5".into(),
        "六" => "6".into(),
        "七" => "7".into(),
        "八" => "8".into(),
        "九" => "9".into(),
        "十" => "10".into(),
        _ => s.to_string(),
    }
}

fn merge_part_answers(dst: &mut [ParsedPart], src: &[ParsedPart]) {
    if src.is_empty() {
        return;
    }
    if let Some(leaf) = last_leaf_mut(dst) {
        if leaf.answer.as_ref().is_none_or(|a| a.trim().is_empty()) {
            if let Some(from) = last_leaf_answer(src) {
                if !from.trim().is_empty() {
                    leaf.answer = Some(from.to_string());
                }
            }
        }
    }
}

fn last_leaf_answer(parts: &[ParsedPart]) -> Option<&str> {
    let p = parts.last()?;
    if p.children.is_empty() {
        p.answer.as_deref()
    } else {
        last_leaf_answer(&p.children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse_q(v: serde_json::Value) -> ParsedQuestion {
        serde_json::from_value(v).expect("ParsedQuestion")
    }

    #[test]
    fn merges_stemless_method_fragment_into_previous() {
        let q16 = parse_q(json!({
            "question_type": "solution",
            "stem": "16. 已知椭圆",
            "question_no": "16",
            "parts": [{
                "id": "a",
                "label": "(2)",
                "stem": "求 l 的方程",
                "analyses": [
                    {"title": "法一", "content": "平移直线"},
                    {"title": "法五", "content": "斜率不存在时"}
                ]
            }]
        }));
        let q2 = parse_q(json!({
            "question_type": "solution",
            "stem": "",
            "question_no": "2",
            "analysis": [
                {"title": "法五", "content": "联立消 y"},
                {"title": "法六", "content": "水平宽乘铅垂高"}
            ]
        }));
        let merged = merge_split_questions(vec![q16, q2]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].question_no.as_deref(), Some("16"));
        let methods: Vec<_> = merged[0].parts[0]
            .analyses
            .iter()
            .map(|a| (a.title.as_str(), a.content.as_str()))
            .collect();
        assert_eq!(methods.len(), 3, "{methods:?}");
        assert!(methods[1].1.contains("斜率不存在时"));
        assert!(methods[1].1.contains("联立消 y"), "同名法五应拼接而不是变成两个页签");
        assert_eq!(methods[2].0, "法六");
        assert!(methods[2].1.contains("水平宽乘铅垂高"));
    }

    #[test]
    fn does_not_merge_two_real_questions() {
        let a = parse_q(json!({
            "question_type": "solution",
            "stem": "16. 已知椭圆",
            "question_no": "16",
            "analysis": [{"title": "法一", "content": "x"}]
        }));
        let b = parse_q(json!({
            "question_type": "solution",
            "stem": "17. 已知函数 f(x)",
            "question_no": "17",
            "analysis": [{"title": "法一", "content": "y"}]
        }));
        let merged = merge_split_questions(vec![a, b]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn merges_fragment_with_methods_dumped_in_stem() {
        let q16 = parse_q(json!({
            "question_type": "solution",
            "stem": "16. 已知椭圆",
            "question_no": "16",
            "parts": [{
                "label": "(2)",
                "stem": "求 l 的方程",
                "analyses": [{"title": "法一", "content": "平移直线"}]
            }]
        }));
        let q2 = parse_q(json!({
            "question_type": "solution",
            "stem": "法五：联立消 y\n法六：水平宽乘铅垂高",
            "question_no": "2",
            "analysis": []
        }));
        let merged = merge_split_questions(vec![q16, q2]);
        assert_eq!(merged.len(), 1);
        let titles: Vec<_> = merged[0].parts[0]
            .analyses
            .iter()
            .map(|a| a.title.as_str())
            .collect();
        assert!(titles.iter().any(|t| t.contains("法六") || t.contains("六")), "{titles:?}");
        assert!(merged[0]
            .parts[0]
            .analyses
            .iter()
            .any(|a| a.content.contains("水平宽")));
    }
}
