pub mod auth;
pub mod config;
pub mod db;
pub mod handlers;
pub mod models;

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::db::DbPool;

/// 应用共享状态
#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub jwt_secret: String,
    pub jwt_expiry_hours: i64,
}

/// 构建应用 Router，用于 main 启动和集成测试
pub fn build_app(state: AppState) -> Router {
    Router::new()
        // 健康检查
        .route("/health", get(handlers::health::health_check))
        // API v1 认证相关
        .route("/api/v1/auth/register", post(handlers::auth::register))
        .route("/api/v1/auth/login", post(handlers::auth::login))
        // 全局中间件
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
