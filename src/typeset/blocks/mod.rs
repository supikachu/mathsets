//! 题型模板注册表（§6.2、T4.1）：`QuestionKind → BlockBuilder`
//!
//! M3 之前「这道题该出哪些块」散在适配器的 match 里：`question_meta` 判题干能不能整块不跨页、
//! `is_written` 判要不要展开问树和垫留白。题型一加，每一处都得跟着改，漏改不会编译报错、只会在
//! 卷面上少一块。注册表把题型差异收成一个对象，适配器只剩一行查表。
//!
//! ## 边界：builder 只管题型差异
//!
//! 题型之间的差异只有三条 [`Policy`]：题干短不短（能不能整块不跨页）、展不展开问树、有没有作答
//! 留白。**模式差异**（四类 Callout、内嵌答案 vs 卷末答案）留在 [`crate::export::pdf`] —— 那是
//! `options` 与 `profile` 的函数，与题型无关。所以 builder 产出的就是「题干 → 小问 → 留白」这段
//! 题面块序列，后面的 callout / answer 由适配器续上。
//!
//! ## 新增题型
//!
//! 实现一个 builder + 一行注册，`ir` 与 `typst_gen` 一行不改。`QuestionKind` 本身是封闭枚举
//! （内容与 wire 共用的词汇表），加变体仍要改它 —— 那是数据侧的一次改动，不再是排版侧的 N 处。
//! 没注册的题型走 [`Registry::builder`] 的兜底（按书面作答处理）：宁可多给一块留白，也不静默
//! 丢内容。
//!
//! ## 依赖方向
//!
//! 本模块不 import `export::content`：问树小问的文本切分器由调用方作为参数传进来（生产环境是
//! [`crate::export::content::split_content`]），否则排版系统就反向依赖了导出引擎的生成器。
//! `QuestionPart` 随 `ExamQuestion` 进来 —— 它是 models 里的纯数据类型，不带装配逻辑。

pub mod blank;
pub mod choice_grid;
pub mod figure_float;

use crate::export::model::{ExamOption, ExamQuestion, ExportOptions, InlineNode, QuestionKind};
use crate::models::question_structure::QuestionPart;
use crate::typeset::blocks::choice_grid::{ChoiceGrid, decide, requires_single_column};
use crate::typeset::ir::{BlockMeta, LayoutBlock, QuestionBlock, SubQuestionBlock};
use crate::typeset::spec::{LayoutSpec, OutputProfile};

/// 文本切分器：原始字符串 → InlineNode 序列（见模块头「依赖方向」）
pub type Splitter = dyn Fn(&str) -> Vec<InlineNode>;

/// 出块策略：题型之间唯三的差异
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// 展开问树（解答题 / 综合题的 `(1)(2)` 小问）
    pub expands_parts: bool,
    /// 需要作答留白（讲义模式另外排除，见 [`blank::plan`]）
    pub wants_blank: bool,
    /// 题干短到可以整块不跨页（还要求题干与选项里都没有表格 / 块级公式）
    pub compact_stem: bool,
}

impl Default for Policy {
    /// 一条都不开：只出题干块
    fn default() -> Self {
        Self {
            expands_parts: false,
            wants_blank: false,
            compact_stem: false,
        }
    }
}

/// 出块需要的版面上下文
pub struct BlockCtx<'a> {
    pub options: &'a ExportOptions,
    pub spec: &'a LayoutSpec,
    pub profile: OutputProfile,
    /// 选项栅格的可用栏宽（em，已扣掉题号悬挂缩进）
    pub available_em: f64,
    pub registry: &'a Registry,
}

/// 一题的出块模板
pub trait BlockBuilder {
    /// 本 builder 负责的题型
    fn kinds(&self) -> &'static [QuestionKind];
    fn policy(&self) -> Policy {
        Policy::default()
    }
    /// 默认实现按 [`Policy`] 走通用序列；需要完全不同版面的题型可整体接管
    fn build(&self, q: &ExamQuestion, ctx: &BlockCtx, split: &Splitter) -> Vec<LayoutBlock> {
        let policy = self.policy();
        let mut out = Vec::new();
        out.push(LayoutBlock::Question(question_block(q, ctx, &policy)));
        if policy.expands_parts {
            push_parts(&q.structure_parts, 0, q.number, split, &mut out);
        }
        if let Some(blank) = blank::plan(q, ctx, &policy) {
            out.push(LayoutBlock::Blank(blank));
        }
        out
    }
}

// ═══════════════════════════ 五个内置 builder ═══════════════════════════

/// 单选题：题干 + 选项栅格，短到可整块不跨页
pub struct ChoiceBuilder;
/// 多选题：版面上与单选同构（差异在答案行，不在块序列），分开注册是给后续各自演进留位
pub struct MultipleChoiceBuilder;
/// 填空题：作答位是题干里的行内下划线（B2 已由装配器挖好），不垫留白
pub struct FillBuilder;
/// 解答题：问树逐小问出块 + 作答留白
pub struct SolutionBuilder;
/// 综合题（前端 bucketType 第 5 桶）：与解答题同构
pub struct CompositeBuilder;

const SHORT: Policy = Policy {
    expands_parts: false,
    wants_blank: false,
    compact_stem: true,
};
const WRITTEN: Policy = Policy {
    expands_parts: true,
    wants_blank: true,
    compact_stem: false,
};

impl BlockBuilder for ChoiceBuilder {
    fn kinds(&self) -> &'static [QuestionKind] {
        &[QuestionKind::SingleChoice]
    }
    fn policy(&self) -> Policy {
        SHORT
    }
}

impl BlockBuilder for MultipleChoiceBuilder {
    fn kinds(&self) -> &'static [QuestionKind] {
        &[QuestionKind::MultiChoice]
    }
    fn policy(&self) -> Policy {
        SHORT
    }
}

impl BlockBuilder for FillBuilder {
    fn kinds(&self) -> &'static [QuestionKind] {
        &[QuestionKind::Fill]
    }
    fn policy(&self) -> Policy {
        SHORT
    }
}

impl BlockBuilder for SolutionBuilder {
    fn kinds(&self) -> &'static [QuestionKind] {
        &[QuestionKind::Solution]
    }
    fn policy(&self) -> Policy {
        WRITTEN
    }
}

impl BlockBuilder for CompositeBuilder {
    fn kinds(&self) -> &'static [QuestionKind] {
        &[QuestionKind::Composite]
    }
    fn policy(&self) -> Policy {
        WRITTEN
    }
}

/// 未注册题型的兜底：按书面作答出块（展开问树、给留白），任何题型至少不丢题干
pub struct FallbackBuilder;

impl BlockBuilder for FallbackBuilder {
    fn kinds(&self) -> &'static [QuestionKind] {
        &[]
    }
    fn policy(&self) -> Policy {
        WRITTEN
    }
}

static FALLBACK: FallbackBuilder = FallbackBuilder;

// ═══════════════════════════ 注册表 ═══════════════════════════

/// `QuestionKind → BlockBuilder` 的查找表
///
/// 表是 `Vec` 不是 `HashMap`：五到十几条的规模下线性扫不比哈希慢，而且查找必须**从后往前**
/// （[`Registry::register`] 后注册者覆盖），测试与未来的题型扩展才能力挽内置表。
#[derive(Clone, Default)]
pub struct Registry {
    builders: Vec<&'static dyn BlockBuilder>,
}

impl Registry {
    pub const fn new() -> Self {
        Self {
            builders: Vec::new(),
        }
    }

    /// 五种内置题型
    pub fn standard() -> Self {
        Self::new()
            .register(&ChoiceBuilder)
            .register(&MultipleChoiceBuilder)
            .register(&FillBuilder)
            .register(&SolutionBuilder)
            .register(&CompositeBuilder)
    }

    /// 链式登记；同一题型后注册者胜出
    pub fn register(mut self, builder: &'static dyn BlockBuilder) -> Self {
        self.builders.push(builder);
        self
    }

    pub fn builder(&self, kind: QuestionKind) -> &'static dyn BlockBuilder {
        self.builders
            .iter()
            .rev()
            .find(|b| b.kinds().contains(&kind))
            .copied()
            .unwrap_or(&FALLBACK)
    }

    /// 该题型是否有专属模板（false = 走兜底）
    pub fn is_registered(&self, kind: QuestionKind) -> bool {
        self.builders.iter().any(|b| b.kinds().contains(&kind))
    }

    /// 单题 → 题面块序列（题干 → 小问 → 留白）
    pub fn expand(&self, q: &ExamQuestion, ctx: &BlockCtx, split: &Splitter) -> Vec<LayoutBlock> {
        self.builder(q.kind).build(q, ctx, split)
    }
}

// ═══════════════════════════ 通用出块件 ═══════════════════════════

fn question_block(q: &ExamQuestion, ctx: &BlockCtx, policy: &Policy) -> QuestionBlock {
    QuestionBlock {
        meta: stem_meta(q, policy),
        number: q.number,
        score: q.score,
        kind: q.kind,
        stem: q.stem.clone(),
        options: q.options.clone(),
        grid: decide(&q.options, ctx.available_em),
        // 图列在 `item` 的悬挂缩进之外，所以按整栏宽判（与 available_em 差一个 indent）
        figure: figure_float::plan(
            &q.stem,
            choice_grid::em_from_mm(f64::from(ctx.spec.column_width_mm())),
        ),
    }
}

/// 题干块的分页元数据
///
/// `keep_with_next` 恒真：题干与它下面的选项/小问/留白必须同页起头。
/// `breakable` 只在「短小题」关掉 —— `compact_stem` 说这个题型可能短，
/// [`requires_single_column`] 再否决掉含表格 / 块级公式 / 显式换行的那些：这类题干动辄半页，
/// 强行不跨页只会在 typst 里溢出一页纸。
fn stem_meta(q: &ExamQuestion, policy: &Policy) -> BlockMeta {
    let compact = policy.compact_stem
        && !requires_single_column(&q.stem)
        && !q.options.iter().any(|o| requires_single_column(&o.content));
    if compact {
        BlockMeta::glued()
    } else {
        BlockMeta::attach()
    }
}

/// 问树展开（含分支节点：只存局部题干的那层也要排出来）
fn push_parts(
    parts: &[QuestionPart],
    depth: u8,
    number: u32,
    split: &Splitter,
    out: &mut Vec<LayoutBlock>,
) {
    for p in parts {
        push_part(p, depth, number, split, out);
    }
}

fn push_part(
    p: &QuestionPart,
    depth: u8,
    number: u32,
    split: &Splitter,
    out: &mut Vec<LayoutBlock>,
) {
    let nodes = split(&p.stem);
    // 题干为空的分支节点：不占版面，只把子节点抬上来
    if !nodes.is_empty() {
        out.push(LayoutBlock::SubQuestion(SubQuestionBlock {
            meta: BlockMeta::attach(),
            number,
            depth,
            label: p.label.clone(),
            stem: nodes,
        }));
    }
    for c in &p.children {
        push_part(c, depth.saturating_add(1), number, split, out);
    }
}

/// 选项栅格（docx `w:tbl` 与 typst `grid()` 共用的那一次判定）
pub fn choice_grid(options: &[ExamOption], available_em: f64) -> ChoiceGrid {
    decide(options, available_em)
}

// ═══════════════════════════════ 测试 ═══════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::model::{AnswerSpace, BlankStyle as WireBlankStyle};
    use crate::typeset::spec::{BlankStyle, LayoutSpec};

    fn split(s: &str) -> Vec<InlineNode> {
        vec![InlineNode::Text { text: s.into() }]
    }

    fn option(label: &str, content: &str) -> ExamOption {
        ExamOption {
            label: label.into(),
            content: split(content),
        }
    }

    fn q(kind: QuestionKind, options: Vec<ExamOption>) -> ExamQuestion {
        ExamQuestion {
            number: 7,
            score: 6.0,
            kind,
            stem: split("题干正文"),
            options,
            answers: vec!["B".into()],
            analyses: Vec::new(),
            structure_parts: Vec::new(),
            callouts: Vec::new(),
            answer_space: None,
            issues: Vec::new(),
        }
    }

    fn part(label: &str, stem: &str, children: Vec<QuestionPart>) -> QuestionPart {
        QuestionPart {
            id: label.into(),
            label: label.into(),
            stem: stem.into(),
            children,
            answer: Some("答案".into()),
            analyses: Vec::new(),
            no_analysis_needed: false,
            label_dirty: false,
        }
    }

    fn kinds_of(blocks: &[LayoutBlock]) -> Vec<&'static str> {
        blocks
            .iter()
            .map(|b| match b {
                LayoutBlock::Question(_) => "question",
                LayoutBlock::SubQuestion(_) => "sub",
                LayoutBlock::Callout(_) => "callout",
                LayoutBlock::Blank(_) => "blank",
                LayoutBlock::Answer(_) => "answer",
            })
            .collect()
    }

    fn stem_of<'a>(blocks: &'a [LayoutBlock]) -> &'a QuestionBlock {
        match &blocks[0] {
            LayoutBlock::Question(b) => b,
            other => panic!("首块应是题干块，实为 {other:?}"),
        }
    }

    /// 留白只在调用方要了作答区时才出（B5），默认 options 里它是 `None`
    fn space_options() -> ExportOptions {
        ExportOptions {
            answer_space: Some(AnswerSpace {
                height_cm: 6.0,
                style: WireBlankStyle::Lines,
            }),
            ..ExportOptions::default()
        }
    }

    fn block_ctx<'a>(
        options: &'a ExportOptions,
        spec: &'a LayoutSpec,
        registry: &'a Registry,
        profile: OutputProfile,
    ) -> BlockCtx<'a> {
        BlockCtx {
            options,
            spec,
            profile,
            available_em: 30.0,
            registry,
        }
    }

    #[test]
    fn standard_registry_covers_every_kind() {
        // 穷尽 match：QuestionKind 加变体时这里编译不过，逼着补 builder
        let all = [
            QuestionKind::SingleChoice,
            QuestionKind::MultiChoice,
            QuestionKind::Fill,
            QuestionKind::Solution,
            QuestionKind::Composite,
        ];
        let reg = Registry::standard();
        for k in all {
            match k {
                QuestionKind::SingleChoice
                | QuestionKind::MultiChoice
                | QuestionKind::Fill
                | QuestionKind::Solution
                | QuestionKind::Composite => {}
            }
            assert!(reg.is_registered(k), "{k:?} 没有专属模板，会静默走兜底");
            assert_eq!(
                reg.builder(k).kinds().first(),
                Some(&k),
                "{k:?} 命中的 builder 不负责它"
            );
        }
    }

    #[test]
    fn short_kinds_emit_a_single_glued_stem() {
        let options = space_options();
        let spec = LayoutSpec::default();
        let reg = Registry::standard();
        let ctx = block_ctx(&options, &spec, &reg, OutputProfile::Student);
        for kind in [
            QuestionKind::SingleChoice,
            QuestionKind::MultiChoice,
            QuestionKind::Fill,
        ] {
            let with_options = q(
                kind,
                vec![
                    option("A", "1"),
                    option("B", "2"),
                    option("C", "3"),
                    option("D", "4"),
                ],
            );
            let blocks = reg.expand(&with_options, &ctx, &split);
            assert_eq!(kinds_of(&blocks), ["question"], "{kind:?}");
            let b = stem_of(&blocks);
            assert!(!b.meta.breakable, "{kind:?} 短题干应整块不跨页");
            assert_eq!((b.grid.columns, b.grid.rows), (4, 1), "{kind:?} 栅格决策");
            assert_eq!(b.number, 7);
            assert_eq!(b.score, 6.0);
        }
    }

    #[test]
    fn solution_expands_parts_then_blank() {
        let options = space_options();
        let spec = LayoutSpec::default();
        let reg = Registry::standard();
        let ctx = block_ctx(&options, &spec, &reg, OutputProfile::Student);
        let mut qn = q(QuestionKind::Solution, Vec::new());
        qn.structure_parts = vec![
            part("(1)", "第一问", vec![part("①", "小问", Vec::new())]),
            part("(2)", "第二问", Vec::new()),
        ];
        let blocks = reg.expand(&qn, &ctx, &split);
        assert_eq!(
            kinds_of(&blocks),
            ["question", "sub", "sub", "sub", "blank"]
        );
        assert!(stem_of(&blocks).meta.breakable, "解答题题干不该关掉跨页");
        let (a, b) = (&blocks[1], &blocks[2]);
        let (LayoutBlock::SubQuestion(x), LayoutBlock::SubQuestion(y)) = (a, b) else {
            unreachable!()
        };
        assert_eq!((x.depth, y.depth), (0, 1), "嵌套层级要落进块里");
        let LayoutBlock::Blank(blank) = blocks.last().unwrap() else {
            unreachable!()
        };
        assert!(!blank.meta.breakable, "留白不许切成半页横线");
    }

    #[test]
    fn teacher_profile_omits_blank_but_keeps_parts() {
        let options = space_options();
        let spec = LayoutSpec::default();
        let reg = Registry::standard();
        let ctx = block_ctx(&options, &spec, &reg, OutputProfile::Teacher);
        let mut qn = q(QuestionKind::Composite, Vec::new());
        qn.structure_parts = vec![part("(1)", "第一问", Vec::new())];
        assert_eq!(
            kinds_of(&reg.expand(&qn, &ctx, &split)),
            ["question", "sub"],
            "讲义模式的答案就是解析，不垫空白"
        );
    }

    fn image(px: u32) -> InlineNode {
        InlineNode::Image {
            alt: None,
            url: "/uploads/a.png".into(),
            width: Some(px),
            align: None,
        }
    }

    #[test]
    fn question_block_plans_the_figure_against_the_whole_column() {
        // T4.3 的接线处。判据吃的是**整栏宽**，不是选项栅格那套 `available_em`：
        // 图格在 `item` 的悬挂缩进之外、享整栏 174mm ⇒ 图列 60.9mm，200px ≈ 52.9mm 放行；
        // 换成 ctx.available_em（30em ≈ 111mm）就只有 38.9mm，同一张图会被否决。
        // 所以「这一张浮起了」本身就是口径证据，而不只是「plan 被调过」。
        let options = ExportOptions::default();
        let spec = LayoutSpec::default();
        let reg = Registry::standard();
        let ctx = block_ctx(&options, &spec, &reg, OutputProfile::Student);
        let mut qn = q(
            QuestionKind::SingleChoice,
            vec![option("A", "1"), option("B", "2")],
        );
        qn.stem = vec![
            InlineNode::Text {
                text: "如图，".into(),
            },
            image(200),
        ];
        assert_eq!(
            stem_of(&reg.expand(&qn, &ctx, &split)).figure,
            Some(figure_float::Split {
                text_end: 1,
                figure_start: 1,
            })
        );
        // 400px ≈ 105mm 连整栏的图列都装不下 → 照旧独占整行
        qn.stem = vec![
            InlineNode::Text {
                text: "如图，".into(),
            },
            image(400),
        ];
        assert_eq!(stem_of(&reg.expand(&qn, &ctx, &split)).figure, None);
    }

    #[test]
    fn unregistered_kind_falls_back_without_losing_content() {
        // 兜底不是摆设：注册表为空 = 「新题型刚进枚举、还没注册」，
        // 此时仍应出题干 + 小问 + 留白，而不是静默出一张空白卷
        let options = space_options();
        let spec = LayoutSpec::default();
        let empty = Registry::new();
        let ctx = block_ctx(&options, &spec, &empty, OutputProfile::Student);
        let mut qn = q(QuestionKind::Solution, Vec::new());
        qn.structure_parts = vec![part("(1)", "第一问", Vec::new())];
        assert_eq!(
            kinds_of(&empty.expand(&qn, &ctx, &split)),
            ["question", "sub", "blank"]
        );
        assert!(!empty.is_registered(QuestionKind::Solution));
    }

    #[test]
    fn registering_a_builder_is_enough_to_take_over_a_kind() {
        // T4.1 的扩展性口径：新模板 = 一个 builder 实现 + 一行注册，
        // ir / typst_gen / 适配器分派处一行不改
        struct ProveBuilder;
        impl BlockBuilder for ProveBuilder {
            fn kinds(&self) -> &'static [QuestionKind] {
                &[QuestionKind::Solution]
            }
            fn build(
                &self,
                q: &ExamQuestion,
                ctx: &BlockCtx,
                _split: &Splitter,
            ) -> Vec<LayoutBlock> {
                // 证明题自己的版面：题干整块不跨页，其后直接一块大留白，不排小问
                let mut out = Vec::new();
                out.push(LayoutBlock::Question(QuestionBlock {
                    meta: BlockMeta::solid(),
                    number: q.number,
                    score: q.score,
                    kind: q.kind,
                    stem: q.stem.clone(),
                    options: Vec::new(),
                    grid: ChoiceGrid {
                        columns: 1,
                        rows: 0,
                    },
                    figure: None,
                }));
                let policy = Policy {
                    wants_blank: true,
                    ..Policy::default()
                };
                if let Some(b) = blank::plan(q, ctx, &policy) {
                    out.push(LayoutBlock::Blank(b));
                }
                out
            }
        }
        static PROVE: ProveBuilder = ProveBuilder;

        let options = space_options();
        let spec = LayoutSpec::default();
        let reg = Registry::standard().register(&PROVE);
        let ctx = block_ctx(&options, &spec, &reg, OutputProfile::Student);
        let mut qn = q(QuestionKind::Solution, Vec::new());
        qn.structure_parts = vec![part("(1)", "第一问", Vec::new())];
        assert_eq!(
            kinds_of(&reg.expand(&qn, &ctx, &split)),
            ["question", "blank"],
            "后注册的模板应接管该题型"
        );
        let meta = stem_of(&reg.expand(&qn, &ctx, &split)).meta;
        assert_eq!((meta.breakable, meta.keep_with_next), (false, false));
        // 其余题型不受影响
        let fill = q(QuestionKind::Fill, Vec::new());
        assert_eq!(kinds_of(&reg.expand(&fill, &ctx, &split)), ["question"]);
    }

    #[test]
    fn long_stem_never_gets_glued_even_for_short_kinds() {
        let options = space_options();
        let spec = LayoutSpec::default();
        let reg = Registry::standard();
        let ctx = block_ctx(&options, &spec, &reg, OutputProfile::Student);
        let mut qn = q(
            QuestionKind::SingleChoice,
            vec![option("A", "1"), option("B", "2")],
        );
        qn.stem = vec![InlineNode::Math {
            latex: "x^2".into(),
            display: true,
        }];
        let blocks = reg.expand(&qn, &ctx, &split);
        let b = stem_of(&blocks);
        assert!(b.meta.breakable, "含块级公式的题干不该关掉跨页");
        assert_eq!((b.grid.columns, b.grid.rows), (2, 1), "两个短选项排 2 列");
    }

    #[test]
    fn per_question_answer_space_overrides_request_level() {
        let options = ExportOptions {
            answer_space: Some(AnswerSpace {
                height_cm: 4.0,
                style: WireBlankStyle::Dots,
            }),
            ..ExportOptions::default()
        };
        let spec = LayoutSpec::default();
        let reg = Registry::standard();
        let ctx = block_ctx(&options, &spec, &reg, OutputProfile::Student);
        let mut qn = q(QuestionKind::Solution, Vec::new());
        qn.answer_space = Some(AnswerSpace {
            height_cm: 10.0,
            style: WireBlankStyle::Blank,
        });
        let blocks = reg.expand(&qn, &ctx, &split);
        let LayoutBlock::Blank(blank) = blocks.last().unwrap() else {
            unreachable!()
        };
        assert_eq!(
            (blank.height_mm, blank.style),
            (100.0, BlankStyle::Blank),
            "题级覆盖应压过请求级"
        );
    }
}
