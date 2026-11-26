//! 外部调用用 API Key 的生成 / 哈希 / 解析
//!
//! 规格见设计文档 §5.2：
//!
//! ```text
//! 完整密钥： ak_<env>_<prefix16>_<secret43>
//!             │    │        │         └─ 32 随机字节 → base64url（无填充，43 字符）
//!             │    │        └─────────── 12 随机字节 → base32（无填充）截断 16 字符，明文入库
//!             │    └──────────────────── live / test，仅分类展示，不参与校验
//!             └───────────────────────── 固定标签，用于「无需解码即可分流」凭证类型
//! ```
//!
//! **存储**：只存 `SHA-256(完整 key)` 的 hex（64 字符），支持等值查找且无时序侧信道。
//!
//! 与 [`crate::crypto::encrypt_api_key`]（AES-GCM 可逆，用于 LLM 供应商密钥等
//! 「需要拿回明文」的场景）的本质区别：外部 API Key 是**单向**的，
//! 数据库泄漏也无法还原出可用的密钥。

use std::fmt::Write as _;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};

/// 固定标签，出现在每个 API Key 最前面，用于凭证类型分流
pub const PREFIX_TAG: &str = "ak";

/// 定位段（明文入库，可出现在列表 / 日志里）字符数
pub const PREFIX_CHARS: usize = 16;

/// 定位段随机字节数（12 字节 → base32 20 字符，截断为 [`PREFIX_CHARS`]）
pub const LOCATOR_BYTES: usize = 12;

/// 密钥段随机字节数（256 bit 熵）
pub const SECRET_BYTES: usize = 32;

/// 密钥段字符数（32 字节 base64url 无填充）
pub const SECRET_CHARS: usize = 43;

/// base32 字母表（RFC 4648 小写，无填充）
const BASE32_ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// API Key 环境标签（仅分类 / 展示用，不参与校验）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiKeyEnv {
    /// 生产
    Live,
    /// 测试
    Test,
}

impl ApiKeyEnv {
    /// 序列化用的字面量
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Test => "test",
        }
    }

    /// 从字面量解析，非法值返回 `None`（调用方决定默认值 / 报错）
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "live" => Some(Self::Live),
            "test" => Some(Self::Test),
            _ => None,
        }
    }
}

/// 一次生成的产物
///
/// `plaintext` **只在创建 / 轮换响应里返回一次**；`key_hash` 才是入库字段。
#[derive(Debug, Clone)]
pub struct GeneratedApiKey {
    /// 完整明文密钥（`ak_live_<prefix16>_<secret43>`）
    pub plaintext: String,
    /// 环境标签
    pub env: ApiKeyEnv,
    /// 定位段（明文入库）
    pub key_prefix: String,
    /// `SHA-256(plaintext)` 的 hex
    pub key_hash: String,
}

/// 生成一个新 API Key
pub fn generate_api_key(env: ApiKeyEnv) -> GeneratedApiKey {
    let locator: [u8; LOCATOR_BYTES] = rand::random();
    let mut key_prefix = base32_nopad_lower(&locator);
    key_prefix.truncate(PREFIX_CHARS);

    let secret_bytes: [u8; SECRET_BYTES] = rand::random();
    let secret = URL_SAFE_NO_PAD.encode(secret_bytes);

    let plaintext = format!("{}_{}_{}_{}", PREFIX_TAG, env.as_str(), key_prefix, secret);
    let key_hash = hash_api_key(&plaintext);

    GeneratedApiKey {
        plaintext,
        env,
        key_prefix,
        key_hash,
    }
}

/// 计算入库用的 `key_hash`（`SHA-256` 的 hex，64 字符）
pub fn hash_api_key(full_key: &str) -> String {
    let digest = Sha256::digest(full_key.as_bytes());
    to_hex(&digest)
}

/// 判断一个 Bearer token 是否是 API Key（`ak_` 前缀）
///
/// JWT 以 `eyJ` 开头、OAuth 访问令牌以 `at_` 开头，互不冲突，
/// 因此无需解码即可分流（设计文档 §5.1）。
pub fn is_api_key_token(token: &str) -> bool {
    token.starts_with("ak_")
}

/// 取出 API Key 的定位段（第 3 段），格式非法返回 `None`
///
/// 用 `splitn(4, '_')` 解析：密钥段是 base64url，字母表包含 `-` 与 `_`，
/// 因此密钥段自身可能含下划线，必须把剩余部分整体当作密钥段。
pub fn parse_api_key_prefix(full_key: &str) -> Option<&str> {
    let mut parts = full_key.splitn(4, '_');
    let tag = parts.next()?;
    let env = parts.next()?;
    let prefix = parts.next()?;
    let secret = parts.next()?;

    if tag != PREFIX_TAG || ApiKeyEnv::parse(env).is_none() {
        return None;
    }
    if prefix.len() != PREFIX_CHARS
        || !prefix
            .bytes()
            .all(|b| BASE32_ALPHABET.contains(&b.to_ascii_lowercase()))
    {
        return None;
    }
    // 密钥段：长度固定 43，字符集为 base64url（含 '-' / '_'）
    if secret.len() != SECRET_CHARS
        || !secret
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    Some(prefix)
}

/// base32（RFC 4648 小写，无填充）编码
fn base32_nopad_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;

    for &byte in bytes {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            let idx = ((buffer >> bits) & 0x1f) as usize;
            out.push(BASE32_ALPHABET[idx] as char);
        }
    }
    if bits > 0 {
        let idx = ((buffer << (5 - bits)) & 0x1f) as usize;
        out.push(BASE32_ALPHABET[idx] as char);
    }
    out
}

/// 字节序列转小写 hex
fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // 写入 String 不会失败
        let _ = write!(&mut out, "{:02x}", byte);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_matches_rfc4648_vectors() {
        // RFC 4648 §10 测试向量（转小写、去填充）
        assert_eq!(base32_nopad_lower(b""), "");
        assert_eq!(base32_nopad_lower(b"f"), "my");
        assert_eq!(base32_nopad_lower(b"fo"), "mzxq");
        assert_eq!(base32_nopad_lower(b"foo"), "mzxw6");
        assert_eq!(base32_nopad_lower(b"foob"), "mzxw6yq");
        assert_eq!(base32_nopad_lower(b"fooba"), "mzxw6ytb");
        assert_eq!(base32_nopad_lower(b"foobar"), "mzxw6ytboi");
    }

    #[test]
    fn generated_key_has_expected_shape() {
        let generated = generate_api_key(ApiKeyEnv::Live);
        // 密钥段是 base64url，可能含 '_'，因此固定切成 4 段
        let parts: Vec<&str> = generated.plaintext.splitn(4, '_').collect();
        assert_eq!(parts.len(), 4, "格式必须是 ak_<env>_<prefix16>_<secret>");
        assert_eq!(parts[0], PREFIX_TAG);
        assert_eq!(parts[1], "live");
        assert_eq!(parts[2].len(), PREFIX_CHARS);
        assert_eq!(parts[3].len(), SECRET_CHARS, "32 字节 base64url 无填充 = 43 字符");

        assert_eq!(generated.key_prefix, parts[2]);
        assert_eq!(generated.key_hash.len(), 64, "SHA-256 hex = 64 字符");
        assert_eq!(generated.env, ApiKeyEnv::Live);
        // 定位段必须是合法 base32 字符集
        assert!(
            generated
                .key_prefix
                .bytes()
                .all(|b| BASE32_ALPHABET.contains(&b))
        );
    }

    #[test]
    fn test_env_and_uniqueness() {
        let generated = generate_api_key(ApiKeyEnv::Test);
        assert!(generated.plaintext.starts_with("ak_test_"));
        assert_eq!(generated.env.as_str(), "test");

        // 两次生成必不相同（256 bit 熵 + 独立定位段）
        let a = generate_api_key(ApiKeyEnv::Live);
        let b = generate_api_key(ApiKeyEnv::Live);
        assert_ne!(a.plaintext, b.plaintext);
        assert_ne!(a.key_hash, b.key_hash);
    }

    #[test]
    fn hash_is_deterministic_and_hex() {
        let key = "ak_live_mzxw6ytboiabcdef_Xq9AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let first = hash_api_key(key);
        let second = hash_api_key(key);
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(
            first,
            hash_api_key("ak_live_mzxw6ytboiabcdef_other")
        );
    }

    #[test]
    fn prefix_roundtrip_and_rejects_malformed() {
        let generated = generate_api_key(ApiKeyEnv::Live);
        assert_eq!(
            parse_api_key_prefix(&generated.plaintext),
            Some(generated.key_prefix.as_str())
        );

        // 非法用例：JWT / 段数不足 / 非法环境 / 非法标签 / 空密钥段
        assert_eq!(parse_api_key_prefix("eyJhbGciOiJIUzI1NiJ9.payload.sig"), None);
        assert_eq!(parse_api_key_prefix("ak_live_short_secret"), None);
        assert_eq!(
            parse_api_key_prefix("ak_staging_mzxw6ytboiabcdef_secret"),
            None
        );
        assert_eq!(
            parse_api_key_prefix("bk_live_mzxw6ytboiabcdef_secret"),
            None
        );
        assert_eq!(parse_api_key_prefix("ak_live_mzxw6ytboiabcdef_"), None);
        // 多余的一段（密钥段含 '_'）
        assert_eq!(
            parse_api_key_prefix("ak_live_mzxw6ytboiabcdef_secret_extra"),
            None
        );
        // 定位段含非 base32 字符（'8' 与 '0' 不在小写 base32 字母表内）
        assert_eq!(
            parse_api_key_prefix("ak_live_7f3c9a1b2d4e6f80_secret"),
            None
        );

        // 密钥段含 '-' / '_'（base64url 字母表）也必须能正常解析
        let underscore_secret = format!(
            "ak_live_{}_{}",
            generated.key_prefix,
            format!("{}{}", "a".repeat(SECRET_CHARS - 1), "_")
        );
        assert_eq!(
            parse_api_key_prefix(&underscore_secret),
            Some(generated.key_prefix.as_str())
        );
    }

    #[test]
    fn credential_kind_detection() {
        assert!(is_api_key_token("ak_live_mzxw6ytboiabcdef_secret"));
        assert!(!is_api_key_token("eyJhbGciOiJIUzI1NiJ9.payload.sig"));
        assert!(!is_api_key_token("at_0123456789"));
    }
}
