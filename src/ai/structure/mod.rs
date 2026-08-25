//! 用 OCR Markdown 把题干 / 答案 / 解析拆开，补上模型漏掉或截断的解法。
//!
//! 长解析卷一次丢给 Stage2 时，模型常把【解析】写进 stem，再被 max_tokens 截断。
//! 这里先切开题干再送模型，最后用原文解法回填。
//!
//! 全自动路径另提供纯脚本 `structure_chunk` → `ScriptDraft`（阶段 1 仍每块打 LLM）。

mod analysis;
mod choice;
mod chunk;
mod confidence;
mod merge;
mod options;
mod validate;

pub use analysis::{
    count_method_headings, finalize_parsed_question, looks_like_analysis_chunk,
    peel_marked_fields, recover_chunk_questions, recover_parsed_questions,
    recover_question_sections, resplit_nested_methods, split_body_and_tail, split_chunk_analysis,
    stage2_llm_input,
};
pub use chunk::{
    extract_chunk_question_no, guess_chunk_question_type, stage2_patch_user_input, structure_chunk,
};
pub use merge::{merge_script_and_llm, restore_script_analysis_if_needed, script_usable};
pub use validate::{
    append_validation_warnings, llm_core_ok, strip_options_residue_from_stem, validate_structured,
    StructuredValidation,
};
pub use confidence::{
    script_skip_accepted, script_skip_accepted_with, should_call_llm, should_call_llm_with,
};
pub(crate) use choice::looks_like_choice_stem;

use crate::ai::types::ParsedQuestion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    High,
    Low,
}

#[derive(Debug, Clone)]
pub struct ScriptDraft {
    pub question: ParsedQuestion,
    pub confidence: Confidence,
    pub reasons: Vec<String>,
    pub method_heading_count: usize,
    pub image_urls_in_chunk: Vec<String>,
}
