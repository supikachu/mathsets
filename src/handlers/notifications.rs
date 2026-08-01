use std::convert::Infallible;
use std::time::Duration;

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::models::notification::{Notification, TicketInfo};
use crate::AppState;

// ===========================================================================
// SSE 一次性票据（安全认证层）
// ===========================================================================

/// POST /api/v1/notifications/ticket — 颁发 SSE 连接票据
///
/// 安全流程：
/// 1. 前端携带标准 `Authorization: Bearer <JWT>` 请求此接口
/// 2. 后端验证 JWT 后，生成 30s 有效期的一次性 ticket
/// 3. 前端拿到 ticket 后，用 `new EventSource('/notifications/stream?ticket=xxx')` 建立连接
/// 4. ticket 使用后立即销毁，防止重放攻击
///
/// 相比直接在 URL 中传 JWT 的优势：
/// - JWT 不会暴露在浏览器历史、代理日志、Nginx Access Log 中
/// - ticket 30s 过期 + 一次性使用，即使泄露也无法重放
pub async fn create_ticket(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let ticket_id = Uuid::new_v4();
    let ticket_info = TicketInfo::new(auth.id);

    state.sse_tickets.insert(ticket_id, ticket_info);

    Ok(Json(json!({
        "ticket": ticket_id.to_string(),
        "expires_in": 30,
    })))
}

/// SSE 流查询参数
#[derive(Deserialize)]
pub struct StreamQuery {
    pub ticket: String,
}

/// GET /api/v1/notifications/stream — SSE 实时通知流
///
/// 认证方式：URL 参数 `?ticket=xxx`（一次性票据，非 JWT）
///
/// 连接建立后：
/// - 实时推送该用户的通知事件（JSON 格式）
/// - 30s 间隔发送 `: heartbeat` 注释帧，防止代理/防火墙超时断连
/// - 客户端断开时，后台任务自动退出，释放资源
pub async fn stream(
    State(state): State<AppState>,
    Query(params): Query<StreamQuery>,
) -> Result<impl axum::response::IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    // 1. 解析 ticket UUID
    let ticket_id = Uuid::parse_str(&params.ticket).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "ticket 格式无效"})),
        )
    })?;

    // 2. 验证并销毁 ticket（一次性使用）
    let (_, ticket_info) = state.sse_tickets.remove(&ticket_id).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "ticket 无效或已使用"})),
        )
    })?;

    // 3. 检查是否过期
    if ticket_info.is_expired() {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "ticket 已过期"})),
        ))
    }

    let user_id = ticket_info.user_id;

    // 4. 订阅广播通道
    let mut rx = state.notify_tx.subscribe();

    // 5. 创建 mpsc 桥接通道（broadcast → mpsc，按 user_id 过滤）
    let (sse_tx, sse_rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(32);

    tokio::spawn(async move {
        tracing::info!("SSE 连接已建立, user_id={}", user_id);

        loop {
            match rx.recv().await {
                // 仅推送属于当前用户的通知
                Ok(event) if event.user_id == user_id => {
                    let json_str = serde_json::to_string(&event.notification)
                        .unwrap_or_else(|_| "{}".into());
                    let sse_event = Event::default().data(json_str);
                    if sse_tx.send(Ok(sse_event)).await.is_err() {
                        // 客户端已断开
                        break;
                    }
                }
                // 其他用户的通知，跳过
                Ok(_) => continue,
                // 广播通道积压（客户端处理过慢），丢弃旧消息并继续
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(
                        "SSE 广播积压, user_id={}, 丢弃 {} 条消息",
                        user_id,
                        n
                    );
                    continue;
                }
                // 广播通道已关闭（所有 sender dropped），退出
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    tracing::info!("广播通道已关闭, SSE 连接终止, user_id={}", user_id);
                    break;
                }
            }
        }

        tracing::info!("SSE 连接已关闭, user_id={}", user_id);
    });

    // 6. 返回 SSE 流（KeepAlive 自动发送 30s 心跳）
    let stream = ReceiverStream::new(sse_rx);
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("heartbeat"),
    ))
}

// ===========================================================================
// 通知 CRUD
// ===========================================================================

/// GET /api/v1/notifications — 通知列表（最新 50 条）
pub async fn list_notifications(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<Vec<Notification>>, (StatusCode, Json<serde_json::Value>)> {
    let notifications = sqlx::query_as::<_, Notification>(
        r#"
        SELECT id, user_id, kind, title, body, resource_type, resource_id, is_read, created_at
        FROM notifications
        WHERE user_id = $1
        ORDER BY created_at DESC
        LIMIT 50
        "#,
    )
    .bind(auth.id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询通知失败: {}", e)})),
        )
    })?;

    Ok(Json(notifications))
}

/// GET /api/v1/notifications/unread-count — 未读通知数量（铃铛红点）
pub async fn unread_count(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE user_id = $1 AND is_read = false",
    )
    .bind(auth.id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询未读数量失败: {}", e)})),
        )
    })?;

    Ok(Json(json!({"count": count})))
}

/// PUT /api/v1/notifications/{id}/read — 标记单条通知为已读
pub async fn mark_read(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let result = sqlx::query(
        "UPDATE notifications SET is_read = true WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(auth.id)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("标记已读失败: {}", e)})),
        )
    })?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "通知不存在或无权操作"})),
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/notifications/:id — 删除单条通知
///
/// 安全：WHERE user_id = auth.id 防越权删除他人通知
pub async fn delete_notification(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let result = sqlx::query(
        "DELETE FROM notifications WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(auth.id)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("删除通知失败: {}", e)})),
        )
    })?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "通知不存在或无权操作"})),
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// PUT /api/v1/notifications/read-all — 标记所有通知为已读
pub async fn mark_all_read(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    sqlx::query(
        "UPDATE notifications SET is_read = true WHERE user_id = $1 AND is_read = false",
    )
    .bind(auth.id)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("全部标记已读失败: {}", e)})),
        )
    })?;

    Ok(StatusCode::NO_CONTENT)
}

// ===========================================================================
// 内部工具函数（Phase 2 工作流事件注入时调用）
// ===========================================================================

/// 发送通知 — 持久化到数据库 + 广播到 SSE 通道
///
/// 此函数为 best-effort：通知发送失败不阻断主业务流程。
/// 调用方应使用 `if let Err(e) = send_notification(...) { tracing::warn!(...) }` 模式。
pub async fn send_notification(
    pool: &sqlx::PgPool,
    notify_tx: &tokio::sync::broadcast::Sender<crate::models::notification::BroadcastEvent>,
    req: crate::models::notification::CreateNotification,
) -> Result<(), sqlx::Error> {
    // 1. 持久化到数据库
    let notification = sqlx::query_as::<_, Notification>(
        r#"
        INSERT INTO notifications (user_id, kind, title, body, resource_type, resource_id)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, user_id, kind, title, body, resource_type, resource_id, is_read, created_at
        "#,
    )
    .bind(req.user_id)
    .bind(&req.kind)
    .bind(&req.title)
    .bind(req.body.as_deref())
    .bind(req.resource_type.as_deref())
    .bind(req.resource_id)
    .fetch_one(pool)
    .await?;

    // 2. 广播到 SSE 通道（失败时仅记录日志，不影响已持久化的通知）
    let event = crate::models::notification::BroadcastEvent {
        user_id: req.user_id,
        notification,
    };
    if notify_tx.send(event).is_err() {
        // 当前没有活跃的 SSE 连接，通知仅持久化在数据库中
        // 用户下次打开消息中心时仍能看到
        tracing::debug!(
            "SSE 广播无接收者（用户未连接），通知仅持久化, user_id={}",
            req.user_id
        );
    }

    Ok(())
}
