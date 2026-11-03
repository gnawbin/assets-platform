//! 运维采集（Docker / Swarm / Kubernetes）连接配置 Service
//!
//! 本模块是《IT资产与运维排查一体化系统落地方案》§3 采集发现引擎的**接口层**：
//! - 连接配置 CRUD（`ops_connection`，凭证以 AES-256-GCM 密文落库）
//! - 连通性探测（草稿 / 已存连接）
//! - 手动采集，并把观测结果 upsert 进 `asset_ops_link`（**不写** `assets` 台账）
//!
//! 设计约束：
//! - `ops` crate 只负责采集，不认识 HTTP / Tauri；本层是唯一业务入口；
//! - 凭证只进不出：`OpsConnectionView` 只暴露 `auth_type` / `has_credential` / `credential_hint`；
//! - HTTP 与 Tauri 两层只做参数解析，不允许绕过本层直接连库。

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use database;
use database::models::OpsConnection;
use ops::connector::docker::DockerConnector;
use ops::connector::k8s::K8sConnector;
use ops::connector::{AuthKind, ConnectionConfig, ConnectionTier, Connector};
use ops::model::{Provider, ResourceSnapshot};
use serde::{Deserialize, Serialize};
use tracing::{error, info};
use utils::crypto::{decrypt_api_key, encrypt_api_key};
use utils::snowflake::next_id;

// ======================== 入参 DTO ========================

/// 认证方式载荷（前端 ↔ service）
///
/// `tag = "type"` 与 `AuthKind::describe()` 的稳定编码一致，前端据此做表单联动。
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpsAuthPayload {
    /// 本机 socket / Windows named pipe
    LocalSocket,
    /// 远程明文 TCP（2375）
    PlainTcp,
    /// 远程 mTLS：证书文件路径（后端主机上必须已存在）
    MutualTls {
        ca_path: String,
        cert_path: String,
        key_path: String,
    },
    /// 远程 mTLS：证书 PEM 原文（推荐，界面录入）
    MutualTlsInline {
        ca_pem: String,
        cert_pem: String,
        key_pem: String,
    },
    /// kubeconfig 文件路径
    KubeconfigFile {
        path: String,
        #[serde(default)]
        context: Option<String>,
    },
    /// kubeconfig YAML 原文
    KubeconfigInline {
        yaml: String,
        #[serde(default)]
        context: Option<String>,
    },
    /// 直连 APIServer
    Direct {
        #[serde(default)]
        token: Option<String>,
        #[serde(default)]
        ca_pem: Option<String>,
        #[serde(default)]
        client_cert_pem: Option<String>,
        #[serde(default)]
        client_key_pem: Option<String>,
        #[serde(default)]
        insecure_skip_tls_verify: bool,
    },
}

impl fmt::Debug for OpsAuthPayload {
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
                insecure_skip_tls_verify,
                ..
            } => f
                .debug_struct("Direct")
                .field("token", &MASK)
                .field("ca_pem", &MASK)
                .field("client_cert_pem", &MASK)
                .field("client_key_pem", &MASK)
                .field("insecure_skip_tls_verify", insecure_skip_tls_verify)
                .finish(),
        }
    }
}

/// 新建 / 更新连接的入参
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpsConnectionInput {
    pub name: String,
    pub provider: String,
    pub endpoint: String,
    /// 更新时可传 `null` 表示「沿用已保存的凭证与认证方式」
    #[serde(default)]
    pub auth: Option<OpsAuthPayload>,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u32>,
    #[serde(default)]
    pub docker_api_version: Option<String>,
    #[serde(default)]
    pub max_items: Option<usize>,
    #[serde(default)]
    pub poll_interval_secs: Option<u32>,
    #[serde(default)]
    pub enabled: Option<bool>,
}
// ======================== 出参 DTO ========================

/// 连接视图（**不含任何凭证明文**）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpsConnectionView {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub provider_label: String,
    pub endpoint: String,
    pub auth_type: String,
    pub has_credential: bool,
    pub credential_hint: Option<String>,
    pub namespace: Option<String>,
    pub timeout_secs: u32,
    pub docker_api_version: Option<String>,
    pub max_items: usize,
    pub poll_interval_secs: u32,
    pub enabled: bool,
    /// 连接层级：local / intranet / remote（仅用于界面风险提示）
    pub tier: String,
    /// 风险提示文案（明文 TCP、跳过 TLS 校验等）
    pub risk_hints: Vec<String>,
    pub last_collect_at: Option<String>,
    pub last_error: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// 连通性探测结果（探测失败也返回本结构，`ok=false`）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpsHealthView {
    pub ok: bool,
    pub provider: String,
    pub provider_label: String,
    pub endpoint: String,
    pub tier: String,
    pub latency_ms: u64,
    pub server_version: Option<String>,
    pub api_version: Option<String>,
    pub authenticated: bool,
    pub authorized: bool,
    pub warnings: Vec<String>,
    pub error: Option<String>,
    /// 故障归类编码：none / network / tls / auth / forbidden / timeout / config / parse / api ...
    pub failure_class: String,
    /// 归类对应的处理建议
    pub failure_hint: String,
    /// 一行中文结论
    pub summary: String,
}

/// 单类资源的采集数量
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpsKindCount {
    pub kind: String,
    pub count: usize,
}

/// 一次手动采集的结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpsCollectResult {
    pub connection_id: String,
    pub provider: String,
    pub endpoint: String,
    /// 采集完成时间（epoch 毫秒，字符串规避前端精度丢失）
    pub collected_at: String,
    pub elapsed_ms: u64,
    pub total: usize,
    pub short_lived: usize,
    /// 本次新增的桥表行数
    pub created: usize,
    /// 本次刷新的桥表行数
    pub updated: usize,
    /// 完整采集时被标记为 gone 的行数（采集不完整时恒为 0）
    pub gone: usize,
    pub counts_by_kind: Vec<OpsKindCount>,
    pub partial_errors: Vec<String>,
}

/// 资源清单视图（LEFT JOIN 台账取 asset_no）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpsResourceLinkView {
    pub id: String,
    pub connection_id: String,
    pub connection_name: Option<String>,
    pub provider: String,
    pub resource_uid: String,
    pub resource_kind: String,
    pub display_name: Option<String>,
    pub asset_id: Option<String>,
    pub asset_no: Option<String>,
    pub status_snapshot: Option<String>,
    pub raw_state: Option<String>,
    pub parent_uid: Option<String>,
    pub attributes: Option<serde_json::Value>,
    pub first_seen_at: Option<String>,
    pub last_seen_at: Option<String>,
}

/// 来源下拉项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpsProviderOption {
    pub value: String,
    pub label: String,
    pub auth_types: Vec<OpsAuthTypeOption>,
}

/// 认证方式下拉项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpsAuthTypeOption {
    pub value: String,
    pub label: String,
    pub requires_credential: bool,
    pub hint: Option<String>,
}

// ======================== 静态元数据 ========================

fn auth_type_option(
    value: &str,
    label: &str,
    requires_credential: bool,
    hint: Option<&str>,
) -> OpsAuthTypeOption {
    OpsAuthTypeOption {
        value: value.to_string(),
        label: label.to_string(),
        requires_credential,
        hint: hint.map(str::to_string),
    }
}

/// 来源 / 认证方式下拉数据（前端表单联动的唯一真源）
pub fn provider_options() -> Vec<OpsProviderOption> {
    let docker_auth = vec![
        auth_type_option("local_socket", "本机 socket / named pipe", false, Some("仅采集后端所在主机")),
        auth_type_option("plain_tcp", "远程明文 TCP（2375）", false, Some("未加密，仅限隔离内网")),
        auth_type_option("mutual_tls", "远程 mTLS（证书文件路径）", true, Some("需后端主机上已有 CA / 证书 / 私钥文件")),
        auth_type_option("mutual_tls_inline", "远程 mTLS（证书 PEM 内联）", true, Some("证书原文加密落库，适合界面录入")),
    ];
    let k8s_auth = vec![
        auth_type_option("kubeconfig_file", "kubeconfig 文件（后端主机）", false, Some("读取后端主机上的 kubeconfig")),
        auth_type_option("kubeconfig_inline", "kubeconfig 内容（上传）", true, Some("YAML 原文加密落库，适合多集群分发")),
        auth_type_option("direct", "直连 APIServer（Token / 客户端证书）", true, Some("最小权限 SA Token 即可")),
    ];
    vec![
        OpsProviderOption {
            value: "docker".to_string(),
            label: Provider::Docker.label().to_string(),
            auth_types: docker_auth.clone(),
        },
        OpsProviderOption {
            value: "swarm".to_string(),
            label: Provider::Swarm.label().to_string(),
            auth_types: docker_auth,
        },
        OpsProviderOption {
            value: "k8s".to_string(),
            label: Provider::Kubernetes.label().to_string(),
            auth_types: k8s_auth,
        },
    ]
}

/// 某来源允许的认证方式（与 ops crate 的 validate 规则一致）
///
/// 只声明本模块负责的容器 / 编排来源；数据资产来源（缓存 / 向量库）的
/// 认证方式由其所属模块负责，见 `Provider::is_data_store`，本模块不声明。
fn allowed_auth_types(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Docker | Provider::Swarm => {
            &["local_socket", "plain_tcp", "mutual_tls", "mutual_tls_inline"]
        }
        Provider::Kubernetes => &["kubeconfig_file", "kubeconfig_inline", "direct"],
        _ => &[],
    }
}

/// 解析来源编码
pub fn provider_from_str(value: &str) -> Result<Provider, String> {
    value
        .parse::<Provider>()
        .map_err(|err| format!("无效的采集来源：{err}"))
}

/// 载荷 → ops 侧认证方式
pub fn auth_payload_to_kind(payload: &OpsAuthPayload) -> AuthKind {
    match payload {
        OpsAuthPayload::LocalSocket => AuthKind::LocalSocket,
        OpsAuthPayload::PlainTcp => AuthKind::PlainTcp,
        OpsAuthPayload::MutualTls {
            ca_path,
            cert_path,
            key_path,
        } => AuthKind::MutualTls {
            ca_path: ca_path.clone(),
            cert_path: cert_path.clone(),
            key_path: key_path.clone(),
        },
        OpsAuthPayload::MutualTlsInline {
            ca_pem,
            cert_pem,
            key_pem,
        } => AuthKind::MutualTlsInline {
            ca_pem: ca_pem.clone(),
            cert_pem: cert_pem.clone(),
            key_pem: key_pem.clone(),
        },
        OpsAuthPayload::KubeconfigFile { path, context } => AuthKind::KubeconfigFile {
            path: path.clone(),
            context: context.clone(),
        },
        OpsAuthPayload::KubeconfigInline { yaml, context } => AuthKind::KubeconfigInline {
            yaml: yaml.clone(),
            context: context.clone(),
        },
        OpsAuthPayload::Direct {
            token,
            ca_pem,
            client_cert_pem,
            client_key_pem,
            insecure_skip_tls_verify,
        } => AuthKind::Direct {
            token: token.clone(),
            ca_pem: ca_pem.clone(),
            client_cert_pem: client_cert_pem.clone(),
            client_key_pem: client_key_pem.clone(),
            insecure_skip_tls_verify: *insecure_skip_tls_verify,
        },
    }
}

/// ops 侧认证方式 → 载荷（落库 / 回读用）
///
/// 只覆盖本模块负责的容器 / 编排来源；数据资产来源（缓存 / 向量库）的认证
/// 载荷归属各自的模块，这里显式返回 `None`——不猜、不用默认值兜底。
pub fn auth_kind_to_payload(kind: &AuthKind) -> Option<OpsAuthPayload> {
    let payload = match kind {
        AuthKind::LocalSocket => OpsAuthPayload::LocalSocket,
        AuthKind::PlainTcp => OpsAuthPayload::PlainTcp,
        AuthKind::MutualTls {
            ca_path,
            cert_path,
            key_path,
        } => OpsAuthPayload::MutualTls {
            ca_path: ca_path.clone(),
            cert_path: cert_path.clone(),
            key_path: key_path.clone(),
        },
        AuthKind::MutualTlsInline {
            ca_pem,
            cert_pem,
            key_pem,
        } => OpsAuthPayload::MutualTlsInline {
            ca_pem: ca_pem.clone(),
            cert_pem: cert_pem.clone(),
            key_pem: key_pem.clone(),
        },
        AuthKind::KubeconfigFile { path, context } => OpsAuthPayload::KubeconfigFile {
            path: path.clone(),
            context: context.clone(),
        },
        AuthKind::KubeconfigInline { yaml, context } => OpsAuthPayload::KubeconfigInline {
            yaml: yaml.clone(),
            context: context.clone(),
        },
        AuthKind::Direct {
            token,
            ca_pem,
            client_cert_pem,
            client_key_pem,
            insecure_skip_tls_verify,
        } => OpsAuthPayload::Direct {
            token: token.clone(),
            ca_pem: ca_pem.clone(),
            client_cert_pem: client_cert_pem.clone(),
            client_key_pem: client_key_pem.clone(),
            insecure_skip_tls_verify: *insecure_skip_tls_verify,
        },
        _ => return None,
    };
    Some(payload)
}
// ======================== 视图与配置辅助 ========================

/// `ops_connection` 的显式列清单（与 `OpsConnection` 字段一一对应）
const CONNECTION_COLUMNS: &str = "id, connection_name, provider, endpoint, auth_type, auth_config_enc, \
     credential_hint, namespace, timeout_secs, docker_api_version, max_items, poll_interval_secs, \
     enabled, last_collect_at, last_error, created_by, created_at, updated_by, updated_at, deleted";

/// 认证方式是否携带需要加密保存的凭证
fn auth_requires_credential(auth_type: &str) -> bool {
    matches!(
        auth_type,
        "mutual_tls" | "mutual_tls_inline" | "kubeconfig_inline" | "direct"
    )
}

/// 凭证提示文案（绝不含明文）
fn credential_hint_for(payload: &OpsAuthPayload) -> Option<String> {
    match payload {
        OpsAuthPayload::MutualTls { .. } => Some("客户端证书文件路径（CA / 证书 / 私钥）已保存".to_string()),
        OpsAuthPayload::MutualTlsInline { .. } => Some("客户端证书 PEM（CA / 证书 / 私钥）已加密保存".to_string()),
        OpsAuthPayload::KubeconfigInline { .. } => Some("kubeconfig 内容已加密保存".to_string()),
        OpsAuthPayload::Direct {
            token,
            client_cert_pem,
            ..
        } => match (token.is_some(), client_cert_pem.is_some()) {
            (true, true) => Some("SA Token 与客户端证书均已加密保存".to_string()),
            (true, false) => Some("SA Token 已加密保存".to_string()),
            (false, true) => Some("客户端证书已加密保存".to_string()),
            (false, false) => None,
        },
        OpsAuthPayload::KubeconfigFile { .. } => {
            Some("kubeconfig 由后端主机本地文件提供".to_string())
        }
        OpsAuthPayload::LocalSocket | OpsAuthPayload::PlainTcp => None,
    }
}

/// 端点层级（local / intranet / remote）
fn tier_of(endpoint: &str) -> String {
    let key = endpoint
        .split("://")
        .nth(1)
        .unwrap_or(endpoint)
        .trim_end_matches('/');
    if key.is_empty() {
        // local:// 或未填写端点：按本机处理（远程端点为空会被 validate 拦下）
        return ConnectionTier::Local.as_str().to_string();
    }
    ConnectionTier::from_endpoint(key).as_str().to_string()
}

/// 风险提示（只依赖 auth_type / 端点，不解密凭证）
fn risk_hints_for(auth_type: &str, endpoint: &str) -> Vec<String> {
    let mut hints = Vec::new();
    match auth_type {
        "plain_tcp" => hints.push(
            "明文 TCP 未加密，凭证与数据可能被旁路嗅探，仅限隔离内网使用".to_string(),
        ),
        "mutual_tls" => hints.push(
            "证书为后端主机本地文件，请确认文件权限与轮换策略".to_string(),
        ),
        "local_socket" => hints.push("仅能采集后端进程所在主机".to_string()),
        "kubeconfig_file" => hints.push(
            "kubeconfig 从后端主机本地读取，请确认文件权限".to_string(),
        ),
        _ => {}
    }
    if tier_of(endpoint) == "remote" {
        hints.push("端点为公网地址，请确认网络与安全策略".to_string());
    }
    hints
}

/// 组装 `ConnectionConfig`（含参数校验与端点归一化）
fn build_config(
    input: &OpsConnectionInput,
    payload: &OpsAuthPayload,
    name: &str,
) -> Result<ConnectionConfig, String> {
    let provider = provider_from_str(&input.provider)?;
    if provider.is_data_store() {
        return Err(format!(
            "{} 属于数据资产来源，本模块不负责其采集",
            provider.label()
        ));
    }
    let kind = auth_payload_to_kind(payload);
    if !allowed_auth_types(provider).contains(&kind.describe()) {
        return Err(format!(
            "{} 不支持 {} 认证方式",
            provider.label(),
            kind.describe()
        ));
    }
    let mut config = ConnectionConfig::new(name.to_string(), provider, input.endpoint.trim(), kind);
    if let Some(ns) = input.namespace.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        config = config.with_namespace(ns);
    }
    if let Some(version) = input
        .docker_api_version
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        config = config.with_docker_api_version(version);
    }
    if let Some(secs) = input.timeout_secs {
        config = config.with_timeout_secs(u64::from(secs.max(1)));
    }
    if let Some(max) = input.max_items {
        config = config.with_max_items(max.max(1));
    }
    config.validate().map_err(|err| err.to_string())?;
    Ok(config)
}

/// 更新场景下沿用旧认证方式时的端点归一化
///
/// 只补本模块负责的来源的 scheme；其它来源（数据资产）的端点原样返回，
/// 由所属模块自己归一化，本层不猜协议。
fn normalize_endpoint_for(provider: Provider, auth_type: &str, endpoint: &str) -> String {
    let endpoint = endpoint.trim().trim_end_matches('/');
    if endpoint.is_empty() {
        return if auth_type == "local_socket" {
            "local://".to_string()
        } else {
            String::new()
        };
    }
    if endpoint.contains("://") {
        return endpoint.to_string();
    }
    let scheme = match provider {
        Provider::Kubernetes => Some("https"),
        Provider::Docker | Provider::Swarm => Some(if auth_type.starts_with("mutual_tls") {
            "https"
        } else {
            "tcp"
        }),
        _ => None,
    };
    match scheme {
        Some(scheme) => format!("{scheme}://{endpoint}"),
        None => endpoint.to_string(),
    }
}

/// 连接行 → 视图（不含凭证）
fn row_to_view(row: &OpsConnection) -> OpsConnectionView {
    OpsConnectionView {
        id: row.id.to_string(),
        name: row.connection_name.clone(),
        provider: row.provider.clone(),
        provider_label: provider_from_str(&row.provider)
            .map(|p| p.label().to_string())
            .unwrap_or_else(|_| row.provider.clone()),
        endpoint: row.endpoint.clone(),
        auth_type: row.auth_type.clone(),
        has_credential: auth_requires_credential(&row.auth_type),
        credential_hint: row.credential_hint.clone(),
        namespace: row.namespace.clone(),
        timeout_secs: row.timeout_secs.max(1) as u32,
        docker_api_version: row.docker_api_version.clone(),
        max_items: row.max_items.max(1) as usize,
        poll_interval_secs: row.poll_interval_secs.max(1) as u32,
        enabled: row.enabled != 0,
        tier: tier_of(&row.endpoint),
        risk_hints: risk_hints_for(&row.auth_type, &row.endpoint),
        last_collect_at: row.last_collect_at.map(|t| t.to_rfc3339()),
        last_error: row.last_error.clone(),
        created_at: row.created_at.map(|t| t.to_rfc3339()),
        updated_at: row.updated_at.map(|t| t.to_rfc3339()),
    }
}

/// 解密并还原认证载荷
fn decrypt_auth(row: &OpsConnection) -> Result<OpsAuthPayload, String> {
    let encrypted = row
        .auth_config_enc
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("连接「{}」缺少凭证配置，请重新编辑保存", row.connection_name))?;
    let json = decrypt_api_key(encrypted).map_err(|err| format!("解密连接凭证失败：{err}"))?;
    serde_json::from_str(&json).map_err(|err| format!("解析连接凭证失败：{err}"))
}

/// 序列化并加密认证载荷
fn encrypt_auth(payload: &OpsAuthPayload) -> Result<String, String> {
    let json = serde_json::to_string(payload).map_err(|err| format!("序列化连接凭证失败：{err}"))?;
    encrypt_api_key(&json).map_err(|err| format!("加密连接凭证失败：{err}"))
}

/// 连接行 → `ConnectionConfig`
fn config_from_row(row: &OpsConnection) -> Result<ConnectionConfig, String> {
    let provider = provider_from_str(&row.provider)?;
    if provider.is_data_store() {
        return Err(format!(
            "{} 属于数据资产来源，本模块不负责其采集",
            provider.label()
        ));
    }
    let payload = decrypt_auth(row)?;
    let kind = auth_payload_to_kind(&payload);
    let mut config = ConnectionConfig::new(
        row.connection_name.clone(),
        provider,
        row.endpoint.clone(),
        kind,
    )
    .with_id(row.id.to_string())
    .with_timeout_secs(row.timeout_secs.max(1) as u64)
    .with_max_items(row.max_items.max(1) as usize);
    if let Some(ns) = row.namespace.as_deref().filter(|value| !value.is_empty()) {
        config = config.with_namespace(ns);
    }
    if let Some(version) = row.docker_api_version.as_deref().filter(|v| !v.is_empty()) {
        config = config.with_docker_api_version(version);
    }
    config.validate().map_err(|err| err.to_string())?;
    Ok(config)
}

// ======================== 连接 CRUD ========================

/// 连接列表（按 id 升序）
pub async fn list_connections() -> Result<Vec<OpsConnectionView>, String> {
    let pool = database::get_read_pool().map_err(|err| format!("数据库连接失败: {err}"))?;
    let p = database::schema_prefix();
    let sql = format!(
        "SELECT {CONNECTION_COLUMNS} FROM {p}ops_connection WHERE deleted = 0 ORDER BY id ASC"
    );
    let rows = sqlx::query_as::<_, OpsConnection>(sqlx::AssertSqlSafe(sql))
        .fetch_all(&pool)
        .await
        .map_err(|err| {
            error!("查询运维连接列表失败: {err}");
            format!("查询连接列表失败: {err}")
        })?;
    Ok(rows.iter().map(row_to_view).collect())
}

/// 读取单行（不存在即报错）
async fn load_row(id: i64) -> Result<OpsConnection, String> {
    let pool = database::get_read_pool().map_err(|err| format!("数据库连接失败: {err}"))?;
    let p = database::schema_prefix();
    let sql = format!(
        "SELECT {CONNECTION_COLUMNS} FROM {p}ops_connection WHERE id = $1 AND deleted = 0"
    );
    sqlx::query_as::<_, OpsConnection>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(&pool)
        .await
        .map_err(|err| format!("查询连接失败: {err}"))?
        .ok_or_else(|| format!("连接不存在：{id}"))
}

/// 单条连接
pub async fn get_connection(id: i64) -> Result<OpsConnectionView, String> {
    let row = load_row(id).await?;
    Ok(row_to_view(&row))
}

/// 同租户内连接名唯一校验
async fn ensure_name_unique(
    pool: &sqlx::PgPool,
    name: &str,
    exclude_id: Option<i64>,
) -> Result<(), String> {
    let p = database::schema_prefix();
    let sql = format!(
        "SELECT id FROM {p}ops_connection WHERE connection_name = $1 AND deleted = 0 \
         AND ($2::BIGINT IS NULL OR id <> $2) LIMIT 1"
    );
    let hit: Option<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(name)
        .bind(exclude_id)
        .fetch_optional(pool)
        .await
        .map_err(|err| format!("校验连接名称失败: {err}"))?;
    if hit.is_some() {
        return Err(format!("连接名称已存在：{name}"));
    }
    Ok(())
}

/// 新建连接
pub async fn create_connection(
    input: OpsConnectionInput,
    user_id: Option<i64>,
) -> Result<OpsConnectionView, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("连接名称不能为空".to_string());
    }
    let provider = provider_from_str(&input.provider)?;
    let payload = input
        .auth
        .clone()
        .ok_or_else(|| "新建连接必须提供认证方式".to_string())?;
    let config = build_config(&input, &payload, name)?;
    let auth_type = auth_payload_to_kind(&payload).describe().to_string();

    let pool = database::get_write_pool().map_err(|err| format!("数据库连接失败: {err}"))?;
    ensure_name_unique(&pool, name, None).await?;

    let p = database::schema_prefix();
    let sql = format!(
        "INSERT INTO {p}ops_connection \
         (id, connection_name, provider, endpoint, auth_type, auth_config_enc, credential_hint, \
          namespace, timeout_secs, docker_api_version, max_items, poll_interval_secs, enabled, \
          created_by, created_at, updated_by, updated_at, deleted) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, NOW(), $14, NOW(), 0) \
         RETURNING {CONNECTION_COLUMNS}"
    );
    let row = sqlx::query_as::<_, OpsConnection>(sqlx::AssertSqlSafe(sql))
        .bind(next_id() as i64)
        .bind(name)
        .bind(provider.as_str())
        .bind(config.endpoint.as_str())
        .bind(&auth_type)
        .bind(encrypt_auth(&payload)?)
        .bind(credential_hint_for(&payload))
        .bind(input.namespace.as_deref().map(str::trim).filter(|v| !v.is_empty()))
        .bind(input.timeout_secs.unwrap_or(15).max(1) as i32)
        .bind(input.docker_api_version.as_deref().map(str::trim).filter(|v| !v.is_empty()))
        .bind(input.max_items.unwrap_or(20000).max(1) as i32)
        .bind(input.poll_interval_secs.unwrap_or(300).max(1) as i32)
        .bind(if input.enabled.unwrap_or(true) { 1_i16 } else { 0_i16 })
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .map_err(|err| {
            error!("创建运维连接失败: name={name}, err={err}");
            format!("创建连接失败: {err}")
        })?;
    info!(
        "创建运维连接成功: id={}, name={}, provider={}",
        row.id, row.connection_name, row.provider
    );
    Ok(row_to_view(&row))
}

/// 更新连接（`auth` 为 `None` 时沿用已保存的凭证与认证方式）
pub async fn update_connection(
    id: i64,
    input: OpsConnectionInput,
    user_id: Option<i64>,
) -> Result<OpsConnectionView, String> {
    let existing = load_row(id).await?;
    let name = input.name.trim();
    if name.is_empty() {
        return Err("连接名称不能为空".to_string());
    }
    let provider = provider_from_str(&input.provider)?;

    let (auth_type, auth_config_enc, credential_hint, endpoint) = match input.auth.as_ref() {
        Some(payload) => {
            let config = build_config(&input, payload, name)?;
            (
                auth_payload_to_kind(payload).describe().to_string(),
                Some(encrypt_auth(payload)?),
                credential_hint_for(payload),
                config.endpoint,
            )
        }
        None => {
            if existing.provider != provider.as_str() {
                return Err("切换采集来源时必须重新选择认证方式".to_string());
            }
            if !allowed_auth_types(provider).contains(&existing.auth_type.as_str()) {
                return Err(format!(
                    "{} 不支持 {} 认证方式",
                    provider.label(),
                    existing.auth_type
                ));
            }
            (
                existing.auth_type.clone(),
                existing.auth_config_enc.clone(),
                existing.credential_hint.clone(),
                normalize_endpoint_for(provider, &existing.auth_type, &input.endpoint),
            )
        }
    };
    if endpoint.trim().is_empty() {
        return Err("远程连接必须填写端点".to_string());
    }

    let pool = database::get_write_pool().map_err(|err| format!("数据库连接失败: {err}"))?;
    ensure_name_unique(&pool, name, Some(id)).await?;

    let p = database::schema_prefix();
    let sql = format!(
        "UPDATE {p}ops_connection SET connection_name = $2, provider = $3, endpoint = $4, \
         auth_type = $5, auth_config_enc = $6, credential_hint = $7, namespace = $8, \
         timeout_secs = $9, docker_api_version = $10, max_items = $11, poll_interval_secs = $12, \
         enabled = $13, updated_by = $14, updated_at = NOW() \
         WHERE id = $1 AND deleted = 0 RETURNING {CONNECTION_COLUMNS}"
    );
    let row = sqlx::query_as::<_, OpsConnection>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(name)
        .bind(provider.as_str())
        .bind(&endpoint)
        .bind(&auth_type)
        .bind(auth_config_enc)
        .bind(credential_hint)
        .bind(input.namespace.as_deref().map(str::trim).filter(|v| !v.is_empty()))
        .bind(input.timeout_secs.unwrap_or(15).max(1) as i32)
        .bind(input.docker_api_version.as_deref().map(str::trim).filter(|v| !v.is_empty()))
        .bind(input.max_items.unwrap_or(20000).max(1) as i32)
        .bind(input.poll_interval_secs.unwrap_or(300).max(1) as i32)
        .bind(if input.enabled.unwrap_or(true) { 1_i16 } else { 0_i16 })
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .map_err(|err| format!("更新连接失败: {err}"))?;
    info!("更新运维连接成功: id={}, name={}", row.id, row.connection_name);
    Ok(row_to_view(&row))
}

/// 删除连接（软删；`asset_ops_link` 历史保留）
pub async fn delete_connection(id: i64) -> Result<(), String> {
    let pool = database::get_write_pool().map_err(|err| format!("数据库连接失败: {err}"))?;
    let p = database::schema_prefix();
    let sql = format!(
        "UPDATE {p}ops_connection SET deleted = 1, updated_at = NOW() WHERE id = $1 AND deleted = 0"
    );
    let result = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|err| format!("删除连接失败: {err}"))?;
    if result.rows_affected() == 0 {
        return Err(format!("连接不存在：{id}"));
    }
    info!("删除运维连接成功: id={id}");
    Ok(())
}

// ======================== 连通性探测 ========================

/// 按来源构建只读连接器
///
/// 只覆盖本模块负责的容器 / 编排来源。数据资产来源（缓存 / 向量库）的采集
/// 实现归属各自的 vector 侧 crate，本模块显式拒绝，绝不「静默当作成功」。
async fn build_connector(config: ConnectionConfig) -> Result<Arc<dyn Connector>, String> {
    match config.provider {
        Provider::Docker | Provider::Swarm => DockerConnector::connect(config)
            .map(|connector| Arc::new(connector) as Arc<dyn Connector>)
            .map_err(|err| format!("初始化 Docker 连接器失败：{err}")),
        Provider::Kubernetes => K8sConnector::connect(config)
            .await
            .map(|connector| Arc::new(connector) as Arc<dyn Connector>)
            .map_err(|err| format!("初始化 Kubernetes 连接器失败：{err}")),
        other => Err(format!(
            "{} 采集尚未接入（当前版本仅支持 Docker / Swarm / Kubernetes）",
            other.label()
        )),
    }
}

/// 探测结果 → 视图
fn health_to_view(health: ops::ConnectionHealth) -> OpsHealthView {
    let failure_class = health.failure.code().to_string();
    let failure_hint = health.failure.hint().to_string();
    let summary = health.summary();
    OpsHealthView {
        ok: health.ok,
        provider: health.provider.as_str().to_string(),
        provider_label: health.provider.label().to_string(),
        tier: tier_of(&health.endpoint),
        endpoint: health.endpoint.clone(),
        latency_ms: health.latency_ms,
        server_version: health.server_version.clone(),
        api_version: health.api_version.clone(),
        authenticated: health.authenticated,
        authorized: health.authorized,
        warnings: health.warnings.clone(),
        error: health.error.clone(),
        failure_class,
        failure_hint,
        summary,
    }
}

/// 把最近一次探测的失败原因回写到连接（成功则清空）
async fn record_health(connection_id: i64, view: &OpsHealthView) {
    let Ok(pool) = database::get_write_pool() else {
        return;
    };
    let p = database::schema_prefix();
    let sql = format!(
        "UPDATE {p}ops_connection SET last_error = $2, updated_at = NOW() WHERE id = $1"
    );
    let text = if view.ok {
        None
    } else {
        view.error.clone().or_else(|| Some(view.summary.clone()))
    };
    if let Err(err) = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(connection_id)
        .bind(text)
        .execute(&pool)
        .await
    {
        error!("回写运维连接探测结果失败: id={connection_id}, err={err}");
    }
}

/// 草稿探测（**不落库**）：用于界面上「先测再存」
pub async fn test_draft(input: &OpsConnectionInput) -> Result<OpsHealthView, String> {
    let payload = input
        .auth
        .clone()
        .ok_or_else(|| "请先选择认证方式".to_string())?;
    let fallback = "草稿连接";
    let name = if input.name.trim().is_empty() {
        fallback
    } else {
        input.name.trim()
    };
    let config = build_config(input, &payload, name)?;
    let connector = build_connector(config).await?;
    let health = connector
        .health()
        .await
        .map_err(|err| format!("探测失败：{err}"))?;
    Ok(health_to_view(health))
}

/// 已存连接探测（结果回写 last_error）
pub async fn test_connection(id: i64) -> Result<OpsHealthView, String> {
    let row = load_row(id).await?;
    let config = config_from_row(&row)?;
    let connector = build_connector(config).await?;
    let health = connector
        .health()
        .await
        .map_err(|err| format!("探测失败：{err}"))?;
    let view = health_to_view(health);
    record_health(id, &view).await;
    info!(
        "运维连接探测完成: id={}, ok={}, failure={}",
        id, view.ok, view.failure_class
    );
    Ok(view)
}

// ======================== 手动采集 ========================

/// 手动采集一次，并把观测结果 upsert 进 `asset_ops_link`（不写 `assets`）
pub async fn collect_now(id: i64) -> Result<OpsCollectResult, String> {
    let row = load_row(id).await?;
    let config = config_from_row(&row)?;
    let provider = config.provider;
    let endpoint = config.endpoint.clone();
    let started_at = chrono::Utc::now();
    let connector = build_connector(config).await?;
    let snapshot = match connector.collect().await {
        Ok(snapshot) => snapshot,
        Err(err) => {
            let message = err.to_string();
            record_collect_error(id, &message).await;
            return Err(format!("采集失败：{message}"));
        }
    };

    let (created, updated) = upsert_links(&row, &snapshot).await?;
    let gone = if snapshot.partial_errors.is_empty() {
        mark_gone(id, started_at).await?
    } else {
        0
    };
    record_collect_success(id).await;

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for record in &snapshot.resources {
        *counts.entry(record.key.kind.as_str()).or_insert(0) += 1;
    }
    let counts_by_kind = counts
        .into_iter()
        .map(|(kind, count)| OpsKindCount {
            kind: kind.to_string(),
            count,
        })
        .collect();

    info!(
        "运维采集完成: id={}, total={}, created={}, updated={}, gone={}",
        id, snapshot.resources.len(), created, updated, gone
    );
    Ok(OpsCollectResult {
        connection_id: id.to_string(),
        provider: provider.as_str().to_string(),
        endpoint,
        collected_at: snapshot.collected_at.to_string(),
        elapsed_ms: snapshot.elapsed_ms,
        total: snapshot.resources.len(),
        short_lived: snapshot.short_lived_count(),
        created,
        updated,
        gone,
        counts_by_kind,
        partial_errors: snapshot.partial_errors.clone(),
    })
}

/// 把快照里的资源按 `(connection_id, resource_uid)` 幂等 upsert
async fn upsert_links(
    row: &OpsConnection,
    snapshot: &ResourceSnapshot,
) -> Result<(usize, usize), String> {
    if snapshot.resources.is_empty() {
        return Ok((0, 0));
    }
    let pool = database::get_write_pool().map_err(|err| format!("数据库连接失败: {err}"))?;
    let p = database::schema_prefix();
    let sql = format!(
        "INSERT INTO {p}asset_ops_link \
         (id, connection_id, provider, resource_uid, resource_kind, display_name, status_snapshot, \
          raw_state, parent_uid, attributes, first_seen_at, last_seen_at, created_by, \
          created_at, updated_at, deleted) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NOW(), NOW(), $11, NOW(), NOW(), 0) \
         ON CONFLICT (connection_id, resource_uid) DO UPDATE SET \
          provider = EXCLUDED.provider, resource_kind = EXCLUDED.resource_kind, \
          display_name = EXCLUDED.display_name, status_snapshot = EXCLUDED.status_snapshot, \
          raw_state = EXCLUDED.raw_state, parent_uid = EXCLUDED.parent_uid, \
          attributes = EXCLUDED.attributes, last_seen_at = NOW(), updated_at = NOW(), deleted = 0 \
         RETURNING (xmax = 0) AS inserted"
    );
    let mut created = 0usize;
    let mut updated = 0usize;
    for record in &snapshot.resources {
        let inserted: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(sql.clone()))
            .bind(next_id() as i64)
            .bind(row.id)
            .bind(row.provider.as_str())
            .bind(record.uid())
            .bind(record.key.kind.as_str())
            .bind(&record.display_name)
            .bind(record.status.as_str())
            .bind(record.raw_state.as_deref())
            .bind(record.parent_uid.as_deref())
            .bind(&record.attributes)
            .bind(row.created_by)
            .fetch_one(&pool)
            .await
            .map_err(|err| format!("写入资源关联失败: {err}"))?;
        if inserted {
            created += 1;
        } else {
            updated += 1;
        }
    }
    Ok((created, updated))
}

/// 完整采集后，把本次未出现的资源标记为 gone（`last_seen_at` 保留最后出现时间）
async fn mark_gone(
    connection_id: i64,
    started_at: chrono::DateTime<chrono::Utc>,
) -> Result<usize, String> {
    let pool = database::get_write_pool().map_err(|err| format!("数据库连接失败: {err}"))?;
    let p = database::schema_prefix();
    let sql = format!(
        "UPDATE {p}asset_ops_link SET status_snapshot = 'gone', updated_at = NOW() \
         WHERE connection_id = $1 AND deleted = 0 AND last_seen_at < $2"
    );
    let result = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(connection_id)
        .bind(started_at)
        .execute(&pool)
        .await
        .map_err(|err| format!("标记下线资源失败: {err}"))?;
    Ok(result.rows_affected() as usize)
}

/// 采集成功：刷新 last_collect_at 并清空 last_error
async fn record_collect_success(connection_id: i64) {
    let Ok(pool) = database::get_write_pool() else {
        return;
    };
    let p = database::schema_prefix();
    let sql = format!(
        "UPDATE {p}ops_connection SET last_collect_at = NOW(), last_error = NULL, \
         updated_at = NOW() WHERE id = $1"
    );
    if let Err(err) = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(connection_id)
        .execute(&pool)
        .await
    {
        error!("回写运维采集时间失败: id={connection_id}, err={err}");
    }
}

/// 采集失败：记录原因（保留 last_collect_at 不变）
async fn record_collect_error(connection_id: i64, message: &str) {
    let Ok(pool) = database::get_write_pool() else {
        return;
    };
    let p = database::schema_prefix();
    let sql = format!(
        "UPDATE {p}ops_connection SET last_error = $2, updated_at = NOW() WHERE id = $1"
    );
    if let Err(err) = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(connection_id)
        .bind(message)
        .execute(&pool)
        .await
    {
        error!("回写运维采集失败原因失败: id={connection_id}, err={err}");
    }
}

// ======================== 资源清单 / 归档 ========================

/// 资源清单查询行（LEFT JOIN 台账取 asset_no）
#[derive(Debug, sqlx::FromRow)]
struct LinkRow {
    id: i64,
    connection_id: i64,
    connection_name: Option<String>,
    provider: String,
    resource_uid: String,
    resource_kind: String,
    display_name: Option<String>,
    asset_id: Option<i64>,
    asset_no: Option<String>,
    status_snapshot: Option<String>,
    raw_state: Option<String>,
    parent_uid: Option<String>,
    attributes: Option<serde_json::Value>,
    first_seen_at: Option<chrono::DateTime<chrono::Utc>>,
    last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// 资源清单的公共 SELECT（三表 LEFT JOIN，台账表只读）
fn link_select(p: &str) -> String {
    format!(
        "SELECT l.id, l.connection_id, c.connection_name, l.provider, l.resource_uid, \
         l.resource_kind, l.display_name, l.asset_id, a.asset_no, l.status_snapshot, \
         l.raw_state, l.parent_uid, l.attributes, l.first_seen_at, l.last_seen_at \
         FROM {p}asset_ops_link l \
         LEFT JOIN {p}ops_connection c ON c.id = l.connection_id \
         LEFT JOIN {p}assets a ON a.id = l.asset_id"
    )
}

fn link_row_to_view(row: LinkRow) -> OpsResourceLinkView {
    OpsResourceLinkView {
        id: row.id.to_string(),
        connection_id: row.connection_id.to_string(),
        connection_name: row.connection_name,
        provider: row.provider,
        resource_uid: row.resource_uid,
        resource_kind: row.resource_kind,
        display_name: row.display_name,
        asset_id: row.asset_id.map(|value| value.to_string()),
        asset_no: row.asset_no,
        status_snapshot: row.status_snapshot,
        raw_state: row.raw_state,
        parent_uid: row.parent_uid,
        attributes: row.attributes,
        first_seen_at: row.first_seen_at.map(|value| value.to_rfc3339()),
        last_seen_at: row.last_seen_at.map(|value| value.to_rfc3339()),
    }
}

/// 资源清单（可按连接过滤，按最近发现时间倒序，最多 2000 条）
pub async fn list_links(connection_id: Option<i64>) -> Result<Vec<OpsResourceLinkView>, String> {
    let pool = database::get_read_pool().map_err(|err| format!("数据库连接失败: {err}"))?;
    let p = database::schema_prefix();
    let sql = format!(
        "{} WHERE l.deleted = 0 AND ($1::BIGINT IS NULL OR l.connection_id = $1) \
         ORDER BY l.last_seen_at DESC LIMIT 2000",
        link_select(&p)
    );
    let rows = sqlx::query_as::<_, LinkRow>(sqlx::AssertSqlSafe(sql))
        .bind(connection_id)
        .fetch_all(&pool)
        .await
        .map_err(|err| {
            error!("查询运维资源清单失败: {err}");
            format!("查询资源清单失败: {err}")
        })?;
    Ok(rows.into_iter().map(link_row_to_view).collect())
}

/// 单条资源关联
async fn link_view_by_id(id: i64) -> Result<OpsResourceLinkView, String> {
    let pool = database::get_read_pool().map_err(|err| format!("数据库连接失败: {err}"))?;
    let p = database::schema_prefix();
    let sql = format!("{} WHERE l.id = $1 AND l.deleted = 0", link_select(&p));
    let row = sqlx::query_as::<_, LinkRow>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(&pool)
        .await
        .map_err(|err| format!("查询资源关联失败: {err}"))?
        .ok_or_else(|| format!("资源关联不存在：{id}"))?;
    Ok(link_row_to_view(row))
}

/// 把观测到的资源归档到已有资产（台账表零结构变更，仅回填 `asset_ops_link.asset_id`）
pub async fn bind_link(
    link_id: i64,
    asset_id: i64,
    _user_id: Option<i64>,
) -> Result<OpsResourceLinkView, String> {
    let pool = database::get_write_pool().map_err(|err| format!("数据库连接失败: {err}"))?;
    let p = database::schema_prefix();

    let link_exists: Option<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT id FROM {p}asset_ops_link WHERE id = $1 AND deleted = 0"
    )))
    .bind(link_id)
    .fetch_optional(&pool)
    .await
    .map_err(|err| format!("查询资源关联失败: {err}"))?;
    if link_exists.is_none() {
        return Err(format!("资源关联不存在：{link_id}"));
    }

    // 台账表只做只读校验，不做任何结构变更
    let asset_no: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT asset_no FROM {p}assets WHERE id = $1 AND (deleted IS NULL OR deleted = 0)"
    )))
    .bind(asset_id)
    .fetch_optional(&pool)
    .await
    .map_err(|err| format!("查询资产失败: {err}"))?;
    if asset_no.is_none() {
        return Err(format!("资产不存在：{asset_id}"));
    }

    sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE {p}asset_ops_link SET asset_id = $2, updated_at = NOW() \
         WHERE id = $1 AND deleted = 0"
    )))
    .bind(link_id)
    .bind(asset_id)
    .execute(&pool)
    .await
    .map_err(|err| format!("绑定资产失败: {err}"))?;

    info!("运维资源绑定资产成功: link_id={link_id}, asset_id={asset_id}");
    link_view_by_id(link_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pem(label: &str) -> String {
        format!("-----BEGIN {label}-----\nMIIB\n-----END {label}-----\n")
    }

    fn all_payloads() -> Vec<OpsAuthPayload> {
        vec![
            OpsAuthPayload::LocalSocket,
            OpsAuthPayload::PlainTcp,
            OpsAuthPayload::MutualTls {
                ca_path: "ca.pem".to_string(),
                cert_path: "cert.pem".to_string(),
                key_path: "key.pem".to_string(),
            },
            OpsAuthPayload::MutualTlsInline {
                ca_pem: pem("CA"),
                cert_pem: pem("CERT"),
                key_pem: pem("KEY"),
            },
            OpsAuthPayload::KubeconfigFile {
                path: "/root/.kube/config".to_string(),
                context: Some("prod".to_string()),
            },
            OpsAuthPayload::KubeconfigInline {
                yaml: "apiVersion: v1".to_string(),
                context: None,
            },
            OpsAuthPayload::Direct {
                token: Some("token".to_string()),
                ca_pem: Some(pem("CA")),
                client_cert_pem: None,
                client_key_pem: None,
                insecure_skip_tls_verify: true,
            },
        ]
    }

    #[test]
    fn auth_payload_round_trips_through_auth_kind() {
        for payload in all_payloads() {
            let kind = auth_payload_to_kind(&payload);
            let back = auth_kind_to_payload(&kind).expect("容器 / 编排认证方式必须可回写");
            assert_eq!(payload, back);
        }
    }

    #[test]
    fn auth_payload_serializes_with_type_tag() {
        let json = serde_json::to_value(OpsAuthPayload::PlainTcp).expect("序列化失败");
        assert_eq!(json.get("type").and_then(|v| v.as_str()), Some("plain_tcp"));
        let json = serde_json::to_value(OpsAuthPayload::MutualTlsInline {
            ca_pem: pem("CA"),
            cert_pem: pem("CERT"),
            key_pem: pem("KEY"),
        })
        .expect("序列化失败");
        assert_eq!(
            json.get("type").and_then(|v| v.as_str()),
            Some("mutual_tls_inline")
        );
    }

    #[test]
    fn auth_payload_debug_masks_secrets() {
        let inline = OpsAuthPayload::MutualTlsInline {
            ca_pem: pem("CA"),
            cert_pem: pem("CERT"),
            key_pem: pem("KEY"),
        };
        let text = format!("{inline:?}");
        assert!(!text.contains("BEGIN"), "PEM 不得出现在 Debug 输出: {text}");
        assert!(text.contains("<已隐藏>"), "{text}");

        let direct = OpsAuthPayload::Direct {
            token: Some("secret-token".to_string()),
            ca_pem: None,
            client_cert_pem: None,
            client_key_pem: None,
            insecure_skip_tls_verify: true,
        };
        let text = format!("{direct:?}");
        assert!(!text.contains("secret-token"), "Token 不得出现在 Debug 输出: {text}");
    }

    #[test]
    fn provider_codes_map_both_ways() {
        for provider in Provider::ALL {
            let code = provider.as_str();
            assert_eq!(provider_from_str(code).expect("解析失败"), provider);
        }
        assert_eq!(
            provider_from_str("kubernetes").expect("别名解析失败"),
            Provider::Kubernetes
        );
        assert!(provider_from_str("vmware").is_err());
    }

    #[test]
    fn allowed_auth_types_follow_provider() {
        assert!(allowed_auth_types(Provider::Docker).contains(&"mutual_tls_inline"));
        assert!(allowed_auth_types(Provider::Swarm).contains(&"plain_tcp"));
        assert!(!allowed_auth_types(Provider::Docker).contains(&"direct"));
        assert!(allowed_auth_types(Provider::Kubernetes).contains(&"direct"));
        assert!(!allowed_auth_types(Provider::Kubernetes).contains(&"plain_tcp"));
    }

    #[test]
    fn risk_hints_flag_plain_tcp_and_public_endpoint() {
        let hints = risk_hints_for("plain_tcp", "tcp://10.0.0.5:2375");
        assert!(hints.iter().any(|hint| hint.contains("明文")), "{hints:?}");
        let remote = risk_hints_for("direct", "https://8.8.8.8:6443");
        assert!(remote.iter().any(|hint| hint.contains("公网")), "{remote:?}");
        assert!(risk_hints_for("direct", "https://10.0.0.9:6443").is_empty());
    }

    #[test]
    fn tier_follows_endpoint() {
        assert_eq!(tier_of("local://"), "local");
        assert_eq!(tier_of("tcp://10.0.0.5:2375"), "intranet");
        assert_eq!(tier_of("https://docker.example.com:2376"), "remote");
    }

    #[test]
    fn endpoint_normalization_matches_auth_type() {
        assert_eq!(
            normalize_endpoint_for(Provider::Docker, "plain_tcp", "10.0.0.5:2375"),
            "tcp://10.0.0.5:2375"
        );
        assert_eq!(
            normalize_endpoint_for(Provider::Docker, "mutual_tls_inline", "10.0.0.5:2376"),
            "https://10.0.0.5:2376"
        );
        assert_eq!(
            normalize_endpoint_for(Provider::Kubernetes, "direct", "10.0.0.10:6443"),
            "https://10.0.0.10:6443"
        );
        assert_eq!(
            normalize_endpoint_for(Provider::Docker, "local_socket", ""),
            "local://"
        );
    }

    #[test]
    fn credential_hints_never_expose_secrets() {
        let payload = OpsAuthPayload::MutualTlsInline {
            ca_pem: pem("CA"),
            cert_pem: pem("CERT"),
            key_pem: pem("KEY"),
        };
        let hint = credential_hint_for(&payload).expect("应有提示");
        assert!(!hint.contains("BEGIN"), "{hint}");
        assert!(auth_requires_credential("mutual_tls_inline"));
        assert!(!auth_requires_credential("local_socket"));
    }
}
