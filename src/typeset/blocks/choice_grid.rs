//! 选项栅格：确定性估宽 → 1×4 / 2×2 / 4×1 决策（T2.5，R7 口径）
//!
//! 纯函数、零 typst 依赖，docx `w:tbl` 与 typst `grid()` 共用同一份判定，避免「两种格式
//! 排出两种列数」。宽度一律以 **em** 为单位（相对正文字号，docx 默认样式 10.5pt），
//! 由调用方把可用栏宽换算成 em 传入 [`decide`]，本模块不内置版面常量。
//!
//! **R7 校准**：旧口径「公式段按源字符数 ×0.55em」对 LaTeX 严重失真 —— `\frac{a+b}{c}`
//! 源串 12 字符，实际渲染只有 `a+b` 那么宽。故按**渲染后字形数**折算：剥掉命令名
//! （`\frac` 本身不印字）、宏参数各计一次（嵌套不重复累加）、上下标按半宽计。
//! 折算分支见 [`math_width`] 与 `Measurer::command`；未知命令按 1em 估 —— 宁可少排一列，
//! 也不要把溢出留在纸上。
//!
//! typst 端可用 `context measure()` 对本模块的判定做兜底复检（R7），那是 M3 的事。

use crate::export::model::{ExamOption, InlineNode};

/// 最宽选项 ≤ 可用栏宽 25% → 1 行 4 列（§6.2 决策表）
pub const FOUR_COLUMN_RATIO: f64 = 0.25;
/// ≤ 50% → 2 行 2 列；再宽则 4 行 1 列
pub const TWO_COLUMN_RATIO: f64 = 0.50;

/// CJK、全角标点与数学符号（含 TeX 给关系符的两侧间距）
const WIDE_EM: f64 = 1.0;
/// 其余非 ASCII 字符（希腊字母等）
const SYMBOL_EM: f64 = 0.8;
/// 数学模式里带自动间距的二元/关系运算符（`=`、`+`、`<`）：字形 0.55 + thick space
const MATH_OP_EM: f64 = 0.85;
/// ASCII 字母数字与普通标点
const ASCII_EM: f64 = 0.55;
/// 文本模式空格（数学模式里的空格无语义，计 0）
const TEXT_SPACE_EM: f64 = 0.3;
/// 上下标折算比例（R7）
const SCRIPT_RATIO: f64 = 0.5;
/// `\left( … \right)` 定界符（可拉伸，比 ASCII 括号宽）
const DELIM_EM: f64 = 0.6;
/// 根号钩
const RADICAL_EM: f64 = 0.6;
/// 上划线/帽子一类附加符号的额外宽度
const ACCENT_EM: f64 = 0.2;
/// 环境内列分隔符 `&` 留下的空隙
const CELL_SEP_EM: f64 = 0.5;
/// 细空格 `\,` `\:` `\;` `\!`
const THIN_EM: f64 = 0.17;
/// 反斜杠空格 `\ `
const BACKSLASH_SPACE_EM: f64 = 0.3;
/// 图片按 96dpi 折算：10.5pt 的 1em ≈ 14px
const PX_PER_EM: f64 = 14.0;
/// 版面尺寸换算：10.5pt 正文在 96dpi 下 1em ≈ 3.7mm（= 14px × 2.54/96）
pub const MM_PER_EM: f64 = 3.7;
/// 选项里的图片最小估宽（老数据没有 width 也不至于算成 0）
const IMAGE_MIN_EM: f64 = 3.0;
/// 表格里出现即单列，宽度不参与比较（给一个必然超阈值的常量）
const BLOCK_TABLE_EM: f64 = 30.0;

/// LaTeX 语法里带列描述符参数 `{cc}` 的环境：该参数只描述排版，渲染不占宽。
/// 名称与 `export::math` 的 `COLUMN_SPEC_ENVS` 部分重合但语义相反 —— 那边为了改写 matrix
/// 要剥掉它，这边为了「不把它算进宽度」要认出它，且环境集合取并集（matrix/cases 的行列
/// 分隔符已经由 `\\` 与 `&` 处理）。
const COLUMN_SPEC_ENVS: &[&str] = &[
    "array",
    "array*",
    "subarray",
    "alignat",
    "alignat*",
    "alignedat",
    "tabular",
    "darray",
];

/// 纯排版指令：渲染后完全不占宽（对应 `export::math` 里折算为空串的那批别名）
const SILENT_CMDS: &[&str] = &[
    "displaystyle",
    "textstyle",
    "scriptstyle",
    "scriptscriptstyle",
    "limits",
    "nolimits",
    "nonumber",
    "notag",
    "noindent",
    "centering",
    "relax",
];

/// 函数名类命令：命令名本身就是印出来的字母（`\log` 占 3 个字形），
/// 与 `\frac` 一类「名字只是宏」相区分 —— 这就是 R7「剥命令名」的边界。
const FUNCTION_WORDS: &[&str] = &[
    "log", "ln", "lg", "lb", "exp", "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos",
    "arctan", "sinh", "cosh", "tanh", "lim", "limsup", "liminf", "max", "min", "det", "dim", "deg",
    "gcd", "lcm", "mod", "arg", "Pr",
];

/// 数学模式里按「字形 + 两侧自动间距」计宽的运算符字符
const MATH_OPS: &[char] = &['+', '-', '=', '<', '>', '*', '/', '|', ':'];

// ═══════════════════════════════ 对外判定 ═══════════════════════════════

/// 版面尺寸（mm）→ em：给 [`decide`] 喂可用栏宽用
///
/// `LayoutSpec::column_width_mm` 出来的是毫米，docx 那边算的是 twips，两边都必须先落到
/// 本模块的 em 口径上，才谈得上「同一算法、两处渲染」。
pub fn em_from_mm(mm: f64) -> f64 {
    mm / MM_PER_EM
}

/// 选项栅格（供 typst `grid()` 与 docx `w:tbl` 共用；`rows` 由选项数与列数推出）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChoiceGrid {
    /// 列数：1 / 2 / 4（选项数不足时可能为 3）
    pub columns: usize,
    /// 行数 = ⌈选项数 / 列数⌉
    pub rows: usize,
}

/// 决策表：所有选项（含「A. 」标签）的最宽者 ÷ 可用栏宽 → 列数
///
/// 单列有三个触发条件：宽度比超过 [`TWO_COLUMN_RATIO`]、选项含换行/块级公式/表格/图组、
/// 公式内部按 `\\` 分行（分段函数、矩阵）。多行内容哪怕很窄也不横排 —— 一行里出现两个高矮
/// 不齐的公式块，比多占三行更难看。
pub fn decide(options: &[ExamOption], available_em: f64) -> ChoiceGrid {
    let count = options.len();
    if count <= 1 {
        return ChoiceGrid {
            columns: 1,
            rows: count,
        };
    }
    let widest = options.iter().map(option_width).fold(0.0_f64, f64::max);
    let blocked = options.iter().any(|o| requires_single_column(&o.content));
    let want = if available_em <= 0.0 || blocked {
        1
    } else {
        let ratio = widest / available_em;
        if ratio <= FOUR_COLUMN_RATIO {
            4
        } else if ratio <= TWO_COLUMN_RATIO {
            2
        } else {
            1
        }
    };
    let columns = want.clamp(1, count);
    ChoiceGrid {
        columns,
        rows: count.div_ceil(columns),
    }
}

/// 单个选项宽度（em）= 「标签. 」+ 内容。列间距由 writer 负责，不计入。
pub fn option_width(option: &ExamOption) -> f64 {
    label_width(&option.label) + inline_width(&option.content)
}

/// 「A. 」标签宽
fn label_width(label: &str) -> f64 {
    if label.is_empty() {
        0.0
    } else {
        text_width(&format!("{label}. "))
    }
}

/// 行内节点序列宽度（em）。遇到 `LineBreak` 按多行处理，取最宽的一行。
pub fn inline_width(nodes: &[InlineNode]) -> f64 {
    let mut cur = 0.0_f64;
    let mut widest_line = 0.0_f64;
    for n in nodes {
        match n {
            InlineNode::Text { text } => cur += text_width(text),
            InlineNode::LineBreak => {
                widest_line = widest_line.max(cur);
                cur = 0.0;
            }
            InlineNode::Math { latex, .. } => cur += math_width(latex).width,
            InlineNode::Image { width, .. } => cur += image_em(*width),
            InlineNode::ImgRow { images, .. } => {
                cur += images
                    .iter()
                    .fold(0.0_f64, |acc, im| acc + image_em(im.width));
            }
            InlineNode::Table { .. } => cur += BLOCK_TABLE_EM,
        }
    }
    widest_line.max(cur)
}

fn image_em(px: Option<u32>) -> f64 {
    px.map_or(IMAGE_MIN_EM, |px| (px as f64 / PX_PER_EM).max(IMAGE_MIN_EM))
}

/// 是否必须单列：显式换行、块级公式、表格、并排图组、公式内部多行
pub fn requires_single_column(nodes: &[InlineNode]) -> bool {
    nodes.iter().any(|n| match n {
        InlineNode::LineBreak | InlineNode::Table { .. } | InlineNode::ImgRow { .. } => true,
        InlineNode::Math { latex, display, .. } => *display || math_width(latex).multiline,
        _ => false,
    })
}

/// 纯文本宽度（em）：CJK/全角 1em、ASCII 0.55em、其余 0.8em
pub fn text_width(text: &str) -> f64 {
    text.chars().map(char_em).sum()
}

fn char_em(c: char) -> f64 {
    if c.is_ascii_whitespace() {
        TEXT_SPACE_EM
    } else if c.is_ascii() {
        ASCII_EM
    } else if is_wide(c) {
        WIDE_EM
    } else {
        SYMBOL_EM
    }
}

// ═══════════════════════════════ 公式估宽 ═══════════════════════════════

/// 一段 LaTeX 的渲染估宽
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MathWidth {
    /// 多行时取最宽的一行（em）
    pub width: f64,
    /// 是否含 `\\` 分行（分段函数、矩阵、align 等）—— 触发单列
    pub multiline: bool,
}

/// LaTeX → 渲染字形宽度（R7 口径）
///
/// 折算规则：
/// - 命令名不占宽（`\frac`、`\left`、环境名与列描述符），纯排版指令占 0；
/// - 上下标 `^{…}`/`_{…}` 按参数半宽计（它们挤在基字符右上方）；
/// - 竖排结构（`\frac` 的两参数、`cases`/`matrix` 的多行）取**较宽的一侧**而不是相加；
/// - 每层宏参数只计一次，嵌套不重复累加；
/// - 未知命令按 1em 估，偏保守。
pub fn math_width(latex: &str) -> MathWidth {
    let chars: Vec<char> = latex.chars().collect();
    Measurer { c: &chars, i: 0 }.measure(false)
}

struct Measurer<'a> {
    c: &'a [char],
    i: usize,
}

/// 命令的度量结果：累加宽度、是否多行、是否为换行符 `\\`
struct CmdOut {
    width: f64,
    multiline: bool,
    line_break: bool,
}

impl CmdOut {
    fn w(width: f64) -> Self {
        Self {
            width,
            multiline: false,
            line_break: false,
        }
    }
    fn nothing() -> Self {
        Self::w(0.0)
    }
    fn break_line() -> Self {
        Self {
            width: 0.0,
            multiline: true,
            line_break: true,
        }
    }
    fn with(mut self, multiline: bool) -> Self {
        self.multiline |= multiline;
        self
    }
}

impl Measurer<'_> {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.i += 1;
        }
        c
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    /// 读到 `}` 或输入结束。`text_mode` 用于 `\text{…}`：空格有意义、`\\` 与 `&` 不是排版记号。
    fn measure(&mut self, text_mode: bool) -> MathWidth {
        let mut lines = Vec::new();
        let mut cur = 0.0_f64;
        let mut multi = false;

        while let Some(c) = self.bump() {
            match c {
                '{' => {
                    let g = self.measure(text_mode);
                    cur += g.width;
                    multi |= g.multiline;
                }
                '}' => break,
                '$' => {}
                '\\' => {
                    let out = self.command(text_mode);
                    if out.line_break {
                        lines.push(cur);
                        cur = 0.0;
                        multi = true;
                    } else {
                        cur += out.width;
                        multi |= out.multiline;
                    }
                }
                '^' | '_' if !text_mode => {
                    let g = self.arg();
                    cur += SCRIPT_RATIO * g.width;
                    multi |= g.multiline;
                }
                '&' if !text_mode => cur += CELL_SEP_EM,
                '~' => cur += if text_mode { WIDE_EM } else { THIN_EM },
                c if c.is_ascii_whitespace() && !text_mode => {}
                c => {
                    cur += if text_mode {
                        char_em(c)
                    } else if is_wide(c) {
                        WIDE_EM
                    } else if MATH_OPS.contains(&c) {
                        MATH_OP_EM
                    } else if c.is_ascii() {
                        ASCII_EM
                    } else {
                        SYMBOL_EM
                    }
                }
            }
        }

        lines.push(cur);
        MathWidth {
            width: lines.into_iter().fold(0.0_f64, f64::max),
            multiline: multi,
        }
    }

    /// 反斜杠命令（游标已吃掉 `\`）
    fn command(&mut self, text_mode: bool) -> CmdOut {
        let name = self.command_name();
        match name.as_str() {
            "\\" => {
                if text_mode {
                    return CmdOut::nothing();
                }
                self.skip_optional('[');
                CmdOut::break_line()
            }
            // 定界符：名字不占宽，后面那个符号占
            "left" | "right" | "middle" | "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr"
            | "Bigl" | "Bigr" | "biggl" | "biggr" | "Biggl" | "Biggr" => {
                CmdOut::w(self.delimiter())
            }
            // 两参数竖排：取宽的一侧
            "frac" | "dfrac" | "tfrac" | "cfrac" | "binom" | "choose" | "atop" => {
                let a = self.arg();
                let b = self.arg();
                CmdOut::w(a.width.max(b.width)).with(a.multiline || b.multiline)
            }
            "overset" | "underset" | "stackrel" => {
                let a = self.arg();
                let b = self.arg();
                CmdOut::w(a.width.max(b.width) + ACCENT_EM).with(a.multiline || b.multiline)
            }
            "sqrt" => {
                let idx = self
                    .delimited_content('[')
                    .map(|s| text_width(&s))
                    .unwrap_or(0.0);
                let g = self.arg();
                CmdOut::w(RADICAL_EM + g.width + SCRIPT_RATIO * idx).with(g.multiline)
            }
            // 参数是排出来的文字，按文本折算
            "text" | "textrm" | "textbf" | "textit" | "textsf" | "mbox" | "operatorname" => {
                CmdOut::w(self.measure_text_arg().width)
            }
            // 参数照原样排（数学模式），命令名与附加符号几乎不额外占宽
            "mathrm" | "mathbf" | "mathbb" | "mathcal" | "mathit" | "mathsf" | "mathfrak"
            | "mathop" | "boldsymbol" | "bm" | "hat" | "widehat" | "vec" | "overrightarrow"
            | "bar" | "overline" | "underline" | "dot" | "ddot" | "tdot" | "tilde"
            | "widetilde" | "overbrace" | "underbrace" => {
                let g = self.arg();
                CmdOut::w(g.width + ACCENT_EM).with(g.multiline)
            }
            // 环境：名字与列描述符都不占宽，分行交给 `\\` 分支
            "begin" | "end" => {
                let env = self.raw_brace_arg();
                if name == "begin" && COLUMN_SPEC_ENVS.contains(&env.as_str()) {
                    self.skip_brace_arg();
                }
                CmdOut::nothing()
            }
            "quad" => CmdOut::w(1.0),
            "qquad" => CmdOut::w(2.0),
            "hspace" | "hspace*" | "kern" | "hfill" => CmdOut::w(
                self.delimited_content('{')
                    .map(|s| parse_length_em(&s))
                    .unwrap_or(0.0),
            ),
            "," | ":" | ";" | "!" => CmdOut::w(THIN_EM),
            " " => CmdOut::w(BACKSLASH_SPACE_EM),
            // 转义出来的普通字符（`\{` `\%` `\_` …）
            s if s.chars().count() == 1 && !s.starts_with(|c: char| c.is_ascii_alphabetic()) => {
                CmdOut::w(text_width(s))
            }
            _ if FUNCTION_WORDS.contains(&name.as_str()) => {
                CmdOut::w(name.chars().count() as f64 * ASCII_EM)
            }
            _ if SILENT_CMDS.contains(&name.as_str()) => CmdOut::nothing(),
            _ if text_mode => CmdOut::nothing(),
            // 未知命令：按一个字形估
            _ => CmdOut::w(WIDE_EM),
        }
    }

    /// `\` 之后的命令名：字母数字串（含 `*`），或单个非字母字符
    fn command_name(&mut self) -> String {
        match self.peek() {
            Some(c) if c.is_ascii_alphabetic() => {
                let mut s = String::new();
                while let Some(c) = self.peek() {
                    if c.is_ascii_alphanumeric() || c == '*' {
                        s.push(c);
                        self.i += 1;
                    } else {
                        break;
                    }
                }
                s
            }
            Some(c) => {
                self.bump();
                c.to_string()
            }
            None => String::new(),
        }
    }

    /// 定界符符号：`\left(`、`\right\{`、`\right.`（占位用的 `.` 不占宽）
    fn delimiter(&mut self) -> f64 {
        self.skip_spaces();
        let raw = match self.peek() {
            Some('\\') => {
                self.bump();
                self.command_name()
            }
            Some(c) => {
                self.bump();
                c.to_string()
            }
            None => return 0.0,
        };
        if raw == "." || raw.is_empty() {
            0.0
        } else {
            DELIM_EM
        }
    }

    /// 取一个参数：`{…}` 组、命令（`\frac12` 形态）、或单个字符
    fn arg(&mut self) -> MathWidth {
        self.skip_spaces();
        match self.peek() {
            Some('{') => {
                self.bump();
                self.measure(false)
            }
            Some('\\') => {
                self.bump();
                let out = self.command(false);
                MathWidth {
                    width: out.width,
                    multiline: out.multiline,
                }
            }
            Some(c) => {
                self.bump();
                MathWidth {
                    width: char_em(c),
                    multiline: false,
                }
            }
            None => MathWidth {
                width: 0.0,
                multiline: false,
            },
        }
    }

    /// `\text{…}` 形态的参数：按文本模式度量
    fn measure_text_arg(&mut self) -> MathWidth {
        self.skip_spaces();
        if self.peek() == Some('{') {
            self.bump();
            self.measure(true)
        } else {
            self.arg()
        }
    }

    /// 读 `{…}` 原文（环境名），不匹配时返回空串
    fn raw_brace_arg(&mut self) -> String {
        self.skip_spaces();
        self.delimited_content('{').unwrap_or_default()
    }

    /// 跳过一组 `{…}`（列描述符）
    fn skip_brace_arg(&mut self) {
        self.skip_spaces();
        if self.peek() == Some('{') {
            self.delimited_content('{');
        }
    }

    /// 读取被 `open` 引导的配对内容（`{…}` 或 `[…]`），未命中返回 None
    fn delimited_content(&mut self, open: char) -> Option<String> {
        let close = match open {
            '{' => '}',
            _ => ']',
        };
        self.skip_spaces();
        if self.peek() != Some(open) {
            return None;
        }
        self.bump();
        let mut depth = 1_usize;
        let mut s = String::new();
        while let Some(c) = self.bump() {
            if c == open {
                depth += 1;
                s.push(c);
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                s.push(c);
            } else {
                s.push(c);
            }
        }
        Some(s)
    }

    /// 可选配对参数（如 `\\[6pt]`）：存在则吃掉
    fn skip_optional(&mut self, open: char) {
        self.skip_spaces();
        if self.peek() == Some(open) {
            self.delimited_content(open);
        }
    }
}

/// `1em` / `6pt` / `2mm` / `0.5cm` / `20px` → em；解析不出来按细空格估
fn parse_length_em(raw: &str) -> f64 {
    let s = raw.trim();
    let digits: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+')
        .collect();
    let Ok(v) = digits.parse::<f64>() else {
        return if s.is_empty() { 0.0 } else { THIN_EM };
    };
    let unit: String = s[digits.len()..]
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    // 1em = 10.5pt ≈ 3.7mm ≈ 0.37cm ≈ 14px
    match unit.as_str() {
        "" | "em" => v,
        "pt" => v / 10.5,
        "mm" => v / 3.7,
        "cm" => v / 0.37,
        "px" => v / PX_PER_EM,
        _ => v,
    }
}

/// 东亚文字、全角标点、数学符号与箭头按整宽计
fn is_wide(c: char) -> bool {
    let u = c as u32;
    (0x1100..=0x115F).contains(&u)
        || (0x2E80..=0x303F).contains(&u)
        || (0x31C0..=0x33FF).contains(&u)
        || (0x4E00..=0x9FFF).contains(&u)
        || (0xAC00..=0xD7A3).contains(&u)
        || (0xF900..=0xFAFF).contains(&u)
        || (0xFE30..=0xFE6F).contains(&u)
        || (0xFF00..=0xFF60).contains(&u)
        || (0xFFE0..=0xFFE6).contains(&u)
        // 广义标点与符号（— ‘ “ ≤ ≥ ∪ ∩ → ⇒ …）：TeX 还给运算符两侧留间距
        || (0x2000..=0x2BFF).contains(&u)
}

// ═══════════════════════════════ 测试 ═══════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// A4 单栏正文宽（21cm − 2×2.5cm = 16cm，1em ≈ 0.37cm）
    const A4: f64 = 43.0;

    fn text(s: &str) -> InlineNode {
        InlineNode::Text { text: s.into() }
    }

    fn math(latex: &str) -> InlineNode {
        InlineNode::Math {
            latex: latex.into(),
            display: false,
        }
    }

    fn opts(list: &[&str]) -> Vec<ExamOption> {
        ["A", "B", "C", "D"]
            .iter()
            .zip(list)
            .map(|(l, s)| ExamOption {
                label: (*l).into(),
                content: vec![text(s)],
            })
            .collect()
    }

    fn math_opts(list: &[&str]) -> Vec<ExamOption> {
        ["A", "B", "C", "D"]
            .iter()
            .zip(list)
            .map(|(l, s)| ExamOption {
                label: (*l).into(),
                content: vec![math(s)],
            })
            .collect()
    }

    fn near(v: f64, want: f64) -> bool {
        (v - want).abs() < 0.06
    }

    #[test]
    fn em_model_for_text() {
        assert!(near(text_width("abcd"), 2.2));
        assert!(near(text_width("甲乙丙"), 3.0));
        assert!(near(text_width("，"), 1.0), "全角标点与 CJK 同宽");
        assert!(near(text_width("a b"), 1.4), "文本模式空格计宽");
        assert!(near(math_width("a b").width, 1.1), "数学模式空格无语义");
    }

    #[test]
    fn frac_counts_rendered_glyphs_not_source_chars() {
        // R7 的原始动机：`\frac{a+b}{c}` 源串 12 字符，旧口径 ×0.55 = 6.6em，实际只有分子宽
        let w = math_width(r"\frac{a+b}{c}").width;
        assert!(
            near(w, 1.95),
            "分子 a(0.55)+ (0.85) +b(0.55)=1.95 分母 0.55，取宽的一侧: {w}"
        );
        assert!(w < 3.0, "旧口径会给到 6.6em: {w}");
        // 嵌套只计一次：外层宽度由内层折算结果贡献，不逐层累加参数
        let nest = math_width(r"\frac{\frac{a+b}{c}}{d}").width;
        assert!(near(nest, 1.95), "嵌套分式仍是最宽的一层: {nest}");
    }

    #[test]
    fn scripts_count_half_width() {
        assert!(near(math_width("x^{10}").width, 1.1));
        assert!(near(math_width("x_{i}^{2}").width, 1.1), "上下标各算一半");
        assert!(near(math_width("x^2").width, 0.825), "单字符上标无需花括号");
        assert!(near(math_width(r"x^\alpha").width, 1.05));
    }

    #[test]
    fn known_command_shapes() {
        assert!(near(math_width(r"\sqrt{x}").width, 1.15), "根号钩 + x");
        assert!(near(math_width(r"\sqrt[3]{x}").width, 1.425), "次数按半宽");
        assert!(near(math_width(r"\text{分钟}").width, 2.0));
        assert!(
            near(math_width(r"\log").width, 1.65),
            "函数名是排出来的字母"
        );
        assert!(near(math_width(r"\,").width, 0.17));
        assert!(near(math_width(r"\quad").width, 1.0));
        assert!(near(math_width(r"\left(x\right)").width, 1.75));
        assert!(
            near(math_width(r"\left.x\right.").width, 0.55),
            "`.` 占位定界符不占宽"
        );
        assert!(near(math_width(r"\displaystyle\frac{1}{2}").width, 0.55));
        assert!(near(math_width(r"\%").width, 0.55), "转义字符按原字符计");
        assert!(
            near(math_width(r"\thicksymbol").width, 1.0),
            "未知命令保守估 1em"
        );
        assert!(
            near(math_width(r"\begin{array}{cc}1\end{array}").width, 0.55),
            "列描述符不占宽"
        );
    }

    #[test]
    fn row_breaks_take_widest_line_and_mark_multiline() {
        let cases = math_width(r"\begin{cases}x^2,&x\ge 0\\-x,&x<0\end{cases}");
        assert!(cases.multiline, "分段函数是多行结构");
        // 第二行：-(0.85) x(0.55) ,(0.55) &(0.5) x(0.55) <(0.85) 0(0.55) = 4.4，比第一行宽
        assert!(
            near(cases.width, 4.4),
            "宽度取较宽的一行而不是两行相加: {}",
            cases.width
        );
        assert!(!math_width(r"\begin{cases}x\end{cases}").multiline);
        // `\\` 的行距可选参数不占宽
        let spaced = math_width(r"\begin{matrix}a\\[6pt]b\end{matrix}");
        assert!(
            spaced.multiline && near(spaced.width, 0.55),
            "两行各一个字符，行距参数不计宽: {spaced:?}"
        );
    }

    #[test]
    fn nested_multiline_formula_propagates_without_splitting_outer_line() {
        let w = math_width(r"1+\frac{\begin{cases}a\\b\end{cases}}{2}");
        assert!(w.multiline, "内部分行要冒泡到选项级判定");
        assert!(
            near(w.width, 1.95),
            "外层仍是一行：1(0.55) + (0.85) + 分式取 max(0.55,0.55): {}",
            w.width
        );
    }

    #[test]
    fn inline_node_widths() {
        assert!(near(
            inline_width(&[text("甲"), math("x"), text("乙")]),
            2.55
        ));
        // 换行取最宽的一行
        assert!(near(
            inline_width(&[text("甲乙"), InlineNode::LineBreak, text("丙")]),
            2.0
        ));
        // 图片按 px→em 折算并有下限
        assert!(near(
            inline_width(&[InlineNode::Image {
                alt: None,
                url: "a.png".into(),
                width: Some(70),
                align: None,
            }]),
            5.0
        ));
        assert!(near(
            inline_width(&[InlineNode::Image {
                alt: None,
                url: "a.png".into(),
                width: None,
                align: None,
            }]),
            IMAGE_MIN_EM
        ));
        // 「A. 」= A(0.55) .(0.55) 空格(0.3) = 1.4
        assert!(near(
            option_width(&ExamOption {
                label: "A".into(),
                content: vec![text("甲")],
            }),
            2.4
        ));
    }

    #[test]
    fn short_options_go_four_across() {
        let grid = decide(&opts(&["1", "2", "3", "4"]), A4);
        assert_eq!(
            grid,
            ChoiceGrid {
                columns: 4,
                rows: 1
            }
        );
    }

    #[test]
    fn medium_options_go_two_by_two() {
        let medium = "甲、乙两地的距离是 12 千米";
        let w = option_width(&ExamOption {
            label: "A".into(),
            content: vec![text(medium)],
        });
        assert!(
            w / A4 > FOUR_COLUMN_RATIO && w / A4 <= TWO_COLUMN_RATIO,
            "用例宽度 {w} 需落在 2×2 区间（10.75 ~ 21.5em）"
        );
        assert_eq!(
            decide(&opts(&[medium; 4]), A4),
            ChoiceGrid {
                columns: 2,
                rows: 2
            }
        );
    }

    #[test]
    fn long_options_stack_to_one_column() {
        let long = "一个超过半栏宽很多的选项内容，例如把整句话塞进选项里";
        assert!(
            option_width(&ExamOption {
                label: "A".into(),
                content: vec![text(long)],
            }) / A4
                > TWO_COLUMN_RATIO
        );
        assert_eq!(
            decide(&opts(&[long; 4]), A4),
            ChoiceGrid {
                columns: 1,
                rows: 4
            }
        );
    }

    #[test]
    fn formula_options_use_rendered_width() {
        // 四个 `\frac{a+b}{c}`：R7 口径 1.4+1.95=3.35em → 14em 栏宽里排 4 列；
        // 旧口径 1.4+6.6=8em 会被误判成单列
        let four = math_opts(&[r"\frac{a+b}{c}"; 4]);
        assert_eq!(decide(&four, 14.0).columns, 4);
        assert_eq!(decide(&four, 10.0).columns, 2);
        assert!(near(option_width(&four[0]), 3.35));
    }

    #[test]
    fn multiline_or_block_content_forces_single_column() {
        assert_eq!(
            decide(&math_opts(&[r"\begin{cases}x^2\\y\end{cases}"; 4]), A4).columns,
            1,
            "含多行公式，哪怕很窄也单列"
        );
        let broken = ["A", "B", "C", "D"]
            .iter()
            .map(|l| ExamOption {
                label: (*l).into(),
                content: vec![text("甲"), InlineNode::LineBreak, text("乙")],
            })
            .collect::<Vec<_>>();
        assert_eq!(
            decide(&broken, A4),
            ChoiceGrid {
                columns: 1,
                rows: 4
            }
        );
        assert!(requires_single_column(&[InlineNode::Math {
            latex: "x".into(),
            display: true,
        }]));
        assert!(requires_single_column(&[InlineNode::Table {
            header: vec!["x".into()],
            aligns: vec![],
            rows: vec![],
        }]));
        assert!(requires_single_column(&[InlineNode::ImgRow {
            align: None,
            images: vec![],
            caption: None,
        }]));
        assert!(!requires_single_column(&[text("甲"), math("x")]));
    }

    #[test]
    fn columns_never_exceed_option_count() {
        assert_eq!(
            decide(&opts(&["1", "2", "3"]), A4),
            ChoiceGrid {
                columns: 3,
                rows: 1
            }
        );
        assert_eq!(
            decide(&opts(&["1"]), A4),
            ChoiceGrid {
                columns: 1,
                rows: 1
            }
        );
        assert_eq!(
            decide(&[], A4),
            ChoiceGrid {
                columns: 1,
                rows: 0
            }
        );
        let five = ["A", "B", "C", "D", "E"]
            .iter()
            .map(|l| ExamOption {
                label: (*l).into(),
                content: vec![text("1")],
            })
            .collect::<Vec<_>>();
        assert_eq!(
            decide(&five, A4),
            ChoiceGrid {
                columns: 4,
                rows: 2
            }
        );
    }

    #[test]
    fn degenerate_available_width_is_single_column() {
        assert_eq!(decide(&opts(&["1"; 4]), 0.0).columns, 1);
        assert_eq!(decide(&opts(&["1"; 4]), -1.0).columns, 1);
    }

    #[test]
    fn length_units() {
        assert!(near(parse_length_em("1em"), 1.0));
        assert!(near(parse_length_em("10.5pt"), 1.0));
        assert!(near(parse_length_em("3.7mm"), 1.0));
        assert!(near(parse_length_em("14px"), 1.0));
        assert!(near(parse_length_em("2cm"), 5.4));
        assert!(near(parse_length_em(""), 0.0));
        assert!(near(math_width(r"\hspace{1em}x").width, 1.55));
    }

    #[test]
    fn malformed_latex_never_panics() {
        for raw in [
            r"\frac{1}{",
            r"\sqrt",
            r"\begin{cases}x",
            r"}{",
            r"\left(",
            r"x^",
            r"\hspace{abc}",
            r"\begin{array}",
            r"\text{未闭合",
            r"{[[{",
            "\\",
            "\\\\",
            "]",
            "}",
            "",
            "\u{0}",
        ] {
            let _ = math_width(raw);
            let _ = requires_single_column(&[math(raw)]);
        }
    }
}
