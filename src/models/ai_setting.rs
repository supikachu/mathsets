use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 数据库行
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserAiSetting {
    pub user_id: Uuid,
    pub provider: String,
    pub api_key_enc: Option<Vec<u8>>,
    pub api_key_iv: Option<Vec<u8>>,
    pub model_text: Option<String>,
    pub model_vision: Option<String>,
    pub updated_at: DateTime<Utc>,
    // ── M3：OCR 引擎配置 ──
    pub ocr_provider: String,
    pub doc2x_api_key_enc: Option<Vec<u8>>,
    pub doc2x_api_key_iv: Option<Vec<u8>>,
    pub mineru_api_endpoint: Option<String>,
    pub mineru_api_key_enc: Option<Vec<u8>>,
    pub mineru_api_key_iv: Option<Vec<u8>>,
}

/// API 响应（不返回明文 Key，仅返回脱敏标志位）
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AiSettingsResponse {
    pub provider: String,
    pub has_api_key: bool,
    pub model_text: Option<String>,
    pub model_vision: Option<String>,
    // ── M3：OCR 引擎配置（脱敏） ──
    pub ocr_provider: String,
    pub has_doc2x_key: bool,
    pub mineru_endpoint: Option<String>,
    pub has_mineru_key: bool,
}

/// 更新请求
#[derive(Deserialize, Debug, Clone)]
pub struct UpdateAiSettingsRequest {
    pub provider: Option<String>,
    pub api_key: Option<String>,
    pub model_text: Option<String>,
    pub model_vision: Option<String>,
    // ── M3：OCR 引擎配置 ──
    /// OCR 引擎：auto / doc2x / mineru / qwen_vl
    pub ocr_provider: Option<String>,
    /// 明文 Doc2X API Key（后端 AES 加密后存储；空字符串=清除，None=不变）
    pub doc2x_api_key: Option<String>,
    /// MinerU 私有部署端点（明文）
    pub mineru_endpoint: Option<String>,
    /// 明文 MinerU API Key（后端 AES 加密后存储；M4 启用）
    pub mineru_api_key: Option<String>,
}

/// 从 base64 字符串解析 32 字节主密钥
pub fn parse_master_key(b64: &str) -> Result<[u8; 32], String> {
    let key_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| format!("base64 解码失败: {e}"))?;
    if key_bytes.len() != 32 {
        return Err(format!(
            "主密钥长度必须为 32 字节，当前 {} 字节",
            key_bytes.len()
        ));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&key_bytes);
    Ok(arr)
}

/// AES-256-GCM 加密 API Key
pub fn encrypt_api_key(
    plaintext: &str,
    master_key: &[u8; 32],
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(master_key));
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng); // 12 字节
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|e| format!("加密失败: {e}"))?;
    Ok((ciphertext, nonce.to_vec()))
}

/// AES-256-GCM 解密 API Key
pub fn decrypt_api_key(
    ciphertext: &[u8],
    nonce: &[u8],
    master_key: &[u8; 32],
) -> Result<String, String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(master_key));
    let nonce = Nonce::from_slice(nonce);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("解密失败: {e}"))?;
    String::from_utf8(plaintext).map_err(|e| format!("UTF-8 转换失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as BASE64;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        // 生成 32 字节测试密钥
        let master_key = [42u8; 32];
        let plaintext = "sk-test-api-key-12345";

        let (ciphertext, nonce) = encrypt_api_key(plaintext, &master_key).unwrap();
        let decrypted = decrypt_api_key(&ciphertext, &nonce, &master_key).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_decrypt_with_wrong_key_fails() {
        let master_key = [42u8; 32];
        let wrong_key = [99u8; 32];
        let plaintext = "sk-test-api-key-12345";

        let (ciphertext, nonce) = encrypt_api_key(plaintext, &master_key).unwrap();
        let result = decrypt_api_key(&ciphertext, &nonce, &wrong_key);

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_master_key() {
        // 32 字节的 base64
        let key_bytes = [0u8; 32];
        let b64 = BASE64.encode(key_bytes);
        let parsed = parse_master_key(&b64).unwrap();
        assert_eq!(parsed, key_bytes);
    }

    #[test]
    fn test_parse_master_key_wrong_length() {
        let key_bytes = [0u8; 16]; // 只有 16 字节
        let b64 = BASE64.encode(key_bytes);
        assert!(parse_master_key(&b64).is_err());
    }
}
