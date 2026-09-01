//! LaTeX → Presentation MathML（T2.2）
//!
//! 这一层的职责只有一个：让 `latex2mathml` 吃得下教辅里的实际写法。两件事都做在进库转换之前，
//! 因此 Word / PDF 两条公式管线共享同一份归一结果。
//!
//! 1. **符号归一**：与前端 `LatexRender.vue` 的 KaTeX 配置逐条对齐（`\emptyset` → `\varnothing`、
//!    U+2205 `∅` → `\varnothing`），保证导出的公式与教师在编辑器里看到的符号一致。
//! 2. **环境改写**：crate 支持 `matrix`/`pmatrix`/`bmatrix`/`vmatrix`/`align`（且 `matrix` 接受 `&`
//!    对齐列与 `\text` 条件），但不支持 `cases`/`aligned`/`array`/`gathered`/`split`/… ——
//!    T2.1 预扫描的 10 条降级全部出于此（分段函数与方程组是最高频写法）。统一改写为 `matrix`，
//!    需要大括号的补 `\left\{ … \right.` / `\left. … \right\}`。
//!
//! 改写的代价是列对齐语义退化（`array{l}` 的左对齐变成 matrix 的居中），属可接受取舍：
//! 比整条公式降级成红色原文更接近原样。

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

/// 公式转换结果
///
/// 失败不 panic、不往调用栈上层抛错：调用方（docx / markdown / typst 生成器）必须降级为
/// 「原文 + 警告」，绝不让单题失败中断整卷（实施计划 §5.3 容错约定）。
#[derive(Debug, Clone, PartialEq)]
pub enum MathOutcome {
    /// Presentation MathML，形如 `<math xmlns="…" display="block">…</math>`
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
        Ok(mathml) => MathOutcome::Ok(mathml),
        Err(e) => MathOutcome::Failed(e.to_string()),
    }
}

/// 与前端一致地归一公式源串：符号替换 + 不支持环境改写
pub fn normalize(latex: &str) -> Cow<'_, str> {
    let has_symbol = latex.contains(r"\emptyset") || latex.contains('\u{2205}');
    let has_env = latex.contains(BEGIN);
    if !has_symbol && !has_env {
        return Cow::Borrowed(latex);
    }
    let mut s = latex.to_string();
    if has_symbol {
        s = s
            .replace(r"\emptyset", r"\varnothing")
            .replace('\u{2205}', r"\varnothing");
    }
    if has_env {
        s = rewrite_envs(&s);
    }
    Cow::Owned(s)
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
        // 明确缺右花括号的用例必须走降级分支（docx 侧据此输出红色原文 + 警告）
        assert!(matches!(
            to_mathml(r"\frac{1}{", true),
            MathOutcome::Failed(_)
        ));
    }

    #[test]
    fn empty_formula_fails() {
        assert!(matches!(to_mathml("   ", false), MathOutcome::Failed(_)));
    }

    #[test]
    fn emptyset_macro_matches_frontend() {
        assert_eq!(normalize(r"A=\emptyset").as_ref(), r"A=\varnothing");
        assert_eq!(normalize("A=∅").as_ref(), r"A=\varnothing");
        // 未命中归一时不拷贝
        assert!(matches!(normalize(r"x^2"), Cow::Borrowed(_)));
        // 归一后确实可转换
        assert!(to_mathml(r"A=\emptyset", true).is_ok());
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
