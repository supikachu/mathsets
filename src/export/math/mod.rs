//! LaTeX → Presentation MathML（T2.2）
//!
//! MathML → OMML 的下一级转换在 [`omml`] 子模块（T2.4）。
//!
//! 这一层的职责只有一个：让 `latex2mathml` 吃得下教辅里的实际写法。三件事都做在进库转换之前，
//! 因此 Word / PDF 两条公式管线共享同一份归一结果。
//!
//! 1. **参数级改写**：`\textcolor{red}{x}` / `\boxed{…}` 取参数内容、`\phantom{…}` 整段删除、
//!    `\substack{a\\b}` 改写为 matrix —— 这些 crate 完全不认，但不改写会把排版信息一起丢掉。
//! 2. **命令别名**：crate 认 `\geq` 不认 `\ge`、认 `\emptyset` 不认 `\varnothing`（方向与前端的
//!    KaTeX 相反），\dfrac/\tfrac/\dots/\lg/… 一律折算成 crate 支持的等价写法，语义不变。
//! 3. **环境改写**：crate 支持 `matrix`/`pmatrix`/`bmatrix`/`vmatrix`/`align`（且 `matrix` 接受 `&`
//!    对齐列与 `\text` 条件），但不支持 `cases`/`aligned`/`array`/`gathered`/`split`/… ——
//!    统一改写为 `matrix`，需要大括号的补 `\left\{ … \right.` / `\left. … \right\}`。
//!
//! 改写的代价是列对齐语义退化（`array{l}` 的左对齐变成 matrix 的居中），属可接受取舍：
//! 比整条公式降级成红色原文更接近原样。
//!
//! 输出侧还有一道 **XML 修复**：crate 把公式里的裸 `<` 原样写进文本节点（`x<0` →
//! `<mo><</mo>`），产出的串根本不是良构 XML，而官方 XSLT 与 roxmltree 都要求良构输入 ——
//! `escape_stray_markup` 按 MathML 标签白名单把非标签的 `<` / `&` 转义掉。
//!
//! **降级判定必须看输出内容**：`latex2mathml` 对不认的命令不返回 `Err`，而是把
//! `[PARSE ERROR: Undefined("Command(\"ge\")")]` 当成 `<mtext>` 塞进结果里 —— 只看 `Result`
//! 会让这串文本直接印进教师的 Word。故 [`to_mathml`] 把含有该标记的输出判为失败。

pub mod omml;

use std::borrow::Cow;

const BEGIN: &str = r"\begin{";
const END: &str = r"\end{";
const MATRIX: &str = "matrix";

/// 需补前置左花括号的环境（分段函数）
const LEFT_BRACE_ENVS: &[&str] = &["cases", "dcases"];
/// 需补后置右花括号的环境（右花括号分段）
const RIGHT_BRACE_ENVS: &[&str] = &["rcases"];
/// 其余不支持的环境：原位改写为 matrix 即可（定界符通常已由外层 `\left\{ … \right.` 提供）
const PLAIN_ENVS: &[&str] = &[
    "aligned",
    "aligned*",
    "align*",
    "alignat",
    "alignat*",
    "alignedat",
    "array",
    "array*",
    "gathered",
    "gather",
    "gather*",
    "split",
    "eqnarray",
    "eqnarray*",
    "subarray",
    "smallmatrix",
];
/// LaTeX 语法里带列描述符参数 `{...}` 的环境（改写为 matrix 时须剥掉）
const COLUMN_SPEC_ENVS: &[&str] = &[
    "array",
    "array*",
    "subarray",
    "alignat",
    "alignat*",
    "alignedat",
    "tabular",
];

/// crate 不认、但存在等价写法的命令：`（原名, 替换串）`，原名不带反斜杠。
/// 只收录**语义不变**的折算；会丢信息的写法（`\nleq`、`\cancel` 的斜线）宁可降级也不静默改写。
const COMMAND_ALIASES: &[(&str, &str)] = &[
    ("ge", r"\geq"),
    ("le", r"\leq"),
    ("ne", r"\neq"),
    ("lt", "<"),
    ("gt", ">"),
    ("dfrac", r"\frac"),
    ("tfrac", r"\frac"),
    ("cfrac", r"\frac"),
    ("displaystyle", ""),
    ("textstyle", ""),
    ("scriptstyle", ""),
    ("scriptscriptstyle", ""),
    ("limits", ""),
    ("nolimits", ""),
    // 手工定界符尺寸提示：crate 只认 \left…\right，丢掉 big/Big 系列最多退化成普通尺寸括号
    ("big", ""),
    ("Big", ""),
    ("bigg", ""),
    ("Bigg", ""),
    ("bigl", ""),
    ("bigr", ""),
    ("Bigl", ""),
    ("Bigr", ""),
    ("biggl", ""),
    ("biggr", ""),
    ("Biggl", ""),
    ("Biggr", ""),
    ("varnothing", r"\emptyset"),
    ("empty", r"\emptyset"),
    ("measuredangle", r"\angle"),
    ("sphericalangle", r"\angle"),
    ("neg", r"\lnot"),
    ("dots", r"\ldots"),
    ("prime", "'"),
    ("textrm", r"\mathrm"),
    ("textnormal", r"\mathrm"),
    ("mathsf", r"\mathrm"),
    ("mathtt", r"\mathrm"),
    ("mathcal", r"\mathrm"),
    ("lg", r"\operatorname{lg}"),
    ("lb", r"\operatorname{lb}"),
    ("gcd", r"\operatorname{gcd}"),
    ("lcm", r"\operatorname{lcm}"),
    ("stackrel", r"\overset"),
    ("overparen", r"\overbrace"),
    ("overgroup", r"\overbrace"),
    ("underparen", r"\underbrace"),
    ("undergroup", r"\underbrace"),
    ("ast", "*"),
    ("vert", "|"),
    ("lvert", "|"),
    ("rvert", "|"),
    ("lbrace", r"\{"),
    ("rbrace", r"\}"),
    ("lbrack", "["),
    ("rbrack", "]"),
    ("degree", r"^{\circ}"),
    ("celsius", r"^{\circ}"),
    ("relbar", r"\to"),
    ("Relbar", r"\Rightarrow"),
    ("implies", r"\Rightarrow"),
];

/// 需要先读出 `{...}` 参数才能改写的命令
#[derive(Debug, Clone, Copy)]
enum ArgRule {
    /// 读 `n` 个参数，用第 `i` 个的内容替换整段（`\textcolor{red}{x}` → `x`）
    KeepArg(usize, usize),
    /// 读 `n` 个参数后整段删除（`\hspace{1mm}`）
    Drop(usize),
    /// 读 1 个参数改写为 `\begin{matrix}…\end{matrix}`（`\substack{a\\b}`）
    ToMatrix,
}

const ARG_RULES: &[(&str, ArgRule)] = &[
    ("textcolor", ArgRule::KeepArg(2, 1)),
    ("href", ArgRule::KeepArg(2, 1)),
    ("boxed", ArgRule::KeepArg(1, 0)),
    ("fbox", ArgRule::KeepArg(1, 0)),
    ("framebox", ArgRule::KeepArg(1, 0)),
    ("mathop", ArgRule::KeepArg(1, 0)),
    ("mathrel", ArgRule::KeepArg(1, 0)),
    ("mathbin", ArgRule::KeepArg(1, 0)),
    ("mathpunct", ArgRule::KeepArg(1, 0)),
    ("substack", ArgRule::ToMatrix),
    ("phantom", ArgRule::Drop(1)),
    ("hphantom", ArgRule::Drop(1)),
    ("vphantom", ArgRule::Drop(1)),
    ("mathstrut", ArgRule::Drop(0)),
    ("hspace", ArgRule::Drop(1)),
    ("kern", ArgRule::Drop(1)),
];

/// 公式转换结果
///
/// 失败不 panic、不往调用栈上层抛错：调用方（docx / markdown / typst 生成器）必须降级为
/// 「原文 + 警告」，绝不让单题失败中断整卷（实施计划 §5.3 容错约定）。
/// [`to_mathml`] 与 [`omml::to_omml`] 两级转换共用同一约定。
#[derive(Debug, Clone, PartialEq)]
pub enum MathOutcome {
    /// 转换产物：[`to_mathml`] 给 Presentation MathML，[`omml::to_omml`] 给 OMML 片段
    Ok(String),
    /// 降级原因（进 `X-Export-Warnings` 的 reason 字段）
    Failed(String),
}

impl MathOutcome {
    pub fn is_ok(&self) -> bool {
        matches!(self, MathOutcome::Ok(_))
    }
}

/// 归一并转换。`display = true` 对应 `$$…$$` / `\[…\]`，false 对应 `$…$` / `\(…\)`
pub fn to_mathml(latex: &str, display: bool) -> MathOutcome {
    let src = normalize(latex);
    if src.trim().is_empty() {
        return MathOutcome::Failed("公式内容为空".to_string());
    }
    let style = if display {
        latex2mathml::DisplayStyle::Block
    } else {
        latex2mathml::DisplayStyle::Inline
    };
    match latex2mathml::latex_to_mathml(src.as_ref(), style) {
        Ok(mathml) => match parse_error_reason(&mathml) {
            Some(reason) => MathOutcome::Failed(reason),
            None => MathOutcome::Ok(escape_stray_markup(&mathml).into_owned()),
        },
        // crate 的错误串两端带裸引号，且不带任何中文上下文；这条要直接给教师看
        Err(e) => MathOutcome::Failed(format!(
            "公式无法解析：{}",
            e.to_string().trim_matches('"')
        )),
    }
}

/// 下游（官方 XSLT、roxmltree）都要求良构 XML，而 crate 会把公式里的裸 `<` / `&` 原样写进文本
/// 节点（`x<0` → `<mo><</mo>`）。按 MathML 标签白名单判别：不是标签开头的 `<` 一律转义。
///
/// crate 还会把 `\langle` / `\rangle` 直接写成 HTML 实体 `&lang;` / `&rang;`（`token.rs` 里
/// `Token::Paren("&lang;")`）—— 非 XML 预定义实体，严格解析器直接报错，故就地折成数字引用。
fn escape_stray_markup(mathml: &str) -> Cow<'_, str> {
    let bytes = mathml.as_bytes();
    let mut out: Option<String> = None;
    let mut copied = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        // （替换串, 消费的字节数）；None 表示该处本来就良构
        let fix = match bytes[i] {
            b'<' if !starts_known_tag(mathml, i) => Some(("&lt;", 1)),
            b'&' if !starts_entity(mathml, i) => match crate_entity(mathml, i) {
                Some((repl, len)) => Some((repl, len)),
                None => Some(("&amp;", 1)),
            },
            _ => None,
        };
        let Some((repl, consumed)) = fix else {
            i += 1;
            continue;
        };
        let buf = out.get_or_insert_with(|| String::with_capacity(mathml.len() + 16));
        buf.push_str(&mathml[copied..i]);
        buf.push_str(repl);
        copied = i + consumed;
        i = copied;
    }
    match out {
        Some(mut buf) => {
            buf.push_str(&mathml[copied..]);
            Cow::Owned(buf)
        }
        None => Cow::Borrowed(mathml),
    }
}

/// crate 独有的 HTML 实体 → XML 数字引用（`（实体名不含 &，替换串, 消费字节数）`）
const CRATE_HTML_ENTITIES: &[(&str, &str)] = &[("lang;", "&#x27E8;"), ("rang;", "&#x27E9;")];

/// `at` 处（`&`）是否为 [`CRATE_HTML_ENTITIES`] 之一
fn crate_entity(s: &str, at: usize) -> Option<(&'static str, usize)> {
    let rest = &s[at + 1..];
    CRATE_HTML_ENTITIES
        .iter()
        .find(|(name, _)| rest.starts_with(name))
        .map(|(name, repl)| (*repl, name.len() + 1))
}

/// `at` 处是否为已知 MathML 标签的开头
fn starts_known_tag(s: &str, at: usize) -> bool {
    let rest = &s[at + 1..];
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.')))
        .unwrap_or(rest.len());
    if end == 0 || !MATHML_TAGS.contains(&&rest[..end]) {
        return false;
    }
    matches!(
        rest.as_bytes().get(end),
        None | Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') | Some(b'/')
    )
}

/// `at` 处是否为实体引用 `&name;` / `&#123;` / `&#x1F;`
fn starts_entity(s: &str, at: usize) -> bool {
    let rest = &s[at + 1..];
    let end = match rest.find(';') {
        Some(e) => e,
        None => return false,
    };
    let body = &rest[..end];
    if body.is_empty() || body.len() > 10 {
        return false;
    }
    match body.strip_prefix('#') {
        Some(hex) => match hex.strip_prefix(['x', 'X'].as_ref()) {
            Some(d) => !d.is_empty() && d.bytes().all(|b| b.is_ascii_hexdigit()),
            None => !body[1..].is_empty() && body[1..].bytes().all(|b| b.is_ascii_digit()),
        },
        None => XML_ENTITIES.contains(&body),
    }
}

/// XML 预定义实体：除此之外，`&name;` 在严格解析器里都是非法（crate 的 `&lang;` 由
/// [`crate_entity`] 就地折成数字引用，其余按裸 `&` 转义成 `&amp;`）
const XML_ENTITIES: &[&str] = &["amp", "lt", "gt", "quot", "apos"];

/// crate 可能产出的 MathML 元素名（含它自带的错误包装 `merror`）
const MATHML_TAGS: &[&str] = &[
    "annotation",
    "annotation-xml",
    "maction",
    "math",
    "menclose",
    "merror",
    "mfenced",
    "mfrac",
    "mi",
    "mmultiscripts",
    "mn",
    "mo",
    "mover",
    "mpadded",
    "mphantom",
    "mprescripts",
    "mroot",
    "mrow",
    "ms",
    "mspace",
    "msqrt",
    "mstyle",
    "msub",
    "msubsup",
    "msup",
    "mtable",
    "mtd",
    "mtext",
    "mtr",
    "munder",
    "munderover",
    "none",
    "semantics",
];

/// 归一公式源串：参数级改写 + 命令别名 + 不支持环境改写
pub fn normalize(latex: &str) -> Cow<'_, str> {
    let staged = rewrite_arg_commands(latex);
    let staged = rename_commands(&staged);
    if staged.contains(BEGIN) {
        return Cow::Owned(rewrite_envs(&staged));
    }
    // 三步都没改动时才敢零拷贝回源串（中间量是局部值，不能逃出本函数）
    if staged == latex {
        Cow::Borrowed(latex)
    } else {
        Cow::Owned(staged.into_owned())
    }
}

/// crate 把不认的写法作为文本节点塞进输出（而非返回 Err）的标记
const PARSE_ERROR_MARK: &str = "[PARSE ERROR: ";

/// 从输出里提取可读的降级原因；不含错误标记时返回 None
fn parse_error_reason(mathml: &str) -> Option<String> {
    let mut notes: Vec<String> = Vec::new();
    let mut pos = 0usize;
    while let Some(found) = mathml[pos..].find(PARSE_ERROR_MARK) {
        let start = pos + found + PARSE_ERROR_MARK.len();
        let end = mathml[start..]
            .find(']')
            .map(|e| start + e)
            .unwrap_or(mathml.len());
        let note = prettify_parse_error(&mathml[start..end]);
        if !notes.contains(&note) {
            notes.push(note);
        }
        pos = end;
    }
    if notes.is_empty() {
        return None;
    }
    let shown = notes.iter().take(3).cloned().collect::<Vec<_>>().join("、");
    Some(if notes.len() > 3 {
        format!("{shown} 等 {} 类", notes.len())
    } else {
        shown
    })
}

/// `Undefined("Command(\"ge\")")` → `不支持的命令 \ge`
fn prettify_parse_error(note: &str) -> String {
    if let Some(name) = token_after(note, "Command(") {
        return format!("不支持的命令 \\{name}");
    }
    if let Some(name) = token_after(note, "Environment(") {
        return format!("不支持的环境 {name}");
    }
    note.to_string()
}

/// 取 `needle` 后第一个被 `\"…\"` 包裹的词（crate 的 Debug 输出形态）
fn token_after(hay: &str, needle: &str) -> Option<String> {
    let at = hay.find(needle)? + needle.len();
    let rest = hay[at..].trim_start_matches(['\\', '"']);
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '*' || c == '_'))
        .unwrap_or(rest.len());
    if end == 0 {
        None
    } else {
        Some(rest[..end].to_string())
    }
}

/// 参数级改写（`\textcolor{red}{x}` → `x`、`\substack{a\\b}` → matrix）
fn rewrite_arg_commands(src: &str) -> Cow<'_, str> {
    if !ARG_RULES.iter().any(|(n, _)| src.contains(n)) {
        return Cow::Borrowed(src);
    }
    let mut s = src.to_string();
    let mut cursor = 0usize;
    loop {
        let Some((start, name, name_end)) = next_command(&s, cursor) else {
            break;
        };
        let edit = ARG_RULES
            .iter()
            .find(|(n, _)| *n == name)
            .and_then(|(_, rule)| apply_arg_rule(&s, name_end, *rule));
        match edit {
            Some((repl, span_end)) => {
                s.replace_range(start..span_end, &repl);
                // 替换进来的内容可能仍含待改写命令，从原位继续扫
                cursor = start;
            }
            None => cursor = name_end,
        }
    }
    Cow::Owned(s)
}

/// 按规则读出参数，返回（替换串, 原段落结束位置）；参数形态不合预期时返回 None
fn apply_arg_rule(s: &str, from: usize, rule: ArgRule) -> Option<(String, usize)> {
    let (argc, keep) = match rule {
        ArgRule::KeepArg(argc, keep) => (argc, Some(keep)),
        ArgRule::Drop(argc) => (argc, None),
        ArgRule::ToMatrix => (1, Some(0)),
    };
    let (span_end, args) = read_braced_args(s, from, argc)?;
    let replacement = match keep {
        Some(i) => {
            let (a, b) = args[i];
            let body = &s[a..b];
            match rule {
                ArgRule::ToMatrix => format!(r"\begin{{{MATRIX}}}{body}\end{{{MATRIX}}}"),
                _ => body.to_string(),
            }
        }
        None => String::new(),
    };
    Some((replacement, span_end))
}

/// 从 `from` 起读 `argc` 个 `{…}`（允许参数间空白），返回（结束位置, 各参数内容区间）
fn read_braced_args(s: &str, from: usize, argc: usize) -> Option<(usize, Vec<(usize, usize)>)> {
    let mut i = from;
    let mut args = Vec::with_capacity(argc);
    for _ in 0..argc {
        while s[i..].starts_with(' ') || s[i..].starts_with('\t') || s[i..].starts_with('\n') {
            i += 1;
        }
        if !s[i..].starts_with('{') {
            return None;
        }
        i += 1;
        let body_start = i;
        let mut depth = 1usize;
        let bytes = s.as_bytes();
        while i < bytes.len() {
            match bytes[i] {
                // `\{` / `\}` 是字面花括号，不计入嵌套深度
                b'\\' => i += 2,
                b'{' => {
                    depth += 1;
                    i += 1;
                }
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    i += 1;
                }
                _ => i += 1,
            }
            while i < bytes.len() && !s.is_char_boundary(i) {
                i += 1;
            }
        }
        if depth != 0 {
            return None;
        }
        args.push((body_start, i));
        i += 1;
    }
    Some((i, args))
}

/// 命令别名折算（按 token 边界匹配，`\ge` 不会命中 `\geq`）
fn rename_commands(src: &str) -> Cow<'_, str> {
    let mut out: Option<String> = None;
    let mut copied = 0usize;
    let mut cursor = 0usize;
    while let Some((start, name, end)) = next_command(src, cursor) {
        cursor = end;
        let Some((_, to)) = COMMAND_ALIASES.iter().find(|(n, _)| *n == name) else {
            continue;
        };
        let mut buf = out
            .take()
            .unwrap_or_else(|| String::with_capacity(src.len() + 16));
        buf.push_str(&src[copied..start]);
        buf.push_str(to);
        copied = end;
        out = Some(buf);
    }
    match out {
        Some(mut buf) => {
            buf.push_str(&src[copied..]);
            Cow::Owned(buf)
        }
        None => Cow::Borrowed(src),
    }
}

/// 从 `from` 起找下一个 `\字母…` 命令，返回（反斜杠位置, 命令名, 名字结束位置）
fn next_command(s: &str, from: usize) -> Option<(usize, &str, usize)> {
    let bytes = s.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut j = start;
        while j < bytes.len() && (bytes[j].is_ascii_alphabetic() || bytes[j] == b'*') {
            j += 1;
        }
        if j > start {
            return Some((i, &s[start..j], j));
        }
        i += 1;
    }
    None
}

/// 扫描并改写不支持的环境；支持的环境原样保留（内容仍递归处理）
fn rewrite_envs(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(begin_at) = rest.find(BEGIN) {
        out.push_str(&rest[..begin_at]);
        let after = &rest[begin_at + BEGIN.len()..];
        let Some(close) = after.find('}') else {
            // `\begin{` 后无 `}`：畸形输入，原样交给 latex2mathml（大概率记警告）
            out.push_str(&rest[begin_at..]);
            return out;
        };
        let name = &after[..close];
        let header_end = begin_at + BEGIN.len() + close + 1;
        let Some((content_start, content_end, block_end)) = find_env_block(rest, header_end, name)
        else {
            // `\begin` / `\end` 不配对：保留本段头部后继续扫描，保证循环一定有进展
            out.push_str(&rest[begin_at..header_end]);
            rest = &rest[header_end..];
            continue;
        };
        if needs_rewrite(name) {
            let rewritten = rewrite_envs(&rest[content_start..content_end]);
            let inner = strip_column_spec(name, rewritten.trim());
            let opened = out.trim_end().ends_with(r"\left\{");
            emit_matrix(&mut out, name, inner, opened, &rest[block_end..]);
        } else {
            out.push_str(&rest[begin_at..content_start]);
            out.push_str(&rewrite_envs(&rest[content_start..content_end]));
            out.push_str(&rest[content_end..block_end]);
        }
        rest = &rest[block_end..];
    }
    out.push_str(rest);
    out
}

/// 找到与 `\begin{name}` 配对的 `\end{name}`，返回（内容起, 内容止, 整块止）
fn find_env_block(src: &str, from: usize, name: &str) -> Option<(usize, usize, usize)> {
    let mut depth = 1usize;
    let mut i = from;
    loop {
        let b = src[i..].find(BEGIN).map(|p| p + i);
        let e = src[i..].find(END).map(|p| p + i);
        match (b, e) {
            (None, None) => return None,
            (Some(bb), None) => {
                depth += 1;
                i = bb + BEGIN.len();
            }
            (None, Some(ee)) => {
                depth -= 1;
                if depth == 0 {
                    return close_block(src, from, ee, name);
                }
                i = ee + END.len();
            }
            (Some(bb), Some(ee)) => {
                if bb < ee {
                    depth += 1;
                    i = bb + BEGIN.len();
                } else {
                    depth -= 1;
                    if depth == 0 {
                        return close_block(src, from, ee, name);
                    }
                    i = ee + END.len();
                }
            }
        }
    }
}

/// 校验 `\end{…}` 的名字与外层 `\begin{…}` 一致（不配对说明源串畸形，交给下游降级）
fn close_block(
    src: &str,
    content_start: usize,
    end_at: usize,
    name: &str,
) -> Option<(usize, usize, usize)> {
    let head = end_at + END.len();
    let close = src[head..].find('}')?;
    if &src[head..head + close] != name {
        return None;
    }
    Some((content_start, end_at, head + close + 1))
}

fn needs_rewrite(name: &str) -> bool {
    LEFT_BRACE_ENVS.contains(&name)
        || RIGHT_BRACE_ENVS.contains(&name)
        || PLAIN_ENVS.contains(&name)
}

/// 剥掉 `\begin{array}{l}` 之类紧跟环境的列描述符（支持平衡的 `{p{2cm}}`）
fn strip_column_spec<'a>(name: &str, content: &'a str) -> &'a str {
    if !COLUMN_SPEC_ENVS.contains(&name) || !content.starts_with('{') {
        return content;
    }
    let mut depth = 0usize;
    for (idx, ch) in content.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return content[idx + 1..].trim_start();
                }
            }
            _ => {}
        }
    }
    content
}

/// 输出一段 matrix，按需要补定界符
///
/// 定界符只补缺失的一侧：语料里 `\left\{\begin{array}{l}…\right.` 已经很常见，
/// 再补一层会得到 `\left\{\left\{ … \right.\right.`，直接编译失败。
fn emit_matrix(out: &mut String, name: &str, inner: &str, opened: bool, after: &str) {
    let wants_left = LEFT_BRACE_ENVS.contains(&name);
    let wants_right = RIGHT_BRACE_ENVS.contains(&name);
    let closed = after.trim_start().starts_with(r"\right\}");
    let add_left = wants_left && !opened;
    let add_right = wants_right && !closed;
    if add_left {
        out.push_str(r"\left\{");
    }
    if add_right {
        out.push_str(r"\left.");
    }
    out.push_str(BEGIN);
    out.push_str(MATRIX);
    out.push('}');
    out.push_str(inner);
    out.push_str(END);
    out.push_str(MATRIX);
    out.push('}');
    if add_left {
        out.push_str(r"\right.");
    }
    if add_right {
        out.push_str(r"\right\}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 断言可转换并返回 MathML（失败时把 reason 与源串一起打出，便于定位）
    fn mathml_of(latex: &str) -> String {
        match to_mathml(latex, true) {
            MathOutcome::Ok(m) => m,
            MathOutcome::Failed(reason) => panic!("转换失败: {reason}\n源: {latex}"),
        }
    }

    #[test]
    fn converts_common_expressions() {
        for latex in [
            r"\frac{1}{x+1}",
            r"\sqrt{2}+\sqrt[3]{a}",
            r"\sum_{i=1}^{n}a_i",
            r"\int_0^1 x^2\,\mathrm{d}x",
            r"\lim_{x\to 0}\frac{\sin x}{x}=1",
            r"\left(\frac{x}{y}\right)^{2}",
            r"\overrightarrow{AB}",
            r"\{1,2\}",
            r"\text{当 } x>0 \text{ 时}",
        ] {
            let mathml = mathml_of(latex);
            assert!(mathml.starts_with("<math "), "{latex} 未产出 mathml 文档");
            assert!(mathml.ends_with("</math>"), "{latex}");
        }
    }

    #[test]
    fn inline_and_block_differ_in_display_attribute() {
        assert!(mathml_of(r"x^2").contains(r#"display="block""#));
        match to_mathml(r"x^2", false) {
            MathOutcome::Ok(m) => assert!(m.contains(r#"display="inline""#)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn malformed_input_fails_with_a_reason_and_never_panics() {
        for latex in [r"\frac{1}{", r"\sqrt", r"](", r"\begin{cases}x", r"a&=b"] {
            let outcome = to_mathml(latex, true);
            if let MathOutcome::Failed(reason) = &outcome {
                assert!(!reason.is_empty(), "{latex} 失败却没给原因");
            }
        }
        // 明确缺右花括号的用例必须走降级分支（docx 侧据此输出红色原文 + 警告），
        // 且原因是给教师看的：带中文定性、不被 crate 的包裹引号污染
        match to_mathml(r"\frac{1}{", true) {
            MathOutcome::Failed(reason) => {
                assert!(reason.starts_with("公式无法解析："), "{reason}");
                assert!(!reason.ends_with('"'), "残留引号: {reason}");
            }
            MathOutcome::Ok(m) => panic!("缺右花括号不该转换成功: {m}"),
        }
    }

    #[test]
    fn empty_formula_fails() {
        assert!(matches!(to_mathml("   ", false), MathOutcome::Failed(_)));
    }

    #[test]
    fn emptyset_matches_the_mathml_backend() {
        // crate 认 \emptyset 与 U+2205，不认 \varnothing（与前端的 KaTeX 方向相反）
        assert_eq!(normalize(r"A=\varnothing").as_ref(), r"A=\emptyset");
        assert_eq!(normalize(r"A=\emptyset").as_ref(), r"A=\emptyset");
        assert_eq!(normalize("A=∅").as_ref(), "A=∅");
        assert!(
            mathml_of(r"A=\emptyset").contains('\u{2205}'),
            "空集符号要落到 MathML 里"
        );
        // 未命中归一时不拷贝
        assert!(matches!(normalize(r"x^2"), Cow::Borrowed(_)));
        assert!(to_mathml(r"A=\emptyset", true).is_ok());
    }

    #[test]
    fn command_aliases_are_token_bounded() {
        assert_eq!(normalize(r"x\ge 0").as_ref(), r"x\geq 0");
        // 已支持的写法不得被二次改写（\ge → \geq → \geqq）
        assert_eq!(normalize(r"x\geq 0").as_ref(), r"x\geq 0");
        assert_eq!(normalize(r"a\ne b").as_ref(), r"a\neq b");
        assert_eq!(
            normalize(r"\lim\limits_{x\to 0}").as_ref(),
            r"\lim_{x\to 0}"
        );
        assert_eq!(
            normalize(r"\displaystyle\frac{1}{2}").as_ref(),
            r"\frac{1}{2}"
        );
    }

    #[test]
    fn aliased_commands_all_convert() {
        for (latex, _) in [
            (r"x\ge 0", "ge"),
            (r"x\le 1", "le"),
            (r"a\ne b", "ne"),
            (r"\dfrac{1}{2}+\tfrac{1}{3}", "dfrac/tfrac"),
            (r"\lg 2 + \ln 3", "lg"),
            (r"\gcd(4,6)=2", "gcd"),
            (r"A=\varnothing", "varnothing"),
            (r"\overparen{AB}", "overparen"),
            (r"\text{测}\mathcal{C}", "mathcal"),
            (r"\sum_{i}a_i\quad \dots \quad a_n", "dots"),
        ] {
            assert!(to_mathml(latex, true).is_ok(), "{latex} 仍降级");
        }
    }

    #[test]
    fn argument_level_rewrites() {
        assert_eq!(normalize(r"\textcolor{red}{x+1}").as_ref(), "x+1");
        assert_eq!(normalize(r"\boxed{S_n}").as_ref(), "S_n");
        assert_eq!(normalize(r"a\hspace{2mm}b").as_ref(), "ab");
        assert_eq!(
            normalize(r"\sum_{\substack{1\le i\le n\\ i\in A}}a_i").as_ref(),
            r"\sum_{\begin{matrix}1\leq i\leq n\\ i\in A\end{matrix}}a_i"
        );
        // 嵌套：内层继续改写
        assert_eq!(
            normalize(r"\boxed{\textcolor{red}{y\ge 0}}").as_ref(),
            "y\\geq 0"
        );
        assert!(to_mathml(r"\textcolor{red}{x+1}", true).is_ok());
        // 参数形态不合预期时原样交下去（不 panic、不误删）
        assert_eq!(normalize(r"\boxed").as_ref(), r"\boxed");
    }

    #[test]
    fn stray_markup_is_escaped_so_output_is_well_formed() {
        // crate 的裸 `<`（x<0 在国内教辅极常见）会让整段 MathML 不是良构 XML，下游解析必炸。
        // 必须用真正的 XML 解析器断言，只看字符串会漏掉「转义时把前文吃掉」这类截断。
        for latex in [
            r"x<0",
            r"a<b\text{且}c>d",
            r"f(x)=\begin{cases}x^2,&x<0\\-x,&x\ge 0\end{cases}",
            r"\{x\mid x<1\}",
            r"\sqrt[3]{\frac{1}{2}}",
            r"0 < p < q",
            r"-b < -a < 1 - b",
            r"\left\langle \vec{a},\vec{b}\right\rangle",
        ] {
            let mathml = mathml_of(latex);
            assert!(
                !mathml.contains("<mo><") && !mathml.contains("< &"),
                "{latex} 仍夹带裸标记: {mathml}"
            );
            assert!(
                roxmltree::Document::parse(&mathml).is_ok(),
                "{latex} 的输出不是良构 XML: {mathml}"
            );
            // 已良构的输出再跑一次不该有任何变化（幂等 = 没有裸标记残留）
            assert_eq!(
                escape_stray_markup(&mathml).as_ref(),
                mathml,
                "{latex} 转义不幂等"
            );
        }
        // 多处裸标记：第二个 `<` 之前的内容不能被丢掉
        let mathml = mathml_of(r"0 < p < q");
        assert!(mathml.starts_with("<math "), "{mathml}");
        assert!(mathml.contains("<mn>0</mn>"), "{mathml}");
        assert_eq!(mathml.matches("&lt;").count(), 2, "{mathml}");
        // crate 的 HTML 实体必须折成 XML 数字引用
        assert!(
            !mathml_of(r"\left\langle x\right\rangle").contains("&lang;"),
            "{}",
            mathml_of(r"\left\langle x\right\rangle")
        );
        assert!(mathml_of(r"x<0").contains("&lt;"));
        // 实体引用不得被二次转义
        assert_eq!(
            escape_stray_markup("<mo>&lt;</mo>").as_ref(),
            "<mo>&lt;</mo>"
        );
        assert_eq!(
            escape_stray_markup("<mn>&#8704;</mn>").as_ref(),
            "<mn>&#8704;</mn>"
        );
        assert_eq!(
            escape_stray_markup("<mo>a &amp; b</mo>").as_ref(),
            "<mo>a &amp; b</mo>"
        );
        assert_eq!(
            escape_stray_markup("<mo>a & b</mo>").as_ref(),
            "<mo>a &amp; b</mo>"
        );
    }

    #[test]
    fn parse_error_output_is_reported_as_failure() {
        // crate 不认的写法会以文本节点混进 MathML，必须判失败而不是印进 Word
        let outcome = to_mathml(r"a\nleq b", true);
        match &outcome {
            MathOutcome::Failed(reason) => {
                assert!(reason.contains(r"\nleq"), "{reason}");
            }
            MathOutcome::Ok(mathml) => panic!("未识别出降级: {mathml}"),
        }
        // 降级输出绝不含 PARSE ERROR 文本
        for latex in [r"\frac{1}{2}", r"f(x)=\begin{cases}x\end{cases}"] {
            assert!(
                !mathml_of(latex).contains("PARSE ERROR"),
                "{latex} 的输出夹带了错误文本"
            );
        }
    }

    #[test]
    fn parse_error_reason_dedups_and_caps() {
        let mathml = "<math><mtext>[PARSE ERROR: Undefined(\"Command(\\\"ge\\\")\")]</mtext>\
                      <mtext>[PARSE ERROR: Undefined(\"Command(\\\"ge\\\")\")]</mtext>\
                      <mtext>[PARSE ERROR: Undefined(\"Environment(\\\"xalignat\\\")\")]</mtext>\
                      <mtext>[PARSE ERROR: UnexpectedToken]</mtext>\
                      <mtext>[PARSE ERROR: SomethingElse]</mtext></math>";
        let reason = parse_error_reason(mathml).expect("应识别出降级");
        assert!(reason.contains(r"不支持的命令 \ge"), "{reason}");
        assert!(reason.contains("不支持的环境 xalignat"), "{reason}");
        assert!(reason.contains("等 4 类"), "{reason}");
        assert!(!reason.contains("SomethingElse"), "{reason}");
        assert_eq!(parse_error_reason("<math><mn>1</mn></math>"), None);
    }

    #[test]
    fn cases_is_rewritten_to_braced_matrix() {
        let src = r"f(x)=\begin{cases}x^2,&x\ge 0\\-x,&x<0\end{cases}";
        let normalized = normalize(src);
        assert!(
            normalized.contains(r"\left\{\begin{matrix}"),
            "{normalized}"
        );
        assert!(normalized.contains(r"\end{matrix}\right."), "{normalized}");
        assert!(mathml_of(src).contains("<mtable"), "cases 应产出表格结构");
    }

    #[test]
    fn array_column_spec_is_stripped() {
        let src = r"\begin{array}{l}x+y=1\\x-y=3\end{array}";
        let normalized = normalize(src);
        assert!(normalized.contains(r"\begin{matrix}x+y=1"), "{normalized}");
        assert!(!normalized.contains("{l}"), "{normalized}");
        assert!(mathml_of(src).contains("<mtable"));
    }

    #[test]
    fn nested_column_spec_is_stripped() {
        let src = r"\begin{array}{p{2cm}}x\\y\end{array}";
        let normalized = normalize(src);
        assert!(normalized.contains(r"\begin{matrix}x\\y"), "{normalized}");
        assert!(to_mathml(src, true).is_ok());
    }

    #[test]
    fn existing_delimiters_are_not_duplicated() {
        // 语料真实形态：外层已有 \left\{ … \right.，改写 aligned 时不得再补一层
        let src = r"\left\{\begin{aligned}a_{1}&=-4\\ d&=3\end{aligned}\right.";
        let normalized = normalize(src);
        assert_eq!(normalized.matches(r"\left\{").count(), 1, "{normalized}");
        assert!(!normalized.contains(r"\right.\left."), "{normalized}");
        assert!(mathml_of(src).contains("<mtable"));

        // cases 前已有 \left\{ 时同样不补
        let src2 = r"\left\{\begin{cases}x=1\end{cases}\right.";
        assert_eq!(
            normalize(src2).matches(r"\left\{").count(),
            1,
            "{}",
            normalize(src2)
        );
    }

    #[test]
    fn rcases_gets_a_full_delimiter_pair() {
        // 裸 rcases：补齐 \left. … \right\} 一对
        let src = r"\begin{rcases}x=1\\y=2\end{rcases}";
        let normalized = normalize(src);
        assert!(
            normalized.starts_with(r"\left.\begin{matrix}"),
            "{normalized}"
        );
        assert!(
            normalized.ends_with(r"\end{matrix}\right\}"),
            "{normalized}"
        );
        assert!(to_mathml(src, true).is_ok());

        // 外层已带定界符：一侧都不补
        let src2 = r"\left.\begin{rcases}x=1\end{rcases}\right\}";
        assert_eq!(
            normalize(src2).matches(r"\left.").count(),
            1,
            "{}",
            normalize(src2)
        );
        assert_eq!(
            normalize(src2).matches(r"\right\}").count(),
            1,
            "{}",
            normalize(src2)
        );
        assert!(to_mathml(src2, true).is_ok());
    }

    #[test]
    fn nested_environments_recurse() {
        // cases 里套 pmatrix：外层改写，内层原样
        let src = r"\begin{cases}\begin{pmatrix}1&2\end{pmatrix},&x>0\\0,&x\le 0\end{cases}";
        let normalized = normalize(src);
        assert!(
            normalized.contains(r"\begin{pmatrix}1&2\end{pmatrix}"),
            "{normalized}"
        );
        assert!(to_mathml(src, true).is_ok());

        // array 里套 aligned：两层都改写
        let src2 = r"\begin{array}{l}\begin{aligned}a&=1\end{aligned}\\b=2\end{array}";
        assert_eq!(normalize(src2).matches(r"\begin{matrix}").count(), 2);
        assert!(to_mathml(src2, true).is_ok());
    }

    #[test]
    fn supported_environments_are_untouched() {
        for src in [
            r"\begin{pmatrix}1&2\\3&4\end{pmatrix}",
            r"\begin{matrix}a&=1\\b&=2\end{matrix}",
            r"\begin{align}a&=1\\b&=2\end{align}",
        ] {
            assert_eq!(normalize(src).as_ref(), src);
        }
    }

    #[test]
    fn unbalanced_or_mismatched_environment_is_left_as_is() {
        // 无配对 \end：保留原文，交给 latex2mathml 降级
        let src = r"\begin{cases}x=1";
        assert_eq!(normalize(src).as_ref(), src);
        // \begin / \end 名字不配对：同样不改写，且不 panic
        let src2 = r"\begin{cases}x=1\end{aligned}";
        assert!(normalize(src2).contains(r"\begin{cases}"));
        let _ = to_mathml(src, true);
        let _ = to_mathml(src2, true);
    }

    #[test]
    fn corpus_failures_from_t2_1_all_convert_now() {
        // T2.1 预扫描里 10 条降级的三种根因环境，各取语料实际形态做回归锚点
        for src in [
            r"\begin{cases} a_1 x + b_1 y = 1 \\ a_2 x + b_2 y = 1 \end{cases}",
            r"f(x) = \begin{cases} x^2 + 4x, & x \geq 0 \\ x^2 - 4x, & x < 0 \end{cases}",
            r"\left\{ \begin{array}{l} 12 - 6a = 0 \\ 12a - 24 = 0 \\ 18 - 12a = 6 - 6a \end{array} \right.",
            r"\left\{\begin{aligned}a_{1}+2d+a_{1}+3d=7\\ 3(a_{1}+d)+a_{1}+4d=5\end{aligned}\right.",
            r"\begin{array}{r l} & {x _ {n} y _ {n + m}} \\ & {= \frac {1}{2}} \end{array}",
            r"\begin{gathered}x=1\\y=2\end{gathered}",
            r"\begin{split}a&=1\\b&=2\end{split}",
        ] {
            assert!(to_mathml(src, true).is_ok(), "仍无法转换: {src}");
        }
    }
}
