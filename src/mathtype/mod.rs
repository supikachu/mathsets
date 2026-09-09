//! MathType 公式资产：按题关联（field + ordinal）
//!
//! 转换链（Windows Worker）：
//! LaTeX → MathML（本进程）→ ole.bin（`MATHTYPE_OLE_CLI`）→ WMF（`MATHTYPE_CONVERT_URL(S)` `/convert_batch`）

pub mod convert;
pub mod extract;
pub mod sync;

pub use convert::MathTypeConvertConfig;
pub use sync::{
    bump_question_priority, load_ready_assets, mark_slots_ai_deferred, mark_slots_from_question,
    sync_question_math_assets, MathTypeAssetKey, ReadyMathAsset, PRIORITY_AI_DEFER,
    PRIORITY_EXPORT, PRIORITY_NORMAL,
};
