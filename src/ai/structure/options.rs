//! 从题干切开 A–D（以及 A–C / A–E）。

use std::sync::LazyLock;

use regex::Regex;

use crate::ai::layout::{is_instruction_numbered_line, question_start_regex};
use crate::ai::types::ParsedOption;

static OPTION_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(?:\*\*)?(?:[\(（]\s*([A-Ea-e])\s*[\)）]|([A-Ea-e])\s*[\.、．\)）])\s*(.*?)\s*$",
    )
    .expect("option line")
});

/// 行首 `A.` / `A、` / `A．` / `A)` / `(A)`，再接连续 B/C/D（或到 E）。
pub(crate) fn extract_choice_options(body: &str) -> Option<(String, Vec<ParsedOption>)> {
    if let Some(hit) = extract_line_start_options(body) {
        return Some(hit);
    }
    super::choice::extract_options_from_stem(body)
}

fn extract_line_start_options(body: &str) -> Option<(String, Vec<ParsedOption>)> {
    let mut items: Vec<(char, String)> = Vec::new();
    let mut first_start: Option<usize> = None;
    let mut expected = 'A';
    let mut offset = 0usize;
    let mut current: Option<(char, String)> = None;

    for line in body.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        let trimmed = content.trim();
        if should_stop_options(trimmed) && current.is_some() {
            break;
        }
        if let Some((letter, rest)) = parse_option_line(trimmed) {
            if first_start.is_none() {
                if letter != 'A' {
                    offset += line.len();
                    continue;
                }
                first_start = Some(offset);
            }
            if letter != expected {
                break;
            }
            if let Some((prev_letter, prev_content)) = current.take() {
                items.push((prev_letter, prev_content));
            }
            current = Some((letter, rest.to_string()));
            match next_option_letter(letter) {
                Some(n) => expected = n,
                None => {
                    items.push(current.take().expect("just set"));
                    break;
                }
            }
        } else if let Some((_, ref mut buf)) = current {
            if !trimmed.is_empty() {
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(trimmed);
            }
        }
        offset += line.len();
    }
    if let Some((letter, content)) = current {
        items.push((letter, content));
    }

    if items.len() < 3 {
        return None;
    }
    let start = first_start?;
    let stem = body[..start].trim_end().to_string();
    if stem.trim().is_empty() {
        return None;
    }
    let opts: Vec<ParsedOption> = items
        .into_iter()
        .map(|(letter, content)| ParsedOption {
            label: letter.to_string(),
            content: content.trim().to_string(),
        })
        .collect();
    Some((stem, opts))
}

fn parse_option_line(line: &str) -> Option<(char, &str)> {
    let cap = OPTION_LINE.captures(line)?;
    let raw = cap
        .get(1)
        .or_else(|| cap.get(2))
        .map(|m| m.as_str())
        .unwrap_or("");
    let letter = raw.chars().next()?.to_ascii_uppercase();
    if !('A'..='E').contains(&letter) {
        return None;
    }
    let rest = cap.get(3).map(|m| m.as_str()).unwrap_or("").trim();
    Some((letter, rest))
}

fn next_option_letter(letter: char) -> Option<char> {
    match letter {
        'A' => Some('B'),
        'B' => Some('C'),
        'C' => Some('D'),
        'D' => Some('E'),
        _ => None,
    }
}

fn should_stop_options(line: &str) -> bool {
    if line.is_empty() {
        return false;
    }
    if super::analysis::looks_like_analysis_chunk(line) {
        return true;
    }
    if question_start_regex().is_match(line) && !is_instruction_numbered_line(line) {
        return true;
    }
    false
}

pub(crate) fn stem_has_option_residue(stem: &str) -> bool {
    stem.contains("A.") || stem.contains("A、") || stem.contains("A．")
}
