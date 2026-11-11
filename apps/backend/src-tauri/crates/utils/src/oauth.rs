//! OAuth 2.1 授权服务器的令牌 / 授权码 / PKCE 工具
//!
//! 设计文档 §7.3 / §7.4 / §7.5：
//!
//! ```text
//! 访问令牌： at_<43 字符 base64url>   —— 资源服务凭前缀即可分流（与 ak_ / eyJ 互不冲突）
//! 刷新令牌： rt_<43 字符 base64url>   —— 只用于 /oauth/token 的 refresh_token 授权
//! 授权码：   oc_<43 字符 base64url>   —— 一次性，TTL 60s，只存 SHA-256
//! 客户端 ID：oc_<16 字符 base64url>   —— 公开标识，明文入库
//! 客户端密钥：ocs_<43 字符 base64url> —— 只存 SHA-256，创建时展示一次
//! ```
//!
//! **存储**：令牌与授权码一律只存 `SHA-256` 的 hex（64 字符，`CHAR(64)`），
//! 数据库泄漏也无法还原出可用凭证（与 `utils::api_key` 的策略一致）。
//!
//! **PKCE**：只支持 `S256`，`plain` 一律拒绝（设计文档 §7.3）。

use std::fmt::Write as _;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};

/// 访问令牌前缀（凭证分流用，与 `ak_` / JWT `eyJ` 不冲突）
pub const ACCESS_TOKEN_PREFIX: &str = "at";

/// 刷新令牌前缀
pub const REFRESH_TOKEN_PREFIX: &str = "rt";

/// 授权码前缀
pub const AUTH_CODE_PREFIX: &str = "oc";

/// 客户端 ID 前缀
pub const CLIENT_ID_PREFIX: &str = "oc";

/// 客户端密钥前缀（与授权码区分，便于日志定位）
pub const CLIENT_SECRET_PREFIX: &str = "ocs";

/// 浏览器会话令牌前缀（方案 B：httpOnly Cookie，设计文档 §7.3）
pub const SESSION_TOKEN_PREFIX: &str = "ss";

/// CSRF 令牌前缀（双提交 Cookie，方案 B 的同意页表单）
pub const CSRF_TOKEN_PREFIX: &str = "cs";

/// 令牌段随机字节数（256 bit 熵）
pub const TOKEN_BYTES: usize = 32;

/// 客户端 ID 随机字节数（12 字节 → base64url 16 字符）
pub const CLIENT_ID_BYTES: usize = 12;

/// PKCE `code_verifier` 最小长度（RFC 7636 §4.1）
pub const PKCE_VERIFIER_MIN_LEN: usize = 43;

/// PKCE `code_verifier` 最大长度（RFC 7636 §4.1）
pub const PKCE_VERIFIER_MAX_LEN: usize = 128;

/// 一次生成的产物：`plaintext` 只返回给调用方一次，`hash` 才是入库字段
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedSecret {
    /// 完整明文
    pub plaintext: String,
    /// `SHA-256(plaintext)` 的 hex（64 字符）
    pub hash: String,
}

impl GeneratedSecret {
    /// 由明文构造（同步算出哈希）
    fn from_plaintext(plaintext: String) -> Self {
        let hash = hash_secret(&plaintext);
        Self { plaintext, hash }
    }
}

/// 计算入库用的 SHA-256 hex（令牌 / 授权码 / 客户端密钥通用）
pub fn hash_secret(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    to_hex(&digest)
}

/// 生成 32 随机字节的 base64url（无填充，43 字符）
fn random_segment() -> String {
    let bytes: [u8; TOKEN_BYTES] = rand::random();
    URL_SAFE_NO_PAD.encode(bytes)
}

/// 生成访问令牌（`at_<43>`）
pub fn generate_access_token() -> GeneratedSecret {
    GeneratedSecret::from_plaintext(format!("{}_{}", ACCESS_TOKEN_PREFIX, random_segment()))
}

/// 生成刷新令牌（`rt_<43>`）
pub fn generate_refresh_token() -> GeneratedSecret {
    GeneratedSecret::from_plaintext(format!("{}_{}", REFRESH_TOKEN_PREFIX, random_segment()))
}

/// 生成授权码（`oc_<43>`，TTL 60s、一次性）
pub fn generate_authorization_code() -> GeneratedSecret {
    GeneratedSecret::from_plaintext(format!("{}_{}", AUTH_CODE_PREFIX, random_segment()))
}

/// 生成客户端 ID（`oc_<16>`，明文入库）
pub fn generate_client_id() -> String {
    let bytes: [u8; CLIENT_ID_BYTES] = rand::random();
    let segment = URL_SAFE_NO_PAD.encode(bytes);
    format!("{}_{}", CLIENT_ID_PREFIX, segment)
}

/// 生成客户端密钥（`ocs_<43>`，只存哈希）
pub fn generate_client_secret() -> GeneratedSecret {
    GeneratedSecret::from_plaintext(format!("{}_{}", CLIENT_SECRET_PREFIX, random_segment()))
}

/// 判断 Bearer token 是否为 OAuth 访问令牌（`at_` 前缀）
pub fn is_access_token(token: &str) -> bool {
    token.starts_with("at_")
}

/// 判断是否为 OAuth 刷新令牌（`rt_` 前缀）
///
/// 刷新令牌**只能**用于 `/oauth/token`，出现在资源请求里一律拒绝（§13.2 #3）。
pub fn is_refresh_token(token: &str) -> bool {
    token.starts_with("rt_")
}

/// 生成浏览器会话令牌（`ss_<43>`，只存 SHA-256）
///
/// 方案 B：登录成功时写入 httpOnly Cookie，`/oauth/authorize` 据此识别
/// 「已在浏览器里登录的终端用户」（设计文档 §7.3 方案 B）。
pub fn generate_session_token() -> GeneratedSecret {
    GeneratedSecret::from_plaintext(format!("{}_{}", SESSION_TOKEN_PREFIX, random_segment()))
}

/// 生成 CSRF 令牌（`cs_<43>`）
///
/// 双提交 Cookie：一份放非 httpOnly Cookie，一份放同意页表单隐藏域，
/// 服务端比较两者是否一致（常数时间），防止第三方站点伪造同意请求。
pub fn generate_csrf_token() -> String {
    format!("{}_{}", CSRF_TOKEN_PREFIX, random_segment())
}

/// 判断是否为会话令牌（防御性校验：会话令牌不得出现在 `Authorization` 头里）
pub fn is_session_token(token: &str) -> bool {
    token.starts_with("ss_")
}

/// 常数时间字符串比较（长度不同直接返回 false）
///
/// 用于比较 PKCE challenge / 客户端密钥哈希，避免时序侧信道。
pub fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0_u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// 校验 PKCE `code_verifier` 的字符集与长度（RFC 7636 §4.1）
///
/// 允许 `[A-Za-z0-9-._~]`，长度 43–128。
pub fn is_valid_code_verifier(verifier: &str) -> bool {
    if verifier.len() < PKCE_VERIFIER_MIN_LEN || verifier.len() > PKCE_VERIFIER_MAX_LEN {
        return false;
    }
    verifier
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

/// 计算 `BASE64URL(SHA-256(ASCII(code_verifier)))`（无填充）
pub fn code_challenge_s256(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// 校验 PKCE（只支持 S256，比较过程为常数时间）
pub fn verify_pkce_s256(verifier: &str, challenge: &str) -> bool {
    if !is_valid_code_verifier(verifier) {
        return false;
    }
    constant_time_eq(&code_challenge_s256(verifier), challenge)
}

/// 字节切片转小写 hex
fn to_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut hex, "{:02x}", byte);
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_token_format_and_hash() {
        let token = generate_access_token();
        assert!(token.plaintext.starts_with("at_"));
        assert_eq!(token.plaintext.len(), 3 + 43, "at_ + 43 字符");
        assert_eq!(token.hash.len(), 64, "SHA-256 hex = 64 字符");
        assert_eq!(hash_secret(&token.plaintext), token.hash);
        assert_ne!(generate_access_token().plaintext, token.plaintext);
    }

    #[test]
    fn refresh_token_and_code_are_distinguishable() {
        let refresh = generate_refresh_token();
        let code = generate_authorization_code();
        assert!(refresh.plaintext.starts_with("rt_"));
        assert!(code.plaintext.starts_with("oc_"));
        assert!(is_refresh_token(&refresh.plaintext));
        assert!(!is_access_token(&refresh.plaintext));
        assert!(!is_refresh_token(&code.plaintext));
        // JWT / API Key 都不能被误判成 OAuth 令牌
        assert!(!is_access_token("eyJhbGciOiJIUzI1NiJ9.payload.sig"));
        assert!(!is_access_token("ak_live_mzxw6ytboiabcdef_secret"));
    }

    #[test]
    fn client_id_and_secret_formats() {
        let client_id = generate_client_id();
        assert!(client_id.starts_with("oc_"));
        assert_eq!(client_id.len(), 3 + 16, "12 字节 base64url = 16 字符");
        assert_ne!(generate_client_id(), client_id);

        let secret = generate_client_secret();
        assert!(secret.plaintext.starts_with("ocs_"));
        assert_eq!(secret.hash.len(), 64);
        assert_eq!(hash_secret(&secret.plaintext), secret.hash);
    }

    #[test]
    fn pkce_known_vector_from_rfc7636_appendix_b() {
        // RFC 7636 附录 B 的测试向量
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert_eq!(code_challenge_s256(verifier), challenge);
        assert!(verify_pkce_s256(verifier, challenge));
        assert!(!verify_pkce_s256(
            verifier,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cN"
        ));
    }

    #[test]
    fn pkce_rejects_plain_and_bad_verifiers() {
        // 长度不足 43
        assert!(!is_valid_code_verifier("short"));
        assert!(!is_valid_code_verifier(&"a".repeat(42)));
        assert!(is_valid_code_verifier(&"a".repeat(43)));
        assert!(is_valid_code_verifier(&"a".repeat(128)));
        assert!(!is_valid_code_verifier(&"a".repeat(129)));
        // 非法字符（空格 / 中文 / 加号）
        assert!(!is_valid_code_verifier(&format!("{} ", "a".repeat(42))));
        assert!(!is_valid_code_verifier(&format!("{}+", "a".repeat(42))));
        assert!(!is_valid_code_verifier(&format!("{}资", "a".repeat(42))));
        // S256 之外的算法不支持
        assert!(!verify_pkce_s256(&"a".repeat(43), "plain-value"));
    }

    #[test]
    fn constant_time_eq_semantics() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "ab"));
        assert!(constant_time_eq("", ""));
    }

    // ===== 方案 B：会话 / CSRF 令牌（设计文档 §7.3）=====

    #[test]
    fn session_token_format_and_hash() {
        let secret = generate_session_token();
        // `ss_` + 43 字符 base64url
        assert!(secret.plaintext.starts_with("ss_"));
        assert_eq!(secret.plaintext.len(), 3 + 43);
        assert_eq!(secret.hash.len(), 64);
        assert_eq!(secret.hash, hash_secret(&secret.plaintext));
        assert!(is_session_token(&secret.plaintext));
        // 与其它前缀互不冲突
        assert!(!is_access_token(&secret.plaintext));
        assert!(!is_refresh_token(&secret.plaintext));
        assert!(!secret.plaintext.starts_with("cs_"));

        // 两次生成必须不同（256 bit 熵）
        assert_ne!(generate_session_token().plaintext, secret.plaintext);
    }

    #[test]
    fn csrf_token_format() {
        let csrf = generate_csrf_token();
        assert!(csrf.starts_with("cs_"));
        assert_eq!(csrf.len(), 3 + 43);
        assert_ne!(generate_csrf_token(), csrf);
        assert!(!is_session_token(&csrf));
    }
}

