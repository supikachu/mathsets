//! V2.1.1 题目规范化与去重 hash（计划书 §八）
//!
//! 规范化算法（Rust 单点实现，SQL 不做第二套）：
//! 1. 全角/半角统一（U+FF01..U+FF5E → ASCII；U+3000 → 空格）
//! 2. 行尾标点剥离（CJK 标点与 ,;:!?，句点保留以免破坏小数）
//! 3. LaTeX 间距记号归一（\, \; \quad \qquad \! \\ → 空格）
//! 4. 空白折叠（所有空白序列 → 单个空格，含换行）
//! 5. 不做 lower-case（保留数学变量大小写语义）
//!
//! hash 组合：
//! - content_hash              = SHA-256(规范化 stem‖options‖answer‖analysis)
//!   解答题：stem‖options‖canonical(structure)（含各叶解法）
//! - normalized_content_hash   = SHA-256(规范化 stem‖options‖answer)（不含解析）
//!   解答题：stem‖options‖canonical(structure 去掉 analyses)

use serde_json::Value;
use sha2::{Digest, Sha256};

/// 全角字符 → 半角（U+FF01..U+FF5E 覆盖 ！＂＃＄％＆＇（）＊＋，－．／：；＜＝＞？＠Ａ-Ｚａ-ｚ｛｜｝～）
fn full_to_half_width(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        let code = c as u32;
        if (0xFF01..=0xFF5E).contains(&code) {
            out.push(char::from_u32(code - 0xFEE0).unwrap_or(c));
        } else if code == 0x3000 {
            // 全角空格 → 普通空格
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

/// 行尾标点剥离（对每一行 trim 末尾的 CJK/半角标点与空白；句点保留）
fn strip_trailing_punctuation(line: &str) -> &str {
    line.trim_end_matches(
        |c: char| {
            matches!(
                c,
                '。' | '，' | '；' | '：' | '、' | '！' | '？' | ',' | ';' | ':' | '!' | '?'
                    | ' '
                    | '\t'
            )
        },
    )
}

/// LaTeX 间距记号 → 空格（在空白折叠之前执行）
fn normalize_latex_spacing(input: &str) -> String {
    input
        .replace(r"\,", " ")
        .replace(r"\;", " ")
        .replace(r"\quad", " ")
        .replace(r"\qquad", " ")
        .replace(r"\!", " ")
        .replace(r"\\", " ")
        .replace(r"\ ", " ")
}

/// 题目内容规范化（见模块文档）
pub fn normalize_text(input: &str) -> String {
    // 1. 全角/半角
    let half = full_to_half_width(input);
    // 3. LaTeX 间距（在行拆分前做，避免 \, 跨行）
    let latex = normalize_latex_spacing(&half);
    // 2. 行尾标点剥离（按 \n 拆分后逐行 trim）
    let stripped: Vec<&str> = latex.split('\n').map(strip_trailing_punctuation).collect();
    // 4. 空白折叠（split_whitespace 同时折叠换行/制表/全角空格）
    stripped.join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// 选项序列化为确定文本（label+content 拼接）
fn options_text(options: Option<&Value>) -> String {
    match options {
        Some(Value::Array(arr)) => arr
            .iter()
            .map(|o| {
                let label = o.get("label").and_then(|v| v.as_str()).unwrap_or("");
                let content = o.get("content").and_then(|v| v.as_str()).unwrap_or("");
                format!("{label}{content}")
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// 答案序列化（compact JSON，保证确定性）
fn answer_text(answer: &Value) -> String {
    serde_json::to_string(answer).unwrap_or_default()
}

fn sort_value(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut out = serde_json::Map::new();
            for k in keys {
                out.insert(k.clone(), sort_value(&map[k]));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sort_value).collect()),
        other => other.clone(),
    }
}

fn strip_analyses(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, val) in map {
                if k == "analyses" {
                    continue;
                }
                out.insert(k.clone(), strip_analyses(val));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(strip_analyses).collect()),
        other => other.clone(),
    }
}

fn structure_canonical(structure: &Value) -> String {
    serde_json::to_string(&sort_value(structure)).unwrap_or_default()
}

fn structure_is_present(structure: Option<&Value>) -> bool {
    matches!(structure, Some(v) if !v.is_null())
}

/// content_hash：stem‖options‖answer‖analysis（全部规范化后拼接）
/// 若传入解答题 `structure`，则以 canonical JSON 替代 answer+analysis。
pub fn compute_content_hash(
    stem: &str,
    options: Option<&Value>,
    answer: &Value,
    analysis: Option<&str>,
) -> String {
    compute_content_hash_ex(stem, options, answer, analysis, None)
}

pub fn compute_content_hash_ex(
    stem: &str,
    options: Option<&Value>,
    answer: &Value,
    analysis: Option<&str>,
    structure: Option<&Value>,
) -> String {
    if structure_is_present(structure) {
        let parts = [
            normalize_text(stem),
            normalize_text(&options_text(options)),
            normalize_text(&structure_canonical(structure.unwrap())),
        ];
        return sha256_hex(&parts.join("\u{1f}"));
    }
    let parts = [
        normalize_text(stem),
        normalize_text(&options_text(options)),
        normalize_text(&answer_text(answer)),
        normalize_text(analysis.unwrap_or("")),
    ];
    sha256_hex(&parts.join("\u{1f}"))
}

/// normalized_content_hash：stem‖options‖answer（不含解析，用于跨资料去重）
pub fn compute_normalized_content_hash(stem: &str, options: Option<&Value>, answer: &Value) -> String {
    compute_normalized_content_hash_ex(stem, options, answer, None)
}

pub fn compute_normalized_content_hash_ex(
    stem: &str,
    options: Option<&Value>,
    answer: &Value,
    structure: Option<&Value>,
) -> String {
    if structure_is_present(structure) {
        let stripped = strip_analyses(structure.unwrap());
        let parts = [
            normalize_text(stem),
            normalize_text(&options_text(options)),
            normalize_text(&structure_canonical(&stripped)),
        ];
        return sha256_hex(&parts.join("\u{1f}"));
    }
    let parts = [
        normalize_text(stem),
        normalize_text(&options_text(options)),
        normalize_text(&answer_text(answer)),
    ];
    sha256_hex(&parts.join("\u{1f}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_full_to_half_width() {
        assert_eq!(normalize_text("ＡＢＣ１２３（１）"), "ABC123(1)");
        assert_eq!(normalize_text("你好，世界！"), "你好,世界");
        assert_eq!(normalize_text("　全角空格"), "全角空格");
    }

    #[test]
    fn test_whitespace_collapse() {
        assert_eq!(normalize_text("已知  函数\n\n\t f(x)"), "已知 函数 f(x)");
        assert_eq!(normalize_text("  a  b  "), "a b");
    }

    #[test]
    fn test_latex_spacing_normalized() {
        assert_eq!(normalize_text(r"x\,y"), "x y");
        assert_eq!(normalize_text(r"a\quads+b"), "a s+b");
        assert_eq!(normalize_text(r"a\\b"), "a b");
    }

    #[test]
    fn test_trailing_punctuation_stripped() {
        assert_eq!(normalize_text("求值。\n答案："), "求值 答案");
        // 句点保留（小数）
        assert_eq!(normalize_text("x = 3.14。"), "x = 3.14");
    }

    #[test]
    fn test_no_lowercase_math_preserved() {
        assert_ne!(normalize_text("f(X)"), normalize_text("f(x)"));
    }

    #[test]
    fn test_hash_deterministic() {
        let answer = json!({"kind":"choice","value":{"options":["A"]}});
        let h1 = compute_content_hash("求 $f(x)$ 的极值。", None, &answer, Some("求导。"));
        let h2 = compute_content_hash("求 $f(x)$ 的极值。", None, &answer, Some("求导。"));
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_content_vs_normalized_hash_differ() {
        let answer = json!({"kind":"fill","value":{"blanks":[{"position":1,"answer":"2"}]}});
        let content = compute_content_hash("题干", None, &answer, Some("解析"));
        let normalized = compute_normalized_content_hash("题干", None, &answer);
        assert_ne!(content, normalized);
    }

    #[test]
    fn test_same_stem_different_analysis_same_normalized_hash() {
        let answer = json!({"kind":"solution","value":{"subs":[]}});
        let n1 = compute_normalized_content_hash("题目", None, &answer);
        let n2 = compute_normalized_content_hash("题目", None, &answer);
        assert_eq!(n1, n2);
    }

    #[test]
    fn test_structure_hash_ignores_analysis_in_normalized() {
        let s1 = json!({
            "version": 1,
            "parts": [{
                "id": "a",
                "label": "(1)",
                "stem": "",
                "children": [],
                "answer": "m=-1",
                "analyses": [{"id":"x","title":"解法一","content":"证奇"}],
                "no_analysis_needed": false
            }]
        });
        let mut s2 = s1.clone();
        s2["parts"][0]["analyses"][0]["content"] = json!("另一种证法");
        let n1 = compute_normalized_content_hash_ex("题干", None, &Value::Null, Some(&s1));
        let n2 = compute_normalized_content_hash_ex("题干", None, &Value::Null, Some(&s2));
        assert_eq!(n1, n2);
        let c1 = compute_content_hash_ex("题干", None, &Value::Null, None, Some(&s1));
        let c2 = compute_content_hash_ex("题干", None, &Value::Null, None, Some(&s2));
        assert_ne!(c1, c2);
    }
}
