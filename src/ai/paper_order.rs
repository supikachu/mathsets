//! 按试卷题号排序：站外 JSON 导入顺序经常与卷面题号不一致。

use std::cmp::Ordering;

/// 解析 "14" / "14." / "14(2)" / "17（1）" → (大题号, 小问)。
pub fn parse_question_no_key(raw: &str) -> Option<(i32, i32)> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    let major: i32 = s[..i].parse().ok()?;
    let rest = s[i..].trim_start_matches(['.', '．', '、', ' ', '\t']);
    let rest = rest
        .strip_prefix('(')
        .or_else(|| rest.strip_prefix('（'))
        .unwrap_or("");
    let rest = rest.trim_start();
    let mut j = 0;
    let rb = rest.as_bytes();
    while j < rb.len() && rb[j].is_ascii_digit() {
        j += 1;
    }
    let minor = if j > 0 {
        rest[..j].parse().unwrap_or(0)
    } else {
        0
    };
    Some((major, minor))
}

/// 题干首行「14.」「第 14 题」等。
pub fn infer_question_no_from_stem(stem: &str) -> Option<String> {
    let line = stem.trim_start().lines().next()?.trim_start();
    let line = line
        .trim_start_matches('#')
        .trim_start_matches(|c: char| c.is_whitespace() || c == '*' || c == '_' || c == '>');
    let line = line.strip_prefix("第").unwrap_or(line).trim_start();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i > 3 {
        return None;
    }
    let n = &line[..i];
    let rest = line[i..].trim_start();
    if rest.is_empty()
        || rest.starts_with(['.', '．', '、', '题'])
        || rest.starts_with('(')
        || rest.starts_with('（')
    {
        return Some(n.to_string());
    }
    None
}

/// 排序键：有卷面题号最优先，其次 display_order，否则垫后。
pub fn paper_order_key(
    question_no: Option<&str>,
    display_order: Option<i32>,
    stem: &str,
) -> (u8, i32, i32) {
    if let Some(pair) = question_no
        .and_then(parse_question_no_key)
        .or_else(|| infer_question_no_from_stem(stem).as_deref().and_then(parse_question_no_key))
    {
        return (0, pair.0, pair.1);
    }
    if let Some(d) = display_order {
        return (1, d, 0);
    }
    (2, i32::MAX, 0)
}

pub fn cmp_paper_order(
    a_no: Option<&str>,
    a_order: Option<i32>,
    a_stem: &str,
    b_no: Option<&str>,
    b_order: Option<i32>,
    b_stem: &str,
) -> Ordering {
    paper_order_key(a_no, a_order, a_stem).cmp(&paper_order_key(b_no, b_order, b_stem))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_sub_question_nos() {
        assert_eq!(parse_question_no_key("14"), Some((14, 0)));
        assert_eq!(parse_question_no_key("14."), Some((14, 0)));
        assert_eq!(parse_question_no_key("17(2)"), Some((17, 2)));
        assert_eq!(parse_question_no_key("17（1）"), Some((17, 1)));
    }

    #[test]
    fn infers_from_stem_prefix() {
        assert_eq!(infer_question_no_from_stem("14. 已知函数"), Some("14".into()));
        assert_eq!(infer_question_no_from_stem("第 4 题 某地"), Some("4".into()));
        assert_eq!(infer_question_no_from_stem("已知椭圆"), None);
    }

    #[test]
    fn sorts_by_paper_number_not_json_order() {
        let mut items = vec![
            (Some("14"), "填空"),
            (Some("4"), "选择"),
            (Some("1"), "选择"),
        ];
        items.sort_by(|a, b| cmp_paper_order(a.0, None, a.1, b.0, None, b.1));
        let nos: Vec<_> = items.iter().map(|x| x.0).collect();
        assert_eq!(nos, vec![Some("1"), Some("4"), Some("14")]);
    }
}
