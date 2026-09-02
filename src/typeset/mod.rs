//! 排版内核：版面决策与渲染（实施计划 §六）
//!
//! 放在 `export::` 之外，是因为同一份决策要被 docx 与 typst 两个出口共用；驱动方向是单向的
//! export → typeset，**两个模块之间的唯一桥是适配器 `export::pdf`**（T3.3）：本模块不调装配器、
//! 不碰生成器、不看 handler，只借用 `export::model` 里的纯数据类型（`InlineNode` 等内容词汇表）。
//! 已落地：`blocks::choice_grid`（T2.5 选项栅格决策）、`spec`（T3.2 版面参数与预设）、
//! `ir`（T3.3 排版域 IR）、`math`（T3.4 LaTeX → Typst 数学源码与降级）、
//! `compiler`（T3.5 World 实现与 PDF/SVG 编译）、`typst_gen`（T3.6 LayoutDoc → main.typ）。
//! M4：`blocks` 全量与母版分离、分页粘连与密封线。

pub mod blocks;
pub mod compiler;
pub mod ir;
pub mod math;
pub mod spec;
pub mod typst_gen;
