use tracing_subscriber::EnvFilter;

use mathset::build_app;
use mathset::config::AppConfig;
use mathset::db;

#[tokio::main]
async fn main() {
    // 加载 .env 文件（如果存在）
    dotenvy::dotenv().ok();

    // 初始化日志 (tracing)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mathset=debug,tower_http=debug".into()),
        )
        .init();

    // 加载配置
    let config = AppConfig::from_env();

    // 连接数据库
    let pool = db::create_pool(&config.database_url, config.database_max_connections).await;

    // 运行迁移
    db::run_migrations(&pool).await;

    tracing::info!("数据库迁移完成");

    // 确保公共空间存在
    if let Err(e) = mathset::auth::permissions::ensure_public_space(&pool).await {
        tracing::warn!("初始化公共空间失败: {}", e);
    }

    // 构建共享状态
    let state = mathset::AppState::new(
        pool,
        config.jwt_secret.clone(),
        config.jwt_expiry_hours,
        config.ai.clone(),
    );

    // 构建路由
    let app = build_app(state);

    // 启动服务器
    tracing::info!("🚀 服务启动于 http://{}:{}", config.host, config.port);

    let listener = tokio::net::TcpListener::bind((config.host.as_str(), config.port))
        .await
        .unwrap_or_else(|e| panic!("无法监听端口 {}:{}: {}", config.host, config.port, e));
    axum::serve(listener, app).await.unwrap();
}
