use async_trait::async_trait;

/// AI 调用错误
#[derive(Debug)]
pub enum AiError {
    /// 未配置 API Key
    NoApiKey,
    /// 上游服务错误（HTTP 状态码 + 消息）
    Upstream(u16, String),
    /// 请求超时
    Timeout,
}

/// AI Provider trait
#[async_trait]
pub trait AiProvider: Send + Sync {
    /// 文本解析（可传入自定义 system prompt）
    async fn parse_text_with_prompt(
        &self,
        text: &str,
        prompt: &str,
        model: Option<&str>,
    ) -> Result<String, AiError>;

    /// 图片 OCR（base64 编码的图片数据，不含 data:image 前缀，可传入自定义 system prompt）
    async fn parse_image_with_prompt(
        &self,
        image_base64: &str,
        prompt: &str,
        model: Option<&str>,
    ) -> Result<String, AiError>;

    /// 多图片调用（一张图片的默认实现复用 parse_image_with_prompt；
    /// 多图片时默认返回不支持错误，由具体 provider 覆盖）
    async fn parse_images_with_prompt(
        &self,
        images_base64: &[String],
        prompt: &str,
        model: Option<&str>,
    ) -> Result<String, AiError> {
        match images_base64 {
            [single] => self.parse_image_with_prompt(single, prompt, model).await,
            _ => Err(AiError::Upstream(
                0,
                format!("provider 不支持一次传入 {} 张图片", images_base64.len()),
            )),
        }
    }

    /// 文本解析（默认实现，使用默认 prompt）
    async fn parse_text(
        &self,
        text: &str,
        model: Option<&str>,
    ) -> Result<String, AiError> {
        self.parse_text_with_prompt(
            text,
            &crate::ai::prompt::TEXT_PARSE_FULL_PROMPT,
            model,
        )
        .await
    }

    /// 图片 OCR（默认实现，使用默认 prompt）
    async fn parse_image(
        &self,
        image_base64: &str,
        model: Option<&str>,
    ) -> Result<String, AiError> {
        self.parse_image_with_prompt(
            image_base64,
            &crate::ai::prompt::IMAGE_OCR_FULL_PROMPT,
            model,
        )
        .await
    }
}

/// 工厂：根据 provider 名称创建实例
pub fn create_provider(
    provider_name: &str,
    api_key: &str,
    base_url: &str,
) -> Box<dyn AiProvider> {
    match provider_name {
        "deepseek" => Box::new(crate::ai::deepseek::DeepSeekProvider::new(
            api_key.to_string(),
            base_url.to_string(),
        )),
        "qwen" => Box::new(crate::ai::deepseek::DeepSeekProvider::new(
            // qwen 兼容 OpenAI 格式，复用 deepseek 实现（仅 base_url 不同）
            api_key.to_string(),
            base_url.to_string(),
        )),
        "openai" => Box::new(crate::ai::deepseek::DeepSeekProvider::new(
            api_key.to_string(),
            base_url.to_string(),
        )),
        _ => Box::new(crate::ai::deepseek::DeepSeekProvider::new(
            api_key.to_string(),
            base_url.to_string(),
        )),
    }
}
