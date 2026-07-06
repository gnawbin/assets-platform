//! 加密工具模块
//!
//! 提供 AES-256-GCM 加密/解密功能，用于 API Key 等敏感信息的安全存储。

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

/// AES-256-GCM 密钥长度（32 字节）
const KEY_LEN: usize = 32;
/// AES-GCM nonce 长度（12 字节）
const NONCE_LEN: usize = 12;

/// 加密 API Key（返回 base64 编码的密文）
///
/// 格式：base64(nonce + ciphertext)
pub fn encrypt_api_key(plaintext: &str) -> Result<String, String> {
    let master_key = std::env::var("LLM_KEY_ENCRYPT_KEY")
        .map_err(|_| "未设置 LLM_KEY_ENCRYPT_KEY 环境变量".to_string())?;

    let key_bytes = master_key.as_bytes();
    if key_bytes.len() < KEY_LEN {
        return Err(format!(
            "LLM_KEY_ENCRYPT_KEY 长度不足，需要 {} 字节",
            KEY_LEN
        ));
    }

    // 取前 32 字节作为 AES 密钥
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&key_bytes[..KEY_LEN]);

    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("创建 AES 密钥失败: {}", e))?;

    // 生成随机 nonce
    let nonce_bytes: [u8; NONCE_LEN] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);

    // 加密
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| format!("加密失败: {}", e))?;

    // 拼接 nonce + ciphertext，返回 base64
    let mut result = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(BASE64.encode(&result))
}

/// 解密 API Key
///
/// 输入：base64(nonce + ciphertext)
pub fn decrypt_api_key(encrypted: &str) -> Result<String, String> {
    let master_key = std::env::var("LLM_KEY_ENCRYPT_KEY")
        .map_err(|_| "未设置 LLM_KEY_ENCRYPT_KEY 环境变量".to_string())?;

    let key_bytes = master_key.as_bytes();
    if key_bytes.len() < KEY_LEN {
        return Err(format!(
            "LLM_KEY_ENCRYPT_KEY 长度不足，需要 {} 字节",
            KEY_LEN
        ));
    }

    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&key_bytes[..KEY_LEN]);

    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("创建 AES 密钥失败: {}", e))?;

    // 解码 base64
    let data = BASE64
        .decode(encrypted)
        .map_err(|e| format!("base64 解码失败: {}", e))?;

    if data.len() < NONCE_LEN {
        return Err("密文长度不足".to_string());
    }

    let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);

    // 解密
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("解密失败: {}", e))?;

    String::from_utf8(plaintext).map_err(|e| format!("UTF-8 解码失败: {}", e))
}

/// 对 API Key 进行脱敏处理（仅用于前端展示）
///
/// 例如：sk-proj-xxxx...xxxxa3f（前后各保留 4 个字符）
pub fn mask_api_key(key: &str) -> String {
    if key.len() <= 12 {
        return "****".to_string();
    }
    let prefix = &key[..4];
    let suffix = &key[key.len() - 4..];
    let masked_len = key.len() - 8;
    let mut masked = String::with_capacity(key.len());
    masked.push_str(prefix);
    for _ in 0..masked_len.min(20) {
        masked.push('*');
    }
    masked.push_str(suffix);
    masked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        std::env::set_var("LLM_KEY_ENCRYPT_KEY", "abcdefghijklmnopqrstuvwxyz123456");
        let original = "sk-test-api-key-12345";
        let encrypted = encrypt_api_key(original).unwrap();
        assert_ne!(encrypted, original);
        let decrypted = decrypt_api_key(&encrypted).unwrap();
        assert_eq!(decrypted, original);
        std::env::remove_var("LLM_KEY_ENCRYPT_KEY");
    }

    #[test]
    fn test_mask_api_key() {
        let key = "sk-proj-abcdefg12345a3f";
        let masked = mask_api_key(key);
        assert!(masked.contains("sk-p"));
        assert!(masked.contains("a3f"));
        assert!(masked.contains('*'));
    }
}
