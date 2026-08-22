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

impl AiError {
    pub fn is_rate_limited(&self) -> bool {
        match self {
            AiError::Upstream(status, msg) => is_rate_limit_message(*status, msg),
            _ => false,
        }
    }
}

/// 智谱 code 1302 / HTTP 429 / 文案含「速率限制」
pub fn is_rate_limit_message(status: u16, msg: &str) -> bool {
    if status == 429 {
        return true;
    }
    let compact: String = msg.chars().filter(|c| !c.is_whitespace()).collect();
    let lower = compact.to_ascii_lowercase();
    compact.contains("\"code\":\"1302\"")
        || compact.contains("\"code\":1302")
        || msg.contains("速率限制")
        || lower.contains("ratelimit")
        || lower.contains("rate_limit")
        || lower.contains("too many requests")
}

pub const RATE_LIMIT_USER_MESSAGE: &str = "AI 服务请求过于频繁（已达速率限制），请稍后再试";

/// OpenRouter 把 Stealth / Ox Alpha 等上游失败包成 400，文案几乎不可读。
pub const OPENROUTER_PROVIDER_ERROR_USER_MESSAGE: &str =
    "OpenRouter 上游（Ox Alpha / Stealth）暂时拒绝了请求，请稍后重试；模型 ID 须为 stealth/ox-alpha";

/// OpenRouter 把上游失败包成 400 `Provider returned error`，常为瞬时故障。
pub fn is_transient_openrouter_error(status: u16, body: &str) -> bool {
    if !matches!(status, 400 | 502 | 503 | 524) {
        return false;
    }
    let lower = body.to_ascii_lowercase();
    lower.contains("provider returned error")
        || lower.contains("\"raw\":\"error\"")
        || (lower.contains("provider_name") && lower.contains("stealth"))
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

    /// 文本解析，带单次调用超时上限。
    ///
    /// Provider 的 Client 超时是全局的（OpenRouter 档位 240s），对「从候选菜单里挑几个
    /// ID」这类短输出调用过于宽松：撞上超时会白等 4 分钟且产出为零。默认实现忽略
    /// `timeout` 回退到 `parse_text_with_prompt`，由具体 provider 覆盖。
    async fn parse_text_with_prompt_timeout(
        &self,
        text: &str,
        prompt: &str,
        model: Option<&str>,
        timeout: Option<std::time::Duration>,
    ) -> Result<String, AiError> {
        let _ = timeout;
        self.parse_text_with_prompt(text, prompt, model).await
    }

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
        "deepseek" | "qwen" | "openai" | "glm" | "zhipu" | "gemini" | "custom" | "openrouter" => {
            Box::new(crate::ai::deepseek::DeepSeekProvider::new(
                api_key.to_string(),
                base_url.to_string(),
            ))
        }
        _ => Box::new(crate::ai::deepseek::DeepSeekProvider::new(
            api_key.to_string(),
            base_url.to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glm_1302_is_rate_limit() {
        let body = r#"{"error":{"code":"1302","message":"您的账户已达到速率限制，请您控制请求频率"}}"#;
        assert!(is_rate_limit_message(429, body));
        assert!(is_rate_limit_message(400, body));
        assert!(AiError::Upstream(429, body.into()).is_rate_limited());
        assert!(!is_rate_limit_message(400, "bad request"));
        assert!(!AiError::Timeout.is_rate_limited());
    }

    #[test]
    fn openrouter_stealth_400_is_transient() {
        let body = r#"{"error":{"message":"Provider returned error","code":400,"metadata":{"raw":"ERROR","provider_name":"Stealth","is_byok":false}}}"#;
        assert!(is_transient_openrouter_error(400, body));
        assert!(!is_transient_openrouter_error(400, "bad request"));
        assert!(!is_transient_openrouter_error(401, body));
    }
}
