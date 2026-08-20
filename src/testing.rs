//! 集成测试辅助（`cargo test` 使用独立库，避免污染 `DATABASE_URL` 开发库）

/// 加载 `.env` 并返回 `DATABASE_URL_TEST`。
///
/// 未配置时返回 `None`；集成测试应跳过或失败，**不得**回退到 `DATABASE_URL`。
pub fn database_url() -> Option<String> {
    let _ = dotenvy::dotenv();
    std::env::var("DATABASE_URL_TEST").ok()
}
