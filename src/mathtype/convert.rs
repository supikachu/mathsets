//! 环境配置 + HTTP `/convert` / `/convert_batch` + MathML→ole.bin CLI

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

/// MathType 转换相关环境变量
#[derive(Debug, Clone)]
pub struct MathTypeConvertConfig {
    /// 例如 `http://127.0.0.1:8099`；空则跳过 WMF 转换
    pub convert_url: Option<String>,
    /// `MathTypeOle.Cli.exe` 路径；空则跳过 ole.bin
    pub ole_cli: Option<PathBuf>,
    /// 提交审核时是否要求全部公式 `ready`（默认 false，避免 Linux CI 阻断）
    pub require_on_submit: bool,
    /// HTTP 超时
    pub timeout: Duration,
}

impl MathTypeConvertConfig {
    pub fn from_env() -> Self {
        let convert_url = std::env::var("MATHTYPE_CONVERT_URL")
            .ok()
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty());
        let ole_cli = std::env::var("MATHTYPE_OLE_CLI")
            .ok()
            .map(|s| PathBuf::from(s.trim()))
            .filter(|p| !p.as_os_str().is_empty());
        let require_on_submit = matches!(
            std::env::var("MATHTYPE_REQUIRE_ON_SUBMIT")
                .ok()
                .as_deref()
                .map(str::trim),
            Some("1") | Some("true") | Some("yes")
        );
        let timeout_secs = std::env::var("MATHTYPE_CONVERT_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(120u64);
        Self {
            convert_url,
            ole_cli,
            require_on_submit,
            timeout: Duration::from_secs(timeout_secs),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.convert_url.is_some() || self.ole_cli.is_some()
    }
}

#[derive(Debug, Serialize)]
struct ConvertBatchRequest {
    equations: Vec<BatchItemIn>,
}

#[derive(Debug, Serialize)]
struct BatchItemIn {
    id: String,
    bin_base64: String,
}

#[derive(Debug, Deserialize)]
pub struct ConvertBatchResponse {
    pub code: i32,
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub results: Vec<BatchItemOut>,
}

#[derive(Debug, Deserialize)]
pub struct BatchItemOut {
    pub id: String,
    #[serde(default)]
    pub wmf_base64: String,
    #[serde(default)]
    pub baseline_offset_pt: f64,
    #[serde(default)]
    pub width_pt: f64,
    #[serde(default)]
    pub height_pt: f64,
    #[serde(default)]
    pub code: i32,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// 单式转换结果（WMF 侧）
#[derive(Debug, Clone)]
pub struct WmfResult {
    pub wmf: Vec<u8>,
    pub baseline_offset_pt: f64,
    pub width_pt: f64,
    pub height_pt: f64,
    pub method: Option<String>,
}

pub async fn health_ok(cfg: &MathTypeConvertConfig) -> bool {
    let Some(base) = &cfg.convert_url else {
        return false;
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .ok();
    let Some(client) = client else {
        return false;
    };
    match client.get(format!("{base}/health")).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// 批量 bin → WMF；`items` 为 `(id, ole_bin_bytes)`
pub async fn convert_batch(
    cfg: &MathTypeConvertConfig,
    items: &[(String, Vec<u8>)],
) -> Result<Vec<(String, Result<WmfResult, String>)>, String> {
    let base = cfg
        .convert_url
        .as_ref()
        .ok_or_else(|| "MATHTYPE_CONVERT_URL 未配置".to_string())?;
    if items.is_empty() {
        return Ok(Vec::new());
    }

    let equations: Vec<BatchItemIn> = items
        .iter()
        .map(|(id, bin)| BatchItemIn {
            id: id.clone(),
            bin_base64: B64.encode(bin),
        })
        .collect();

    let client = reqwest::Client::builder()
        .timeout(cfg.timeout)
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .post(format!("{base}/convert_batch"))
        .json(&ConvertBatchRequest { equations })
        .send()
        .await
        .map_err(|e| format!("convert_batch 请求失败: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("convert_batch HTTP {}", resp.status()));
    }

    let body: ConvertBatchResponse = resp
        .json()
        .await
        .map_err(|e| format!("convert_batch 响应解析失败: {e}"))?;

    if body.code != 0 && body.results.is_empty() {
        return Err("convert_batch 返回 code!=0 且无 results".into());
    }

    let mut out = Vec::with_capacity(body.results.len());
    for item in body.results {
        if item.code != 0 {
            out.push((
                item.id,
                Err(item
                    .error
                    .unwrap_or_else(|| format!("convert failed code={}", item.code))),
            ));
            continue;
        }
        match B64.decode(item.wmf_base64.trim()) {
            Ok(wmf) if wmf.len() >= 22 => out.push((
                item.id,
                Ok(WmfResult {
                    wmf,
                    baseline_offset_pt: item.baseline_offset_pt,
                    width_pt: item.width_pt,
                    height_pt: item.height_pt,
                    method: item.method,
                }),
            )),
            Ok(_) => out.push((item.id, Err("WMF too short".into()))),
            Err(e) => out.push((item.id, Err(format!("wmf base64: {e}")))),
        }
    }
    Ok(out)
}

/// 调用 MathTypeOle.Cli：MathML 文件 → ole.bin
pub async fn mathml_to_ole_bin(cfg: &MathTypeConvertConfig, mathml: &str) -> Result<Vec<u8>, String> {
    let cli = cfg
        .ole_cli
        .as_ref()
        .ok_or_else(|| "MATHTYPE_OLE_CLI 未配置".to_string())?;
    if !cli.exists() {
        return Err(format!("MATHTYPE_OLE_CLI 不存在: {}", cli.display()));
    }

    let temp = std::env::temp_dir().join(format!("mathset-mt-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp)
        .await
        .map_err(|e| e.to_string())?;
    let mathml_path = temp.join("in.mathml.xml");
    let bin_path = temp.join("out.bin");
    tokio::fs::write(&mathml_path, mathml.as_bytes())
        .await
        .map_err(|e| e.to_string())?;

    let status = Command::new(cli)
        .arg(&mathml_path)
        .arg(&bin_path)
        .arg("--bin")
        .arg("--quiet")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .await
        .map_err(|e| format!("启动 MathTypeOle.Cli 失败: {e}"))?;

    if !status.success() {
        let _ = tokio::fs::remove_dir_all(&temp).await;
        return Err(format!("MathTypeOle.Cli exit {:?}", status.code()));
    }

    let bytes = tokio::fs::read(&bin_path)
        .await
        .map_err(|e| format!("读取 ole.bin 失败: {e}"))?;
    let _ = tokio::fs::remove_dir_all(&temp).await;
    if bytes.len() < 8 || bytes[0..4] != [0xD0, 0xCF, 0x11, 0xE0] {
        return Err("ole.bin 魔数无效（非 CFB）".into());
    }
    Ok(bytes)
}

/// 解析默认 CLI 路径：仓库内 Release 构建产物
pub fn default_ole_cli_hint(repo_root: &Path) -> PathBuf {
    repo_root.join(
        "tools/mathtype-ole/MathTypeOle.Cli/bin/Release/net472/MathTypeOle.Cli.exe",
    )
}
