/// 应用配置，从环境变量加载
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub jwt_secret: String,
    pub jwt_expiry_hours: i64,
    pub host: String,
    pub port: u16,
}

impl AppConfig {
    /// 从环境变量加载配置，缺失时使用默认值
    pub fn from_env() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL")
                .expect("DATABASE_URL 环境变量未设置"),
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
        }
    }
}
