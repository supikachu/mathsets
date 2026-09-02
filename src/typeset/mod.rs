//! 排版内核：版面决策与渲染（实施计划 §六）
//!
//! 放在 `export::` 之外，是因为同一份决策要被 docx 与 typst 两个出口共用；驱动方向是单向的
//! export → typeset，**两个模块之间的唯一桥是适配器 `export::pdf`**（T3.3）：本模块不调装配器、
//! 不碰生成器、不看 handler，只借用 `export::model` 里的纯数据类型（`InlineNode` 等内容词汇表）。
//! 已落地：`blocks::choice_grid`（T2.5 选项栅格决策）、`spec`（T3.2 版面参数与预设）、
//! `ir`（T3.3 排版域 IR）。
//! M3 待落地：`math` / `compiler` / `typst_gen`；M4：`blocks` 全量与母版分离。

pub mod blocks;
pub mod ir;
pub mod spec;
