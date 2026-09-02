//! 排版域 IR（LayoutDoc / LayoutBlock）— 任务 T3.3，实施计划 §6.2
//!
//! 两层 IR 的第二层：`ExamBundle` 讲「这道题有什么内容」，本模块讲「这一页该怎么排」。
//! 唯一的生产者适配器是 [`crate::export::pdf`]（模块间唯一接口），唯一的消费者是
//! `typst_gen`（M3）与后续 block builder（M4）。
//!
//! ## 三条不变式
//!
//! 1. **不留原始文本**：IR 里的文本一律是已切分的 `Vec<InlineNode>`（问树 `stem`、
//!    解析 `content` 都在适配器里过 `split_content`）。渲染器不需要懂 Markdown/LaTeX 定界符。
//! 2. **分页元数据在每个块上**：[`BlockMeta`] 的 `breakable` / `keep_with_next` 是 docx
//!    `keepNext` / typst `#block(breakable:)` 的共同上游，跨格式同一套判定。
//!    §6.2 里的 `keep_together` 不单独设字段 —— 它就是 `breakable: false`，两个旋钮管
//!    同一件事必然漂移。
//! 3. **块序列是线性的**：一道题可以展开成「题干块 + 若干小问块 + Callout 块 + 留白块」，
//!    渲染器顺序走一遍即可，不需要递归下降题型结构。
//!
//! ## 依赖方向
//!
//! typeset 不碰导出引擎的装配器、生成器与 handler；只用 `export::model` 里的**内容词汇表**
//! （`InlineNode` / `ExamOption` / `Callout` / `QuestionKind` / `Issue` 这些纯数据类型，
//! [`crate::typeset::blocks::choice_grid`] 从 T2.5 起就吃它们）。因此排版系统仍可被别的来源
//! 复用：任何来源只要能造出 `InlineNode`，就能拼出一棵 LayoutDoc。
//!
//! 本模块不过 HTTP 边界（`/export/pdf` 传的是 `ExamRequest`），故不导 TS 绑定。

use crate::export::model::{Callout, ExamOption, InlineNode, Issue, QuestionKind};
use crate::typeset::blocks::choice_grid::ChoiceGrid;
use crate::typeset::spec::{BlankStyle, LayoutSpec, OutputProfile, ResolvedBlank};

/// 分页元数据（§6.2：每种块都带）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockMeta {
    /// false → typst `#block(breakable: false)` / docx `keepLines`：整块不许跨页
    pub breakable: bool,
    /// true → 与下一块同页：typst `#block(above: ..)` 前置粘连 / docx `keepNext`
    pub keep_with_next: bool,
}

impl Default for BlockMeta {
    fn default() -> Self {
        Self::flow()
    }
}

impl BlockMeta {
    /// 可跨页、不与后块粘连（长段落、解析正文）
    pub const fn flow() -> Self {
        Self {
            breakable: true,
            keep_with_next: false,
        }
    }
    /// 可跨页，但必须与下一块同页（大题标题、小问题干 → 留白）
    pub const fn attach() -> Self {
        Self {
            breakable: true,
            keep_with_next: true,
        }
    }
    /// 整块不跨页且与下一块粘连（短小题干 → 选项栅格）
    pub const fn glued() -> Self {
        Self {
            breakable: false,
            keep_with_next: true,
        }
    }
    /// 整块不跨页、后面独立（留白区：不许切成半页横线）
    pub const fn solid() -> Self {
        Self {
            breakable: false,
            keep_with_next: false,
        }
    }
}

// ═══════════════════════════════ 文档级 ═══════════════════════════════

/// 一份可排版的文档（typst_gen 的唯一输入）
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutDoc {
    pub title: String,
    pub subtitle: Option<String>,
    /// 卷头元信息（首页大卷头 + 页眉页脚共用）
    pub meta: DocumentMeta,
    /// 输出口径（由 `ExportMode` 翻译而来）
    pub profile: OutputProfile,
    /// 已定稿的版面参数（mode 默认或请求覆盖 + profile 回填）
    pub spec: LayoutSpec,
    pub sections: Vec<Section>,
    /// 卷末答案（`options.answer_at_end`）；内嵌答案以 [`LayoutBlock::Answer`] 混在 blocks 里
    pub answer_key: Vec<AnswerBlock>,
    /// 适配器阶段发现的问题（留白样式冲突等）；题级 issues 仍留在 ExamBundle
    pub issues: Vec<Issue>,
}

impl LayoutDoc {
    /// 全卷题数（答案区/预检的口径校验用）
    pub fn question_count(&self) -> usize {
        self.sections.iter().map(|s| s.header.question_count).sum()
    }

    /// 是否需要在文末排「参考答案」区
    pub fn has_answer_key(&self) -> bool {
        !self.answer_key.is_empty()
    }
}

/// 卷头元信息（§6.3 首页大卷头）
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DocumentMeta {
    pub school: Option<String>,
    pub duration_min: Option<u32>,
    /// 卷面总分：`exam_meta.total_score`，缺省时按各题分值求和
    pub total_score: f64,
    /// 考试说明（逐条）
    pub instructions: Vec<String>,
}

/// 大题
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub header: SectionHeader,
    pub blocks: Vec<LayoutBlock>,
}

/// 大题标题（灰底 + 题数分值框，§6.3 `section-header`）
#[derive(Debug, Clone, PartialEq)]
pub struct SectionHeader {
    pub meta: BlockMeta,
    pub title: String,
    pub instruction: Option<String>,
    pub question_count: usize,
    pub total_score: f64,
}

/// 版面块（§6.2 枚举）
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutBlock {
    Question(QuestionBlock),
    SubQuestion(SubQuestionBlock),
    Callout(CalloutBlock),
    Blank(BlankBlock),
    Answer(AnswerBlock),
}

impl LayoutBlock {
    pub fn meta(&self) -> BlockMeta {
        match self {
            Self::Question(b) => b.meta,
            Self::SubQuestion(b) => b.meta,
            Self::Callout(b) => b.meta,
            Self::Blank(b) => b.meta,
            Self::Answer(b) => b.meta,
        }
    }

    /// 关联题号（诊断与警告定位用）
    pub fn question_no(&self) -> u32 {
        match self {
            Self::Question(b) => b.number,
            Self::SubQuestion(b) => b.number,
            Self::Callout(b) => b.number,
            Self::Blank(b) => b.number,
            Self::Answer(b) => b.number,
        }
    }
}

/// 题干块（含选项栅格）
#[derive(Debug, Clone, PartialEq)]
pub struct QuestionBlock {
    pub meta: BlockMeta,
    pub number: u32,
    pub score: f64,
    pub kind: QuestionKind,
    pub stem: Vec<InlineNode>,
    /// 选项（非选择题为空）
    pub options: Vec<ExamOption>,
    /// 选项栅格：与 docx `w:tbl` 同一次 [`crate::typeset::blocks::choice_grid::decide`]
    pub grid: ChoiceGrid,
}

/// 小问块（解答题问树展开；`depth` 供渲染器逐层缩进）
#[derive(Debug, Clone, PartialEq)]
pub struct SubQuestionBlock {
    pub meta: BlockMeta,
    pub number: u32,
    pub depth: u8,
    /// 小问标号（`(1)` / `①`），空串 = 无标号
    pub label: String,
    pub stem: Vec<InlineNode>,
}

/// 提示框块（教师模式四类）
#[derive(Debug, Clone, PartialEq)]
pub struct CalloutBlock {
    pub meta: BlockMeta,
    pub number: u32,
    pub callout: Callout,
}

/// 答题留白块（B5 合并结果：高度与开关来自 options，样式来自 spec，冲突时 options 胜）
#[derive(Debug, Clone, PartialEq)]
pub struct BlankBlock {
    pub meta: BlockMeta,
    pub number: u32,
    pub height_mm: f32,
    pub style: BlankStyle,
}

impl BlankBlock {
    pub fn new(number: u32, blank: &ResolvedBlank) -> Self {
        Self {
            meta: BlockMeta::solid(),
            number,
            height_mm: blank.height_mm,
            style: blank.style,
        }
    }
}

/// 答案 / 解析块（内嵌或卷末）
#[derive(Debug, Clone, PartialEq)]
pub struct AnswerBlock {
    pub meta: BlockMeta,
    pub number: u32,
    pub kind: QuestionKind,
    /// 答案行：解答题逐小问（带 label），其余题型通常一条
    pub lines: Vec<AnswerLine>,
    /// 解析（`options.include_analysis` 才非空）
    pub analyses: Vec<AnalysisEntry>,
}

impl AnswerBlock {
    /// 本题既无答案又无解析 → 不该出现在版面上
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.analyses.is_empty()
    }
}

/// 一条答案（`label` 为空即纯答案文本）
#[derive(Debug, Clone, PartialEq)]
pub struct AnswerLine {
    pub label: String,
    pub nodes: Vec<InlineNode>,
}

/// 一段解析（`title` 来自 `AnalysisBlock.title`，可为空）
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisEntry {
    pub title: String,
    pub nodes: Vec<InlineNode>,
}
