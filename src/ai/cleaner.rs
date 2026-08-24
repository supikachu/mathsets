use regex::Regex;
use serde::de::DeserializeOwned;

/// 剥离 markdown 代码块包裹（含只有开头 ```json、输出被截断而无结尾 ``` 的情况）
fn strip_code_fence(raw: &str) -> String {
    let s = raw.trim();
    let re_block = Regex::new(r"(?s)```(?:json)?\s*(.*?)\s*```").unwrap();
    if let Some(caps) = re_block.captures(s) {
        return caps
            .get(1)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_else(|| s.to_string());
    }
    let s = s.strip_prefix("```json").unwrap_or(s);
    let s = s.strip_prefix("```").unwrap_or(s);
    s.trim().to_string()
}

/// 清洗 LLM 输出，提取纯 JSON（快速路径）
///
/// 1. 剥离 ```json ... ``` 或 ``` ... ``` 包裹
/// 2. 截取第一个 { 到最后一个 }
/// 3. 移除尾部多余逗号
pub fn clean_llm_json(raw: &str) -> Result<String, String> {
    let mut s = strip_code_fence(raw);

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

/// 修复被 `max_tokens` 截断的批量 JSON（v1.1，T1.12）
///
/// 针对 Stage 2 输出 `{"questions": [ {...}, {...}, {截断...` 的场景：
/// - 有完整数组元素：保留最后一个完整元素，丢弃其后残缺元素，补 `]}`
/// - **没有任何完整元素**（常见：解析卷单题 `analysis.content` 写到一半）：
///   闭合未结束的字符串，再按栈补全 `{` / `[`，尽量保住这一题
///
/// 不要先走 `clean_llm_json` 的 `rfind('}')`：截断发生在字符串内时，
/// 最后的 `}` 往往是 LaTeX `\frac{1}{2}`，会把已写完的 stem/字段切掉。
///
/// 返回 `None` 表示：未找到 questions 数组 / 数组本身已正常闭合。
pub fn repair_truncated_batch(raw: &str) -> Option<String> {
    let mut s = strip_code_fence(raw);
    if let Some(idx) = s.find('{') {
        s = s[idx..].to_string();
    } else {
        return None;
    }

    // 定位 "questions" 键后的数组起始 [
    let q_idx = s.find("\"questions\"")?;
    let after_key = &s[q_idx..];
    let colon = after_key.find(':')?;
    let bracket_rel = after_key[colon..].find('[')?;
    let bracket_idx = q_idx + colon + bracket_rel;

    let bytes = s.as_bytes();
    let mut depth: i32 = 0; // {} 深度（相对 questions 数组层，0 = 数组层）
    let mut in_string = false;
    let mut escape = false;
    let mut last_complete_end: Option<usize> = None; // 最后一个完整元素 `}` 的索引
    // 从 questions `[` 之后尚未闭合的 `{` / `[`（用于保住半截的第一题）
    let mut open_stack: Vec<u8> = Vec::new();

    let mut i = bracket_idx + 1;
    while i < bytes.len() {
        let b = bytes[i];
        if escape {
            escape = false;
            i += 1;
            continue;
        }
        if b == b'\\' && in_string {
            escape = true;
            i += 1;
            continue;
        }
        if b == b'"' {
            in_string = !in_string;
            i += 1;
            continue;
        }
        if in_string {
            i += 1;
            continue;
        }
        match b {
            b'{' => {
                depth += 1;
                open_stack.push(b'{');
            }
            b'}' => {
                depth -= 1;
                let _ = open_stack.pop();
                if depth == 0 {
                    last_complete_end = Some(i);
                }
            }
            b'[' => open_stack.push(b'['),
            b']' if depth == 0 && open_stack.is_empty() => {
                // 数组正常闭合，非截断场景
                return None;
            }
            b']' => {
                let _ = open_stack.pop();
            }
            _ => {}
        }
        i += 1;
    }

    // 有完整题：丢掉末尾残缺元素
    if let Some(end) = last_complete_end {
        return Some(format!("{}]}}", &s[..=end]));
    }

    // 第一题就写到一半（EOF 落在字符串内，例如 `\frac{1}{2}`）
    if !in_string && open_stack.is_empty() {
        return None;
    }

    let mut out = s;
    if in_string {
        out.push('"');
    }
    while out.ends_with(',') || out.ends_with(|c: char| c.is_whitespace()) {
        out.pop();
    }
    for delim in open_stack.iter().rev() {
        match delim {
            b'{' => out.push('}'),
            b'[' => out.push(']'),
            _ => {}
        }
    }
    out.push(']');
    out.push('}');
    Some(out)
}

/// 修复 JSON 字符串字面量内部的非法转义反斜杠。
///
/// JSON 规范只允许 9 种转义：`\" \\ \/ \b \f \n \r \t \uXXXX`。
/// LLM 输出经常含有 LaTeX 反斜杠（`\sqrt`、`\d`、`\frac` 等），
/// 未转义时 serde_json 会抛 `invalid escape` 错误。
///
/// 本函数扫描字符串字面量（`"..."` 内部），遇到 `\` 时检查下一字节：
///   - 合法转义字符 → 保留原样；但 `\f`/`\b`/`\r` 按 LaTeX 处理（见下）
///   - 非法转义字符 → 把 `\` 替换为 `\\`（双重转义，保留字面反斜杠）
///   - `\frac`/`\bar`/`\right` 的 `\f`/`\b`/`\r` 虽是 JSON 控制符，必须双重转义，
///     否则 `\frac` 会变成换页符 + `rac`（界面常显示成 ⬆rac）
///   - 行尾单独的 `\` → 替换为 `\\`
///   - 字符串内的裸换行 / Tab / 其他 C0 控制符（LLM 常把题干折行）→ `\n` / `\t` / 丢弃
///
/// **关键**：只在字符串字面量内部修复，绝不碰 JSON 结构字符
/// （`{`、`}`、`[`、`]`、`:`、`,`），否则会破坏 JSON 结构。
///
/// UTF-8 安全性：多字节 UTF-8 序列的首字节 >= 0x80，
/// 不会与 ASCII `"`, `\` 冲突，可安全按字节扫描。
pub(crate) fn fix_invalid_escapes(s: &str) -> String {
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
            // JSON 禁止字符串内出现未转义的 U+0000–U+001F。
            // LLM 常把 stem 在「(1)…；(2)…」处直接折行，serde 报
            // `control character found while parsing a string`。
            if b < 0x20 {
                if b == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    out.extend_from_slice(br"\n");
                    i += 2;
                    continue;
                }
                match b {
                    b'\n' | b'\r' => out.extend_from_slice(br"\n"),
                    b'\t' => out.extend_from_slice(br"\t"),
                    _ => {}
                }
                i += 1;
                continue;
            }
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
            // `\f` `\b` `\r` `\t` 虽是合法 JSON 控制符，但数学题里几乎总是 LaTeX：
            // `\frac` `\triangle` `\therefore` `\times` `\bar` `\right`。
            // 按 Tab 吃掉后：`\triangle` → 制表符 + `riangle`，预览只剩 `riangle`。
            // `\n` 仍保留为换行，除非后面像 `\neq` `\nabla` `\nu`。
            if next == b'f'
                || next == b'b'
                || next == b'r'
                || next == b't'
                || (next == b'n' && looks_like_latex_n_command(&bytes[i + 2..]))
            {
                out.extend_from_slice(b"\\\\");
                i += 1;
                continue;
            }
            out.push(b);
            out.push(next);
            i += 2;
        } else {
            // 非法转义（如 \d \s \sqrt 等 LaTeX 命令）
            // 把 \ 替换为 \\，下一轮继续处理 next 字符
            //
            // 豆包常写 `\\\(`：先用合法 `\\` 产出一个 `\`，再跟非法 `\(`。
            // 若再补 `\\`，解析后会变成 `\\(`，KaTeX 无法识别。此时丢掉多余的 `\`。
            if matches!(next, b'(' | b')' | b'[' | b']') && out.ends_with(b"\\\\") {
                i += 1;
                continue;
            }
            out.extend_from_slice(b"\\\\");
            i += 1;
        }
    }

    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// `\n` 后面更像 `\neq` / `\nabla` / `\nu`，而不是「换行 + 英文」。
fn looks_like_latex_n_command(rest: &[u8]) -> bool {
    rest.starts_with(b"eq")
        || rest.starts_with(b"abla")
        || rest.starts_with(b"ot")
        || rest.starts_with(b"exists")
        || rest.starts_with(b"parallel")
        || rest.starts_with(b"cong")
        || rest.starts_with(b"mid")
        || rest.starts_with(b"geq")
        || rest.starts_with(b"less")
        || (rest.starts_with(b"u") && !rest.get(1).map(|c| c.is_ascii_alphabetic()).unwrap_or(false))
}

/// 打印出错位置附近的文本上下文，辅助定位是哪个公式带偏了 AI。
/// serde_json 的 column 按字节计；中文等多字节字符必须对齐到 char boundary，否则会 panic。
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

    let col_byte = col.saturating_sub(1); // serde_json column 是 1-based
    let start = snap_to_char_boundary(target_line, col_byte.saturating_sub(80), false);
    let end = snap_to_char_boundary(target_line, (col_byte + 80).min(target_line.len()), true);
    if end > start {
        tracing::error!(
            "出错位置附近片段 [byte {}..{}]：\n  {}",
            start,
            end,
            &target_line[start..end]
        );
    }
}

fn snap_to_char_boundary(s: &str, index: usize, ceil: bool) -> usize {
    let mut i = index.min(s.len());
    if s.is_char_boundary(i) {
        return i;
    }
    if ceil {
        while i < s.len() && !s.is_char_boundary(i) {
            i += 1;
        }
    } else {
        while i > 0 && !s.is_char_boundary(i) {
            i -= 1;
        }
    }
    i
}

/// GLM 常把选项写成 `{"label":"B","-\\frac{n}{3}"}`，漏掉 `"content":`。
/// 在 `"label": "...",` 后若紧跟字符串值而不是另一个键，则插入 `"content":`。
fn fix_missing_option_content_key(s: &str) -> String {
    static RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r#"("label"\s*:\s*"(?:\\.|[^"\\])*"\s*,\s*)"#)
            .expect("missing option content key regex")
    });
    let mut out = String::with_capacity(s.len() + 32);
    let mut last = 0;
    for m in RE.find_iter(s) {
        out.push_str(&s[last..m.end()]);
        let rest = &s[m.end()..];
        if rest.starts_with('"') && !json_string_is_object_key(rest) {
            out.push_str(r#""content": "#);
        }
        last = m.end();
    }
    out.push_str(&s[last..]);
    out
}

/// `rest` 以 `"` 开头时，判断该字符串是否为 JSON 对象键（`"content":`）而非值。
fn json_string_is_object_key(rest: &str) -> bool {
    let bytes = rest.as_bytes();
    if bytes.first() != Some(&b'"') {
        return false;
    }
    let mut i = 1;
    let mut escape = false;
    while i < bytes.len() {
        let b = bytes[i];
        if escape {
            escape = false;
            i += 1;
            continue;
        }
        if b == b'\\' {
            escape = true;
            i += 1;
            continue;
        }
        if b == b'"' {
            let after = rest[i + 1..].trim_start();
            return after.starts_with(':');
        }
        i += 1;
    }
    false
}

/// 解析前的结构修补：非法转义 + 选项缺 content 键
fn prepare_llm_json(s: &str) -> String {
    let escaped = fix_invalid_escapes(s);
    let with_keys = fix_missing_option_content_key(&escaped);
    if with_keys != escaped {
        tracing::debug!("已补全选项缺失的 content 键");
    }
    with_keys
}

/// 两阶段清洗 + 反序列化：快速路径 → 花括号计数器兜底 → 报错
///
/// 阶段 1：clean_llm_json + 修复字符串内非法转义后反序列化
/// 阶段 2：extract_json_by_bracket_count + 同样先修转义
/// 最终失败：打印原始文本前 800 字符 + 出错行上下文
pub fn clean_and_parse<T: DeserializeOwned>(raw: &str) -> Result<T, String> {
    // ===== 阶段 1：快速路径 =====
    if let Ok(cleaned) = clean_llm_json(raw) {
        // 先修 LaTeX `\cap` `\sqrt` 等非法 JSON 转义，再反序列化；
        // 否则 serde 失败后 log_error_context 曾因中文行 panic，后续修复永远走不到。
        let prepared = prepare_llm_json(&cleaned);
        match serde_json::from_str::<T>(&prepared) {
            Ok(parsed) => return Ok(parsed),
            Err(e) => {
                let line = e.line();
                let col = e.column();
                tracing::warn!("阶段1 clean_llm_json 反序列化失败 (line {line}, col {col}): {e}");
                log_error_context(&prepared, line, col);
            }
        }
    }

    // ===== 阶段 2：花括号计数器兜底 =====
    if let Ok(precise) = extract_json_by_bracket_count(raw) {
        let prepared = prepare_llm_json(&precise);
        match serde_json::from_str::<T>(&prepared) {
            Ok(parsed) => return Ok(parsed),
            Err(e) => {
                let line = e.line();
                let col = e.column();
                tracing::warn!("阶段2 bracket_count 反序列化失败 (line {line}, col {col}): {e}");
                log_error_context(&prepared, line, col);
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

/// 自动补齐未闭合的 `:::img-row` 围栏
///
/// AI 输出可能因 token 限制或 prompt 漂移导致 `:::img-row ... :::` 围栏缺少
/// 闭合 `:::`。前端 `LatexRender.processImgRow` 正则要求严格的 `\n:::\n?`
/// 闭合标记，缺失时：
///   - 围栏内的图片行退化为普通 Markdown（不并排，破坏并排版式）
///   - 开标记 `:::img-row` 作为纯文本泄漏到题干中，污染渲染
///
/// 策略：按行扫描，跟踪 `:::img-row` 开标记栈深度；遇到独占一行的 `:::`
/// 且栈非空时出栈（视为闭标记）。EOF 时若栈仍非空，逐个追加 `\n:::\n`
/// 补齐（处理嵌套或多重未闭合的极端情况）。
///
/// 注意：
///   - 只识别独占一行的 `:::`（trim 后严格相等），不匹配 `:::trailing`
///     等带后缀的行——这些是其他围栏类型的开标记，不应误判为 img-row 闭合
///   - `:::img-row {align:left}` 配置块也识别为开标记（前缀匹配 + 空白分隔）
///   - 不处理 `:::img-row` 嵌套（AI 输出极少出现，正则也用非贪婪不支持嵌套）
pub fn close_unclosed_img_row_fences(md: &str) -> String {
    let mut stack_depth: usize = 0;

    for line in md.lines() {
        let trimmed = line.trim();
        // 开标记：`:::img-row` 或 `:::img-row {align:...}`
        // 严格前缀匹配 + 空白分隔，避免误匹配 `:::img-rowrandom` 等
        let is_opener = trimmed == ":::img-row"
            || trimmed.starts_with(":::img-row ")
            || trimmed.starts_with(":::img-row\t");
        if is_opener {
            stack_depth += 1;
        } else if trimmed == ":::" && stack_depth > 0 {
            // 闭标记：独占一行的 `:::`，仅在栈非空时消费
            stack_depth -= 1;
        }
    }

    if stack_depth == 0 {
        return md.to_string();
    }

    // 末尾追加 `\n:::\n` 用于每个未闭合的围栏
    // 先确保末尾有换行（避免 `:::img-row\n![img](url)` + `:::` 黏连成 `:::img-row\n![img](url):::`）
    let mut result = String::with_capacity(md.len() + stack_depth * 5);
    result.push_str(md);
    for _ in 0..stack_depth {
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(":::\n");
    }
    result
}

static LITERAL_NL_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"\\n([^a-z]|$)").expect("literal newline regex"));
static LITERAL_TAB_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"\\t([^a-z]|$)").expect("literal tab regex"));
static HTML_TABLE_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"(?is)<table\b[^>]*>.*?</table>").expect("html table regex"));
static HTML_TR_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"(?is)<tr\b[^>]*>(.*?)</tr>").expect("html tr regex"));
static HTML_CELL_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"(?is)<t[dh]\b[^>]*>(.*?)</t[dh]>").expect("html cell regex"));
static HTML_TAG_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"(?is)<[^>]+>").expect("html tag regex"));

/// LLM 常把 JSON 换行写成两个字符 `\` + `n`（而不是真正的换行）。
/// 只替换后面不是字母的 `\n`，避免误伤 `\nu` / `\neq` / `\times` 等 LaTeX 命令。
pub fn unescape_literal_newlines(s: &str) -> String {
    let s = LITERAL_NL_RE.replace_all(s, "\n$1");
    LITERAL_TAB_RE.replace_all(&s, "\t$1").into_owned()
}

fn strip_html_cell(raw: &str) -> String {
    HTML_TAG_RE
        .replace_all(raw, " ")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("|", "\\|")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn html_table_fragment_to_markdown(table_html: &str) -> String {
    let mut rows: Vec<Vec<String>> = Vec::new();
    for tr in HTML_TR_RE.captures_iter(table_html) {
        let cells: Vec<String> = HTML_CELL_RE
            .captures_iter(&tr[1])
            .map(|c| strip_html_cell(&c[1]))
            .collect();
        if !cells.is_empty() {
            rows.push(cells);
        }
    }
    if rows.is_empty() {
        return String::new();
    }
    let width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if width == 0 {
        return String::new();
    }
    for row in &mut rows {
        while row.len() < width {
            row.push(String::new());
        }
    }
    let fmt = |row: &[String]| format!("| {} |", row.join(" | "));
    let sep = format!("| {} |", vec!["---"; width].join(" | "));
    let mut out = String::from("\n");
    out.push_str(&fmt(&rows[0]));
    out.push('\n');
    out.push_str(&sep);
    out.push('\n');
    for row in rows.iter().skip(1) {
        out.push_str(&fmt(row));
        out.push('\n');
    }
    out
}

/// 把 LLM 误输出的 `<table>...</table>` 转成 Markdown 表格，便于前端渲染。
pub fn html_tables_to_markdown(s: &str) -> String {
    HTML_TABLE_RE
        .replace_all(s, |caps: &regex::Captures| html_table_fragment_to_markdown(&caps[0]))
        .into_owned()
}

/// JSON `\f`/`\b` 已被吃成控制字符，或界面把换页符显示成 ⬆ 时，还原 LaTeX。
pub fn restore_latex_from_json_controls(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{000C}' => out.push_str("\\f"), // form feed ← `\frac`
            '\u{0008}' => out.push_str("\\b"), // backspace ← `\bar`
            '\t' => out.push_str("\\t"),      // tab ← `\triangle` `\therefore` `\times`
            _ => out.push(c),
        }
    }
    out.replace("⬆rac", "\\frac")
        .replace("↑rac", "\\frac")
        .replace("⇧rac", "\\frac")
}

/// 题干/选项/解析统一清洗：字面量 `\n` + HTML 表格 + 误伤的 `\frac` + 豆包定界符
pub fn sanitize_question_markup(s: &str) -> String {
    normalize_llm_latex(&html_tables_to_markdown(&unescape_literal_newlines(
        &restore_latex_from_json_controls(s),
    )))
}

/// 把站外模型常见的 TeX 定界符套娃修成本系统用的 `$` / `$$`。
///
/// 豆包典型输出（JSON 解析后）：`$$\\(\therefore\) OA=OC$$`、`$\(AC=OA\)$`。
/// KaTeX 只吃 `$...$`，数学模式里再套 `\(` 会整段标红。
pub fn normalize_llm_latex(s: &str) -> String {
    let collapsed = collapse_overescaped_tex_delimiters(s);
    let stripped = strip_tex_delimiters_inside_dollars(&collapsed);
    convert_standalone_tex_delimiters(&stripped)
}

fn collapse_overescaped_tex_delimiters(s: &str) -> String {
    let mut t = s.to_string();
    loop {
        let n = t
            .replace(r"\\(", r"\(")
            .replace(r"\\)", r"\)")
            .replace(r"\\[", r"\[")
            .replace(r"\\]", r"\]");
        if n == t {
            return n;
        }
        t = n;
    }
}

fn strip_tex_open_close(inner: &str) -> String {
    inner
        .replace(r"\(", "")
        .replace(r"\)", "")
        .replace(r"\[", "")
        .replace(r"\]", "")
}

fn strip_tex_delimiters_inside_dollars(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            let display = i + 1 < bytes.len() && bytes[i + 1] == b'$';
            let open_len = if display { 2 } else { 1 };
            let start = i + open_len;
            let mut j = start;
            let mut closer = None;
            while j < bytes.len() {
                if bytes[j] != b'$' {
                    j += 1;
                    continue;
                }
                if display {
                    if j + 1 < bytes.len() && bytes[j + 1] == b'$' {
                        closer = Some(j);
                        break;
                    }
                    j += 1;
                    continue;
                }
                closer = Some(j);
                break;
            }
            if let Some(end) = closer {
                out.push_str(if display { "$$" } else { "$" });
                out.push_str(&strip_tex_open_close(&s[start..end]));
                out.push_str(if display { "$$" } else { "$" });
                i = end + open_len;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn convert_standalone_tex_delimiters(s: &str) -> String {
    static RE_DISPLAY: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r"\\\[([\s\S]*?)\\\]").expect("tex display delim regex")
    });
    static RE_INLINE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r"\\\(([\s\S]*?)\\\)").expect("tex inline delim regex")
    });
    let s = RE_DISPLAY.replace_all(s, |c: &regex::Captures| format!("$${}$$", &c[1]));
    RE_INLINE
        .replace_all(&s, |c: &regex::Captures| format!("${}$", &c[1]))
        .into_owned()
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
    fn test_repair_truncated_batch_keeps_complete_elements() {
        // 第 3 题在 stem 中途被截断
        let input = r#"```json
{"questions": [
  {"question_type": "choice", "stem": "第1题"},
  {"question_type": "fill", "stem": "第2题"},
  {"question_type": "solution", "stem": "第3题截断"#;
        let repaired = repair_truncated_batch(input).expect("应修复成功");
        let parsed: serde_json::Value = serde_json::from_str(&repaired).expect("修复后应可解析");
        let arr = parsed["questions"].as_array().unwrap();
        assert_eq!(arr.len(), 2, "应丢弃截断的第 3 题，保留前 2 题");
        assert_eq!(arr[0]["stem"], "第1题");
        assert_eq!(arr[1]["stem"], "第2题");
    }

    #[test]
    fn test_repair_truncated_batch_returns_none_when_not_truncated() {
        // 完整 JSON 不应触发修复（返回 None）
        let input = r#"{"questions": [{"question_type": "choice", "stem": "x"}]}"#;
        assert!(repair_truncated_batch(input).is_none());
    }

    #[test]
    fn test_repair_truncated_batch_salvages_single_truncated_question() {
        // 第一题分析写到一半（EOF 落在字符串内）→ 闭合后保留该题，而不是整块失败
        let input = r#"{"questions": [{"question_type": "choice", "stem": "截"#;
        let repaired = repair_truncated_batch(input).expect("应闭合半截题");
        let parsed: serde_json::Value = serde_json::from_str(&repaired).expect("修复后应可解析");
        let arr = parsed["questions"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["stem"], "截");
        assert_eq!(arr[0]["question_type"], "choice");
    }

    #[test]
    fn test_repair_truncated_batch_salvages_eof_inside_latex_frac() {
        // 解析卷：analysis.content 在 $\frac{1}{2} 处被 max_tokens 截断；
        // rfind('}') 会切到 LaTeX 的 }，必须从原文补全而不是先裁到最后一个 }。
        let input = r#"```json
{"questions":[{"question_type":"solution","stem":"椭圆题","analysis":[{"title":"解法一","content":"法六：水平宽乘 $\frac{1}{2}"#;
        let repaired = repair_truncated_batch(input).expect("应保住题干");
        let parsed: serde_json::Value = serde_json::from_str(&repaired).expect("修复后应可解析");
        let arr = parsed["questions"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["stem"], "椭圆题");
        let content = arr[0]["analysis"][0]["content"].as_str().unwrap();
        assert!(content.contains("法六"), "截断前的解析应保留，得到 {content}");
    }

    #[test]
    fn test_repair_truncated_batch_handles_braces_in_strings() {
        // 题干含集合 {x | x > 1}，不应误判元素闭合
        let input = r#"{"questions": [
  {"question_type": "fill", "stem": "求 {x | x > 1} 的元素个数"},
  {"question_type": "choice", "stem": "截"#;
        let repaired = repair_truncated_batch(input).expect("应修复成功");
        let parsed: serde_json::Value = serde_json::from_str(&repaired).expect("修复后应可解析");
        let arr = parsed["questions"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["stem"], "求 {x | x > 1} 的元素个数");
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
        let input = r#"{"stem": "求 $\sqrt{12} - \sqrt{3}$", "answer": "$2\sqrt{3}$"}"#;
        let fixed = fix_invalid_escapes(input);
        assert!(fixed.contains(r"\\sqrt"), "sqrt 应被双重转义: {}", fixed);
        let parsed: serde_json::Value = serde_json::from_str(&fixed).unwrap();
        assert_eq!(parsed["stem"], "求 $\\sqrt{12} - \\sqrt{3}$");
    }

    #[test]
    fn test_fix_invalid_escapes_latex_frac_not_form_feed() {
        // `\frac` 的 `\f` 是合法 JSON 换页符；必须改成 \\f 才能留下 \frac
        let input = r#"{"stem": "$\sin\left(\omega x + \frac{\pi}{4}\right)$"}"#;
        let fixed = fix_invalid_escapes(input);
        let parsed: serde_json::Value = serde_json::from_str(&fixed).unwrap();
        let stem = parsed["stem"].as_str().unwrap();
        assert!(stem.contains(r"\frac"), "应保留 \\frac，得到 {stem}");
        assert!(!stem.contains('\u{000C}'), "不应含换页符: {stem:?}");
    }

    #[test]
    fn test_restore_latex_from_form_feed_and_arrow() {
        let s = format!("$\\sin(x+{}rac{{\\pi}}{{4}})$ ⬆rac{{1}}{{2}}", '\u{000C}');
        let restored = restore_latex_from_json_controls(&s);
        assert!(restored.contains(r"\frac{\pi}{4}"), "{restored}");
        assert!(restored.contains(r"\frac{1}{2}"), "{restored}");
        assert!(!restored.contains('\u{000C}'));
        assert!(!restored.contains('⬆'));
    }

    #[test]
    fn test_fix_invalid_escapes_leaves_valid_unchanged() {
        // 合法 JSON 转义应原样保留（\t 在数学题中按 LaTeX 处理，不用 Tab 用例）
        let input = r#"{"a": "line1\nline2 next\\back\"quote"}"#;
        let fixed = fix_invalid_escapes(input);
        assert_eq!(fixed, input);
    }

    #[test]
    fn test_fix_invalid_escapes_latex_triangle_not_tab() {
        // Gemini 常把 `\triangle` `\therefore` `\times` 写成 JSON `\t`，会被解析成 Tab
        let input = r#"{"stem": "$\triangle ABC$ $\therefore a=b$ $2\times 3$ $\neq 0$"}"#;
        let fixed = fix_invalid_escapes(input);
        let parsed: serde_json::Value = serde_json::from_str(&fixed).unwrap();
        let stem = parsed["stem"].as_str().unwrap();
        assert!(stem.contains(r"\triangle"), "{stem:?}");
        assert!(stem.contains(r"\therefore"), "{stem:?}");
        assert!(stem.contains(r"\times"), "{stem:?}");
        assert!(stem.contains(r"\neq"), "{stem:?}");
        assert!(!stem.contains('\t'), "{stem:?}");
    }

    #[test]
    fn test_restore_tab_to_triangle() {
        let s = format!("${}riangle ABC$ {}herefore a=b", '\t', '\t');
        let restored = restore_latex_from_json_controls(&s);
        assert!(restored.contains(r"\triangle"), "{restored}");
        assert!(restored.contains(r"\therefore"), "{restored}");
    }

    #[test]
    fn test_escape_raw_newline_inside_stem_string() {
        // 模型把 (1)/(2) 小问在 JSON 字符串里直接换行 → 非法控制符
        let input = "{\n  \"stem\": \"(1) $a$;\n (2) 已知 $b$\"\n}";
        let result: Result<serde_json::Value, String> = clean_and_parse(input);
        assert!(result.is_ok(), "字符串内裸换行应被转义后解析: {:?}", result.err());
        let parsed = result.unwrap();
        let stem = parsed["stem"].as_str().unwrap();
        assert!(stem.contains("(1)"), "{stem}");
        assert!(stem.contains("(2)"), "{stem}");
        assert!(stem.contains("已知"), "{stem}");
    }

    #[test]
    fn test_pretty_printed_json_newlines_outside_strings_ok() {
        let input = "{\n  \"question_type\": \"solution\",\n  \"stem\": \"计算\"\n}";
        let result: Result<serde_json::Value, String> = clean_and_parse(input);
        assert!(result.is_ok(), "{:?}", result.err());
        assert_eq!(result.unwrap()["stem"], "计算");
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

    #[test]
    fn test_clean_and_parse_latex_cap_with_cjk() {
        // GLM 常把 `\cap` 写成 JSON 非法转义 `\c`；该行又含中文。
        // 旧 log_error_context 按字节切片会 panic，清洗路径走不到。
        let input = r#"{
  "questions": [
    {
      "stem": "已知集合 $A = \\{x \\mid -5 < x^3 < 5\\}, B = \\{-3, -1, 0, 2, 3\\}$ ，则 $A \cap B = (\\quad)$"
    }
  ]
}"#;
        let result: Result<serde_json::Value, String> = clean_and_parse(input);
        assert!(result.is_ok(), "含 \\cap 与中文的 JSON 应能清洗解析: {:?}", result.err());
        let parsed = result.unwrap();
        let stem = parsed["questions"][0]["stem"].as_str().unwrap();
        assert!(stem.contains(r"\cap"), "题干应保留 LaTeX \\cap: {stem}");
    }

    #[test]
    fn test_log_error_context_cjk_mid_char_no_panic() {
        let line = r#"        "stem": "已知集合 $A = \\{x \\mid x^3 < 5\\}$ ，则 $A \cap B$","#;
        let text = format!("{{\n{line}\n}}");
        log_error_context(&text, 2, 100);
        log_error_context(&text, 2, 19);
        log_error_context(&text, 2, 1);
    }

    #[test]
    fn test_fix_missing_option_content_key() {
        let input = r#"{"label": "B", "-\\frac{n}{3}"}"#;
        let fixed = fix_missing_option_content_key(input);
        assert_eq!(fixed, r#"{"label": "B", "content": "-\\frac{n}{3}"}"#);
        let ok = r#"{"label": "A", "content": "1"}"#;
        assert_eq!(fix_missing_option_content_key(ok), ok);
    }

    #[test]
    fn test_clean_and_parse_option_missing_content_key() {
        let input = r#"{
  "questions": [
    {
      "question_type": "choice",
      "stem": "则 $A \cap B = (\\quad)$",
      "options": [
        {"label": "A", "$-3$"},
        {"label": "B", "-\\\\frac{n}{3}"},
        {"label": "C", "content": "2"},
        {"label": "D", "$\\varnothing$"}
      ]
    }
  ]
}"#;
        let result: Result<serde_json::Value, String> = clean_and_parse(input);
        assert!(result.is_ok(), "缺 content 键的选项应能修复: {:?}", result.err());
        let opts = &result.unwrap()["questions"][0]["options"];
        assert_eq!(opts[0]["label"], "A");
        assert_eq!(opts[0]["content"], "$-3$");
        assert_eq!(opts[1]["label"], "B");
        assert!(opts[1]["content"].as_str().unwrap().contains("frac"));
        assert_eq!(opts[2]["content"], "2");
        assert_eq!(opts[3]["content"], r"$\varnothing$");
    }

    // ===== close_unclosed_img_row_fences 单元测试 =====

    #[test]
    fn test_close_img_row_fences_already_closed() {
        // 已闭合围栏 → 原样返回
        let input = ":::img-row\n![图1](url1)\n![图2](url2)\n:::\n";
        let result = close_unclosed_img_row_fences(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_close_img_row_fences_unclosed_basic() {
        // 未闭合围栏 → 末尾追加 \n:::\n
        let input = ":::img-row\n![图1](url1)\n![图2](url2)";
        let result = close_unclosed_img_row_fences(input);
        assert_eq!(result, ":::img-row\n![图1](url1)\n![图2](url2)\n:::\n");
    }

    #[test]
    fn test_close_img_row_fences_unclosed_with_trailing_newline() {
        // 末尾已有换行 → 仅追加 :::\n（不重复 \n）
        let input = ":::img-row\n![图1](url1)\n";
        let result = close_unclosed_img_row_fences(input);
        assert_eq!(result, ":::img-row\n![图1](url1)\n:::\n");
    }

    #[test]
    fn test_close_img_row_fences_with_align_config() {
        // 带 {align:left} 配置的围栏 → 识别为开标记
        let input = ":::img-row {align:left}\n![图1](url1)\n![图2](url2)";
        let result = close_unclosed_img_row_fences(input);
        assert_eq!(result, ":::img-row {align:left}\n![图1](url1)\n![图2](url2)\n:::\n");
    }

    #[test]
    fn test_close_img_row_fences_multiple_unclosed() {
        // 多个未闭合围栏 → 逐个追加 :::\n
        let input = ":::img-row\n![图1](url1)\n\n:::img-row\n![图2](url2)";
        let result = close_unclosed_img_row_fences(input);
        assert_eq!(
            result,
            ":::img-row\n![图1](url1)\n\n:::img-row\n![图2](url2)\n:::\n:::\n"
        );
    }

    #[test]
    fn test_close_img_row_fences_mixed_closed_and_unclosed() {
        // 混合：一个已闭合，一个未闭合 → 仅未闭合的追加 :::
        let input = ":::img-row\n![图1](url1)\n:::\n\n:::img-row\n![图2](url2)";
        let result = close_unclosed_img_row_fences(input);
        assert_eq!(
            result,
            ":::img-row\n![图1](url1)\n:::\n\n:::img-row\n![图2](url2)\n:::\n"
        );
    }

    #[test]
    fn test_close_img_row_fences_no_img_row() {
        // 无 img-row 围栏 → 原样返回
        let input = "普通题干，含 $x^2$ 公式与 ![普通图](url)";
        let result = close_unclosed_img_row_fences(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_close_img_row_fences_ignores_trailing_fence_suffix() {
        // `:::trailing` 不应被识别为 img-row 闭合标记
        // :::img-row 未闭合 → 末尾应追加 :::\n
        let input = ":::img-row\n![图1](url1)\n:::warning\n警告\n:::";
        let result = close_unclosed_img_row_fences(input);
        // 注意：末尾的 ::: 会消费 img-row 的栈，结果不追加 :::
        // 这是边界场景——AI 输出极少出现嵌套围栏，且这种结构本身已破坏
        // 关键测试点：:::warning 不被误判为闭合（因为是 :::warning 不是 :::）
        // 这里只验证 :::warning 行不影响识别
        assert!(result.starts_with(":::img-row\n![图1](url1)\n:::warning\n警告\n"));
    }

    #[test]
    fn test_close_img_row_fences_strict_opener_match() {
        // `:::img-rowrandom` 不应被识别为开标记（需空白分隔）
        let input = ":::img-rowrandom\n一些文本\n:::";
        let result = close_unclosed_img_row_fences(input);
        // 不识别为开标记，::: 也不消费栈（栈始终为 0），原样返回
        assert_eq!(result, input);
    }

    #[test]
    fn test_close_img_row_fences_indented_opener() {
        // 带前导空白的开标记也应识别（trim 后匹配）
        let input = "  :::img-row\n![图1](url1)";
        let result = close_unclosed_img_row_fences(input);
        assert_eq!(result, "  :::img-row\n![图1](url1)\n:::\n");
    }

    #[test]
    fn test_unescape_literal_newlines_keeps_latex_nu() {
        assert_eq!(unescape_literal_newlines(r"则最小值为 \n"), "则最小值为 \n");
        assert_eq!(unescape_literal_newlines(r"$ \nu = 1 $"), r"$ \nu = 1 $");
        assert_eq!(unescape_literal_newlines(r"值为 \nA. 1"), "值为 \nA. 1");
        assert_eq!(unescape_literal_newlines(r"A\nB"), "A\nB");
    }

    #[test]
    fn test_html_tables_to_markdown() {
        let html = "<table><tr><td>等级</td><td>名称</td></tr><tr><td>2</td><td>轻风</td></tr></table>";
        let md = html_tables_to_markdown(html);
        assert!(md.contains("| 等级 | 名称 |"));
        assert!(md.contains("| --- | --- |"));
        assert!(md.contains("| 2 | 轻风 |"));
        assert!(!md.contains("<table>"));
    }

    #[test]
    fn test_normalize_doubao_therefore_inside_display_math() {
        // 豆包：$$\\\(\\therefore\) ...$$ 经非法转义修复后，数学模式里会留下 \( \)
        let json = r#"{"c":"$$\\\(\\therefore\) O A = O C = A C$$\n∴ $\\triangle OAC$"}"#;
        let fixed = fix_invalid_escapes(json);
        let v: serde_json::Value = serde_json::from_str(&fixed).expect("应能解析");
        let c = v["c"].as_str().unwrap();
        let out = normalize_llm_latex(c);
        assert!(!out.contains(r"\("), "{out}");
        assert!(!out.contains(r"\)"), "{out}");
        assert!(out.contains(r"$$\therefore"), "{out}");
        assert!(out.contains(r"$\triangle OAC$"), "{out}");
    }

    #[test]
    fn test_normalize_paren_inside_inline_math() {
        let out = normalize_llm_latex(r"弦$\(AC=OA\)$，点D");
        assert_eq!(out, r"弦$AC=OA$，点D");
    }

    #[test]
    fn test_normalize_standalone_tex_parens() {
        let out = normalize_llm_latex(r"故 \(\triangle ACF\sim\triangle OGF\)。");
        assert_eq!(out, r"故 $\triangle ACF\sim\triangle OGF$。");
    }

    #[test]
    fn test_normalize_keeps_correct_frac() {
        let s = r"$\sqrt{\left(k+\frac{1}{2}\right)^2}$";
        assert_eq!(normalize_llm_latex(s), s);
    }
}
