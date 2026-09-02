//! 排版内核：版面决策与渲染（实施计划 §六）
//!
//! M2 只落地与导出格式无关的**纯版面决策**（选项栅格），typst 相关模块（spec / ir /
//! compiler / typst_gen）在 M3 引入。放在这里而不是 `export::` 下，是因为同一份决策要被
//! docx 与 typst 两个出口共用。

pub mod blocks;
