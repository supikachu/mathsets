use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

/// 数据库连接池类型别名
pub type DbPool = sqlx::PgPool;

/// 创建数据库连接池
pub async fn create_pool(database_url: &str, max_connections: u32) -> DbPool {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(8))
        // 空闲连接及时回收，避免 NAT/Postgres 先掐掉半开连接，导致下一次请求挂死
        .idle_timeout(Duration::from_secs(60))
        .max_lifetime(Duration::from_secs(30 * 60))
        .connect(database_url)
        .await
        .expect("无法连接到数据库，请检查 DATABASE_URL")
}

/// 运行数据库迁移
pub async fn run_migrations(pool: &DbPool) {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .expect("数据库迁移失败");
}
