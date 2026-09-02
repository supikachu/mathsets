//! LaTeX → Typst 数学源码（实施计划 §6.3、任务分解 T3.4）
//!
//! 依赖方向与 `spec.rs` 同口径：本文件一个 export 符号都不碰。
//!
//! ## mitex 0.2.4 的输出契约（本文件的四处防御都由此而来）
//!
//! 1. `convert_math` 交出的是**裸数学体**，不含 `$…$`。包装由 [`to_typst`] 负责，
//!    而「首尾留不留空白」就是 typst 的块级/行级判据（`EquationElem` 文档：
//!    whitespace after the opening dollar sign lifts it into a block），所以
//!    `display` 走 `$ … $`、行内走 `$…$`。
//! 2. mitex 会吐出若干**要求调用方自己定义**的辅助标识符（`mitexsqrt`、`mitexarray`、
//!    `textmath` …），也会吐一批 typst 0.15 里已经改了名的符号（`\cap` → `sect`，现名
//!    `inter`；实测 `unknown variable: sect`）。[`MITEX_PREAMBLE`] 把其中语义明确、常用
//!    的那些定义出来；剩下的由 [`unresolved_name`] 拦住并降级。这条不是洁癖：一个解析不
//!    出来的名字会让 typst 报 unknown variable，而那是**整卷编译失败**，比少一个公式严重
//!    一个量级。三张名单都不手抄 —— mitex 会吐哪些词从 `DEFAULT_SPEC` 现读，typst 认哪些
//!    名字直接问 `typst::Library`，我们定义了哪些从 preamble 文本现读。
//!    （反例：`zws` 与 `space.nobreak` 看着像要自己定义，实测都是 typst 原生符号，
//!    定义同名反而把原生行为盖了。）
//! 3. 上游坑：`convert_inner` 把传入的 `spec` 只用在 parse 阶段，convert 阶段仍取
//!    `DEFAULT_SPEC`（`mitex-0.2.4/src/converter.rs` 尾部）。所以**给 mitex 传自定义
//!    命令表是无效的**；扩展覆盖只能事后补 —— 输出是纯文本，缺什么名字就在 preamble
//!    里定义什么名字。`mitex_spec_gen::DEFAULT_SPEC` 的 alias/key 就是完整的输出词表，
//!    探针单测 [`dump_unresolved_names`] 会把它列出来。
//! 4. 数学函数（`mat` / `frac` / `root` …）只存在于 typst 的**数学作用域**，顶层
//!    `#let` 的函数体按代码作用域解析看不见它们 —— preamble 里必须写 `math.mat(..)`。
//! 5. typst 的裸 `%` 是行注释，会把方程尾巴整段吃掉，而 mitex 把 `\%` 原样写成 `%`：
//!    出口处统一折回 `\%`（[`escape_percent`]）。
//!
//! 失败口径与 Word 路径一致：降级 + 由调用方记 Issue，一枚坏公式不许失败整卷。

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::OnceLock;

use mitex::convert_math;
use typst::{Library, LibraryExt};

/// 转换成功：可直接拼进 typst markup 的数学片段。
///
/// 失败时返回降级原因（中文，会进 `X-Export-Warnings`，教师看）。
pub fn to_typst(latex: &str, display: bool) -> Result<String, String> {
    let src = normalize(latex);
    let body = match convert_math(src.as_ref(), None) {
        Ok(body) => body,
        Err(reason) => return Err(failure_reason(&reason)),
    };
    // typst 里裸 `%` 是行注释，会把方程尾巴整段吃掉（mitex 把 `\%` 原样写成 `%`）
    let body = escape_percent(&body);
    if let Some(name) = unresolved_name(&body) {
        return Err(format!("PDF 暂不支持该构造（{name}）"));
    }
    // 数学体里出现未转义的 `$` 会提前关掉方程，后面整段按普通文本排 —— 宁可降级。
    if has_unescaped_dollar(&body) {
        return Err("转换结果含游离的 $".to_string());
    }
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err("转换结果为空".to_string());
    }
    Ok(if display {
        format!("$ {trimmed} $")
    } else {
        format!("${trimmed}$")
    })
}

/// 降级呈现：红色等宽原文。行内片段，不破坏段落节奏。
///
/// 字体名写 `DejaVu Sans Mono`（typst-assets 内嵌）；缺该字体时 typst 自行回退，
/// 颜色仍是判定「这一处是原文而非公式」的视觉锚点。
pub fn degraded(latex: &str) -> String {
    format!(
        "#text(fill: rgb(\"#B00000\"), font: \"DejaVu Sans Mono\", size: 0.9em)[#({})]",
        typst_str(latex)
    )
}

/// 随生成源码一起注入的 typst 定义块（由 `typst_gen` 输出一次）。
///
/// 数学函数一律走 `math.` 前缀：`mat` / `frac` 只存在于 typst 的**数学作用域**，
/// 顶层 `#let` 的函数体按代码作用域解析，直接写 `mat(..)` 会得到
/// `unknown variable: mat`（实测）。
pub const MITEX_PREAMBLE: &str = "\
// mitex 转换结果依赖的定义（typst 作用域里没有这些名字）
// 两个反例记在这：`zws`（零宽空格）与 `space.nobreak`（LaTeX 的 `~`）都是 typst 原生
// 符号，自己定义同名只会把原生行为盖掉。
#let mitexdisplay(body) = math.display(body)
// mitex 的 sqrt 约定：一个参数是被开方式，两个参数是「次数 + 被开方式」。
// 不能直接转发给 root —— 它的 index 是第一个位置参数且是可选的，`root(x)` 会报
// `missing argument: radicand`（实测）。
#let mitexsqrt(..args) = {
  let ps = args.pos()
  if ps.len() == 1 { math.sqrt(ps.at(0)) } else { math.root(ps.at(0), ps.at(1)) }
}
#let mitexarray(arg0: none, ..args) = math.mat(..args)
#let mitexcolor(color, body) = text(fill: color, body)
#let mitexoverbrace(..args) = math.overbrace(..args)
#let mitexunderbrace(..args) = math.underbrace(..args)
#let textmath(body) = math.text(body)
// typst 0.15 把 `\\cap` 的符号名从 sect 改成了 inter，而 mitex 0.2.4 仍写 sect（实测
// `unknown variable: sect`）
#let sect = math.inter
#let dfrac(..args) = math.frac(..args)
#let tfrac(..args) = math.frac(..args)
#let pmatrix(..args) = math.mat(..args)
#let bmatrix(..args) = math.mat(delim: \"[\", ..args)
#let vmatrix(..args) = math.mat(delim: \"|\", ..args)
#let Bmatrix(..args) = math.mat(delim: \"{\", ..args)
#let Vmatrix(..args) = math.mat(delim: \"‖\", ..args)
#let matrix(..args) = math.mat(delim: none, ..args)
#let smallmatrix(..args) = math.mat(delim: none, ..args)
#let mitexbold(body) = math.bold(body)
#let mitexitalic(body) = math.italic(body)
#let mitexupright(body) = math.upright(body)
#let mitexsans(body) = math.sans(body)
#let mitexmono(body) = math.mono(body)
#let mitexcal(body) = math.cal(body)
#let mitexmathbf(body) = math.bold(body)
";

/// 喂给 mitex 之前的源串清洗。
///
/// 只放「mitex 明确会挂且改写无损」的规则，其余交给降级 —— 每条规则都该有对应的
/// 单测盯着，否则它会在某次改动后静默失效。
fn normalize(latex: &str) -> Cow<'_, str> {
    // `\text{}` 里的 `%` 会被 mitex 当注释吃掉行尾：先转义成 `\%`
    if latex.contains("%") && !latex.contains(r"\%") {
        return Cow::Owned(latex.replace("%", r"\%"));
    }
    Cow::Borrowed(latex)
}

/// preamble 里 `#let` 定义的名字集合，从 [`MITEX_PREAMBLE`] 现读。
///
/// 不另立一张表：守卫判「我们定义了」和模板写出定义块必须同源，否则改一边忘一边
/// 就是整卷编译失败。
fn preamble_names() -> &'static HashSet<&'static str> {
    static NAMES: OnceLock<HashSet<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| {
        MITEX_PREAMBLE
            .lines()
            .filter_map(|line| line.strip_prefix("#let "))
            .filter_map(|line| line.split(['(', '=', ' ', ':']).next())
            .collect()
    })
}

/// typst 原生认识的名字，分数学作用域与全局作用域两份。
///
/// 同样不手抄：升级 typst 时新增/改名的函数会自动跟上，守卫不会因此变松或误伤。
/// 名字从 `Library` 拷成 owned 字符串再存 —— `Library` 是闭包里的局部值，借它的
/// `&str` 出作用域就是悬垂。
fn typst_names() -> &'static TypstNames {
    static NAMES: OnceLock<TypstNames> = OnceLock::new();
    NAMES.get_or_init(|| {
        let lib = Library::default();
        let collect = |scope: &typst::foundations::Scope| -> HashSet<String> {
            scope.iter().map(|(name, _)| name.to_string()).collect()
        };
        TypstNames {
            math: collect(lib.math.scope()),
            global: collect(lib.global.scope()),
        }
    })
}

struct TypstNames {
    /// 数学模式下裸标识符（`frac(…)`、`space.nobreak`）能解析到的名字
    math: HashSet<String>,
    /// `#` 前缀（代码求值）能解析到的名字
    global: HashSet<String>,
}

/// mitex 可能写进输出的标识符词表（根段，不含点号后的部分）。
///
/// 有了这张表，输出里的裸字母串才分得清「是 mitex 发的名字」还是「公式里的普通字母」
/// （`$abc$` 里 `abc` 只是三个变量）。`DEFAULT_SPEC` 是进程级静态，所以借得到的 `&str`
/// 可以当 `'static` 存。
fn mitex_words() -> &'static HashSet<&'static str> {
    use mitex::CommandSpecItem;

    static WORDS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    WORDS.get_or_init(|| {
        mitex_spec_gen::DEFAULT_SPEC
            .items()
            .filter_map(|(key, item)| {
                let alias = match item {
                    CommandSpecItem::Cmd(c) => c.alias.as_deref(),
                    CommandSpecItem::Env(e) => e.alias.as_deref(),
                };
                let emitted = alias.unwrap_or(key);
                let bare = emitted.strip_prefix('#').unwrap_or(emitted);
                // 点号形式（`space.nobreak`）只有根段是名字
                let root = bare.split('.').next().unwrap_or(bare);
                (!root.is_empty() && root.bytes().all(|b| b.is_ascii_alphabetic())).then_some(root)
            })
            .collect()
    })
}

/// 输出里第一处「typst 解析不出来」的名字。
///
/// 判定与 typst 自己的解析口径对齐，四种位置上的字母串才是**名字**，其余在数学模式里
/// 只是普通文本（`$abc$` 合法）：
/// 1. `#name`（代码求值，含 `#name[...]`）→ 查全局作用域；
/// 2. 裸 `name(`（数学里的函数调用）→ 查数学作用域；
/// 3. 裸 `name.`（数学里的字段访问，如 mitex 的 `space.nobreak`）→ 查数学作用域；
/// 4. 裸 `name` 但它是 mitex 词表里的一个名字 → 查数学作用域。
///
/// 第 4 条是语料单测逼出来的：`$A sect  B$`（`\cap`）里的 `sect` 后面既没有 `(` 也没有
/// `.`，前三条规则整条放过，typst 却报 `unknown variable` —— 那是**整卷**编译失败。
///
/// 点号后面的那段是字段，由被访问的值自己负责，不再当名字查。
fn unresolved_name(body: &str) -> Option<String> {
    let b = body.as_bytes();
    let names = typst_names();
    let words = mitex_words();
    let mut i = 0usize;
    while let Some(at) = find_word(b, i) {
        let end = at
            + b[at..]
                .iter()
                .take_while(|c| c.is_ascii_alphabetic())
                .count();
        let word = &body[at..end];
        let hash_before = at > 0 && b[at - 1] == b'#';
        let dot_before = at > 0 && b[at - 1] == b'.';
        let needed = hash_before || matches!(b.get(end), Some(b'(' | b'.')) || words.contains(word);
        if needed && !dot_before {
            let pool = if hash_before {
                &names.global
            } else {
                &names.math
            };
            if !pool.contains(word) && !preamble_names().contains(word) {
                return Some(word.to_string());
            }
        }
        // 名字检查过了才跳内容块：`#name[...]` 的方括号里是 markup，不是数学体，
        // 里面的字母串不该当名字查 —— 但 `#name` 本身必须查。
        i = if hash_before && b.get(end) == Some(&b'[') {
            skip_balanced(b, end)
        } else {
            end
        };
    }
    None
}

/// 下一个「前面不是字母」的 ASCII 字母串起点
fn find_word(b: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i < b.len() {
        if b[i].is_ascii_alphabetic() && (i == 0 || !b[i - 1].is_ascii_alphabetic()) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 从 `[` 起跳到配平的 `]` 之后（markup 内容块，允许嵌套）
fn skip_balanced(b: &[u8], open: usize) -> usize {
    let mut depth = 0usize;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    b.len()
}

/// 把输出里的裸 `%` 折成 `\%`（已有的 `\%` 不动）
fn escape_percent(body: &str) -> String {
    if !body.contains('%') {
        return body.to_string();
    }
    let mut out = String::with_capacity(body.len() + 4);
    let mut escaped = false;
    for ch in body.chars() {
        if escaped {
            escaped = false;
            out.push(ch);
            continue;
        }
        if ch == '\\' {
            escaped = true;
        } else if ch == '%' {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// 是否有未被 `\\` 转义的 `$`
fn has_unescaped_dollar(s: &str) -> bool {
    let mut escaped = false;
    for ch in s.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '$' => return true,
            _ => {}
        }
    }
    false
}

/// mitex 的错误串是英文短句（`unknown command: \foo`），教师看不懂也不如中文具体
fn failure_reason(raw: &str) -> String {
    let core = raw
        .trim()
        .trim_start_matches("error: ")
        .trim_end_matches('.');
    if let Some(name) = core.strip_prefix("unknown command: \\") {
        return format!("不支持的命令 \\{name}");
    }
    if let Some(name) = core.strip_prefix("unknown environment: \\") {
        return format!("不支持的环境 {name}");
    }
    if core.is_empty() {
        return "公式无法解析".to_string();
    }
    format!("公式无法解析：{core}")
}

/// 把任意文本落成 typst **字符串字面量**（含首尾引号）
///
/// 这是排版域唯一的「外部文本进 typst 源码」通道，`typst_gen` 与 [`degraded`] 共用。
/// 为什么不用 markup 转义（`\_` 那一套）：typst 的 markup 层会解释太多东西 —— 行内 `//`
/// 是行注释、行首 `- ` / `1. ` / `= ` 变列表与标题、`*` 变粗体、`--` 与 `...` 变连字（全部
/// 实测）。字符串字面量则逐字不动，连 `#("a//b")` 与 `#("[甲]{乙}")` 都照原样上图，所以
/// 只需要处理引号与反斜杠本身，转义面从「一整个语法层」缩到三个字符。
pub(crate) fn typst_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            // C0 控制字符与 DEL：留在串里就是 PDF 上的豆腐块，题库里的垃圾字符不值得保
            '\r' | '\u{0}'..='\u{8}' | '\u{b}'..='\u{1f}' | '\u{7f}' => {}
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// 共建语料：`dump_corpus` 用它打印转换结果，`compiler::tests` 用它逐枚编译。
/// 两处共用一份，「守卫判能过」与「typst 真编译」才会互相印证。
#[cfg(test)]
pub(crate) const CORPUS: &[&str] = &[
    // 分式与根式
    r"\frac{1}{2}",
    r"\dfrac{x+1}{x-1}",
    r"\tfrac{1}{3}",
    r"\sqrt{a^2+b^2}",
    r"\sqrt[3]{x}",
    r"\sqrt{x+y}",
    r"\binom{5}{2}",
    // 上下标与嵌套
    r"x^{2}_{i}",
    r"a_{1}+a_{2}+\cdots+a_{n}",
    r"e^{i\pi}+1=0",
    r"2^{x}>4",
    r"\left(\dfrac{1}{2}\right)",
    r"\left|x\right| \ge 2",
    r"|a|",
    // 集合与逻辑
    r"\{1,2,3\}",
    r"\{x\mid x>0\}",
    r"A\cap B",
    r"A\cup B",
    r"A\setminus B",
    r"\complement_{\mathbb{R}}A",
    r"\complement_U A \cap B",
    r"x\in\mathbb{N}^*",
    r"\forall x \in A",
    r"\exists x \in \mathbb{Z}",
    r"\varnothing",
    r"\emptyset",
    // 函数与运算
    r"\log_2 8",
    r"\lg 2 + \lg 5",
    r"\ln x",
    r"\sin\left(\dfrac{\pi}{6}\right)",
    r"\tan \dfrac{\pi}{4}",
    r"\lim_{x\to 0}\dfrac{\sin x}{x}",
    r"\int_0^1 x^2\,dx",
    r"\sum_{k=1}^{n}k=\dfrac{n(n+1)}{2}",
    r"f(x)=\dfrac{1}{x}",
    r"f: A \to B",
    r"a \cdot b = |a||b|\cos\theta",
    r"x_1 + x_2 = -\dfrac{b}{a}",
    r"\Delta = b^2 - 4ac \ge 0",
    // 几何与向量
    r"\overrightarrow{AB}",
    r"\vec{a}\cdot\vec{b}",
    r"\vec{e_1} \cdot \vec{e_2} = 0",
    r"\overrightarrow{a} \parallel \overrightarrow{b}",
    r"\angle ABC",
    r"\angle ABC = 90^\circ",
    r"\triangle ABC",
    r"S_{\triangle ABC}",
    r"\overline{AB}",
    r"\hat{a}",
    // 矩阵、数组与分段
    r"\begin{cases}x,&x>0\\-x,&x\le 0\end{cases}",
    r"\begin{pmatrix}1&2\\3&4\end{pmatrix}",
    r"\begin{bmatrix}1&2\\3&4\end{bmatrix}",
    r"\begin{vmatrix}1&2\\3&4\end{vmatrix}",
    r"\begin{Bmatrix}1&2\\3&4\end{Bmatrix}",
    r"\begin{Vmatrix}1&2\\3&4\end{Vmatrix}",
    r"\begin{matrix}1&2\\3&4\end{matrix}",
    r"\begin{array}{cc}1&2\end{array}",
    r"\left\{\begin{array}{l}x+y=1\\x-y=3\end{array}\right.",
    r"\left\{\begin{matrix}x+y=1\\x-y=3\end{matrix}\right.",
    // 中文、百分号与间距
    r"\text{则}a>b",
    r"\text{50%}",
    r"\text{甲、乙}",
    r"\text{解得} x = 3",
    r"\%off",
    r"a\,b",
    r"a\;b",
    r"a\:b",
    r"a\quad b",
    r"a\qquad b",
    r"a\ b",
    r"a~b",
    // 大括号与上下标注
    r"\overbrace{a+a+\cdots+a}^{n\text{个}}",
    r"\underbrace{1+1}_{2}",
    r"\dfrac{x^2}{4}+\dfrac{y^2}{3}=1",
    r"\frac{\pi}{2} < \alpha < \pi",
];

/// 已知「mitex 转得动但 typst 0.15 不认」的构造：必须降级，不能进源码。
#[cfg(test)]
pub(crate) const UNSUPPORTED: &[&str] = &[
    r"\argmax_x f",
    r"\xcancel{ab}",
    r"\textbf{甲}",
    r"\includegraphics{a.png}",
    // 间距命令里 `\,` `\;` `\:` 落在 typst 原生的 thin/med/thick 上，`\!` 却是 mitex 自造
    // 的 negthinspace（typst 0.15 无此名），所以只有它要降级。
    r"a\!b",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fraction_and_nested_scripts_convert_to_inline_math() {
        let out = to_typst(r"\frac{1}{2}x^{2}_{i}", false).unwrap();
        assert!(out.starts_with('$') && out.ends_with('$'), "{out}");
        assert!(
            !out.starts_with("$ ") && !out.ends_with(" $"),
            "行级不该首尾留白：{out}"
        );
        assert!(out.contains("frac"), "{out}");
    }

    #[test]
    fn display_math_is_lifted_by_surrounding_spaces() {
        let out = to_typst(r"a=b", true).unwrap();
        assert!(
            out.starts_with("$ ") && out.ends_with(" $"),
            "块级判据是首尾空白：{out}"
        );
    }

    #[test]
    fn matrix_and_cases_survive() {
        assert!(to_typst(r"\begin{pmatrix}a&b\\c&d\end{pmatrix}", false).is_ok());
        assert!(to_typst(r"\begin{cases}x,&x>0\\-x,&x\le 0\end{cases}", false).is_ok());
    }

    #[test]
    fn malformed_inputs_never_panic() {
        // mitex 0.2.4 对括号不配平是宽容的（`\frac{1}{` 实测返回 Ok），所以这里守的是
        // 真正的契约：任意畸形输入都只能得到 Ok 或 Err，不允许 panic 打断整卷。
        for bad in [
            r"\frac{1}{",
            r"\begin{array}{cc}",
            r"\sqrt",
            r"}{",
            r"\text{",
            r"\\",
            r"\%",
            r"$",
            r"#",
            "",
        ] {
            if let Err(reason) = to_typst(bad, false) {
                assert!(!reason.is_empty(), "{bad:?} 的降级原因是空的");
                assert!(!reason.starts_with('"'), "不该把裸引号带给教师：{reason}");
            }
            let _ = degraded(bad);
        }
    }

    #[test]
    fn unknown_command_reason_is_chinese() {
        let err = to_typst(r"\nosuchcmd{a}", false).unwrap_err();
        assert_eq!(err, "不支持的命令 \\nosuchcmd");
    }

    #[test]
    fn guard_checks_only_name_positions() {
        let cases = [
            // typst 原生数学函数
            ("frac(1 ,2 )", None),
            ("mat(1 ,2 ;3 ,4 )", None),
            ("cases(1 zws ,2 )", None),
            // 我们定义的：preamble 名字与 mitex 的辅助名
            ("dfrac(1 ,2 )", None),
            ("mitexarray(arg0: c c ,1 zws ,2 )", None),
            ("#textmath[则];", None),
            ("space.nobreak ", None),
            ("zws ,", None),
            ("A sect  B", None), // `\cap`：typst 0.15 改了名，preamble 里已补
            // 查不出来的名字必须拦住（位置四选一：#name / name( / name. / mitex 词表裸词）
            ("frobnicate(1)", Some("frobnicate")),
            ("#frobnicate[甲]", Some("frobnicate")),
            ("frobnicate.deep", Some("frobnicate")),
            ("a argmax b", Some("argmax")),
            ("a bcancel b", Some("bcancel")),
            // 不该被当成名字的：普通字母串、转义括号、点号后的字段、内容块内部
            ("abc", None),
            ("frobnicate bar", None), // 不在 mitex 词表里又不处于名字位置 → 只是普通字母
            (r"f \(x\)", None),
            ("space.frobnicate ", None), // 点号后的字段由被访问的值负责，不查名字
            ("#textmath[abc(1)]", None),
        ];
        for (body, want) in cases {
            assert_eq!(unresolved_name(body).as_deref(), want, "判定不符：{body:?}");
        }
    }

    #[test]
    fn every_preamble_definition_is_recognized_by_the_guard() {
        for name in preamble_names() {
            let body = format!("{name}(1)");
            assert!(
                unresolved_name(&body).is_none(),
                "{name} 在 preamble 里定义了却仍被拦 —— 守卫与定义块不同源"
            );
        }
    }

    #[test]
    fn preamble_never_shadows_a_typst_definition() {
        // 数学模式里词法作用域优先于数学作用域：preamble 里若出现 `frac`、`zws` 这类
        // typst 已有的名字，就会把原生行为悄悄换掉，排版结果再也对不上文档。
        let names = typst_names();
        for name in preamble_names() {
            assert!(
                !names.math.contains(*name),
                "{name} 会盖掉 typst 的数学定义"
            );
            assert!(
                !names.global.contains(*name),
                "{name} 会盖掉 typst 的全局定义"
            );
        }
    }

    #[test]
    fn bare_dollar_in_output_degrades() {
        // 防御路径：直接验证判据本身，不依赖上游是否真会吐出 `$`
        assert!(has_unescaped_dollar(r"a $ b"));
        assert!(!has_unescaped_dollar(r"a \$ b"));
        // `\\$` 是「转义反斜杠 + 裸 $」，$ 仍然生效 —— 也算未转义
        assert!(has_unescaped_dollar(r"a \\$ b"));
    }

    #[test]
    fn degraded_lands_latex_verbatim_in_a_string() {
        // 降级原文里满是 markup 触发符（`#` `[` `]` `{` `\` `$`）：它们必须整体关在
        // `#("…")` 里逐字上图，而不是靠逐个转义侥幸不出错
        let s = degraded("含 \\ 与 \" 的 $\\frac{#a}$");
        assert!(s.starts_with("#text(fill:"), "{s}");
        assert!(s.ends_with(r#"[#("含 \\ 与 \" 的 $\\frac{#a}$")]"#), "{s}");
    }

    #[test]
    fn typst_str_only_escapes_the_string_delimiters() {
        assert_eq!(typst_str("甲"), r#""甲""#);
        assert_eq!(typst_str("a\"b"), r#""a\"b""#);
        assert_eq!(typst_str(r"a\b"), r#""a\\b""#);
        // markup 触发符原样留在串里：`#("= 甲")` 实测不成标题
        assert_eq!(typst_str("- 甲 // 乙"), r#""- 甲 // 乙""#);
        assert_eq!(typst_str("a\r\nb"), r#""a\nb""#);
        assert_eq!(typst_str("甲\u{1}\u{7f}乙"), r#""甲乙""#);
    }

    #[test]
    fn percent_survives_normalization() {
        // `\text{50%}` 里的裸 % 会被当成行注释，转换结果整段丢掉
        assert!(to_typst(r"\text{50%}", false).is_ok());
        assert_eq!(escape_percent("a % b \\% c"), r"a \% b \% c");
        for latex in [r"\text{50%}", r"\%off"] {
            let out = to_typst(latex, false).unwrap();
            assert_eq!(escape_percent(&out), out, "结果里还有裸 %：{out}");
        }
    }

    /// 覆盖率摸底：遍历 [`CORPUS`] 打印 mitex 的转换结果，看失败面与它吐出的辅助名。
    /// 需要看输出时 `cargo test --lib typeset::math -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn dump_corpus() {
        for latex in CORPUS {
            match to_typst(latex, false) {
                Ok(s) => println!("OK   {latex}\n     -> {s}"),
                Err(e) => println!("FAIL {latex}\n     -> {e}"),
            }
        }
    }

    /// 已知不支持的构造：降级 + 中文原因，绝不进源码（一个认不出的名字会失败整卷）。
    #[test]
    fn unsupported_construct_degrades() {
        for latex in UNSUPPORTED {
            let err = to_typst(latex, false).expect_err("{latex} 居然转换成功了");
            assert!(err.contains("不支持"), "{latex} 的降级原因措辞不对：{err}");
        }
    }

    /// mitex 真正会写进输出的标识符词表：逐条查 typst 的两个作用域，打印两边都不认识的。
    ///
    /// mitex 的输出名 = `alias`（可能带 `#`）或 LaTeX 侧的 key（`converter.rs` 的
    /// `convert_command_sym` / `convert_normal_command`），所以这张表是**可枚举的**，
    /// 不用靠语料猜。查不到 = typst 原生没有这个名字，必须由我们的 preamble 补上。
    ///
    /// 点号形式（`space.nobreak`、`triangle.stroked.t`）按 typst 的口径只查根段：
    /// 根段是 Symbol 时，后面那段是它的变体名，由符号表负责，所以我们另列一张
    /// 「根段是 Symbol」的表，用来核对变体名是不是 typst 真有。
    #[test]
    #[ignore]
    fn dump_unresolved_names() {
        use mitex::CommandSpecItem;
        use typst::Library;

        let lib = Library::default();
        let mut unresolved = Vec::new();
        let mut symbol_paths = Vec::new();
        for (key, item) in mitex_spec_gen::DEFAULT_SPEC.items() {
            let alias = match item {
                CommandSpecItem::Cmd(c) => c.alias.as_deref(),
                CommandSpecItem::Env(e) => e.alias.as_deref(),
            };
            let emitted = alias.unwrap_or(key);
            let bare = emitted.strip_prefix('#').unwrap_or(emitted);
            let root = bare.split('.').next().unwrap_or("");
            // 符号类 alias 常是 `>=`、`→` 这类非标识符写法，不参与作用域查找
            if root.is_empty() || !root.bytes().all(|b| b.is_ascii_alphabetic()) {
                continue;
            }
            let math_hit = lib.math.scope().get(root);
            let known = math_hit.is_some() || lib.global.scope().get(root).is_some();
            if !known {
                unresolved.push(format!("{emitted} <- {key}"));
            } else if bare.contains('.') {
                let variants = match math_hit.map(|b| b.read()) {
                    Some(typst::foundations::Value::Symbol(sym)) => format!(
                        "{} / mod={}",
                        sym.variants().map(|v| v.1).collect::<Vec<_>>().join("|"),
                        sym.modifiers().collect::<Vec<_>>().join("|"),
                    ),
                    _ => "-".to_string(),
                };
                symbol_paths.push(format!("{emitted} <- {key} 根段变体={variants}"));
            }
        }
        for list in [&mut unresolved, &mut symbol_paths] {
            list.sort();
            list.dedup();
        }
        for line in &symbol_paths {
            println!("SYM  {line}");
        }
        for line in &unresolved {
            println!("MISS {line}");
        }
        println!(
            "共 {} 个名字 typst 原生不认，{} 个点号形式",
            unresolved.len(),
            symbol_paths.len()
        );
    }

    /// 一组名字在 typst 两个作用域里各是什么，用来定「点号访问该放行谁」。
    #[test]
    #[ignore]
    fn dump_scope_facts() {
        use typst::foundations::Value;

        let lib = Library::default();
        let describe = |v: &Value| match v {
            Value::Module(_) => "module".to_string(),
            Value::Func(_) => "func".to_string(),
            Value::Content(_) => "content".to_string(),
            Value::Str(_) => "str".to_string(),
            Value::Symbol(sym) => format!(
                "symbol[{}]",
                sym.variants().map(|v| v.1).collect::<Vec<_>>().join("|")
            ),
            _ => "other".to_string(),
        };
        for name in ["space", "math", "text", "mat", "frac", "equation", "calc"] {
            println!(
                "{name}: math={} global={}",
                lib.math
                    .scope()
                    .get(name)
                    .map(|b| describe(b.read()))
                    .unwrap_or("-".to_string()),
                lib.global
                    .scope()
                    .get(name)
                    .map(|b| describe(b.read()))
                    .unwrap_or("-".to_string()),
            );
        }
        let space = lib.math.scope().get("space").expect("常量");
        if let Value::Symbol(sym) = space.read() {
            println!("space 修饰符={:?}", sym.modifiers().collect::<Vec<_>>());
        }
    }

    /// typst 两个作用域的全部名字：给「mitex 吐出但 typst 不认」的名字找现名。
    /// `cargo test --lib typeset::math::tests::dump_typst_names -- --ignored --nocapture > names.txt`
    #[test]
    #[ignore]
    fn dump_typst_names() {
        let lib = Library::default();
        let dump = |label: &str, scope: &typst::foundations::Scope| {
            let mut names: Vec<&str> = scope.iter().map(|(n, _)| n.as_str()).collect();
            names.sort_unstable();
            println!("--- {label}（{} 个）", names.len());
            for n in names {
                println!("{n}");
            }
        };
        dump("math", lib.math.scope());
        dump("global", lib.global.scope());
    }
}
