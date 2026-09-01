//! 模块 A：导出引擎（Markdown / DOCX / PDF 三格式直出）
//!
//! 架构见 `docs/导出引擎与排版系统_实施计划.md` §三：
//! - `model.rs`   两层 IR 的第一层 ExamBundle（导出域：内容与语义）
//! - `content.rs` stem 文本 → InlineNode 切分（T1.3）
//! - `assembler.rs` 批量取题 + 权限过滤 + 选项解析 + 问树展开（T1.4）
//! - `assets.rs`  图片抓取（本地映射 + 外链拉取，T1.5）
//! - `markdown.rs` Markdown 生成器 + bundle zip（T1.6）

pub mod assembler;
pub mod content;
pub mod model;
