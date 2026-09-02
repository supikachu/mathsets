//! Presentation MathML → OMML（T2.4）
//!
//! 规则逐条对应微软官方 `MML2OMML.XSL` 的模板（下文每个函数都标了模板名）。按修订 R2，XSL 本体
//! 不入库也不在运行时执行：`tests/snapshots/*.omml` 是开发期用外部 XSLT 引擎跑官方 XSL 得到的
//! **事实输出**，[`to_omml`] 的产物必须在 XML 规范化后与之逐节点一致（见文件末尾的黄金快照测试）。
//! 因此这里照抄 XSL 的判断顺序与默认值，包括它自身偏保守的取舍（颜色属性直接丢弃、`mspace`
//! 整节点消失等）。
//!
//! 三处与 XSL 有据可查的差异，都不影响本管线可达的输入：
//!
//! 1. **mglyph 分支未实现**：`latex2mathml` 不产 `mglyph`，凡带 `mml:*[child::mml:mglyph]` 的
//!    模板与 `mglyph` 自身模板都不可达；真遇到了会走「未认出 → 递归子节点 + 警告」的降级路径。
//! 2. **`maligngroup` / `malignmark` 不处理**：crate 同样不产，`ProcessEqArrayRow` 里的对齐点
//!    抽取分支随之省略。
//! 3. **属性不区分 `@foo` 与 `@mml:foo`**：XSL 全程「先取不带前缀的写法，取不到再退 `mml:` 前缀」，
//!    而 XML 里不带前缀的属性不属于任何命名空间。`roxmltree` 的 `attribute(local_name)` 按 local
//!    name 匹配，一次查询即覆盖两种写法。两种情形会与 XSL 不同：同一节点两种拼法都写（本实现取
//!    文档序第一个），以及只写 `mml:` 前缀（XSL 的部分选择器会取空、落个空串，本实现仍拿到真值）。
//!    `latex2mathml` 产出的都是默认命名空间下的无 prefix 写法，两种情形都不可达。

use std::borrow::Cow;
use std::io::Cursor;

use quick_xml::escape;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::writer::Writer;
use roxmltree::Node;

use super::MathOutcome;

/// roxmltree 的 `Node` 带两个生命周期（文档 / 输入文本），本模块统一用同名的简写，
/// 因为解析出的 `Document` 始终活在整个转换期间。
type Nd<'a> = Node<'a, 'a>;

/// OMML 命名空间，输出里的 `m:` 前缀
const OMML_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
/// MathML 命名空间，官方 XSL 在根节点上连带声明（即使片段里没有 `mml:` 节点）
const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";
/// function-app 算符（U+2061）：XSL 用它识别 omml 的 `m:func`
const FUNCTION_APP: char = '\u{2061}';

/// MathML → OMML 片段。
///
/// 输入是 [`super::to_mathml`] 的产物（良构 Presentation MathML），输出
/// `<m:oMath xmlns:m="…" xmlns:mml="…">…</m:oMath>`，可直接嵌进 `w:p`；
/// `math/@display` 对应的 `m:oMathPara` 由 docx writer 决定（XSL 的 `match="/"` 恒出 `m:oMath`）。
///
/// 与 [`super::to_mathml`] 同一套容错约定：解析失败 / 根节点不是 `math` → [`MathOutcome::Failed`]，
/// 调用方降级为「原文 + 警告」，绝不让单题失败中断整卷（实施计划 §5.3）。单个构造认不出来不算
/// 失败：照 XSL 的 catch-all 递归子节点并记 `warn`。
pub fn to_omml(mathml: &str) -> MathOutcome {
    let doc = match roxmltree::Document::parse(mathml) {
        Ok(doc) => doc,
        Err(e) => return MathOutcome::Failed(format!("MathML 非良构: {e}")),
    };
    let root = doc.root_element();
    if local(root) != "math" {
        return MathOutcome::Failed("MathML 根节点不是 <math>".to_string());
    }

    let mut conv = Omml::new();
    conv.open_root();
    for child in kids(root) {
        conv.element(child);
    }
    conv.close_root();
    MathOutcome::Ok(conv.finish())
}

// ---------------------------------------------------------------------------
// 节点访问
// ---------------------------------------------------------------------------

/// 取 local name。本模块只按 local name 分发：`to_mathml` 的输出统一落在 MathML 默认命名空间下，
/// 非 MathML 命名空间的元素在 XSL 里同样落到 catch-all，行为差别只有「是否警告」。
fn local(n: Nd<'_>) -> &str {
    n.tag_name().name()
}

/// 属性值。按 local name 匹配，故 `@foo` 与 `@mml:foo` 两种写法一次覆盖（见模块头第 3 条）。
fn attr<'a>(n: Nd<'a>, name: &str) -> Option<&'a str> {
    n.attribute(name)
}

/// 属性值，缺省为空串（对应 XSL 里 `not(@x)` 与 `@x=''` 合并判断的写法）
fn attr_or<'a>(n: Nd<'a>, name: &str) -> &'a str {
    attr(n, name).unwrap_or("")
}

/// 元素子节点（对应 XSL 的 `child::*`，文本节点不算）
fn kids(n: Nd<'_>) -> Vec<Nd<'_>> {
    n.children().filter(|c| c.is_element()).collect()
}

/// XPath string-value：所有后代文本节点按文档序拼接
fn string_value(n: Nd<'_>) -> String {
    n.descendants()
        .skip(1)
        .filter(|d| d.is_text())
        .filter_map(|d| d.text())
        .collect()
}

/// XSLT `normalize-space()`：trim + 内部空白串折叠成单个 U+0020
fn normalize_space(s: &str) -> Cow<'_, str> {
    let mut out = String::with_capacity(s.len());
    let mut pending = false;
    for ch in s.chars() {
        if matches!(ch, ' ' | '\t' | '\n' | '\r') {
            pending = !out.is_empty();
        } else {
            if pending {
                out.push(' ');
            }
            pending = false;
            out.push(ch);
        }
    }
    if out == s {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(out)
    }
}

/// XSL `translate(s, $StrUCAlphabet, $StrLCAlphabet)`：只折 ASCII 大写
fn xsl_lower(s: &str) -> Cow<'_, str> {
    if !s.is_ascii() || !s.contains(|c: char| c.is_ascii_uppercase()) {
        return Cow::Borrowed(s);
    }
    Cow::Owned(s.chars().map(|c| c.to_ascii_lowercase()).collect())
}

/// XSL `translate(…, '12345678', '99999999')` 后找 `9`
fn has_nonzero_digit(s: &str) -> bool {
    s.chars().any(|c| matches!(c, '1'..='9'))
}

/// XSL `translate(…, '123456789', '000000000')` 后找 `0`
fn has_digit(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit())
}

/// XSL `FFull`：无量纲字符串按「占满」处理，只有出现数字且全为 0 才算压扁
fn is_full(s: &str) -> bool {
    if has_nonzero_digit(s) {
        true
    } else {
        !has_digit(s)
    }
}

/// XSL `string(number(text)) != 'NaN'`：XPath 1.0 数值字面量文法
fn is_xsl_number(s: &str) -> bool {
    let t = s.trim();
    let split = t.find(['e', 'E']);
    let (mant, exp) = match split {
        Some(i) => (&t[..i], Some(&t[i + 1..])),
        None => (t, None),
    };
    let mant_ok = {
        let m = mant.strip_prefix(['+', '-']).unwrap_or(mant);
        let mut digits = 0usize;
        let mut dot = false;
        let mut ok = true;
        for c in m.chars() {
            match c {
                '0'..='9' => digits += 1,
                '.' if !dot => dot = true,
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        ok && digits > 0
    };
    let exp_ok = exp.is_none_or(|e| {
        let e = e.strip_prefix(|c| c == '+' || c == '-').unwrap_or(e);
        !e.is_empty() && e.chars().all(|c| c.is_ascii_digit())
    });
    mant_ok && exp_ok
}

/// 属性值是 XSL 布尔串 `'true'`（大小写不敏感，因为 XSL 先 translate）
fn flag_true(n: Nd<'_>, name: &str) -> bool {
    xsl_lower(attr_or(n, name)) == "true"
}

/// 属性值为空 / 缺失，或等于 `none`
fn none_like(n: Nd<'_>, name: &str) -> bool {
    matches!(xsl_lower(attr_or(n, name)).as_ref(), "" | "none")
}

// ---------------------------------------------------------------------------
// token 属性判定
// (XSL: FNonGlyphToken / FStartOfRun / GetFontCur / FNor / isNary / FIsBar / FIsAcc / FIsGroupChr)
// ---------------------------------------------------------------------------

fn is_token(n: Nd<'_>) -> bool {
    matches!(local(n), "mi" | "mn" | "mo" | "ms" | "mtext")
}

fn is_token_elem(n: Option<Nd<'_>>) -> bool {
    n.is_some_and(is_token)
}

/// XSL `FNonGlyphToken`：mglyph 分支不可达，故等价于 [`is_token`]
fn is_non_glyph_token(n: Nd<'_>) -> bool {
    is_token(n)
}

/// XSL `string-length(normalize-space(.))`，按字符计
fn text_len(n: Nd<'_>) -> usize {
    normalize_space(&string_value(n)).chars().count()
}

/// XSL `normalize-space($ndCur)`：token 要写进 `m:t` 的文本（尚未过 OutputText）
fn token_text(n: Nd<'_>) -> String {
    normalize_space(&string_value(n)).into_owned()
}

/// XSL `GetFontCur`：由 `mathvariant` / `fontstyle` / `fontweight` 折算 omml 的字体语义
fn font_cur(n: Nd<'_>) -> String {
    let variant = attr_or(n, "mathvariant");
    if !variant.is_empty() {
        return variant.to_string();
    }
    let style = attr_or(n, "fontstyle");
    let weight = attr_or(n, "fontweight");
    // 默认 italic + normal 的一类：单字符标识符、可解析成数的 mn、以及所有 mo
    let default_italic = match local(n) {
        "mi" => text_len(n) <= 1,
        "mn" => is_xsl_number(&string_value(n)),
        "mo" => true,
        _ => false,
    };
    if default_italic {
        if style == "normal" && weight == "bold" {
            "bold".into()
        } else if style == "normal" {
            "normal".into()
        } else if weight == "bold" {
            "bi".into()
        } else {
            "italic".into()
        }
    } else if style == "italic" && weight == "bold" {
        "bi".into()
    } else if style == "italic" {
        "italic".into()
    } else if weight == "bold" {
        "bold".into()
    } else {
        "normal".into()
    }
}

/// XSL `FNor`：只有 `mtext` 对应 omml 的 normal 样式（`m:nor`）
fn is_nor(n: Nd<'_>) -> bool {
    local(n) == "mtext"
}

/// XSL `isNaryOper`：可作为 n-ary 运算符的字符
fn is_nary_oper(text: &str) -> bool {
    matches!(
        text,
        "\u{222B}"
            | "\u{222C}"
            | "\u{222D}"
            | "\u{222E}"
            | "\u{222F}"
            | "\u{2230}"
            | "\u{2232}"
            | "\u{2233}"
            | "\u{2231}"
            | "\u{2229}"
            | "\u{222A}"
            | "\u{220F}"
            | "\u{2210}"
            | "\u{2211}"
            | "\u{22C0}"
            | "\u{22C1}"
            | "\u{22C2}"
            | "\u{22C3}"
    )
}

/// XSL `CreateNaryProp` 里 `m:grow` 认定的 13 个算符：n-ary 字符里只有这些会长大
/// （U+222C / U+222D / U+2210 / U+2231 是 n-ary 但不在此列）
const NARY_GROW_CHARS: &[&str] = &[
    "\u{222B}", "\u{222E}", "\u{222F}", "\u{2232}", "\u{2233}", "\u{2229}", "\u{222A}", "\u{220F}",
    "\u{2211}", "\u{22C0}", "\u{22C1}", "\u{22C2}", "\u{22C3}",
];

/// XSL `isNary`：`n` 是否为 n-ary 运算符 —— 文本是 n-ary 字符、未被 accent、
/// 且 descendant-or-self 链上只出现 mo/mstyle/mrow，最后一个元素是 mo
fn is_nary(n: Nd<'_>) -> bool {
    if !is_nary_oper(&token_text(n)) || accent_flag(n) {
        return false;
    }
    let mut last: Option<Nd<'_>> = None;
    for d in n.descendants().filter(|d| d.is_element()) {
        if !matches!(local(d), "mo" | "mstyle" | "mrow") {
            return false;
        }
        last = Some(d);
    }
    last.is_some_and(|d| local(d) == "mo")
}

/// XSL `isNary` 里的 accent 判定：父节点是 munder 看 `accentunder`，否则看 `accent`
fn accent_flag(n: Nd<'_>) -> bool {
    let Some(parent) = n.parent().filter(|p| p.is_element()) else {
        return false;
    };
    flag_true(
        parent,
        if local(parent) == "munder" {
            "accentunder"
        } else {
            "accent"
        },
    )
}

/// XSL `FIsBar`：`n` 是 munder/mover、非 accent，且第二子是 bar 字符的 mo
fn is_bar(n: Nd<'_>) -> bool {
    let under = local(n) == "munder";
    if flag_true(n, if under { "accentunder" } else { "accent" }) {
        return false;
    }
    let children = kids(n);
    let Some(&second) = children.get(1) else {
        return false;
    };
    if local(second) != "mo" {
        return false;
    }
    let operator = string_value(second);
    if under {
        matches!(operator.as_str(), "\u{0332}" | "\u{005F}")
    } else {
        matches!(operator.as_str(), "\u{0305}" | "\u{00AF}")
    }
}

/// XSL `FIsAcc`：mover 且 accent 为 true，第二子是单字符 mo。
/// `mo/@accent` 优先于 `mover/@accent`，前者缺失或为空时才看后者。
fn is_acc(n: Nd<'_>) -> bool {
    let children = kids(n);
    let Some(&second) = children.get(1) else {
        return false;
    };
    if local(second) != "mo" {
        return false;
    }
    let mo_accent = attr_or(second, "accent");
    let accented = xsl_lower(mo_accent) == "true"
        || (mo_accent.is_empty() && xsl_lower(attr_or(n, "accent")) == "true");
    accented && string_value(second).chars().count() <= 1
}

/// XSL `FIsGroupChr`：accent 显式为 false，两子是 (mrow, mo) 或 (mo, mrow)，且 mo 文本不超过一字符
fn is_group_chr(n: Nd<'_>) -> bool {
    if !matches!(local(n), "munder" | "mover") {
        return false;
    }
    let under = local(n) == "munder";
    if xsl_lower(attr_or(n, if under { "accentunder" } else { "accent" })) != "false" {
        return false;
    }
    let children = kids(n);
    if children.len() != 2 {
        return false;
    }
    let (a, b) = (local(children[0]), local(children[1]));
    if !((a == "mrow" && b == "mo") || (a == "mo" && b == "mrow")) {
        return false;
    }
    // XSL `string-length($ndCur/child::mml:mo)`：取第一个 mo 子的 string-value
    children
        .iter()
        .copied()
        .find(|c| local(*c) == "mo")
        .is_some_and(|mo| string_value(mo).chars().count() <= 1)
}

/// XSL `ToUpperCombining`：非组合字符换成对应的上组合字符
fn to_upper_combining(ch: &str) -> &str {
    match ch {
        "\u{02D8}" => "\u{0306}", // BREVE
        "\u{00B8}" => "\u{0312}", // CEDILLA
        "\u{0060}" => "\u{0300}", // GRAVE
        "\u{002D}" => "\u{0305}", // HYPHEN-MINUS → OVERLINE
        "\u{2212}" => "\u{0305}", // MINUS → OVERLINE
        "\u{002E}" => "\u{0307}", // FULL STOP → DOT ABOVE
        "\u{02D9}" => "\u{0307}", // DOT ABOVE
        "\u{02DD}" => "\u{030B}", // DOUBLE ACUTE
        "\u{00B4}" => "\u{0301}", // ACUTE
        "\u{007E}" => "\u{0303}", // TILDE
        "\u{02DC}" => "\u{0303}", // SMALL TILDE
        "\u{00A8}" => "\u{0308}", // DIAERESIS
        "\u{02C7}" => "\u{030C}", // CARON
        "\u{005E}" => "\u{0302}", // CIRCUMFLEX
        "\u{00AF}" => "\u{0305}", // MACRON
        "\u{2192}" => "\u{20D7}", // RIGHTWARDS ARROW
        "\u{27F6}" => "\u{20D7}", // LONG RIGHTWARDS ARROW
        "\u{2190}" => "\u{20D6}", // LEFTWARDS ARROW
        other => other,
    }
}

/// XSL `FIsNaryArgument`：`n` 是否紧跟在 n-ary 结构之后（其内容已由该结构的 `m:e` 吞下）
fn is_nary_argument(n: Nd<'_>) -> bool {
    let Some(prev) = n.prev_sibling_element() else {
        return false;
    };
    if !matches!(
        local(prev),
        "munder" | "mover" | "munderover" | "msub" | "msup" | "msubsup"
    ) {
        return false;
    }
    kids(prev).first().is_some_and(|first| is_nary(*first))
}

/// XSL `FLinearFrac`：`mrow` 三子且中子为 `/` 的 mo —— 对应 omml 的线性分数
fn is_linear_frac(n: Nd<'_>) -> bool {
    let children = kids(n);
    local(n) == "mrow"
        && children.len() == 3
        && local(children[1]) == "mo"
        && normalize_space(&string_value(children[1])) == "/"
}

/// XSL `FIsFunc`：`mrow` 三子且中子为 U+2061 的 mo —— 对应 omml 的 `m:func`
fn is_func(n: Nd<'_>) -> bool {
    let children = kids(n);
    local(n) == "mrow"
        && children.len() == 3
        && local(children[1]) == "mo"
        && normalize_space(&string_value(children[1]))
            .chars()
            .eq([FUNCTION_APP])
}

/// XSL `FBar`：`linethickness` 是否仍表示「有线」
fn has_bar(line_thickness: &str) -> bool {
    let t = xsl_lower(line_thickness);
    if t.is_empty() || matches!(t.as_ref(), "thin" | "medium" | "thick") {
        return true;
    }
    has_nonzero_digit(&t)
}

/// XSL `FIsEqArray`：无框线 / 列线 / 行线、无带标签行，且每个 `mtr` 子恰有一个 `mtd`
fn is_eq_array(n: Nd<'_>) -> bool {
    if !(none_like(n, "frame") && none_like(n, "columnlines") && none_like(n, "rowlines")) {
        return false;
    }
    let rows = kids(n);
    !rows.iter().any(|r| local(*r) == "mlabeledtr")
        && rows.iter().all(|r| {
            local(*r) != "mtr" || kids(*r).iter().filter(|c| local(**c) == "mtd").count() == 1
        })
}

/// XSL `CountMaxElmtsInRow`：按行取元素数（`mlabeledtr` 减 1，非行元素按 1）的最大值
fn max_elements_in_row(rows: &[Nd<'_>]) -> usize {
    let mut max = 0usize;
    for &row in rows {
        let count = match local(row) {
            "mlabeledtr" => kids(row).len().saturating_sub(1),
            "mtr" => kids(row).len(),
            _ => 1,
        };
        max = max.max(count);
    }
    max
}

/// XSL `OutputMs`：`ms` 文本按 lquote / rquote 包引号，两串都缺失或为空时缺省 U+0022
fn output_ms(n: Nd<'_>) -> String {
    let quote = |name: &str| {
        let v = attr_or(n, name);
        if v.is_empty() {
            "\"".to_string()
        } else {
            v.to_string()
        }
    };
    format!("{}{}{}", quote("lquote"), token_text(n), quote("rquote"))
}

// ---------------------------------------------------------------------------
// 转换器
// ---------------------------------------------------------------------------

/// XSL `fShouldCollect` 允许的父节点
const COLLECTABLE_PARENTS: &[&str] = &[
    "mrow", "mstyle", "msqrt", "menclose", "math", "mphantom", "mtd", "maction",
];

struct Omml {
    out: Writer<Cursor<Vec<u8>>>,
}

impl Omml {
    fn new() -> Self {
        Omml {
            out: Writer::new(Cursor::new(Vec::new())),
        }
    }

    /// 内存游标写入不会失败，出错只可能是编程错误
    fn write(&mut self, ev: Event<'_>) {
        self.out
            .write_event(ev)
            .expect("OMML 写入内存缓冲失败（不该发生）");
    }

    fn open(&mut self, name: &str) {
        self.write(Event::Start(BytesStart::new(name)));
    }

    fn close(&mut self, name: &str) {
        self.write(Event::End(BytesEnd::new(name)));
    }

    /// `<m:xxx/>`
    fn empty(&mut self, name: &str) {
        self.write(Event::Empty(BytesStart::new(name)));
    }

    /// `<m:xxx m:val="…"/>`
    fn empty_val(&mut self, name: &str, val: &str) {
        let mut start = BytesStart::new(name);
        start.push_attribute(("m:val", escape::escape(val).as_ref()));
        self.write(Event::Empty(start));
    }

    /// XSL 的 `match="/"`
    fn open_root(&mut self) {
        let mut start = BytesStart::new("m:oMath");
        start.push_attribute(("xmlns:m", OMML_NS));
        start.push_attribute(("xmlns:mml", MATHML_NS));
        self.write(Event::Start(start));
    }

    fn close_root(&mut self) {
        self.close("m:oMath");
    }

    fn finish(self) -> String {
        let cursor = self.out.into_inner();
        String::from_utf8(cursor.into_inner()).expect("OMML 片段必须是 UTF-8")
    }

    /// XSL `OutputText`：剥掉不可见乘号与零宽空格、`⩵` 折成 `==`、nbsp 换普通空格
    fn output_text(&mut self, raw: &str) {
        let stripped: String = raw
            .chars()
            .filter(|c| !matches!(c, '\u{2062}' | '\u{200B}'))
            .collect();
        let text = stripped.replace('\u{2A75}', "==").replace('\u{00A0}', " ");
        self.write(Event::Text(BytesText::from_escaped(
            escape::partial_escape(&text).as_ref(),
        )));
    }

    /// XSL `CreateArgProp`：祖先或自身里有 `mstyle/@scriptlevel ∈ {0,1,2}` 才写 `m:argPr`。
    /// 取值用 `ancestor-or-self::mml:mstyle[@scriptlevel][1]` —— 该轴是逆文档序，`[1]` 即**最近**的
    /// 带 `@scriptlevel` 的 mstyle，所以自内向外第一个命中就是答案。
    fn arg_prop(&mut self, n: Nd<'_>) {
        let levels: Vec<String> = n
            .ancestors()
            .filter(|a| a.is_element() && local(*a) == "mstyle")
            .filter_map(|a| attr(a, "scriptlevel"))
            .map(|v| xsl_lower(v).into_owned())
            .collect();
        if !levels.iter().any(|v| matches!(v.as_str(), "0" | "1" | "2")) {
            return;
        }
        self.open("m:argPr");
        self.empty_val("m:scrLvl", levels.first().map(String::as_str).unwrap_or(""));
        self.close("m:argPr");
    }

    /// XSL catch-all `match="*"`：只递归元素子节点（裸文本丢弃）
    fn children(&mut self, n: Nd<'_>) {
        for child in kids(n) {
            self.element(child);
        }
    }

    // -----------------------------------------------------------------------
    // 分发
    // -----------------------------------------------------------------------

    fn element(&mut self, n: Nd<'_>) {
        match local(n) {
            "mi" | "mn" | "mo" | "ms" | "mtext" => self.token(n),
            "mrow" | "mstyle" => self.mrow(n),
            "mfrac" => self.mfrac(n),
            "menclose" | "msqrt" => self.enclose_or_sqrt(n),
            "mroot" => self.mroot(n),
            "msub" => self.msub(n),
            "msup" => self.msup(n),
            "msubsup" => self.msubsup(n),
            "munder" => self.munder(n),
            "mover" => self.mover(n),
            "munderover" => self.munderover(n),
            "mfenced" => self.mfenced(n),
            "mmultiscripts" => self.mmultiscripts(n),
            "mtable" => self.mtable(n),
            "mpadded" => self.mpadded(n),
            "mphantom" => self.mphantom(n),
            // XSL 无模板且无元素子节点：catch-all 递归后自然消失。mspace 因此整节点丢失，
            // 与官方输出一致（见 space.omml）。
            "mspace" | "none" | "mprescripts" => {}
            other => {
                tracing::warn!(
                    "OMML 转换：未认出的 MathML 元素 <{other}>，按 catch-all 递归子节点"
                );
                self.children(n);
            }
        }
    }

    // -----------------------------------------------------------------------
    // token → m:r（XSL: token 模板 + FStartOfRun + CreateRunWithSameProp）
    // -----------------------------------------------------------------------

    fn token(&mut self, n: Nd<'_>) {
        let Some(parent) = n.parent().filter(|p| p.is_element()) else {
            return;
        };
        let siblings = kids(parent);
        let Some(index) = siblings.iter().position(|s| *s == n) else {
            return;
        };
        // XSL `fShouldCollect`：父节点是分组容器，且父节点本身不是线性分数 / func 结构
        let collect = COLLECTABLE_PARENTS.contains(&local(parent))
            && !is_linear_frac(parent)
            && !is_func(parent);
        if !collect {
            // 不参与合流：单独一个 run。注意此路径 XSL 不给 `ms` 加引号
            self.write_run(n, &[n], false);
            return;
        }
        // XSL `FStartOfRun`：只有 run 的第一个 token 负责写出整段
        if index > 0 && is_non_glyph_token(siblings[index - 1]) {
            return;
        }

        // XSL `CreateRunWithSameProp`：贪心吃掉后续同属性 token；被不同属性的 token 截断时，
        // 从该 token 起重写一个 run（对应 XSL 的尾递归）
        let mut start = index;
        loop {
            let first = siblings[start];
            let mut end = start + 1;
            while end < siblings.len() && self.compatible(first, siblings[end]) {
                end += 1;
            }
            self.write_run(first, &siblings[start..end], true);
            if end >= siblings.len() || !is_token_elem(siblings.get(end).copied()) {
                return;
            }
            start = end;
        }
    }

    /// XSL `nndBeforeLim` 谓词的反面：两个 token 能否合进同一个 `m:t`
    fn compatible(&self, first: Nd<'_>, n: Nd<'_>) -> bool {
        if !is_token(n) {
            return false;
        }
        // mtext 折成 omml 的 `m:nor`，只能与 mtext 相合
        if (local(first) == "mtext") != (local(n) == "mtext") {
            return false;
        }
        let font = font_cur(first);
        let mv = attr_or(n, "mathvariant");
        let fs = attr_or(n, "fontstyle");
        let fw = attr_or(n, "fontweight");
        let no_variant = mv.is_empty();
        let name = local(n);
        let mi_long = name == "mi" && text_len(n) > 1;
        let mi_short = name == "mi" && text_len(n) <= 1;
        let numeric = name == "mn" && is_xsl_number(&string_value(n));
        let non_numeric = name == "mn" && !is_xsl_number(&string_value(n));
        let no_font_attrs = no_variant && fs.is_empty() && fw.is_empty();

        let font_match = match font.as_str() {
            v if !v.is_empty() && v == mv => true,
            "normal" => {
                mv == "normal"
                    || (no_variant && ((fs == "normal" && fw != "bold") || mi_long || non_numeric))
                    // XSL 末尾追加的 normal 分支：不带任何字体属性的多字符 mi、以及 ms / mtext
                    || (no_font_attrs && (mi_long || matches!(name, "ms" | "mtext")))
            }
            "italic" => {
                mv == "italic"
                    || (no_variant
                        && ((fs == "italic" && fw != "bold")
                            || numeric
                            || name == "mo"
                            || mi_short))
            }
            "bold" => mv == "bold" || (no_variant && fw == "bold" && (fs == "normal" || mi_short)),
            "bi" | "bold-italic" => {
                mv == "bold-italic"
                    || (no_variant
                        && fw == "bold"
                        && (fs == "italic" || matches!(name, "mn" | "mo") || mi_short))
            }
            // $sFontCur=''（GetFontCur 不会返回空串）与 monospace 等分支：不可达
            _ => false,
        };
        if !font_match {
            return false;
        }
        attr_or(first, "font-family") == attr_or(n, "font-family")
    }

    /// XSL `CreateRunProp` → `CreateMathRPR` + `m:t`
    fn write_run(&mut self, first: Nd<'_>, group: &[Nd<'_>], quote_ms: bool) {
        self.open("m:r");
        let font = font_cur(first);
        let nor = is_nor(first);
        if nor || (font != "italic" && !font.is_empty()) {
            self.open("m:rPr");
            if nor {
                self.empty("m:nor");
            }
            self.scr_sty_prop(&font, nor);
            self.close("m:rPr");
        }
        let raw: String = group
            .iter()
            .map(|t| {
                if quote_ms && local(*t) == "ms" {
                    output_ms(*t)
                } else {
                    token_text(*t)
                }
            })
            .collect();
        self.open("m:t");
        self.output_text(&raw);
        self.close("m:t");
        self.close("m:r");
    }

    /// XSL `CreateMathScrStyProp`：`m:scr` 在前、`m:sty` 在后
    fn scr_sty_prop(&mut self, font: &str, nor: bool) {
        let (scr, sty) = match font {
            "normal" if !nor => (None, Some("p")),
            "bold" => (None, Some("b")),
            "script" => (Some("script"), None),
            "bold-script" => (Some("script"), Some("b")),
            "double-struck" => (Some("double-struck"), Some("p")),
            "fraktur" => (Some("fraktur"), Some("p")),
            "bold-fraktur" => (Some("fraktur"), Some("b")),
            "sans-serif" => (Some("sans-serif"), Some("p")),
            "bold-sans-serif" => (Some("sans-serif"), Some("b")),
            "sans-serif-italic" => (Some("sans-serif"), None),
            "sans-serif-bold-italic" => (Some("sans-serif"), Some("bi")),
            "bi" | "bold-italic" => (None, Some("bi")),
            // italic / monospace / normal+nor：XSL 不写任何子元素
            _ => (None, None),
        };
        if let Some(v) = scr {
            self.empty_val("m:scr", v);
        }
        if let Some(v) = sty {
            self.empty_val("m:sty", v);
        }
    }

    // -----------------------------------------------------------------------
    // 结构
    // -----------------------------------------------------------------------

    /// XSL `match mml:mrow | mml:mstyle`
    fn mrow(&mut self, n: Nd<'_>) {
        if is_nary_argument(n) {
            return;
        }
        self.row_body(n, n);
    }

    /// XSL `mrow` 模板体 + `NaryHandleMrowMstyle` 的非 mstyle 分支。
    /// `owner` 是 `CreateArgProp` 的上下文节点（线性分数走 XSL 的 `MakeLinearFraction`）。
    fn row_body(&mut self, n: Nd<'_>, owner: Nd<'_>) {
        let children = kids(n);
        if is_linear_frac(n) {
            self.linear_fraction(owner, &children);
            return;
        }
        if is_func(n) {
            // XSL `WriteFunc`
            self.open("m:func");
            self.open("m:fName");
            self.element(children[0]);
            self.close("m:fName");
            self.open("m:e");
            self.element(children[2]);
            self.close("m:e");
            self.close("m:func");
            return;
        }
        for child in children {
            self.element(child);
        }
    }

    /// XSL `MakeLinearFraction`
    fn linear_fraction(&mut self, owner: Nd<'_>, children: &[Nd<'_>]) {
        self.open("m:f");
        self.open("m:fPr");
        self.empty_val("m:type", "lin");
        self.close("m:fPr");
        self.arg_child(owner, children.first().copied(), "m:num");
        self.arg_child(owner, children.get(2).copied(), "m:den");
        self.close("m:f");
    }

    /// XSL `match mml:mfrac`
    fn mfrac(&mut self, n: Nd<'_>) {
        let children = kids(n);
        let bar = has_bar(attr_or(n, "linethickness"));
        let ty = if !bar {
            "noBar"
        } else if flag_true(n, "bevelled") {
            "skw"
        } else {
            "bar"
        };
        self.open("m:f");
        self.open("m:fPr");
        self.empty_val("m:type", ty);
        self.close("m:fPr");
        self.arg_child(n, children.first().copied(), "m:num");
        self.arg_child(n, children.get(1).copied(), "m:den");
        self.close("m:f");
    }

    /// `m:num` / `m:den` / `m:e` / `m:sub` / `m:sup` / `m:lim` / `m:deg` 的公共形状：
    /// 先 `CreateArgProp`（上下文为外层构造节点），再转换一个子元素
    fn arg_child(&mut self, owner: Nd<'_>, child: Option<Nd<'_>>, name: &str) {
        self.open(name);
        self.arg_prop(owner);
        if let Some(child) = child {
            self.element(child);
        }
        self.close(name);
    }

    /// XSL `match mml:menclose | mml:msqrt`
    fn enclose_or_sqrt(&mut self, n: Nd<'_>) {
        let notation = xsl_lower(attr_or(n, "notation"));
        let notation = notation.as_ref();
        if notation == "radical" || notation.is_empty() || local(n) == "msqrt" {
            self.open("m:rad");
            self.open("m:radPr");
            self.empty_val("m:degHide", "on");
            self.close("m:radPr");
            // msqrt 的次数恒空：XSL 这里只调 CreateArgProp，不 apply-templates
            self.arg_child(n, None, "m:deg");
            self.open("m:e");
            self.arg_prop(n);
            self.children(n);
            self.close("m:e");
            self.close("m:rad");
            return;
        }
        if matches!(notation, "actuarial" | "longdiv") {
            return;
        }

        let has = |needle: &str| notation.contains(needle);
        let box_like = has("box") || has("circle");
        let (top, bot, left, right) = (has("top"), has("bottom"), has("left"), has("right"));
        let (strike_h, strike_v) = (has("horizontalstrike"), has("verticalstrike"));
        let (strike_bltr, strike_tlbr) = (has("updiagonalstrike"), has("downdiagonalstrike"));
        let need_pr = strike_h
            || strike_v
            || strike_bltr
            || strike_tlbr
            || !(box_like || (top && bot && left && right));

        self.open("m:borderBox");
        if need_pr {
            self.open("m:borderBoxPr");
            if !box_like {
                for (hide, on) in [
                    ("m:hideTop", !top),
                    ("m:hideBot", !bot),
                    ("m:hideLeft", !left),
                    ("m:hideRight", !right),
                ] {
                    if on {
                        self.empty_val(hide, "on");
                    }
                }
            }
            for (strike, on) in [
                ("m:strikeH", strike_h),
                ("m:strikeV", strike_v),
                ("m:strikeBLTR", strike_bltr),
                ("m:strikeTLBR", strike_tlbr),
            ] {
                if on {
                    self.empty_val(strike, "on");
                }
            }
            self.close("m:borderBoxPr");
        }
        self.open("m:e");
        self.arg_prop(n);
        self.children(n);
        self.close("m:e");
        self.close("m:borderBox");
    }

    /// XSL `match mml:mroot`：`m:deg` 取第二子、`m:e` 取第一子
    fn mroot(&mut self, n: Nd<'_>) {
        let children = kids(n);
        self.open("m:rad");
        self.open("m:radPr");
        self.empty_val("m:degHide", "off");
        self.close("m:radPr");
        self.arg_child(n, children.get(1).copied(), "m:deg");
        self.arg_child(n, children.first().copied(), "m:e");
        self.close("m:rad");
    }

    /// XSL `match mml:msub`
    fn msub(&mut self, n: Nd<'_>) {
        let children = kids(n);
        if children.first().is_some_and(|b| is_nary(*b)) {
            return self.nary(n, "msub", &children);
        }
        self.open("m:sSub");
        self.arg_child(n, children.first().copied(), "m:e");
        self.arg_child(n, children.get(1).copied(), "m:sub");
        self.close("m:sSub");
    }

    /// XSL `match mml:msup`
    fn msup(&mut self, n: Nd<'_>) {
        let children = kids(n);
        if children.first().is_some_and(|b| is_nary(*b)) {
            return self.nary(n, "msup", &children);
        }
        self.open("m:sSup");
        self.arg_child(n, children.first().copied(), "m:e");
        self.arg_child(n, children.get(1).copied(), "m:sup");
        self.close("m:sSup");
    }

    /// XSL `match mml:msubsup`
    fn msubsup(&mut self, n: Nd<'_>) {
        let children = kids(n);
        if children.first().is_some_and(|b| is_nary(*b)) {
            return self.nary(n, "msubsup", &children);
        }
        self.open("m:sSubSup");
        self.arg_child(n, children.first().copied(), "m:e");
        self.arg_child(n, children.get(1).copied(), "m:sub");
        self.arg_child(n, children.get(2).copied(), "m:sup");
        self.close("m:sSubSup");
    }

    /// XSL `match mml:munder`：nary → bar → groupChr → limLow（注意 munder 没有 acc 分支）
    fn munder(&mut self, n: Nd<'_>) {
        let children = kids(n);
        if children.first().is_some_and(|b| is_nary(*b)) {
            return self.nary(n, "munder", &children);
        }
        if is_bar(n) {
            return self.bar(n, "bot");
        }
        if is_group_chr(n) {
            return self.group_chr(n, true);
        }
        self.open("m:limLow");
        self.arg_child(n, children.first().copied(), "m:e");
        self.arg_child(n, children.get(1).copied(), "m:lim");
        self.close("m:limLow");
    }

    /// XSL `match mml:mover`：nary → bar → acc → groupChr → limUpp
    fn mover(&mut self, n: Nd<'_>) {
        let children = kids(n);
        if children.first().is_some_and(|b| is_nary(*b)) {
            return self.nary(n, "mover", &children);
        }
        if is_bar(n) {
            return self.bar(n, "top");
        }
        if is_acc(n) {
            let chr = children
                .get(1)
                .map(|s| to_upper_combining(string_value(*s).as_str()).to_string())
                .unwrap_or_default();
            self.open("m:acc");
            self.open("m:accPr");
            self.empty_val("m:chr", &chr);
            self.close("m:accPr");
            self.arg_child(n, children.first().copied(), "m:e");
            return self.close("m:acc");
        }
        if is_group_chr(n) {
            return self.group_chr(n, false);
        }
        self.open("m:limUpp");
        self.arg_child(n, children.first().copied(), "m:e");
        self.arg_child(n, children.get(1).copied(), "m:lim");
        self.close("m:limUpp");
    }

    /// XSL `match mml:munderover`（非 n-ary 时拆成 limUpp 套 limLow）
    fn munderover(&mut self, n: Nd<'_>) {
        let children = kids(n);
        if children.first().is_some_and(|b| is_nary(*b)) {
            return self.nary(n, "munderover", &children);
        }
        self.open("m:limUpp");
        self.open("m:e");
        self.arg_prop(n);
        self.open("m:limLow");
        self.arg_child(n, children.first().copied(), "m:e");
        self.arg_child(n, children.get(1).copied(), "m:lim");
        self.close("m:limLow");
        self.close("m:e");
        self.arg_child(n, children.get(2).copied(), "m:lim");
        self.close("m:limUpp");
    }

    /// XSL `munder` / `mover` 的 bar 分支
    fn bar(&mut self, n: Nd<'_>, pos: &str) {
        let children = kids(n);
        self.open("m:bar");
        self.open("m:barPr");
        self.empty_val("m:pos", pos);
        self.close("m:barPr");
        self.arg_child(n, children.first().copied(), "m:e");
        self.close("m:bar");
    }

    /// XSL `munder` / `mover` 的 groupChr 分支。`under` 由调用方按元素名 literal 传入，
    /// 决定 `m:pos` / `m:vertJc` 的取向；`chr` 取第一个 `mo` 子的 string-value（XSL 不做 normalize）。
    fn group_chr(&mut self, n: Nd<'_>, under: bool) {
        let children = kids(n);
        let chr = children
            .iter()
            .copied()
            .find(|c| local(*c) == "mo")
            .map(string_value)
            .unwrap_or_default();
        let first_is_row = children.first().is_some_and(|c| local(*c) == "mrow");
        let (pos, vert_jc) = if under {
            (if first_is_row { "bot" } else { "top" }, "top")
        } else {
            (if first_is_row { "top" } else { "bot" }, "bot")
        };
        self.open("m:groupChr");
        self.open("m:groupChrPr");
        self.empty_val("m:chr", &chr);
        self.empty_val("m:pos", pos);
        self.empty_val("m:vertJc", vert_jc);
        self.close("m:groupChrPr");
        // XSL：`m:e` 里没有 CreateArgProp，且只 apply-templates 到 `mml:mrow` 子节点
        self.open("m:e");
        for child in children.iter().copied().filter(|c| local(*c) == "mrow") {
            self.element(child);
        }
        self.close("m:e");
        self.close("m:groupChr");
    }

    /// XSL `match mml:mfenced` + `CreateDelimProp`
    fn mfenced(&mut self, n: Nd<'_>) {
        let open_valid = n.has_attribute("open");
        let close_valid = n.has_attribute("close");
        let sep_valid = n.has_attribute("separators");
        let open = attr_or(n, "open");
        let close = attr_or(n, "close");
        let separators = attr_or(n, "separators");
        // MathML 可以有多个 separator，OMML 的 `m:d` 只认一个 —— 取首字符
        let sep_chr = separators.chars().next().map(|c| c.to_string());
        let sep_is_default = sep_chr.as_deref() == Some("|");

        self.open("m:d");
        if (open_valid && open != "(") || (close_valid && close != ")") || !sep_is_default {
            self.open("m:dPr");
            if open_valid && open != "(" {
                self.empty_val("m:begChr", open);
            }
            if !sep_is_default {
                if !sep_valid {
                    // 未给 separators：MathML 的默认是 `,`，OMML 的是 `|`，必须写出来
                    self.empty_val("m:sepChr", ",");
                } else {
                    self.empty_val("m:sepChr", sep_chr.as_deref().unwrap_or(""));
                }
            }
            if close_valid && close != ")" {
                self.empty_val("m:endChr", close);
            }
            self.close("m:dPr");
        }
        for child in kids(n) {
            self.open("m:e");
            self.arg_prop(child);
            self.element(child);
            self.close("m:e");
        }
        self.close("m:d");
    }

    /// XSL `match mml:mmultiscripts`。
    /// 计数口径照抄 XSL：`position()` 从 1 起算并把 base 也算进去，故 super 计数要再减 1；
    /// `none` 是占位符，计入 position 但不计入 Strict 计数。
    fn mmultiscripts(&mut self, n: Nd<'_>) {
        let children = kids(n);
        let base = children.first().copied();
        let pre_index = children.iter().position(|c| local(*c) == "mprescripts");
        let scripts_end = pre_index.unwrap_or(children.len());
        let scripts = &children[1.min(children.len())..scripts_end];
        let prescripts: &[Nd<'_>] = match pre_index {
            Some(i) => &children[i + 1..],
            None => &[],
        };
        let count_slot = |even: bool| -> isize {
            (0..scripts_end)
                .filter(|&i| ((i + 1) % 2 == 0) == even && local(children[i]) != "none")
                .count() as isize
        };
        let cnd_super = count_slot(false) - 1;
        let cnd_sub = count_slot(true);
        let has_prescript = prescripts.iter().any(|c| local(*c) != "none");
        let has_script = cnd_super + cnd_sub > 0;

        match (has_prescript, has_script) {
            (false, false) => {
                if let Some(base) = base {
                    self.element(base);
                }
            }
            (false, true) => {
                if cnd_super > 0 && cnd_sub > 0 {
                    self.open("m:sSubSup");
                    self.arg_child(n, base, "m:e");
                    self.split_scripts(&children[1..], n);
                    self.close("m:sSubSup");
                } else if cnd_sub > 0 {
                    self.open("m:sSub");
                    self.arg_child(n, base, "m:e");
                    self.plain_scripts("m:sub", &children[1..]);
                    self.close("m:sSub");
                } else {
                    self.open("m:sSup");
                    self.arg_child(n, base, "m:e");
                    self.plain_scripts("m:sup", &children[1..]);
                    self.close("m:sSup");
                }
            }
            (true, false) => {
                self.open("m:sPre");
                self.arg_child(n, base, "m:e");
                self.split_scripts(prescripts, n);
                self.close("m:sPre");
            }
            (true, true) => {
                self.open("m:sPre");
                self.open("m:e");
                self.arg_prop(n);
                if cnd_super > 0 && cnd_sub > 0 {
                    self.open("m:sSubSup");
                    self.arg_child(n, base, "m:e");
                    self.split_scripts(scripts, n);
                    self.close("m:sSubSup");
                } else if cnd_sub > 0 {
                    self.open("m:sSub");
                    self.arg_child(n, base, "m:e");
                    self.plain_scripts("m:sub", scripts);
                    self.close("m:sSub");
                } else {
                    self.open("m:sSup");
                    self.arg_child(n, base, "m:e");
                    self.plain_scripts("m:sup", scripts);
                    self.close("m:sSup");
                }
                self.close("m:e");
                self.split_scripts(prescripts, n);
                self.close("m:sPre");
            }
        }
    }

    /// XSL `SplitScripts`：`m:sub` 收第 1、3、5… 个，`m:sup` 收第 2、4、6… 个（XPath 的
    /// `position()` 从 1 起算，落到 0 基下标就是 sub 收偶数下标），两段都写 `CreateArgProp`
    fn split_scripts(&mut self, nodes: &[Nd<'_>], owner: Nd<'_>) {
        for (name, parity) in [("m:sub", 0usize), ("m:sup", 1usize)] {
            self.open(name);
            self.arg_prop(owner);
            for (i, node) in nodes.iter().enumerate() {
                if i % 2 == parity {
                    self.element(*node);
                }
            }
            self.close(name);
        }
    }

    /// XSL 里 `m:sub` / `m:sup` 直接 `apply-templates select="*[position() > 1]"` 的写法（不带 argPr）
    fn plain_scripts(&mut self, name: &str, nodes: &[Nd<'_>]) {
        self.open(name);
        for node in nodes {
            self.element(*node);
        }
        self.close(name);
    }

    /// XSL `match mml:mtable`
    fn mtable(&mut self, n: Nd<'_>) {
        if is_eq_array(n) {
            self.open("m:eqArr");
            for row in kids(n).into_iter().filter(|r| local(*r) == "mtr") {
                let cells: Vec<Node> = kids(row)
                    .into_iter()
                    .filter(|c| local(*c) == "mtd")
                    .collect();
                self.open("m:e");
                self.eq_array_row(&cells);
                self.close("m:e");
            }
            return self.close("m:eqArr");
        }

        let rows = kids(n);
        let max_in_row = max_elements_in_row(&rows);
        self.open("m:m");
        self.open("m:mPr");
        self.empty_val("m:baseJc", "center");
        self.empty_val("m:plcHide", "on");
        self.open("m:mcs");
        self.open("m:mc");
        self.open("m:mcPr");
        self.empty_val("m:count", &max_in_row.to_string());
        self.empty_val("m:mcJc", "center");
        self.close("m:mcPr");
        self.close("m:mc");
        self.close("m:mcs");
        self.close("m:mPr");
        for row in rows {
            self.open("m:mr");
            let cells = kids(row);
            match local(row) {
                "mtr" => {
                    for cell in &cells {
                        self.cell(*cell);
                    }
                    self.empty_cells(max_in_row as isize - cells.len() as isize);
                }
                "mlabeledtr" => {
                    for cell in cells.iter().skip(1) {
                        self.cell(*cell);
                    }
                    self.empty_cells(max_in_row as isize - (cells.len() as isize - 1));
                }
                _ => {
                    self.cell(row);
                    self.empty_cells(max_in_row as isize - 1);
                }
            }
            self.close("m:mr");
        }
        self.close("m:m");
    }

    /// `m:m` 的一格：`m:e` 包住该单元内容
    fn cell(&mut self, node: Nd<'_>) {
        self.open("m:e");
        self.element(node);
        self.close("m:e");
    }

    /// XSL `CreateEmptyElmt`：补齐行内缺列用的空 `m:e`
    fn empty_cells(&mut self, count: isize) {
        for _ in 0..count.max(0) {
            self.empty("m:e");
        }
    }

    /// XSL `ProcessEqArrayRow`：展平 `mtd` / `mrow` / `mstyle` 的层级，行间补 `&`。
    /// `maligngroup` / `malignmark` 不可达，故其分支省略。
    fn eq_array_row(&mut self, nodes: &[Nd<'_>]) {
        for node in nodes {
            for child in kids(*node) {
                if is_nary_argument(child) {
                    continue;
                }
                let plain_group = matches!(local(child), "mrow" | "mstyle")
                    && !(local(child) == "mrow" && (is_linear_frac(child) || is_func(child)));
                if plain_group {
                    self.eq_array_row(&[child]);
                } else {
                    self.element(child);
                }
            }
        }
    }

    /// XSL `match mml:mpadded`
    fn mpadded(&mut self, n: Nd<'_>) {
        let inside_phantom = n
            .parent()
            .is_some_and(|p| p.is_element() && local(p) == "mphantom");
        let no_siblings = n.prev_sibling_element().is_none() && n.next_sibling_element().is_none();
        if inside_phantom && no_siblings {
            // mphantom 已经把 phantom 属性算好了，这里只出内容
            return self.children(n);
        }
        self.open("m:phant");
        self.phant_properties_core(true, &padded_box(n));
        self.open("m:e");
        self.children(n);
        self.close("m:e");
        self.close("m:phant");
    }

    /// XSL `match mml:mphantom` + `CreatePhantProperties`
    fn mphantom(&mut self, n: Nd<'_>) {
        self.open("m:phant");
        let children = kids(n);
        let smashed = children.len() == 1 && local(children[0]) == "mpadded";
        let box_props = if smashed {
            padded_box(children[0])
        } else {
            [true; 3]
        };
        self.phant_properties_core(false, &box_props);
        self.open("m:e");
        self.children(n);
        self.close("m:e");
        self.close("m:phant");
    }

    /// XSL `CreatePhantPropertiesCore`：全默认则不写 `m:phantPr`
    fn phant_properties_core(&mut self, show: bool, full: &[bool; 3]) {
        let &[width, height, depth] = full;
        if show && width && height && depth {
            return;
        }
        self.open("m:phantPr");
        if !show {
            self.empty_val("m:show", "off");
        }
        if !width {
            self.empty_val("m:zeroWid", "on");
        }
        if !height {
            self.empty_val("m:zeroAsc", "on");
        }
        if !depth {
            self.empty_val("m:zeroDesc", "on");
        }
        self.close("m:phantPr");
    }

    // -----------------------------------------------------------------------
    // n-ary
    // -----------------------------------------------------------------------

    /// XSL `isNary` 命中后的 `m:nary`：6 种 MathML 结构共用，差别只在哪个槽有子、limLoc 与 hide
    fn nary(&mut self, n: Nd<'_>, kind: &str, children: &[Nd<'_>]) {
        let chr = children.first().map(|b| token_text(*b)).unwrap_or_default();
        self.open("m:nary");
        self.open("m:naryPr");
        self.empty_val("m:chr", &chr);
        let und_ovr = matches!(kind, "munder" | "mover" | "munderover");
        self.empty_val("m:limLoc", if und_ovr { "undOvr" } else { "subSup" });
        // `m:grow`：stretchy 优先，其次按字符是否会长大（XSL 列的那 13 个字符）
        let stretchy = children.first().map_or(String::new(), |b| {
            xsl_lower(attr_or(*b, "stretchy")).into_owned()
        });
        let grow = match stretchy.as_str() {
            "true" => "1",
            "false" => "0",
            _ if NARY_GROW_CHARS.contains(&chr.as_str()) => "1",
            _ => "0",
        };
        self.empty_val("m:grow", grow);
        self.empty_val(
            "m:subHide",
            if matches!(kind, "mover" | "msup") {
                "on"
            } else {
                "off"
            },
        );
        self.empty_val(
            "m:supHide",
            if matches!(kind, "munder" | "msub") {
                "on"
            } else {
                "off"
            },
        );
        self.close("m:naryPr");

        let sub = match kind {
            "munder" | "msub" | "munderover" | "msubsup" => children.get(1).copied(),
            _ => None,
        };
        let sup = match kind {
            "mover" | "msup" => children.get(1).copied(),
            "munderover" | "msubsup" => children.get(2).copied(),
            _ => None,
        };
        self.arg_child(n, sub, "m:sub");
        self.arg_child(n, sup, "m:sup");
        self.open("m:e");
        self.arg_prop(n);
        // XSL `NaryHandleMrowMstyle`：紧跟的 mrow / mstyle 被吸收进这个 `m:e`
        //（这两个元素自身的模板又会因 `FIsNaryArgument` 短路，故不会重复输出）
        if let Some(next) = n.next_sibling_element() {
            match local(next) {
                "mrow" => self.row_body(next, n),
                "mstyle" => self.children(next),
                _ => {}
            }
        }
        self.close("m:e");
        self.close("m:nary");
    }
}

/// XSL `FFull` 用到的一族属性：`mpadded` 的 width / height / depth
fn padded_box(n: Nd<'_>) -> [bool; 3] {
    [
        is_full(attr_or(n, "width")),
        is_full(attr_or(n, "height")),
        is_full(attr_or(n, "depth")),
    ]
}

// ---------------------------------------------------------------------------
// 黄金快照测试（T2.4）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod golden {
    use super::*;
    use std::path::Path;

    /// 规范化：命名空间前缀按 URI 统一 → 属性按名排序 → 空白归一，然后重序列化
    fn canon(xml: &str) -> String {
        let doc =
            roxmltree::Document::parse(xml).unwrap_or_else(|e| panic!("快照不是良构 XML: {e}"));
        let mut out = String::new();
        elem(&doc.root_element(), &mut out);
        out
    }

    fn prefix(uri: Option<&str>) -> Option<&'static str> {
        match uri {
            Some(OMML_NS) => Some("m"),
            Some(MATHML_NS) => Some("mml"),
            Some(_) => Some("?ns"),
            None => None,
        }
    }

    fn qualified(uri: Option<&str>, name: &str) -> String {
        match prefix(uri) {
            Some(p) => format!("{p}:{name}"),
            None => name.to_string(),
        }
    }

    fn elem(n: &Nd<'_>, out: &mut String) {
        out.push('<');
        out.push_str(&qualified(n.tag_name().namespace(), local(*n)));
        let mut attrs: Vec<(String, String)> = n
            .attributes()
            .map(|a| (qualified(a.namespace(), a.name()), a.value().to_string()))
            .collect();
        attrs.sort();
        for (k, v) in attrs {
            out.push_str(&format!(" {k}=\"{v}\""));
        }
        // 直接文本子节点合并后归一空白（lxml 美化输出留下的缩进文本因此消失）
        let text: String = n
            .children()
            .filter(|c| c.is_text())
            .filter_map(|c| c.text())
            .collect();
        let text = normalize_space(&text);
        let children: Vec<Nd<'_>> = n.children().filter(|c| c.is_element()).collect();
        if text.is_empty() && children.is_empty() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        out.push_str(&text);
        for c in children {
            elem(&c, out);
        }
        out.push_str(&format!(
            "</{}>",
            qualified(n.tag_name().namespace(), local(*n))
        ));
    }

    fn cases() -> Vec<std::path::PathBuf> {
        let dir = Path::new("tests/snapshots/cases");
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("读 {dir:?} 失败: {e}（工作目录应是仓库根）"))
            .map(|e| e.expect("固件目录可读").path())
            .filter(|p| p.extension().is_some_and(|x| x == "mathml"))
            .collect();
        files.sort();
        files
    }

    #[test]
    fn matches_xsl_golden_snapshots() {
        let mut checked = 0usize;
        for case in cases() {
            let stem = case.file_stem().expect("用例有文件名").to_string_lossy();
            let mathml = std::fs::read_to_string(&case).expect("用例可读");
            let golden = Path::new("tests/snapshots").join(format!("{stem}.omml"));
            assert!(
                golden.exists(),
                "用例 {stem} 缺黄金快照：先跑 scripts/gen_omml_snapshots.py"
            );
            let expect = std::fs::read_to_string(&golden).expect("快照可读");
            let got = match to_omml(&mathml) {
                MathOutcome::Ok(omml) => omml,
                MathOutcome::Failed(reason) => panic!("{stem}: 转换降级（{reason}）"),
            };
            assert_eq!(
                canon(&got),
                canon(&expect),
                "{stem}: 与官方 XSL 输出不一致\n实际: {}\n期望: {}",
                canon(&got),
                canon(&expect)
            );
            checked += 1;
        }
        assert!(checked >= 50, "快照用例过少: {checked}");
    }

    #[test]
    fn every_snapshot_has_a_case() {
        for entry in std::fs::read_dir("tests/snapshots").expect("快照目录可读") {
            let path = entry.expect("可读").path();
            if path.extension().is_some_and(|e| e == "omml") {
                let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
                assert!(
                    Path::new("tests/snapshots/cases")
                        .join(format!("{stem}.mathml"))
                        .exists(),
                    "快照 {stem}.omml 没有对应用例"
                );
            }
        }
    }

    #[test]
    fn degrades_without_panicking_on_unusable_input() {
        assert!(matches!(to_omml("<math><mfrac>"), MathOutcome::Failed(_)));
        assert!(matches!(
            to_omml("<mrow><mi>x</mi></mrow>"),
            MathOutcome::Failed(_)
        ));
        // 认不出的构造（merror 在 XSL 里没有模板）：递归子节点、保住内容，不 panic 也不降级
        let omml = omml_of(r#"<merror><mtext>坏</mtext></merror>"#);
        assert!(omml.contains("<m:nor/>"), "mtext 应走 normal 样式: {omml}");
        assert_eq!(texts(&omml), vec!["坏"], "内容不该丢: {omml}");
    }

    #[test]
    fn collects_like_runs_and_splits_unlike_ones() {
        // mi + mn + mo 属性相容 → 合成一个 m:t；mtext 另起一段
        let omml = omml_of(r#"<mi>x</mi><mn>1</mn><mo>+</mo><mtext>a</mtext><mtext>b</mtext>"#);
        assert_eq!(
            texts(&omml),
            vec!["x1+", "ab"],
            "合流结果应按 token 属性分组: {omml}"
        );
    }

    #[test]
    fn output_text_strips_and_replaces_control_chars() {
        let omml = omml_of(r#"<mi>a</mi><mo>&#x2062;</mo><mo>&#x2A75;</mo><mtext>&#xa0;b</mtext>"#);
        assert_eq!(texts(&omml), vec!["a==", " b"], "OutputText 语义: {omml}");
    }

    #[test]
    fn nary_absorbs_following_row() {
        let omml = omml_of(
            r#"<msubsup><mo>&#x222B;</mo><mn>0</mn><mn>1</mn></msubsup><mrow><mi>f</mi><mo>&#x2061;</mo><mrow><mo>(</mo><mi>x</mi><mo>)</mo></mrow></mrow>"#,
        );
        assert!(omml.contains("<m:nary>"), "{omml}");
        assert!(
            omml.contains("<m:e><m:func>"),
            "n-ary 的 m:e 应吸收后续 mrow（含其 func 结构）: {omml}"
        );
        assert_eq!(texts(&omml), vec!["0", "1", "f", "(x)"], "{omml}");
    }

    /// 由 MathML 片段构造整棵 `math` 并转换
    fn omml_of(inner: &str) -> String {
        let mathml = format!(r#"<math xmlns="http://www.w3.org/1998/Math/MathML">{inner}</math>"#);
        match to_omml(&mathml) {
            MathOutcome::Ok(omml) => omml,
            MathOutcome::Failed(reason) => panic!("转换降级: {reason}"),
        }
    }

    /// 抽出所有 `m:t` 文本（按文档序），用于快速断言 run 的切分。
    /// 不再归一空白：写出的 `m:t` 内容本身已归一，再归一会把 nbsp→空格这类语义抹平。
    fn texts(omml: &str) -> Vec<String> {
        let doc = roxmltree::Document::parse(omml).expect("OMML 良构");
        doc.descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "t")
            .map(|n| string_value(n))
            .collect()
    }
}
