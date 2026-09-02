//! 模块 A：导出引擎（Markdown / DOCX / PDF 三格式直出）
//!
//! 架构见 `docs/导出引擎与排版系统_实施计划.md` §三：
//! - `model.rs`   两层 IR 的第一层 ExamBundle（导出域：内容与语义）
//! - `content.rs` stem 文本 → InlineNode 切分（T1.3）
//! - `assembler.rs` 批量取题 + 权限过滤 + 选项解析 + 问树展开（T1.4）
//! - `assets.rs`  图片抓取（本地 /uploads/** 映射 + 外链 reqwest 拉取，T1.5）
//! - `math/`      LaTeX → MathML 归一与转换（T2.2），`math/omml.rs` MathML → OMML（T2.4）
//! - `docx/`      OOXML 打包与静态部件（T2.6），`docx/writer.rs` 内容写入（T2.7）
//! - `markdown.rs` Markdown 生成器 + bundle zip（T1.6）

pub mod assembler;
pub mod assets;
pub mod content;
pub mod docx;
pub mod markdown;
pub mod math;
pub mod model;
