//! 连接器抽象层
//!
//! [`Connector`] 只有三件事：健康探测（`health`）、全量采集（`collect`）、
//! 是否支持增量订阅（`supports_watch`，本轮全部 `false`，watch 留给调度器迭代）。
//! 所有连接参数统一收敛到 [`ConnectionConfig`]，凭证由 [`AuthKind`] 承载并且
//! **永不进入日志**（`Debug` 已掩码）。
//!
//! 只读纪律（方案 §6）：任何实现都不得调用 exec / start / stop / delete 类接口，
//! 不得直连 K8s 节点的 `docker.sock`，ConfigMap / Secret 只取元数据。

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{FailureClass, OpsError, OpsResult};
use crate::model::{Provider, ResourceSnapshot};

#[cfg(feature = "docker")]
pub mod docker;
#[cfg(feature = "k8s")]
pub mod k8s;

// 数据存储（Redis / Valkey、PostgreSQL+pgvector、LanceDB）的采集实现不在这里：
// 驱动依赖分别归属 crates/vector-redis、crates/vector-pgvector、crates/vector-lance，
// 它们通过 crates/vector 的 `VectorStore` 抽象对外提供能力，再经 service 组装成本 crate
// 的 [`Connector`]。本模块只保留协议无关的 [`AuthKind`] / [`ConnectionConfig`]。

/// 连接认证方式
///
/// 刻意手写 `Debug`：凭证字段必须掩码后输出，避免误入日志。
#[derive(Clone)]
pub enum AuthKind {
    /// 本机 socket（Linux `/var/run/docker.sock`、Windows `npipe:////./pipe/docker_engine`）
    LocalSocket,
    /// 明文 HTTP 端点（`tcp://host:2375`，仅限隔离内网，界面需提示风险）
    PlainTcp,
    /// 双向 TLS（Docker `https://host:2376`，需 CA + 客户端证书 + 私钥）
    MutualTls {
        /// CA 证书路径（校验服务端）
        ca_path: String,
        /// 客户端证书路径
        cert_path: String,
        /// 客户端私钥路径
        key_path: String,
    },
    /// 双向 TLS，证书以 PEM 原文内联（远程录入场景：不要求后端主机上存在证书文件）
    MutualTlsInline {
        /// CA 证书 PEM 原文
        ca_pem: String,
        /// 客户端证书 PEM 原文
        cert_pem: String,
        /// 客户端私钥 PEM 原文
        key_pem: String,
    },
    /// 读取本机 kubeconfig 文件
    KubeconfigFile {
        /// kubeconfig 路径
        path: String,
        /// 指定 context，`None` 表示用 `current-context`
        context: Option<String>,
    },
    /// 直接给出 kubeconfig 内容（多集群分发场景，由前端上传）
    KubeconfigInline {
        /// kubeconfig YAML 原文
        yaml: String,
        /// 指定 context
        context: Option<String>,
    },
    /// 直连 APIServer（Bearer Token 或客户端证书二选一/并存）
    Direct {
        /// ServiceAccount / 用户 Token
        token: Option<String>,
        /// CA 证书 PEM 原文（`insecure_skip_tls_verify` 为真时可省略）
        ca_pem: Option<String>,
        /// 客户端证书 PEM 原文
        client_cert_pem: Option<String>,
        /// 客户端私钥 PEM 原文
        client_key_pem: Option<String>,
        /// 跳过证书校验（自签集群录入初期常用，落库时需审计）
        insecure_skip_tls_verify: bool,
    },
    /// Redis / Valkey ACL 用户密码（RESP `AUTH user pass`）
    ///
    /// 采集账号必须是**只读**账号（`+@read +@connection +ping +info +config|get`
    /// 之类的最小 ACL），本 crate 不会执行任何写命令。
    RedisAcl {
        /// ACL 用户名（6.0 之前固定为 `default`）
        username: String,
        /// ACL 密码
        password: String,
    },
    /// Redis / Valkey TLS（`rediss://`）+ 可选 ACL
    RedisTls {
        /// ACL 用户名，`None` 表示无认证（仅内网测试环境）
        username: Option<String>,
        /// ACL 密码
        password: Option<String>,
        /// CA 证书 PEM 原文（自签证书时需要）
        ca_pem: Option<String>,
        /// 跳过证书校验（自签环境，落库时需审计）
        insecure_skip_tls_verify: bool,
    },
    /// PostgreSQL 密码认证（pgvector 采集账号必须是**只读**角色）
    PgPassword {
        /// 只读角色名
        username: String,
        /// 角色密码
        password: String,
    },
    /// LanceDB 本地目录（后端进程可直接读写的文件系统路径）
    LanceLocal {
        /// 数据库目录（`file:///data/lance` 去掉 scheme 后的路径）
        path: String,
    },
    /// LanceDB 对象存储（S3 / MinIO / OSS 兼容 S3 协议）
    LanceCloud {
        /// Access Key ID
        access_key: String,
        /// Secret Access Key
        secret_key: String,
        /// 区域（MinIO 可留空）
        region: Option<String>,
        /// 自定义 endpoint（MinIO / 私有网关）
        endpoint: Option<String>,
    },
}

impl AuthKind {
    /// 稳定编码（落 `ops_connection.auth_type`、探针输出）
    pub const fn describe(&self) -> &'static str {
        match self {
            Self::LocalSocket => "local_socket",
            Self::PlainTcp => "plain_tcp",
            Self::MutualTls { .. } => "mutual_tls",
            Self::MutualTlsInline { .. } => "mutual_tls_inline",
            Self::KubeconfigFile { .. } => "kubeconfig_file",
            Self::KubeconfigInline { .. } => "kubeconfig_inline",
            Self::Direct { .. } => "direct",
            Self::RedisAcl { .. } => "redis_acl",
            Self::RedisTls { .. } => "redis_tls",
            Self::PgPassword { .. } => "pg_password",
            Self::LanceLocal { .. } => "lance_local",
            Self::LanceCloud { .. } => "lance_cloud",
        }
    }

    /// 该认证方式是否携带需要加密落库的凭证
    ///
    /// 与 `service::ops_service::auth_requires_credential` 必须保持一致：
    /// 前者用于「是否需要加密列」，后者用于「界面上是否提示必填」。
    pub const fn requires_credential(&self) -> bool {
        matches!(
            self,
            Self::MutualTls { .. }
                | Self::MutualTlsInline { .. }
                | Self::KubeconfigInline { .. }
                | Self::Direct { .. }
                | Self::RedisAcl { .. }
                | Self::RedisTls { .. }
                | Self::PgPassword { .. }
                | Self::LanceCloud { .. }
        )
    }

    /// 是否为「本地路径」认证（不涉网，无需凭证，也不会触发公网告警）
    pub const fn is_local_path(&self) -> bool {
        matches!(self, Self::LanceLocal { .. })
    }

    /// 是否为本机 socket（决定是否需要「仅本机可采集」提示）
    pub const fn is_local_socket(&self) -> bool {
        matches!(self, Self::LocalSocket)
    }
}

impl fmt::Debug for AuthKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const MASK: &str = "<已隐藏>";
        match self {
            Self::LocalSocket => f.write_str("LocalSocket"),
            Self::PlainTcp => f.write_str("PlainTcp"),
            Self::MutualTls { ca_path, .. } => f
                .debug_struct("MutualTls")
                .field("ca_path", ca_path)
                .field("cert_path", &MASK)
                .field("key_path", &MASK)
                .finish(),
            Self::MutualTlsInline { .. } => f
                .debug_struct("MutualTlsInline")
                .field("ca_pem", &MASK)
                .field("cert_pem", &MASK)
                .field("key_pem", &MASK)
                .finish(),
            Self::KubeconfigFile { path, context } => f
                .debug_struct("KubeconfigFile")
                .field("path", path)
                .field("context", context)
                .finish(),
            Self::KubeconfigInline { context, .. } => f
                .debug_struct("KubeconfigInline")
                .field("yaml", &MASK)
                .field("context", context)
                .finish(),
            Self::Direct {
                token,
                ca_pem,
                client_cert_pem,
                client_key_pem,
                insecure_skip_tls_verify,
            } => f
                .debug_struct("Direct")
                .field("token", &token.as_ref().map(|_| MASK))
                .field("ca_pem", &ca_pem.as_ref().map(|_| MASK))
                .field("client_cert_pem", &client_cert_pem.as_ref().map(|_| MASK))
                .field("client_key_pem", &client_key_pem.as_ref().map(|_| MASK))
                .field("insecure_skip_tls_verify", insecure_skip_tls_verify)
                .finish(),
            Self::RedisAcl { username, .. } => f
                .debug_struct("RedisAcl")
                .field("username", username)
                .field("password", &MASK)
                .finish(),
            Self::RedisTls {
                username,
                insecure_skip_tls_verify,
                ..
            } => f
                .debug_struct("RedisTls")
                .field("username", username)
                .field("password", &MASK)
                .field("ca_pem", &MASK)
                .field("insecure_skip_tls_verify", insecure_skip_tls_verify)
                .finish(),
            Self::PgPassword { username, .. } => f
                .debug_struct("PgPassword")
                .field("username", username)
                .field("password", &MASK)
                .finish(),
            Self::LanceLocal { path } => f.debug_struct("LanceLocal").field("path", path).finish(),
            Self::LanceCloud {
                region, endpoint, ..
            } => f
                .debug_struct("LanceCloud")
                .field("access_key", &MASK)
                .field("secret_key", &MASK)
                .field("region", region)
                .field("endpoint", endpoint)
                .finish(),
        }
    }
}

/// 连接层级：用于界面风险提示与超时预算建议
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionTier {
    /// 本机（socket / 127.0.0.1 / localhost）
    Local,
    /// 内网（10/8、172.16/12、192.168/16 等私有地址）
    Intranet,
    /// 跨网（公网地址，需额外确认安全策略）
    Remote,
}

impl ConnectionTier {
    /// 稳定编码
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Intranet => "intranet",
            Self::Remote => "remote",
        }
    }

    /// 由归一化端点猜测层级（仅用于提示，不作为安全决策依据）
    pub fn from_endpoint(endpoint_key: &str) -> Self {
        let endpoint_key = endpoint_key.trim();
        // 允许直接传带 scheme 的原始端点（探针 / 界面回显都用得上）
        let key = endpoint_key
            .split_once("://")
            .map_or(endpoint_key, |(_, rest)| rest);
        // 本地文件路径（LanceDB `file:///data/lance`、Windows `D:\data\lance`）不涉网，
        // 若按「域名无法判定 → Remote」处理会误报「端点为公网地址」的安全告警。
        if looks_like_local_path(key) {
            return Self::Local;
        }
        let host = strip_port(key)
            .trim_start_matches('[')
            .trim_end_matches(']');
        if matches!(host, "local" | "localhost" | "127.0.0.1" | "::1") {
            return Self::Local;
        }
        let octets: Vec<u8> = host
            .split('.')
            .filter_map(|part| part.parse::<u8>().ok())
            .collect();
        if octets.len() == 4 {
            let first = octets.first().copied().unwrap_or_default();
            let second = octets.get(1).copied().unwrap_or_default();
            let private = first == 10
                || (first == 172 && (16..=31).contains(&second))
                || (first == 192 && second == 168);
            return if private {
                Self::Intranet
            } else {
                Self::Remote
            };
        }
        // 域名无法判定，按跨网处理（由运维显式确认）
        Self::Remote
    }
}

/// 去掉 `host:port` 里的端口（IPv6 字面量与无端口输入原样返回）
pub fn strip_port(endpoint_key: &str) -> &str {
    match endpoint_key.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => endpoint_key,
    }
}

/// 是否为本地文件系统路径
///
/// 覆盖四种写法：`file:///data/lance`（已去掉 scheme 后为 `/data/lance`）、
/// POSIX 绝对路径 `/data/lance`、Windows 盘符 `D:\data\lance` / `D:/data/lance`、
/// UNC 共享 `\\nas\share\lance`。数据存储来源（LanceDB）用本地目录是**正常形态**，
/// 不能被判成跨网端点。
pub fn looks_like_local_path(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    if value
        .get(..5)
        .is_some_and(|head| head.eq_ignore_ascii_case("file:"))
    {
        return true;
    }
    if value.starts_with('/') || value.starts_with('\\') {
        return true;
    }
    let bytes = value.as_bytes();
    bytes.len() > 2
        && bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(1) == Some(&b':')
        && matches!(bytes.get(2), Some(b'\\' | b'/'))
}

/// 默认请求超时（秒）
pub const DEFAULT_TIMEOUT_SECS: u64 = 15;
/// 单次采集单类资源的上限，防止超大集群把内存打爆
pub const DEFAULT_MAX_ITEMS: usize = 20_000;

/// 一个采集连接的完整配置（对应 `ops_connection` 表的一行）
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    /// `ops_connection.id`，未落库时为 `None`
    pub id: Option<String>,
    /// 连接名称（界面展示、注册表主键兜底）
    pub name: String,
    /// 采集来源
    pub provider: Provider,
    /// 端点（带 scheme）：`local://`、`tcp://host:2375`、`https://host:2376`、
    /// `https://apiserver:6443`；kubeconfig 模式在连接成功后回填真实 server
    pub endpoint: String,
    /// 认证方式
    pub auth: AuthKind,
    /// 单次请求超时
    pub timeout: Duration,
    /// Docker API 版本（如 `1.43`），`None` 用 bollard 默认
    pub docker_api_version: Option<String>,
    /// 限定采集的命名空间（仅 K8s，`None` 表示全集群）
    pub namespace: Option<String>,
    /// 单类资源采集上限
    pub max_items: usize,
}

impl ConnectionConfig {
    /// 用端点构造，端点会按 provider / 认证方式自动补齐 scheme
    pub fn new(
        name: impl Into<String>,
        provider: Provider,
        endpoint: impl Into<String>,
        auth: AuthKind,
    ) -> Self {
        let endpoint = endpoint.into();
        let normalized = normalize_endpoint(provider, &endpoint, &auth);
        Self {
            id: None,
            name: name.into(),
            provider,
            endpoint: normalized,
            auth,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            docker_api_version: None,
            namespace: None,
            max_items: DEFAULT_MAX_ITEMS,
        }
    }

    /// 本机 Docker（Linux socket / Windows npipe）
    pub fn local_docker(name: impl Into<String>) -> Self {
        Self::new(name, Provider::Docker, "local://", AuthKind::LocalSocket)
    }

    /// 本机 Docker，启用 Swarm 采集（同一 socket，额外拉 Node/Service/Task）
    pub fn local_swarm(name: impl Into<String>) -> Self {
        Self::new(name, Provider::Swarm, "local://", AuthKind::LocalSocket)
    }

    /// 远程 Docker（明文 TCP，仅限隔离内网）
    pub fn docker_tcp(name: impl Into<String>, endpoint: &str) -> Self {
        Self::new(name, Provider::Docker, endpoint, AuthKind::PlainTcp)
    }

    /// 远程 Docker（mTLS）
    pub fn docker_tls(
        name: impl Into<String>,
        endpoint: &str,
        ca_path: impl Into<String>,
        cert_path: impl Into<String>,
        key_path: impl Into<String>,
    ) -> Self {
        Self::new(
            name,
            Provider::Docker,
            endpoint,
            AuthKind::MutualTls {
                ca_path: ca_path.into(),
                cert_path: cert_path.into(),
                key_path: key_path.into(),
            },
        )
    }

    /// 远程 Docker（mTLS，证书以 PEM 原文内联）
    pub fn docker_tls_inline(
        name: impl Into<String>,
        endpoint: &str,
        ca_pem: impl Into<String>,
        cert_pem: impl Into<String>,
        key_pem: impl Into<String>,
    ) -> Self {
        Self::new(
            name,
            Provider::Docker,
            endpoint,
            AuthKind::MutualTlsInline {
                ca_pem: ca_pem.into(),
                cert_pem: cert_pem.into(),
                key_pem: key_pem.into(),
            },
        )
    }

    /// 远程 Swarm Manager（mTLS，证书以 PEM 原文内联）
    pub fn swarm_tls_inline(
        name: impl Into<String>,
        endpoint: &str,
        ca_pem: impl Into<String>,
        cert_pem: impl Into<String>,
        key_pem: impl Into<String>,
    ) -> Self {
        Self::new(
            name,
            Provider::Swarm,
            endpoint,
            AuthKind::MutualTlsInline {
                ca_pem: ca_pem.into(),
                cert_pem: cert_pem.into(),
                key_pem: key_pem.into(),
            },
        )
    }

    /// 远程 Swarm Manager（明文 TCP）
    pub fn swarm_tcp(name: impl Into<String>, endpoint: &str) -> Self {
        Self::new(name, Provider::Swarm, endpoint, AuthKind::PlainTcp)
    }

    /// 远程 Swarm Manager（mTLS）
    pub fn swarm_tls(
        name: impl Into<String>,
        endpoint: &str,
        ca_path: impl Into<String>,
        cert_path: impl Into<String>,
        key_path: impl Into<String>,
    ) -> Self {
        Self::new(
            name,
            Provider::Swarm,
            endpoint,
            AuthKind::MutualTls {
                ca_path: ca_path.into(),
                cert_path: cert_path.into(),
                key_path: key_path.into(),
            },
        )
    }

    /// 读取本机 kubeconfig
    pub fn kubeconfig(
        name: impl Into<String>,
        path: impl Into<String>,
        context: Option<String>,
    ) -> Self {
        Self::new(
            name,
            Provider::Kubernetes,
            "",
            AuthKind::KubeconfigFile {
                path: path.into(),
                context,
            },
        )
    }

    /// 直连 APIServer（Bearer Token + 可选 CA PEM）
    pub fn kubernetes_token(
        name: impl Into<String>,
        endpoint: &str,
        token: impl Into<String>,
        ca_pem: Option<String>,
        insecure_skip_tls_verify: bool,
    ) -> Self {
        Self::new(
            name,
            Provider::Kubernetes,
            endpoint,
            AuthKind::Direct {
                token: Some(token.into()),
                ca_pem,
                client_cert_pem: None,
                client_key_pem: None,
                insecure_skip_tls_verify,
            },
        )
    }
}

impl ConnectionConfig {
    /// 连接唯一标识（落库前用连接名兜底）
    pub fn key(&self) -> &str {
        self.id.as_deref().unwrap_or(&self.name)
    }

    /// 去掉 scheme 与末尾 `/` 的端点，用于拼 `resource_uid`
    ///
    /// 本机 socket 归一化为 `local`，避免 `npipe:////./pipe/...` 里的斜杠破坏 uid 结构；
    /// 远程连接为空串时（kubeconfig 尚未解析出 server）返回空串，由 `validate` 兜底。
    ///
    /// **本地路径与对象存储**（LanceDB `file:///data/lance`、`s3://bucket/prefix`）里的 `/`
    /// 同样会破坏 `provider://endpoint/kind/name` 的段结构，统一改写成 `_`：
    /// `file:///data/lance` → `_data_lance`（稳定且可重复，保证 uid 幂等）。
    pub fn endpoint_key(&self) -> String {
        if self.auth.is_local_socket() {
            return "local".to_string();
        }
        let rest = self
            .endpoint
            .split_once("://")
            .map_or(self.endpoint.as_str(), |(_, rest)| rest);
        let rest = rest.trim_end_matches('/');
        if rest.contains('/') {
            rest.replace('/', "_")
        } else {
            rest.to_string()
        }
    }

    /// 连接层级（界面提示用）
    pub fn tier(&self) -> ConnectionTier {
        // 本地路径在 `endpoint_key()` 里已被改写，这里用原始端点判定
        if self.auth.is_local_path() || looks_like_local_path(&self.endpoint) {
            return ConnectionTier::Local;
        }
        ConnectionTier::from_endpoint(&self.endpoint_key())
    }

    /// 请求超时（秒，最小 1 秒；bollard 只接受秒）
    pub fn timeout_secs(&self) -> u64 {
        self.timeout.as_secs().max(1)
    }

    /// 设置连接 id
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// 设置超时
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout.max(Duration::from_secs(1));
        self
    }

    /// 设置超时（秒）
    #[must_use]
    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout = Duration::from_secs(secs.max(1));
        self
    }

    /// 指定 Docker API 版本
    #[must_use]
    pub fn with_docker_api_version(mut self, version: impl Into<String>) -> Self {
        self.docker_api_version = Some(version.into());
        self
    }

    /// 限定 K8s 命名空间
    #[must_use]
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// 设置单类资源上限
    #[must_use]
    pub fn with_max_items(mut self, max_items: usize) -> Self {
        self.max_items = max_items.max(1);
        self
    }
}

impl ConnectionConfig {
    /// 校验配置是否足以发起连接（不触网）
    pub fn validate(&self) -> OpsResult<()> {
        if self.name.trim().is_empty() {
            return Err(OpsError::Config("连接名称不能为空".to_string()));
        }
        match self.provider {
            Provider::Docker | Provider::Swarm => self.validate_docker()?,
            Provider::Kubernetes => self.validate_kubernetes()?,
            Provider::Redis | Provider::Valkey => self.validate_cache()?,
            Provider::PgVector => self.validate_postgres()?,
            Provider::LanceDb => self.validate_lancedb()?,
        }
        Ok(())
    }

    fn validate_docker(&self) -> OpsResult<()> {
        match &self.auth {
            AuthKind::LocalSocket | AuthKind::PlainTcp => {}
            AuthKind::MutualTls {
                ca_path,
                cert_path,
                key_path,
            } => {
                for (label, path) in [
                    ("CA 证书", ca_path.as_str()),
                    ("客户端证书", cert_path.as_str()),
                    ("客户端私钥", key_path.as_str()),
                ] {
                    if path.trim().is_empty() {
                        return Err(OpsError::Config(format!(
                            "{} 认证缺少{label}路径",
                            self.provider.label()
                        )));
                    }
                }
            }
            AuthKind::MutualTlsInline {
                ca_pem,
                cert_pem,
                key_pem,
            } => {
                for (label, pem) in [
                    ("CA 证书", ca_pem.as_str()),
                    ("客户端证书", cert_pem.as_str()),
                    ("客户端私钥", key_pem.as_str()),
                ] {
                    if pem.trim().is_empty() {
                        return Err(OpsError::Config(format!(
                            "{} 认证缺少{label} PEM",
                            self.provider.label()
                        )));
                    }
                }
            }
            other => {
                return Err(OpsError::Config(format!(
                    "{} 不支持 {} 认证方式",
                    self.provider.label(),
                    other.describe()
                )));
            }
        }
        if !self.auth.is_local_socket() && self.endpoint.trim().is_empty() {
            return Err(OpsError::Config(
                "远程 Docker 端点不能为空（形如 tcp://10.0.0.5:2375）".to_string(),
            ));
        }
        Ok(())
    }

    fn validate_kubernetes(&self) -> OpsResult<()> {
        match &self.auth {
            AuthKind::KubeconfigFile { path, .. } => {
                if path.trim().is_empty() {
                    return Err(OpsError::Config("kubeconfig 路径不能为空".to_string()));
                }
            }
            AuthKind::KubeconfigInline { yaml, .. } => {
                if yaml.trim().is_empty() {
                    return Err(OpsError::Config("kubeconfig 内容不能为空".to_string()));
                }
            }
            AuthKind::Direct {
                token,
                ca_pem,
                client_cert_pem,
                client_key_pem,
                insecure_skip_tls_verify,
            } => {
                let has_token = token.as_ref().is_some_and(|value| !value.trim().is_empty());
                let has_cert = client_cert_pem
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty())
                    && client_key_pem
                        .as_ref()
                        .is_some_and(|value| !value.trim().is_empty());
                if !has_token && !has_cert {
                    return Err(OpsError::Config(
                        "K8s 直连需要 Token 或客户端证书/私钥".to_string(),
                    ));
                }
                if !*insecure_skip_tls_verify
                    && ca_pem.as_ref().is_none_or(|ca| ca.trim().is_empty())
                {
                    return Err(OpsError::Config(
                        "K8s 直连未提供 CA 时必须显式允许跳过证书校验".to_string(),
                    ));
                }
                if self.endpoint.trim().is_empty() {
                    return Err(OpsError::Config(
                        "K8s 端点不能为空（形如 https://10.0.0.10:6443）".to_string(),
                    ));
                }
            }
            other => {
                return Err(OpsError::Config(format!(
                    "Kubernetes 不支持 {} 认证方式",
                    other.describe()
                )));
            }
        }
        Ok(())
    }

    /// 缓存（Redis / Valkey）配置校验
    ///
    /// 只校验「能否发起一次 AUTH + PING」，**不**校验账号是否只读：
    /// 只读性由 `vector-redis` 的命令白名单保证，探测结果里再给出 ACL 风险告警。
    fn validate_cache(&self) -> OpsResult<()> {
        match &self.auth {
            AuthKind::RedisAcl { username, password } => {
                if username.trim().is_empty() {
                    return Err(OpsError::Config(
                        "Redis ACL 用户名不能为空（未启用 ACL 的实例请填 default）".to_string(),
                    ));
                }
                if password.is_empty() {
                    return Err(OpsError::Config("Redis ACL 密码不能为空".to_string()));
                }
            }
            AuthKind::RedisTls {
                username, password, ..
            } => {
                let has_user = username
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty());
                let has_password = password.as_ref().is_some_and(|value| !value.is_empty());
                if has_user != has_password {
                    return Err(OpsError::Config(
                        "Redis TLS 的用户名与密码必须同时提供或同时留空".to_string(),
                    ));
                }
            }
            other => {
                return Err(OpsError::Config(format!(
                    "{} 不支持 {} 认证方式",
                    self.provider.label(),
                    other.describe()
                )));
            }
        }
        if self.endpoint.trim().is_empty() {
            return Err(OpsError::Config(
                "Redis / Valkey 端点不能为空（形如 redis://10.0.0.7:6379）".to_string(),
            ));
        }
        Ok(())
    }

    /// pgvector 配置校验
    ///
    /// 采集账号必须是**只读角色**（`SELECT` + `pg_catalog` 权限），
    /// 连接器只发 `SELECT`，且整个采集过程包在 `READ ONLY` 事务里（见 `vector-pgvector`）。
    fn validate_postgres(&self) -> OpsResult<()> {
        match &self.auth {
            AuthKind::PgPassword { username, password } => {
                if username.trim().is_empty() {
                    return Err(OpsError::Config("pgvector 采集账号不能为空".to_string()));
                }
                if password.is_empty() {
                    return Err(OpsError::Config(
                        "pgvector 采集账号密码不能为空".to_string(),
                    ));
                }
            }
            other => {
                return Err(OpsError::Config(format!(
                    "{} 不支持 {} 认证方式",
                    self.provider.label(),
                    other.describe()
                )));
            }
        }
        if self.endpoint.trim().is_empty() {
            return Err(OpsError::Config(
                "pgvector 端点不能为空（形如 postgresql://10.0.0.8:5432/assets）".to_string(),
            ));
        }
        Ok(())
    }

    /// LanceDB 配置校验（本地目录不需要凭证，对象存储需要 AK/SK）
    fn validate_lancedb(&self) -> OpsResult<()> {
        match &self.auth {
            AuthKind::LanceLocal { path } => {
                if path.trim().is_empty() && self.endpoint.trim().is_empty() {
                    return Err(OpsError::Config(
                        "LanceDB 本地目录不能为空（形如 file:///data/lance）".to_string(),
                    ));
                }
            }
            AuthKind::LanceCloud {
                access_key,
                secret_key,
                ..
            } => {
                if access_key.trim().is_empty() || secret_key.trim().is_empty() {
                    return Err(OpsError::Config(
                        "LanceDB 对象存储需要同时提供 Access Key 与 Secret Key".to_string(),
                    ));
                }
                if self.endpoint.trim().is_empty() {
                    return Err(OpsError::Config(
                        "LanceDB 对象存储端点不能为空（形如 s3://bucket/prefix）".to_string(),
                    ));
                }
            }
            other => {
                return Err(OpsError::Config(format!(
                    "{} 不支持 {} 认证方式",
                    self.provider.label(),
                    other.describe()
                )));
            }
        }
        Ok(())
    }

    /// 确认本次构建启用了该 provider 需要的采集实现
    ///
    /// Docker / Swarm / K8s 的实现就在本 crate 内，用 feature 开关控制；
    /// 数据存储（Redis / Valkey、pgvector、LanceDB）的驱动依赖分别归属
    /// `vector-redis`、`vector-pgvector`、`vector-lance`，本 crate 不做判断——
    /// 可用性由组合方（`service::ops_service::build_connector`）在其侧断言。
    pub fn ensure_feature(&self) -> OpsResult<()> {
        match self.provider {
            Provider::Docker | Provider::Swarm => {
                #[cfg(feature = "docker")]
                {
                    Ok(())
                }
                #[cfg(not(feature = "docker"))]
                {
                    Err(OpsError::Unsupported("docker"))
                }
            }
            Provider::Kubernetes => {
                #[cfg(feature = "k8s")]
                {
                    Ok(())
                }
                #[cfg(not(feature = "k8s"))]
                {
                    Err(OpsError::Unsupported("k8s"))
                }
            }
            Provider::Redis | Provider::Valkey | Provider::PgVector | Provider::LanceDb => Ok(()),
        }
    }
}

/// 端点归一化：缺 scheme 时按 provider / 认证方式补齐，去掉末尾 `/`
///
/// - 本机 Docker → `local://`
/// - Docker/Swarm 明文 → `tcp://host:2375`
/// - Docker/Swarm mTLS、K8s → `https://host:port`
/// - Redis / Valkey → `redis://host:6379`（TLS 认证补 `rediss://`）
/// - pgvector → `postgresql://host:5432/db`（`postgres://` 保留原样）
/// - LanceDB → `file:///path`（本地）/ `s3://bucket/prefix`（对象存储）
pub fn normalize_endpoint(provider: Provider, endpoint: &str, auth: &AuthKind) -> String {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        // 本机 socket 允许省略端点；远程连接留空交给 `validate` 报错
        return if auth.is_local_socket() {
            "local://".to_string()
        } else {
            String::new()
        };
    }
    if endpoint.contains("://") {
        return endpoint.trim_end_matches('/').to_string();
    }
    let scheme = match provider {
        Provider::Kubernetes => "https",
        Provider::Docker | Provider::Swarm => {
            if matches!(auth, AuthKind::MutualTls { .. } | AuthKind::MutualTlsInline { .. }) {
                "https"
            } else {
                "tcp"
            }
        }
        Provider::Redis | Provider::Valkey => {
            if matches!(auth, AuthKind::RedisTls { .. }) {
                "rediss"
            } else {
                "redis"
            }
        }
        Provider::PgVector => "postgresql",
        Provider::LanceDb => {
            if matches!(auth, AuthKind::LanceCloud { .. }) {
                "s3"
            } else {
                "file"
            }
        }
    };
    format!("{scheme}://{}", endpoint.trim_end_matches('/'))
}

/// 只读采集连接器
///
/// 实现者必须保证：`health` / `collect` 只发只读请求，且失败信息可归类
/// （返回 [`OpsError`]，由 [`OpsError::classify`] 给出 [`FailureClass`]）。
#[async_trait]
pub trait Connector: Send + Sync {
    /// 连接配置
    fn config(&self) -> &ConnectionConfig;

    /// 采集来源
    fn provider(&self) -> Provider {
        self.config().provider
    }

    /// 连通性 + 只读权限探测
    async fn health(&self) -> OpsResult<ConnectionHealth>;

    /// 全量采集（一次快照）
    async fn collect(&self) -> OpsResult<ResourceSnapshot>;

    /// 是否支持 watch 增量订阅（本轮全部 `false`）
    fn supports_watch(&self) -> bool {
        false
    }
}

/// 连接器注册表
///
/// 调度器按连接名（或 `ops_connection.id`）取连接器；注册表本身不持有运行时状态，
/// clone 成本只有 `Arc` 递增，便于多任务共享。
#[derive(Default, Clone)]
pub struct ConnectorRegistry {
    connectors: BTreeMap<String, Arc<dyn Connector>>,
}

impl fmt::Debug for ConnectorRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectorRegistry")
            .field("keys", &self.connectors.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ConnectorRegistry {
    /// 空注册表
    pub fn new() -> Self {
        Self::default()
    }

    /// 按当前 feature 注册本机默认连接器（本机 Docker socket）
    #[allow(unused_mut)]
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        #[cfg(feature = "docker")]
        match docker::DockerConnector::local_default() {
            Ok(connector) => {
                if let Err(err) = registry.register(Arc::new(connector)) {
                    tracing::warn!(error = %err, "注册本机 Docker 连接器失败");
                }
            }
            Err(err) => tracing::debug!(error = %err, "跳过本机 Docker 连接器"),
        }
        registry
    }

    /// 注册连接器（重名报错，避免静默覆盖生产连接）
    pub fn register(&mut self, connector: Arc<dyn Connector>) -> OpsResult<()> {
        let key = connector.config().key().to_string();
        if self.connectors.contains_key(&key) {
            return Err(OpsError::Config(format!("连接 {key} 已注册")));
        }
        tracing::debug!(connection = %key, provider = %connector.provider(), "注册运维连接器");
        self.connectors.insert(key, connector);
        Ok(())
    }

    /// 移除连接器
    pub fn remove(&mut self, key: &str) -> Option<Arc<dyn Connector>> {
        self.connectors.remove(key)
    }

    /// 按 key 取连接器
    pub fn get(&self, key: &str) -> Option<Arc<dyn Connector>> {
        self.connectors.get(key).cloned()
    }

    /// 已注册连接数
    pub fn len(&self) -> usize {
        self.connectors.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.connectors.is_empty()
    }

    /// 全部 key（按字典序）
    pub fn keys(&self) -> Vec<&str> {
        self.connectors.keys().map(String::as_str).collect()
    }

    /// 全部连接及其来源
    pub fn connections(&self) -> Vec<(String, Provider)> {
        self.connectors
            .iter()
            .map(|(key, connector)| (key.clone(), connector.provider()))
            .collect()
    }

    /// 逐个探测（顺序执行，避免同时打爆远端与触发风控）
    pub async fn health_all(&self) -> Vec<ConnectionHealth> {
        let mut results = Vec::with_capacity(self.connectors.len());
        for (key, connector) in &self.connectors {
            let mut health = match connector.health().await {
                Ok(health) => health,
                Err(err) => {
                    ConnectionHealth::new(connector.provider(), connector.config().endpoint.clone())
                        .with_failure(&err, 0)
                }
            };
            if health.endpoint.is_empty() {
                health.endpoint = connector.config().endpoint.clone();
            }
            tracing::debug!(connection = %key, ok = health.ok, failure = health.failure.code(), "连接探测完成");
            results.push(health);
        }
        results
    }

    /// 逐个采集（顺序执行；单个失败不影响其它连接）
    pub async fn collect_all(&self) -> Vec<(String, OpsResult<ResourceSnapshot>)> {
        let mut results = Vec::with_capacity(self.connectors.len());
        for (key, connector) in &self.connectors {
            results.push((key.clone(), connector.collect().await));
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_scheme_is_inferred() {
        assert_eq!(
            normalize_endpoint(Provider::Docker, "10.0.0.5:2375", &AuthKind::PlainTcp),
            "tcp://10.0.0.5:2375"
        );
        assert_eq!(
            normalize_endpoint(
                Provider::Swarm,
                "10.0.0.6:2376",
                &AuthKind::MutualTls {
                    ca_path: "a".into(),
                    cert_path: "b".into(),
                    key_path: "c".into(),
                }
            ),
            "https://10.0.0.6:2376"
        );
        assert_eq!(
            normalize_endpoint(
                Provider::Kubernetes,
                "10.0.0.10:6443",
                &AuthKind::Direct {
                    token: Some("t".into()),
                    ca_pem: None,
                    client_cert_pem: None,
                    client_key_pem: None,
                    insecure_skip_tls_verify: true,
                }
            ),
            "https://10.0.0.10:6443"
        );
        // 已带 scheme 只去掉末尾斜杠
        assert_eq!(
            normalize_endpoint(
                Provider::Docker,
                "unix:///var/run/docker.sock/",
                &AuthKind::LocalSocket
            ),
            "unix:///var/run/docker.sock"
        );
        // 空端点：本机 socket 归一化为 local://，远程连接保留空串交给 validate 报错
        assert_eq!(
            normalize_endpoint(
                Provider::Kubernetes,
                " ",
                &AuthKind::KubeconfigFile {
                    path: "/root/.kube/config".into(),
                    context: None,
                }
            ),
            ""
        );
        assert_eq!(
            normalize_endpoint(Provider::Docker, "", &AuthKind::PlainTcp),
            ""
        );
        assert_eq!(
            normalize_endpoint(Provider::Docker, "", &AuthKind::LocalSocket),
            "local://"
        );
    }

    #[test]
    fn endpoint_key_and_tier() {
        let local = ConnectionConfig::local_docker("本机");
        assert_eq!(local.endpoint_key(), "local");
        assert_eq!(local.tier(), ConnectionTier::Local);

        let tcp = ConnectionConfig::docker_tcp("机房 A", "tcp://10.0.0.5:2375/");
        assert_eq!(tcp.endpoint_key(), "10.0.0.5:2375");
        assert_eq!(tcp.tier(), ConnectionTier::Intranet);

        let remote = ConnectionConfig::docker_tcp("云上", "https://docker.example.com:2376");
        assert_eq!(remote.endpoint_key(), "docker.example.com:2376");
        assert_eq!(remote.tier(), ConnectionTier::Remote);

        assert_eq!(
            ConnectionTier::from_endpoint("192.168.1.20:6443"),
            ConnectionTier::Intranet
        );
        assert_eq!(
            ConnectionTier::from_endpoint("172.20.0.9:6443"),
            ConnectionTier::Intranet
        );
        assert_eq!(
            ConnectionTier::from_endpoint("172.40.0.9:6443"),
            ConnectionTier::Remote
        );
        assert_eq!(
            ConnectionTier::from_endpoint("127.0.0.1"),
            ConnectionTier::Local
        );
        assert_eq!(
            ConnectionTier::from_endpoint("local"),
            ConnectionTier::Local
        );
        assert_eq!(ConnectionTier::Local.as_str(), "local");
    }

    #[test]
    fn timeout_is_clamped_to_at_least_one_second() {
        let config = ConnectionConfig::local_docker("本机").with_timeout(Duration::ZERO);
        assert_eq!(config.timeout_secs(), 1);
        assert_eq!(config.timeout, Duration::from_secs(1));
        assert_eq!(
            ConnectionConfig::local_docker("本机")
                .with_timeout_secs(0)
                .timeout_secs(),
            1
        );
        assert_eq!(
            ConnectionConfig::local_docker("本机").timeout_secs(),
            DEFAULT_TIMEOUT_SECS
        );
    }

    #[test]
    fn validation_rejects_incomplete_configs() {
        // 远程 Docker 必须有端点
        let empty_endpoint =
            ConnectionConfig::new("机房", Provider::Docker, "", AuthKind::PlainTcp);
        assert!(empty_endpoint.validate().is_err());

        // mTLS 必须给全三个路径
        let missing_cert =
            ConnectionConfig::docker_tls("机房", "10.0.0.5:2376", "/ca.pem", "", "/key.pem");
        let err = missing_cert.validate().expect_err("缺少证书路径应报错");
        assert_eq!(err.classify(), FailureClass::Config);

        // K8s 直连没有 CA 又没允许跳过校验
        let no_ca = ConnectionConfig::new(
            "集群",
            Provider::Kubernetes,
            "10.0.0.10:6443",
            AuthKind::Direct {
                token: Some("token".into()),
                ca_pem: None,
                client_cert_pem: None,
                client_key_pem: None,
                insecure_skip_tls_verify: false,
            },
        );
        assert!(no_ca.validate().is_err());

        // K8s 直连缺凭证
        let no_credential = ConnectionConfig::new(
            "集群",
            Provider::Kubernetes,
            "10.0.0.10:6443",
            AuthKind::Direct {
                token: None,
                ca_pem: Some("ca".into()),
                client_cert_pem: None,
                client_key_pem: None,
                insecure_skip_tls_verify: false,
            },
        );
        assert!(no_credential.validate().is_err());

        // Docker 不接受 kubeconfig 认证
        let wrong_auth = ConnectionConfig::new(
            "错配",
            Provider::Docker,
            "10.0.0.5:2375",
            AuthKind::KubeconfigFile {
                path: "/root/.kube/config".into(),
                context: None,
            },
        );
        assert!(wrong_auth.validate().is_err());

        // 合法配置
        assert!(ConnectionConfig::local_docker("本机").validate().is_ok());
        assert!(
            ConnectionConfig::docker_tcp("机房", "10.0.0.5:2375")
                .validate()
                .is_ok()
        );
        assert!(
            ConnectionConfig::kubeconfig("本机集群", "C:/Users/ops/.kube/config", None)
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn debug_output_masks_secrets() {
        let auth = AuthKind::Direct {
            token: Some("super-secret-token".into()),
            ca_pem: Some("-----BEGIN CERTIFICATE-----".into()),
            client_cert_pem: Some("cert".into()),
            client_key_pem: Some("key".into()),
            insecure_skip_tls_verify: true,
        };
        let rendered = format!("{auth:?}");
        assert!(!rendered.contains("super-secret-token"));
        assert!(!rendered.contains("BEGIN CERTIFICATE"));
        assert!(rendered.contains("<已隐藏>"));
        assert_eq!(auth.describe(), "direct");

        let config = ConnectionConfig::new("集群", Provider::Kubernetes, "h:6443", auth);
        let rendered = format!("{config:?}");
        assert!(
            !rendered.contains("super-secret-token"),
            "连接配置不应泄露凭证"
        );
    }

    #[test]
    fn health_summary_is_human_readable() {
        let mut health = ConnectionHealth::new(Provider::Docker, "tcp://10.0.0.5:2375");
        health.ok = true;
        health.server_version = Some("27.3.1".into());
        health.latency_ms = 18;
        let ok_summary = health.summary();
        assert!(ok_summary.contains("连通正常"));
        assert!(ok_summary.contains("27.3.1"));

        let failed = ConnectionHealth::new(Provider::Docker, "tcp://10.0.0.9:2375").with_failure(
            &OpsError::Connect("tcp connect error: 由于目标计算机积极拒绝".into()),
            5,
        );
        assert_eq!(failed.failure, FailureClass::Network);
        assert!(failed.summary().contains("网络不可达"));
        assert!(failed.error.is_some());
        assert_eq!(failed.latency_ms, 5);
    }

    /// 注册表测试用的最小假实现（不触网、不依赖任何 feature）
    struct Dummy {
        config: ConnectionConfig,
        fail: bool,
    }

    #[async_trait]
    impl Connector for Dummy {
        fn config(&self) -> &ConnectionConfig {
            &self.config
        }

        async fn health(&self) -> OpsResult<ConnectionHealth> {
            if self.fail {
                return Err(OpsError::Connect("模拟连接失败".into()));
            }
            let mut health = ConnectionHealth::new(self.provider(), self.config.endpoint.clone());
            health.ok = true;
            health.server_version = Some("27.0.0".into());
            Ok(health)
        }

        async fn collect(&self) -> OpsResult<ResourceSnapshot> {
            let mut snapshot =
                ResourceSnapshot::new(self.provider(), self.config.endpoint.clone(), 3);
            if self.fail {
                snapshot.push_error("模拟部分失败");
            }
            Ok(snapshot)
        }
    }

    fn dummy(name: &str, fail: bool) -> Arc<Dummy> {
        Arc::new(Dummy {
            config: ConnectionConfig::local_docker(name),
            fail,
        })
    }

    #[test]
    fn registry_rejects_duplicate_names() {
        let mut registry = ConnectorRegistry::new();
        assert!(registry.is_empty());
        registry
            .register(dummy("本机", false))
            .expect("首次注册应成功");
        assert!(
            registry.register(dummy("本机", false)).is_err(),
            "重名注册必须报错"
        );
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.keys(), vec!["本机"]);
        assert!(registry.get("本机").is_some());
        assert!(registry.get("不存在").is_none());
        assert_eq!(
            registry.connections(),
            vec![("本机".to_string(), Provider::Docker)]
        );
        assert!(!format!("{registry:?}").is_empty());
        assert!(registry.remove("本机").is_some());
        assert!(registry.is_empty());
    }

    #[tokio::test]
    async fn registry_probes_and_collects_sequentially() {
        let mut registry = ConnectorRegistry::new();
        registry
            .register(dummy("正常连接", false))
            .expect("注册失败");
        registry
            .register(dummy("故障连接", true))
            .expect("注册失败");

        let health = registry.health_all().await;
        assert_eq!(health.len(), 2, "顺序探测应覆盖全部连接");
        let ok = health.iter().filter(|item| item.ok).count();
        assert_eq!(ok, 1, "故障连接不应被标记为正常");
        let failed = health.iter().find(|item| !item.ok).expect("应有失败项");
        assert_eq!(failed.failure, FailureClass::Network);

        let collected = registry.collect_all().await;
        assert_eq!(collected.len(), 2);
        assert!(collected.iter().all(|(_, snapshot)| snapshot.is_ok()));
        let keys: Vec<&str> = collected.iter().map(|(key, _)| key.as_str()).collect();
        // BTreeMap 按 Unicode 码点排序：故（U+6545）< 正（U+6B63）
        assert_eq!(
            keys,
            vec!["故障连接", "正常连接"],
            "结果应按连接名字典序返回"
        );
    }
}

///
/// 探测失败不返回 `Err`：调用方需要看到「失败到哪一步」（DNS / TCP / TLS / 401 / 403），
/// 因此失败信息也放进结构体，并且带上界面可直接展示的 [`FailureClass`]。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionHealth {
    /// 采集来源
    pub provider: Provider,
    /// 端点（带 scheme 原样）
    pub endpoint: String,
    /// 是否连通且具备只读权限
    pub ok: bool,
    /// 探测耗时（毫秒）
    pub latency_ms: u64,
    /// 服务端版本（Docker `Version` / K8s `gitVersion`）
    pub server_version: Option<String>,
    /// 服务端 API 版本（Docker `ApiVersion` / K8s `major.minor`）
    pub api_version: Option<String>,
    /// 认证是否通过
    pub authenticated: bool,
    /// 只读权限是否足够
    pub authorized: bool,
    /// 非致命问题（例如 Swarm 未启用、个别命名空间无权限）
    pub warnings: Vec<String>,
    /// 失败原文
    pub error: Option<String>,
    /// 故障归类
    pub failure: FailureClass,
}

impl ConnectionHealth {
    /// 新建「尚未探测」的结果
    pub fn new(provider: Provider, endpoint: impl Into<String>) -> Self {
        Self {
            provider,
            endpoint: endpoint.into(),
            ok: false,
            latency_ms: 0,
            server_version: None,
            api_version: None,
            authenticated: false,
            authorized: false,
            warnings: Vec::new(),
            error: None,
            failure: FailureClass::None,
        }
    }

    /// 记录一次失败（保留已探到的版本信息）
    #[must_use]
    pub fn with_failure(mut self, error: &OpsError, latency_ms: u64) -> Self {
        self.latency_ms = latency_ms;
        self.failure = error.classify();
        self.error = Some(error.to_string());
        self
    }

    /// 追加非致命告警
    pub fn warn(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }

    /// 一行中文结论（探针 / 界面 toast 直接用）
    pub fn summary(&self) -> String {
        if self.ok {
            let version = self.server_version.as_deref().unwrap_or("未知版本");
            format!(
                "{} {} 连通正常（{}，{} ms）",
                self.provider.label(),
                self.endpoint,
                version,
                self.latency_ms
            )
        } else {
            format!(
                "{} {} 探测失败：{}（{}）",
                self.provider.label(),
                self.endpoint,
                self.failure.label(),
                self.failure.hint()
            )
        }
    }
}

#[cfg(test)]
mod pem_inline_tests {
    use super::*;

    fn sample_pem(label: &str) -> String {
        format!("-----BEGIN {label}-----\nMIIB\n-----END {label}-----\n")
    }

    #[test]
    fn inline_mtls_endpoint_uses_https() {
        let config = ConnectionConfig::docker_tls_inline(
            "远程 Docker",
            "10.0.0.5:2376",
            sample_pem("CA"),
            sample_pem("CERT"),
            sample_pem("KEY"),
        );
        assert_eq!(config.endpoint, "https://10.0.0.5:2376");
        assert!(config.validate().is_ok(), "内联 mTLS 配置应当校验通过");
        assert_eq!(config.auth.describe(), "mutual_tls_inline");
    }

    #[test]
    fn inline_mtls_requires_all_pem() {
        for (ca, cert, key, label) in [
            (String::new(), sample_pem("CERT"), sample_pem("KEY"), "CA 证书"),
            (sample_pem("CA"), String::new(), sample_pem("KEY"), "客户端证书"),
            (sample_pem("CA"), sample_pem("CERT"), String::new(), "客户端私钥"),
        ] {
            let config =
                ConnectionConfig::docker_tls_inline("远程 Docker", "10.0.0.5:2376", ca, cert, key);
            let err = config.validate().expect_err("空 PEM 必须被拒绝");
            assert!(
                err.to_string().contains(label),
                "错误信息应包含缺失项 {label}: {err}"
            );
        }
    }

    #[test]
    fn inline_mtls_debug_is_masked() {
        let auth = AuthKind::MutualTlsInline {
            ca_pem: sample_pem("CA"),
            cert_pem: sample_pem("CERT"),
            key_pem: sample_pem("KEY"),
        };
        let debug = format!("{auth:?}");
        assert!(!debug.contains("BEGIN"), "PEM 不得出现在 Debug 输出: {debug}");
        assert!(debug.contains("<已隐藏>"), "{debug}");
    }
}

#[cfg(test)]
mod datastore_tests {
    use super::*;
    use crate::model::{ResourceKey, ResourceKind};

    fn cache_config(auth: AuthKind) -> ConnectionConfig {
        ConnectionConfig::new("缓存", Provider::Redis, "10.0.0.7:6379", auth)
    }

    #[test]
    fn cache_endpoint_and_auth_scheme() {
        let plain = cache_config(AuthKind::RedisAcl {
            username: "ops_ro".into(),
            password: "secret".into(),
        });
        assert_eq!(plain.endpoint, "redis://10.0.0.7:6379");
        assert_eq!(plain.auth.describe(), "redis_acl");
        assert!(plain.auth.requires_credential());
        assert_eq!(plain.tier(), ConnectionTier::Intranet);
        assert!(plain.validate().is_ok());

        let tls = cache_config(AuthKind::RedisTls {
            username: None,
            password: None,
            ca_pem: None,
            insecure_skip_tls_verify: false,
        });
        assert_eq!(tls.endpoint, "rediss://10.0.0.7:6379");
        assert_eq!(tls.auth.describe(), "redis_tls");
        assert!(tls.validate().is_ok());
    }

    #[test]
    fn cache_validation_requires_credentials() {
        let no_user = cache_config(AuthKind::RedisAcl {
            username: "  ".into(),
            password: "p".into(),
        });
        assert!(no_user.validate().is_err());

        let no_password = cache_config(AuthKind::RedisAcl {
            username: "default".into(),
            password: String::new(),
        });
        assert!(no_password.validate().is_err());

        // 用户名与密码必须成对出现
        let half = cache_config(AuthKind::RedisTls {
            username: Some("default".into()),
            password: None,
            ca_pem: None,
            insecure_skip_tls_verify: false,
        });
        assert!(half.validate().is_err());

        // 端点不能为空
        let empty = ConnectionConfig::new(
            "缓存",
            Provider::Valkey,
            "",
            AuthKind::RedisAcl {
                username: "default".into(),
                password: "p".into(),
            },
        );
        assert!(empty.validate().is_err());
    }

    #[test]
    fn postgres_endpoint_and_validation() {
        let config = ConnectionConfig::new(
            "pgvector",
            Provider::PgVector,
            "10.0.0.8:5432",
            AuthKind::PgPassword {
                username: "ops_ro".into(),
                password: "p".into(),
            },
        );
        assert_eq!(config.endpoint, "postgresql://10.0.0.8:5432");
        assert_eq!(config.endpoint_key(), "10.0.0.8:5432");
        assert_eq!(config.tier(), ConnectionTier::Intranet);
        assert!(config.validate().is_ok());

        let no_password = ConnectionConfig::new(
            "pgvector",
            Provider::PgVector,
            "10.0.0.8:5432",
            AuthKind::PgPassword {
                username: "ops_ro".into(),
                password: String::new(),
            },
        );
        assert!(no_password.validate().is_err());
    }

    #[test]
    fn local_lance_path_is_local_tier() {
        // 曾经的缺陷：file:///data/lance 被判成 Remote，界面误报「端点为公网地址」
        let config = ConnectionConfig::new(
            "向量副本",
            Provider::LanceDb,
            "file:///data/lance",
            AuthKind::LanceLocal {
                path: "/data/lance".into(),
            },
        );
        assert_eq!(config.endpoint, "file:///data/lance");
        assert_eq!(config.tier(), ConnectionTier::Local);
        assert_eq!(config.endpoint_key(), "_data_lance");
        assert!(config.validate().is_ok());

        // uid 段结构不被路径斜杠破坏，且能反解
        let key = ResourceKey::new(
            Provider::LanceDb,
            config.endpoint_key(),
            ResourceKind::VectorTable,
            "chunks",
        );
        assert_eq!(key.uid(), "lancedb://_data_lance/vector_table/chunks");
        assert!(ResourceKey::parse(&key.uid()).is_ok());

        assert_eq!(
            ConnectionTier::from_endpoint("D:\\data\\lance"),
            ConnectionTier::Local
        );
        assert_eq!(
            ConnectionTier::from_endpoint("/data/lance"),
            ConnectionTier::Local
        );
        assert_eq!(
            ConnectionTier::from_endpoint("file:///data/lance"),
            ConnectionTier::Local
        );
        assert!(!looks_like_local_path("10.0.0.7:6379"));
    }

    #[test]
    fn lance_cloud_requires_key_pair() {
        let cloud = ConnectionConfig::new(
            "向量副本",
            Provider::LanceDb,
            "s3://vector-bucket/lance",
            AuthKind::LanceCloud {
                access_key: "AKIAEXAMPLE".to_string(),
                secret_key: "sk-secret".to_string(),
                region: Some("cn-north-1".to_string()),
                endpoint: None,
            },
        );
        assert_eq!(cloud.endpoint, "s3://vector-bucket/lance");
        assert_eq!(cloud.auth.describe(), "lance_cloud");
        assert_eq!(cloud.tier(), ConnectionTier::Remote);
        assert!(cloud.validate().is_ok());

        // 只给 AK、SK 为空白：必须报错（否则运行期才会在 SDK 里炸）
        let half = ConnectionConfig::new(
            "向量副本",
            Provider::LanceDb,
            "s3://vector-bucket/lance",
            AuthKind::LanceCloud {
                access_key: "AKIAEXAMPLE".to_string(),
                secret_key: "   ".to_string(),
                region: None,
                endpoint: None,
            },
        );
        assert!(half.validate().is_err());

        // 本地目录既没路径也没端点：必须报错
        let empty_local = ConnectionConfig::new(
            "向量副本",
            Provider::LanceDb,
            "",
            AuthKind::LanceLocal {
                path: String::new(),
            },
        );
        assert!(empty_local.validate().is_err());
    }

    #[test]
    fn datastore_debug_masks_secrets() {
        let cache = cache_config(AuthKind::RedisAcl {
            username: "ops_ro".to_string(),
            password: "cache-secret".to_string(),
        });
        let rendered = format!("{cache:?}");
        assert!(!rendered.contains("cache-secret"), "{rendered}");
        assert!(rendered.contains("ops_ro"), "{rendered}");

        let postgres = ConnectionConfig::new(
            "pgvector",
            Provider::PgVector,
            "10.0.0.8:5432",
            AuthKind::PgPassword {
                username: "ops_ro".to_string(),
                password: "pg-secret".to_string(),
            },
        );
        let rendered = format!("{postgres:?}");
        assert!(!rendered.contains("pg-secret"), "{rendered}");

        let lance = ConnectionConfig::new(
            "向量副本",
            Provider::LanceDb,
            "s3://vector-bucket/lance",
            AuthKind::LanceCloud {
                access_key: "AKIAEXAMPLE".to_string(),
                secret_key: "sk-secret".to_string(),
                region: None,
                endpoint: Some("http://minio:9000".to_string()),
            },
        );
        let rendered = format!("{lance:?}");
        assert!(!rendered.contains("sk-secret"), "{rendered}");
        assert!(!rendered.contains("AKIAEXAMPLE"), "{rendered}");
    }
}


