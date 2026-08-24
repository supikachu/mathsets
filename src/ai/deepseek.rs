use crate::ai::provider::{AiError, AiProvider};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// 单次 chat 调用的耗时归因。
///
/// 上层只看到一个笼统的总耗时（例如打标的 `extract_ms`），里面可能混着限流窗口等待和
/// 429 退避睡眠，无法判断究竟是模型慢还是配额不够。`wait_ms` 把这部分单独摘出来，
/// 模型实际响应时间即 `total_ms - wait_ms`。Drop 里输出以覆盖全部返回路径。
struct ChatCallTrace {
    model: String,
    started: Instant,
    wait_ms: u64,
    attempts: u32,
}

impl ChatCallTrace {
    fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            started: Instant::now(),
            wait_ms: 0,
            attempts: 0,
        }
    }

    fn add_wait(&mut self, d: Duration) {
        self.wait_ms = self.wait_ms.saturating_add(d.as_millis() as u64);
    }
}

impl Drop for ChatCallTrace {
    fn drop(&mut self) {
        let total_ms = self.started.elapsed().as_millis() as u64;
        tracing::info!(
            model = %self.model,
            total_ms,
            wait_ms = self.wait_ms,
            model_ms = total_ms.saturating_sub(self.wait_ms),
            attempts = self.attempts,
            "LLM 调用耗时（wait_ms 为限流窗口与退避睡眠，不计入模型耗时）"
        );
    }
}

/// DeepSeek Provider（兼容 OpenAI chat/completions 格式）
/// 通义千问、OpenAI、智谱 GLM、Gemini 也可复用（仅 base_url / 模型名不同）
pub struct DeepSeekProvider {
    api_key: String,
    base_url: String,
    client: Client,
}

impl DeepSeekProvider {
    pub fn new(api_key: String, base_url: String) -> Self {
        // Ox Alpha 等推理模型 P99 端到端可超过 3 分钟
        let timeout_secs = if is_openrouter_base(&base_url) { 240 } else { 180 };
        let client = Client::builder()
            // Stage2 多题批量结构化（max_tokens=8192）单次响应可能 60-150s，
            // 120s 边界过紧会触发 body 读取 TimedOut；放宽至 180s 与前端轮询上限对齐
            .timeout(Duration::from_secs(timeout_secs))
            .no_proxy() // 临时强制直连，绕过系统代理（排查 2s 超时）
            .build()
            .expect("无法创建 reqwest Client");
        Self {
            api_key,
            base_url,
            client,
        }
    }

    async fn send_chat<T: Serialize>(&self, body: &T) -> Result<String, AiError> {
        self.send_chat_with_timeout(body, None).await
    }

    /// 发送 chat/completions；遇 429 / 智谱 1302 时退避重试 2s→4s→8s。
    /// Gemini 免费档先走官方 RPM/TPM/RPD 窗口，再发请求。
    ///
    /// `timeout` 覆盖 Client 的全局超时，供短输出调用（如打标收敛）收紧上限。
    async fn send_chat_with_timeout<T: Serialize>(
        &self,
        body: &T,
        timeout: Option<Duration>,
    ) -> Result<String, AiError> {
        const MAX_RETRIES: u32 = 3;
        let url = chat_completions_url(&self.base_url);
        let payload = serde_json::to_value(body)
            .map_err(|e| AiError::Upstream(0, format!("序列化请求失败: {e}")))?;
        let payload = adapt_chat_payload(&self.base_url, payload);
        let gemini = crate::ai::gemini_limit::is_gemini_base(&self.base_url);
        let model_name = payload.get("model").and_then(|v| v.as_str());
        let mut trace = ChatCallTrace::new(model_name.unwrap_or("unknown"));
        if gemini {
            let model = model_name.unwrap_or("gemini-3.7-flash");
            let tokens = crate::ai::gemini_limit::estimate_input_tokens(&payload);
            let waited = Instant::now();
            let acquired = crate::ai::gemini_limit::acquire(model, tokens).await;
            trace.add_wait(waited.elapsed());
            acquired?;
        }

        let mut last_rate_limit: Option<AiError> = None;

        for attempt in 0..=MAX_RETRIES {
            trace.attempts = attempt + 1;
            let mut req = apply_openai_compat_headers(
                self.client.post(&url),
                &self.base_url,
                &self.api_key,
            )
            .json(&payload);
            if let Some(t) = timeout {
                req = req.timeout(t);
            }
            let resp = req.send().await;

            let resp = match resp {
                Ok(r) => r,
                Err(e) if e.is_timeout() => return Err(AiError::Timeout),
                Err(e) => return Err(AiError::Upstream(0, format!("{:?}", e))),
            };

            let status = resp.status().as_u16();
            let retry_after = retry_after_secs(&resp);
            if resp.status().is_success() {
                let chat_resp: ChatResponse = resp
                    .json()
                    .await
                    .map_err(|e| AiError::Upstream(status, format!("{:?}", e)))?;
                return chat_resp
                    .choices
                    .into_iter()
                    .next()
                    .and_then(|c| message_text(&c.message))
                    .ok_or_else(|| {
                        AiError::Upstream(status, "响应中无正文（推理模型可能把答案放在 reasoning 里）".to_string())
                    });
            }

            let body_text = resp.text().await.unwrap_or_default();
            let retryable = (crate::ai::provider::is_rate_limit_message(status, &body_text)
                || crate::ai::provider::is_transient_openrouter_error(status, &body_text))
                && attempt < MAX_RETRIES;
            if retryable {
                let daily = gemini_daily_quota_exhausted(retry_after, &body_text);
                if daily {
                    return Err(AiError::Upstream(
                        status,
                        crate::ai::gemini_limit::GEMINI_RPD_USER_MESSAGE.into(),
                    ));
                }
                let delay_secs = if gemini {
                    retry_after.unwrap_or(15).clamp(5, 60)
                } else {
                    retry_after.unwrap_or(2u64.saturating_pow(attempt + 1)).min(30)
                };
                tracing::warn!(
                    "AI 上游 HTTP {status}，{delay_secs}s 后重试 ({}/{})",
                    attempt + 1,
                    MAX_RETRIES
                );
                last_rate_limit = Some(AiError::Upstream(status, body_text));
                let backoff = Instant::now();
                tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                trace.add_wait(backoff.elapsed());
                continue;
            }
            return Err(AiError::Upstream(status, body_text));
        }

        Err(last_rate_limit.unwrap_or_else(|| {
            AiError::Upstream(429, "rate limited".into())
        }))
    }
}

fn retry_after_secs(resp: &reqwest::Response) -> Option<u64> {
    let raw = resp.headers().get("retry-after")?.to_str().ok()?;
    raw.parse::<u64>().ok().map(|s| s.max(1))
}

fn gemini_daily_quota_exhausted(retry_after: Option<u64>, body: &str) -> bool {
    if retry_after.unwrap_or(0) > 90 {
        return true;
    }
    let lower = body.to_ascii_lowercase();
    lower.contains("per day")
        || lower.contains("requests per day")
        || lower.contains("daily quota")
        || lower.contains("rpd")
}

pub(crate) fn is_openrouter_base(base_url: &str) -> bool {
    base_url.to_ascii_lowercase().contains("openrouter.ai")
}

/// OpenAI 兼容请求头；OpenRouter 另附 HTTP-Referer / X-Title（部分免费模型会校验）。
pub(crate) fn apply_openai_compat_headers(
    req: reqwest::RequestBuilder,
    base_url: &str,
    api_key: &str,
) -> reqwest::RequestBuilder {
    let req = req.header("Authorization", format!("Bearer {api_key}"));
    if is_openrouter_base(base_url) {
        let referer = std::env::var("OPENROUTER_HTTP_REFERER")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "https://github.com/supikachu/mathsets".to_string());
        let title = std::env::var("OPENROUTER_APP_TITLE")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "MathSet".to_string());
        req.header("HTTP-Referer", referer).header("X-Title", title)
    } else {
        req
    }
}

/// 拼接 chat completions URL。
///
/// - DeepSeek / 通义 compatible-mode / OpenAI 根域名：`{base}/v1/chat/completions`
/// - 智谱 GLM `.../paas/v4`、Gemini OpenAI 兼容层 `.../openai`、已含 `/v1`：`{base}/chat/completions`
pub(crate) fn chat_completions_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        return base.to_string();
    }
    let lower = base.to_ascii_lowercase();
    if lower.ends_with("/v1")
        || lower.ends_with("/v4")
        || lower.contains("/paas/v4")
        || lower.ends_with("/openai")
        || lower.contains("/v1beta/openai")
    {
        return format!("{base}/chat/completions");
    }
    format!("{base}/v1/chat/completions")
}

/// 拼接 OpenAI 兼容 `GET /models` 探测地址（已含 `/v1` 时不再重复拼接）。
pub(crate) fn openai_models_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let lower = base.to_ascii_lowercase();
    if lower.ends_with("/models") {
        return base.to_string();
    }
    if lower.ends_with("/v1")
        || lower.ends_with("/v4")
        || lower.contains("/paas/v4")
        || lower.ends_with("/openai")
        || lower.contains("/v1beta/openai")
    {
        return format!("{base}/models");
    }
    format!("{base}/v1/models")
}

/// OpenRouter 上的推理/隐身模型（Ox Alpha 等）：对 temperature / system 更挑剔。
pub(crate) fn is_reasoning_model(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.contains("ox-alpha")
        || m.contains("stealth/")
        || m.contains("/o1")
        || m.contains("/o3")
        || m.contains("/o4")
        || m.contains("reasoning")
}

/// 按模型调整请求体：推理模型去掉 temperature、把 system 并入 user。
fn adapt_chat_payload(base_url: &str, mut payload: serde_json::Value) -> serde_json::Value {
    let model = payload
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if !is_reasoning_model(&model) && !is_openrouter_base(base_url) {
        return payload;
    }
    if is_reasoning_model(&model) {
        if let Some(obj) = payload.as_object_mut() {
            obj.remove("temperature");
            if obj
                .get("max_tokens")
                .and_then(|v| v.as_u64())
                .is_some_and(|n| n < 16384)
            {
                obj.insert("max_tokens".into(), serde_json::json!(16384));
            }
        }
        fold_system_into_user(&mut payload);
    }
    payload
}

fn fold_system_into_user(payload: &mut serde_json::Value) {
    let Some(msgs) = payload.get_mut("messages").and_then(|m| m.as_array_mut()) else {
        return;
    };
    if msgs.len() < 2 {
        return;
    }
    if msgs[0].get("role").and_then(|r| r.as_str()) != Some("system") {
        return;
    }
    let sys_text = match msgs[0].get("content") {
        Some(serde_json::Value::String(s)) => s.clone(),
        _ => return,
    };
    let Some(ui) = msgs
        .iter()
        .position(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
    else {
        return;
    };
    match msgs.get_mut(ui).and_then(|m| m.get_mut("content")) {
        Some(serde_json::Value::String(s)) => {
            *s = format!("{sys_text}\n\n{s}");
        }
        Some(serde_json::Value::Array(arr)) => {
            arr.insert(0, serde_json::json!({"type": "text", "text": sys_text}));
        }
        _ => return,
    }
    msgs.remove(0);
}

fn message_text(msg: &ChatResponseMessage) -> Option<String> {
    let from_content = match &msg.content {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => Some(s.clone()),
        Some(serde_json::Value::Array(arr)) => {
            let t: String = arr
                .iter()
                .filter_map(|p| {
                    p.get("text")
                        .and_then(|v| v.as_str())
                        .or_else(|| p.as_str())
                })
                .collect::<Vec<_>>()
                .join("");
            if t.trim().is_empty() {
                None
            } else {
                Some(t)
            }
        }
        _ => None,
    };
    from_content.or_else(|| {
        msg.reasoning
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .cloned()
    })
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
    #[serde(default)]
    content: Option<serde_json::Value>,
    #[serde(default)]
    reasoning: Option<String>,
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
        self.parse_text_with_prompt_timeout(text, prompt, model, None)
            .await
    }

    async fn parse_text_with_prompt_timeout(
        &self,
        text: &str,
        prompt: &str,
        model: Option<&str>,
        timeout: Option<Duration>,
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
            // v1.1（T1.12）：Stage 2 批量多题结构化需要更大输出空间，
            // 避免整卷多题被 max_tokens 截断导致 JSON 残缺。
            // 单题 parse_text 同样受益，DeepSeek-V3 上限内安全。
            max_tokens: 8192,
        };

        self.send_chat_with_timeout(&req, timeout).await
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

        self.send_chat(&req).await
    }

    /// 多图片调用：user content 数组携带 N 张 image_url（OpenAI 兼容格式）
    async fn parse_images_with_prompt(
        &self,
        images_base64: &[String],
        prompt: &str,
        model: Option<&str>,
    ) -> Result<String, AiError> {
        let model_name = model.unwrap_or("qwen-vl-plus").to_string();

        let image_parts: Vec<serde_json::Value> = images_base64
            .iter()
            .map(|b64| {
                serde_json::json!({
                    "type": "image_url",
                    "image_url": {
                        "url": format!("data:image/png;base64,{}", b64)
                    }
                })
            })
            .collect();

        let req = serde_json::json!({
            "model": model_name,
            "messages": [
                {
                    "role": "system",
                    "content": prompt
                },
                {
                    "role": "user",
                    "content": image_parts
                }
            ],
            "temperature": 0.1,
            "max_tokens": 4096
        });

        self.send_chat(&req).await
    }
}

#[cfg(test)]
mod tests {
    use super::chat_completions_url;

    #[test]
    fn test_chat_url_deepseek_and_qwen() {
        assert_eq!(
            chat_completions_url("https://api.deepseek.com"),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://dashscope.aliyuncs.com/compatible-mode"),
            "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://api.openai.com"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_chat_url_glm_v4_and_gemini() {
        assert_eq!(
            chat_completions_url("https://open.bigmodel.cn/api/paas/v4"),
            "https://open.bigmodel.cn/api/paas/v4/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://open.bigmodel.cn/api/paas/v4/"),
            "https://open.bigmodel.cn/api/paas/v4/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://generativelanguage.googleapis.com/v1beta/openai"),
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://open.bigmodel.cn/api/paas/v4/chat/completions"),
            "https://open.bigmodel.cn/api/paas/v4/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://openrouter.ai/api/v1"),
            "https://openrouter.ai/api/v1/chat/completions"
        );
    }

    #[test]
    fn test_models_url_openrouter_and_deepseek() {
        use super::openai_models_url;
        assert_eq!(
            openai_models_url("https://openrouter.ai/api/v1"),
            "https://openrouter.ai/api/v1/models"
        );
        assert_eq!(
            openai_models_url("https://api.deepseek.com"),
            "https://api.deepseek.com/v1/models"
        );
        assert_eq!(
            openai_models_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/models"
        );
        assert!(super::is_openrouter_base("https://openrouter.ai/api/v1"));
        assert!(!super::is_openrouter_base("https://api.deepseek.com"));
    }

    #[test]
    fn ox_alpha_payload_drops_temperature_and_folds_system() {
        let raw = serde_json::json!({
            "model": "stealth/ox-alpha",
            "temperature": 0.1,
            "max_tokens": 8192,
            "messages": [
                {"role": "system", "content": "SYS"},
                {"role": "user", "content": "USER"}
            ]
        });
        let adapted = super::adapt_chat_payload("https://openrouter.ai/api/v1", raw);
        assert!(adapted.get("temperature").is_none());
        assert_eq!(adapted["max_tokens"], 16384);
        let msgs = adapted["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "SYS\n\nUSER");
        assert!(super::is_reasoning_model("stealth/ox-alpha"));
        assert!(!super::is_reasoning_model("deepseek-chat"));
    }
}
