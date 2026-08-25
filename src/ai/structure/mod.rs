//! 用 OCR Markdown 把题干 / 答案 / 解析拆开，补上模型漏掉或截断的解法。
//!
//! 长解析卷一次丢给 Stage2 时，模型常把【解析】写进 stem，再被 max_tokens 截断。
//! 这里先切开题干再送模型，最后用原文解法回填。

mod analysis;
mod choice;

pub use analysis::{
    finalize_parsed_question, peel_marked_fields, recover_chunk_questions, recover_parsed_questions,
    recover_question_sections, resplit_nested_methods, split_body_and_tail, stage2_llm_input,
};
pub(crate) use choice::looks_like_choice_stem;
