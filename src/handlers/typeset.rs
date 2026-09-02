//! 排版参数端点（T3.7）— 只读预设，不渲染
//!
//! R1 把 PDF 出口唯一化到 `POST /export/pdf`：不给渲染单独开一条 HTTP 通道，否则同一套版式
//! 会有两条会漂移的流水线（认证、素材预取、警告头各一份）。本模块因此只剩「预设下拉要什么」。

use axum::Json;

use crate::typeset::spec::{ProfilePreset, presets};

/// GET /api/v1/typeset/profiles — 内置版面预设（§6.1 四套，每套带完整 `spec`）
///
/// 前端「先选预设再微调」的数据源：改过的字段整体回传 `ExamRequest.spec` 即可（T3.3 的口径
/// 是请求带 spec 就整体替换预设）。`spec.profile` 告诉前端每套预设属于哪个输出口径。
pub async fn list_profiles() -> Json<Vec<ProfilePreset>> {
    Json(presets())
}
