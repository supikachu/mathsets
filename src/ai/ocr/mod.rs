//! OCR 引擎抽象层
//!
//! 解耦「图片/PDF → Markdown」与「Markdown → 结构化 JSON」两阶段流水线。
//! 本模块只负责 Stage 1：将图片（或 PDF）识别为含 LaTeX 与图片链接的纯 Markdown，
//! Stage 2 由 `ai::provider::AiProvider`（DeepSeek-V3 文本模型）复用现有结构化解析。
//!
//! M1 实现 `QwenVlOcrProvider`（兜底，等价重构前的视觉识别路径）；
//! M2 接入 `Doc2XProvider`（Doc2X 同步图片 + 异步 PDF），实现同一 `OcrProvider` trait 热插拔；
//! M4 接入 `MineruProvider`（私有部署，原支持 PDF/图片同步解析）。

pub mod doc2x;
pub mod mineru;
pub mod qwen_vl;

pub use doc2x::Doc2XProvider;
pub use mineru::MineruProvider;
pub use qwen_vl::QwenVlOcrProvider;

use async_trait::async_trait;

/// OCR 引擎调用错误
///
/// 与 `ai::provider::AiError` 对应，但语义聚焦于 OCR 层，
/// 便于在两阶段流水线中区分 Stage 1（OCR）与 Stage 2（LLM）的失败来源。
#[derive(Debug)]
pub enum OcrError {
    /// 引擎不支持 PDF 直传（如 Qwen-VL 仅支持图片）
    UnsupportedPdf,
    /// 未配置引擎所需 API Key
    NoApiKey,
    /// 上游服务错误（HTTP 状态码 + 消息）
    Upstream(u16, String),
    /// 请求超时
    Timeout,
}

/// 将 `AiError` 映射为 `OcrError`（Stage 1 OCR 内部复用 DeepSeek vision 调用时转换）
pub(crate) fn map_ai_to_ocr_error(e: crate::ai::provider::AiError) -> OcrError {
    match e {
        crate::ai::provider::AiError::NoApiKey => OcrError::NoApiKey,
        crate::ai::provider::AiError::Timeout => OcrError::Timeout,
        crate::ai::provider::AiError::Upstream(code, msg) => OcrError::Upstream(code, msg),
    }
}

/// PDF 直传进度回调：参数为 0~100 百分比
///
/// 引擎轮询期间周期性触发（如 Doc2X 每 3s 一次）；
/// 无数值进度的引擎（MinerU 云端）不触发，调用方依赖心跳保活。
pub type PdfProgressCallback = std::sync::Arc<dyn Fn(u8) + Send + Sync>;

/// 从 JSON Value 解析百分比（0~100）：支持数字与数字字符串，其余 → None
pub(crate) fn parse_percent_value(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// OCR 引擎 trait
///
/// 每个引擎实现本 trait 即可被 `create_ocr_provider` 工厂装配到两阶段流水线。
/// `ocr_image` 输出纯 Markdown（含 `$...$` / `$$...$$` LaTeX 与 `![图](url)` 图片链接），
/// 不输出 JSON、不输出代码块标记。
#[async_trait]
pub trait OcrProvider: Send + Sync {
    /// 引擎标识，用于日志与配额记账：`"qwen_vl"` | `"doc2x"` | `"mineru_local"` | `"mineru_api"`
    fn id(&self) -> &'static str;

    /// 单图 OCR → Markdown（含 LaTeX）
    ///
    /// `image_b64` 为 base64 编码的图片数据，不含 `data:image` 前缀。
    async fn ocr_image(&self, image_b64: &str) -> Result<String, OcrError>;

    /// 是否支持 PDF 直传（Doc2X / MinerU 原生支持，Qwen-VL 不支持）
    fn supports_pdf(&self) -> bool {
        false
    }

    /// PDF 原生异步 OCR → 全文 Markdown（v1.1）
    ///
    /// 仅 `supports_pdf()=true` 的引擎实现（Doc2X 走 submit→poll 异步路径）。
    /// 默认实现返回 `UnsupportedPdf`，Qwen-VL 走前端逐页图片兜底。
    async fn ocr_pdf_async(&self, _pdf_bytes: &[u8]) -> Result<String, OcrError> {
        Err(OcrError::UnsupportedPdf)
    }

    /// PDF 直传（带进度回调）→ 全文 Markdown
    ///
    /// 默认委托 `ocr_pdf_async`（不触发回调）；有数值进度的引擎（Doc2X）
    /// 覆盖本方法在轮询循环中上报 0~100 百分比。
    async fn ocr_pdf_async_with_progress(
        &self,
        pdf_bytes: &[u8],
        _on_progress: &PdfProgressCallback,
    ) -> Result<String, OcrError> {
        self.ocr_pdf_async(pdf_bytes).await
    }
}

/// OCR 引擎配置（由 `resolve_ocr_config` 填充后传入工厂）
///
/// M1 仅承载 `qwen_vl` 所需字段；M2 扩展 doc2x（base_url + api_key）；
/// M4 扩展 mineru 时在此结构追加。
#[derive(Debug, Clone)]
pub struct OcrConfig {
    /// 解析后的引擎 id：`"qwen_vl"` | `"doc2x"`
    pub provider: String,
    /// 引擎 API Key
    pub api_key: String,
    /// 引擎 base_url（OpenAI 兼容端点 / Doc2X v1 端点）
    pub base_url: String,
    /// 视觉模型名（如 `qwen-vl-plus`），None 走 provider 默认；Doc2X 不使用此字段
    pub model: Option<String>,
    /// 题目图片落盘根目录（如 "./uploads"），用于 MinerU 云端模式解压 zip 时搬运 images/*
    pub upload_dir: Option<String>,
}

/// 工厂：按配置创建 OCR 引擎实例
///
/// - `qwen_vl` / `auto` / 空 → `QwenVlOcrProvider`（兜底，等价重构前行为）
/// - `doc2x` → `Doc2XProvider`（M2 接入，原生支持 PDF）
/// - `mineru_local` / `mineru_api` → `MineruProvider`（M4 接入，私有部署，原生支持 PDF）
/// - 未知 provider → 兜底 qwen_vl，保证未配置用户行为等价重构前（AC-07）
pub fn create_ocr_provider(cfg: &OcrConfig) -> Box<dyn OcrProvider> {
    match cfg.provider.as_str() {
        "doc2x" => Box::new(Doc2XProvider::new(
            cfg.api_key.clone(),
            cfg.base_url.clone(),
        )),
        "mineru_local" | "mineru_api" => Box::new(
            MineruProvider::new(cfg.api_key.clone(), cfg.base_url.clone())
                .with_upload_dir(cfg.upload_dir.clone()),
        ),
        "qwen_vl" | "auto" | "" => Box::new(QwenVlOcrProvider::new(
            cfg.api_key.clone(),
            cfg.base_url.clone(),
            cfg.model.clone(),
        )),
        other => {
            // 兜底 qwen_vl，避免未知 provider 导致整个流水线中断
            tracing::warn!(
                "OCR 引擎 `{other}` 在当前版本未实现，自动降级 qwen_vl 兜底"
            );
            Box::new(QwenVlOcrProvider::new(
                cfg.api_key.clone(),
                cfg.base_url.clone(),
                cfg.model.clone(),
            ))
        }
    }
}

/// 判断 OCR 错误是否应触发引擎降级（M2 新增）
///
/// 仅在「切换到 Qwen-VL 兜底可能成功」的错误上返回 true：
/// - `NoApiKey` — 主引擎 Key 无效，切 Qwen-VL（用平台 Vision Key）可恢复
/// - `Timeout` — 主引擎超时，切 Qwen-VL 重试
/// - `Upstream(429, _)` — 限流，切引擎避开
/// - `Upstream(401 | 403, _)` — 鉴权失败，切 Qwen-VL
/// - `Upstream(0, _)` — 网络/解析层错误，切 Qwen-VL 重试
///
/// 不降级的情况：
/// - `UnsupportedPdf` — Qwen-VL 也不支持 PDF，降级无意义
/// - `Upstream(5xx, _)` — 主引擎整体宕机；切 Qwen-VL 也可能受影响，让错误透传更清晰
/// - `Upstream(4xx 其他, _)` — 400/404 等客户端请求格式问题，切引擎不解决
pub fn should_fallback(e: &OcrError) -> bool {
    match e {
        OcrError::NoApiKey | OcrError::Timeout => true,
        OcrError::UnsupportedPdf => false,
        OcrError::Upstream(code, _) => matches!(code, 0 | 401 | 403 | 429),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_ocr_provider_auto_maps_to_qwen_vl() {
        let cfg = OcrConfig {
            provider: "auto".into(),
            api_key: "k".into(),
            base_url: "https://example.com".into(),
            model: None,
            upload_dir: None,
        };
        let p = create_ocr_provider(&cfg);
        assert_eq!(p.id(), "qwen_vl");
        assert!(!p.supports_pdf());
    }

    #[test]
    fn test_create_ocr_provider_qwen_vl() {
        let cfg = OcrConfig {
            provider: "qwen_vl".into(),
            api_key: "k".into(),
            base_url: "https://example.com".into(),
            model: Some("qwen-vl-plus".into()),
            upload_dir: None,
        };
        let p = create_ocr_provider(&cfg);
        assert_eq!(p.id(), "qwen_vl");
    }

    #[test]
    fn test_create_ocr_provider_doc2x() {
        // M2：doc2x 现已实现，工厂应直接装配 Doc2XProvider
        // v2 迁移：base_url 改为官方 v2 裸域名
        let cfg = OcrConfig {
            provider: "doc2x".into(),
            api_key: "sk-test".into(),
            base_url: "https://v2.doc2x.noedgeai.com".into(),
            model: None,
            upload_dir: None,
        };
        let p = create_ocr_provider(&cfg);
        assert_eq!(p.id(), "doc2x");
        assert!(p.supports_pdf());
    }

    #[test]
    fn test_create_ocr_provider_mineru_local() {
        // M4：mineru_local 现已实现，工厂应直接装配 MineruProvider
        let cfg = OcrConfig {
            provider: "mineru_local".into(),
            api_key: "sk-test".into(),
            base_url: "http://127.0.0.1:8000".into(),
            model: None,
            upload_dir: None,
        };
        let p = create_ocr_provider(&cfg);
        assert_eq!(p.id(), "mineru_local");
        assert!(p.supports_pdf());
    }

    #[test]
    fn test_create_ocr_provider_mineru_api_alias() {
        // mineru_api 与 mineru_local 等价（仅命名差异，装配同一 Provider）
        let cfg = OcrConfig {
            provider: "mineru_api".into(),
            api_key: "".into(),
            base_url: "http://mineru.internal".into(),
            model: None,
            upload_dir: None,
        };
        let p = create_ocr_provider(&cfg);
        assert_eq!(p.id(), "mineru_local");
    }

    #[test]
    fn test_create_ocr_provider_unknown_falls_back_to_qwen_vl() {
        let cfg = OcrConfig {
            provider: "nonexistent_engine".into(),
            api_key: "k".into(),
            base_url: "https://example.com".into(),
            model: None,
            upload_dir: None,
        };
        let p = create_ocr_provider(&cfg);
        // 未知引擎兜底为 qwen_vl
        assert_eq!(p.id(), "qwen_vl");
    }

    #[tokio::test]
    async fn test_ocr_pdf_async_default_unsupported() {
        let cfg = OcrConfig {
            provider: "qwen_vl".into(),
            api_key: "k".into(),
            base_url: "https://example.com".into(),
            model: None,
            upload_dir: None,
        };
        let p = create_ocr_provider(&cfg);
        let res = p.ocr_pdf_async(b"fake-pdf").await;
        assert!(matches!(res, Err(OcrError::UnsupportedPdf)));
    }

    #[test]
    fn test_map_ai_to_ocr_error() {
        assert!(matches!(
            map_ai_to_ocr_error(crate::ai::provider::AiError::NoApiKey),
            OcrError::NoApiKey
        ));
        assert!(matches!(
            map_ai_to_ocr_error(crate::ai::provider::AiError::Timeout),
            OcrError::Timeout
        ));
        assert!(matches!(
            map_ai_to_ocr_error(crate::ai::provider::AiError::Upstream(429, "x".into())),
            OcrError::Upstream(429, _)
        ));
    }

    #[test]
    fn test_should_fallback_recoverable_errors() {
        // 可恢复错误：切换 Qwen-VL 兜底
        assert!(should_fallback(&OcrError::NoApiKey));
        assert!(should_fallback(&OcrError::Timeout));
        assert!(should_fallback(&OcrError::Upstream(0, "network".into())));
        assert!(should_fallback(&OcrError::Upstream(401, "unauthorized".into())));
        assert!(should_fallback(&OcrError::Upstream(403, "forbidden".into())));
        assert!(should_fallback(&OcrError::Upstream(429, "rate limit".into())));
    }

    #[test]
    fn test_should_fallback_unrecoverable_errors() {
        // 不可恢复错误：让错误透传，不切兜底
        assert!(!should_fallback(&OcrError::UnsupportedPdf));
        assert!(!should_fallback(&OcrError::Upstream(400, "bad request".into())));
        assert!(!should_fallback(&OcrError::Upstream(404, "not found".into())));
        assert!(!should_fallback(&OcrError::Upstream(500, "internal".into())));
        assert!(!should_fallback(&OcrError::Upstream(502, "bad gateway".into())));
        assert!(!should_fallback(&OcrError::Upstream(503, "unavailable".into())));
    }
}
