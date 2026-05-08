pub mod auth;
pub mod config;
pub mod db;
pub mod handlers;
pub mod models;

use axum::{
    routing::{delete, get, post, put},
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
        // 知识点树
        .route("/api/v1/knowledge-points", get(handlers::knowledge_points::list_knowledge_points))
        .route("/api/v1/knowledge-points", post(handlers::knowledge_points::create_knowledge_point))
        .route(
            "/api/v1/knowledge-points/{id}",
            put(handlers::knowledge_points::update_knowledge_point),
        )
        .route(
            "/api/v1/knowledge-points/{id}",
            delete(handlers::knowledge_points::delete_knowledge_point),
        )
        // 题目 CRUD
        .route("/api/v1/questions", get(handlers::questions::list_questions))
        .route("/api/v1/questions", post(handlers::questions::create_question))
        .route("/api/v1/questions/{id}", get(handlers::questions::get_question))
        .route(
            "/api/v1/questions/{id}",
            put(handlers::questions::update_question),
        )
        .route(
            "/api/v1/questions/{id}",
            delete(handlers::questions::delete_question),
        )
        // 审核
        .route(
            "/api/v1/questions/{id}/submit",
            post(handlers::questions::submit_question),
        )
        .route(
            "/api/v1/questions/{id}/review",
            post(handlers::questions::review_question),
        )
        // 全局中间件
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
