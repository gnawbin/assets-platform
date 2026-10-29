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
}

impl AuthKind {
    /// 稳定编码（落 `ops_connection.auth_type`、探针输出）
    pub const fn describe(&self) -> &'static str {
        match self {
            Self::LocalSocket => "local_socket",
            Self::PlainTcp => "plain_tcp",
            Self::MutualTls { .. } => "mutual_tls",
            Self::KubeconfigFile { .. } => "kubeconfig_file",
            Self::KubeconfigInline { .. } => "kubeconfig_inline",
            Self::Direct { .. } => "direct",
        }
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
        let host = strip_port(endpoint_key)
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
    pub fn endpoint_key(&self) -> String {
        if self.auth.is_local_socket() {
            return "local".to_string();
        }
        let rest = self
            .endpoint
            .split_once("://")
            .map_or(self.endpoint.as_str(), |(_, rest)| rest);
        rest.trim_end_matches('/').to_string()
    }

    /// 连接层级（界面提示用）
    pub fn tier(&self) -> ConnectionTier {
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

    /// 确认本次构建启用了该 provider 需要的 feature
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
        }
    }
}

/// 端点归一化：缺 scheme 时按 provider / 认证方式补齐，去掉末尾 `/`
///
/// - 本机 Docker → `local://`
/// - Docker/Swarm 明文 → `tcp://host:2375`
/// - Docker/Swarm mTLS、K8s → `https://host:port`
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
            if matches!(auth, AuthKind::MutualTls { .. }) {
                "https"
            } else {
                "tcp"
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
