//! 从题目文本中按固定字段顺序抽出公式槽位

use serde_json::Value;

use crate::export::content::split_content;
use crate::export::model::InlineNode;
use crate::models::question_structure::parse_structure;

/// 与 `question_math_assets.field` CHECK 一致
pub const FIELDS: &[&str] = &[
    "stem",
    "options",
    "correct_answer",
    "analysis",
    "structure",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormulaSlot {
    pub field: &'static str,
    pub ordinal: i32,
    pub latex: String,
    pub display: bool,
}

/// 抽公式：字段顺序 stem → options → correct_answer → analysis → structure
///
/// options / structure 按稳定顺序遍历（数组下标 / 问树 DFS），避免 JSON Object 无序。
pub fn extract_slots(
    stem: &str,
    options: Option<&Value>,
    correct_answer: Option<&Value>,
    analysis: Option<&str>,
    structure: Option<&Value>,
) -> Vec<FormulaSlot> {
    let mut out = Vec::new();
    push_field(&mut out, "stem", stem);
    push_options_field(&mut out, options);
    push_json_field(&mut out, "correct_answer", correct_answer);
    if let Some(a) = analysis {
        push_field(&mut out, "analysis", a);
    }
    push_structure_field(&mut out, structure);
    out
}

fn push_options_field(out: &mut Vec<FormulaSlot>, value: Option<&Value>) {
    let Some(Value::Array(items)) = value else {
        push_json_field(out, "options", value);
        return;
    };
    let mut buf = String::new();
    for item in items {
        match item {
            Value::String(s) => {
                buf.push_str(s);
                buf.push('\n');
            }
            Value::Object(map) => {
                for key in ["content", "text", "value", "label"] {
                    if let Some(Value::String(s)) = map.get(key) {
                        buf.push_str(s);
                        buf.push('\n');
                    }
                }
            }
            _ => collect_strings(Some(item), &mut buf),
        }
    }
    if !buf.is_empty() {
        push_field(out, "options", &buf);
    }
}

fn push_structure_field(out: &mut Vec<FormulaSlot>, value: Option<&Value>) {
    if let Some(parsed) = parse_structure(value) {
        let mut ordinal = 0i32;
        walk_parts_math(&parsed.parts, out, &mut ordinal);
        return;
    }
    push_json_field(out, "structure", value);
}

fn walk_parts_math(
    parts: &[crate::models::question_structure::QuestionPart],
    out: &mut Vec<FormulaSlot>,
    ordinal: &mut i32,
) {
    for p in parts {
        push_math_into(out, "structure", &p.stem, ordinal);
        if let Some(ans) = &p.answer {
            push_math_into(out, "structure", ans, ordinal);
        }
        for blk in &p.analyses {
            push_math_into(out, "structure", &blk.content, ordinal);
        }
        walk_parts_math(&p.children, out, ordinal);
    }
}

fn push_math_into(
    out: &mut Vec<FormulaSlot>,
    field: &'static str,
    text: &str,
    ordinal: &mut i32,
) {
    for (latex, display) in math_in(text) {
        out.push(FormulaSlot {
            field,
            ordinal: *ordinal,
            latex,
            display,
        });
        *ordinal += 1;
    }
}

fn push_json_field(out: &mut Vec<FormulaSlot>, field: &'static str, value: Option<&Value>) {
    let mut buf = String::new();
    collect_strings(value, &mut buf);
    if !buf.is_empty() {
        push_field(out, field, &buf);
    }
}

fn push_field(out: &mut Vec<FormulaSlot>, field: &'static str, text: &str) {
    let mut ordinal = 0i32;
    for (latex, display) in math_in(text) {
        out.push(FormulaSlot {
            field,
            ordinal,
            latex,
            display,
        });
        ordinal += 1;
    }
}

fn collect_strings(v: Option<&Value>, buf: &mut String) {
    let Some(v) = v else { return };
    match v {
        Value::String(s) => {
            buf.push_str(s);
            buf.push('\n');
        }
        Value::Array(items) => {
            for item in items {
                collect_strings(Some(item), buf);
            }
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for k in keys {
                collect_strings(map.get(k), buf);
            }
        }
        _ => {}
    }
}

fn math_in(text: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    collect_math(&split_content(text), &mut out);
    out
}

fn collect_math(nodes: &[InlineNode], out: &mut Vec<(String, bool)>) {
    for node in nodes {
        match node {
            InlineNode::Math { latex, display } => out.push((latex.clone(), *display)),
            InlineNode::Table { header, rows, .. } => {
                for cell in header.iter().chain(rows.iter().flatten()) {
                    collect_math(&split_content(cell), out);
                }
            }
            InlineNode::ImgRow { caption, .. } => {
                if let Some(c) = caption {
                    collect_math(&split_content(c), out);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_stem_and_options_with_ordinals() {
        let slots = extract_slots(
            r"设 $a=1$，求 $$\frac{1}{2}$$",
            Some(&json!([{"label":"A","content":"$x^2$"}])),
            None,
            Some("提示：$b$"),
            None,
        );
        assert_eq!(slots.len(), 4);
        assert_eq!(slots[0].field, "stem");
        assert_eq!(slots[0].ordinal, 0);
        assert_eq!(slots[1].field, "stem");
        assert_eq!(slots[1].ordinal, 1);
        assert!(slots[1].display);
        assert_eq!(slots[2].field, "options");
        assert_eq!(slots[2].latex, "x^2");
        assert_eq!(slots[3].field, "analysis");
    }
}
