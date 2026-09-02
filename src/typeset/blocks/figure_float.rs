//! 左文右图：把题干尾部的配图钉进右栏（T4.3，§6.2）
//!
//! 纯函数、零 typst 依赖，与 [`choice_grid`] 同一口径：宽度一律用 **em**（相对正文字号），
//! 由调用方把栏宽换算后传进来。判定结果 [`Split`] 只是两个下标，`typst_gen` 照着它把 `stem`
//! 切成左右两格 —— 内容一个字节都不改，所以「浮动与否」永远不改变卷面上的文字。
//!
//! ## 为什么右栏不会失宽
//!
//! 版式是 `grid(columns: (1fr, 35%))`。typst 先按父容器宽折算所有**相对**轨道，剩下的才分给
//! `fr`（`typst-layout::grid::layouter::measure_columns`：`Sizing::Rel` 走
//! `relative_to(regions.base().x)`，`grow_fractional_columns` 只拿余额）。所以右栏恒等于
//! 栏宽 × [`FIGURE_SHARE`]，与左栏文字长短无关；图再按 Rust 算出的绝对毫米宽画出来，
//! 两头都不 flex —— 这就是验收口径「图列宽度恒定不失宽」的成因。
//!
//! ## 放行条件（每一条都对应一种惊喜）
//!
//! - **只浮动尾部那一枚**：图后面还有文字时，那段文字会被挤进左栏 65% 另起一段，读起来像漏了一段；
//! - **必须是单张图**：图组（≥ 2 张并排）要在 30mm 的右栏里横排，必然折行；
//! - **`width` 必须显式给出**：没有 px 宽就无从估宽，而图片在 typst 里不会自己缩小，
//!   装不下就是栏外溢出；
//! - **`align` 必须没写过**：`{align: center}` 的原意是「在栏里居中」，浮动后居中对象变成
//!   那 30mm 的右栏，作者的意图被悄悄改写；
//! - **左栏不许有表格与块级公式**：两者都按整栏宽设计，压到 65% 会在自己内部溢出；
//! - **估宽留 10% 余量**：em↔mm 折算、`cjk-latin-spacing` 的自动间距、以及 docx 与 typst 的
//!   悬挂缩进口径差（2.0em vs 2.6em）都可能让估宽偏乐观。宁可让它独占一行，也不把溢出留在纸上。

use crate::export::model::InlineNode;
use crate::typeset::blocks::choice_grid;

/// 右栏（图列）占栏宽的比例（§6.2 定值；typst 模板里那个 `35%` 必须与它一致）
pub const FIGURE_SHARE: f64 = 0.35;

/// 估宽余量：装得进九成右栏才算装得进右栏
const FIT_MARGIN: f64 = 0.9;

/// 题干切分：`[0, text_end)` 进左栏，`[figure_start, len)` 进右栏
///
/// 存下标而不是存节点副本：[`crate::typeset::ir::QuestionBlock::figure`] 因此照样廉价可复制，
/// 而渲染器直接借用原切片 —— 浮动是版面的事，不该复制内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Split {
    /// 左栏文字的结束处（紧邻图的那些只为让图独立成行的 `LineBreak` 不计入）
    pub text_end: usize,
    /// 右栏配图开始处
    pub figure_start: usize,
}

impl Split {
    /// 按本切分取左右两栏的节点切片
    pub fn parts<'a>(&self, stem: &'a [InlineNode]) -> (&'a [InlineNode], &'a [InlineNode]) {
        (&stem[..self.text_end], &stem[self.figure_start..])
    }
}

/// 右栏宽度（em）：`column_em` 是**整栏**宽，未扣悬挂缩进 —— 图格在 `item` 的 inset 之外，
/// 跟左栏文字共享整栏，不会被缩进吃掉
///
/// 渲染器不必再给图片补一道宽度上限：放行条件已经保证画出来的宽 ≤ 九成右栏。
pub fn figure_cell_em(column_em: f64) -> f64 {
    column_em * FIGURE_SHARE
}

/// 这道题干要不要左文右图；要的话在哪切
///
/// 返回 `None` 时图照旧独占整行 —— 那是 M3 以来的既有行为，不浮动不是降级，只是这道题不适合。
pub fn plan(stem: &[InlineNode], column_em: f64) -> Option<Split> {
    let figure_start = stem.len().checked_sub(1)?;
    let width_em = figure_width_em(&stem[figure_start])?;
    if width_em > figure_cell_em(column_em) * FIT_MARGIN {
        return None;
    }

    // 图前面那几个换行只是为了把图顶到独立一行，浮动之后留在左栏就是凭空一个空行
    let mut text_end = figure_start;
    while text_end > 0 && matches!(stem[text_end - 1], InlineNode::LineBreak) {
        text_end -= 1;
    }
    let text = &stem[..text_end];
    if text.is_empty() || !text_flows(text) {
        return None;
    }
    Some(Split {
        text_end,
        figure_start,
    })
}

/// 尾部这一枚是不是「可以浮动的单图」，是则给出估宽（em）
fn figure_width_em(node: &InlineNode) -> Option<f64> {
    let floatable = match node {
        InlineNode::Image {
            width: Some(_),
            align: None,
            ..
        } => true,
        // 图注会自己在右栏折行，不参与估宽；两张以上并排则直接否决
        InlineNode::ImgRow {
            align: None,
            images,
            ..
        } => images.len() == 1 && images[0].width.is_some(),
        _ => false,
    };
    floatable.then(|| choice_grid::inline_width(std::slice::from_ref(node)))
}

/// 左栏放不放得下：表格与块级公式按整栏宽设计，压到 65% 会在自己内部溢出。
/// 硬换行不算在内 —— 左栏里换行本来就是正常的多行题干。
fn text_flows(text: &[InlineNode]) -> bool {
    !text.iter().any(|n| match n {
        InlineNode::Table { .. } => true,
        InlineNode::Math { display, .. } => *display,
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::model::{ImageAlign, InlineImage};

    /// a4_practice 的整栏宽：86mm ≈ 23.2em ⇒ 右栏 ≈ 8.1em ≈ 30mm
    const COLUMN_EM: f64 = 86.0 / choice_grid::MM_PER_EM;
    /// 1em ≈ 14px ⇒ 右栏约 113px，留一成余量后 90px 装得下、200px 装不下
    const NARROW_PX: u32 = 90;
    const WIDE_PX: u32 = 200;

    fn text(s: &str) -> InlineNode {
        InlineNode::Text { text: s.into() }
    }

    fn image(px: Option<u32>) -> InlineNode {
        InlineNode::Image {
            alt: None,
            url: "/uploads/a.png".into(),
            width: px,
            align: None,
        }
    }

    fn image_with(px: u32, align: Option<ImageAlign>) -> InlineNode {
        InlineNode::Image {
            alt: None,
            url: "/uploads/a.png".into(),
            width: Some(px),
            align,
        }
    }

    fn one(px: u32) -> InlineImage {
        InlineImage {
            alt: None,
            url: "/uploads/a.png".into(),
            width: Some(px),
        }
    }

    fn row(images: Vec<InlineImage>) -> InlineNode {
        InlineNode::ImgRow {
            align: None,
            images,
            caption: None,
        }
    }

    #[test]
    fn trailing_narrow_image_floats_and_the_text_keeps_its_range() {
        let stem = vec![
            text("如图，"),
            InlineNode::LineBreak,
            image(Some(NARROW_PX)),
        ];
        let split = plan(&stem, COLUMN_EM).expect("窄图应浮动");
        assert_eq!(
            split,
            Split {
                text_end: 1,
                figure_start: 2
            }
        );
        let (left, right) = split.parts(&stem);
        assert_eq!(left, &[text("如图，")], "为垫图而生的换行不该留在左栏");
        assert_eq!(right, &[image(Some(NARROW_PX))]);
    }

    #[test]
    fn wide_image_keeps_its_own_full_width_row() {
        let stem = vec![text("如图，"), image(Some(WIDE_PX))];
        assert_eq!(plan(&stem, COLUMN_EM), None, "装不进右栏的图不许浮动");
        // 栏宽为零时连窄图也装不下
        let stem = vec![text("如图，"), image(Some(NARROW_PX))];
        assert_eq!(plan(&stem, 0.0), None);
    }

    #[test]
    fn figure_must_declare_its_width() {
        let stem = vec![text("如图，"), image(None)];
        assert_eq!(plan(&stem, COLUMN_EM), None);
        let stem = vec![
            text("如图，"),
            row(vec![InlineImage {
                width: None,
                ..one(1)
            }]),
        ];
        assert_eq!(plan(&stem, COLUMN_EM), None);
    }

    #[test]
    fn explicit_align_is_the_authors_not_ours() {
        for align in [ImageAlign::Left, ImageAlign::Center, ImageAlign::Right] {
            let stem = vec![text("如图，"), image_with(NARROW_PX, Some(align))];
            assert_eq!(plan(&stem, COLUMN_EM), None, "{align:?} 对齐是作者要的");
        }
        assert!(plan(&[text("如图，"), image_with(NARROW_PX, None)], COLUMN_EM).is_some());
    }

    #[test]
    fn only_a_trailing_figure_floats() {
        let stem = vec![image(Some(NARROW_PX)), text("则甲数为（　）")];
        assert_eq!(plan(&stem, COLUMN_EM), None, "图在文字前面时不许浮动");
    }

    #[test]
    fn a_bare_figure_has_nothing_to_stand_beside() {
        assert_eq!(plan(&[image(Some(NARROW_PX))], COLUMN_EM), None);
        assert_eq!(plan(&[], COLUMN_EM), None);
    }

    #[test]
    fn side_by_side_rows_stay_in_the_flow() {
        // 尾部是单图：即使前面还有一组并排图，浮动的也只是尾部这一枚
        let stem = vec![
            text("如图，"),
            row(vec![one(40), one(40)]),
            image(Some(NARROW_PX)),
        ];
        assert!(plan(&stem, COLUMN_EM).is_some());
        // 尾部本身是图组：30mm 右栏放不下两张并排
        let stem = vec![text("如图，"), row(vec![one(NARROW_PX), one(NARROW_PX)])];
        assert_eq!(plan(&stem, COLUMN_EM), None);
    }

    #[test]
    fn single_image_row_floats_with_its_caption() {
        let stem = vec![
            text("如图，"),
            InlineNode::ImgRow {
                align: None,
                images: vec![one(NARROW_PX)],
                caption: Some("图 1 三视图".into()),
            },
        ];
        assert!(plan(&stem, COLUMN_EM).is_some());
    }

    #[test]
    fn tables_and_display_math_need_the_whole_column() {
        let stem = vec![
            text("甲"),
            InlineNode::Table {
                header: vec!["x".into()],
                aligns: vec![],
                rows: vec![vec!["1".into()]],
            },
            image(Some(NARROW_PX)),
        ];
        assert_eq!(plan(&stem, COLUMN_EM), None);
        let stem = vec![
            text("甲"),
            InlineNode::Math {
                latex: "a=b".into(),
                display: true,
            },
            image(Some(NARROW_PX)),
        ];
        assert_eq!(plan(&stem, COLUMN_EM), None, "块级公式按整栏排");
        let stem = vec![
            text("甲"),
            InlineNode::Math {
                latex: "a=b".into(),
                display: false,
            },
            image(Some(NARROW_PX)),
        ];
        assert!(plan(&stem, COLUMN_EM).is_some(), "行内公式照常留在左栏");
    }

    #[test]
    fn only_the_breaks_touching_the_figure_are_trimmed() {
        let stem = vec![
            text("第一行"),
            InlineNode::LineBreak,
            text("第二行"),
            InlineNode::LineBreak,
            InlineNode::LineBreak,
            image(Some(NARROW_PX)),
        ];
        let split = plan(&stem, COLUMN_EM).unwrap();
        assert_eq!(split.text_end, 3, "只剪掉紧贴图的那批换行");
        assert_eq!(split.figure_start, 5);
    }
}
