pub mod ai;
pub mod auth;
pub mod config;
pub mod db;
pub mod handlers;
pub mod models;

use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use tower::limit::GlobalConcurrencyLimitLayer;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::db::DbPool;

use std::sync::Arc;

/// 应用共享状态内部数据
pub struct AppStateInner {
    pub pool: DbPool,
    pub jwt_secret: String,
    pub jwt_expiry_hours: i64,
    pub ai_config: crate::config::AiConfig,
}

/// 应用共享状态（通过 Arc 包裹，Clone 成本为 O(1)）
#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<AppStateInner>,
}

impl std::ops::Deref for AppState {
    type Target = AppStateInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl AppState {
    pub fn new(
        pool: DbPool,
        jwt_secret: String,
        jwt_expiry_hours: i64,
        ai_config: crate::config::AiConfig,
    ) -> Self {
        Self {
            inner: Arc::new(AppStateInner {
                pool,
                jwt_secret,
                jwt_expiry_hours,
                ai_config,
            }),
        }
    }
}

/// 构建应用 Router，用于 main 启动和集成测试
pub fn build_app(state: AppState) -> Router {
    // ─── 需要 JWT 认证的保护路由 ───
    let protected_routes = Router::new()
        // 题库空间
        .route("/spaces", get(handlers::spaces::list_spaces))
        .route("/spaces", post(handlers::spaces::create_team_space))
        .route("/spaces/{id}", get(handlers::spaces::get_space_detail))
        .route("/spaces/{id}", put(handlers::spaces::update_space))
        .route("/spaces/{id}", delete(handlers::spaces::delete_space))
        .route("/spaces/{id}/members", post(handlers::spaces::add_member))
        .route("/spaces/{id}/members/{user_id}", put(handlers::spaces::update_member))
        .route(
            "/spaces/{id}/members/{user_id}",
            delete(handlers::spaces::remove_member),
        )
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
        // 管理员用户管理
        .route("/admin/users", get(handlers::auth::list_users))
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
        // 公共库流通
        .route(
            "/questions/{id}/contribute",
            post(handlers::questions::contribute_to_public),
        )
        .route(
            "/questions/{id}/import",
            post(handlers::questions::import_question),
        )
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
        // AI 智能录入
        .route("/ai/parse-text", post(handlers::ai::parse_text))
        .route("/ai/settings", get(handlers::ai::get_settings))
        .route("/ai/settings", put(handlers::ai::update_settings))
        // parse-image 路由单独套全局限流层 + body 限制（补丁六后端：全局最多 10 个并发 OCR 请求）
        .merge(
            Router::new()
                .route("/ai/parse-image", post(handlers::ai::parse_image))
                .layer(GlobalConcurrencyLimitLayer::new(10))
                .layer(DefaultBodyLimit::max(10 * 1024 * 1024)),
        )
        // 标签管理
        .route("/tags", get(handlers::tags::list_tags))
        .route("/tags", post(handlers::tags::create_tag))
        .route("/tags/suggest", get(handlers::tags::suggest_tags))
        .route("/tags/{id}", put(handlers::tags::update_tag))
        .route("/tags/{id}", delete(handlers::tags::delete_tag))
        .route("/tags/{id}/merge", post(handlers::tags::merge_tag))
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
