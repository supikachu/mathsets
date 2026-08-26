//! 解答题：把总题干末尾的（1）（2）问句填进 `parts[].stem`。

use std::sync::LazyLock;

use regex::Regex;

use crate::ai::types::{ParsedPart, ParsedQuestion};

static SUB_MARK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[（(][ \t]*([0-9一二三四五六七八九十]+)[ \t]*[）)]").expect("sub mark")
});

static ASK_AFTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[ \t]*(?:求证|证明|计算|判断|写出|探究|试问|请问|求|试|若|已知|设|当|请)")
        .expect("ask after")
});

pub fn peel_solution_sub_stems(q: &mut ParsedQuestion) {
    if q.question_type != "solution" {
        return;
    }
    let Some((shared, subs)) = extract_sub_stems(&q.stem) else {
        return;
    };
    if subs.is_empty() {
        return;
    }
    q.stem = shared;
    if q.parts.is_empty() {
        q.ensure_solution_parts();
    }
    grow_leaves(q, subs.len() as u32);
    for (n, text) in subs {
        if let Some(leaf) = find_leaf_mut(&mut q.parts, n) {
            if leaf.stem.trim().is_empty() {
                leaf.stem = text;
            }
        }
    }
}

fn extract_sub_stems(stem: &str) -> Option<(String, Vec<(u32, String)>)> {
    let marks = sub_marks_outside_math(stem);
    if marks.is_empty() {
        return None;
    }
    let first = marks[0].0;
    let shared = stem[..first].trim().to_string();
    if !shared_has_given(&shared) {
        return None;
    }
    let mut subs = Vec::new();
    for (i, &(start, num)) in marks.iter().enumerate() {
        let end = marks.get(i + 1).map(|(s, _)| *s).unwrap_or(stem.len());
        let chunk = stem[start..end].trim();
        let rest = SUB_MARK
            .find(chunk)
            .map(|m| chunk[m.end()..].trim())
            .unwrap_or(chunk);
        if rest.is_empty() {
            continue;
        }
        subs.push((num, rest.to_string()));
    }
    if subs.is_empty() {
        return None;
    }
    Some((shared, subs))
}

fn shared_has_given(shared: &str) -> bool {
    let t = strip_leading_question_no(shared).trim();
    if t.chars().count() < 8 {
        return false;
    }
    t.contains('$')
        || t.contains("已知")
        || t.contains("记")
        || t.contains("设")
        || t.contains("如图")
        || t.contains("若")
}

fn strip_leading_question_no(s: &str) -> &str {
    let t = s.trim_start();
    let bytes = t.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return t;
    }
    let rest = t[i..].trim_start_matches(['.', '．', '、', ' ', '\t']);
    if rest.len() < t.len() {
        rest
    } else {
        t
    }
}

fn sub_marks_outside_math(text: &str) -> Vec<(usize, u32)> {
    let mut out = Vec::new();
    for cap in SUB_MARK.captures_iter(text) {
        let m = cap.get(0).expect("match");
        if is_inside_math(text, m.start()) {
            continue;
        }
        let after = &text[m.end()..];
        if !ASK_AFTER.is_match(after) {
            continue;
        }
        let Some(num) = parse_cn_num(cap.get(1).map(|g| g.as_str()).unwrap_or("")) else {
            continue;
        };
        out.push((m.start(), num));
    }
    out
}

fn is_inside_math(text: &str, index: usize) -> bool {
    text[..index].matches('$').count() % 2 == 1
}

fn parse_cn_num(s: &str) -> Option<u32> {
    let t = s.trim();
    if t.chars().all(|c| c.is_ascii_digit()) {
        return t.parse().ok();
    }
    Some(match t {
        "一" => 1,
        "二" => 2,
        "三" => 3,
        "四" => 4,
        "五" => 5,
        "六" => 6,
        "七" => 7,
        "八" => 8,
        "九" => 9,
        "十" => 10,
        _ => return None,
    })
}

fn grow_leaves(q: &mut ParsedQuestion, n: u32) {
    q.ensure_solution_parts();
    let simple = q.parts.len() == 1
        && q.parts[0].children.is_empty()
        && q.parts[0].stem.trim().is_empty()
        && n > 1;
    if simple {
        q.parts.clear();
    }
    let mut have = walk_leaf_count(&q.parts);
    while have < n {
        have += 1;
        q.parts.push(empty_leaf(have));
    }
}

fn empty_leaf(n: u32) -> ParsedPart {
    ParsedPart {
        id: uuid::Uuid::new_v4().to_string(),
        label: format!("({n})"),
        stem: String::new(),
        children: Vec::new(),
        answer: Some(String::new()),
        analyses: Vec::new(),
        no_analysis_needed: false,
    }
}

fn walk_leaf_count(parts: &[ParsedPart]) -> u32 {
    parts
        .iter()
        .map(|p| {
            if p.children.is_empty() {
                1
            } else {
                walk_leaf_count(&p.children)
            }
        })
        .sum()
}

fn find_leaf_mut(parts: &mut [ParsedPart], n: u32) -> Option<&mut ParsedPart> {
    let mut numbered: Option<usize> = None;
    {
        let leaves = collect_leaf_labels(parts);
        for (i, label) in leaves.iter().enumerate() {
            if label_number(label) == Some(n) {
                numbered = Some(i);
                break;
            }
        }
        if numbered.is_none() {
            numbered = n
                .checked_sub(1)
                .map(|i| i as usize)
                .filter(|&i| i < leaves.len());
        }
    }
    let idx = numbered?;
    leaves_mut(parts).into_iter().nth(idx)
}

fn collect_leaf_labels(parts: &[ParsedPart]) -> Vec<String> {
    let mut out = Vec::new();
    fn rec(parts: &[ParsedPart], out: &mut Vec<String>) {
        for p in parts {
            if p.children.is_empty() {
                out.push(p.label.clone());
            } else {
                rec(&p.children, out);
            }
        }
    }
    rec(parts, &mut out);
    out
}

fn leaves_mut(parts: &mut [ParsedPart]) -> Vec<&mut ParsedPart> {
    let mut out = Vec::new();
    fn rec<'a>(parts: &'a mut [ParsedPart], out: &mut Vec<&'a mut ParsedPart>) {
        for p in parts {
            if p.children.is_empty() {
                out.push(p);
            } else {
                rec(&mut p.children, out);
            }
        }
    }
    rec(parts, &mut out);
    out
}

fn label_number(label: &str) -> Option<u32> {
    SUB_MARK
        .captures(label)
        .and_then(|c| parse_cn_num(c.get(1).map(|g| g.as_str()).unwrap_or("")))
}
