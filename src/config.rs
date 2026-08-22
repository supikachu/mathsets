/// 应用配置，从环境变量加载
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub database_max_connections: u32,
    pub jwt_secret: String,
    pub jwt_expiry_hours: i64,
    pub host: String,
    pub port: u16,
    pub ai: AiConfig,
    /// 用户上传文件根目录（头像等），默认 ./uploads
    pub upload_dir: String,
}

/// AI 服务配置（平台默认 Key + 加密密钥 + 默认模型）
#[derive(Debug, Clone)]
pub struct AiConfig {
    pub default_provider: String,
    pub deepseek_api_key: Option<String>,
    pub deepseek_base_url: String,
    pub qwen_api_key: Option<String>,
    pub qwen_base_url: String,
    pub openai_api_key: Option<String>,
    pub openai_base_url: String,
    pub glm_api_key: Option<String>,
    pub glm_base_url: String,
    pub gemini_api_key: Option<String>,
    pub gemini_base_url: String,
    /// 用户个人 API Key 的 AES-256-GCM 主密钥（base64 编码的 32 字节）
    pub key_encryption_key: Option<String>,
    pub default_model_text: String,
    pub default_model_vision: String,
    /// 打标专用模型；未设置时回退 `default_model_text`。
    /// 打标是结构化分类任务，用推理模型会把单题拖到分钟级。
    pub default_model_tagging: Option<String>,
    /// Doc2X OCR 引擎平台默认 API Key（用户未配个人 Key 时兜底）
    pub doc2x_api_key: Option<String>,
    /// Doc2X OCR 引擎 base_url（默认官方 v2 端点，裸域名，路径需含 /api/v2 前缀）
    pub doc2x_base_url: String,
    /// 打标向量召回。生产默认开；`TAGGING_VECTOR_RECALL=0` 关闭。测试默认关。
    pub tagging_vector_recall: bool,
}

impl AiConfig {
    /// 从环境变量加载 AI 配置，缺失时使用默认值
    pub fn from_env() -> Self {
        Self {
            default_provider: std::env::var("AI_DEFAULT_PROVIDER")
                .unwrap_or_else(|_| "deepseek".to_string()),
            deepseek_api_key: std::env::var("DEEPSEEK_API_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
            deepseek_base_url: std::env::var("DEEPSEEK_BASE_URL")
                .unwrap_or_else(|_| "https://api.deepseek.com".to_string()),
            qwen_api_key: std::env::var("QWEN_API_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
            qwen_base_url: std::env::var("QWEN_BASE_URL")
                .unwrap_or_else(|_| "https://dashscope.aliyuncs.com/compatible-mode".to_string()),
            openai_api_key: std::env::var("OPENAI_API_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
            openai_base_url: std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com".to_string()),
            glm_api_key: std::env::var("GLM_API_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
            glm_base_url: std::env::var("GLM_BASE_URL")
                .unwrap_or_else(|_| "https://open.bigmodel.cn/api/paas/v4".to_string()),
            gemini_api_key: std::env::var("GEMINI_API_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
            gemini_base_url: std::env::var("GEMINI_BASE_URL")
                .unwrap_or_else(|_| {
                    "https://generativelanguage.googleapis.com/v1beta/openai".to_string()
                }),
            key_encryption_key: std::env::var("AI_KEY_ENCRYPTION_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
            default_model_text: std::env::var("AI_DEFAULT_MODEL_TEXT")
                .unwrap_or_else(|_| "deepseek-chat".to_string()),
            default_model_vision: std::env::var("AI_DEFAULT_MODEL_VISION")
                .unwrap_or_else(|_| "qwen-vl-plus".to_string()),
            default_model_tagging: std::env::var("AI_DEFAULT_MODEL_TAGGING")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            // v1.1（M2）：Doc2X OCR 引擎配置
            // v2 迁移：官方已弃用 noedgex.com/v1 域名，统一改用 v2.doc2x.noedgeai.com
            doc2x_api_key: std::env::var("DOC2X_API_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
            doc2x_base_url: std::env::var("DOC2X_BASE_URL")
                .unwrap_or_else(|_| "https://v2.doc2x.noedgeai.com".to_string()),
            tagging_vector_recall: std::env::var("TAGGING_VECTOR_RECALL")
                .ok()
                .as_deref()
                != Some("0"),
        }
    }

    /// 平台默认 Key + Base URL。未知名称回退 DeepSeek。
    pub fn credentials_for(&self, provider: &str) -> (Option<String>, &'static str, String) {
        match provider {
            "qwen" => (self.qwen_api_key.clone(), "qwen", self.qwen_base_url.clone()),
            "openai" => (
                self.openai_api_key.clone(),
                "openai",
                self.openai_base_url.clone(),
            ),
            "glm" | "zhipu" => (self.glm_api_key.clone(), "glm", self.glm_base_url.clone()),
            "gemini" => (
                self.gemini_api_key.clone(),
                "gemini",
                self.gemini_base_url.clone(),
            ),
            "custom" | "openrouter" => (None, "custom", DEFAULT_OPENROUTER_BASE_URL.to_string()),
            _ => (
                self.deepseek_api_key.clone(),
                "deepseek",
                self.deepseek_base_url.clone(),
            ),
        }
    }
}

/// OpenRouter 默认 OpenAI 兼容根路径（含 `/v1`）
pub const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// 用户自填 Base URL 的 OpenAI 兼容服务（含 OpenRouter）
pub fn is_custom_llm_provider(provider: &str) -> bool {
    matches!(provider, "custom" | "openrouter")
}

/// 规范化用户填写的 LLM Base URL；空则默认 OpenRouter。
pub fn normalize_llm_base_url(raw: Option<&str>) -> Result<String, String> {
    let s = raw.unwrap_or("").trim().trim_end_matches('/').to_string();
    if s.is_empty() {
        return Ok(DEFAULT_OPENROUTER_BASE_URL.to_string());
    }
    let lower = s.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return Err("API 地址必须以 http:// 或 https:// 开头".into());
    }
    Ok(s)
}

/// 文本清洗 / 拆题 / 打标用的默认模型名
pub fn default_text_model(provider: &str) -> &'static str {
    match provider {
        "glm" | "zhipu" => "glm-4-flash",
        "gemini" => "gemini-3.7-flash",
        "qwen" => "qwen-plus",
        "openai" => "gpt-4o-mini",
        "custom" | "openrouter" => "stealth/ox-alpha",
        _ => "deepseek-chat",
    }
}

impl AppConfig {
    /// 从环境变量加载配置，缺失时使用默认值
    pub fn from_env() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL")
                .expect("DATABASE_URL 环境变量未设置"),
            database_max_connections: std::env::var("DATABASE_MAX_CONNECTIONS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(50),
            jwt_secret: std::env::var("JWT_SECRET")
                .unwrap_or_else(|_| "mathset-dev-secret-key".to_string()),
            jwt_expiry_hours: std::env::var("JWT_EXPIRY_HOURS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(24),
            host: std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3000),
            ai: AiConfig::from_env(),
            upload_dir: std::env::var("UPLOAD_DIR")
                .unwrap_or_else(|_| "./uploads".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_provider_helpers() {
        assert!(is_custom_llm_provider("custom"));
        assert!(is_custom_llm_provider("openrouter"));
        assert!(!is_custom_llm_provider("deepseek"));
        assert_eq!(
            normalize_llm_base_url(None).unwrap(),
            DEFAULT_OPENROUTER_BASE_URL
        );
        assert_eq!(
            normalize_llm_base_url(Some(" https://openrouter.ai/api/v1/ ")).unwrap(),
            "https://openrouter.ai/api/v1"
        );
        assert!(normalize_llm_base_url(Some("openrouter.ai/api/v1")).is_err());
        assert_eq!(default_text_model("custom"), "stealth/ox-alpha");
    }
}
