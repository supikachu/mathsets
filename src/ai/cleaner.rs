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

/// 两阶段清洗 + 反序列化：快速路径 → 花括号计数器兜底 → 报错
pub fn clean_and_parse<T: DeserializeOwned>(raw: &str) -> Result<T, String> {
    // 阶段 1：快速路径（rfind 截取）
    if let Ok(cleaned) = clean_llm_json(raw) {
        if let Ok(parsed) = serde_json::from_str::<T>(&cleaned) {
            return Ok(parsed);
        }
    }
    // 阶段 2：花括号计数器兜底
    if let Ok(precise) = extract_json_by_bracket_count(raw) {
        if let Ok(parsed) = serde_json::from_str::<T>(&precise) {
            return Ok(parsed);
        }
    }
    Err("AI 返回 JSON 反序列化失败（两条清洗路径均未通过）".into())
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
}
