//! 容器日志 / 性能 Service（Docker + K8s，只读）
//!
//! 本模块只做「连接行 → 连接器 → ops 只读方法 → 视图」的编排：
//! - 凭证解密 / 端点归一化完全复用 [`crate::ops_service::config_from_row`]，
//!   不复制任何加密逻辑；
//! - 日志正文只随响应返回，**不落库、不进 tracing**；
//! - 错误语义与实施方案 §3.4 一致：连接 / 容器 / Pod 不存在 → 404，
//!   上游超时 → 504，其余参数或 Docker/K8s API 错误 → 400。

use ops::connector::docker::DockerConnector;
use ops::connector::k8s::K8sConnector;
use ops::model::Provider;
use ops::{ContainerLogsRequest, OpsError, PodLogsRequest};
use serde::{Deserialize, Serialize};

use crate::ops_service;

/// 容器日志 / 性能的统一错误（供 HTTP 层映射状态码）
#[derive(Debug, Clone)]
pub enum ContainerProbeError {
    /// 连接 / 容器 / Pod 不存在
    NotFound(String),
    /// 参数或上游 API 错误
    BadRequest(String),
    /// 上游超时
    Timeout(String),
    /// 服务端内部错误（数据库等）
    Internal(String),
}

impl std::fmt::Display for ContainerProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(message)
            | Self::BadRequest(message)
            | Self::Timeout(message)
            | Self::Internal(message) => f.write_str(message),
        }
    }
}

impl ContainerProbeError {
    /// 转成命令层 / service 通用字符串
    #[must_use]
    pub fn into_message(self) -> String {
        self.to_string()
    }
}

/// 单行容器日志
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct OpsContainerLogLine {
    /// stdout / stderr / stdin / console
    pub stream: String,
    /// 行内容（保留 ANSI 转义）
    pub message: String,
}

/// Docker 容器日志视图
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct OpsContainerLogsView {
    pub container: String,
    pub lines: Vec<OpsContainerLogLine>,
    pub bytes: usize,
    /// 超字节上限时必须显式置真
    pub truncated: bool,
}

/// Docker 容器性能视图（数值已换算，前端不再二次计算）
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct OpsContainerStatsView {
    pub container: String,
    pub read_at_ms: i64,
    /// CPU%（无基线时为 null）
    pub cpu_percent: Option<f64>,
    pub mem_used_bytes: u64,
    pub mem_limit_bytes: u64,
    pub mem_percent: f64,
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,
    pub pids: Option<u64>,
    pub blkio_read_bytes: Option<u64>,
    pub blkio_write_bytes: Option<u64>,
}

/// K8s Pod 日志视图
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct OpsPodLogsView {
    pub namespace: String,
    pub pod: String,
    /// 实际使用的容器（未指定时为默认容器）
    pub container: Option<String>,
    /// Pod 内全部容器名（前端容器下拉数据源）
    pub containers: Vec<String>,
    pub previous: bool,
    pub lines: Vec<String>,
    pub bytes: usize,
    pub truncated: bool,
}

/// 单个容器的指标
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct OpsPodContainerMetrics {
    pub name: String,
    /// CPU 使用量（nanocores）
    pub cpu_nanocores: u64,
    /// 内存使用量（bytes）
    pub memory_bytes: u64,
}

/// K8s Pod 指标视图（metrics-server，仅 CPU / 内存）
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct OpsPodMetricsView {
    pub namespace: String,
    pub pod: String,
    /// Pod 内全部容器名（前端容器下拉数据源）
    pub containers: Vec<String>,
    /// 采样窗口（如 `15s`）
    pub window: Option<String>,
    /// 采样时间（RFC3339 原文）
    pub timestamp: Option<String>,
    /// 容器级明细
    pub container_metrics: Vec<OpsPodContainerMetrics>,
    /// 所有容器 CPU 之和（nanocores）
    pub cpu_nanocores: u64,
    /// 所有容器内存之和（bytes）
    pub memory_bytes: u64,
}

/// ops 错误 → 统一错误
fn map_ops_error(err: OpsError) -> ContainerProbeError {
    match err {
        OpsError::Timeout(_) => ContainerProbeError::Timeout(err.to_string()),
        OpsError::Api(message) => {
            let lowered = message.to_ascii_lowercase();
            // metrics-server 缺失 / 无权限：统一成 400 + 可读提示（不要误判成资源 404）
            if lowered.contains("metrics-server") {
                return ContainerProbeError::BadRequest(message);
            }
            // 非 json-file / journald 日志驱动：给出可操作的替代命令
            if lowered.contains("logging driver") || lowered.contains("does not support reading") {
                return ContainerProbeError::BadRequest(format!(
                    "该容器日志驱动不支持 API 读取，请改用 `docker logs`：{message}"
                ));
            }
            if lowered.contains("404")
                || lowered.contains("not found")
                || lowered.contains("no such container")
                || lowered.contains("不存在")
            {
                ContainerProbeError::NotFound(message)
            } else {
                ContainerProbeError::BadRequest(message)
            }
        }
        other => ContainerProbeError::BadRequest(other.to_string()),
    }
}

/// 读取连接行（软删视为不存在）
async fn load_connection(
    connection_id: i64,
) -> Result<database::models::OpsConnection, ContainerProbeError> {
    ops_service::load_row(connection_id)
        .await
        .map_err(|message| {
            if message.contains("不存在") {
                ContainerProbeError::NotFound(message)
            } else {
                ContainerProbeError::Internal(message)
            }
        })
}

/// 连接行 → Docker 连接器（只接受 docker / swarm）
async fn docker_connector(connection_id: i64) -> Result<DockerConnector, ContainerProbeError> {
    let row = load_connection(connection_id).await?;
    let provider = ops_service::provider_from_str(&row.provider)
        .map_err(ContainerProbeError::BadRequest)?;
    if !matches!(provider, Provider::Docker | Provider::Swarm) {
        return Err(ContainerProbeError::BadRequest(format!(
            "连接「{}」不是 Docker / Swarm 来源",
            row.connection_name
        )));
    }
    let config = ops_service::config_from_row(&row).map_err(ContainerProbeError::BadRequest)?;
    DockerConnector::connect(config).map_err(map_ops_error)
}

/// 连接行 → K8s 连接器（只接受 k8s）
async fn k8s_connector(connection_id: i64) -> Result<K8sConnector, ContainerProbeError> {
    let row = load_connection(connection_id).await?;
    let provider = ops_service::provider_from_str(&row.provider)
        .map_err(ContainerProbeError::BadRequest)?;
    if provider != Provider::Kubernetes {
        return Err(ContainerProbeError::BadRequest(format!(
            "连接「{}」不是 Kubernetes 来源",
            row.connection_name
        )));
    }
    let config = ops_service::config_from_row(&row).map_err(ContainerProbeError::BadRequest)?;
    K8sConnector::connect(config).await.map_err(map_ops_error)
}


/// 拉取 Docker 容器日志
///
/// # Errors
///
/// 连接 / 容器不存在、参数错误、daemon 超时或 API 错误。
pub async fn container_logs(
    connection_id: i64,
    container: &str,
    lines: Option<u32>,
    timestamps: bool,
) -> Result<OpsContainerLogsView, ContainerProbeError> {
    let container = container.trim();
    if container.is_empty() {
        return Err(ContainerProbeError::BadRequest(
            "容器 ID / 名称不能为空".to_string(),
        ));
    }
    let connector = docker_connector(connection_id).await?;
    let request = ContainerLogsRequest {
        lines,
        timestamps,
        max_bytes: None,
    };
    let logs = connector
        .container_logs(container, &request)
        .await
        .map_err(map_ops_error)?;
    Ok(OpsContainerLogsView {
        container: logs.container,
        lines: logs
            .lines
            .into_iter()
            .map(|line| OpsContainerLogLine {
                stream: line.stream.as_str().to_string(),
                message: line.message,
            })
            .collect(),
        bytes: logs.bytes,
        truncated: logs.truncated,
    })
}

/// 拉取 Docker 容器性能快照
///
/// # Errors
///
/// 连接 / 容器不存在、容器未运行、参数错误或 daemon 超时。
pub async fn container_stats(
    connection_id: i64,
    container: &str,
    two_samples: bool,
) -> Result<OpsContainerStatsView, ContainerProbeError> {
    let container = container.trim();
    if container.is_empty() {
        return Err(ContainerProbeError::BadRequest(
            "容器 ID / 名称不能为空".to_string(),
        ));
    }
    let connector = docker_connector(connection_id).await?;
    let stats = connector
        .container_stats(container, two_samples)
        .await
        .map_err(map_ops_error)?;
    Ok(OpsContainerStatsView {
        container: stats.container,
        read_at_ms: stats.read_at_ms,
        cpu_percent: stats.cpu_percent,
        mem_used_bytes: stats.mem_used_bytes,
        mem_limit_bytes: stats.mem_limit_bytes,
        mem_percent: stats.mem_percent,
        net_rx_bytes: stats.net_rx_bytes,
        net_tx_bytes: stats.net_tx_bytes,
        pids: stats.pids,
        blkio_read_bytes: stats.blkio_read_bytes,
        blkio_write_bytes: stats.blkio_write_bytes,
    })
}

/// 拉取 K8s Pod 日志
///
/// # Errors
///
/// 连接 / Pod / 容器不存在、容器名不匹配、APIServer 超时或 API 错误。
pub async fn pod_logs(
    connection_id: i64,
    namespace: &str,
    pod: &str,
    container: Option<&str>,
    previous: bool,
    lines: Option<u32>,
    timestamps: bool,
) -> Result<OpsPodLogsView, ContainerProbeError> {
    let namespace = namespace.trim();
    let pod = pod.trim();
    if namespace.is_empty() || pod.is_empty() {
        return Err(ContainerProbeError::BadRequest(
            "命名空间与 Pod 名不能为空".to_string(),
        ));
    }
    let connector = k8s_connector(connection_id).await?;
    let containers = connector
        .pod_containers(namespace, pod)
        .await
        .map_err(map_ops_error)?;
    let requested = container.map(str::trim).filter(|value| !value.is_empty());
    if let Some(name) = requested
        && !containers.iter().any(|item| item == name)
    {
        return Err(ContainerProbeError::BadRequest(format!(
            "Pod {namespace}/{pod} 中不存在容器 {name}"
        )));
    }
    let selected = requested
        .map(ToString::to_string)
        .or_else(|| containers.first().cloned());
    let request = PodLogsRequest {
        namespace: namespace.to_string(),
        pod: pod.to_string(),
        container: selected.clone(),
        previous,
        lines: lines.unwrap_or(ops::k8s_probe::DEFAULT_LOG_LINES),
        timestamps,
        max_bytes: None,
    };
    let logs = connector.pod_logs(&request).await.map_err(map_ops_error)?;
    Ok(OpsPodLogsView {
        namespace: logs.namespace,
        pod: logs.pod,
        container: logs.container.or(selected),
        containers,
        previous: logs.previous,
        lines: logs.lines,
        bytes: logs.bytes,
        truncated: logs.truncated,
    })
}

/// 拉取 K8s Pod 指标（metrics-server；仅 CPU / 内存）
///
/// # Errors
///
/// 连接 / Pod 不存在、metrics-server 未安装或无权限、APIServer 超时。
pub async fn pod_metrics(
    connection_id: i64,
    namespace: &str,
    pod: &str,
) -> Result<OpsPodMetricsView, ContainerProbeError> {
    let namespace = namespace.trim();
    let pod = pod.trim();
    if namespace.is_empty() || pod.is_empty() {
        return Err(ContainerProbeError::BadRequest(
            "命名空间与 Pod 名不能为空".to_string(),
        ));
    }
    let connector = k8s_connector(connection_id).await?;
    let containers = connector
        .pod_containers(namespace, pod)
        .await
        .map_err(map_ops_error)?;
    let metrics = connector
        .pod_metrics(namespace, pod)
        .await
        .map_err(map_ops_error)?;
    Ok(OpsPodMetricsView {
        namespace: metrics.namespace,
        pod: metrics.pod,
        containers,
        window: metrics.window,
        timestamp: metrics.timestamp,
        container_metrics: metrics
            .containers
            .into_iter()
            .map(|item| OpsPodContainerMetrics {
                name: item.name,
                cpu_nanocores: item.cpu_nanocores,
                memory_bytes: item.memory_bytes,
            })
            .collect(),
        cpu_nanocores: metrics.cpu_nanocores,
        memory_bytes: metrics.memory_bytes,
    })
}

