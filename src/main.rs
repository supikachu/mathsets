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

    // 运行数据库自动迁移
    tracing::info!("开始执行数据库结构迁移...");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("数据库迁移执行失败！");
    tracing::info!("数据库迁移全部完成！");

    // 确保公共空间存在
    if let Err(e) = mathset::auth::permissions::ensure_public_space(&pool).await {
        tracing::warn!("初始化公共空间失败: {}", e);
    }


    // 确保上传目录存在（头像文件落盘位置）
    let upload_avatars_dir = std::path::Path::new(&config.upload_dir).join("avatars");
    if let Err(e) = std::fs::create_dir_all(&upload_avatars_dir) {
        tracing::warn!(
            "创建上传目录失败 {:?}: {}",
            upload_avatars_dir,
            e
        );
    } else {
        tracing::info!("📁 上传目录就绪: {:?}", upload_avatars_dir);
    }

    // 确保题目配图目录存在（题目图片落盘位置）
    let upload_questions_dir = std::path::Path::new(&config.upload_dir).join("questions");
    if let Err(e) = std::fs::create_dir_all(&upload_questions_dir) {
        tracing::warn!(
            "创建题目配图目录失败 {:?}: {}",
            upload_questions_dir,
            e
        );
    } else {
        tracing::info!("📁 题目配图目录就绪: {:?}", upload_questions_dir);
    }

    // 构建共享状态
    let state = mathset::AppState::new(
        pool,
        config.jwt_secret.clone(),
        config.jwt_expiry_hours,
        config.ai.clone(),
        config.upload_dir.clone(),
    );

    // 启动 AI 解析 worker 后台协程
    // 拾取 pending 任务 → 调用 LLM → 落库为新题目（草稿）→ 标记任务 completed/failed
    tokio::spawn(mathset::workers::ai_parse_worker::start_worker(state.clone()));
    tracing::info!("🤖 AI 解析 worker 已在后台启动");

    // 启动 SSE 票据过期清理后台任务（每 5 分钟清理一次过期 ticket）
    {
        let cleanup_state = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            loop {
                interval.tick().await;
                let before = cleanup_state.sse_tickets.len();
                cleanup_state
                    .sse_tickets
                    .retain(|_, info| !info.is_expired());
                let removed = before - cleanup_state.sse_tickets.len();
                if removed > 0 {
                    tracing::debug!("SSE 票据清理: 移除 {} 个过期票据", removed);
                }
            }
        });
        tracing::info!("🧹 SSE 票据清理任务已启动 (5 分钟间隔)");
    }

    // 构建路由
    let app = build_app(state);

    // 启动服务器
    tracing::info!("🚀 服务启动于 http://{}:{}", config.host, config.port);

    let listener = tokio::net::TcpListener::bind((config.host.as_str(), config.port))
        .await
        .unwrap_or_else(|e| panic!("无法监听端口 {}:{}: {}", config.host, config.port, e));
    axum::serve(listener, app).await.unwrap();
}
