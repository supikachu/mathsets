pub mod ai;
pub mod auth;
pub mod config;
pub mod db;
pub mod handlers;
pub mod models;
pub mod util;
pub mod workers;

use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::db::DbPool;

use std::sync::Arc;
use uuid::Uuid;

/// 应用共享状态内部数据
pub struct AppStateInner {
    pub pool: DbPool,
    pub jwt_secret: String,
    pub jwt_expiry_hours: i64,
    pub ai_config: crate::config::AiConfig,
    /// 文件上传根目录（如 ./uploads），头像等用户文件落盘位置
    pub upload_dir: String,
    /// SSE 广播通道 — 工作流事件通过此通道推送到所有 SSE 连接
    pub notify_tx: tokio::sync::broadcast::Sender<crate::models::notification::BroadcastEvent>,
    /// SSE 一次性票据存储（30s TTL，使用后立即销毁）
    pub sse_tickets: Arc<dashmap::DashMap<Uuid, crate::models::notification::TicketInfo>>,
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
        upload_dir: String,
    ) -> Self {
        // 广播通道：容量 256，超出时旧消息被丢弃（SSE 客户端会收到 Lagged 警告）
        let (notify_tx, _) = tokio::sync::broadcast::channel(256);
        let sse_tickets = Arc::new(dashmap::DashMap::new());

        Self {
            inner: Arc::new(AppStateInner {
                pool,
                jwt_secret,
                jwt_expiry_hours,
                ai_config,
                upload_dir,
                notify_tx,
                sse_tickets,
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
        .route(
            "/spaces/{id}/transfer/{target_user_id}",
            put(handlers::spaces::transfer_ownership),
        )
        .route("/spaces/{id}/leave", delete(handlers::spaces::leave_space))
        // 知识树（KnowledgeTree）— B3 新增：多棵树容器（数学知识/能力/教材章节）
        .route("/knowledge-trees", get(handlers::knowledge_trees::list_knowledge_trees))
        .route("/knowledge-trees", post(handlers::knowledge_trees::create_knowledge_tree))
        .route("/knowledge-trees/{id}", put(handlers::knowledge_trees::update_knowledge_tree))
        .route("/knowledge-trees/{id}", delete(handlers::knowledge_trees::delete_knowledge_tree))
        // 知识点节点（KnowledgeNode）— B3 新增：基于 LTREE 的物化路径树
        .route("/knowledge-trees/{tree_id}/nodes", get(handlers::knowledge_nodes::list_nodes_by_tree))
        .route("/knowledge-trees/{tree_id}/nodes/tree", get(handlers::knowledge_nodes::get_node_tree))
        .route("/knowledge-nodes", post(handlers::knowledge_nodes::create_node))
        .route("/knowledge-nodes/{id}", get(handlers::knowledge_nodes::get_node))
        .route("/knowledge-nodes/{id}", put(handlers::knowledge_nodes::update_node))
        .route("/knowledge-nodes/{id}", delete(handlers::knowledge_nodes::delete_node))
        .route("/knowledge-nodes/{id}/descendants", get(handlers::knowledge_nodes::get_descendants))
        .route("/knowledge-nodes/{id}/move", post(handlers::knowledge_nodes::move_node))
        // V2.1.1 标签治理：canonical 合并（环检测 + 审计，不物理删除）
        .route(
            "/knowledge-nodes/{id}/merge",
            post(handlers::tag_governance::merge_knowledge_node),
        )
        // V2.1.1 标签候选审核队列（仅管理员）
        .route(
            "/admin/tag-candidates",
            get(handlers::tag_governance::list_tag_candidates),
        )
        .route(
            "/admin/tag-candidates/{id}",
            get(handlers::tag_governance::get_tag_candidate),
        )
        .route(
            "/admin/tag-candidates/{id}/approve",
            post(handlers::tag_governance::approve_tag_candidate),
        )
        .route(
            "/admin/tag-candidates/{id}/reject",
            post(handlers::tag_governance::reject_tag_candidate),
        )
        // V2.1.1 标签使用情况
        .route("/tags/{id}/usage", get(handlers::tag_governance::get_tag_usage))
        // AI 智能打标 — B3 新增：LLM 提取 + pg_trgm/JSONB 三级模糊匹配
        .route("/questions/ai-tagging", post(handlers::ai_tagging::ai_tagging))
        // 当前用户信息
        .route("/auth/me", get(handlers::auth::me))
        // 用户中心（个人资料 + 头像 + 密码）
        .route(
            "/users/me",
            get(handlers::users::get_my_profile).put(handlers::users::update_my_profile),
        )
        .route("/users/password", put(handlers::users::change_password))
        // 头像上传单独套 2MB 限制
        .merge(
            Router::new()
                .route("/users/avatar", post(handlers::users::upload_avatar))
                .layer(DefaultBodyLimit::max(2 * 1024 * 1024)),
        )
        // 题目配图上传单独套 10MB 限制（几何大图/坐标系需更高上限）
        .merge(
            Router::new()
                .route("/uploads/images", post(handlers::uploads::upload_image))
                .layer(DefaultBodyLimit::max(10 * 1024 * 1024)),
        )
        // 管理员用户管理
        .route("/admin/users", get(handlers::auth::list_users).post(handlers::auth::create_user))
        .route("/admin/users/{id}", get(handlers::auth::get_user).delete(handlers::auth::delete_user))
        .route("/admin/users/{id}/role", put(handlers::auth::update_user_role))
        .route("/admin/users/{id}/status", put(handlers::auth::update_user_status))
        // 题目统计（必须在 {id} 之前注册）
        .route("/questions/stats", get(handlers::questions::question_stats))
        // 待补全计数（必须在 {id} 之前注册）
        .route(
            "/questions/incomplete-count",
            get(handlers::questions::incomplete_count),
        )
        // 批量提交审核（必须在 {id} 之前注册）
        .route(
            "/questions/batch-submit",
            post(handlers::questions::batch_submit_questions),
        )
        // 题目 CRUD
        .route("/questions", get(handlers::questions::list_questions))
        .route("/questions", post(handlers::questions::create_question))
        .route("/questions/{id}", get(handlers::questions::get_question))
        .route("/questions/{id}", put(handlers::questions::update_question))
        .route("/questions/{id}", delete(handlers::questions::delete_question))
        // 教研状态机（Draft → Pending → Published / Rejected → Draft）
        .route(
            "/questions/{id}/submit",
            post(handlers::questions::submit_for_review),
        )
        .route(
            "/questions/{id}/approve",
            post(handlers::questions::approve_question),
        )
        .route(
            "/questions/{id}/reject",
            post(handlers::questions::reject_question),
        )
        // 公共库流通
        .route(
            "/questions/{id}/contribute",
            post(handlers::questions::contribute_to_public),
        )
        .route(
            "/questions/{id}/import",
            post(handlers::questions::import_question),
        )
        // 推库申请（公共题库终审流程）
        .route(
            "/questions/{id}/submit-to-public",
            post(handlers::public_library::submit_to_public),
        )
        .route(
            "/questions/{id}/public-submission",
            get(handlers::public_library::get_question_submission_status),
        )
        .route(
            "/public-library/pending",
            get(handlers::public_library::list_pending),
        )
        .route(
            "/public-library/{id}/review",
            post(handlers::public_library::review_submission),
        )
        .route(
            "/public-library/{id}",
            delete(handlers::public_library::withdraw_submission),
        )
        // 跨空间克隆题目（深拷贝 + 强制 Draft + origin_question_id）
        .route(
            "/questions/{id}/clone",
            post(handlers::spaces::clone_question),
        )
        // 反向查询：题目被引用的试卷列表
        .route(
            "/questions/{id}/papers",
            get(handlers::papers::get_question_papers),
        )
        // V2.1.1 统一来源视图（Document → Paper/Collection → Question）
        .route(
            "/questions/{id}/sources",
            get(handlers::papers::get_question_sources),
        )
        // V2.1.1 数据质量概览（仅管理员）
        .route(
            "/admin/data-quality/summary",
            get(handlers::admin::data_quality_summary),
        )
        // 试卷 CRUD
        .route("/papers", get(handlers::papers::list_papers))
        .route("/papers", post(handlers::papers::create_paper))
        // 试卷轻量列表（仅 id + title，供下拉选择，必须放在 /papers/{id} 之前）
        .route("/papers/brief", get(handlers::papers::list_papers_brief))
        .route("/papers/{id}", get(handlers::papers::get_paper))
        .route("/papers/{id}", put(handlers::papers::update_paper))
        .route("/papers/{id}", delete(handlers::papers::delete_paper))
        .route("/papers/{id}/publish", post(handlers::papers::publish_paper))
        // 试卷题目管理
        .route("/papers/{paper_id}/questions", post(handlers::papers::add_question_to_paper))
        .route("/papers/{paper_id}/questions/{question_id}", put(handlers::papers::update_paper_question))
        .route("/papers/{paper_id}/questions/{question_id}", delete(handlers::papers::remove_question_from_paper))
        // V2.1.1 题目集合（QuestionCollection）
        .route("/collections", get(handlers::collections::list_collections))
        .route("/collections/{id}", get(handlers::collections::get_collection))
        .route(
            "/collections/{id}/questions/batch",
            post(handlers::collections::batch_add_questions),
        )
        .route(
            "/collections/{id}/questions/{question_id}",
            delete(handlers::collections::remove_collection_question),
        )
        // AI 智能录入（设置）
        .route("/ai/settings", get(handlers::ai::get_settings))
        .route("/ai/settings", put(handlers::ai::update_settings))
        // OCR 引擎连接测试（轻量探测，不消耗配额）
        .route("/ai/ocr/test-connection", post(handlers::ai::test_ocr_connection))
        // V2.1.1 异步解析任务（POST 创建 + GET 进度 + 取消）
        .route("/ai/parse-task", post(handlers::ai_tasks::submit_parse_task))
        .route(
            "/ai/parse-task/{id}",
            get(handlers::ai_tasks::get_task_status),
        )
        .route(
            "/ai/parse-task/{id}/cancel",
            post(handlers::ai_tasks::cancel_task),
        )
        // V2.1.1 资料/Document：上传页图集 + 列表 + 详情
        .route("/ai/documents", get(handlers::documents::list_documents))
        .route("/ai/documents/{id}", get(handlers::documents::get_document))
        .route(
            "/ai/documents/{id}/classify",
            post(handlers::documents::classify_document),
        )
        .route(
            "/ai/documents/{id}/confirm",
            post(handlers::documents::confirm_document),
        )
        // 资料上传单独套大体积限制（≤30 页 × 10MB）
        .merge(
            Router::new()
                .route("/ai/documents", post(handlers::documents::upload_document))
                .layer(DefaultBodyLimit::max(80 * 1024 * 1024)),
        )
        // 标签管理
        .route("/tags", get(handlers::tags::list_tags))
        .route("/tags", post(handlers::tags::create_tag))
        .route("/tags/suggest", get(handlers::tags::suggest_tags))
        .route("/tags/{id}", put(handlers::tags::update_tag))
        .route("/tags/{id}", delete(handlers::tags::delete_tag))
        .route("/tags/{id}/merge", post(handlers::tags::merge_tag))
        // 通知系统（ticket 颁发 + 列表 + 已读标记）
        .route("/notifications/ticket", post(handlers::notifications::create_ticket))
        .route("/notifications", get(handlers::notifications::list_notifications))
        .route("/notifications/unread-count", get(handlers::notifications::unread_count))
        .route("/notifications/read-all", put(handlers::notifications::mark_all_read))
        .route("/notifications/{id}/read", put(handlers::notifications::mark_read))
        .route(
            "/notifications/{id}",
            delete(handlers::notifications::delete_notification),
        )
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
        // SSE 通知流（无需 JWT，使用一次性 ticket 认证）
        .route(
            "/api/v1/notifications/stream",
            get(handlers::notifications::stream),
        )
        // API v1 保护模块（需要 JWT）
        .nest("/api/v1", protected_routes)
        // 静态文件服务（用户头像等）— 无需认证，直接通过 URL 访问
        .nest_service("/uploads", ServeDir::new(&state.upload_dir))
        // 全局中间件
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
