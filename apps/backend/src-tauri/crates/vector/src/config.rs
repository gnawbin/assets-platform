//! 连接配置与凭证
//!
//! 凭证只在本结构里流转：`Debug` 全部掩码，`redacted_endpoint` 会抹掉
//! URL 里的 `user:pass@`，因此可以安全地打进日志或返回给界面。

use std::fmt;

use crate::error::{VectorError, VectorResult};
use crate::model::is_identifier;

/// 默认连接 / 单次操作超时（毫秒）
pub const DEFAULT_TIMEOUT_MS: u64 = 10_000;
/// 允许配置的最小超时（毫秒）
pub const MIN_TIMEOUT_MS: u64 = 100;
/// 允许配置的最大超时（毫秒）
pub const MAX_TIMEOUT_MS: u64 = 120_000;

/// 掩码占位符
const MASKED: &str = "<已隐藏>";

/// 存储凭证
///
/// 手写 `Debug`：`Password` / `Token` / `AwsKey` 的敏感字段一律掩码输出。
#[derive(Clone, Default, PartialEq, Eq)]
pub enum StoreAuth {
    /// 匿名（本地 LanceDB 目录、无密码的 Valkey）
    #[default]
    None,
    /// 用户名 + 密码（Redis ACL、PostgreSQL 角色）
    Password {
        /// 用户名
        username: String,
        /// 密码
        password: String,
    },
    /// 令牌（Bearer / API Key）
    Token {
        /// 令牌原文
        token: String,
    },
    /// 对象存储 AK/SK（LanceDB on S3 / OSS / MinIO）
    AwsKey {
        /// Access Key ID
        access_key: String,
        /// Secret Access Key
        secret_key: String,
        /// 临时凭证的会话令牌
        session_token: Option<String>,
        /// 区域
        region: Option<String>,
        /// 自定义端点（MinIO / 私有 S3 网关）
        endpoint: Option<String>,
    },
}

impl StoreAuth {
    /// 匿名
    pub fn none() -> Self {
        Self::None
    }

    /// 用户名 + 密码
    pub fn password(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self::Password {
            username: username.into(),
            password: password.into(),
        }
    }

    /// 令牌
    pub fn token(token: impl Into<String>) -> Self {
        Self::Token {
            token: token.into(),
        }
    }

    /// 对象存储 AK/SK
    pub fn aws_key(access_key: impl Into<String>, secret_key: impl Into<String>) -> Self {
        Self::AwsKey {
            access_key: access_key.into(),
            secret_key: secret_key.into(),
            session_token: None,
            region: None,
            endpoint: None,
        }
    }

    /// 设置区域
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        if let Self::AwsKey { region: slot, .. } = &mut self {
            *slot = Some(region.into());
        }
        self
    }

    /// 设置对象存储端点
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        if let Self::AwsKey { endpoint: slot, .. } = &mut self {
            *slot = Some(endpoint.into());
        }
        self
    }

    /// 设置临时凭证会话令牌
    pub fn with_session_token(mut self, token: impl Into<String>) -> Self {
        if let Self::AwsKey {
            session_token: slot,
            ..
        } = &mut self
        {
            *slot = Some(token.into());
        }
        self
    }

    /// 稳定标识（`none` / `password` / `token` / `aws_key`）
    pub fn kind(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Password { .. } => "password",
            Self::Token { .. } => "token",
            Self::AwsKey { .. } => "aws_key",
        }
    }

    /// 是否需要凭证
    pub fn requires_credential(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// 凭证字段是否填全（`None` 视为完整）
    pub fn is_complete(&self) -> bool {
        match self {
            Self::None => true,
            Self::Password { username, password } => {
                !username.trim().is_empty() && !password.is_empty()
            }
            Self::Token { token } => !token.trim().is_empty(),
            Self::AwsKey {
                access_key,
                secret_key,
                ..
            } => !access_key.trim().is_empty() && !secret_key.trim().is_empty(),
        }
    }

    /// 用户名（仅 `Password` 有）
    pub fn username(&self) -> Option<&str> {
        match self {
            Self::Password { username, .. } => Some(username),
            _ => None,
        }
    }
}

impl fmt::Debug for StoreAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("StoreAuth::None"),
            Self::Password { username, .. } => f
                .debug_struct("StoreAuth::Password")
                .field("username", username)
                .field("password", &MASKED)
                .finish(),
            Self::Token { .. } => f
                .debug_struct("StoreAuth::Token")
                .field("token", &MASKED)
                .finish(),
            Self::AwsKey {
                access_key,
                region,
                endpoint,
                ..
            } => f
                .debug_struct("StoreAuth::AwsKey")
                .field("access_key", &MASKED)
                .field("secret_key", &MASKED)
                .field("region", region)
                .field("endpoint", endpoint)
                .field("access_key_len", &access_key.len())
                .finish(),
        }
    }
}

/// 存储连接配置
#[derive(Clone, PartialEq, Eq)]
pub struct StoreConfig {
    /// 端点：`redis://host:6379`、`postgresql://host:5432/db`、
    /// `file:///data/lance`、`s3://bucket/prefix`
    pub endpoint: String,
    /// 命名空间：PostgreSQL 取 schema、Redis 取 key 前缀、LanceDB 忽略
    pub namespace: Option<String>,
    /// 凭证
    pub auth: StoreAuth,
    /// 超时（毫秒，100..=120000）
    pub timeout_ms: u64,
}

impl StoreConfig {
    /// 以端点构造默认配置（匿名、无命名空间、10s 超时）
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            namespace: None,
            auth: StoreAuth::None,
            timeout_ms: DEFAULT_TIMEOUT_MS,
        }
    }

    /// 设置凭证
    pub fn with_auth(mut self, auth: StoreAuth) -> Self {
        self.auth = auth;
        self
    }

    /// 设置命名空间
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// 设置超时
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// 端点 scheme（无 scheme 时返回空串）
    pub fn scheme(&self) -> &str {
        match self.endpoint.trim().split_once("://") {
            Some((scheme, _)) => scheme,
            None => "",
        }
    }

    /// 端点主机部分（去掉 scheme、凭证与路径），用于日志与连接去重
    pub fn host(&self) -> &str {
        let endpoint = self.endpoint.trim();
        let rest = endpoint
            .split_once("://")
            .map_or(endpoint, |(_, rest)| rest);
        let rest = rest.rsplit_once('@').map_or(rest, |(_, host)| host);
        let end = rest.find(['/', '?']).unwrap_or(rest.len());
        &rest[..end]
    }

    /// 可安全外发的端点：抹掉 URL 里的 `user:pass@`
    pub fn redacted_endpoint(&self) -> String {
        let endpoint = self.endpoint.trim();
        let Some((scheme, rest)) = endpoint.split_once("://") else {
            return endpoint.to_string();
        };
        let authority_end = rest.find('/').unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        let tail = &rest[authority_end..];
        match authority.rsplit_once('@') {
            Some((credentials, host)) if !credentials.is_empty() => {
                let user = credentials.split(':').next().unwrap_or("");
                if user.is_empty() {
                    format!("{scheme}://{MASKED}@{host}{tail}")
                } else {
                    format!("{scheme}://{user}:{MASKED}@{host}{tail}")
                }
            }
            _ => endpoint.to_string(),
        }
    }

    /// 超时
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.timeout_ms)
    }

    /// 校验端点、超时、命名空间与凭证完整性
    pub fn validate(&self) -> VectorResult<()> {
        let endpoint = self.endpoint.trim();
        if endpoint.is_empty() {
            return Err(VectorError::config("端点不能为空"));
        }
        if endpoint.contains("://") {
            if self.scheme().is_empty() {
                return Err(VectorError::config(format!(
                    "端点 scheme 非法（形如 redis://host:6379）：{endpoint}"
                )));
            }
        } else if endpoint.contains(char::is_whitespace) {
            return Err(VectorError::config(format!("端点不能包含空白字符：{endpoint}")));
        }
        if !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&self.timeout_ms) {
            return Err(VectorError::config(format!(
                "超时必须在 {MIN_TIMEOUT_MS}..={MAX_TIMEOUT_MS} 毫秒之间：{}",
                self.timeout_ms
            )));
        }
        if let Some(namespace) = &self.namespace {
            if !is_identifier(namespace) {
                return Err(VectorError::config(format!(
                    "命名空间只能是字母 / 数字 / 下划线：{namespace}"
                )));
            }
        }
        if !self.auth.is_complete() {
            return Err(VectorError::config(format!(
                "{} 凭证不完整（用户名与密码 / 令牌 / AK 与 SK 必须成对提供）",
                self.auth.kind()
            )));
        }
        Ok(())
    }
}

impl fmt::Debug for StoreConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoreConfig")
            .field("endpoint", &self.redacted_endpoint())
            .field("namespace", &self.namespace)
            .field("auth", &self.auth)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_masked_in_debug() {
        let password = StoreAuth::password("ops_ro", "super-secret");
        let rendered = format!("{password:?}");
        assert!(!rendered.contains("super-secret"), "{rendered}");
        assert!(rendered.contains(MASKED), "{rendered}");
        assert!(rendered.contains("ops_ro"), "{rendered}");

        let token = StoreAuth::token("tok-123456");
        assert!(!format!("{token:?}").contains("tok-123456"));

        let aws = StoreAuth::aws_key("AKIAEXAMPLE", "sk-secret")
            .with_region("cn-north-1")
            .with_endpoint("http://minio:9000");
        let rendered = format!("{aws:?}");
        assert!(!rendered.contains("AKIAEXAMPLE"), "{rendered}");
        assert!(!rendered.contains("sk-secret"), "{rendered}");
        assert!(rendered.contains("cn-north-1"), "{rendered}");
        assert!(rendered.contains("minio:9000"), "{rendered}");
    }

    #[test]
    fn auth_completeness_rules() {
        assert!(StoreAuth::none().is_complete());
        assert!(!StoreAuth::none().requires_credential());
        assert!(StoreAuth::password("u", "p").is_complete());
        assert!(StoreAuth::password("u", "p").requires_credential());
        assert!(!StoreAuth::password("u", "").is_complete());
        assert!(!StoreAuth::password("  ", "p").is_complete());
        assert!(!StoreAuth::token("   ").is_complete());
        assert!(!StoreAuth::aws_key("ak", "").is_complete());
        assert_eq!(StoreAuth::password("u", "p").kind(), "password");
        assert_eq!(StoreAuth::token("t").kind(), "token");
        assert_eq!(StoreAuth::aws_key("a", "b").kind(), "aws_key");
        assert_eq!(StoreAuth::password("u", "p").username(), Some("u"));
        assert_eq!(StoreAuth::none().username(), None);
    }

    #[test]
    fn config_validation_covers_bounds() {
        assert!(StoreConfig::new("redis://10.0.0.7:6379").validate().is_ok());
        assert!(StoreConfig::new("   ").validate().is_err());
        assert!(
            StoreConfig::new("redis://h")
                .with_timeout_ms(MIN_TIMEOUT_MS - 1)
                .validate()
                .is_err()
        );
        assert!(
            StoreConfig::new("redis://h")
                .with_timeout_ms(MAX_TIMEOUT_MS + 1)
                .validate()
                .is_err()
        );
        assert!(
            StoreConfig::new("redis://h")
                .with_namespace("bad-name")
                .validate()
                .is_err()
        );
        assert!(
            StoreConfig::new("redis://h")
                .with_namespace("tenant_a")
                .validate()
                .is_ok()
        );
        assert!(
            StoreConfig::new("redis://h")
                .with_auth(StoreAuth::password("u", ""))
                .validate()
                .is_err()
        );
        assert!(StoreConfig::new("host with space:6379").validate().is_err());
        assert!(StoreConfig::new("://nohost").validate().is_err());
    }

    #[test]
    fn endpoint_helpers_parse_scheme_host_and_redact() {
        let config = StoreConfig::new("postgresql://ops_ro:s3cret@10.0.0.8:5432/assets");
        assert_eq!(config.scheme(), "postgresql");
        assert_eq!(config.host(), "10.0.0.8:5432");
        let redacted = config.redacted_endpoint();
        assert!(!redacted.contains("s3cret"), "{redacted}");
        assert!(redacted.contains("ops_ro"), "{redacted}");
        assert!(redacted.contains("10.0.0.8:5432/assets"), "{redacted}");

        // 无 scheme：原样返回
        let plain = StoreConfig::new("10.0.0.7:6379");
        assert_eq!(plain.scheme(), "");
        assert_eq!(plain.host(), "10.0.0.7:6379");
        assert_eq!(plain.redacted_endpoint(), "10.0.0.7:6379");

        // 只有密码、没有用户名
        let anonymous = StoreConfig::new("redis://:onlypass@cache:6379");
        assert_eq!(anonymous.host(), "cache:6379");
        assert!(!anonymous.redacted_endpoint().contains("onlypass"));

        // 整体 Debug 不含密码
        let config = StoreConfig::new("postgresql://ops_ro:s3cret@db:5432/assets")
            .with_auth(StoreAuth::password("ops_ro", "s3cret"));
        assert!(!format!("{config:?}").contains("s3cret"));
        assert_eq!(
            config.timeout(),
            std::time::Duration::from_millis(DEFAULT_TIMEOUT_MS)
        );
    }
}
