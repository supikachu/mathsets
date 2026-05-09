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
        // 教研组
        .route("/groups", get(handlers::groups::list_groups))
        .route("/groups", post(handlers::groups::create_group))
        .route("/groups/{id}", get(handlers::groups::get_group))
        .route("/groups/{id}", put(handlers::groups::update_group))
        .route("/groups/{id}", delete(handlers::groups::delete_group))
        .route("/groups/{id}/members", post(handlers::groups::add_member))
        .route("/groups/{id}/members/{user_id}", delete(handlers::groups::remove_member))
        .route("/groups/{id}/members/{user_id}", put(handlers::groups::set_leader))
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
        // 当前用户信息
        .route("/auth/me", get(handlers::auth::me))
        // 题目统计（必须在 {id} 之前注册）
        .route("/questions/stats", get(handlers::questions::question_stats))
        // 题目 CRUD
        .route("/questions", get(handlers::questions::list_questions))
        .route("/questions", post(handlers::questions::create_question))
        .route("/questions/{id}", get(handlers::questions::get_question))
        .route("/questions/{id}", put(handlers::questions::update_question))
        .route("/questions/{id}", delete(handlers::questions::delete_question))
        // 审核
        .route("/questions/{id}/submit", post(handlers::questions::submit_question))
        .route("/questions/{id}/review", post(handlers::questions::review_question))
        // 试卷 CRUD
        .route("/papers", get(handlers::papers::list_papers))
        .route("/papers", post(handlers::papers::create_paper))
        .route("/papers/{id}", get(handlers::papers::get_paper))
        .route("/papers/{id}", put(handlers::papers::update_paper))
        .route("/papers/{id}", delete(handlers::papers::delete_paper))
        .route("/papers/{id}/publish", post(handlers::papers::publish_paper))
        // 试卷题目管理
        .route("/papers/{paper_id}/questions", post(handlers::papers::add_question_to_paper))
        .route("/papers/{paper_id}/questions/{question_id}", put(handlers::papers::update_paper_question))
        .route("/papers/{paper_id}/questions/{question_id}", delete(handlers::papers::remove_question_from_paper))
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
