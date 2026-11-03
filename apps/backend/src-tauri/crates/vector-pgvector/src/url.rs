//! 连接串构造（纯函数，便于单测）
//!
//! sqlx 只接受 URL 形式的连接串，而我们在 [`StoreConfig`] 里把端点与凭证
//! 分开存（凭证单独加密保存）。本模块负责把两者拼回去，并做三件事：
//! 1. 补 scheme（允许只填 `host:port/db`）；
//! 2. 用户名 / 密码按 RFC 3986 百分号编码（`p@ss:word` 不会破坏 URL）；
//! 3. 端点自带 `user:pass@` 时以端点为准，避免重复拼接。

use std::fmt::Write as _;

use vector::{StoreAuth, StoreConfig, VectorError, VectorResult};

/// 允许的端点 scheme
const ALLOWED_SCHEMES: [&str; 3] = ["postgres", "postgresql", "pg"];

/// 把 [`StoreConfig`] 拼成 sqlx 可用的连接串
pub fn connect_url(config: &StoreConfig) -> VectorResult<String> {
    config.validate()?;
    let endpoint = config.endpoint.trim();
    let scheme = config.scheme().to_ascii_lowercase();
    if !scheme.is_empty() && !ALLOWED_SCHEMES.contains(&scheme.as_str()) {
        return Err(VectorError::config(format!(
            "pgvector 端点 scheme 必须是 postgresql://，实际：{scheme}"
        )));
    }
    let url = if scheme.is_empty() {
        format!("postgresql://{}", endpoint.trim_start_matches('/'))
    } else {
        endpoint.to_string()
    };
    match &config.auth {
        StoreAuth::None => Ok(url),
        StoreAuth::Password { username, password } => {
            if authority(&url).contains('@') {
                return Ok(url);
            }
            Ok(with_credentials(&url, username, password))
        }
        StoreAuth::Token { .. } => Err(VectorError::config(
            "PostgreSQL 只支持用户名 + 密码认证，不支持令牌",
        )),
        StoreAuth::AwsKey { .. } => Err(VectorError::config(
            "PostgreSQL 不支持对象存储 AK/SK 认证",
        )),
    }
}

/// 取出 URL 的 authority 段（不含 scheme，截到第一个 `/`）
fn authority(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let end = rest.find('/').unwrap_or(rest.len());
    &rest[..end]
}

/// 在 URL 里插入百分号编码后的 `user:pass@`
pub fn with_credentials(url: &str, username: &str, password: &str) -> String {
    let (scheme, rest) = match url.split_once("://") {
        Some((scheme, rest)) => (scheme, rest),
        None => ("postgresql", url),
    };
    format!(
        "{scheme}://{}:{}@{rest}",
        percent_encode(username),
        percent_encode(password)
    )
}

/// RFC 3986 百分号编码：只保留 unreserved 字符
pub fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char);
            }
            _ => {
                // String 的 write! 不会失败
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vector::StoreAuth;

    fn config(endpoint: &str, auth: StoreAuth) -> StoreConfig {
        StoreConfig::new(endpoint).with_auth(auth)
    }

    #[test]
    fn scheme_is_added_when_missing() {
        let url = connect_url(&config("10.0.0.8:5432/assets", StoreAuth::None)).unwrap();
        assert_eq!(url, "postgresql://10.0.0.8:5432/assets");

        let url = connect_url(&config("postgresql://10.0.0.8:5432/assets", StoreAuth::None));
        assert_eq!(url.unwrap(), "postgresql://10.0.0.8:5432/assets");

        // pg / postgres 都是合法别名
        assert!(connect_url(&config("pg://db:5432/assets", StoreAuth::None)).is_ok());
        assert!(connect_url(&config("postgres://db:5432/assets", StoreAuth::None)).is_ok());
        // 其它 scheme 直接拒绝
        assert!(connect_url(&config("redis://db:6379", StoreAuth::None)).is_err());
    }

    #[test]
    fn credentials_are_injected_and_encoded() {
        let url = connect_url(&config(
            "postgresql://10.0.0.8:5432/assets",
            StoreAuth::password("ops_ro", "p@ss:word/1"),
        ))
        .unwrap();
        assert_eq!(
            url,
            "postgresql://ops_ro:p%40ss%3Aword%2F1@10.0.0.8:5432/assets"
        );
    }

    #[test]
    fn endpoint_credentials_win_over_config() {
        let url = connect_url(&config(
            "postgresql://inline:inlinepw@10.0.0.8:5432/assets",
            StoreAuth::password("ops_ro", "other"),
        ))
        .unwrap();
        assert_eq!(url, "postgresql://inline:inlinepw@10.0.0.8:5432/assets");
        assert!(!url.contains("other"));
    }

    #[test]
    fn non_password_auth_is_rejected() {
        assert!(
            connect_url(&config(
                "postgresql://10.0.0.8:5432/assets",
                StoreAuth::token("t")
            ))
            .is_err()
        );
        assert!(
            connect_url(&config(
                "postgresql://10.0.0.8:5432/assets",
                StoreAuth::aws_key("ak", "sk")
            ))
            .is_err()
        );
        // 凭证不完整（有用户名没密码）由 StoreConfig::validate 拦下
        assert!(
            connect_url(&config(
                "postgresql://10.0.0.8:5432/assets",
                StoreAuth::password("ops_ro", "")
            ))
            .is_err()
        );
    }

    #[test]
    fn percent_encode_covers_reserved_bytes() {
        assert_eq!(percent_encode("ops_ro-1.2~x"), "ops_ro-1.2~x");
        assert_eq!(percent_encode("a b"), "a%20b");
        assert_eq!(percent_encode("密码"), "%E5%AF%86%E7%A0%81");
        assert_eq!(percent_encode("@:/?#[]"), "%40%3A%2F%3F%23%5B%5D");
    }
}
