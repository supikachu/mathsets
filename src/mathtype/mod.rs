//! MathType 公式资产：按题关联（field + ordinal）
//!
//! 转换链（Windows Worker）：
//! LaTeX → MathML（本进程）→ ole.bin（`MATHTYPE_OLE_CLI`）→ WMF（`MATHTYPE_CONVERT_URL` `/convert_batch`）

pub mod convert;
pub mod extract;
pub mod sync;

pub use convert::MathTypeConvertConfig;
pub use sync::{
    load_ready_assets, mark_slots_from_question, sync_question_math_assets, MathTypeAssetKey,
    ReadyMathAsset,
};
