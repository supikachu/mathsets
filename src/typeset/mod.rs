//! 排版内核：版面决策与渲染（实施计划 §六）
//!
//! 放在 `export::` 之外，是因为同一份决策要被 docx 与 typst 两个出口共用；反过来 typeset
//! 不 import export —— 两个模块之间的唯一桥是适配器 `export::pdf`（T3.3）。
//! 已落地：`blocks::choice_grid`（T2.5 选项栅格决策）、`spec`（T3.2 版面参数与预设）。
//! M3 待落地：`ir` / `math` / `compiler` / `typst_gen`；M4：`blocks` 全量与母版分离。

pub mod blocks;
pub mod spec;
