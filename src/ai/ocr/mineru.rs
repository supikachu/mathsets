//! MinerU OCR 引擎（M4 新增，v1.2 重构支持官方云端 Precision API）
//!
//! 官方仓库：https://github.com/opendatalab/MinerU
//!
//! ## 双模式自动适配
//!
//! 根据 `base_url` 是否含 `mineru.net` 自动选择请求路径：
//!
//! ### 私有部署模式（默认）
//! base_url 不含 `mineru.net`（如 `http://127.0.0.1:8000`）。
//! 走 `/file_parse` multipart 同步路径：上传文件二进制 → 阻塞等待 → 返回 Markdown。
//!
//! ### 官方云端 Precision API 模式
//! base_url 为 `https://mineru.net/api` 或含 `mineru.net`。
//! 走 `/v4/file-urls/batch` 签名上传异步路径：
//! 1. `POST /v4/file-urls/batch`（JSON + Bearer Token）→ 获取 `batch_id` + 签名上传 URL
//! 2. `PUT` 文件二进制到签名 URL（不带 Content-Type，避免 OSS SignatureDoesNotMatch）
//! 3. 轮询 `GET /v4/extract-results/batch/{batch_id}` → 获取 `full_zip_url`
//! 4. 下载 zip → 解压 `full.md`
//!
//! `is_ocr: true` + `enable_formula: true` 确保数学公式与手写试卷完整识别。
//! `model_version: "vlm"`（推荐）提供更高精度的版面理解。
//!
//! ## 优雅降级
//! 云端返回 401/403（Key 无效）或 404/500/HTML 错误页时，捕获为 `OcrError::Upstream(0, ...)`（code=0
//! 触发 `should_fallback` → 自动降级 Qwen-VL），避免程序 crash。

use async_trait::async_trait;
use base64::Engine as _;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

use super::{OcrError, OcrProvider};

/// 云端轮询间隔（秒）
const CLOUD_POLL_INTERVAL_SECS: u64 = 3;
/// 云端最大轮询次数（3s × 100 = 5 分钟超时）
const CLOUD_MAX_POLLS: u32 = 100;

/// MinerU OCR 引擎
///
/// - `api_key`：可选 Bearer Token（私有部署免鉴权时为空；云端 API 必填）
/// - `base_url`：服务端点（末尾斜杠自动 trim），含 `mineru.net` 时走云端路径
pub struct MineruProvider {
    api_key: Option<String>,
    base_url: String,
    client: Client,
    /// 题目图片落盘根目录（如 "./uploads"），云端模式解压 zip 时把 images/* 落盘到这里
    upload_dir: Option<String>,
}

impl MineruProvider {
    /// 构造 MinerU 引擎实例
    ///
    /// `api_key` 为空字符串时等价于 None（无鉴权）。
    pub fn new(api_key: String, base_url: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .no_proxy() // 与 Doc2XProvider 一致：绕过系统代理，避免本机代理拦截内网请求
            .build()
            .expect("无法创建 reqwest Client");
        Self {
            api_key: if api_key.trim().is_empty() {
                None
            } else {
                Some(api_key)
            },
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
            upload_dir: None,
        }
    }

    /// 链式设置 upload_dir，用于 MinerU 云端模式解压 zip 时搬运 images/* 图片到标准目录
    pub fn with_upload_dir(mut self, upload_dir: Option<String>) -> Self {
        self.upload_dir = upload_dir.filter(|s| !s.trim().is_empty());
        self
    }

    /// 构造可选 Authorization Bearer header（仅当 api_key 非空时返回）
    fn auth_header(&self) -> Option<String> {
        self.api_key.as_ref().map(|k| format!("Bearer {k}"))
    }

    /// 拼接 base_url 与相对路径（base_url 末尾斜杠已在构造时 trim）
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    /// 判断是否为官方云端 API（base_url 含 `mineru.net`）
    ///
    /// 注意：`mineru.internal` 等私有域名不含 `mineru.net`，不会被误判为云端。
    fn is_cloud(&self) -> bool {
        self.base_url.to_lowercase().contains("mineru.net")
    }
}

// ---------------------------------------------------------------------------
// 私有部署响应类型（/file_parse 同步路径）
// ---------------------------------------------------------------------------

/// `/file_parse` 同步响应：`{ code: 0, data: { md_content, images } }`
///
/// 兼容多种字段命名（官方 `md_content` / 分支 `md` / `markdown` / `content`）。
#[derive(Deserialize)]
struct FileParseResponse {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    data: Option<FileParseData>,
}

#[derive(Deserialize, Default)]
struct FileParseData {
    #[serde(default, alias = "md", alias = "markdown", alias = "content")]
    md_content: Option<String>,
}

/// 从私有部署响应 JSON 中提取 Markdown 文本
fn extract_markdown(resp: &FileParseResponse) -> Result<String, OcrError> {
    if resp.code != 0 {
        let msg = resp
            .msg
            .clone()
            .unwrap_or_else(|| format!("MinerU 返回非零 code={}", resp.code));
        return Err(OcrError::Upstream(0, msg));
    }

    let data = resp
        .data
        .as_ref()
        .ok_or_else(|| OcrError::Upstream(0, "MinerU 响应缺少 data 字段".to_string()))?;

    data.md_content
        .clone()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| OcrError::Upstream(0, "MinerU 返回空 Markdown".to_string()))
}

// ---------------------------------------------------------------------------
// 云端 Precision API 请求/响应类型（/v4/file-urls/batch + /v4/extract-results/batch）
// ---------------------------------------------------------------------------

/// `POST /v4/file-urls/batch` 请求体
///
/// `is_ocr: true` + `enable_formula: true` 是识别数学公式和手写试卷的关键参数。
/// `model_version: "vlm"`（推荐）提供更高精度的版面理解。
#[derive(Serialize)]
struct CloudBatchRequest {
    files: Vec<CloudBatchFile>,
    model_version: String,
    is_ocr: bool,
    enable_formula: bool,
    enable_table: bool,
    language: String,
}

#[derive(Serialize)]
struct CloudBatchFile {
    name: String,
}

/// 云端通用响应（batch 提交与轮询共用同一信封）
#[derive(Deserialize)]
struct CloudBatchResponse {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    data: Option<CloudBatchData>,
}

/// data 字段（提交阶段含 `file_urls`，轮询阶段含 `extract_result`）
///
/// 提交阶段：`{ batch_id, file_urls: [...] }`
/// 轮询阶段：`{ batch_id, extract_result: [{ state, full_zip_url, err_msg }] }`
#[derive(Deserialize, Default)]
struct CloudBatchData {
    #[serde(default, alias = "batchId")]
    batch_id: Option<String>,
    /// 提交阶段返回的签名上传 URL 列表
    #[serde(default)]
    file_urls: Vec<String>,
    /// 轮询阶段的解析结果列表
    #[serde(default)]
    extract_result: Vec<ExtractResult>,
}

#[derive(Deserialize, Default)]
struct ExtractResult {
    /// 任务状态：waiting-file / pending / running / converting / done / failed
    #[serde(default)]
    state: Option<String>,
    /// 完成后的 zip 下载 URL
    #[serde(default)]
    full_zip_url: Option<String>,
    #[serde(default)]
    err_msg: Option<String>,
}

/// 校验云端 HTTP 响应并反序列化为 JSON
///
/// 防护策略：
/// 1. HTTP 401/403 → `NoApiKey`（Key 无效，触发降级 Qwen-VL）
/// 2. 非 2xx → 捕获 body → `Upstream(0, ...)` 触发降级
/// 3. Content-Type 不含 json → 视为 HTML 错误页 → `Upstream(0, ...)` 触发降级
/// 4. JSON 反序列化失败 → `Upstream(0, ...)` 触发降级
async fn check_and_parse_cloud<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
) -> Result<T, OcrError> {
    let status = resp.status().as_u16();
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    // 1. 鉴权失败 → NoApiKey（触发降级）
    if status == 401 || status == 403 {
        let body = resp.text().await.unwrap_or_default();
        let preview: String = body.chars().take(200).collect();
        tracing::warn!("MinerU 云端鉴权失败: HTTP {status}, body: {preview}");
        return Err(OcrError::NoApiKey);
    }

    // 2. 非 2xx → 捕获 body 并降级
    if status != 200 && status != 201 {
        let body = resp.text().await.unwrap_or_default();
        let preview: String = body.chars().take(200).collect();
        tracing::warn!(
            "MinerU 云端返回非 2xx: HTTP {status}, Content-Type: {ct}, body 预览: {preview}"
        );
        return Err(OcrError::Upstream(
            0,
            format!("MinerU 云端 HTTP {status}: {preview}"),
        ));
    }

    // 3. Content-Type 不含 json → 可能是 HTML 错误页（404/500 网关返回 HTML）
    if !ct.contains("json") {
        let body = resp.text().await.unwrap_or_default();
        let preview: String = body.chars().take(200).collect();
        tracing::warn!(
            "MinerU 云端返回非 JSON 响应 (Content-Type: {ct}), 可能是 HTML 错误页, body 预览: {preview}"
        );
        return Err(OcrError::Upstream(
            0,
            format!("MinerU 云端返回非 JSON (Content-Type: {ct})"),
        ));
    }

    // 4. 反序列化 JSON
    resp.json::<T>()
        .await
        .map_err(|e| {
            tracing::warn!("MinerU 云端 JSON 反序列化失败: {e}");
            OcrError::Upstream(0, format!("MinerU 云端 JSON 解析失败: {e}"))
        })
}

#[async_trait]
impl OcrProvider for MineruProvider {
    fn id(&self) -> &'static str {
        "mineru_local"
    }

    /// MinerU 原生支持 PDF 直传（私有 /file_parse 与云端签名上传均支持）
    fn supports_pdf(&self) -> bool {
        true
    }

    /// 单图 OCR → Markdown
    ///
    /// - 云端模式：base64 解码 → 签名上传 → 轮询 → 下载 zip → 提取 full.md
    /// - 私有模式：base64 解码 → multipart 上传 /file_parse → 同步返回
    async fn ocr_image(&self, image_b64: &str) -> Result<String, OcrError> {
        let image_bytes = base64::engine::general_purpose::STANDARD
            .decode(image_b64.as_bytes())
            .map_err(|e| OcrError::Upstream(0, format!("base64 解码失败: {e}")))?;

        if self.is_cloud() {
            self.cloud_upload_and_parse(&image_bytes, "image.png")
                .await
        } else {
            self.upload_and_parse(image_bytes, "image.png", "image/png")
                .await
        }
    }

    /// PDF OCR → 全文 Markdown
    ///
    /// - 云端模式：签名上传 → 轮询 → 下载 zip → 提取 full.md
    /// - 私有模式：multipart 上传 /file_parse → 同步阻塞返回
    async fn ocr_pdf_async(&self, pdf_bytes: &[u8]) -> Result<String, OcrError> {
        if self.is_cloud() {
            self.cloud_upload_and_parse(pdf_bytes, "input.pdf")
                .await
        } else {
            self.upload_and_parse(pdf_bytes.to_vec(), "input.pdf", "application/pdf")
                .await
        }
    }
}

impl MineruProvider {
    // -----------------------------------------------------------------------
    // 云端 Precision API 路径（/v4/file-urls/batch → PUT → poll → zip）
    // -----------------------------------------------------------------------

    /// 云端解析完整流程：签名上传 → 轮询 → 下载 zip → 提取 Markdown
    ///
    /// 流程：
    /// 1. `POST /v4/file-urls/batch`（JSON + Bearer Token）→ 获取 `batch_id` + 签名上传 URL
    /// 2. `PUT` 文件二进制到签名 URL（不带 Content-Type）
    /// 3. 轮询 `GET /v4/extract-results/batch/{batch_id}` → 获取 `full_zip_url`
    /// 4. 下载 zip → 解压 `full.md`
    async fn cloud_upload_and_parse(
        &self,
        bytes: &[u8],
        filename: &str,
    ) -> Result<String, OcrError> {
        // 1. 申请签名上传地址
        let (batch_id, upload_url) = self.request_upload_url(filename).await?;

        // 2. PUT 文件到签名 URL
        self.upload_to_signed_url(&upload_url, bytes).await?;

        // 3. 轮询结果
        let zip_url = self.poll_batch_result(&batch_id).await?;

        // 4. 下载 zip 并提取 Markdown
        self.download_and_extract_markdown(&zip_url).await
    }

    /// 申请签名上传地址：`POST /v4/file-urls/batch`
    ///
    /// 返回 `(batch_id, upload_url)`。
    async fn request_upload_url(&self, filename: &str) -> Result<(String, String), OcrError> {
        let body = CloudBatchRequest {
            files: vec![CloudBatchFile {
                name: filename.to_string(),
            }],
            model_version: "vlm".to_string(),
            is_ocr: true,
            enable_formula: true,
            enable_table: true,
            language: "ch".to_string(),
        };

        let url = self.url("/v4/file-urls/batch");
        let mut req = self.client.post(&url).json(&body);
        if let Some(h) = self.auth_header() {
            req = req.header("Authorization", h);
        }

        tracing::info!("MinerU 云端申请签名上传地址: {}", url);

        let resp = req.send().await.map_err(|e| {
            if e.is_timeout() {
                OcrError::Timeout
            } else {
                tracing::warn!("MinerU 云端请求失败: {e:?}");
                OcrError::Upstream(0, format!("{e:?}"))
            }
        })?;

        let parsed: CloudBatchResponse = check_and_parse_cloud(resp).await?;

        if parsed.code != 0 {
            let msg = parsed
                .msg
                .unwrap_or_else(|| format!("MinerU 云端返回非零 code={}", parsed.code));
            return Err(OcrError::Upstream(0, msg));
        }

        let data = parsed.data.ok_or_else(|| {
            OcrError::Upstream(0, "MinerU 云端响应缺少 data 字段".to_string())
        })?;

        let batch_id = data.batch_id.ok_or_else(|| {
            OcrError::Upstream(0, "MinerU 云端响应缺少 batch_id".to_string())
        })?;

        let upload_url = data.file_urls.into_iter().next().ok_or_else(|| {
            OcrError::Upstream(0, "MinerU 云端响应缺少 file_urls".to_string())
        })?;

        tracing::info!("MinerU 云端签名上传地址已获取, batch_id={batch_id}");
        Ok((batch_id, upload_url))
    }

    /// PUT 文件二进制到签名 URL（不带 Content-Type）
    ///
    /// **关键**：签名 URL 的 Signature 已包含请求方法(PUT)与参数，发送时
    /// 不能带 `Content-Type` header，否则 Aliyun OSS 会返回 `SignatureDoesNotMatch`（HTTP 403）。
    async fn upload_to_signed_url(&self, url: &str, bytes: &[u8]) -> Result<(), OcrError> {
        tracing::info!(
            "MinerU 云端上传文件到签名 URL ({} 字节)",
            bytes.len()
        );

        let resp = self
            .client
            .put(url)
            .body(bytes.to_vec())
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    OcrError::Timeout
                } else {
                    tracing::warn!("MinerU 云端上传失败: {e:?}");
                    OcrError::Upstream(0, format!("{e:?}"))
                }
            })?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let preview: String = body.chars().take(300).collect();
            tracing::warn!(
                "MinerU 云端签名上传失败: HTTP {status}, body: {preview}"
            );
            return Err(OcrError::Upstream(
                status.as_u16(),
                format!("签名上传失败: {preview}"),
            ));
        }

        tracing::info!("MinerU 云端文件上传成功");
        Ok(())
    }

    /// 轮询批量任务结果：`GET /v4/extract-results/batch/{batch_id}`
    ///
    /// 每 3s 轮询一次，最多 100 次（5 分钟）。当 `state=done` 时取 `full_zip_url`。
    async fn poll_batch_result(&self, batch_id: &str) -> Result<String, OcrError> {
        let url = self.url(&format!("/v4/extract-results/batch/{batch_id}"));

        for attempt in 0..CLOUD_MAX_POLLS {
            tokio::time::sleep(Duration::from_secs(CLOUD_POLL_INTERVAL_SECS)).await;

            let mut req = self.client.get(&url);
            if let Some(h) = self.auth_header() {
                req = req.header("Authorization", h);
            }

            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) if e.is_timeout() => return Err(OcrError::Timeout),
                Err(e) => {
                    tracing::warn!("MinerU 云端轮询请求失败: {e:?}");
                    return Err(OcrError::Upstream(0, format!("{e:?}")));
                }
            };

            let parsed: CloudBatchResponse = check_and_parse_cloud(resp).await?;

            if parsed.code != 0 {
                let msg = parsed
                    .msg
                    .unwrap_or_else(|| format!("code={}", parsed.code));
                tracing::warn!("MinerU 云端轮询返回非零 code: {msg}");
                return Err(OcrError::Upstream(0, msg));
            }

            let data = match parsed.data {
                Some(d) => d,
                None => {
                    tracing::debug!(
                        "MinerU 云端轮询 {}/{}: batch_id={batch_id} 等待中 (无 data)",
                        attempt + 1,
                        CLOUD_MAX_POLLS
                    );
                    continue;
                }
            };

            // 检查 extract_result 中的任务状态
            for result in &data.extract_result {
                if let Some(state) = &result.state {
                    let s = state.to_lowercase();
                    if matches!(s.as_str(), "done" | "completed" | "success" | "finished") {
                        if let Some(zip_url) = &result.full_zip_url {
                            if !zip_url.is_empty() {
                                tracing::info!(
                                    "MinerU 云端任务 {batch_id} 完成 (轮询 {attempt} 次), zip_url 已获取"
                                );
                                return Ok(zip_url.clone());
                            }
                        }
                        let err_msg = result.err_msg.as_deref().unwrap_or("任务完成但未返回 zip_url");
                        return Err(OcrError::Upstream(0, err_msg.to_string()));
                    }
                    if matches!(s.as_str(), "failed" | "error") {
                        let err_msg = result.err_msg.as_deref().unwrap_or("任务失败");
                        tracing::warn!("MinerU 云端任务 {batch_id} 失败: {err_msg}");
                        return Err(OcrError::Upstream(0, err_msg.to_string()));
                    }
                    // waiting-file / pending / running / converting → 继续轮询
                }
            }

            tracing::debug!(
                "MinerU 云端轮询 {}/{}: batch_id={batch_id} 等待中",
                attempt + 1,
                CLOUD_MAX_POLLS
            );
        }

        tracing::warn!("MinerU 云端任务 {batch_id} 轮询超时 ({CLOUD_MAX_POLLS} 次)");
        Err(OcrError::Timeout)
    }

    /// 下载 zip 并提取 `full.md`，同时把 `images/*` 图片落盘到 `{upload_dir}/questions/`
    ///
    /// Precision API 完成后返回 `full_zip_url`，下载 zip 包后：
    /// 1. 提取 `full.md` 文本
    /// 2. 收集 `images/*.png|.jpg|.jpeg|.gif|.webp` 图片字节
    /// 3. 若配置了 `upload_dir`，为每张图片生成 UUID 文件名写入 `{upload_dir}/questions/`，
    ///    并用正则把 md 中的 `![](images/xxx.png)` 引用替换为 `/uploads/questions/{uuid}.ext`
    /// 4. `upload_dir` 为 None 时（向后兼容）仅返回 md，丢弃图片字节并 warn
    async fn download_and_extract_markdown(&self, zip_url: &str) -> Result<String, OcrError> {
        tracing::info!("MinerU 云端下载结果 zip: {zip_url}");

        let resp = self.client.get(zip_url).send().await.map_err(|e| {
            if e.is_timeout() {
                OcrError::Timeout
            } else {
                tracing::warn!("MinerU 云端下载 zip 失败: {e:?}");
                OcrError::Upstream(0, format!("{e:?}"))
            }
        })?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            let preview: String = body.chars().take(200).collect();
            return Err(OcrError::Upstream(
                status,
                format!("下载 zip 失败: {preview}"),
            ));
        }

        let zip_bytes = resp.bytes().await.map_err(|e| {
            OcrError::Upstream(0, format!("读取 zip 字节失败: {e}"))
        })?;

        let cursor = std::io::Cursor::new(&zip_bytes[..]);
        let mut archive = zip::ZipArchive::new(cursor).map_err(|e| {
            OcrError::Upstream(0, format!("zip 解压失败: {e}"))
        })?;

        // 收集 full.md 文本 + images/* 图片字节
        let mut md_content: Option<String> = None;
        let mut images: Vec<(String, Vec<u8>)> = vec![]; // (zip 内完整路径, 字节)

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| {
                OcrError::Upstream(0, format!("zip 读取条目失败: {e}"))
            })?;
            let name = file.name().to_string();
            let lower = name.to_lowercase();

            // 提取 full.md（可能在子目录中）
            if name.ends_with("full.md") {
                let mut content = String::new();
                file.read_to_string(&mut content).map_err(|e| {
                    OcrError::Upstream(0, format!("读取 full.md 失败: {e}"))
                })?;
                if !content.trim().is_empty() {
                    md_content = Some(content);
                }
                continue;
            }

            // 提取 images/* 图片（路径含 images/ 前缀，扩展名是常见图片格式）
            if (lower.contains("images/") || lower.contains("images\\"))
                && (lower.ends_with(".png")
                    || lower.ends_with(".jpg")
                    || lower.ends_with(".jpeg")
                    || lower.ends_with(".gif")
                    || lower.ends_with(".webp"))
            {
                let mut bytes = Vec::new();
                if let Err(e) = file.read_to_end(&mut bytes) {
                    tracing::warn!("读取 zip 内图片 {name} 失败: {e}");
                    continue;
                }
                images.push((name.clone(), bytes));
            }
        }

        let mut md = md_content.ok_or_else(|| {
            OcrError::Upstream(0, "zip 中未找到 full.md".to_string())
        })?;

        tracing::info!(
            "MinerU 云端已提取 Markdown ({} 字符) + {} 张图片",
            md.chars().count(),
            images.len()
        );

        // 落盘图片 + 替换 md 路径（仅当配置了 upload_dir）
        if let Some(upload_dir) = &self.upload_dir {
            if images.is_empty() {
                return Ok(md);
            }

            let questions_dir = format!("{}/questions", upload_dir.trim_end_matches('/'));
            if let Err(e) = tokio::fs::create_dir_all(&questions_dir).await {
                tracing::warn!("创建图片目录 {questions_dir} 失败: {e}，跳过图片落盘");
                return Ok(md);
            }

            // 构建路径映射：images/xxx.png → /uploads/questions/{uuid}.png
            let mut url_map: HashMap<String, String> = HashMap::new();
            let mut written = 0usize;

            for (orig_path, bytes) in &images {
                // 提取文件名（images/abc.png → abc.png）
                let orig_filename = Path::new(orig_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("image");
                let ext = Path::new(orig_filename)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("png");
                let new_name = format!("{}.{}", Uuid::new_v4(), ext);
                let new_path = format!("{}/{}", questions_dir, new_name);
                let new_url = format!("/uploads/questions/{}", new_name);

                if let Err(e) = tokio::fs::write(&new_path, bytes).await {
                    tracing::warn!("写入图片失败 {orig_path} -> {new_path}: {e}");
                    continue;
                }
                // 归一化 key：去掉可能的 ./ 或 ../ 前缀，统一为 images/xxx.png
                let normalized = orig_path
                    .trim_start_matches("./")
                    .trim_start_matches("../")
                    .replace('\\', "/");
                url_map.insert(normalized, new_url);
                written += 1;
            }

            // 用正则替换 md 中所有 ![](images/xxx.png) 和 ![](./images/xxx.png)
            // group 1 = `![alt](`，group 2 = 可选 `./`|`../`，group 3 = `images/xxx.png`，group 4 = `)`
            let re = Regex::new(r"(!\[[^\]]*\]\()(\.{1,2}/)?(images/[^\s)]+)(\))")
                .map_err(|e| OcrError::Upstream(0, format!("正则编译失败: {e}")))?;

            md = re
                .replace_all(&md, |caps: &regex::Captures| -> String {
                    // group 3 = images/xxx.png 路径（非可选，必命中）
                    let orig_path = match caps.get(3) {
                        Some(m) => m.as_str(),
                        None => return caps[0].to_string(),
                    };
                    match url_map.get(orig_path) {
                        Some(new_url) => format!("{}{}{}", &caps[1], new_url, &caps[4]),
                        // 未命中映射（可能图片落盘失败），保留原样
                        None => caps[0].to_string(),
                    }
                })
                .to_string();

            tracing::info!(
                "MinerU zip 解压完成: 提取 {} 张图片（成功落盘 {}）到 {}",
                images.len(),
                written,
                questions_dir
            );
        } else if !images.is_empty() {
            tracing::warn!(
                "MinerU zip 包含 {} 张图片，但未配置 upload_dir，图片被丢弃（md 中保留 images/* 原始引用）",
                images.len()
            );
        }

        Ok(md)
    }

    // -----------------------------------------------------------------------
    // 私有部署路径（/file_parse multipart 同步）
    // -----------------------------------------------------------------------

    /// 共享上传逻辑：multipart/form-data 字段 `file` → POST /file_parse → 解析响应
    async fn upload_and_parse(
        &self,
        bytes: Vec<u8>,
        filename: &str,
        mime: &str,
    ) -> Result<String, OcrError> {
        let url = self.url("/file_parse");

        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(filename.to_string())
            .mime_str(mime)
            .map_err(|e| OcrError::Upstream(0, format!("构造 multipart 失败: {e}")))?;
        let form = reqwest::multipart::Form::new().part("file", part);

        let mut req = self.client.post(&url).multipart(form);
        if let Some(h) = self.auth_header() {
            req = req.header("Authorization", h);
        }

        let resp = req.send().await;
        let resp = match resp {
            Ok(r) => r,
            Err(e) if e.is_timeout() => return Err(OcrError::Timeout),
            Err(e) => return Err(OcrError::Upstream(0, format!("{:?}", e))),
        };

        let status = resp.status().as_u16();
        // 401/403 → 鉴权失败（仅当用户配置了 api_key 时可能出现）
        if status == 401 || status == 403 {
            let _body = resp.text().await.unwrap_or_default();
            return Err(OcrError::NoApiKey);
        }
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(OcrError::Upstream(status, body));
        }

        let parsed: FileParseResponse = resp
            .json()
            .await
            .map_err(|e| OcrError::Upstream(status, format!("响应反序列化失败: {e}")))?;

        extract_markdown(&parsed)
    }
}

// ---------------------------------------------------------------------------
// 单元测试（不访问真实网络，仅校验 URL 构造、模式判定、响应解析）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_provider() -> MineruProvider {
        MineruProvider::new("".to_string(), "http://127.0.0.1:8000".to_string())
    }

    fn make_provider_with_key() -> MineruProvider {
        MineruProvider::new("sk-mineru".to_string(), "http://mineru.internal".to_string())
    }

    fn make_cloud_provider() -> MineruProvider {
        MineruProvider::new(
            "cloud-token".to_string(),
            "https://mineru.net/api".to_string(),
        )
    }

    #[test]
    fn test_id_and_pdf_support() {
        let p = make_provider();
        assert_eq!(p.id(), "mineru_local");
        assert!(p.supports_pdf());
    }

    #[test]
    fn test_url_construction_file_parse() {
        let p = make_provider();
        assert_eq!(p.url("/file_parse"), "http://127.0.0.1:8000/file_parse");
    }

    #[test]
    fn test_url_trims_trailing_slash() {
        let p = MineruProvider::new("".into(), "http://127.0.0.1:8000/".into());
        assert_eq!(p.url("/file_parse"), "http://127.0.0.1:8000/file_parse");
    }

    #[test]
    fn test_auth_header_none_when_no_key() {
        let p = make_provider();
        assert!(p.auth_header().is_none());
    }

    #[test]
    fn test_auth_header_bearer_when_key_present() {
        let p = make_provider_with_key();
        assert_eq!(p.auth_header().as_deref(), Some("Bearer sk-mineru"));
    }

    #[test]
    fn test_empty_api_key_treated_as_none() {
        let p = MineruProvider::new("   ".to_string(), "http://x".to_string());
        assert!(p.auth_header().is_none());
    }

    // --- 云端模式判定 ---

    #[test]
    fn test_is_cloud_detects_mineru_net() {
        let p = make_cloud_provider();
        assert!(p.is_cloud());
    }

    #[test]
    fn test_is_cloud_detects_mineru_net_with_trailing_slash() {
        let p = MineruProvider::new("".into(), "https://mineru.net/api/".into());
        assert!(p.is_cloud());
    }

    #[test]
    fn test_is_cloud_case_insensitive() {
        let p = MineruProvider::new("".into(), "HTTPS://MINERU.NET/API".into());
        assert!(p.is_cloud());
    }

    #[test]
    fn test_is_cloud_false_for_private_endpoint() {
        // 私有域名 mineru.internal 不含 mineru.net，不误判为云端
        let p = make_provider_with_key();
        assert!(!p.is_cloud());

        let p2 = make_provider();
        assert!(!p2.is_cloud());
    }

    #[test]
    fn test_cloud_url_construction() {
        let p = make_cloud_provider();
        assert_eq!(
            p.url("/v4/file-urls/batch"),
            "https://mineru.net/api/v4/file-urls/batch"
        );
        assert_eq!(
            p.url("/v4/extract-results/batch/abc-123"),
            "https://mineru.net/api/v4/extract-results/batch/abc-123"
        );
    }

    // --- 私有部署响应解析 ---

    #[test]
    fn test_extract_markdown_official_md_content() {
        let raw = serde_json::json!({
            "code": 0,
            "data": { "md_content": "# 标题\n\n$E=mc^2$" }
        });
        let resp: FileParseResponse = serde_json::from_value(raw).unwrap();
        let md = extract_markdown(&resp).unwrap();
        assert_eq!(md, "# 标题\n\n$E=mc^2$");
    }

    #[test]
    fn test_extract_markdown_alias_md() {
        let raw = serde_json::json!({
            "code": 0,
            "data": { "md": "别名字段" }
        });
        let resp: FileParseResponse = serde_json::from_value(raw).unwrap();
        let md = extract_markdown(&resp).unwrap();
        assert_eq!(md, "别名字段");
    }

    #[test]
    fn test_extract_markdown_alias_markdown() {
        let raw = serde_json::json!({
            "code": 0,
            "data": { "markdown": "markdown 别名" }
        });
        let resp: FileParseResponse = serde_json::from_value(raw).unwrap();
        let md = extract_markdown(&resp).unwrap();
        assert_eq!(md, "markdown 别名");
    }

    #[test]
    fn test_extract_markdown_alias_content() {
        let raw = serde_json::json!({
            "code": 0,
            "data": { "content": "content 别名" }
        });
        let resp: FileParseResponse = serde_json::from_value(raw).unwrap();
        let md = extract_markdown(&resp).unwrap();
        assert_eq!(md, "content 别名");
    }

    #[test]
    fn test_extract_markdown_non_zero_code() {
        let raw = serde_json::json!({
            "code": 500,
            "msg": "internal error"
        });
        let resp: FileParseResponse = serde_json::from_value(raw).unwrap();
        let err = extract_markdown(&resp).unwrap_err();
        assert!(matches!(err, OcrError::Upstream(0, _)));
    }

    #[test]
    fn test_extract_markdown_missing_data() {
        let raw = serde_json::json!({ "code": 0 });
        let resp: FileParseResponse = serde_json::from_value(raw).unwrap();
        let err = extract_markdown(&resp).unwrap_err();
        assert!(matches!(err, OcrError::Upstream(0, _)));
    }

    #[test]
    fn test_extract_markdown_empty_md() {
        let raw = serde_json::json!({
            "code": 0,
            "data": { "md_content": "   " }
        });
        let resp: FileParseResponse = serde_json::from_value(raw).unwrap();
        let err = extract_markdown(&resp).unwrap_err();
        assert!(matches!(err, OcrError::Upstream(0, _)));
    }

    #[test]
    fn test_extract_markdown_no_md_fields() {
        let raw = serde_json::json!({
            "code": 0,
            "data": { "images": [] }
        });
        let resp: FileParseResponse = serde_json::from_value(raw).unwrap();
        let err = extract_markdown(&resp).unwrap_err();
        assert!(matches!(err, OcrError::Upstream(0, _)));
    }

    #[test]
    fn test_response_with_extra_fields_tolerated() {
        let raw = serde_json::json!({
            "code": 0,
            "msg": null,
            "data": {
                "md_content": "# 题目",
                "images": [{ "path": "/x.png", "type": "png" }],
                "mid_json": { "page_idx": 0 }
            }
        });
        let resp: FileParseResponse = serde_json::from_value(raw).unwrap();
        let md = extract_markdown(&resp).unwrap();
        assert_eq!(md, "# 题目");
    }

    // --- 云端 Precision API 响应解析 ---

    #[test]
    fn test_cloud_batch_response_submit() {
        // 提交阶段：返回 batch_id + file_urls
        let raw = serde_json::json!({
            "code": 0,
            "data": {
                "batch_id": "batch-abc-123",
                "file_urls": ["https://oss.example.com/signed-upload-url"]
            }
        });
        let resp: CloudBatchResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(resp.code, 0);
        let data = resp.data.unwrap();
        assert_eq!(data.batch_id.as_deref(), Some("batch-abc-123"));
        assert_eq!(data.file_urls.len(), 1);
    }

    #[test]
    fn test_cloud_batch_response_poll_done() {
        // 轮询阶段：extract_result 含 state=done + full_zip_url
        let raw = serde_json::json!({
            "code": 0,
            "data": {
                "batch_id": "batch-abc-123",
                "extract_result": [{
                    "state": "done",
                    "full_zip_url": "https://cdn.example.com/result.zip"
                }]
            }
        });
        let resp: CloudBatchResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(resp.code, 0);
        let data = resp.data.unwrap();
        assert_eq!(data.extract_result.len(), 1);
        let result = &data.extract_result[0];
        assert_eq!(result.state.as_deref(), Some("done"));
        assert_eq!(
            result.full_zip_url.as_deref(),
            Some("https://cdn.example.com/result.zip")
        );
    }

    #[test]
    fn test_cloud_batch_response_poll_processing() {
        let raw = serde_json::json!({
            "code": 0,
            "data": {
                "batch_id": "b1",
                "extract_result": [{ "state": "running" }]
            }
        });
        let resp: CloudBatchResponse = serde_json::from_value(raw).unwrap();
        let data = resp.data.unwrap();
        assert_eq!(data.extract_result[0].state.as_deref(), Some("running"));
    }

    #[test]
    fn test_cloud_batch_response_poll_failed() {
        let raw = serde_json::json!({
            "code": 0,
            "data": {
                "batch_id": "b1",
                "extract_result": [{
                    "state": "failed",
                    "err_msg": "文件格式不支持"
                }]
            }
        });
        let resp: CloudBatchResponse = serde_json::from_value(raw).unwrap();
        let data = resp.data.unwrap();
        assert_eq!(data.extract_result[0].state.as_deref(), Some("failed"));
        assert_eq!(data.extract_result[0].err_msg.as_deref(), Some("文件格式不支持"));
    }

    #[test]
    fn test_cloud_batch_request_serialization() {
        // 验证请求体 JSON 结构正确（含 is_ocr + enable_formula + model_version）
        let req = CloudBatchRequest {
            files: vec![CloudBatchFile {
                name: "input.pdf".to_string(),
            }],
            model_version: "vlm".to_string(),
            is_ocr: true,
            enable_formula: true,
            enable_table: true,
            language: "ch".to_string(),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["files"][0]["name"], "input.pdf");
        assert_eq!(json["model_version"], "vlm");
        assert_eq!(json["is_ocr"], true);
        assert_eq!(json["enable_formula"], true);
        assert_eq!(json["enable_table"], true);
        assert_eq!(json["language"], "ch");
    }
}
