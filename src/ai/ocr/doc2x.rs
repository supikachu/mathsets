//! Doc2X OCR 引擎（M2 新增，v2 API 迁移）
//!
//! 官方文档：https://doc2x.noedgeai.com/help/zh-cn/api/
//!
//! ## 鉴权
//! 所有请求 header 携带 `Authorization: Bearer sk-xxx`。
//!
//! ## 图片 OCR（同步）
//! POST `/api/v2/parse/img/layout`，body 为**原始二进制**（非 base64、非 formdata）。
//! 响应：`{code:"success", data:{result:{pages:[{md:"..."}]}, uid:"..."}}`，
//! 拼接所有 `pages[].md`。
//!
//! ## PDF OCR（异步 submit→poll）
//! 1. POST `/api/v2/parse/preupload`（仅需 auth header，body 可选 `{"model":"v2"|"v3-2026"|留空}`）
//!    → `{data:{uid, url}}`
//! 2. HTTP PUT 文件二进制到 `url`（**不带 auth header**，OSS 直传，url 仅可使用一次）
//! 3. 轮询 GET `/api/v2/parse/status?uid=xxx`（每 3s）→
//!    `{code:"success", data:{status:"processing"|"success"|"failed",
//!    result:{pages:[{md:"..."}]}, progress, detail}}`，超时 120s。
//!
//! ## base_url 约定
//! 默认 `https://v2.doc2x.noedgeai.com`（裸域名，不含版本前缀），
//! 各接口路径需显式含 `/api/v2` 前缀。官方已弃用旧 `noedgex.com/v1` 域名。

use async_trait::async_trait;
use base64::Engine as _;
use reqwest::Client;
use serde::Deserialize;
use std::time::{Duration, Instant};

use super::{OcrError, OcrProvider};

/// Doc2X OCR 引擎
pub struct Doc2XProvider {
    api_key: String,
    base_url: String,
    client: Client,
}

impl Doc2XProvider {
    pub fn new(api_key: String, base_url: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .no_proxy() // 与 DeepSeekProvider 一致：绕过系统代理
            .build()
            .expect("无法创建 reqwest Client");
        Self {
            api_key,
            base_url,
            client,
        }
    }

    /// 构造 Authorization Bearer header 值
    fn auth_header(&self) -> String {
        format!("Bearer {}", self.api_key)
    }

    /// 拼接 base_url 与相对路径（自动处理多余斜杠）
    fn url(&self, path: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        if path.starts_with('?') {
            format!("{base}{path}")
        } else {
            format!("{base}{}", path)
        }
    }
}

// ---------------------------------------------------------------------------
// 响应类型（v2 鲁棒性策略）
//
// - 仅 `pages[].md` 为核心提取目标，其余字段全部 Option + `#[serde(default)]`
// - 不加 `#[serde(deny_unknown_fields)]`，未知字段由 serde 默认忽略
// - `data` 也设为 Option，缺失或非对象时不阻断反序列化，由调用方做兜底
// - 派生 `Debug` 便于 tracing 日志打印
// ---------------------------------------------------------------------------

/// `/api/v2/parse/img/layout` 同步响应
#[derive(Deserialize, Debug)]
struct ImgLayoutResponse {
    #[serde(default)]
    code: Option<String>,
    /// data 缺失或非对象时为 None / Null
    #[serde(default)]
    data: Option<serde_json::Value>,
    #[serde(default)]
    msg: Option<String>,
}

/// `/api/v2/parse/preupload` 响应：`{code, data:{uid, url}}`
#[derive(Deserialize, Debug)]
struct PreuploadResponse {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    data: Option<PreuploadData>,
    #[serde(default)]
    msg: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
struct PreuploadData {
    #[serde(default)]
    uid: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

/// `/api/v2/parse/status` 响应
#[derive(Deserialize, Debug)]
struct StatusResponse {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    data: Option<StatusData>,
    #[serde(default)]
    msg: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
struct StatusData {
    #[serde(default)]
    status: Option<String>,
    /// success 时才有 result.pages
    #[serde(default)]
    result: Option<StatusResult>,
    /// 进度（0~100），仅 processing 时有意义
    #[serde(default)]
    progress: Option<serde_json::Value>,
    /// 失败原因，仅 failed 时有意义
    #[serde(default)]
    detail: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
struct StatusResult {
    #[serde(default)]
    pages: Vec<StatusPage>,
}

#[derive(Deserialize, Debug, Default)]
struct StatusPage {
    #[serde(default)]
    md: Option<String>,
}

// ---------------------------------------------------------------------------
// 辅助：从 JSON Value 中提取 pages[].md 并拼接
// ---------------------------------------------------------------------------

/// 拼接多页 Markdown 字符串（共享逻辑）
///
/// 空列表返回错误（Doc2X 应至少返回一页内容）。
fn join_md_strings(md_list: Vec<String>) -> Result<String, OcrError> {
    if md_list.is_empty() {
        return Err(OcrError::Upstream(0, "Doc2X 返回空 Markdown".to_string()));
    }
    Ok(md_list.join("\n\n"))
}

/// 截断字符串用于日志打印，避免日志过长
fn truncate_for_log(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}...(truncated, total {count} chars)")
    }
}

/// 剥离 data URI 前缀（如 `data:image/png;base64,`），返回纯 base64 字符串
///
/// 兼容前端 canvas.toDataURL() 输出的 data URI 格式；
/// 输入已是纯 base64（无前缀）时原样返回。
fn strip_data_uri_prefix(s: &str) -> String {
    if let Some(idx) = s.find(";base64,") {
        // 找到 ;base64, 后的部分（idx 是 ";base64," 起始位置，+8 跳过 ";base64,"）
        s[idx + 8..].to_string()
    } else if s.starts_with("data:") {
        // 极端情况：data: 开头但无 ;base64,（不该出现，但兜底处理）
        if let Some(comma) = s.find(',') {
            s[comma + 1..].to_string()
        } else {
            s.to_string()
        }
    } else {
        s.to_string()
    }
}

/// 根据图片二进制前几个字节（magic bytes）检测 MIME 类型
///
/// Doc2X 官方 curl 示例虽未显式带 Content-Type，但 reqwest 默认发送
/// `application/octet-stream`，可能影响 Doc2X 对图片格式的识别。
/// 显式设置正确的 Content-Type 有助于上游正确解析。
///
/// 支持 JPEG / PNG / WebP，未知格式返回 `application/octet-stream`。
fn detect_content_type(bytes: &[u8]) -> &'static str {
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        "image/jpeg"
    } else if bytes.len() >= 8
        && bytes[0..4] == [0x89, 0x50, 0x4E, 0x47]
        && bytes[4..8] == [0x0D, 0x0A, 0x1A, 0x0A]
    {
        "image/png"
    } else if bytes.len() >= 12
        && bytes[0..4] == [0x52, 0x49, 0x46, 0x46] // "RIFF"
        && bytes[8..12] == [0x57, 0x45, 0x42, 0x50]
    {
        // "WEBP"
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

/// 把 reqwest::Error 的 source chain 拼成字符串，便于诊断 DNS/TLS/连接层失败根因
///
/// reqwest::Error 的 Display 较简略，Debug 冗长；source chain 才包含
/// hyper::Error / std::io::Error / openssl::ssl::Error 等真实根因。
fn format_reqwest_error_chain(e: &reqwest::Error) -> String {
    let mut chain = String::new();
    let mut current: Option<&dyn std::error::Error> = Some(e);
    let mut depth = 0;
    while let Some(err) = current {
        if depth > 0 {
            chain.push_str(" → ");
        }
        chain.push_str(&format!("{err}"));
        current = err.source();
        depth += 1;
        if depth > 8 {
            chain.push_str(" → ...(too deep)");
            break;
        }
    }
    chain
}

/// 在 tracing::error! 中打印 reqwest 错误的完整诊断信息
///
/// `context` 是业务上下文（如 "Doc2X img/layout POST"），用于在日志中定位调用点。
fn log_reqwest_error(context: &str, url: &str, e: &reqwest::Error) {
    tracing::error!(
        "{} 失败: url={}, is_timeout={}, is_connect={}, is_request={}, is_decode={}, \
         is_redirect={}, is_status={}, is_body={}, source_chain={}",
        context,
        url,
        e.is_timeout(),
        e.is_connect(),
        e.is_request(),
        e.is_decode(),
        e.is_redirect(),
        e.is_status(),
        e.is_body(),
        format_reqwest_error_chain(e)
    );
}

/// 从 `/api/v2/parse/img/layout` 响应的 `data` 对象中提取 `result.pages[].md` 并拼接
///
/// `data` 为 None / Null / 非对象时返回错误。
fn extract_pages_md_from_value(data: Option<&serde_json::Value>) -> Result<String, OcrError> {
    let data = data.unwrap_or(&serde_json::Value::Null);
    let pages = data
        .get("result")
        .and_then(|r| r.get("pages"))
        .and_then(|p| p.as_array())
        .ok_or_else(|| {
            OcrError::Upstream(0, "Doc2X 响应缺少 data.result.pages".to_string())
        })?;

    let md: Vec<String> = pages
        .iter()
        .filter_map(|p| p.get("md").and_then(|m| m.as_str()).map(|s| s.to_string()))
        .collect();

    join_md_strings(md)
}

/// 从 `/api/v2/parse/status` success 响应的 `result.pages` 结构体提取并拼接 `md` 字段
fn extract_pages_md_from_struct(result: &StatusResult) -> Result<String, OcrError> {
    let md: Vec<String> = result
        .pages
        .iter()
        .filter_map(|p| p.md.clone())
        .filter(|s| !s.is_empty())
        .collect();
    join_md_strings(md)
}

#[async_trait]
impl OcrProvider for Doc2XProvider {
    fn id(&self) -> &'static str {
        "doc2x"
    }

    /// Doc2X 原生支持 PDF 直传（走 ocr_pdf_async 异步路径）
    fn supports_pdf(&self) -> bool {
        true
    }

    /// 单图 OCR → Markdown
    ///
    /// `image_b64` 为 base64 编码的图片数据，先解码为原始二进制后以 raw body 发送。
    /// 兼容 `data:image/...;base64,xxxx` 前缀（前端可能传入 data URI）。
    async fn ocr_image(&self, image_b64: &str) -> Result<String, OcrError> {
        // 自动剥离 data URI 前缀（data:image/png;base64,）
        let pure_b64 = strip_data_uri_prefix(image_b64);

        let image_bytes = match base64::engine::general_purpose::STANDARD.decode(pure_b64.as_bytes()) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(
                    "Doc2X base64 解码失败: err={}, input_len={}, pure_b64_len={}, \
                     input_preview={:?}, pure_b64_preview={:?}",
                    e,
                    image_b64.len(),
                    pure_b64.len(),
                    truncate_for_log(image_b64, 100),
                    truncate_for_log(&pure_b64, 100)
                );
                return Err(OcrError::Upstream(0, format!("base64 解码失败: {e}")));
            }
        };

        let content_type = detect_content_type(&image_bytes);

        // Doc2X 官方仅支持 JPEG/PNG 二进制（见官方文档 img/layout 接口），
        // 前端 imageCompressor 为保真使用 WebP 0.95（补丁九），需在此转码为 PNG。
        // Qwen-VL 等其他 OCR 引擎不受影响（各自实现支持 WebP）。
        let (post_bytes, post_content_type) = if content_type == "image/webp" {
            tracing::debug!(
                "Doc2X 检测到 WebP 图片，转码为 PNG（Doc2X 不支持 WebP 格式），原始 {} 字节",
                image_bytes.len()
            );
            let img = image::load_from_memory(&image_bytes).map_err(|e| {
                tracing::error!(
                    "Doc2X WebP 解码失败: err={}, image_bytes_len={}",
                    e,
                    image_bytes.len()
                );
                OcrError::Upstream(0, format!("WebP 解码失败: {e}"))
            })?;
            let mut png_bytes = Vec::new();
            img.write_to(
                &mut std::io::Cursor::new(&mut png_bytes),
                image::ImageFormat::Png,
            )
            .map_err(|e| {
                tracing::error!("Doc2X PNG 编码失败: err={}", e);
                OcrError::Upstream(0, format!("PNG 编码失败: {e}"))
            })?;
            tracing::debug!(
                "Doc2X WebP→PNG 转码完成: WebP {} 字节 → PNG {} 字节",
                image_bytes.len(),
                png_bytes.len()
            );
            (png_bytes, "image/png")
        } else {
            (image_bytes, content_type)
        };

        tracing::debug!(
            "Doc2X img/layout 准备发送: image_bytes_len={}, base_url={}, content_type={}, \
             magic_bytes_hex={}",
            post_bytes.len(),
            self.base_url,
            post_content_type,
            post_bytes
                .iter()
                .take(16)
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );

        let url = self.url("/api/v2/parse/img/layout");
        let resp = self
            .client
            .post(&url)
            .header("Authorization", self.auth_header())
            .header("Content-Type", post_content_type)
            .body(post_bytes)
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) if e.is_timeout() => return Err(OcrError::Timeout),
            Err(e) => {
                log_reqwest_error("Doc2X img/layout POST", &url, &e);
                return Err(OcrError::Upstream(0, format!("{e}")));
            }
        };

        let status = resp.status().as_u16();
        // 401/403 → API Key 无效或权限不足
        if status == 401 || status == 403 {
            let _body = resp.text().await.unwrap_or_default();
            return Err(OcrError::NoApiKey);
        }
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(OcrError::Upstream(status, body));
        }

        // 先取原始文本，再反序列化；失败时打印原始响应以便排查结构差异
        let raw_text = resp.text().await.unwrap_or_default();
        let parsed: ImgLayoutResponse = match serde_json::from_str(&raw_text) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(
                    "Doc2X img/layout JSON 反序列化失败, status={}, err={}, 原始响应: {}",
                    status,
                    e,
                    truncate_for_log(&raw_text, 2000)
                );
                return Err(OcrError::Upstream(
                    status,
                    format!("响应反序列化失败: {e}"),
                ));
            }
        };

        if parsed.code.as_deref() != Some("success") {
            let msg = parsed.msg.clone().unwrap_or_else(|| "未知错误".to_string());
            let code = parsed.code.clone().unwrap_or_else(|| "<missing>".to_string());
            tracing::error!(
                "Doc2X img/layout 业务失败: code={}, msg={}, status={}, 原始响应: {}",
                code,
                msg,
                status,
                truncate_for_log(&raw_text, 2000)
            );
            return Err(OcrError::Upstream(0, msg));
        }

        // data.result.pages 路径提取
        extract_pages_md_from_value(parsed.data.as_ref())
    }

    /// PDF 异步 OCR → 全文 Markdown（无进度回调）
    ///
    /// submit (preupload) → PUT 上传 → poll status
    async fn ocr_pdf_async(&self, pdf_bytes: &[u8]) -> Result<String, OcrError> {
        self.ocr_pdf_async_inner(pdf_bytes, None).await
    }

    /// PDF 异步 OCR（带进度回调）：processing 期间每 3s 上报 0~100 百分比
    async fn ocr_pdf_async_with_progress(
        &self,
        pdf_bytes: &[u8],
        on_progress: &crate::ai::ocr::PdfProgressCallback,
    ) -> Result<String, OcrError> {
        self.ocr_pdf_async_inner(pdf_bytes, Some(on_progress)).await
    }
}

impl Doc2XProvider {
    /// PDF 直传实现主体
    ///
    /// submit (preupload) → PUT 上传 → poll status；
    /// `on_progress` 存在时在 processing 轮询分支上报 `data.progress` 百分比。
    async fn ocr_pdf_async_inner(
        &self,
        pdf_bytes: &[u8],
        on_progress: Option<&crate::ai::ocr::PdfProgressCallback>,
    ) -> Result<String, OcrError> {
        // ── Step 1：preupload 获取上传 url ──
        let preupload_url = self.url("/api/v2/parse/preupload");
        let resp = self
            .client
            .post(&preupload_url)
            .header("Authorization", self.auth_header())
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) if e.is_timeout() => return Err(OcrError::Timeout),
            Err(e) => {
                log_reqwest_error("Doc2X preupload POST", &preupload_url, &e);
                return Err(OcrError::Upstream(0, format!("{e}")));
            }
        };

        let status = resp.status().as_u16();
        if status == 401 || status == 403 {
            let _body = resp.text().await.unwrap_or_default();
            return Err(OcrError::NoApiKey);
        }
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(OcrError::Upstream(status, body));
        }

        // 先取原始文本，再反序列化；失败时打印原始响应以便排查结构差异
        let raw_text = resp.text().await.unwrap_or_default();
        let pre: PreuploadResponse = match serde_json::from_str(&raw_text) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(
                    "Doc2X preupload JSON 反序列化失败, status={}, err={}, 原始响应: {}",
                    status,
                    e,
                    truncate_for_log(&raw_text, 2000)
                );
                return Err(OcrError::Upstream(
                    status,
                    format!("preupload 反序列化失败: {e}"),
                ));
            }
        };
        if pre.code.as_deref() != Some("success") {
            let msg = pre.msg.unwrap_or_else(|| "preupload 失败".to_string());
            return Err(OcrError::Upstream(0, msg));
        }
        let data = pre.data.unwrap_or_default();
        let uid = data.uid.ok_or_else(|| {
            OcrError::Upstream(0, "preupload 响应缺少 data.uid".to_string())
        })?;
        let upload_url = data.url.ok_or_else(|| {
            OcrError::Upstream(0, "preupload 响应缺少 data.url".to_string())
        })?;

        // ── Step 2：PUT 上传 PDF 二进制（不带 auth header） ──
        let put_resp = self
            .client
            .put(&upload_url)
            .body(pdf_bytes.to_vec())
            .send()
            .await;

        let put_resp = match put_resp {
            Ok(r) => r,
            Err(e) if e.is_timeout() => return Err(OcrError::Timeout),
            Err(e) => {
                log_reqwest_error("Doc2X PUT 上传 PDF 到 OSS", &upload_url, &e);
                return Err(OcrError::Upstream(0, format!("上传 PDF 失败: {e}")));
            }
        };
        if !put_resp.status().is_success() {
            let code = put_resp.status().as_u16();
            let body = put_resp.text().await.unwrap_or_default();
            return Err(OcrError::Upstream(code, format!("上传 PDF 失败: {body}")));
        }

        // ── Step 3：轮询 status（每 3s，超时 120s） ──
        let start = Instant::now();
        let timeout = Duration::from_secs(120);
        let poll_url = self.url(&format!("/api/v2/parse/status?uid={uid}"));

        loop {
            if start.elapsed() >= timeout {
                return Err(OcrError::Timeout);
            }

            let resp = self
                .client
                .get(&poll_url)
                .header("Authorization", self.auth_header())
                .send()
                .await;

            let resp = match resp {
                Ok(r) => r,
                Err(e) if e.is_timeout() => return Err(OcrError::Timeout),
                Err(e) => {
                    log_reqwest_error("Doc2X status poll GET", &poll_url, &e);
                    return Err(OcrError::Upstream(0, format!("轮询失败: {e}")));
                }
            };

            let status_code = resp.status().as_u16();
            if status_code == 401 || status_code == 403 {
                let _body = resp.text().await.unwrap_or_default();
                return Err(OcrError::NoApiKey);
            }
            if !resp.status().is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(OcrError::Upstream(status_code, body));
            }

            // 先取原始文本，再反序列化；失败时打印原始响应以便排查结构差异
            let raw_text = resp.text().await.unwrap_or_default();
            let st: StatusResponse = match serde_json::from_str(&raw_text) {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!(
                        "Doc2X status JSON 反序列化失败, status={}, err={}, 原始响应: {}",
                        status_code,
                        e,
                        truncate_for_log(&raw_text, 2000)
                    );
                    return Err(OcrError::Upstream(
                        status_code,
                        format!("status 反序列化失败: {e}"),
                    ));
                }
            };

            if st.code.as_deref() != Some("success") {
                let msg = st.msg.unwrap_or_else(|| "status 失败".to_string());
                return Err(OcrError::Upstream(0, msg));
            }

            let data = match st.data {
                Some(d) => d,
                None => {
                    return Err(OcrError::Upstream(
                        0,
                        "status 响应缺少 data 字段".to_string(),
                    ))
                }
            };

            match data.status.as_deref() {
                Some("success") => {
                    let result = data.result.ok_or_else(|| {
                        OcrError::Upstream(0, "success 但缺少 result".to_string())
                    })?;
                    return extract_pages_md_from_struct(&result);
                }
                Some("failed") => {
                    let msg = data
                        .detail
                        .clone()
                        .or_else(|| st.msg.clone())
                        .unwrap_or_else(|| "Doc2X 处理失败".to_string());
                    return Err(OcrError::Upstream(0, msg));
                }
                // processing / 其他状态 / None → 继续 polling
                _ => {
                    // 上报进度（0~100；progress 缺失/不可解析时跳过本轮）
                    if let (Some(cb), Some(pct)) = (
                        on_progress,
                        data.progress
                            .as_ref()
                            .and_then(crate::ai::ocr::parse_percent_value)
                            .map(|f| f.clamp(0.0, 100.0) as u8),
                    ) {
                        cb(pct);
                    }
                    tracing::debug!(
                        uid = %uid,
                        status = ?data.status,
                        progress = ?data.progress,
                        "Doc2X PDF 解析中，3s 后继续轮询"
                    );
                }
            }

            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }
}

// ---------------------------------------------------------------------------
// 单元测试（不访问真实网络，仅校验 URL 构造、响应解析、JSON 反序列化）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_provider() -> Doc2XProvider {
        Doc2XProvider::new("sk-test".into(), "https://v2.doc2x.noedgeai.com".into())
    }

    #[test]
    fn test_id_and_pdf_support() {
        let p = make_provider();
        assert_eq!(p.id(), "doc2x");
        assert!(p.supports_pdf());
    }

    /// 网络连通性探针（手动运行，不参与 CI）
    ///
    /// 运行方式：
    ///   cargo test --lib ai::ocr::doc2x::tests::network_probe_doc2x_connectivity -- --ignored --nocapture
    ///
    /// 用与生产代码完全相同的 reqwest Client 配置调用 Doc2X status 接口，
    /// 打印 reqwest 的真实错误（含 source chain），用于诊断"网络错误"根因。
    #[tokio::test]
    #[ignore = "网络探针：手动运行，需要外网连通"]
    async fn network_probe_doc2x_connectivity() {
        let p = make_provider();
        let url = p.url("/api/v2/parse/status?uid=test-connection-probe");

        println!("=== 探针 URL: {url} ===");
        println!("=== reqwest Client 配置: timeout=120s, no_proxy=true, rustls-tls (webpki-roots) ===");

        let resp = p.client.get(&url).send().await;
        match resp {
            Ok(r) => {
                println!("=== HTTP 响应成功 ===");
                println!("  status: {}", r.status());
                println!("  headers:");
                for (k, v) in r.headers().iter() {
                    println!("    {k}: {}", v.to_str().unwrap_or("<binary>"));
                }
                let body = r.text().await.unwrap_or_default();
                println!("  body (前 500 字符): {}", &body.chars().take(500).collect::<String>());
            }
            Err(ref e) => {
                println!("=== reqwest 错误 ===");
                println!("  Display: {e}");
                println!("  Debug:   {e:?}");
                println!("  is_timeout:  {}", e.is_timeout());
                println!("  is_connect:  {}", e.is_connect());
                println!("  is_request:  {}", e.is_request());
                println!("  is_decode:   {}", e.is_decode());
                println!("  is_redirect: {}", e.is_redirect());
                println!("  is_status:   {}", e.is_status());
                println!("  is_body:     {}", e.is_body());
                println!("  source_chain: {}", format_reqwest_error_chain(e));
                // format_reqwest_error_chain 已包含完整 source chain（最多 8 层）
            }
        }
        // 测试断言永远成功，仅用于打印诊断
        assert!(true);
    }

    #[test]
    fn test_url_construction_img_layout() {
        let p = make_provider();
        assert_eq!(
            p.url("/api/v2/parse/img/layout"),
            "https://v2.doc2x.noedgeai.com/api/v2/parse/img/layout"
        );
    }

    #[test]
    fn test_url_construction_status_with_query() {
        let p = make_provider();
        assert_eq!(
            p.url("/api/v2/parse/status?uid=abc123"),
            "https://v2.doc2x.noedgeai.com/api/v2/parse/status?uid=abc123"
        );
    }

    #[test]
    fn test_url_trims_trailing_slash() {
        let p = Doc2XProvider::new("k".into(), "https://v2.doc2x.noedgeai.com/".into());
        assert_eq!(
            p.url("/api/v2/parse/preupload"),
            "https://v2.doc2x.noedgeai.com/api/v2/parse/preupload"
        );
    }

    #[test]
    fn test_auth_header() {
        let p = Doc2XProvider::new("sk-abc".into(), "https://x".into());
        assert_eq!(p.auth_header(), "Bearer sk-abc");
    }

    #[test]
    fn test_extract_pages_md_single_page() {
        let v = serde_json::json!({
            "result": {
                "pages": [{ "md": "# 标题\n\n$E=mc^2$" }]
            }
        });
        let md = extract_pages_md_from_value(Some(&v)).unwrap();
        assert_eq!(md, "# 标题\n\n$E=mc^2$");
    }

    #[test]
    fn test_extract_pages_md_multi_page_joined() {
        let v = serde_json::json!({
            "result": {
                "pages": [
                    { "md": "第一页" },
                    { "md": "第二页" }
                ]
            }
        });
        let md = extract_pages_md_from_value(Some(&v)).unwrap();
        assert_eq!(md, "第一页\n\n第二页");
    }

    #[test]
    fn test_extract_pages_md_missing_result() {
        let v = serde_json::json!({ "foo": "bar" });
        let err = extract_pages_md_from_value(Some(&v)).unwrap_err();
        assert!(matches!(err, OcrError::Upstream(0, _)));
    }

    #[test]
    fn test_extract_pages_md_empty_pages() {
        let v = serde_json::json!({ "result": { "pages": [] } });
        let err = extract_pages_md_from_value(Some(&v)).unwrap_err();
        assert!(matches!(err, OcrError::Upstream(0, _)));
    }

    #[test]
    fn test_extract_pages_md_page_missing_md_field() {
        let v = serde_json::json!({
            "result": { "pages": [{ "other": "x" }] }
        });
        let err = extract_pages_md_from_value(Some(&v)).unwrap_err();
        assert!(matches!(err, OcrError::Upstream(0, _)));
    }

    #[test]
    fn test_extract_pages_md_skips_pages_without_md() {
        let v = serde_json::json!({
            "result": {
                "pages": [
                    { "md": "有内容" },
                    { "other": "无 md 字段" },
                    { "md": "后段" }
                ]
            }
        });
        let md = extract_pages_md_from_value(Some(&v)).unwrap();
        assert_eq!(md, "有内容\n\n后段");
    }

    #[test]
    fn test_extract_pages_md_none_data_returns_err() {
        // v2 鲁棒性：data 缺失时 extract 应返回错误而非 panic
        let err = extract_pages_md_from_value(None).unwrap_err();
        assert!(matches!(err, OcrError::Upstream(0, _)));
    }

    #[test]
    fn test_extract_pages_md_from_struct_success() {
        let result = StatusResult {
            pages: vec![
                StatusPage { md: Some("第一页".into()) },
                StatusPage { md: None },
                StatusPage { md: Some("后段".into()) },
            ],
        };
        let md = extract_pages_md_from_struct(&result).unwrap();
        assert_eq!(md, "第一页\n\n后段");
    }

    #[test]
    fn test_extract_pages_md_from_struct_all_empty() {
        let result = StatusResult {
            pages: vec![StatusPage { md: None }, StatusPage { md: Some("".into()) }],
        };
        let err = extract_pages_md_from_struct(&result).unwrap_err();
        assert!(matches!(err, OcrError::Upstream(0, _)));
    }

    #[test]
    fn test_join_md_strings_empty() {
        let err = join_md_strings(vec![]).unwrap_err();
        assert!(matches!(err, OcrError::Upstream(0, _)));
    }

    #[test]
    fn test_join_md_strings_single() {
        let md = join_md_strings(vec!["only".into()]).unwrap();
        assert_eq!(md, "only");
    }

    #[test]
    fn test_preupload_response_parsing() {
        let raw = serde_json::json!({
            "code": "success",
            "data": { "uid": "u-123", "url": "https://upload.example/abc" },
            "msg": null
        });
        let pre: PreuploadResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(pre.code.as_deref(), Some("success"));
        let data = pre.data.unwrap();
        assert_eq!(data.uid.as_deref(), Some("u-123"));
        assert_eq!(data.url.as_deref(), Some("https://upload.example/abc"));
    }

    #[test]
    fn test_preupload_response_tolerates_extra_fields() {
        // v2 鲁棒性：响应含未声明字段（model / expire / extra）不应导致反序列化失败
        let raw = serde_json::json!({
            "code": "success",
            "data": {
                "uid": "u-456",
                "url": "https://upload.example/def",
                "model": "v3-2026",
                "expire": 1718000000,
                "extra_nested": { "foo": "bar" }
            },
            "request_id": "req-abc",
            "extra_top": [1, 2, 3]
        });
        let pre: PreuploadResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(pre.code.as_deref(), Some("success"));
        let data = pre.data.unwrap();
        assert_eq!(data.uid.as_deref(), Some("u-456"));
        assert_eq!(data.url.as_deref(), Some("https://upload.example/def"));
    }

    #[test]
    fn test_preupload_response_missing_code_defaults_to_none() {
        // v2 鲁棒性：code 缺失时不应 panic，应为 None（调用方按 != success 处理）
        let raw = serde_json::json!({
            "data": { "uid": "u-x", "url": "https://upload.example/x" }
        });
        let pre: PreuploadResponse = serde_json::from_value(raw).unwrap();
        assert!(pre.code.is_none());
    }

    #[test]
    fn test_status_response_processing() {
        let raw = serde_json::json!({
            "code": "success",
            "data": { "status": "processing", "progress": 42 }
        });
        let st: StatusResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(st.code.as_deref(), Some("success"));
        let data = st.data.unwrap();
        assert_eq!(data.status.as_deref(), Some("processing"));
        assert!(data.result.is_none());
    }

    #[test]
    fn test_status_response_success_with_pages() {
        let raw = serde_json::json!({
            "code": "success",
            "data": {
                "status": "success",
                "result": { "pages": [{ "md": "完成" }] },
                "progress": 100
            }
        });
        let st: StatusResponse = serde_json::from_value(raw).unwrap();
        let data = st.data.unwrap();
        assert_eq!(data.status.as_deref(), Some("success"));
        let result = data.result.unwrap();
        assert_eq!(result.pages.len(), 1);
        assert_eq!(result.pages[0].md.as_deref(), Some("完成"));
    }

    #[test]
    fn test_status_response_failed() {
        let raw = serde_json::json!({
            "code": "success",
            "data": { "status": "failed" },
            "msg": "PDF 解析失败"
        });
        let st: StatusResponse = serde_json::from_value(raw).unwrap();
        let data = st.data.unwrap();
        assert_eq!(data.status.as_deref(), Some("failed"));
        assert!(data.result.is_none());
    }

    #[test]
    fn test_status_response_tolerates_unknown_status_value() {
        // v2 鲁棒性：未知 status 字符串（如 "queued"）不应破坏反序列化
        let raw = serde_json::json!({
            "code": "success",
            "data": {
                "status": "queued",
                "extra_field": "ignored",
                "result": null
            }
        });
        let st: StatusResponse = serde_json::from_value(raw).unwrap();
        let data = st.data.unwrap();
        assert_eq!(data.status.as_deref(), Some("queued"));
        assert!(data.result.is_none());
    }

    #[test]
    fn test_truncate_for_log_short_string() {
        assert_eq!(truncate_for_log("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_for_log_long_string() {
        let long = "a".repeat(5000);
        let truncated = truncate_for_log(&long, 100);
        assert!(truncated.starts_with(&"a".repeat(100)));
        assert!(truncated.contains("truncated"));
        assert!(truncated.contains("5000 chars"));
    }

    #[test]
    fn test_strip_data_uri_prefix_with_data_uri() {
        let input = "data:image/png;base64,iVBORw0KGgoAAAANS";
        let pure = strip_data_uri_prefix(input);
        assert_eq!(pure, "iVBORw0KGgoAAAANS");
    }

    #[test]
    fn test_strip_data_uri_prefix_with_jpeg() {
        let input = "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQ";
        let pure = strip_data_uri_prefix(input);
        assert_eq!(pure, "/9j/4AAQSkZJRgABAQ");
    }

    #[test]
    fn test_strip_data_uri_prefix_without_prefix() {
        let input = "iVBORw0KGgoAAAANS";
        let pure = strip_data_uri_prefix(input);
        assert_eq!(pure, "iVBORw0KGgoAAAANS");
    }

    #[test]
    fn test_strip_data_uri_prefix_empty() {
        let pure = strip_data_uri_prefix("");
        assert_eq!(pure, "");
    }
}
