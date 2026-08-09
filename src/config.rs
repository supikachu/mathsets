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
    /// 用户个人 API Key 的 AES-256-GCM 主密钥（base64 编码的 32 字节）
    pub key_encryption_key: Option<String>,
    pub default_model_text: String,
    pub default_model_vision: String,
    /// Doc2X OCR 引擎平台默认 API Key（用户未配个人 Key 时兜底）
    pub doc2x_api_key: Option<String>,
    /// Doc2X OCR 引擎 base_url（默认官方 v1 端点）
    pub doc2x_base_url: String,
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
            key_encryption_key: std::env::var("AI_KEY_ENCRYPTION_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
            default_model_text: std::env::var("AI_DEFAULT_MODEL_TEXT")
                .unwrap_or_else(|_| "deepseek-chat".to_string()),
            default_model_vision: std::env::var("AI_DEFAULT_MODEL_VISION")
                .unwrap_or_else(|_| "qwen-vl-plus".to_string()),
            // v1.1（M2）：Doc2X OCR 引擎配置
            doc2x_api_key: std::env::var("DOC2X_API_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
            doc2x_base_url: std::env::var("DOC2X_BASE_URL")
                .unwrap_or_else(|_| "https://api.doc2x.noedgex.com/v1".to_string()),
        }
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
