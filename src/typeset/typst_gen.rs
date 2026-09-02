//! typst 源码生成器（任务分解 T3.6，实施计划 §6.3）
//!
//! 输入是 [`LayoutDoc`]，输出一整份可以直接交给 [`crate::typeset::compiler`] 的 `main.typ`
//! 源码 —— 母版（`#set` 与函数库）与正文同处一份文件：typst 的 `#set` 生效范围就是「它之后
//! 的内容」，拆成两个文件只会多一次 `#include` 的虚路径管理。
//!
//! ## 文本一律走字符串，不走 markup（实测口径）
//!
//! 这是本模块最重要的一条约束，三条实测理由：
//!
//! 1. 行内的 `//` 在 markup 里**就是行注释**：`前//后` 编出来只剩「前后」，题干里的
//!    `https://…` 会被吃掉半行；`/* … */` 同理。
//! 2. 行首的 `- ` / `1. ` / `= ` 会变成**列表与标题** —— 解析正文里「- 讨论三种情况」极常见。
//! 3. `*粗*`、`--`→en dash、`...`→`…`、`"`→弯引号 都是 markup 层的自动改写。
//!
//! 而 `#("…")`（代码模式的字符串字面量落进 markup）实测**完全不解释**：`#("- 甲")`、
//! `#("#let x = 1")`、`#("[甲]{乙}")`、`#("a//b")`、`#("价格 $5 与 100%")` 都逐字上图，
//! 连续空格照留、长中文串照折行。于是所有外部文本只需要过一个转义器
//! [`crate::typeset::math::typst_str`]（字符串字面量转义），不再需要一套 markup 转义规则，
//! 也就消灭了「漏转义一个字符就把整卷排版改掉」这一整类问题。
//!
//! ## 三条不变式（与 [`crate::typeset::ir`] 对齐）
//!
//! 1. 公式在这里才转换（[`to_typst`]）：转换失败**只降级这一处**并记 [`Issue`]，绝不向上传错
//!    —— 一枚公式不许弄坏一张卷子，字段口径与 docx 侧一致。
//! 2. `BlockMeta::breakable` 忠实落到 `block(breakable:)`；`keep_with_next` 在 typst 0.15
//!    **没有对应原语**（实测：typst-library / typst-layout 源码里搜不到 `keep_with_next`，
//!    `par` 也没有 `keep-lines-together`），M4 的落法是 [`plan_groups`] 把一条 `keep` 链
//!    折成一枚 `block(breakable: false)` 壳。链能粘多长由估高决定：图 / 表格 / 块级公式 /
//!    Callout / 答案区这些**估不准高**的块一律不粘 —— 造出一块超过一页的整块，代价是版面
//!    溢出裁切，比原来的腰斩难看得多。docx 侧走的是 `w:keepNext`，两边同一份 IR 语义。
//! 3. 图片只认调用方给的路径表：`images` 由 `export::pdf` 抓好后按「原始 URL → 可用素材」
//!    传进来。`None` = 上游已经记过原因（抓取失败、超限、非图片），这里静默跳图；表里
//!    压根没有这个 URL 才由本模块补一条 Issue。两种情况都不许弄坏整卷。
//!
//! ## 单位
//!
//! 版面尺寸全部由 [`LayoutSpec`] 换算成 mm 绝对值写进源码。不用 `min(30mm, 100%)` 这类运行时
//! 约束：typst 0.15 里 `min` 不是全局函数，`calc.min` 又不许 ratio 与 length 比较（实测），
//! 宽度上限只能在 Rust 侧按 [`column_width_mm`](LayoutSpec::column_width_mm) 裁。

use std::collections::HashMap;
use std::ops::Range;

use crate::export::model::{
    CalloutKind, ExamOption, ImageAlign, InlineImage, InlineNode, Issue, IssueField, IssueSeverity,
    TableAlign,
};
use crate::typeset::blocks::choice_grid;
use crate::typeset::blocks::figure_float::Split;
use crate::typeset::ir::{AnswerBlock, BlockMeta, LayoutBlock, LayoutDoc, QuestionBlock, Section};
use crate::typeset::math::{MITEX_PREAMBLE, degraded, to_typst, typst_str};
use crate::typeset::spec::{BlankStyle, ColorMode, LayoutSpec};

/// 生成结果：可直接编译的 typst 源码 + 生成期发现的问题
#[derive(Debug)]
pub struct Generated {
    pub source: String,
    pub issues: Vec<Issue>,
}

/// 生成一份完整源码。
///
/// `images` 是「原始 URL → 素材」的表，由 `export::pdf` 预取后填好：
/// `Some(path)` = typst 可见路径（`/uploads/…` 或 `/ext/<n>.<ext>`），直接上图；
/// `None` = 上游已判定拿不到**并且记过 Issue**，这里静默跳图（再报一条只会撑爆警告头）；
/// 表里没有这个 URL = 调用方漏装素材表，由本模块补一条 Issue 后跳图。
/// 三种情况都不许弄坏整卷。
pub fn generate(doc: &LayoutDoc, images: &HashMap<String, Option<String>>) -> Generated {
    let spec = &doc.spec;
    let mut g = Gen {
        images,
        spec,
        issues: Vec::new(),
        number: None,
        field: IssueField::Structure,
        black: spec.color == ColorMode::PrintBlackOnly,
    };
    let body = g.body(doc);

    let mut source = String::with_capacity(MITEX_PREAMBLE.len() + body.len() + 2048);
    source.push_str(MITEX_PREAMBLE);
    source.push('\n');
    source.push_str(&g.prologue(doc));
    source.push_str(FUNCTION_LIBRARY);
    source.push_str(&body);
    source.push('\n');
    Generated {
        source,
        issues: g.issues,
    }
}

// ═══════════════════════════════════ 母版 ═══════════════════════════════════

/// 函数库 v1：整段是常量，不进 `format!` —— 花括号不必转义，版式漂了就是在改这个常量。
///
/// 只依赖母版里的四个变量（`accent` / `analysis-ink` / `body-font` / `heading-font`），
/// 它们由 [`Gen::prologue`] 按 `LayoutSpec` 生成。
const FUNCTION_LIBRARY: &str = r#"
// ─────────────────────────── 函数库 v1（T3.6）───────────────────────────
// M3 基础版：题块 / 大题标题 / 选项栅格 / 提示框 / 解析 / 留白 / 卷头。
// 分页粘连（keep_with_next）在 typst 0.15 没有原语，动态页眉与密封线在 M4。

/// 题块：首行是「5. （3 分）」这类标号，续行按 indent 缩进（hanging indent）。
/// label 为空则不占行首；indent 同时决定块缩进与首行回抽，两者不会各改一半。
#let item(label, indent: 2.6em, above: 3pt, breakable: true, body) = block(
  width: 100%,
  breakable: breakable,
  inset: (left: indent),
  above: above,
)[
  #set par(first-line-indent: (amount: 0em - indent, all: false))
  #if label != "" {
    text(weight: "bold")[#label];
    h(0.3em)
  }
  #body
]

/// 选项栅格：列数与 docx 同源（typeset::blocks::choice_grid），不各排各的。
///
/// **R7 兜底**：Rust 侧的估宽是先验，只有 typst 这边能拿到渲染后的真实宽度。所以多列时用
/// `layout` 取「这一栏实际分给栅格的宽度」（`inset` 已经扣掉了题号悬挂缩进，T4.3 的左文右图
/// 之后也会自动变小），用 `measure` 量每个单元格**不折行**的自然宽，装不下就降列：
/// 4 → 2 → 1。跳过 3 是故意的 —— 四枚选项排成 3+1 比排成 2+2 难看。
/// 三处误差都由它兜：估宽对没见过的 LaTeX 命令偏乐观、图片宽要解码后才知道、
/// 以及 docx 与 typst 的悬挂缩进口径差（2.0em vs 2.6em）。
/// 判定为单列时一个单元格都不量 —— 不是为省钱：20 题卷实测 113ms（开兜底）对 114ms（全单列），
/// 500ms/卷的预算很宽（探针见 `cost_of_the_measure_fallback`），单列本来就装不下溢出这回事。
#let choices(cols, ..cells) = {
  let gut = 10pt
  if cols <= 1 {
    grid(columns: 1, gutter: (gut, 2pt), ..cells)
  } else {
    layout(size => {
      let want = {
        let w = 0pt
        // `..cells` 收来的是 arguments 不是数组，循环得先 `.pos()`（实测：直接 loop 报
        // "cannot loop over arguments"）；下面的 `..cells` 展开传参仍然照旧可用
        for cell in cells.pos() {
          let natural = measure(cell).width
          if natural > w { w = natural }
        }
        w
      }
      let fits(c) = want <= (size.width - (c - 1) * gut) / c
      let n = if fits(cols) { cols } else if cols >= 4 and fits(2) { 2 } else { 1 }
      grid(columns: n, gutter: (gut, 2pt), ..cells)
    })
  }
}

/// 左文右图（T4.3）：题干尾部的配图并排进右栏，文字仍在左栏的悬挂缩进里。
///
/// 右栏恒为 `35%` —— 与 `typeset::blocks::figure_float::FIGURE_SHARE` 同值，漂了有测试会红。
/// typst 先按父容器宽折算**相对**轨道、再把余额分给 `1fr`，所以左栏文字长短挤不动图列，
/// 这就是「图不失宽」的机械成因。Rust 侧只放行「估宽装得进九成右栏」的尾部单图。
/// 选项（`rest`）排在栅格**外面**通栏：四列栅格塞进 65% 的左栏必然挤成一列。
/// 左格自己就是一个 `item` —— 悬挂缩进只在这一处出现，图格在缩进之外、享整栏宽。
#let figure-float(label, figure, indent: 2.6em, above: 3pt, breakable: true, rest: none, body) = block(
  width: 100%,
  breakable: breakable,
  above: above,
)[
  #grid(
    columns: (1fr, 35%),
    gutter: 6pt,
    item(label, indent: indent, above: 0pt)[#body],
    align(right + top)[#figure],
  )
  #if rest != none {
    block(width: 100%, inset: (left: indent))[#rest]
  }
]

/// 粘连壳（T4.5）：typst 0.15 **没有** keep-with-next 原语（实测：typst-library /
/// typst-layout 源码里搜不到 `keep_with_next`，`par` 也没有 `keep-lines-together`），
/// 能把 N 个块钉在同一页的只有「包进一个 breakable: false 的 block」。
/// 粘哪一段由 Rust 侧按估高决定：一条链再长也不许超过栏高，否则就是溢出裁切。
/// `above` 是给整组补回的那口气 —— 壳内首块的前置间距在容器边界上会被吞掉。
#let keep-together(body) = block(width: 100%, breakable: false, above: 3pt)[#body]

/// 大题标题：灰底 + 左题名右「共 N 题 · X 分」
#let section-header(title, meta) = block(
  width: 100%,
  fill: luma(236),
  stroke: (left: 2.4pt + accent),
  radius: 2pt,
  inset: (x: 6pt, y: 3pt),
  above: 8pt,
  below: 4pt,
  breakable: false,
)[
  #grid(columns: (1fr, auto), gutter: 6pt, align(left + horizon)[#text(font: heading-font, weight: "bold")[#title]], align(right + horizon)[#text(size: 9pt, fill: luma(70))[#meta]])
]

/// 提示框（教师模式四类）：色条与底色由 Rust 侧给 —— 印前纯黑时它们只能是黑白
#let callout-box(bar, bg, title, body) = block(
  width: 100%,
  fill: bg,
  stroke: (left: 2pt + bar),
  radius: 2pt,
  inset: (x: 6pt, y: 3pt),
  above: 3pt,
  breakable: false,
)[
  #text(size: 9.5pt, font: heading-font, weight: "bold", fill: bar)[#title]#linebreak()#body
]

/// 一段解析：小一号灰字，标题加粗后另起一行
#let analysis(title, body) = {
  set text(size: 9.5pt, fill: analysis-ink)
  if title != "" {
    text(weight: "bold")[#title];
    linebreak()
  }
  body
}

// 答题留白三式：横线 / 点阵 / 纯空白。高度与行数都由 Rust 算好，模板不做算术。
#let blank-lines(h, n) = block(width: 100%, height: h, clip: true, breakable: false)[
  #for _ in range(n) {
    line(length: 100%, stroke: 0.5pt + luma(120));
    parbreak()
  }
]
#let blank-dots(h, n) = block(width: 100%, height: h, clip: true, breakable: false)[
  #for _ in range(n) {
    line(length: 100%, stroke: (paint: luma(150), thickness: 0.5pt, dash: "dotted"));
    parbreak()
  }
]
#let blank-space(h) = block(width: 100%, height: h, breakable: false)

/// 卷头（简化版）：题名 + 副题 + 一行元信息 + 注意事项。
/// 完整考卷卷头（学校/班级/姓名栏与密封线）在 T4.9。
#let masthead(title, subtitle: none, meta: none, instructions: ()) = block(
  width: 100%,
  below: 8pt,
)[
  #set par(justify: false)
  #align(center)[#text(font: heading-font, size: 16pt, weight: "bold")[#title]]
  #if subtitle != none { align(center)[#text(size: 10.5pt, fill: luma(70))[#subtitle]] }
  #if meta != none { align(center)[#text(size: 9.5pt)[#meta]] }
  #if instructions.len() > 0 {
    block(
      width: 100%,
      stroke: (top: 0.4pt + luma(180), bottom: 0.4pt + luma(180)),
      inset: (x: 4pt, y: 3pt),
      above: 5pt,
    )[
      #text(size: 9.5pt, weight: "bold")[注意事项：]#linebreak()
      #for s in instructions {
        text(size: 9.5pt)[#s];
        linebreak()
      }
    ]
  }
]
"#;

/// 1px（编辑器口径 96dpi）→ mm。与 docx 侧 `PX_CM` 同一折算，两边不许漂
const PX_MM: f32 = 25.4 / 96.0;
/// 每深一层小问的额外缩进（em）
const SUB_INDENT_EM: f32 = 1.2;
/// 题号悬挂缩进宽度（em）
const HANG_EM: f32 = 2.6;
/// 留白区每行横线/点阵的间距（mm）
const BLANK_LINE_MM: f32 = 8.0;
/// 相邻两块之间的 `par.spacing: 0.55em`（10.5pt 下约 2mm）—— 只给 T4.5 的估高用
const PAR_SPACING_MM: f64 = 2.0;

// ═══════════════════════════════════ 生成器 ═══════════════════════════════════

struct Gen<'a> {
    images: &'a HashMap<String, Option<String>>,
    spec: &'a LayoutSpec,
    issues: Vec<Issue>,
    /// 当前题号：Issue 归属用，卷级内容为 None
    number: Option<u32>,
    /// 当前正在渲染的字段：公式降级时记进 Issue.field
    field: IssueField,
    /// 印前纯黑（K100）：彩色装饰一律退成黑白
    black: bool,
}

impl Gen<'_> {
    // ------------------------------------------------------------ 母版部分

    /// 变量与 `#set`：spec 的每个字段都在这里落地，函数库只认那四个变量名。
    fn prologue(&self, doc: &LayoutDoc) -> String {
        let spec = self.spec;
        let (w, h) = spec.paper.size_mm();
        let m = &spec.margins;
        let mut s = String::new();

        s.push_str(&format!(
            "#let body-font = ({}, \"Libertinus Serif\")\n",
            typst_str(&spec.fonts.body)
        ));
        s.push_str(&format!(
            "#let heading-font = ({}, \"Libertinus Serif\")\n",
            typst_str(&spec.fonts.heading)
        ));
        // 强调色与解析墨色：纯黑模式下不留任何彩色
        let accent = if self.black {
            "luma(0%)"
        } else {
            "rgb(\"#1F4E79\")"
        };
        let analysis = if self.black {
            "luma(0%)"
        } else {
            "rgb(\"#3D4A5C\")"
        };
        s.push_str(&format!("#let accent = {accent}\n"));
        s.push_str(&format!("#let analysis-ink = {analysis}\n"));

        // 页面：纸张 + 栏数 + 边距 + 页码。
        //
        // 栏数走**页级** `columns`（typst 在 `pages/run.rs` 里同时读 `PageElem::columns` 与
        // `ColumnsElem::gutter`，后者就是一枚 `#set columns`）。不用 `#columns(2)[…]` 包壳：
        // 那是一个布局容器，容器里 `#pagebreak` 直接报
        // "pagebreaks are not allowed inside of containers"（实测）—— 卷末答案就没法另起一页，
        // T4 的脚注与行号同样会在容器里失效。
        let mut page = format!(
            "#set page(width: {w}mm, height: {h}mm, columns: {}, margin: (top: {}mm, right: {}mm, bottom: {}mm, left: {}mm)",
            spec.columns.max(1),
            mm(m.top_mm),
            mm(m.right_mm),
            mm(m.bottom_mm),
            mm(m.left_mm)
        );
        if spec.header_footer.page_number {
            page.push_str(", numbering: \"1\", number-align: center");
            // 计数器只能在 context 里求值：裸 `counter(page).display()` 报
            // "can only be used when context is known"（实测）
            page.push_str(
                ", footer: context align(center)[#text(size: 9pt, fill: luma(110))[第 #counter(page).display(\"1\") 页 / 共 #counter(page).final().first() 页]]",
            );
        }
        if spec.header_footer.header_title {
            // 静态近似：整份文档一个页眉（取卷名）。逐栏取当前大题名是 T4.10。
            page.push_str(&format!(
                ", header: align(center)[#text(size: 9pt, fill: luma(120))[#({})]]",
                typst_str(&doc.title)
            ));
        }
        s.push_str(&page);
        s.push_str(")\n");
        // 栏距与栏数同处一页级口径：`ColumnsElem::gutter` 就是 typst 读的那枚 set 规则，
        // 单栏时 `column_gutter_mm()` 已经归零，写一条 0 的规则不改版面。
        s.push_str(&format!(
            "#set columns(gutter: {}mm)\n",
            mm(spec.column_gutter_mm())
        ));

        s.push_str(r#"#set text(font: body-font, size: 10.5pt, lang: "zh", region: "cn")"#);
        s.push('\n');
        if self.black {
            s.push_str("#set text(fill: luma(0%))\n");
        }
        // 数学字体在 typst 0.15 里**没有旋钮**：`math` 是模块不是元素（`#set math(font:)`
        // 报 "expected function, found module"），而 `EquationElem` 自己的 show_set 又把
        // `text.font` 硬设成 New Computer Modern Math —— 实测连
        // `#show math.equation: set text(font: …)` 都盖不动。所以 `spec.fonts.math` 在
        // PDF 侧无落点（docx 侧同理：OMML 由 Word 自己挑字体），这里不写注定无效的规则。
        s.push_str("#set par(justify: true, leading: 0.7em, spacing: 0.55em)\n");
        s.push('\n');
        s
    }

    // ------------------------------------------------------------ 正文骨架

    fn body(&mut self, doc: &LayoutDoc) -> String {
        let mut b = String::new();
        b.push_str(&self.masthead(doc));
        for section in &doc.sections {
            b.push_str(&self.section(section));
        }
        if doc.has_answer_key() {
            b.push_str("#pagebreak(weak: true)\n");
            b.push_str(&format!(
                "#section-header({}, {})\n",
                typst_str("参考答案与解析"),
                typst_str(&format!("共 {} 题", doc.question_count()))
            ));
            for answer in &doc.answer_key {
                self.number = Some(answer.number);
                self.field = IssueField::Answer;
                b.push_str(&self.answer(answer));
            }
        }
        b
    }

    fn masthead(&mut self, doc: &LayoutDoc) -> String {
        self.number = None;
        self.field = IssueField::Structure;
        let meta = self.meta_line(doc);
        let mut s = format!("#masthead({}", typst_str(&doc.title));
        if let Some(sub) = doc.subtitle.as_deref().filter(|t| !t.is_empty()) {
            s.push_str(&format!(", subtitle: {}", typst_str(sub)));
        }
        if !meta.is_empty() {
            s.push_str(&format!(", meta: {}", typst_str(&meta)));
        }
        let notes: Vec<String> = doc
            .meta
            .instructions
            .iter()
            .filter(|t| !t.trim().is_empty())
            .map(|t| typst_str(t))
            .collect();
        if !notes.is_empty() {
            // 单元素数组必须带尾逗号：typst 里 `("甲")` 只是括号，`("甲",)` 才是数组
            let tail = if notes.len() == 1 { "," } else { "" };
            s.push_str(&format!(", instructions: ({}{})", notes.join(", "), tail));
        }
        s.push_str(")\n");
        s
    }

    /// 卷头元信息那一行：`xx中学 · 120 分钟 · 满分 150 分`
    fn meta_line(&self, doc: &LayoutDoc) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(school) = doc.meta.school.as_deref().filter(|t| !t.is_empty()) {
            parts.push(school.to_string());
        }
        if let Some(min) = doc.meta.duration_min {
            parts.push(format!("{min} 分钟"));
        }
        if doc.meta.total_score > 0.0 {
            parts.push(format!("满分 {} 分", score(doc.meta.total_score)));
        }
        parts.join(" · ")
    }

    fn section(&mut self, section: &Section) -> String {
        let header = &section.header;
        self.number = None;
        self.field = IssueField::Structure;
        let meta = format!(
            "共 {} 题 · {} 分",
            header.question_count,
            score(header.total_score)
        );
        let mut s = format!(
            "#section-header({}, {})\n",
            typst_str(&header.title),
            typst_str(&meta)
        );
        if let Some(instruction) = header
            .instruction
            .as_deref()
            .filter(|t| !t.trim().is_empty())
        {
            s.push_str(&format!(
                "#item(\"\", indent: 0em, above: 1pt, breakable: false)[#({})]\n",
                typst_str(instruction)
            ));
        }
        for group in plan_groups(&section.blocks, self.spec) {
            let slice = &section.blocks[group];
            if slice.len() > 1 {
                s.push_str(&self.keep_together(slice));
            } else {
                s.push_str(&self.block(&slice[0]));
            }
        }
        s
    }

    fn block(&mut self, block: &LayoutBlock) -> String {
        match block {
            LayoutBlock::Question(q) => {
                self.number = Some(q.number);
                self.field = IssueField::Stem;
                let label = format!("{}. （{} 分）", q.number, score(q.score));
                let choices = self.choices(&q.options, q.grid.columns);
                match q.figure {
                    Some(split) => self.float_figure(&label, split, q, choices.as_deref()),
                    None => {
                        let mut content = self.nodes(&q.stem);
                        if let Some(choices) = &choices {
                            content.push_str("\n\n");
                            content.push_str(choices);
                            content.push('\n');
                        }
                        self.item(&label, HANG_EM, 3.0, q.meta, &content)
                    }
                }
            }
            LayoutBlock::SubQuestion(sub) => {
                self.number = Some(sub.number);
                self.field = IssueField::Stem;
                let label = if sub.label.is_empty() {
                    String::new()
                } else {
                    format!("{} ", sub.label)
                };
                let indent = HANG_EM + SUB_INDENT_EM * sub.depth as f32;
                let stem = self.nodes(&sub.stem);
                self.item(&label, indent, 1.0, sub.meta, &stem)
            }
            LayoutBlock::Callout(callout) => {
                self.number = Some(callout.number);
                self.field = IssueField::Analysis;
                let (bar, bg) = self.callout_colors(callout.callout.kind);
                format!(
                    "#callout-box({}, {}, {}, [{}])\n",
                    bar,
                    bg,
                    typst_str(&callout.callout.title),
                    self.nodes(&callout.callout.nodes)
                )
            }
            LayoutBlock::Blank(blank) => {
                self.number = Some(blank.number);
                self.field = IssueField::Structure;
                let h = format!("{}mm", mm(blank.height_mm.max(0.0)));
                let lines = ((blank.height_mm / BLANK_LINE_MM).floor() as i32).max(1);
                match blank.style {
                    BlankStyle::Lines => format!("#blank-lines({h}, {lines})\n"),
                    BlankStyle::Dots => format!("#blank-dots({h}, {lines})\n"),
                    BlankStyle::Blank => format!("#blank-space({h})\n"),
                }
            }
            LayoutBlock::Answer(answer) => {
                self.number = Some(answer.number);
                self.field = IssueField::Answer;
                self.answer(answer)
            }
        }
    }

    /// `#item(...)` 调用行：标号与缩进统一在这里拼，四种块共用一个入口
    fn item(
        &mut self,
        label: &str,
        indent: f32,
        above: f32,
        meta: BlockMeta,
        content: &str,
    ) -> String {
        format!(
            "#item({}, indent: {}em, above: {}pt, breakable: {})[{}]\n",
            typst_str(label),
            mm(indent),
            mm(above),
            meta.breakable,
            content
        )
    }

    /// 选项栅格调用（`None` = 这道题没有选项）：`#choices(列数, [A. …], [B. …])`
    fn choices(&mut self, options: &[ExamOption], columns: usize) -> Option<String> {
        if options.is_empty() {
            return None;
        }
        let cells: Vec<String> = options
            .iter()
            .map(|o| {
                let prefix = if o.label.is_empty() {
                    String::new()
                } else {
                    format!("{}. ", o.label)
                };
                format!(
                    "[#({}){}]",
                    typst_str(&prefix),
                    self.nodes_with(&o.content, IssueField::Choice)
                )
            })
            .collect();
        Some(format!("#choices({},{})", columns.max(1), cells.join(", ")))
    }

    /// 左文右图（T4.3）：题干按 `split` 切成左右两格，配图并排进右栏
    ///
    /// `label` 与 `figure` 只能**按位置**传，两处实测：写 `figure:` 直接报
    /// "the argument `figure` is positional"；行尾的 `[body]` 填的是**下一个未填的位置参数**，
    /// 所以模板里连 `item(label: …)` 都不能写成具名 —— 那样 body 会落到 label 上，报
    /// "missing argument: body"。
    /// 粘连（T4.5）走不到这一块：题干含图时 [`block_height_mm`] 恒为 `None`，浮动题永远不会
    /// 被裹进 `keep-together` 壳 —— 也就不会出现「壳里套一枚不许跨页的图」这种溢出裁切。
    fn float_figure(
        &mut self,
        label: &str,
        split: Split,
        q: &QuestionBlock,
        choices: Option<&str>,
    ) -> String {
        let (left, right) = split.parts(&q.stem);
        let body = self.nodes(left);
        let figure = self.nodes(right);
        let rest = match choices {
            Some(choices) => format!(", rest: [{}]", choices),
            None => String::new(),
        };
        format!(
            "#figure-float({}, [{}], indent: {}em, above: 3pt, breakable: {}{})[{}]\n",
            typst_str(label),
            figure.trim(),
            mm(HANG_EM),
            q.meta.breakable,
            rest,
            body.trim()
        )
    }

    /// 粘连壳：把已经生成好的 N 个块整体钉在同一页（T4.5）
    fn keep_together(&mut self, blocks: &[LayoutBlock]) -> String {
        let mut inner = String::new();
        for b in blocks {
            inner.push_str(&self.block(b));
        }
        format!("#keep-together[\n{inner}]\n")
    }

    /// 答案块：`5. B` + 逐段解析；既无答案又无解析的块直接跳过
    fn answer(&mut self, answer: &AnswerBlock) -> String {
        if answer.is_empty() {
            return String::new();
        }
        let mut content = String::new();
        for line in &answer.lines {
            if !content.is_empty() {
                content.push_str("#linebreak()");
            }
            if !line.label.is_empty() {
                content.push_str(&format!("#({})", typst_str(&format!("{} ", line.label))));
            }
            self.field = IssueField::Answer;
            content.push_str(&self.nodes_with(&line.nodes, IssueField::Answer));
        }
        for entry in &answer.analyses {
            if !content.is_empty() {
                content.push('\n');
            }
            self.field = IssueField::Analysis;
            content.push_str(&format!(
                "#analysis({}, [{}])",
                typst_str(&entry.title),
                self.nodes_with(&entry.nodes, IssueField::Analysis)
            ));
        }
        self.item(
            &format!("{}. ", answer.number),
            HANG_EM,
            6.0,
            answer.meta,
            &content,
        )
    }

    /// 四类提示框的配色；纯黑模式下只剩黑与白
    fn callout_colors(&self, kind: CalloutKind) -> (&'static str, &'static str) {
        if self.black {
            return ("luma(0%)", "luma(100%)");
        }
        match kind {
            CalloutKind::Knowledge => ("rgb(\"#1F4E79\")", "rgb(\"#EAF1F8\")"),
            CalloutKind::ErrorProne => ("rgb(\"#B00000\")", "rgb(\"#FCECEA\")"),
            CalloutKind::Tip => ("rgb(\"#2E7D32\")", "rgb(\"#EAF6EC\")"),
            CalloutKind::Approach => ("rgb(\"#B26A00\")", "rgb(\"#FFF5E6\")"),
        }
    }

    // ------------------------------------------------------------ 行内内容

    fn nodes(&mut self, nodes: &[InlineNode]) -> String {
        let field = self.field;
        self.nodes_with(nodes, field)
    }

    /// 把一段行内节点铺成 markup 片段（文本走 `#("…")`，公式走 `$…$`）
    fn nodes_with(&mut self, nodes: &[InlineNode], field: IssueField) -> String {
        let mut out = String::new();
        for node in nodes {
            match node {
                InlineNode::Text { text } => {
                    if !text.is_empty() {
                        out.push_str(&format!("#({})", typst_str(text)));
                    }
                }
                InlineNode::LineBreak => out.push_str("#linebreak()"),
                InlineNode::Math { latex, display } => {
                    out.push_str(&self.math(latex, *display, field));
                }
                InlineNode::Image {
                    alt,
                    url,
                    width,
                    align,
                } => {
                    if let Some(image) = self.image(url, alt.as_deref(), *width) {
                        out.push_str("\n\n");
                        out.push_str(&self.align(align, &image));
                        out.push('\n');
                    }
                }
                InlineNode::ImgRow {
                    align,
                    images,
                    caption,
                } => {
                    let row = self.img_row(images);
                    if row.is_empty() {
                        continue;
                    }
                    out.push_str("\n\n");
                    out.push_str(&self.align(align, &row));
                    if let Some(caption) = caption.as_deref().filter(|t| !t.trim().is_empty()) {
                        out.push_str(&format!(
                            "\n#text(size: 9pt, fill: luma(80))[#({})]",
                            typst_str(caption)
                        ));
                    }
                    out.push('\n');
                }
                InlineNode::Table {
                    header,
                    aligns,
                    rows,
                } => {
                    let table = self.table(header, aligns, rows);
                    if !table.is_empty() {
                        out.push_str("\n\n");
                        out.push_str(&table);
                        out.push('\n');
                    }
                }
            }
        }
        out
    }

    /// 一枚公式：转不动就降级 + 记 Issue，绝不向上传错
    fn math(&mut self, latex: &str, display: bool, field: IssueField) -> String {
        match to_typst(latex, display) {
            // 块级公式独立成段：两边各留一个空行，段内 `$…$` 会自己占一行
            Ok(code) if display => format!("\n{code}\n"),
            Ok(code) => code,
            Err(reason) => {
                self.issue(
                    field,
                    IssueSeverity::Warning,
                    Some(latex),
                    format!("PDF 公式降级：{reason}"),
                );
                degraded(latex)
            }
        }
    }

    fn image(&mut self, url: &str, alt: Option<&str>, width: Option<u32>) -> Option<String> {
        // 表里有值但为 None = 上游预取已记明原因地跳过；这里再报一条只会把警告清单撑爆
        let path = match self.images.get(url) {
            None => {
                self.issue(
                    IssueField::Image,
                    IssueSeverity::Warning,
                    None,
                    format!("PDF 跳过图片 {url}：没有可用素材"),
                );
                return None;
            }
            Some(path) => path.as_deref()?,
        };
        let mut args = vec![typst_str(path)];
        if let Some(alt) = alt.filter(|t| !t.trim().is_empty()) {
            args.push(format!("alt: {}", typst_str(alt)));
        }
        if let Some(px) = width {
            // 上限只能是绝对值：typst 0.15 的 min 不许 ratio 与 length 比较（实测）
            let width_mm = (px as f32 * PX_MM).clamp(1.0, self.spec.column_width_mm());
            args.push(format!("width: {}mm", mm(width_mm)));
        }
        Some(format!("#image({})", args.join(", ")))
    }

    fn img_row(&mut self, images: &[InlineImage]) -> String {
        let cells: Vec<String> = images
            .iter()
            .filter_map(|image| self.image(&image.url, image.alt.as_deref(), image.width))
            .map(|src| format!("[{src}]"))
            .collect();
        if cells.is_empty() {
            return String::new();
        }
        format!(
            "#grid(columns: {}, gutter: 4pt, {})",
            cells.len(),
            cells.join(", ")
        )
    }

    /// 管道表格：单元格是原始文本，v1 只逐字上图（不二次解析其中的加粗与公式）
    fn table(&mut self, header: &[String], aligns: &[TableAlign], rows: &[Vec<String>]) -> String {
        let cols = header
            .len()
            .max(rows.iter().map(Vec::len).max().unwrap_or(0));
        if cols == 0 {
            return String::new();
        }
        let mut args: Vec<String> = Vec::new();
        if !header.is_empty() {
            args.push(format!("table.header({})", cells(header, cols, true)));
        }
        for row in rows {
            args.push(cells(row, cols, false));
        }
        format!(
            "#table(columns: {}, gutter: 4pt, inset: (x: 3pt, y: 1pt), stroke: 0.4pt + luma(170), align: ({}), {})",
            cols,
            aligns_for(aligns, cols),
            args.join(", ")
        )
    }

    fn align(&self, align: &Option<ImageAlign>, content: &str) -> String {
        match align {
            Some(align) => format!(
                "#align({})[{content}]",
                match align {
                    ImageAlign::Left => "left",
                    ImageAlign::Center => "center",
                    ImageAlign::Right => "right",
                }
            ),
            None => content.to_string(),
        }
    }

    fn issue(
        &mut self,
        field: IssueField,
        severity: IssueSeverity,
        latex: Option<&str>,
        reason: String,
    ) {
        self.issues.push(Issue {
            question_no: self.number,
            field,
            severity,
            latex: latex.map(String::from),
            reason,
        });
    }
}

// ═══════════════════════════════════ 小工具 ═══════════════════════════════════

/// 一行表格单元格：补齐到 `cols` 列，`bold` 用于表头
fn cells(row: &[String], cols: usize, bold: bool) -> String {
    (0..cols)
        .map(|i| {
            let text = typst_str(row.get(i).map(String::as_str).unwrap_or(""));
            if bold {
                format!("[#text(weight: \"bold\")[#({text})]]")
            } else {
                format!("[#({text})]")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// 各列对齐：缺省左对齐，长度补齐到列数
fn aligns_for(aligns: &[TableAlign], cols: usize) -> String {
    (0..cols)
        .map(
            |i| match aligns.get(i).copied().unwrap_or(TableAlign::Left) {
                TableAlign::Left => "left",
                TableAlign::Center => "center",
                TableAlign::Right => "right",
            },
        )
        .collect::<Vec<_>>()
        .join(", ")
}

/// 这段内容的高估得准吗：只能按「宽 ÷ 栏宽 = 行数」折行估的内容才算准。
///
/// 硬换行 / 表格 / 并排图组 / 块级公式由 [`choice_grid::requires_single_column`] 认（与栅格
/// 决策同一判据，不各写一套），图片另外单独排除 —— 只知道像素宽、不知道宽高比，
/// 等比缩放后的毫米高在 Rust 侧无从得知。
fn measurable(nodes: &[InlineNode]) -> bool {
    !choice_grid::requires_single_column(nodes)
        && !nodes
            .iter()
            .any(|n| matches!(n, InlineNode::Image { .. } | InlineNode::ImgRow { .. }))
}

/// 把块序列切成「一次出的连续段」（返回 `Range<usize>`，长度 1 = 照常单出）
///
/// 规则：`keep_with_next` 链一直粘到它的终结块（第一个不再往前粘的块），但**只粘预算装得下的
/// 那段后缀**。从链尾往回吞而不是从链头往后吞，是因为最要命的孤立场景就在链尾 ——「最后一个小问
/// 的标号留在页脚，一整块作答区跑到下一页去」。预算吞不下时前面的块照旧各自可跨页：
/// 长题干本来就允许腰斩，为了粘它做出一块超过一页的整块，代价是版面溢出，比腰斩难看得多。
fn plan_groups(blocks: &[LayoutBlock], spec: &LayoutSpec) -> Vec<Range<usize>> {
    let budget = budget_mm(spec);
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut i = 0;
    while i < blocks.len() {
        if !blocks[i].meta().keep_with_next {
            out.push(i..i + 1);
            i += 1;
            continue;
        }
        // term = 这条链要粘到的终结块下标（链尾自己带 keep 时粘到序列末尾）
        let mut term = i;
        while term + 1 < blocks.len() && blocks[term].meta().keep_with_next {
            term += 1;
        }
        // 先试粘到终结块；终结块估不准（提示框、内嵌答案）就退一格，只粘链内的题面块。
        // 退让丢的是「与终结块那一环」的粘连，不是整条链。
        let mut glue: Option<Range<usize>> = None;
        for end in [term, term.saturating_sub(1)] {
            if end <= i {
                continue;
            }
            if let Some(start) = suffix_start(blocks, i, end, budget, spec) {
                glue = Some(start..end + 1);
                break;
            }
        }
        match glue {
            Some(range) => {
                // 壳前面的块照旧各自可跨页
                for idx in i..range.start {
                    out.push(idx..idx + 1);
                }
                // 回跳到壳尾而不是链尾：退让丢下的那些块（估不准的终结块）仍然要出
                i = range.end;
                out.push(range);
            }
            None => {
                for idx in i..=term {
                    out.push(idx..idx + 1);
                }
                i = term + 1;
            }
        }
    }
    out
}

/// 从 `end` 往回吞到预算边界，返回可合并段的最左下标（吞不满两块算失败）
///
/// 任何一块估不准就整段放弃：合并段的壳里混进一张高度未知的图，等于赌一次溢出裁切。
fn suffix_start(
    blocks: &[LayoutBlock],
    from: usize,
    end: usize,
    budget: f64,
    spec: &LayoutSpec,
) -> Option<usize> {
    let mut used = 0.0_f64;
    let mut start = end + 1;
    let mut k = end + 1;
    while k > from {
        k -= 1;
        let h = block_height_mm(&blocks[k], spec)?;
        if used + h > budget {
            break;
        }
        used += h;
        start = k;
    }
    (end + 1 - start >= 2).then_some(start)
}

/// 一块的保守估高（mm）。`None` = 估不准（图、表格、显式换行、块级公式、Callout、答案区）
///
/// `None` 的口径永远是「那就别粘」，不是「照粘」。
fn block_height_mm(block: &LayoutBlock, spec: &LayoutSpec) -> Option<f64> {
    let avail = est_avail_em(spec);
    // 一行放不下就折行：宽度（em，含标号占位）÷ 可用栏宽
    let lines_of = |nodes: &[InlineNode], slack_em: f64| -> Option<f64> {
        if !measurable(nodes) {
            return None;
        }
        Some(
            ((choice_grid::inline_width(nodes) + slack_em) / avail)
                .ceil()
                .max(1.0),
        )
    };
    let lines = match block {
        // 留白的高是确定的：它就是 `BlankBlock::height_mm`
        LayoutBlock::Blank(b) => return Some(f64::from(b.height_mm.max(0.0))),
        LayoutBlock::Question(q) => {
            // 首行被「12. （5 分）」占掉一截，题号宽度按悬挂缩进计
            let mut lines = lines_of(&q.stem, HANG_EM as f64)?;
            if !q.options.is_empty() {
                let col_em = (avail / q.grid.columns.max(1) as f64).max(1.0);
                let mut per_row = 1.0_f64;
                for o in &q.options {
                    if !measurable(&o.content) {
                        return None;
                    }
                    let w = choice_grid::inline_width(&o.content) + HANG_EM as f64;
                    per_row = per_row.max((w / col_em).ceil().max(1.0));
                }
                // 每行都按最宽那条算：宁可估高、少粘一块，也不许估低做出超页整块
                lines += per_row * q.grid.rows.max(1) as f64;
            }
            lines
        }
        LayoutBlock::SubQuestion(s) => lines_of(&s.stem, HANG_EM as f64)?,
        // Callout 与答案块自带底色 / 内衬 / 多段解析，版面高度不在估得准的范围里
        LayoutBlock::Callout(_) | LayoutBlock::Answer(_) => return None,
    };
    // 行高 8mm 与留白横线同口径（BLANK_LINE_MM），块间再留 par spacing
    Some(lines * f64::from(BLANK_LINE_MM) + PAR_SPACING_MM)
}

/// 一组的预算：版心高（纸高 − 上下边距）的 3/4。留 1/4 是给估高误差和首页大卷头的余量
fn budget_mm(spec: &LayoutSpec) -> f64 {
    let (_, h) = spec.paper.size_mm();
    let text = f64::from(h) - f64::from(spec.margins.top_mm + spec.margins.bottom_mm);
    text * 0.75
}

/// 估高用的可用栏宽（em）：栏宽减题号悬挂缩进，与适配器 `export::pdf` 同一口径
fn est_avail_em(spec: &LayoutSpec) -> f64 {
    (choice_grid::em_from_mm(f64::from(spec.column_width_mm())) - HANG_EM as f64).max(1.0)
}

/// 长度落地：Rust 的 Display 对浮点会省掉无用的零（`22.0` → `22`，`8.5` → `8.5`）
fn mm(value: f32) -> String {
    format!("{}", (value * 100.0).round() / 100.0)
}

/// 分值：整数分不带小数点
fn score(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

// ═══════════════════════════════════ 测试 ═══════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::model::{Callout, ExamOption, QuestionKind};
    use crate::typeset::blocks::choice_grid;
    use crate::typeset::blocks::figure_float::{self, FIGURE_SHARE};
    use crate::typeset::compiler::{
        CompileRequest, PlacedImage, PlacedRun, compile_paged, compile_pdf, font_dirs,
        placed_images, placed_pages, rendered_pages, rendered_runs,
    };
    use crate::typeset::ir::{
        AnalysisEntry, AnswerLine, BlankBlock, CalloutBlock, DocumentMeta, QuestionBlock, Section,
        SectionHeader, SubQuestionBlock,
    };
    use crate::typeset::spec::OutputProfile;
    use std::path::{Path, PathBuf};

    /// 1×1 灰块 SVG：注入型素材（`/ext/<n>`）的替身，省掉一张二进制固件
    const DOT_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="#333"/></svg>"##;
    const REMOTE: &str = "https://cdn.example.com/a.png";

    fn text(s: &str) -> InlineNode {
        InlineNode::Text {
            text: s.to_string(),
        }
    }

    fn math(latex: &str) -> InlineNode {
        InlineNode::Math {
            latex: latex.to_string(),
            display: false,
        }
    }

    fn stem(i: usize) -> Vec<InlineNode> {
        vec![
            text("已知函数 f(x) = "),
            math(r"\sin(2x + \frac{\pi}{3})"),
            text(&format!("，则第 {i} 题的最小正周期为（　）")),
        ]
    }

    /// 选项栅格的可用栏宽（em）：与适配器 `export::pdf` 同一口径（栏宽 − 题号悬挂缩进）
    fn available_em(spec: &LayoutSpec) -> f64 {
        (choice_grid::em_from_mm(f64::from(spec.column_width_mm())) - 2.0).max(1.0)
    }

    fn question(i: usize, kind: QuestionKind, score: f64, avail_em: f64) -> QuestionBlock {
        let options = match kind {
            QuestionKind::SingleChoice | QuestionKind::MultiChoice => ('A'..='D')
                .map(|label| ExamOption {
                    label: label.to_string(),
                    content: vec![math(&format!(
                        "\\frac{{{}}}{{x_{{{i}}}}}",
                        label as u8 - b'A' + 1
                    ))],
                })
                .collect(),
            _ => Vec::new(),
        };
        let grid = choice_grid::decide(&options, avail_em);
        QuestionBlock {
            meta: BlockMeta::flow(),
            number: i as u32,
            score,
            kind,
            stem: stem(i),
            options,
            grid,
            figure: None,
        }
    }

    fn callout(i: usize, kind: CalloutKind, title: &str) -> CalloutBlock {
        CalloutBlock {
            meta: BlockMeta::flow(),
            number: i as u32,
            callout: Callout {
                kind,
                title: title.to_string(),
                nodes: vec![text("先化简解析式，再对照周期公式 T = 2π/|ω|。")],
            },
        }
    }

    /// 20 题仿真卷：四种题型 + 小问 + 图 + 提示框 + 留白 + 卷末答案
    fn sim_doc(spec: LayoutSpec) -> LayoutDoc {
        let avail = available_em(&spec);
        let mut sections: Vec<Section> = Vec::new();
        let groups: Vec<(String, QuestionKind, usize)> = vec![
            ("一、单项选择题".into(), QuestionKind::SingleChoice, 8),
            ("二、多项选择题".into(), QuestionKind::MultiChoice, 4),
            ("三、填空题".into(), QuestionKind::Fill, 4),
            ("四、解答题".into(), QuestionKind::Solution, 4),
        ];
        let mut number = 1usize;
        for (title, kind, count) in groups {
            let mut blocks = Vec::new();
            for _ in 0..count {
                let i = number;
                number += 1;
                let mut q = question(
                    i,
                    kind,
                    if kind == QuestionKind::Solution {
                        12.0
                    } else {
                        5.0
                    },
                    avail,
                );
                if i == 3 {
                    // 块级公式 + 外链图：都要在真编译里活下来
                    q.stem.push(text("，其图象为"));
                    q.stem.push(InlineNode::Math {
                        latex: r"y = A\cos(\omega x)".into(),
                        display: true,
                    });
                    q.stem.push(InlineNode::Image {
                        alt: Some("函数图象".into()),
                        url: REMOTE.into(),
                        width: Some(420),
                        align: Some(crate::export::model::ImageAlign::Center),
                    });
                }
                blocks.push(LayoutBlock::Question(q));
                if kind == QuestionKind::Solution {
                    blocks.push(LayoutBlock::SubQuestion(SubQuestionBlock {
                        meta: BlockMeta::flow(),
                        number: i as u32,
                        depth: 1,
                        label: "(1)".into(),
                        stem: vec![text("求 f(x) 的单调递增区间；")],
                    }));
                    blocks.push(LayoutBlock::Callout(callout(
                        i,
                        CalloutKind::Approach,
                        "思路拆解",
                    )));
                    blocks.push(LayoutBlock::Blank(BlankBlock::new(
                        i as u32,
                        &spec.resolve_blank(Some(6.0)).unwrap(),
                    )));
                } else if kind == QuestionKind::SingleChoice {
                    blocks.push(LayoutBlock::Callout(callout(
                        i,
                        CalloutKind::ErrorProne,
                        "易错警示",
                    )));
                }
            }
            sections.push(Section {
                header: SectionHeader {
                    meta: BlockMeta::glued(),
                    title,
                    instruction: (kind == QuestionKind::SingleChoice)
                        .then(|| "每小题给出的四个选项中，只有一个符合题意。".to_string()),
                    question_count: count,
                    total_score: count as f64
                        * if kind == QuestionKind::Solution {
                            12.0
                        } else {
                            5.0
                        },
                },
                blocks,
            });
        }
        let answer_key = (1..=number - 1)
            .map(|i| AnswerBlock {
                meta: BlockMeta::flow(),
                number: i as u32,
                kind: QuestionKind::Fill,
                lines: vec![
                    AnswerLine {
                        label: String::new(),
                        nodes: vec![text("B")],
                    },
                    AnswerLine {
                        label: "(1)".into(),
                        nodes: vec![
                            text("递增区间为 "),
                            math(r"[-\frac{\pi}{6}, \frac{\pi}{3}]"),
                        ],
                    },
                ],
                analyses: vec![AnalysisEntry {
                    title: "解析".into(),
                    nodes: vec![text("由周期公式直接得解。")],
                }],
            })
            .collect();
        LayoutDoc {
            title: "三角函数综合练习".into(),
            subtitle: Some("2025 届高三周测".into()),
            meta: DocumentMeta {
                school: Some("示例中学".into()),
                duration_min: Some(90),
                total_score: 100.0,
                instructions: vec![
                    "1. 答题前请填写姓名与考号。".into(),
                    "2. 本卷共 20 题。".into(),
                ],
            },
            profile: spec.profile,
            spec,
            sections,
            answer_key,
            issues: Vec::new(),
        }
    }

    fn images() -> HashMap<String, Option<String>> {
        HashMap::from([(REMOTE.to_string(), Some("/ext/0.svg".to_string()))])
    }

    fn request<'a>(
        source: &'a str,
        dirs: &'a [PathBuf],
        injected: &'a [(String, Vec<u8>)],
    ) -> CompileRequest<'a> {
        CompileRequest {
            source,
            upload_dir: Path::new("uploads"),
            font_dirs: dirs,
            injected,
        }
    }

    /// 仿真卷里那枚外链图的替身字节
    fn dot_injected() -> Vec<(String, Vec<u8>)> {
        vec![("/ext/0.svg".to_string(), DOT_SVG.as_bytes().to_vec())]
    }

    fn generate_and_compile(
        spec: LayoutSpec,
        doc_images: Option<HashMap<String, Option<String>>>,
    ) -> String {
        let doc = sim_doc(spec);
        let generated = generate(&doc, doc_images.as_ref().unwrap_or(&images()));
        let dirs = font_dirs();
        let injected = dot_injected();
        let req = request(&generated.source, &dirs, &injected);
        match compile_pdf(&req) {
            Ok(out) => {
                assert!(out.output.starts_with(b"%PDF"), "产物不是 PDF");
                generated.source
            }
            // 诊断不带行列号（compiler 的口径），只能整份源码一起给
            Err(err) => panic!(
                "编译失败：{:?}\n---- 源码 ----\n{}",
                err.diagnostics, generated.source
            ),
        }
    }

    /// 版面明文：把所有文字段不加分隔拼起来（换行折出来的两段之间不该有空格）
    fn glue(runs: &[crate::typeset::compiler::RenderedRun]) -> String {
        runs.iter().map(|r| r.text.as_str()).collect()
    }

    fn is_cjk(c: char) -> bool {
        matches!(c,
            '\u{3000}'..='\u{303f}' | '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}'
            | '\u{ff00}'..='\u{ffef}')
    }

    // ------------------------------------------------------------ 结构断言

    #[test]
    fn source_carries_master_and_function_calls() {
        let doc = sim_doc(LayoutSpec::for_profile(OutputProfile::Teacher));
        let g = generate(&doc, &images());
        let s = &g.source;
        // 母版：mitex 定义块只出现一次，set 规则齐全
        assert!(s.starts_with(MITEX_PREAMBLE));
        assert_eq!(
            s.matches(MITEX_PREAMBLE.split('\n').next().unwrap())
                .count(),
            1
        );
        for needle in [
            "#set page(",
            "#set text(font: body-font",
            "#set par(justify: true",
            "#let item(",
            "#let choices(",
            "#let section-header(",
            "#let callout-box(",
            "#let analysis(",
            "#let blank-lines(",
            "#let blank-dots(",
            "#let blank-space(",
            "#let masthead(",
        ] {
            assert!(s.contains(needle), "函数库缺 {needle}");
        }
        // 正文：五类调用都在
        for needle in [
            "#masthead(",
            "#section-header(",
            "#item(",
            "#choices(",
            "#callout-box(",
            "#blank-lines(",
            "#analysis(",
            "#pagebreak(weak: true)",
        ] {
            assert!(s.contains(needle), "正文缺 {needle}");
        }
        assert_eq!(
            s.matches("#section-header(").count(),
            5,
            "4 个大题标题 + 1 个卷末答案区标题（函数库里的 #let 定义不算调用）"
        );
        // 单栏：页级栏数 1，且不包容器
        assert!(s.contains("#set page(width: 210mm, height: 297mm, columns: 1"));
        assert!(
            !s.contains("#columns("),
            "栏只能页级设置，容器版会让 #pagebreak 失效"
        );
        assert!(!s.contains("#pagebreak(here:"), "typst 0.15 没有 here 参数");
    }

    #[test]
    fn multi_column_wraps_body_and_keeps_gutter() {
        let spec = LayoutSpec::preset("a4_practice").unwrap();
        let s = generate(&sim_doc(spec), &images()).source;
        assert!(s.contains("#set page(width: 210mm, height: 297mm, columns: 2"));
        assert!(s.contains("#set columns(gutter: 8mm)"));
        assert!(s.contains("width: 210mm, height: 297mm"));
        // 容器版会把卷末答案的 #pagebreak 变成编译错误
        assert!(
            !s.contains("#columns("),
            "栏必须页级设置，不许用 #columns() 容器"
        );
    }

    #[test]
    fn a3_fold_preset_lands_as_landscape_two_columns() {
        let spec = LayoutSpec::preset("a3_fold_exam").unwrap();
        let s = generate(&sim_doc(spec), &images()).source;
        assert!(s.contains("width: 420mm, height: 297mm"));
        assert!(s.contains("columns: 2"));
        assert!(s.contains("#set columns(gutter: 12mm)"));
        // 页码：计数器只能在 context 里求值
        assert!(s.contains("footer: context align(center)"));
        assert!(s.contains("第 #counter(page).display(\"1\") 页"));
    }

    #[test]
    fn fonts_and_colors_come_from_spec() {
        let mut spec = LayoutSpec::default();
        spec.fonts.body = "Fira Sans".into();
        spec.fonts.math = "STIX Two Math".into();
        let s = generate(&sim_doc(spec), &images()).source;
        assert!(s.contains("#let body-font = (\"Fira Sans\", \"Libertinus Serif\")"));
        // 实测：`#set math(font:)` 是无效语法，数学字体恒为 New Computer Modern Math
        assert!(!s.contains("#set math("));
        // 教师模式的四类提示框各带自己的配色
        assert!(s.contains("#callout-box(rgb(\"#B00000\"), rgb(\"#FCECEA\")"));
        assert!(s.contains("#callout-box(rgb(\"#B26A00\"), rgb(\"#FFF5E6\")"));
    }

    #[test]
    fn print_black_only_flattens_every_color() {
        let spec = LayoutSpec {
            color: ColorMode::PrintBlackOnly,
            ..Default::default()
        };
        let s = generate(&sim_doc(spec), &images()).source;
        assert!(s.contains("#set text(fill: luma(0%))"));
        assert!(s.contains("#let accent = luma(0%)"));
        assert!(!s.contains("rgb(\"#1F4E79\")"), "纯黑模式不许留彩色强调色");
        assert!(!s.contains("rgb(\"#FCECEA\")"), "提示框底色也得退成白");
        assert!(s.contains("#callout-box(luma(0%), luma(100%)"));
    }

    #[test]
    fn image_width_is_capped_by_column_and_recorded_in_mm() {
        let spec = LayoutSpec::preset("a4_practice").unwrap(); // 栏宽 86mm
        let s = generate(&sim_doc(spec), &images()).source;
        // 420px ≈ 111mm → 必须裁到栏宽 86mm
        assert!(
            s.contains("#image(\"/ext/0.svg\", alt: \"函数图象\", width: 86mm)"),
            "{s}"
        );
        assert!(!s.contains("111mm"));
    }

    #[test]
    fn missing_image_is_skipped_and_reported_not_fatal() {
        let doc = sim_doc(LayoutSpec::default());
        let g = generate(&doc, &HashMap::new());
        assert!(!g.source.contains("#image("), "没有素材就不该有 #image");
        let issue = g
            .issues
            .iter()
            .find(|i| i.field == IssueField::Image)
            .expect("缺图必须记 Issue");
        assert_eq!(issue.question_no, Some(3));
        assert_eq!(issue.severity, IssueSeverity::Warning);
        assert!(issue.reason.contains(REMOTE), "{}", issue.reason);
    }

    #[test]
    fn upstream_rejected_image_is_skipped_silently() {
        // 表里有值但为 None = 预取阶段已经报过「为什么拿不到」，这里再补一条就是重复警告
        let doc = sim_doc(LayoutSpec::default());
        let images = HashMap::from([(REMOTE.to_string(), None)]);
        let g = generate(&doc, &images);
        assert!(!g.source.contains("#image("), "{:?}", g.source);
        assert!(
            g.issues.iter().all(|i| i.field != IssueField::Image),
            "{:?}",
            g.issues
        );
    }

    #[test]
    fn unknown_command_degrades_one_formula_without_touching_the_rest() {
        // UNSUPPORTED 里的 `\argmax_x`：mitex 转得动但 typst 不认，守卫会拦下来
        let spec = LayoutSpec::default();
        let mut doc = sim_doc(spec.clone());
        let mut q = question(1, QuestionKind::Fill, 5.0, available_em(&spec));
        q.stem = vec![
            text("求 "),
            InlineNode::Math {
                latex: r"\argmax_x f".into(),
                display: false,
            },
            text(" 的值域"),
        ];
        doc.sections[0].blocks[0] = LayoutBlock::Question(q);
        let g = generate(&doc, &images());
        let issue = g
            .issues
            .iter()
            .find(|i| i.field == IssueField::Stem)
            .expect("降级必须记 Issue");
        assert_eq!(issue.question_no, Some(1));
        assert_eq!(issue.latex.as_deref(), Some(r"\argmax_x f"));
        assert!(issue.reason.starts_with("PDF 公式降级："));
        assert!(
            g.source.contains(r#"#("\\argmax_x f")"#),
            "降级要把原文逐字上图：{}",
            g.source
        );
        // 其余公式不受影响：第 2 题照常带着它的题干与选项
        assert!(g.source.contains("则第 2 题的最小正周期为"));
        assert!(g.source.contains("#choices("));
    }

    // ------------------------------------------------------------ 真编译

    #[test]
    fn simulated_20_questions_compile_to_pdf() {
        let source = generate_and_compile(LayoutSpec::for_profile(OutputProfile::Teacher), None);
        assert!(source.contains("三角函数综合练习"));
    }

    #[test]
    fn simulated_paper_has_no_tofu_and_keeps_its_text() {
        let doc = sim_doc(LayoutSpec::preset("a4_practice").unwrap());
        let g = generate(&doc, &images());
        let dirs = font_dirs();
        let injected = dot_injected();
        let req = request(&g.source, &dirs, &injected);
        let compiled = compile_paged(&req).unwrap_or_else(|e| panic!("{}", e.summary()));
        assert!(compiled.output.pages().len() > 1, "20 题不该只有一页");
        let runs = rendered_runs(&compiled.output);
        let text = glue(&runs);
        for needle in [
            "三角函数综合练习",
            "单项选择题",
            "参考答案与解析",
            "易错警示",
            "第 1 页",
        ] {
            assert!(text.contains(needle), "版面缺「{needle}」");
        }
        // 豆腐块判定：每一个中文字都得由思源系字体画
        let bad: Vec<&str> = runs
            .iter()
            .filter(|r| r.text.chars().any(is_cjk) && !r.family.starts_with("Source Han"))
            .map(|r| r.family.as_str())
            .collect();
        assert!(bad.is_empty(), "中文字被非思源字体画了：{bad:?}");
    }

    #[test]
    fn markup_triggers_stay_literal_text() {
        let mut spec = LayoutSpec::default();
        spec.header_footer.header_title = true;
        let mut doc = sim_doc(spec);
        doc.title = "= 标题（一）".into();
        let question = match &mut doc.sections[0].blocks[0] {
            LayoutBlock::Question(q) => q,
            _ => unreachable!(),
        };
        question.stem = vec![
            text("- 讨论三种情况"),
            InlineNode::LineBreak,
            text("1. 第一种 // 注释样"),
            InlineNode::LineBreak,
            text("2. 第二种 *粗* 与 ] 和 [ 与 $5"),
            InlineNode::LineBreak,
            InlineNode::Table {
                header: vec!["表头".into(), "- 甲".into()],
                aligns: vec![],
                rows: vec![vec!["\"引号\"".into(), "= 乙".into()]],
            },
        ];
        let g = generate(&doc, &images());
        let dirs = font_dirs();
        let injected = dot_injected();
        let req = request(&g.source, &dirs, &injected);
        let compiled = compile_paged(&req).unwrap_or_else(|e| panic!("{}", e.summary()));
        let runs = rendered_runs(&compiled.output);
        let text = glue(&runs);
        assert!(text.contains("- 讨论三种情况"), "{text}");
        assert!(text.contains("1. 第一种 // 注释样"), "{text}");
        assert!(text.contains("2. 第二种 *粗* 与 ] 和 [ 与 $5"), "{text}");
        assert!(text.contains("= 标题（一）"), "页眉里的 = 也不许变成标题");
        assert!(text.contains("- 甲"), "表格单元格里的 - 变成了列表");
        assert!(text.contains("\"引号\""), "表格单元格里的引号被改写了");
        // 列表与标题的特征产物：项目符号与标题字体
        assert!(
            !runs.iter().any(|r| r.text.trim() == "•"),
            "行首 - 变成了列表项目符号"
        );
        // 外部文本进 markup 必须包成 `#("…")`：裸字面量 `["…"]` 会被 typst 当成引号文本
        // 智能配对，实测把选项标签排成了 “A. “（选项 / 表格格 / 图注 / 答题标签四处同罪）
        assert!(text.contains("A. "), "选项标签没按明文上图");
        let smart: Vec<&str> = runs
            .iter()
            .map(|r| r.text.as_str())
            .filter(|t| t.contains('“') || t.contains('”'))
            .collect();
        assert!(
            smart.is_empty(),
            "版面出现了智能引号（字符串漏了 `#(…)`）：{smart:?}"
        );
    }

    #[test]
    fn student_profile_omits_teacher_only_blocks() {
        // 留白开关与答案位置由 options 决定（适配器已经算完），这里只验生成器忠实落地
        let mut doc = sim_doc(LayoutSpec::for_profile(OutputProfile::Student));
        doc.answer_key.clear();
        for section in &mut doc.sections {
            section
                .blocks
                .retain(|b| !matches!(b, LayoutBlock::Callout(_)));
        }
        let g = generate(&doc, &images());
        assert!(!g.source.contains("#callout-box("), "教师专属块不该出现");
        assert!(!g.source.contains("参考答案与解析"));
        assert!(!g.source.contains("#analysis("));
        assert!(g.source.contains("#blank-lines("));
    }

    // ---------------------------------------------------- T4.5 防跨页与粘连

    /// 题干文字：1~4 句，随题号变化
    ///
    /// 必须有多行，单行块永远不会被页界劈开 —— 边界卷就造不出来，用例沦为空断言。
    fn stem_text(i: usize) -> String {
        let mut s =
            format!("第 {i} 题题干：已知函数由下面的条件给出，请据此求出它的解析式与最小正周期。");
        for _ in 0..i % 4 {
            s.push_str("进一步地，设新函数为原函数与余弦函数之和，求它在给定闭区间上的最值，并说明参数取值对结果的影响。");
        }
        s
    }

    /// 一题的题面块：题干与小问都往前粘（`BlockMeta::attach()` 的语义就是「粘住下一块」）
    fn pair(i: usize, stem_meta: BlockMeta) -> Vec<LayoutBlock> {
        vec![
            LayoutBlock::Question(QuestionBlock {
                meta: stem_meta,
                number: i as u32,
                score: 6.0,
                kind: QuestionKind::Solution,
                stem: vec![text(&stem_text(i))],
                options: Vec::new(),
                grid: choice_grid::ChoiceGrid {
                    columns: 1,
                    rows: 0,
                },
                figure: None,
            }),
            LayoutBlock::SubQuestion(SubQuestionBlock {
                meta: stem_meta,
                number: i as u32,
                depth: 0,
                label: format!("({i}) "),
                stem: vec![text(&format!("第 {i} 小问：写出 f 的单调区间。"))],
            }),
        ]
    }

    /// 一题的链收尾：清掉最后一块的 `keep_with_next`
    ///
    /// 与适配器 `export::pdf::question_blocks` 同一条规则 —— 粘连只在一题之内。模板给最后一块
    /// 置位是「粘住它后面的小问 / 留白」的意思，传到序列末尾就会把两道题焊成一枚整块。
    fn closed(mut run: Vec<LayoutBlock>) -> Vec<LayoutBlock> {
        if let Some(last) = run.last_mut() {
            last.meta_mut().keep_with_next = false;
        }
        run
    }

    /// 单大题、N 道同构小题；`tail` 是第 N 题自己的后续块（留白、提示框），仍属该题的 run
    fn pair_doc(stem_meta: BlockMeta, n: usize, tail: Vec<LayoutBlock>) -> LayoutDoc {
        let mut doc = sim_doc(LayoutSpec::for_profile(OutputProfile::Student));
        doc.answer_key.clear();
        doc.sections.truncate(1);
        doc.sections[0].header.instruction = None;
        doc.sections[0].header.question_count = n;
        // 页脚逐页画一遍，正文跨页时它的明文会插在题干中间，把「粘连没改内容」这条断言带偏
        doc.spec.header_footer.page_number = false;
        let mut blocks: Vec<LayoutBlock> =
            (1..n).flat_map(|i| closed(pair(i, stem_meta))).collect();
        let mut last = pair(n, stem_meta);
        last.extend(tail);
        blocks.extend(closed(last));
        doc.sections[0].blocks = blocks;
        doc
    }

    fn blank(number: u32, height_mm: f32) -> LayoutBlock {
        LayoutBlock::Blank(BlankBlock {
            meta: BlockMeta::solid(),
            number,
            height_mm,
            style: BlankStyle::Lines,
        })
    }

    fn callout_block(number: u32) -> LayoutBlock {
        LayoutBlock::Callout(callout(number as usize, CalloutKind::Approach, "思路拆解"))
    }

    /// `plan_groups` 的通用不变式，任何块序列都得守住，返回分段供用例比对自己的期望
    ///
    /// 1. 分段不重不漏地覆盖整条序列 —— 退让路径曾把没进壳的终结块直接跳过，留白就此消失；
    /// 2. 一枚壳里不许出现模板没要求的粘连：非末位块必须本来就 `keep_with_next`。
    fn planned(blocks: &[LayoutBlock], spec: &LayoutSpec) -> Vec<Range<usize>> {
        let groups = plan_groups(blocks, spec);
        let mut next = 0;
        for r in &groups {
            assert_eq!(r.start, next, "分段之间有缺口或重叠：{groups:?}");
            for idx in r.start..r.end - 1 {
                assert!(
                    blocks[idx].meta().keep_with_next,
                    "组 {r:?} 里的块 {idx} 没要求粘住下一块，不该被粘上"
                );
            }
            next = r.end;
        }
        assert_eq!(next, blocks.len(), "有块被漏掉：{groups:?}");
        groups
    }

    /// 编译到「每页一段明文」
    fn compile_pages(doc: &LayoutDoc) -> Vec<Vec<crate::typeset::compiler::RenderedRun>> {
        let generated = generate(doc, &HashMap::new());
        let dirs = font_dirs();
        let req = request(&generated.source, &dirs, &[]);
        match compile_paged(&req) {
            Ok(out) => rendered_pages(&out.output),
            Err(err) => panic!(
                "编译失败：{:?}\n---- 源码 ----\n{}",
                err.diagnostics, generated.source
            ),
        }
    }

    fn flat(pages: &[Vec<crate::typeset::compiler::RenderedRun>]) -> String {
        pages.iter().map(|p| glue(p)).collect()
    }

    /// 某段文字第一次出现在哪一页
    fn page_of(
        pages: &[Vec<crate::typeset::compiler::RenderedRun>],
        needle: &str,
    ) -> Option<usize> {
        pages.iter().position(|p| glue(p).contains(needle))
    }

    /// 题干与小问被页界劈开的题数
    fn straddling(pages: &[Vec<crate::typeset::compiler::RenderedRun>], n: usize) -> usize {
        (1..=n)
            .filter(|i| {
                let stem = page_of(pages, &format!("第 {i} 题题干"));
                let sub = page_of(pages, &format!("第 {i} 小问"));
                stem.is_some() && stem != sub
            })
            .count()
    }

    #[test]
    fn keep_chain_becomes_one_unbreakable_shell() {
        let doc = pair_doc(BlockMeta::attach(), 3, vec![blank(3, 60.0)]);
        let blocks = &doc.sections[0].blocks;
        // 每道短题一枚整块，第三题的留白算在本题的壳里
        assert_eq!(
            planned(blocks, &doc.spec),
            vec![0..2, 2..4, 4..7],
            "题干 → 小问 → 留白应各自粘成一枚整块"
        );
        let g = generate(&doc, &HashMap::new());
        assert_eq!(g.source.matches("#keep-together[").count(), 3);
        // 壳只改分页，不改内容：一个字都没少
        for needle in ["第 1 题题干", "第 3 小问"] {
            assert!(g.source.contains(needle), "粘连把 {needle} 弄丢了");
        }
        assert!(g.source.contains("#blank-lines("), "留白仍然要画出来");
    }

    #[test]
    fn two_questions_are_never_glued_into_one_shell() {
        // 收尾规则不在这里就守不住：每块都置 keep 时，整段会变成一枚超过一页的整块
        let doc = pair_doc(BlockMeta::attach(), 2, Vec::new());
        assert_eq!(
            planned(&doc.sections[0].blocks, &doc.spec),
            vec![0..2, 2..4],
            "跨题不许粘"
        );
    }

    #[test]
    fn unestimatable_terminator_only_costs_the_last_link() {
        // 链尾接的是提示框（高度估不准）：退一格，只粘链内的题干与小问
        let doc = pair_doc(BlockMeta::attach(), 1, vec![callout_block(1)]);
        assert_eq!(
            planned(&doc.sections[0].blocks, &doc.spec),
            vec![0..2, 2..3],
            "估不准高的终结块应留在壳外"
        );
        let g = generate(&doc, &HashMap::new());
        assert_eq!(g.source.matches("#keep-together[").count(), 1);
        assert!(g.source.contains("#callout-box("), "提示框本身仍要出");
    }

    #[test]
    fn oversized_blank_is_never_wrapped_into_a_group() {
        // 240mm 的留白比整页版心还高（预算 189.75mm）：绝不允许成为壳的一员，那会溢出裁切
        let doc = pair_doc(BlockMeta::attach(), 1, vec![blank(1, 240.0)]);
        assert_eq!(
            planned(&doc.sections[0].blocks, &doc.spec),
            vec![0..2, 2..3],
            "超预算的留白必须留在壳外"
        );
        let g = generate(&doc, &HashMap::new());
        assert_eq!(
            g.source.matches("#keep-together[").count(),
            1,
            "题干与小问仍然互粘"
        );
        assert!(g.source.contains("#blank-lines("), "留白仍然要画出来");
    }

    #[test]
    fn long_stem_may_break_but_its_subquestion_and_answer_space_stay_together() {
        // 链头高过预算时不硬粘：腰斩的是题干，小问标号和它的作答区不能分家（T4.5 验收口径）
        let mut doc = pair_doc(BlockMeta::attach(), 1, vec![blank(1, 60.0)]);
        let LayoutBlock::Question(q) = &mut doc.sections[0].blocks[0] else {
            unreachable!()
        };
        // 25 行 ≈ 202mm，已经高过预算 189.75mm：把它粘进壳就是拿版面溢出换整齐
        q.stem = vec![text(
            &"请逐步推导并写出必要的文字说明、方程和演算步骤，注意单位与取值范围。".repeat(25),
        )];
        assert_eq!(
            planned(&doc.sections[0].blocks, &doc.spec),
            vec![0..1, 1..3],
            "题干照旧可跨页，小问与留白粘成一枚整块"
        );
        let g = generate(&doc, &HashMap::new());
        assert_eq!(g.source.matches("#keep-together[").count(), 1);
        assert!(g.source.contains("#blank-lines("), "留白仍然要画出来");
    }

    #[test]
    fn inline_image_question_stays_splitable() {
        // 图片的高度要等比缩放后才知道，Rust 侧估不准 → 整题不粘
        let mut doc = pair_doc(BlockMeta::attach(), 1, Vec::new());
        let LayoutBlock::Question(q) = &mut doc.sections[0].blocks[0] else {
            unreachable!()
        };
        q.stem.push(InlineNode::Image {
            alt: Some("图象".into()),
            url: REMOTE.into(),
            width: Some(200),
            align: None,
        });
        assert_eq!(
            planned(&doc.sections[0].blocks, &doc.spec),
            vec![0..1, 1..2],
            "一块估不准，整条链都不该粘"
        );
        let g = generate(&doc, &images());
        assert_eq!(
            g.source.matches("#keep-together[").count(),
            0,
            "估不准高的块不许进壳"
        );
    }

    #[test]
    fn gluing_moves_questions_off_the_page_boundary() {
        let n = 30;
        let loose = compile_pages(&pair_doc(BlockMeta::flow(), n, Vec::new()));
        let glued = compile_pages(&pair_doc(BlockMeta::attach(), n, Vec::new()));
        assert!(
            straddling(&loose, n) > 0,
            "边界卷没造出来：{n} 道题全落在页内，本用例就成了空断言"
        );
        assert_eq!(
            straddling(&glued, n),
            0,
            "带 keep_with_next 的题干与小问必须同页"
        );
        // 粘连只改分页，不许改内容
        assert_eq!(flat(&glued), flat(&loose), "粘连把版面文字改了");
    }

    // ------------------------------------------------------ T4.2 选项栅格与降列

    /// 短选项：连标签 ≈ 10mm，四列单元格（16.4mm）装得下
    const SHORT: &str = "12";
    /// 中选项：连标签 ≈ 27mm，四列装不下、半栏装得下
    const MEDIUM: &str = "甲乙丙丁戊己庚";
    /// 长选项：连标签 ≈ 98mm，半栏（36.4mm）也装不下
    const LONG: &str = "一个超过半栏宽很多的选项内容，例如把整句话都塞进选项里";

    /// 一道四选项的题：`columns` 由调用方**直接给定**，故意给错才能验 R7 的运行时兜底
    fn choice_doc(columns: usize, options: [&str; 4]) -> LayoutDoc {
        let mut doc = sim_doc(LayoutSpec::for_profile(OutputProfile::Student));
        doc.answer_key.clear();
        doc.sections.truncate(1);
        doc.sections[0].header.instruction = None;
        doc.sections[0].header.question_count = 1;
        doc.spec.header_footer.page_number = false;
        doc.sections[0].blocks = vec![LayoutBlock::Question(QuestionBlock {
            meta: BlockMeta::flow(),
            number: 1,
            score: 5.0,
            kind: QuestionKind::SingleChoice,
            stem: vec![text("已知甲、乙两数满足下列条件，则甲数为（　）")],
            options: ('A'..='D')
                .zip(options)
                .map(|(label, content)| ExamOption {
                    label: label.to_string(),
                    content: vec![text(content)],
                })
                .collect(),
            grid: choice_grid::ChoiceGrid {
                columns,
                rows: 4usize.div_ceil(columns.max(1)),
            },
            figure: None,
        })];
        doc
    }

    /// 四枚选项标签在版面上的落点（毫米），下标 0..4 即 A..D
    ///
    /// 锚点选标签而不是选项正文：`A. ` 是每枚选项**单元格行首**的唯一明文，它的 x 就是该列的
    /// 左边界、y 就是该行的基线，且不会与题干里出现的字母撞车。
    fn option_positions(doc: &LayoutDoc) -> Vec<(f64, f64)> {
        let generated = generate(doc, &HashMap::new());
        let dirs = font_dirs();
        let req = request(&generated.source, &dirs, &[]);
        let out = match compile_paged(&req) {
            Ok(out) => out,
            Err(err) => panic!(
                "编译失败：{:?}\n---- 源码 ----\n{}",
                err.diagnostics, generated.source
            ),
        };
        let mut hits = [(0.0_f64, 0.0_f64); 4];
        let mut seen = [0_usize; 4];
        let mut all: Vec<String> = Vec::new();
        for placed in placed_pages(&out.output).into_iter().flatten() {
            all.push(placed.run.text.clone());
            let matched = ('A'..='D').position(|c| placed.run.text.starts_with(&format!("{c}. ")));
            let Some(index) = matched else {
                continue;
            };
            seen[index] += 1;
            hits[index] = (placed.x_mm, placed.y_mm);
        }
        assert_eq!(
            seen, [1; 4],
            "选项标签在版面上不是各出现一次：{seen:?}\n版面文字段：{all:?}"
        );
        hits.to_vec()
    }

    /// 按容差把坐标归并成「档」：落在同一档 = 同一行（或同一列）
    fn lanes(values: &[f64], tol: f64) -> usize {
        let mut sorted = values.to_vec();
        sorted.sort_by(f64::total_cmp);
        let mut count = 0_usize;
        let mut prev = f64::MIN;
        for v in sorted {
            if v - prev > tol {
                count += 1;
            }
            prev = v;
        }
        count
    }

    /// 实测栅格 `(行数, 列数)`：同一行（列）的标签坐标差在 0.01mm 量级，跨行（列）则分别是
    /// 行高（≥ 3.4mm）与 1/4 栏宽（≥ 16mm），3mm 容差足以把两者分开。
    fn measured_grid(positions: &[(f64, f64)]) -> (usize, usize) {
        let xs: Vec<f64> = positions.iter().map(|p| p.0).collect();
        let ys: Vec<f64> = positions.iter().map(|p| p.1).collect();
        (lanes(&ys, 3.0), lanes(&xs, 3.0))
    }

    #[test]
    fn short_options_render_one_row_of_four() {
        let positions = option_positions(&choice_doc(4, [SHORT; 4]));
        assert_eq!(measured_grid(&positions), (1, 4), "{positions:?}");
        // 列序 = 选项序：A 在最左、D 在最右，靠栅格本身的排布而不是别的技巧
        let xs: Vec<f64> = positions.iter().map(|p| p.0).collect();
        assert!(
            xs.windows(2).all(|w| w[0] < w[1]),
            "四列未按 A→D 从左到右：{xs:?}"
        );
    }

    #[test]
    fn medium_options_render_two_by_two() {
        let positions = option_positions(&choice_doc(2, [MEDIUM; 4]));
        assert_eq!(measured_grid(&positions), (2, 2), "{positions:?}");
        // 行优先填充：A B 占第一行、C D 占第二行。typst grid 默认就是这样，但「先横后竖」
        // 是卷面语义（教师按 A B / C D 念答案），不能只靠默认行为不写断言。
        let same_row = |p: (f64, f64), q: (f64, f64)| (p.1 - q.1).abs() < 0.5;
        assert!(
            same_row(positions[0], positions[1]),
            "A B 不在同一行：{positions:?}"
        );
        assert!(
            same_row(positions[2], positions[3]),
            "C D 不在同一行：{positions:?}"
        );
        assert!(
            positions[0].1 < positions[2].1,
            "第一行不在第二行之上：{positions:?}"
        );
        assert!(positions[0].0 < positions[1].0, "第一行 A 不在 B 左边");
        assert!(positions[2].0 < positions[3].0, "第二行 C 不在 D 左边");
    }

    #[test]
    fn long_options_render_four_rows_of_one() {
        let positions = option_positions(&choice_doc(1, [LONG; 4]));
        assert_eq!(measured_grid(&positions), (4, 1), "{positions:?}");
    }

    #[test]
    fn measure_fallback_drops_a_column_when_rust_over_decided() {
        // R7：估宽判定 4 列、实测装不下 → 运行时降到 2 列，而不是把溢出留在纸上
        let by_two = option_positions(&choice_doc(4, [MEDIUM; 4]));
        assert_eq!(measured_grid(&by_two), (2, 2), "{by_two:?}");
        // 降到 2 列仍装不下 → 再降到 1 列（4 → 1 中间不必在 2 列上停留）
        let by_one = option_positions(&choice_doc(4, [LONG; 4]));
        assert_eq!(measured_grid(&by_one), (4, 1), "{by_one:?}");
        // 只有 2 列的判定没有「降 2」可降，直接落到 1 列
        let two_to_one = option_positions(&choice_doc(2, [LONG; 4]));
        assert_eq!(measured_grid(&two_to_one), (4, 1), "{two_to_one:?}");
    }

    #[test]
    fn dropping_columns_never_loses_option_text() {
        // 兜底只改版面，不改内容：同一个决定过 4 列与老实给 2 列，明文必须一字不差
        let wide = compile_pages(&choice_doc(4, [MEDIUM; 4]));
        let fitted = compile_pages(&choice_doc(2, [MEDIUM; 4]));
        assert_eq!(flat(&wide), flat(&fitted), "降列把选项文字改动了");
    }

    #[test]
    fn grid_source_wires_the_rust_decision_and_the_measure_ladder() {
        let doc = choice_doc(2, [MEDIUM; 4]);
        let s = generate(&doc, &HashMap::new()).source;
        // Rust 的列数决策忠实传进模板（改的是模板参数，不是模板里的常量）
        assert!(s.contains("#choices(2,"), "Rust 的列数决策没传进模板：{s}");
        // 兜底逻辑在函数库里：取实际栏宽 + 量自然宽 + 逐级降列
        for needle in [
            "#let choices(cols, ..cells)",
            "layout(size =>",
            "measure(cell).width",
            "let fits(c) = want <= (size.width - (c - 1) * gut) / c",
            "cols >= 4 and fits(2)",
        ] {
            assert!(s.contains(needle), "函数库缺 {needle}");
        }
    }

    /// R7 兜底的成本：整卷编译一次多少毫秒（预算 500ms/卷）
    ///
    /// `cargo test --lib typeset::typst_gen::tests::cost_of_the_measure_fallback -- --ignored --nocapture`
    ///
    /// 对照组把所有题判成单列 —— 单列走 `cols <= 1` 分支，一个单元格都不量。两组的版面长度不同
    /// （单列多占行），所以把页数一起打出来，别把版面差异读成兜底的开销。
    #[test]
    #[ignore]
    fn cost_of_the_measure_fallback() {
        use std::time::{Duration, Instant};

        let compile = |doc: &LayoutDoc| -> (Duration, usize) {
            let generated = generate(doc, &HashMap::new());
            let dirs = font_dirs();
            let started = Instant::now();
            let out = compile_paged(&request(&generated.source, &dirs, &[]))
                .unwrap_or_else(|e| panic!("{}", e.summary()));
            (started.elapsed(), out.output.pages().len())
        };

        let measured = sim_doc(LayoutSpec::preset("a4_practice").unwrap());
        let mut unmeasured = sim_doc(LayoutSpec::preset("a4_practice").unwrap());
        for block in unmeasured.sections.iter_mut().flat_map(|s| &mut s.blocks) {
            if let LayoutBlock::Question(q) = block {
                q.grid.columns = 1;
                q.grid.rows = q.options.len().max(1);
            }
        }

        // 第一次编译付掉字体池解析（进程级记忆化），之后才是逐请求成本
        let best = |doc: &LayoutDoc| -> (Duration, usize) {
            let warm = compile(doc);
            (0..3)
                .map(|_| compile(doc))
                .fold(warm, |a, b| if b.0 < a.0 { b } else { a })
        };
        let (with_measure, pages_m) = best(&measured);
        let (skip, pages_s) = best(&unmeasured);
        println!(
            "兜底开：{}ms / {pages_m} 页；全单列（不量）：{}ms / {pages_s} 页；差 {}ms",
            with_measure.as_millis(),
            skip.as_millis(),
            with_measure.as_millis() as i64 - skip.as_millis() as i64
        );
    }

    // ───────────────────────────────────────────────────── T4.3 左文右图

    /// 1×1 像素 PNG。帧树里的 `FrameItem::Image` 只认**栅格**图，SVG 会被转成矢量 group
    /// （实测），所以「图到底画成了多宽」这类断言必须喂 PNG，不能沿用 `DOT_SVG`
    fn dot_png() -> Vec<u8> {
        use ::image::ExtendedColorType;
        use ::image::ImageEncoder as _;
        let mut buf = Vec::new();
        ::image::codecs::png::PngEncoder::new(&mut buf)
            .write_image(&[255, 0, 0, 255], 1, 1, ExtendedColorType::Rgba8)
            .expect("1×1 PNG 编码不会失败");
        buf
    }

    /// 题干尾部那一枚配图（`px` = 编辑器口径的像素宽）
    fn figure_px(px: u32) -> InlineNode {
        InlineNode::Image {
            alt: None,
            url: REMOTE.into(),
            width: Some(px),
            align: None,
        }
    }

    /// 一道配图题。`figure` 由 `figure_float::plan` **现场判定**，口径与
    /// `blocks::question_block` 一致：整栏宽，不是扣掉悬挂缩进的 `available_em`
    fn figure_doc(stem: Vec<InlineNode>, options: usize) -> LayoutDoc {
        let mut doc = sim_doc(LayoutSpec::for_profile(OutputProfile::Student));
        doc.answer_key.clear();
        doc.sections.truncate(1);
        doc.sections[0].header.instruction = None;
        doc.sections[0].header.question_count = 1;
        doc.spec.header_footer.page_number = false;
        let column_em = choice_grid::em_from_mm(f64::from(doc.spec.column_width_mm()));
        let figure = figure_float::plan(&stem, column_em);
        let grid = choice_grid::ChoiceGrid {
            columns: options.max(1),
            rows: options.div_ceil(options.max(1)),
        };
        doc.sections[0].blocks = vec![LayoutBlock::Question(QuestionBlock {
            meta: BlockMeta::flow(),
            number: 1,
            score: 5.0,
            kind: QuestionKind::SingleChoice,
            stem,
            options: ('A'..='D')
                .take(options)
                .map(|label| ExamOption {
                    label: label.to_string(),
                    content: vec![text(SHORT)],
                })
                .collect(),
            grid,
            figure,
        })];
        doc
    }

    /// 编译一道配图题：回读画出来的图片与按页分组的文字段。
    ///
    /// `width:` 参数只是我方的意图，`FrameItem::Image` 里的 `Size` 才是纸上的事实。
    fn compile_figure(doc: &LayoutDoc) -> (Vec<PlacedImage>, Vec<Vec<PlacedRun>>) {
        let path = "/ext/0.png".to_string();
        let generated = generate(doc, &HashMap::from([(REMOTE.to_string(), Some(path))]));
        let dirs = font_dirs();
        let injected = vec![("/ext/0.png".to_string(), dot_png())];
        let req = request(&generated.source, &dirs, &injected);
        let out = match compile_paged(&req) {
            Ok(out) => out,
            Err(err) => panic!(
                "编译失败：{:?}\n---- 源码 ----\n{}",
                err.diagnostics, generated.source
            ),
        };
        let images: Vec<PlacedImage> = placed_images(&out.output).into_iter().flatten().collect();
        assert_eq!(images.len(), 1, "版面上应该恰好一张配图：{images:?}");
        (images, placed_pages(&out.output))
    }

    /// 版面上那张图画出来的（宽, 左边界），毫米
    fn drawn_figure(doc: &LayoutDoc) -> (f64, f64) {
        let (images, _) = compile_figure(doc);
        (images[0].w_mm, images[0].x_mm)
    }

    /// 这道题在 IR 里判成浮动了没有
    fn floated(doc: &LayoutDoc) -> bool {
        let LayoutBlock::Question(q) = &doc.sections[0].blocks[0] else {
            unreachable!()
        };
        q.figure.is_some()
    }

    /// 长题干在版面上实际占了几行（每行一枚文字段）
    fn wrapped_lines(pages: &[Vec<PlacedRun>]) -> usize {
        pages
            .iter()
            .flatten()
            .filter(|r| !r.run.text.is_empty() && r.run.text.chars().all(|c| STEM.contains(c)))
            .count()
    }

    /// a4_practice 的图列宽（毫米）：86mm 栏 × 35%
    fn figure_cell_mm() -> f64 {
        f64::from(LayoutSpec::for_profile(OutputProfile::Student).column_width_mm()) * FIGURE_SHARE
    }

    /// 只由这十个汉字组成的长题干：折行成几行都能靠字符集认出来
    const STEM: &str = "甲乙丙丁戊己庚辛壬癸";

    #[test]
    fn figure_column_width_is_the_same_for_short_and_long_text() {
        // 验收口径「图列宽度恒定不失宽」：左栏从一行写到十行，右栏那枚图的宽度不许动
        let cell = figure_cell_mm();
        let short = figure_doc(vec![text("如图，求阴影部分面积。"), figure_px(90)], 0);
        let long = figure_doc(vec![text(&STEM.repeat(12)), figure_px(90)], 0);
        assert!(
            floated(&short) && floated(&long),
            "两份都该浮动，否则宽度相等是废话"
        );
        let (short_text, _) = compile_figure(&short);
        let (long_text, long_pages) = compile_figure(&long);
        assert!(
            (short_text[0].w_mm - long_text[0].w_mm).abs() < 0.2,
            "图宽随文字长短漂了：短文 {:?} vs 长文 {:?}",
            short_text[0],
            long_text[0]
        );
        assert!(
            (short_text[0].x_mm - long_text[0].x_mm).abs() < 0.2,
            "图的位置随文字长短漂了：短文 {:?} vs 长文 {:?}",
            short_text[0],
            long_text[0]
        );
        assert!(
            long_text[0].w_mm < cell,
            "图宽 {:.1}mm 超过了 {:.1}mm 的图列",
            long_text[0].w_mm,
            cell
        );
        assert!(
            long_text[0].w_mm > 20.0,
            "90px 的图只画了 {:.1}mm",
            long_text[0].w_mm
        );
        // 左栏真的被 65% 挤窄了：同一段字，浮动比通栏多占行。少了这一条，
        // 「栅格根本没生效」也能通过上面所有断言。
        let flowed = {
            let mut doc = long.clone();
            let LayoutBlock::Question(q) = &mut doc.sections[0].blocks[0] else {
                unreachable!()
            };
            q.figure = None;
            doc
        };
        let (_, flowed_pages) = compile_figure(&flowed);
        assert!(
            wrapped_lines(&long_pages) > wrapped_lines(&flowed_pages),
            "左栏没比整栏窄：浮动 {} 行 vs 通栏 {} 行",
            wrapped_lines(&long_pages),
            wrapped_lines(&flowed_pages)
        );
    }

    #[test]
    fn floated_figure_sits_right_of_the_text_and_inside_the_column() {
        let (w, x) = drawn_figure(&figure_doc(
            vec![text("如图，求阴影部分面积。"), figure_px(90)],
            4,
        ));
        // 左边距 15mm + 半栏 43mm：图必须在右半栏里，选项栅格才有它自己的位置
        assert!(x > 58.0, "图没排到右栏：x = {x:.1}mm");
        assert!(
            x + w < 101.5,
            "图溢出栏外：右边界 {:.1}mm（栏右边界 101mm）",
            x + w
        );
    }

    #[test]
    fn wide_figure_is_left_in_the_flow() {
        // 200px ≈ 53mm，装不进 30mm 的图列 → 照旧独占整行，不许硬塞
        let doc = figure_doc(vec![text("如图，求阴影部分面积。"), figure_px(200)], 0);
        assert!(!floated(&doc), "判定阶段就不该放行这张宽图");
        let s = generate(&doc, &images()).source;
        assert!(!s.contains("#figure-float("), "宽图不该浮动：{s}");
        assert!(s.contains("#image("), "宽图仍然要画出来：{s}");
    }

    #[test]
    fn float_source_wires_the_tracks_and_the_options() {
        let doc = figure_doc(vec![text("如图，求阴影部分面积。"), figure_px(90)], 4);
        let s = generate(&doc, &images()).source;
        // 图列比例只有一个来源：Rust 侧的 FIGURE_SHARE
        assert!(
            s.contains(&format!("columns: (1fr, {}%)", FIGURE_SHARE * 100.0)),
            "模板里的图列比例与 FIGURE_SHARE 不一致：{s}"
        );
        for needle in ["#figure-float(", "\", [#image(", "rest: [#choices(4,"] {
            assert!(s.contains(needle), "函数库缺 {needle}：{s}");
        }
    }

    #[test]
    fn floating_rearranges_the_paper_without_changing_its_text() {
        let stem = vec![
            text("如图，甲、乙两人在同一直线上相向而行。"),
            InlineNode::LineBreak,
            figure_px(90),
        ];
        let mut doc = figure_doc(stem, 0);
        let floated = flat(&compile_pages(&doc));
        if let LayoutBlock::Question(q) = &mut doc.sections[0].blocks[0] {
            q.figure = None;
        }
        let flowed = flat(&compile_pages(&doc));
        assert_eq!(floated, flowed, "浮动把卷面文字改动了");
    }
}
