//! 连接串构造（纯函数，便于单测）
//!
//! 端点可以写成三种形式，本模块统一归一化成 `redis://` / `rediss://`：
//! - `redis://10.0.0.7:6379`（推荐，`rediss://` 表示 TLS）；
//! - `10.0.0.7:6379`（自动补 `redis://`）；
//! - `valkey://cache:6379`（兼容写法，输出仍是 `redis://`）。
//!
//! ACL 用户名 / 密码按 RFC 3986 百分号编码后拼成 `redis://user:pass@host:port`，
//! 因此密码里的 `@`、`/`、`:` 不会破坏 URL。

use std::fmt::Write as _;

use vector::{StoreAuth, StoreConfig, VectorError, VectorResult};

/// 明文 scheme
const PLAIN_SCHEMES: [&str; 2] = ["redis", "valkey"];
/// TLS scheme
const TLS_SCHEMES: [&str; 2] = ["rediss", "valkeys"];
/// 默认端口
pub const DEFAULT_PORT: u16 = 6379;

/// 把 [`StoreConfig`] 拼成客户端可用的 URL
pub fn connect_url(config: &StoreConfig) -> VectorResult<String> {
    config.validate()?;
    let endpoint = config.endpoint.trim();
    let scheme = config.scheme().to_ascii_lowercase();
    if !scheme.is_empty()
        && !PLAIN_SCHEMES.contains(&scheme.as_str())
        && !TLS_SCHEMES.contains(&scheme.as_str())
    {
        return Err(VectorError::config(format!(
            "Redis 端点 scheme 必须是 redis:// 或 rediss://，实际：{scheme}"
        )));
    }
    let tls = TLS_SCHEMES.contains(&scheme.as_str());
    let rest = if scheme.is_empty() {
        endpoint.trim_start_matches('/')
    } else {
        endpoint
            .split_once("://")
            .map_or(endpoint, |(_, rest)| rest)
    };
    let rest = with_default_port(rest);

    let with_auth = match &config.auth {
        StoreAuth::None => rest,
        StoreAuth::Password { username, password } => {
            if authority(&rest).contains('@') {
                rest
            } else {
                format!(
                    "{}:{}@{rest}",
                    percent_encode(username.trim()),
                    percent_encode(password)
                )
            }
        }
        StoreAuth::Token { .. } => {
            return Err(VectorError::config(
                "Redis 不支持令牌认证，请使用 ACL 用户名 + 密码",
            ));
        }
        StoreAuth::AwsKey { .. } => {
            return Err(VectorError::config("Redis 不支持对象存储 AK/SK 认证"));
        }
    };
    Ok(format!(
        "{}://{with_auth}",
        if tls { "rediss" } else { "redis" }
    ))
}

/// 端点里的 key 前缀（命名空间）：`tenant_a` → `tenant_a:`
///
/// Redis 没有 schema 概念，用 key 前缀隔离租户；始终以 `:` 结尾，
/// 避免 `a` 与 `ab` 两个命名空间互相前缀命中。
pub fn key_prefix(config: &StoreConfig) -> String {
    match &config.namespace {
        Some(namespace) if !namespace.is_empty() => format!("{namespace}:"),
        _ => String::new(),
    }
}

/// 取出 authority 段（不含 scheme，截到第一个 `/`）
fn authority(url: &str) -> &str {
    let end = url.find('/').unwrap_or(url.len());
    &url[..end]
}

/// 补默认端口（authority 里没有 `:` 时补 6379）
fn with_default_port(rest: &str) -> String {
    let end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..end];
    let tail = &rest[end..];
    // IPv6 字面量 `[::1]` 里的冒号不算端口分隔
    let host_part = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let has_port = if host_part.starts_with('[') {
        host_part.contains("]:")
    } else {
        host_part.contains(':')
    };
    if has_port || host_part.is_empty() {
        rest.to_string()
    } else {
        format!("{host_part}:{DEFAULT_PORT}{tail}")
    }
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

    #[test]
    fn scheme_and_port_are_normalized() {
        assert_eq!(
            connect_url(&StoreConfig::new("10.0.0.7")).unwrap(),
            "redis://10.0.0.7:6379"
        );
        assert_eq!(
            connect_url(&StoreConfig::new("10.0.0.7:6380")).unwrap(),
            "redis://10.0.0.7:6380"
        );
        assert_eq!(
            connect_url(&StoreConfig::new("rediss://cache:6379")).unwrap(),
            "rediss://cache:6379"
        );
        assert_eq!(
            connect_url(&StoreConfig::new("valkey://cache:6379")).unwrap(),
            "redis://cache:6379"
        );
        assert_eq!(
            connect_url(&StoreConfig::new("valkeys://cache:6379")).unwrap(),
            "rediss://cache:6379"
        );

        // 带库号（path）
        let mut with_db = StoreConfig::new("cache");
        with_db.endpoint = "cache/2".to_string();
        assert_eq!(connect_url(&with_db).unwrap(), "redis://cache:6379/2");

        // 其它 scheme 拒绝
        assert!(connect_url(&StoreConfig::new("postgresql://db:5432")).is_err());
    }

    #[test]
    fn acl_credentials_are_encoded() {
        let config =
            StoreConfig::new("10.0.0.7:6379").with_auth(StoreAuth::password("ops_ro", "p@ss:1"));
        assert_eq!(
            connect_url(&config).unwrap(),
            "redis://ops_ro:p%40ss%3A1@10.0.0.7:6379"
        );

        // 用户名和密码必须成对（空白用户名由 StoreConfig::validate 拦下）
        let config = StoreConfig::new("10.0.0.7:6379").with_auth(StoreAuth::password("  ", "pw"));
        assert!(connect_url(&config).is_err());
    }

    #[test]
    fn endpoint_credentials_win_and_unsupported_auth_fails() {
        let config = StoreConfig::new("redis://inline:pw@cache:6379")
            .with_auth(StoreAuth::password("ops_ro", "other"));
        assert_eq!(
            connect_url(&config).unwrap(),
            "redis://inline:pw@cache:6379"
        );

        assert!(connect_url(&StoreConfig::new("cache").with_auth(StoreAuth::token("t"))).is_err());
        assert!(
            connect_url(&StoreConfig::new("cache").with_auth(StoreAuth::aws_key("ak", "sk")))
                .is_err()
        );
        // 有用户名没密码：由 StoreConfig::validate 拦下
        assert!(
            connect_url(&StoreConfig::new("cache").with_auth(StoreAuth::password("ops_ro", "")))
                .is_err()
        );
    }

    #[test]
    fn key_prefix_requires_namespace() {
        assert_eq!(key_prefix(&StoreConfig::new("cache")), "");
        assert_eq!(
            key_prefix(&StoreConfig::new("cache").with_namespace("tenant_a")),
            "tenant_a:"
        );
    }

    #[test]
    fn ipv6_authority_keeps_port_logic() {
        assert_eq!(with_default_port("[::1]"), "[::1]:6379");
        assert_eq!(with_default_port("[::1]:6380"), "[::1]:6380");
        assert_eq!(with_default_port("cache"), "cache:6379");
        assert_eq!(with_default_port("cache/x"), "cache:6379/x");
    }
}
