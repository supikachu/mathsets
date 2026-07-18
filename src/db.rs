use sqlx::postgres::PgPoolOptions;

/// 数据库连接池类型别名
pub type DbPool = sqlx::PgPool;

/// 创建数据库连接池
pub async fn create_pool(database_url: &str, max_connections: u32) -> DbPool {
    PgPoolOptions::new()
        .max_connections(max_connections)
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
