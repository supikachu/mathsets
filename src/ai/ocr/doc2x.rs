//! Doc2X OCR 引擎（M2 新增）
//!
//! 官方文档：https://doc2x.noedgeai.com/help/zh-cn/api/
//!
//! ## 鉴权
//! 所有请求 header 携带 `Authorization: Bearer sk-xxx`。
//!
//! ## 图片 OCR（同步）
//! POST `/parse/img/layout`，body 为**原始二进制**（非 base64、非 formdata）。
//! 响应：`{code:"success", data:{result:{pages:[{md:"..."}]}, uid:"..."}}`，
//! 拼接所有 `pages[].md`。
//!
//! ## PDF OCR（异步 submit→poll）
//! 1. POST `/parse/preupload`（仅需 auth header，无 body）→ `{data:{uid, url}}`
//! 2. HTTP PUT 文件二进制到 `url`（**不带 auth header**）
//! 3. 轮询 GET `/parse/status?uid=xxx`（每 3s）→
//!    `{code:"success", data:{status:"processing"|"success"|"failed",
//!    result:{pages:[{md:"..."}]}, progress}}`，超时 120s。
//!
//! ## base_url 约定
//! 默认 `https://api.doc2x.noedgex.com/v1`，已含版本前缀，
//! 各路径直接追加（不再补 `/api/v2`）。官方 v2 域名用户可改 `DOC2X_BASE_URL`
//! 指向 `https://v2.doc2x.noedgeai.com` 并自行适配路径前缀。

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
// 响应类型（仅反序列化 Doc2X 真实返回的字段，多余字段忽略）
// ---------------------------------------------------------------------------

/// `/parse/img/layout` 同步响应
#[derive(Deserialize)]
struct ImgLayoutResponse {
    code: String,
    /// data 缺失或非对象时用 serde_json::Value 兜底
    data: serde_json::Value,
    msg: Option<String>,
}

/// `/parse/preupload` 响应：`{code, data:{uid, url}}`
#[derive(Deserialize)]
struct PreuploadResponse {
    code: String,
    data: PreuploadData,
    msg: Option<String>,
}

#[derive(Deserialize)]
struct PreuploadData {
    uid: String,
    url: String,
}

/// `/parse/status` 响应
#[derive(Deserialize)]
struct StatusResponse {
    code: String,
    data: StatusData,
    msg: Option<String>,
}

#[derive(Deserialize)]
struct StatusData {
    status: String,
    /// success 时才有 result.pages
    result: Option<StatusResult>,
}

#[derive(Deserialize)]
struct StatusResult {
    pages: Vec<StatusPage>,
}

#[derive(Deserialize)]
struct StatusPage {
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

/// 从 `/parse/img/layout` 响应的 `data` 对象中提取 `result.pages[].md` 并拼接
fn extract_pages_md_from_value(data: &serde_json::Value) -> Result<String, OcrError> {
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

/// 从 `/parse/status` success 响应的 `result.pages` 结构体提取并拼接 `md` 字段
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
    async fn ocr_image(&self, image_b64: &str) -> Result<String, OcrError> {
        let image_bytes = base64::engine::general_purpose::STANDARD
            .decode(image_b64.as_bytes())
            .map_err(|e| OcrError::Upstream(0, format!("base64 解码失败: {e}")))?;

        let url = self.url("/parse/img/layout");
        let resp = self
            .client
            .post(&url)
            .header("Authorization", self.auth_header())
            .body(image_bytes)
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) if e.is_timeout() => return Err(OcrError::Timeout),
            Err(e) => return Err(OcrError::Upstream(0, format!("{:?}", e))),
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

        let parsed: ImgLayoutResponse = resp
            .json()
            .await
            .map_err(|e| OcrError::Upstream(status, format!("响应反序列化失败: {e}")))?;

        if parsed.code != "success" {
            let msg = parsed.msg.unwrap_or_else(|| "未知错误".to_string());
            return Err(OcrError::Upstream(0, msg));
        }

        // data.result.pages 路径提取
        extract_pages_md_from_value(&parsed.data)
    }

    /// PDF 异步 OCR → 全文 Markdown
    ///
    /// submit (preupload) → PUT 上传 → poll status
    async fn ocr_pdf_async(&self, pdf_bytes: &[u8]) -> Result<String, OcrError> {
        // ── Step 1：preupload 获取上传 url ──
        let preupload_url = self.url("/parse/preupload");
        let resp = self
            .client
            .post(&preupload_url)
            .header("Authorization", self.auth_header())
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) if e.is_timeout() => return Err(OcrError::Timeout),
            Err(e) => return Err(OcrError::Upstream(0, format!("{:?}", e))),
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

        let pre: PreuploadResponse = resp
            .json()
            .await
            .map_err(|e| OcrError::Upstream(status, format!("preupload 反序列化失败: {e}")))?;
        if pre.code != "success" {
            let msg = pre.msg.unwrap_or_else(|| "preupload 失败".to_string());
            return Err(OcrError::Upstream(0, msg));
        }
        let uid = pre.data.uid;
        let upload_url = pre.data.url;

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
            Err(e) => return Err(OcrError::Upstream(0, format!("上传 PDF 失败: {:?}", e))),
        };
        if !put_resp.status().is_success() {
            let code = put_resp.status().as_u16();
            let body = put_resp.text().await.unwrap_or_default();
            return Err(OcrError::Upstream(code, format!("上传 PDF 失败: {body}")));
        }

        // ── Step 3：轮询 status（每 3s，超时 120s） ──
        let start = Instant::now();
        let timeout = Duration::from_secs(120);
        let poll_url = self.url(&format!("/parse/status?uid={uid}"));

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
                Err(e) => return Err(OcrError::Upstream(0, format!("轮询失败: {:?}", e))),
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

            let st: StatusResponse = match resp.json().await {
                Ok(v) => v,
                Err(e) => {
                    return Err(OcrError::Upstream(
                        status_code,
                        format!("status 反序列化失败: {e}"),
                    ))
                }
            };

            if st.code != "success" {
                let msg = st.msg.unwrap_or_else(|| "status 失败".to_string());
                return Err(OcrError::Upstream(0, msg));
            }

            match st.data.status.as_str() {
                "success" => {
                    let result = st
                        .data
                        .result
                        .ok_or_else(|| OcrError::Upstream(0, "success 但缺少 result".to_string()))?;
                    return extract_pages_md_from_struct(&result);
                }
                "failed" => {
                    let msg = st.msg.unwrap_or_else(|| "Doc2X 处理失败".to_string());
                    return Err(OcrError::Upstream(0, msg));
                }
                // processing / 其他状态 → 继续 polling
                _ => {}
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
        Doc2XProvider::new("sk-test".into(), "https://api.doc2x.noedgex.com/v1".into())
    }

    #[test]
    fn test_id_and_pdf_support() {
        let p = make_provider();
        assert_eq!(p.id(), "doc2x");
        assert!(p.supports_pdf());
    }

    #[test]
    fn test_url_construction_img_layout() {
        let p = make_provider();
        assert_eq!(
            p.url("/parse/img/layout"),
            "https://api.doc2x.noedgex.com/v1/parse/img/layout"
        );
    }

    #[test]
    fn test_url_construction_status_with_query() {
        let p = make_provider();
        assert_eq!(
            p.url("/parse/status?uid=abc123"),
            "https://api.doc2x.noedgex.com/v1/parse/status?uid=abc123"
        );
    }

    #[test]
    fn test_url_trims_trailing_slash() {
        let p = Doc2XProvider::new("k".into(), "https://api.doc2x.noedgex.com/v1/".into());
        assert_eq!(
            p.url("/parse/preupload"),
            "https://api.doc2x.noedgex.com/v1/parse/preupload"
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
        let md = extract_pages_md_from_value(&v).unwrap();
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
        let md = extract_pages_md_from_value(&v).unwrap();
        assert_eq!(md, "第一页\n\n第二页");
    }

    #[test]
    fn test_extract_pages_md_missing_result() {
        let v = serde_json::json!({ "foo": "bar" });
        let err = extract_pages_md_from_value(&v).unwrap_err();
        assert!(matches!(err, OcrError::Upstream(0, _)));
    }

    #[test]
    fn test_extract_pages_md_empty_pages() {
        let v = serde_json::json!({ "result": { "pages": [] } });
        let err = extract_pages_md_from_value(&v).unwrap_err();
        assert!(matches!(err, OcrError::Upstream(0, _)));
    }

    #[test]
    fn test_extract_pages_md_page_missing_md_field() {
        let v = serde_json::json!({
            "result": { "pages": [{ "other": "x" }] }
        });
        let err = extract_pages_md_from_value(&v).unwrap_err();
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
        let md = extract_pages_md_from_value(&v).unwrap();
        assert_eq!(md, "有内容\n\n后段");
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
        assert_eq!(pre.code, "success");
        assert_eq!(pre.data.uid, "u-123");
        assert_eq!(pre.data.url, "https://upload.example/abc");
    }

    #[test]
    fn test_status_response_processing() {
        let raw = serde_json::json!({
            "code": "success",
            "data": { "status": "processing", "progress": 42 }
        });
        let st: StatusResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(st.data.status, "processing");
        assert!(st.data.result.is_none());
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
        assert_eq!(st.data.status, "success");
        let result = st.data.result.unwrap();
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
        assert_eq!(st.data.status, "failed");
        assert!(st.data.result.is_none());
    }
}
