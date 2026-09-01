//! 图片抓取器（T1.5）— 实施计划 B1
//!
//! 导出期图片统一入口：本地 `/uploads/**` → `upload_dir` 磁盘映射直读；
//! 外链（http/https）经 reqwest 拉取（超时 5s / 上限 10MB / 流式截断）。
//! 格式经 `infer` 嗅探（SVG 为文本特判），供三种生成器决定嵌入策略
//! （docx 直嵌 PNG/JPEG、SVG 跳过记警告；markdown/typst 全格式可用）。

use std::path::{Path, PathBuf};
use std::time::Duration;

/// 抓取限制（测试可注入小超时/小上限）
#[derive(Debug, Clone, Copy)]
pub struct FetchLimits {
    pub timeout: Duration,
    pub max_bytes: usize,
}

impl Default for FetchLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(5),
            max_bytes: 10 * 1024 * 1024,
        }
    }
}

/// 抓取产物：字节 + 嗅探出的扩展名
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedImage {
    pub bytes: Vec<u8>,
    /// 小写扩展名（png / jpg / gif / webp / svg …）
    pub ext: String,
    /// 本地命中还是外链拉取
    pub remote: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchImageError {
    /// URL 形态不支持（非 /uploads/** 也非 http(s)，或路径穿越）
    InvalidUrl(String),
    /// 本地文件不存在
    NotFound(PathBuf),
    /// 外链请求超时
    Timeout,
    /// 超过字节上限
    TooLarge { limit: usize },
    /// 嗅探不出图片格式
    UnsupportedFormat,
    /// 网络 / IO 错误（含非 2xx）
    Request(String),
    /// 磁盘读取错误
    Io(String),
}

impl std::fmt::Display for FetchImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUrl(u) => write!(f, "不支持的图片地址: {}", u),
            Self::NotFound(p) => write!(f, "本地图片不存在: {}", p.display()),
            Self::Timeout => write!(f, "外链图片拉取超时"),
            Self::TooLarge { limit } => write!(f, "图片超过大小上限 {} 字节", limit),
            Self::UnsupportedFormat => write!(f, "无法识别的图片格式"),
            Self::Request(m) => write!(f, "图片拉取失败: {}", m),
            Self::Io(m) => write!(f, "图片读取失败: {}", m),
        }
    }
}

impl std::error::Error for FetchImageError {}

/// 默认限制抓取（5s / 10MB）
pub async fn fetch_image(url: &str, upload_dir: &Path) -> Result<FetchedImage, FetchImageError> {
    fetch_image_with(url, upload_dir, FetchLimits::default()).await
}

pub async fn fetch_image_with(
    url: &str,
    upload_dir: &Path,
    limits: FetchLimits,
) -> Result<FetchedImage, FetchImageError> {
    if let Some(rel) = url.strip_prefix("/uploads/") {
        return fetch_local(rel, upload_dir).await;
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return fetch_remote(url, limits).await;
    }
    Err(FetchImageError::InvalidUrl(url.to_string()))
}

// ── 本地映射 ──

async fn fetch_local(rel: &str, upload_dir: &Path) -> Result<FetchedImage, FetchImageError> {
    let rel_path = Path::new(rel);
    // 路径穿越防御：拒绝任何 ".." 组件
    if rel_path
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir))
    {
        return Err(FetchImageError::InvalidUrl(format!("/uploads/{}", rel)));
    }
    let full = upload_dir.join(rel_path);
    if !full.is_file() {
        return Err(FetchImageError::NotFound(full));
    }
    let bytes = tokio::fs::read(&full)
        .await
        .map_err(|e| FetchImageError::Io(e.to_string()))?;
    let ext = sniff_format(&bytes)?;
    Ok(FetchedImage {
        bytes,
        ext,
        remote: false,
    })
}

// ── 外链拉取 ──

async fn fetch_remote(url: &str, limits: FetchLimits) -> Result<FetchedImage, FetchImageError> {
    let client = reqwest::Client::builder()
        .timeout(limits.timeout)
        .build()
        .map_err(|e| FetchImageError::Request(e.to_string()))?;
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| map_reqwest_err(e))?;
    if !resp.status().is_success() {
        return Err(FetchImageError::Request(format!("HTTP {}", resp.status())));
    }
    // Content-Length 预检（真实大小以流式累计为准）
    if let Some(len) = resp.content_length() {
        if len as usize > limits.max_bytes {
            return Err(FetchImageError::TooLarge {
                limit: limits.max_bytes,
            });
        }
    }
    let mut bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| map_reqwest_err(e))?
    {
        if bytes.len() + chunk.len() > limits.max_bytes {
            return Err(FetchImageError::TooLarge {
                limit: limits.max_bytes,
            });
        }
        bytes.extend_from_slice(&chunk);
    }
    let ext = sniff_format(&bytes)?;
    Ok(FetchedImage {
        bytes,
        ext,
        remote: true,
    })
}

fn map_reqwest_err(e: reqwest::Error) -> FetchImageError {
    if e.is_timeout() {
        FetchImageError::Timeout
    } else {
        FetchImageError::Request(e.to_string())
    }
}

// ── 格式嗅探 ──

/// infer 嗅探二进制图片；SVG（文本）特判头部
fn sniff_format(bytes: &[u8]) -> Result<String, FetchImageError> {
    if let Some(kind) = infer::get(bytes) {
        // infer 会把 <?xml 头嗅探为 xml —— 继续下探确认是否 SVG
        if kind.extension() != "xml" {
            return Ok(kind.extension().to_string());
        }
    }
    let head = &bytes[..bytes.len().min(512)];
    if let Ok(s) = std::str::from_utf8(head) {
        let t = s.trim_start();
        if t.starts_with("<svg")
            || (t.starts_with("<?xml") && s.contains("<svg"))
        {
            return Ok("svg".to_string());
        }
    }
    Err(FetchImageError::UnsupportedFormat)
}

// ═══════════════════════════ 单元测试（五条路径） ═══════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// 最小 PNG（infer 只看签名即可识别）
    const PNG_SIG: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

    fn temp_upload_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mathset-assets-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 手写最小 HTTP 服务器：`delay` 模拟慢响应（超时路径）
    async fn spawn_http_server(
        status: &'static str,
        extra_headers: &'static str,
        body: Vec<u8>,
        delay: Option<Duration>,
    ) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let body = body.clone();
                let delay = delay;
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    // 读掉请求头（够用即可）
                    let _ = sock.read(&mut buf).await;
                    if let Some(d) = delay {
                        tokio::time::sleep(d).await;
                    }
                    let head = format!(
                        "HTTP/1.1 {}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n",
                        status,
                        extra_headers,
                        body.len()
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&body).await;
                });
            }
        });
        format!("http://{}/img.png", addr)
    }

    fn small_limits() -> FetchLimits {
        FetchLimits {
            timeout: Duration::from_millis(500),
            max_bytes: 1024,
        }
    }

    #[tokio::test]
    async fn local_upload_hit() {
        let dir = temp_upload_dir();
        std::fs::create_dir_all(dir.join("questions")).unwrap();
        std::fs::write(dir.join("questions/a.png"), PNG_SIG).unwrap();

        let img = fetch_image("/uploads/questions/a.png", &dir).await.unwrap();
        assert_eq!(img.bytes, PNG_SIG);
        assert_eq!(img.ext, "png");
        assert!(!img.remote);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn local_traversal_rejected_and_miss() {
        let dir = temp_upload_dir();
        // 路径穿越拒绝
        let err = fetch_image("/uploads/../secret.txt", &dir).await.unwrap_err();
        assert!(matches!(err, FetchImageError::InvalidUrl(_)));
        // 本地未命中
        let err = fetch_image("/uploads/questions/missing.png", &dir).await.unwrap_err();
        assert!(matches!(err, FetchImageError::NotFound(_)));
        // 非法形态
        let err = fetch_image("ftp://x/y.png", &dir).await.unwrap_err();
        assert!(matches!(err, FetchImageError::InvalidUrl(_)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn remote_success() {
        let url = spawn_http_server("200 OK", "", PNG_SIG.to_vec(), None).await;
        let img = fetch_image(&url, Path::new("./uploads")).await.unwrap();
        assert_eq!(img.ext, "png");
        assert!(img.remote);
        assert_eq!(img.bytes, PNG_SIG);
    }

    #[tokio::test]
    async fn remote_timeout() {
        // 服务器延迟 2s，客户端超时 500ms
        let url = spawn_http_server("200 OK", "", PNG_SIG.to_vec(), Some(Duration::from_secs(2))).await;
        let err = fetch_image_with(&url, Path::new("./uploads"), small_limits())
            .await
            .unwrap_err();
        assert_eq!(err, FetchImageError::Timeout);
    }

    #[tokio::test]
    async fn remote_too_large() {
        // body 超过 1KB 上限（无 Content-Length 预检也靠流式累计截断）
        let url = spawn_http_server("200 OK", "", vec![0u8; 4096], None).await;
        let err = fetch_image_with(&url, Path::new("./uploads"), small_limits())
            .await
            .unwrap_err();
        assert_eq!(
            err,
            FetchImageError::TooLarge { limit: 1024 }
        );
    }

    #[tokio::test]
    async fn remote_unsupported_format() {
        let url = spawn_http_server("200 OK", "", b"hello world, not an image".to_vec(), None).await;
        let err = fetch_image(&url, Path::new("./uploads")).await.unwrap_err();
        assert_eq!(err, FetchImageError::UnsupportedFormat);
    }

    #[tokio::test]
    async fn remote_http_error_status() {
        let url = spawn_http_server("404 Not Found", "", Vec::new(), None).await;
        let err = fetch_image(&url, Path::new("./uploads")).await.unwrap_err();
        assert!(matches!(err, FetchImageError::Request(m) if m.contains("404")));
    }

    #[tokio::test]
    async fn svg_sniffed_from_text() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10"></svg>"#;
        assert_eq!(sniff_format(svg).unwrap(), "svg");
        let xml_svg = br#"<?xml version="1.0"?><svg></svg>"#;
        assert_eq!(sniff_format(xml_svg).unwrap(), "svg");
        assert_eq!(sniff_format(PNG_SIG).unwrap(), "png");
        assert_eq!(sniff_format(b"plain text"), Err(FetchImageError::UnsupportedFormat));
    }
}
