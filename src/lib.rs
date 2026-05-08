pub mod auth;
pub mod config;
pub mod db;
pub mod handlers;
pub mod models;

use axum::{
    middleware,
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
    // ─── 需要 JWT 认证的保护路由 ───
    let protected_routes = Router::new()
        // 知识点树
        .route("/knowledge-points", get(handlers::knowledge_points::list_knowledge_points))
        .route("/knowledge-points", post(handlers::knowledge_points::create_knowledge_point))
        .route(
            "/knowledge-points/{id}",
            put(handlers::knowledge_points::update_knowledge_point),
        )
        .route(
            "/knowledge-points/{id}",
            delete(handlers::knowledge_points::delete_knowledge_point),
        )
        // 题目 CRUD
        .route("/questions", get(handlers::questions::list_questions))
        .route("/questions", post(handlers::questions::create_question))
        .route("/questions/{id}", get(handlers::questions::get_question))
        .route("/questions/{id}", put(handlers::questions::update_question))
        .route("/questions/{id}", delete(handlers::questions::delete_question))
        // 审核
        .route("/questions/{id}/submit", post(handlers::questions::submit_question))
        .route("/questions/{id}/review", post(handlers::questions::review_question))
        // 统一应用认证中间件
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::require_auth,
        ));

    // ─── 公开路由 + 保护路由合并 ───
    Router::new()
        // 健康检查（无需认证）
        .route("/health", get(handlers::health::health_check))
        // API v1 认证模块（无需认证）
        .route("/api/v1/auth/register", post(handlers::auth::register))
        .route("/api/v1/auth/login", post(handlers::auth::login))
        // API v1 保护模块（需要 JWT）
        .nest("/api/v1", protected_routes)
        // 全局中间件
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
