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
    /// 文本解析
    async fn parse_text(
        &self,
        text: &str,
        model: Option<&str>,
    ) -> Result<String, AiError>;

    /// 图片 OCR（base64 编码的图片数据，不含 data:image 前缀）
    async fn parse_image(
        &self,
        image_base64: &str,
        model: Option<&str>,
    ) -> Result<String, AiError>;
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
