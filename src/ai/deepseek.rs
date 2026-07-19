use crate::ai::provider::{AiError, AiProvider};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// DeepSeek Provider（兼容 OpenAI /v1/chat/completions 格式）
/// 通义千问、OpenAI 也可复用此实现（仅 base_url 不同）
pub struct DeepSeekProvider {
    api_key: String,
    base_url: String,
    client: Client,
}

impl DeepSeekProvider {
    pub fn new(api_key: String, base_url: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .no_proxy() // 临时强制直连，绕过系统代理（排查 2s 超时）
            .build()
            .expect("无法创建 reqwest Client");
        Self {
            api_key,
            base_url,
            client,
        }
    }
}

/// OpenAI 格式的 chat 请求
#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

/// OpenAI 格式的 chat 响应
#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    content: String,
}

/// 图片消息的 content（多模态）
#[derive(Serialize)]
struct ImageContent {
    #[serde(rename = "type")]
    content_type: &'static str,
    text: Option<String>,
    image_url: Option<ImageUrl>,
}

#[derive(Serialize)]
struct ImageUrl {
    url: String,
}

#[async_trait]
impl AiProvider for DeepSeekProvider {
    async fn parse_text_with_prompt(
        &self,
        text: &str,
        prompt: &str,
        model: Option<&str>,
    ) -> Result<String, AiError> {
        let model_name = model.unwrap_or("deepseek-chat").to_string();

        let req = ChatRequest {
            model: model_name,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: prompt.to_string(),
                },
                ChatMessage {
                    role: "user",
                    content: text.to_string(),
                },
            ],
            temperature: 0.1,
            max_tokens: 4096,
        };

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&req)
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) if e.is_timeout() => return Err(AiError::Timeout),
            // 用 Debug 格式输出完整 source 链（TLS / TCP / DNS 等底层错误）
            Err(e) => return Err(AiError::Upstream(0, format!("{:?}", e))),
        };

        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AiError::Upstream(status, body));
        }

        let chat_resp: ChatResponse = resp
            .json()
            .await
            .map_err(|e| AiError::Upstream(status, format!("{:?}", e)))?;

        chat_resp
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| AiError::Upstream(status, "响应中无 choices".to_string()))
    }

    async fn parse_image_with_prompt(
        &self,
        image_base64: &str,
        prompt: &str,
        model: Option<&str>,
    ) -> Result<String, AiError> {
        let model_name = model.unwrap_or("qwen-vl-plus").to_string();

        // 多模态消息使用 serde_json::Value 构造（content 是数组）
        let req = serde_json::json!({
            "model": model_name,
            "messages": [
                {
                    "role": "system",
                    "content": prompt
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:image/png;base64,{}", image_base64)
                            }
                        }
                    ]
                }
            ],
            "temperature": 0.1,
            "max_tokens": 4096
        });

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&req)
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) if e.is_timeout() => return Err(AiError::Timeout),
            // 用 Debug 格式输出完整 source 链（TLS / TCP / DNS 等底层错误）
            Err(e) => return Err(AiError::Upstream(0, format!("{:?}", e))),
        };

        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AiError::Upstream(status, body));
        }

        let chat_resp: ChatResponse = resp
            .json()
            .await
            .map_err(|e| AiError::Upstream(status, format!("{:?}", e)))?;

        chat_resp
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| AiError::Upstream(status, "响应中无 choices".to_string()))
    }
}
