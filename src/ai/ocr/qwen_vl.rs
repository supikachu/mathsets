//! Qwen-VL OCR 引擎（兜底实现）
//!
//! 包装现有 `DeepSeekProvider::parse_image_with_prompt`（OpenAI 兼容多模态接口），
//! 调用视觉模型（默认 `qwen-vl-plus`）输出**纯 Markdown**（含 LaTeX 与图片占位符），
//! 而非结构化 JSON。结构化由 Stage 2 文本 LLM 负责。
//!
//! 行为等价重构前的视觉识别能力，作为未配置 OCR 引擎时的兜底（AC-07）。

use async_trait::async_trait;

use super::{map_ai_to_ocr_error, OcrError, OcrProvider};
use crate::ai::deepseek::DeepSeekProvider;
use crate::ai::prompt::QWEN_VL_OCR_PROMPT;
use crate::ai::provider::AiProvider;

/// Qwen-VL OCR 引擎
pub struct QwenVlOcrProvider {
    inner: DeepSeekProvider,
    model: Option<String>,
}

impl QwenVlOcrProvider {
    pub fn new(api_key: String, base_url: String, model: Option<String>) -> Self {
        Self {
            inner: DeepSeekProvider::new(api_key, base_url),
            model,
        }
    }
}

#[async_trait]
impl OcrProvider for QwenVlOcrProvider {
    fn id(&self) -> &'static str {
        "qwen_vl"
    }

    /// Qwen-VL 不支持 PDF 直传，仍需前端逐页渲染为图片后调用 `ocr_image`
    fn supports_pdf(&self) -> bool {
        false
    }

    async fn ocr_image(&self, image_b64: &str) -> Result<String, OcrError> {
        let markdown = self
            .inner
            .parse_image_with_prompt(image_b64, QWEN_VL_OCR_PROMPT, self.model.as_deref())
            .await
            .map_err(map_ai_to_ocr_error)?;
        Ok(markdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qwen_vl_id_and_pdf_support() {
        let p = QwenVlOcrProvider::new("k".into(), "https://example.com".into(), None);
        assert_eq!(p.id(), "qwen_vl");
        assert!(!p.supports_pdf());
    }
}
