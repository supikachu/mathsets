//! 排版端点（T3.7 预设只读 + T5.2 预览）
//!
//! R1 把**文件出口**唯一化到 `POST /export/pdf`：不给渲染单独开一条出字节的 HTTP 通道，否则
//! 同一套版式会有两条会漂移的流水线（认证、素材预取、警告头各一份）。预览不在此列 —— 它不出
//! 文件、不是交付物，而是「印之前看一眼 + 一份预检清单」，消费的是同一条链的中间产物（一次编译
//! 的帧树），因此它落在本模块，但**编译走的还是 `export::pdf::generate_preview`**（R12）。

use std::path::Path;

use axum::{
    Extension, Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::json;
use ts_rs::TS;

use crate::AppState;
use crate::auth::middleware::AuthUser;
use crate::export::assembler::assemble_exam;
use crate::export::model::{ExamRequest, Issue};
use crate::export::pdf::{build_layout_doc, generate_preview};
use crate::handlers::export::collect_question_issues;
use crate::handlers::questions::db_err;
use crate::typeset::spec::{ProfilePreset, presets};

/// `POST /typeset/preview` 的响应（§6.5）
///
/// 裸载荷、无信封，与 `/export/*` 的文件响应一致。`pages` 的下标是**物理页**：A3 对折卷一张纸
/// 两个逻辑页（R4），预览里就是一页纸上左右两栏，教师看到的就是要印的东西。
#[derive(Debug, Serialize, TS)]
#[ts(export, export_to = "../frontend/src/api/types/layout.ts")]
pub struct PreviewResponse {
    /// 逐页 SVG 源码，自包含（字形已描边，浏览器无需装字体），代价是文字不可选中
    pub pages: Vec<String>,
    pub page_count: usize,
    /// 生成期问题（装配、素材、公式降级）+ 印前预检发现（T5.1），全部结构化
    pub issues: Vec<Issue>,
    /// typst 自己的告警原文：给人看即可，不值得编成 `Issue`
    pub warnings: Vec<String>,
}

/// GET /api/v1/typeset/profiles — 内置版面预设（§6.1 四套，每套带完整 `spec`）
///
/// 前端「先选预设再微调」的数据源：改过的字段整体回传 `ExamRequest.spec` 即可（T3.3 的口径
/// 是请求带 spec 就整体替换预设）。`spec.profile` 告诉前端每套预设属于哪个输出口径。
pub async fn list_profiles() -> Json<Vec<ProfilePreset>> {
    Json(presets())
}

/// POST /api/v1/typeset/preview — 逐页 SVG 预览 + 印前预检（T5.2）
///
/// 请求体与 `/export/pdf` 完全相同（[`ExamRequest`]），换的只是出口：装配 → `LayoutDoc` →
/// **编译一次** → 帧树（喂预检）+ 逐页 SVG。预览与导出因此不可能排出两份不同的卷（R12）。
///
/// 坏公式、坏图、溢流、低清位图全都只记进 `issues`；唯一开天窗的还是 typst 编译本身，那时
/// 与 PDF 出口同样回 500 —— 给教师看半张卷子比告诉他失败了更坏。
pub async fn preview(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Json(req): Json<ExamRequest>,
) -> Response {
    let assembled = match assemble_exam(&state.pool, &auth, &req).await {
        Ok(a) => a,
        Err(e) => return db_err(e.to_string()).into_response(),
    };
    let doc = build_layout_doc(&assembled.bundle, &req.options, req.spec.as_ref());

    let result = match generate_preview(&doc, Path::new(&state.upload_dir)).await {
        Ok(r) => r,
        Err(e) => return preview_failed(e.summary()).into_response(),
    };

    let mut issues = assembled.issues;
    collect_question_issues(&assembled.bundle, &mut issues);
    issues.extend(doc.issues.iter().cloned());
    issues.extend(result.issues);
    let page_count = result.pages.len();

    Json(PreviewResponse {
        pages: result.pages,
        page_count,
        issues,
        warnings: result.warnings,
    })
    .into_response()
}

/// 预览失败：与 PDF 出口同一个错误码，措辞不同（预览没有「改用其他格式」这条退路）
fn preview_failed(detail: String) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!("预览排版失败: {detail}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "预览排版失败，请稍后重试",
            "code": "ERR_TYPESET_COMPILE_FAILED",
            "detail": detail,
        })),
    )
}

// 载荷大小记在这儿备查：字形描边后一页 SVG 约 200KB，二十页的卷子响应 ~4MB，而本服务没有
// 挂压缩层。T5.4 的基准要把它算进去，T5.5 的前端则应当逐页取用（滚动到哪页渲染哪页），
// 不要一次性把整卷塞进 DOM。
