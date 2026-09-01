//! 导出端点（模块 A 的 HTTP 入口）
//!
//! T1.4 仅注册 `/export/markdown` 路由占位（契约见实施计划 §四）；
//! Markdown 生成器与警告通道（`Content-Disposition` RFC 5987 中文名 +
//! `X-Export-Warnings` 截断策略）于 T1.6-1.7 落地。

use axum::{extract::State, http::StatusCode, Extension, Json};
use serde_json::json;

use crate::auth::middleware::AuthUser;
use crate::export::model::ExamRequest;
use crate::AppState;

/// POST /api/v1/export/markdown — Markdown 导出（T1.6-1.7 实现）
pub async fn export_markdown(
    State(_state): State<AppState>,
    Extension(_auth): Extension<AuthUser>,
    Json(_req): Json<ExamRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "Markdown 导出尚未实现，将在 T1.6-1.7 落地",
            "code": "ERR_EXPORT_NOT_IMPLEMENTED"
        })),
    )
}
