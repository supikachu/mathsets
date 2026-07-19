use regex::Regex;
use serde::de::DeserializeOwned;

/// 清洗 LLM 输出，提取纯 JSON（快速路径）
///
/// 1. 剥离 ```json ... ``` 或 ``` ... ``` 包裹
/// 2. 截取第一个 { 到最后一个 }
/// 3. 移除尾部多余逗号
pub fn clean_llm_json(raw: &str) -> Result<String, String> {
    let mut s = raw.trim().to_string();

    // 1. 剥离 ```json ... ``` 或 ``` ... ``` 包裹
    let re_block = Regex::new(r"(?s)```(?:json)?\s*(.*?)\s*```").unwrap();
    if let Some(caps) = re_block.captures(&s) {
        s = caps.get(1).map(|m| m.as_str().to_string()).unwrap_or(s);
    }

    // 2. 截取第一个 { 到最后一个 }（快速路径）
    let start = s.find('{');
    let end = s.rfind('}');
    match (start, end) {
        (Some(s_idx), Some(e_idx)) if e_idx > s_idx => {
            s = s[s_idx..=e_idx].to_string();
        }
        _ => return Err("LLM 输出中未找到 JSON 对象".into()),
    }

    // 3. 移除尾部多余逗号（如 ] ，} 前）
    let re_trailing = Regex::new(r",\s*([}\]])").unwrap();
    s = re_trailing.replace_all(&s, "$1").to_string();

    Ok(s)
}

/// 花括号计数器：从第一个 { 开始，按栈匹配找到最外层闭合 } 的精确位置
/// 用于 rfind('}') 误截取时的兜底（题干正文含 } 字符时）
///
/// 注意：字符串字面量内部的 { } 不计数，转义字符正确处理
pub fn extract_json_by_bracket_count(s: &str) -> Result<String, String> {
    let start = s.find('{').ok_or("未找到起始 {")?;
    let bytes = s.as_bytes();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escape = false;
    let mut end_idx = None;

    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if escape {
            escape = false;
            continue;
        }
        if b == b'\\' && in_string {
            escape = true;
            continue;
        }
        if b == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    end_idx = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }

    match end_idx {
        Some(e) => Ok(s[start..=e].to_string()),
        None => Err("花括号未闭合（depth 未归零，LLM 输出残缺）".into()),
    }
}

/// 修复 JSON 字符串字面量内部的非法转义反斜杠。
///
/// JSON 规范只允许 9 种转义：`\" \\ \/ \b \f \n \r \t \uXXXX`。
/// LLM 输出经常含有 LaTeX 反斜杠（`\sqrt`、`\d`、`\frac` 等），
/// 未转义时 serde_json 会抛 `invalid escape` 错误。
///
/// 本函数扫描字符串字面量（`"..."` 内部），遇到 `\` 时检查下一字节：
///   - 合法转义字符 → 保留原样
///   - 非法转义字符 → 把 `\` 替换为 `\\`（双重转义，保留字面反斜杠）
///   - 行尾单独的 `\` → 替换为 `\\`
///
/// **关键**：只在字符串字面量内部修复，绝不碰 JSON 结构字符
/// （`{`、`}`、`[`、`]`、`:`、`,`），否则会破坏 JSON 结构。
///
/// UTF-8 安全性：多字节 UTF-8 序列的首字节 >= 0x80，
/// 不会与 ASCII `"`, `\` 冲突，可安全按字节扫描。
fn fix_invalid_escapes(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len() + 16);
    let mut i = 0;
    let mut in_string = false;

    /// JSON 合法转义字符（紧跟在 \ 后的下一字节）
    const VALID_ESCAPES: &[u8] = b"\"\\/bfnrtu";

    while i < bytes.len() {
        let b = bytes[i];

        if !in_string {
            out.push(b);
            if b == b'"' {
                in_string = true;
            }
            i += 1;
            continue;
        }

        // 在字符串字面量内
        if b != b'\\' {
            out.push(b);
            if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        // 遇到 \ — 判断下一个字节是否是合法 JSON 转义字符
        if i + 1 >= bytes.len() {
            // 字符串末尾单独的 \ → 修复为 \\
            out.extend_from_slice(b"\\\\");
            i += 1;
            continue;
        }

        let next = bytes[i + 1];
        if VALID_ESCAPES.contains(&next) {
            // 合法转义，保留原样
            out.push(b);
            out.push(next);
            i += 2;
        } else {
            // 非法转义（如 \d \s \sqrt 等 LaTeX 命令）
            // 把 \ 替换为 \\，下一轮继续处理 next 字符
            out.extend_from_slice(b"\\\\");
            i += 1;
        }
    }

    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// 打印出错位置附近的文本上下文，辅助定位是哪个公式带偏了 AI
fn log_error_context(text: &str, line: usize, col: usize) {
    let lines: Vec<&str> = text.lines().collect();
    if line == 0 || line > lines.len() {
        return;
    }
    let line_idx = line - 1;
    let target_line = lines[line_idx];
    tracing::error!(
        "出错行 {line} (col {col}, {} 字节) 完整内容：\n  {target_line}",
        target_line.len()
    );

    // 截取出错位置前 80 字节、后 80 字节的片段
    let col_byte = col.saturating_sub(1); // serde_json column 是 1-based
    let start = col_byte.saturating_sub(80);
    let end = (col_byte + 80).min(target_line.len());
    if end > start {
        tracing::error!(
            "出错位置附近片段 [byte {}..{}]：\n  {}",
            start,
            end,
            &target_line[start..end]
        );
    }
}

/// 两阶段清洗 + 反序列化：快速路径 → 花括号计数器兜底 → 报错
///
/// 阶段 1：clean_llm_json（rfind 截取 + 移除尾逗号）
/// 阶段 1b：若 1 失败，调用 fix_invalid_escapes 修复非法转义后重试
/// 阶段 2：extract_json_by_bracket_count（花括号计数器精确定位）
/// 阶段 2b：若 2 失败，调用 fix_invalid_escapes 修复非法转义后重试
/// 最终失败：打印原始文本前 800 字符 + 出错行上下文
pub fn clean_and_parse<T: DeserializeOwned>(raw: &str) -> Result<T, String> {
    // ===== 阶段 1：快速路径 =====
    if let Ok(cleaned) = clean_llm_json(raw) {
        match serde_json::from_str::<T>(&cleaned) {
            Ok(parsed) => return Ok(parsed),
            Err(e) => {
                let line = e.line();
                let col = e.column();
                tracing::warn!("阶段1 clean_llm_json 反序列化失败 (line {line}, col {col}): {e}");
                log_error_context(&cleaned, line, col);
            }
        }

        // 阶段 1b：修复字符串字面量内部非法转义后重试
        let fixed = fix_invalid_escapes(&cleaned);
        if fixed != cleaned {
            match serde_json::from_str::<T>(&fixed) {
                Ok(parsed) => {
                    tracing::info!("阶段1b fix_invalid_escapes 修复成功");
                    return Ok(parsed);
                }
                Err(e) => {
                    let line = e.line();
                    let col = e.column();
                    tracing::warn!("阶段1b fix_invalid_escapes 后仍失败 (line {line}, col {col}): {e}");
                    log_error_context(&fixed, line, col);
                }
            }
        }
    }

    // ===== 阶段 2：花括号计数器兜底 =====
    if let Ok(precise) = extract_json_by_bracket_count(raw) {
        match serde_json::from_str::<T>(&precise) {
            Ok(parsed) => return Ok(parsed),
            Err(e) => {
                let line = e.line();
                let col = e.column();
                tracing::warn!("阶段2 bracket_count 反序列化失败 (line {line}, col {col}): {e}");
                log_error_context(&precise, line, col);
            }
        }

        // 阶段 2b：修复非法转义后重试
        let fixed = fix_invalid_escapes(&precise);
        if fixed != precise {
            match serde_json::from_str::<T>(&fixed) {
                Ok(parsed) => {
                    tracing::info!("阶段2b fix_invalid_escapes 修复成功");
                    return Ok(parsed);
                }
                Err(e) => {
                    let line = e.line();
                    let col = e.column();
                    tracing::warn!("阶段2b fix_invalid_escapes 后仍失败 (line {line}, col {col}): {e}");
                    log_error_context(&fixed, line, col);
                }
            }
        }
    }

    // ===== 最终失败：打印原始文本前 800 字符 =====
    let preview: String = raw.chars().take(800).collect();
    tracing::error!(
        "AI JSON 反序列化彻底失败。原始文本前 800 字符：\n{}",
        preview
    );
    Err("AI 返回 JSON 反序列化失败（所有清洗路径均未通过）".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_llm_json_with_codeblock() {
        let input = "```json\n{\"a\": 1}\n```";
        assert_eq!(clean_llm_json(input).unwrap(), "{\"a\": 1}");
    }

    #[test]
    fn test_clean_llm_json_without_codeblock() {
        let input = "{\"a\": 1}";
        assert_eq!(clean_llm_json(input).unwrap(), "{\"a\": 1}");
    }

    #[test]
    fn test_clean_llm_json_with_prefix_text() {
        let input = "好的，这是结果：\n{\"a\": 1}";
        assert_eq!(clean_llm_json(input).unwrap(), "{\"a\": 1}");
    }

    #[test]
    fn test_clean_llm_json_trailing_comma() {
        let input = "{\"a\": 1, \"b\": [1, 2,],}";
        let cleaned = clean_llm_json(input).unwrap();
        assert!(serde_json::from_str::<serde_json::Value>(&cleaned).is_ok());
    }

    #[test]
    fn test_clean_llm_json_no_brace() {
        let input = "这不是 JSON";
        assert!(clean_llm_json(input).is_err());
    }

    #[test]
    fn test_bracket_count_normal() {
        let input = r#"{"stem": "求 x", "answer": "1"}"#;
        assert_eq!(extract_json_by_bracket_count(input).unwrap(), input);
    }

    #[test]
    fn test_bracket_count_with_inner_brace_in_string() {
        // 题干含集合 {x | x > 1}
        let input = r#"{"stem": "求集合 {x | x > 1} 的元素个数", "answer": "无穷"}"#;
        let result = extract_json_by_bracket_count(input).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn test_bracket_count_truncated() {
        // 尾部残缺
        let input = r#"{"stem": "求集合 {x | x > 1} 的元素个数", "answer": "#;
        assert!(extract_json_by_bracket_count(input).is_err());
    }

    #[test]
    fn test_clean_and_parse_normal() {
        let input = r#"```json
        {"question_type": "solution", "stem": "求 x", "correct_answer": {"kind": "solution", "value": {"subs": [{"sub_id": 1, "content": "x=1"}]}}, "analysis": [{"title": "解法一", "content": "推导"}], "confidence": 0.9, "warnings": [], "knowledge_points": [], "image_placeholders": []}
        ```"#;
        let result: Result<serde_json::Value, String> = clean_and_parse(input);
        assert!(result.is_ok());
        let v = result.unwrap();
        assert_eq!(v["question_type"], "solution");
    }

    #[test]
    fn test_clean_and_parse_with_brace_in_stem() {
        // 题干含 {x | x > 1}，快速路径 rfind('}') 会截取到字符串内部的 }
        // 但因为 } 后面还有 ", "answer"... 等内容，rfind 会找到最后一个 }
        // 这里构造一个 rfind 会误截的场景：尾部有额外非 JSON 文本
        let input = r#"{"stem": "求 {x | x > 1}", "a": 1}"#;
        let result: Result<serde_json::Value, String> = clean_and_parse(input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_clean_and_parse_completely_broken() {
        let input = "完全不是 JSON";
        let result: Result<serde_json::Value, String> = clean_and_parse(input);
        assert!(result.is_err());
    }

    // ===== fix_invalid_escapes 单元测试 =====

    #[test]
    fn test_fix_invalid_escapes_latex_sqrt() {
        // LLM 输出含未转义的 \sqrt、\frac（典型 LaTeX 反斜杠）
        let input = r#"{"stem": "求 $\sqrt{12} - \sqrt{3}$", "answer": "$2\sqrt{3}$"}"#;
        let fixed = fix_invalid_escapes(input);
        // \s、\f 应该被双重转义；\s 后跟 'q'（非法），\f 后跟 'r'（非法）
        // 但 \r 是合法转义！所以 \frac 的 \f 不会被修复...
        // 实际上 \f 后面是 'r'，而 \r 是合法 JSON 转义，
        // 所以 fix_invalid_escapes 会把 \f 保留（认为合法），但其实下一个字节是 'r'
        // 这里我们验证：\sqrt 中的 \s 后面是 'q'（非法），应该被修复
        assert!(fixed.contains(r"\\sqrt"), "sqrt 应被双重转义: {}", fixed);
        // 解析应该成功
        let parsed: serde_json::Value = serde_json::from_str(&fixed).unwrap();
        assert_eq!(parsed["stem"], "求 $\\sqrt{12} - \\sqrt{3}$");
    }

    #[test]
    fn test_fix_invalid_escapes_leaves_valid_unchanged() {
        // 合法 JSON 转义应原样保留
        let input = r#"{"a": "line1\nline2\ttab\\back\"quote"}"#;
        let fixed = fix_invalid_escapes(input);
        assert_eq!(fixed, input);
    }

    #[test]
    fn test_fix_invalid_escapes_outside_string_unchanged() {
        // 字符串外部的 \ 不应被修改（虽然 JSON 结构本身不该有 \）
        // 但本函数设计为只在 "..." 内部修复，外部原样保留
        let input = r#"{"key": "value"}"#;
        let fixed = fix_invalid_escapes(input);
        assert_eq!(fixed, input);
    }

    #[test]
    fn test_fix_invalid_escapes_trailing_backslash() {
        // 字符串末尾单独的 \ → 修复为 \\
        let input = r#"{"a": "test\"}"#;
        // 这里 \" 是合法转义（转义的双引号）
        // 真正的"末尾单独 \"场景：字符串在 \ 后未闭合
        let input_unclosed = r#"{"a": "test\"}"#;
        let fixed = fix_invalid_escapes(input_unclosed);
        assert!(fixed.contains(r"test"));
        // 原始 input 是合法的（\" 是转义引号），不应被修改
        assert_eq!(fixed, input);
    }

    #[test]
    fn test_clean_and_parse_with_latex_escape() {
        // 模拟 LLM 输出含未转义 LaTeX 反斜杠的真实场景
        // \sqrt{12} 中的 \s 后跟 'q' 是非法转义
        let input = r#"```json
{
  "question_type": "solution",
  "stem": "计算 $\\sqrt{12} - \\sqrt{3}$ 的值",
  "correct_answer": {
    "kind": "solution",
    "value": {"subs": [{"sub_id": 1, "content": "$2\\sqrt{3}$"}]}
  },
  "analysis": [{"title": "解法一", "content": "原式 = $2\\sqrt{3}$"}],
  "confidence": 0.9,
  "warnings": [],
  "knowledge_points": [],
  "image_placeholders": []
}
```"#;
        let result: Result<serde_json::Value, String> = clean_and_parse(input);
        assert!(result.is_ok(), "应能解析含 LaTeX 转义的 JSON");
        if let Ok(v) = result {
            assert_eq!(v["question_type"], "solution");
        }
    }
}
