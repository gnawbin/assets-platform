//! Docker / Swarm 只读连接器（bollard 实现）
//!
//! 覆盖两类来源：
//! - [`Provider::Docker`]：单机容器 / 镜像 / 网络 / 卷；
//! - [`Provider::Swarm`]：在单机基础上追加 Node / Service / Task（**只连 Manager**，
//!   Worker 上的 `/nodes` 会 403）。
//!
//! 只读纪律（方案 §6）：只用 `/_ping`、`/version`、`/info` 与 `list_*` 系列 GET 接口，
//! 不 exec、不启动/停止、不删除、不 prune。
//!
//! # bollard 0.21 的连接参数
//! `connect_with_*` 的超时是 `u64` **秒**，版本参数是 `&ClientVersion`；
//! 因此 [`ConnectionConfig::timeout_secs`] 与 [`client_version`] 负责把配置翻译成 bollard 的口径。

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use async_trait::async_trait;
use bollard::Docker;
use bollard::errors::Error as BollardError;
use bollard::models::{ContainerSummary, ImageSummary, Network, Node, Service, Task, Volume};
use bollard::query_parameters::{
    ListContainersOptionsBuilder, ListImagesOptionsBuilder, ListNetworksOptionsBuilder,
    ListNodesOptionsBuilder, ListServicesOptionsBuilder, ListTasksOptionsBuilder,
    ListVolumesOptionsBuilder,
};
use bollard::{API_DEFAULT_VERSION, ClientVersion};

use super::{AuthKind, ConnectionConfig, ConnectionHealth, Connector};
use crate::error::{FailureClass, OpsError, OpsResult};
use crate::model::{
    Provider, ResourceKey, ResourceKind, ResourceRecord, ResourceSnapshot, ResourceStatus,
};

/// Docker / Swarm 只读连接器
pub struct DockerConnector {
    docker: Docker,
    config: ConnectionConfig,
}

impl DockerConnector {
    /// 连接本机 Docker（Linux `/var/run/docker.sock` / Windows named pipe）
    pub fn local_default() -> OpsResult<Self> {
        Self::connect(ConnectionConfig::local_docker("本机 Docker"))
    }

    /// 连接本机 Swarm Manager（同一 socket，额外采集集群对象）
    pub fn local_swarm() -> OpsResult<Self> {
        Self::connect(ConnectionConfig::local_swarm("本机 Swarm"))
    }

    /// 按配置建立连接（bollard 懒连接，不发请求）
    pub fn connect(config: ConnectionConfig) -> OpsResult<Self> {
        config.validate()?;
        config.ensure_feature()?;
        let version = client_version(config.docker_api_version.as_deref())?;
        let timeout = config.timeout_secs();
        let docker = match &config.auth {
            AuthKind::LocalSocket => Docker::connect_with_local_defaults(),
            AuthKind::PlainTcp => Docker::connect_with_http(&config.endpoint, timeout, &version),
            AuthKind::MutualTls {
                ca_path,
                cert_path,
                key_path,
            } => Docker::connect_with_ssl(
                &config.endpoint,
                Path::new(key_path),
                Path::new(cert_path),
                Path::new(ca_path),
                timeout,
                &version,
            ),
            AuthKind::KubeconfigFile { .. } | AuthKind::KubeconfigInline { .. } => {
                return Err(OpsError::Unsupported("Docker 不支持 kubeconfig 认证"));
            }
            AuthKind::Direct { .. } => {
                return Err(OpsError::Unsupported("Docker 不支持 K8s 直连认证"));
            }
        }
        .map_err(map_ctor_error)?;
        Ok(Self { docker, config })
    }

    /// 用已有的 [`Docker`] 构造（测试注入假 transport 时使用）
    pub fn with_docker(docker: Docker, config: ConnectionConfig) -> OpsResult<Self> {
        config.validate()?;
        Ok(Self { docker, config })
    }

    /// 底层 bollard 客户端
    pub fn docker(&self) -> &Docker {
        &self.docker
    }
}

/// 把配置里的 API 版本字符串翻译成 bollard 的 [`ClientVersion`]
pub fn client_version(raw: Option<&str>) -> OpsResult<ClientVersion> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(ClientVersion {
            major_version: API_DEFAULT_VERSION.major_version,
            minor_version: API_DEFAULT_VERSION.minor_version,
        });
    };
    let cleaned = raw.trim_start_matches(['v', 'V']);
    let mut parts = cleaned.split('.');
    let major = parts.next().unwrap_or_default();
    let minor = parts.next().unwrap_or_default();
    let parse = |value: &str, label: &str| {
        value
            .parse::<usize>()
            .map_err(|_| OpsError::Config(format!("Docker API 版本非法：{raw}（{label}不是数字）")))
    };
    Ok(ClientVersion {
        major_version: parse(major, "主版本")?,
        minor_version: parse(minor, "次版本")?,
    })
}

/// 构造期错误（socket 不存在、证书读不到 ...）
fn map_ctor_error(err: BollardError) -> OpsError {
    if let BollardError::SocketNotFoundError(path) = &err {
        return OpsError::Connect(format!(
            "Docker socket 不存在：{path}（Docker Desktop / dockerd 是否已启动？）"
        ));
    }
    classify_transport(err.to_string())
}

/// 请求期错误（401/403/超时/JSON 解析 ...）
fn map_request_error(action: &str, err: BollardError) -> OpsError {
    if let BollardError::DockerResponseServerError {
        status_code,
        message,
    } = &err
    {
        return match status_code {
            401 => OpsError::Auth(message.clone()),
            403 => OpsError::Forbidden(message.clone()),
            _ => OpsError::Api(format!("{action} 失败：HTTP {status_code} {message}")),
        };
    }
    let message = format!("{action} 失败：{err}");
    let lowered = message.to_ascii_lowercase();
    if lowered.contains("json") || lowered.contains("deserialize") {
        return OpsError::Parse(message);
    }
    classify_transport(message)
}

/// 按消息关键字归类传输层错误（TLS / 超时 / 其它网络问题）
fn classify_transport(message: String) -> OpsError {
    let lowered = message.to_ascii_lowercase();
    if lowered.contains("certificate")
        || lowered.contains("tls")
        || lowered.contains("ssl")
        || lowered.contains("unknown issuer")
    {
        return OpsError::Tls(message);
    }
    if lowered.contains("timed out") || lowered.contains("timeout") {
        return OpsError::Timeout(std::time::Duration::from_secs(1));
    }
    OpsError::Connect(message)
}

/// 毫秒耗时（溢出时饱和）
fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// 采集单类资源时遇到非致命错误：认证/权限/配置/TLS 直接失败，其余记为部分失败
fn absorb_kind_error(
    snapshot: &mut ResourceSnapshot,
    action: &str,
    err: BollardError,
) -> OpsResult<()> {
    let mapped = map_request_error(action, err);
    if matches!(
        mapped.classify(),
        FailureClass::Auth | FailureClass::Forbidden | FailureClass::Config | FailureClass::Tls
    ) {
        return Err(mapped);
    }
    tracing::warn!(action, error = %mapped, "Docker 采集部分失败");
    snapshot.push_error(format!("{action}：{mapped}"));
    Ok(())
}

/// 超限时记录部分失败（不静默截断）
fn absorb_truncation(
    snapshot: &mut ResourceSnapshot,
    kind: ResourceKind,
    total: usize,
    max_items: usize,
) {
    if total > max_items {
        snapshot.push_error(format!(
            "{} 共 {total} 条，仅保留前 {max_items} 条（可调大 max_items / OPS_MAX_ITEMS）",
            kind.label()
        ));
    }
}

/// `HashMap` → `BTreeMap`（保证 JSON 输出稳定，便于 diff）
fn to_sorted_map(labels: &std::collections::HashMap<String, String>) -> BTreeMap<String, String> {
    labels
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// 可选标签转有序 map
fn to_sorted_map_opt(
    labels: &Option<std::collections::HashMap<String, String>>,
) -> BTreeMap<String, String> {
    labels.as_ref().map(to_sorted_map).unwrap_or_default()
}

/// bollard 的时间戳（`time` 特性下是 `OffsetDateTime`）→ epoch 毫秒
fn date_millis(value: &time::OffsetDateTime) -> i64 {
    value.unix_timestamp().saturating_mul(1000)
}

/// 短 ID（取前 12 位，镜像/容器展示用）
fn short_id(id: &str) -> String {
    let trimmed = id.trim_start_matches("sha256:");
    trimmed.chars().take(12).collect()
}

/// epoch 秒 → epoch 毫秒
fn secs_to_millis(secs: i64) -> Option<i64> {
    (secs > 0).then(|| secs.saturating_mul(1000))
}

#[async_trait]
impl Connector for DockerConnector {
    fn config(&self) -> &ConnectionConfig {
        &self.config
    }

    async fn health(&self) -> OpsResult<ConnectionHealth> {
        let started = Instant::now();
        let mut health = ConnectionHealth::new(self.config.provider, self.config.endpoint.clone());

        if let Err(err) = self.docker.ping().await {
            return Ok(
                health.with_failure(&map_request_error("GET /_ping", err), elapsed_ms(started))
            );
        }
        health.authenticated = true;

        match self.docker.version().await {
            Ok(version) => {
                health.server_version = version.version;
                health.api_version = version.api_version;
            }
            Err(err) => {
                return Ok(health
                    .with_failure(&map_request_error("GET /version", err), elapsed_ms(started)));
            }
        }

        // 只读权限探测：容器列表是采集主链路，权限不足必须立刻暴露
        let probe = self
            .docker
            .list_containers(Some(ListContainersOptionsBuilder::new().limit(1).build()))
            .await;
        match probe {
            Ok(_) => health.authorized = true,
            Err(err) => {
                let mapped = map_request_error("GET /containers/json", err);
                if matches!(
                    mapped.classify(),
                    FailureClass::Auth | FailureClass::Forbidden
                ) {
                    return Ok(health.with_failure(&mapped, elapsed_ms(started)));
                }
                health.warn(format!("容器列表探测异常：{mapped}"));
            }
        }

        if self.config.provider == Provider::Swarm {
            match self.docker.info().await {
                Ok(info) => {
                    if info.swarm.is_none() {
                        health.warn("目标未启用 Swarm，无法采集 Node / Service / Task");
                    }
                }
                Err(err) => {
                    let mapped = map_request_error("GET /info", err);
                    health.warn(format!("{mapped}"));
                }
            }
        }

        health.ok = health.authorized;
        health.latency_ms = elapsed_ms(started);
        Ok(health)
    }

    async fn collect(&self) -> OpsResult<ResourceSnapshot> {
        let started = Instant::now();
        let endpoint = self.config.endpoint_key();
        let mut snapshot =
            ResourceSnapshot::new(self.config.provider, self.config.endpoint.clone(), 0);

        self.collect_containers(&endpoint, &mut snapshot).await?;
        self.collect_images(&endpoint, &mut snapshot).await?;
        self.collect_networks(&endpoint, &mut snapshot).await?;
        self.collect_volumes(&endpoint, &mut snapshot).await?;

        // Swarm 对象只在 Manager 上可读；Worker 会返回 403（统一走部分失败处理）
        if self.config.provider == Provider::Swarm {
            self.collect_swarm_nodes(&endpoint, &mut snapshot).await?;
            self.collect_swarm_services(&endpoint, &mut snapshot)
                .await?;
            self.collect_swarm_tasks(&endpoint, &mut snapshot).await?;
        }

        snapshot.elapsed_ms = elapsed_ms(started);
        tracing::debug!(
            provider = %snapshot.provider,
            total = snapshot.resources.len(),
            partial_errors = snapshot.partial_errors.len(),
            "Docker 采集完成"
        );
        Ok(snapshot)
    }
}

impl DockerConnector {
    async fn collect_containers(
        &self,
        endpoint: &str,
        snapshot: &mut ResourceSnapshot,
    ) -> OpsResult<()> {
        let options = ListContainersOptionsBuilder::new().all(true).build();
        let containers = match self.docker.list_containers(Some(options)).await {
            Ok(containers) => containers,
            Err(err) => return absorb_kind_error(snapshot, "GET /containers/json", err),
        };
        let total = containers.len();
        for summary in containers.into_iter().take(self.config.max_items) {
            if let Some(record) = container_record(endpoint, summary) {
                snapshot.push(record);
            }
        }
        absorb_truncation(
            snapshot,
            ResourceKind::Container,
            total,
            self.config.max_items,
        );
        Ok(())
    }

    async fn collect_images(
        &self,
        endpoint: &str,
        snapshot: &mut ResourceSnapshot,
    ) -> OpsResult<()> {
        let options = ListImagesOptionsBuilder::new().all(true).build();
        let images = match self.docker.list_images(Some(options)).await {
            Ok(images) => images,
            Err(err) => return absorb_kind_error(snapshot, "GET /images/json", err),
        };
        let total = images.len();
        for summary in images.into_iter().take(self.config.max_items) {
            snapshot.push(image_record(endpoint, summary));
        }
        absorb_truncation(snapshot, ResourceKind::Image, total, self.config.max_items);
        Ok(())
    }

    async fn collect_networks(
        &self,
        endpoint: &str,
        snapshot: &mut ResourceSnapshot,
    ) -> OpsResult<()> {
        let options = ListNetworksOptionsBuilder::new().build();
        let networks = match self.docker.list_networks(Some(options)).await {
            Ok(networks) => networks,
            Err(err) => return absorb_kind_error(snapshot, "GET /networks", err),
        };
        let total = networks.len();
        for network in networks.into_iter().take(self.config.max_items) {
            if let Some(record) = network_record(endpoint, network) {
                snapshot.push(record);
            }
        }
        absorb_truncation(
            snapshot,
            ResourceKind::Network,
            total,
            self.config.max_items,
        );
        Ok(())
    }

    async fn collect_volumes(
        &self,
        endpoint: &str,
        snapshot: &mut ResourceSnapshot,
    ) -> OpsResult<()> {
        let options = ListVolumesOptionsBuilder::new().build();
        let response = match self.docker.list_volumes(Some(options)).await {
            Ok(response) => response,
            Err(err) => return absorb_kind_error(snapshot, "GET /volumes", err),
        };
        // Docker 把「卷驱动告警」放在 200 响应的 Warnings 里，必须转成部分失败
        for warning in response.warnings.unwrap_or_default() {
            snapshot.push_error(format!("Docker 卷告警：{warning}"));
        }
        let volumes = response.volumes.unwrap_or_default();
        let total = volumes.len();
        for volume in volumes.into_iter().take(self.config.max_items) {
            snapshot.push(volume_record(endpoint, volume));
        }
        absorb_truncation(snapshot, ResourceKind::Volume, total, self.config.max_items);
        Ok(())
    }

    async fn collect_swarm_nodes(
        &self,
        endpoint: &str,
        snapshot: &mut ResourceSnapshot,
    ) -> OpsResult<()> {
        let options = ListNodesOptionsBuilder::new().build();
        let nodes = match self.docker.list_nodes(Some(options)).await {
            Ok(nodes) => nodes,
            Err(err) => return absorb_kind_error(snapshot, "GET /nodes", err),
        };
        let total = nodes.len();
        for node in nodes.into_iter().take(self.config.max_items) {
            if let Some(record) = node_record(endpoint, node) {
                snapshot.push(record);
            }
        }
        absorb_truncation(
            snapshot,
            ResourceKind::SwarmNode,
            total,
            self.config.max_items,
        );
        Ok(())
    }

    async fn collect_swarm_services(
        &self,
        endpoint: &str,
        snapshot: &mut ResourceSnapshot,
    ) -> OpsResult<()> {
        // status=true 才会返回 running / desired tasks 计数，用于判定服务健康度
        let options = ListServicesOptionsBuilder::new().status(true).build();
        let services = match self.docker.list_services(Some(options)).await {
            Ok(services) => services,
            Err(err) => return absorb_kind_error(snapshot, "GET /services", err),
        };
        let total = services.len();
        for service in services.into_iter().take(self.config.max_items) {
            if let Some(record) = service_record(endpoint, service) {
                snapshot.push(record);
            }
        }
        absorb_truncation(
            snapshot,
            ResourceKind::SwarmService,
            total,
            self.config.max_items,
        );
        Ok(())
    }

    async fn collect_swarm_tasks(
        &self,
        endpoint: &str,
        snapshot: &mut ResourceSnapshot,
    ) -> OpsResult<()> {
        let options = ListTasksOptionsBuilder::new().build();
        let tasks = match self.docker.list_tasks(Some(options)).await {
            Ok(tasks) => tasks,
            Err(err) => return absorb_kind_error(snapshot, "GET /tasks", err),
        };
        let total = tasks.len();
        for task in tasks.into_iter().take(self.config.max_items) {
            if let Some(record) = task_record(endpoint, task) {
                snapshot.push(record);
            }
        }
        absorb_truncation(
            snapshot,
            ResourceKind::SwarmTask,
            total,
            self.config.max_items,
        );
        Ok(())
    }
}

/// 容器状态归一化
///
/// 停用（exited/dead/stopping）视作「已下线」：资源仍在，但不对外提供服务，
/// 台账保留记录、不物理删除（方案 §2.3）。中间态（created/paused/restarting）无法判定业务状态。
fn container_status(raw_state: Option<&str>) -> ResourceStatus {
    match raw_state {
        Some("running") => ResourceStatus::Running,
        Some("exited" | "dead" | "stopping") => ResourceStatus::Offline,
        Some("removing") => ResourceStatus::PendingOffline,
        _ => ResourceStatus::Unknown,
    }
}

/// 容器 → 资源记录（缺 ID 时丢弃，避免生成假 uid）
fn container_record(endpoint: &str, summary: ContainerSummary) -> Option<ResourceRecord> {
    let id = summary.id.clone().filter(|id| !id.is_empty())?;
    let display_name = summary
        .names
        .as_ref()
        .and_then(|names| names.first())
        .map(|name| name.trim_start_matches('/').to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| short_id(&id));
    let raw_state = summary
        .state
        .map(|state| state.to_string())
        .filter(|state| !state.is_empty());
    let status = container_status(raw_state.as_deref());
    let mut record = ResourceRecord::new(
        ResourceKey::new(Provider::Docker, endpoint, ResourceKind::Container, id),
        display_name,
        status,
    );
    record.raw_state = raw_state;
    record.image = summary.image.clone();
    record.created_at = summary.created.and_then(secs_to_millis);
    record.labels = to_sorted_map_opt(&summary.labels);
    record.attributes = serde_json::json!({
        "image_id": summary.image_id,
        "command": summary.command,
        "human_status": summary.status,
        "port_count": summary.ports.as_ref().map(Vec::len),
        "mount_count": summary.mounts.as_ref().map(Vec::len),
        "has_healthcheck": summary.health.is_some(),
    });
    Some(record)
}

/// 镜像 → 资源记录（镜像无生命周期状态，恒为「正常运行」）
fn image_record(endpoint: &str, summary: ImageSummary) -> ResourceRecord {
    let display_name = summary
        .repo_tags
        .first()
        .cloned()
        .filter(|tag| !tag.is_empty())
        .unwrap_or_else(|| short_id(&summary.id));
    let mut record = ResourceRecord::new(
        ResourceKey::new(
            Provider::Docker,
            endpoint,
            ResourceKind::Image,
            summary.id.clone(),
        ),
        display_name,
        ResourceStatus::Running,
    );
    record.created_at = secs_to_millis(summary.created);
    record.labels = to_sorted_map(&summary.labels);
    record.attributes = serde_json::json!({
        "size": summary.size,
        "containers": summary.containers,
        "repo_tags": summary.repo_tags,
        "repo_digests": summary.repo_digests,
        "parent_id": summary.parent_id,
    });
    record
}

/// 网络 → 资源记录
fn network_record(endpoint: &str, network: Network) -> Option<ResourceRecord> {
    let id = network.id.clone().filter(|id| !id.is_empty())?;
    let display_name = network
        .name
        .clone()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| short_id(&id));
    let mut record = ResourceRecord::new(
        ResourceKey::new(Provider::Docker, endpoint, ResourceKind::Network, id),
        display_name,
        ResourceStatus::Running,
    );
    record.created_at = network.created.as_ref().map(date_millis);
    record.labels = to_sorted_map_opt(&network.labels);
    record.attributes = serde_json::json!({
        "driver": network.driver,
        "scope": network.scope,
        "internal": network.internal,
        "attachable": network.attachable,
        "ingress": network.ingress,
    });
    Some(record)
}

/// 卷 → 资源记录
fn volume_record(endpoint: &str, volume: Volume) -> ResourceRecord {
    let mut record = ResourceRecord::new(
        ResourceKey::new(
            Provider::Docker,
            endpoint,
            ResourceKind::Volume,
            volume.name.clone(),
        ),
        volume.name.clone(),
        ResourceStatus::Running,
    );
    record.created_at = volume.created_at.as_ref().map(date_millis);
    record.labels = to_sorted_map(&volume.labels);
    record.attributes = serde_json::json!({
        "driver": volume.driver,
        "mountpoint": volume.mountpoint,
        "scope": volume.scope.map(|scope| scope.to_string()),
    });
    record
}

/// Swarm 节点状态归一化
fn node_status(raw_state: Option<&str>) -> ResourceStatus {
    match raw_state {
        Some("ready") => ResourceStatus::Running,
        Some("down") => ResourceStatus::Offline,
        _ => ResourceStatus::Unknown,
    }
}

/// 服务健康度：副本齐 → 正常；全灭 → 已下线；部分可用 → 无法判定（原文保留 running/desired）
fn service_status(running: u64, desired: u64) -> ResourceStatus {
    if desired == 0 {
        ResourceStatus::Unknown
    } else if running >= desired {
        ResourceStatus::Running
    } else if running == 0 {
        ResourceStatus::Offline
    } else {
        ResourceStatus::Unknown
    }
}

/// Swarm Task 状态归一化（Task 属短时动态资产）
fn task_status(raw_state: Option<&str>) -> ResourceStatus {
    match raw_state {
        Some("running") => ResourceStatus::Running,
        Some("complete" | "failed" | "rejected" | "shutdown" | "remove" | "orphaned") => {
            ResourceStatus::Offline
        }
        _ => ResourceStatus::Unknown,
    }
}

/// Swarm 节点 → 资源记录（`role=manager` 即集群控制器）
fn node_record(endpoint: &str, node: Node) -> Option<ResourceRecord> {
    let id = node.id.clone().filter(|id| !id.is_empty())?;
    let display_name = node
        .spec
        .as_ref()
        .and_then(|spec| spec.name.clone())
        .or_else(|| {
            node.description
                .as_ref()
                .and_then(|description| description.hostname.clone())
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| short_id(&id));
    let raw_state = node
        .status
        .as_ref()
        .and_then(|status| status.state)
        .map(|state| state.to_string());
    let status = node_status(raw_state.as_deref());
    let labels = node.spec.as_ref().and_then(|spec| spec.labels.clone());
    let role = node
        .spec
        .as_ref()
        .and_then(|spec| spec.role)
        .map(|role| role.to_string());
    let availability = node
        .spec
        .as_ref()
        .and_then(|spec| spec.availability)
        .map(|availability| availability.to_string());
    let addr = node.status.as_ref().and_then(|status| status.addr.clone());

    let mut record = ResourceRecord::new(
        ResourceKey::new(Provider::Swarm, endpoint, ResourceKind::SwarmNode, id),
        display_name,
        status,
    );
    record.raw_state = raw_state;
    record.created_at = node.created_at.as_ref().map(date_millis);
    record.labels = to_sorted_map_opt(&labels);
    record.attributes = serde_json::json!({
        "role": role,
        "availability": availability,
        "addr": addr,
    });
    Some(record)
}

/// Swarm 服务 → 资源记录
fn service_record(endpoint: &str, service: Service) -> Option<ResourceRecord> {
    let id = service.id.clone().filter(|id| !id.is_empty())?;
    let display_name = service
        .spec
        .as_ref()
        .and_then(|spec| spec.name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| short_id(&id));
    let labels = service.spec.as_ref().and_then(|spec| spec.labels.clone());
    let running = service
        .service_status
        .as_ref()
        .and_then(|status| status.running_tasks)
        .unwrap_or(0);
    let desired = service
        .service_status
        .as_ref()
        .and_then(|status| status.desired_tasks)
        .unwrap_or(0);
    let completed = service
        .service_status
        .as_ref()
        .and_then(|status| status.completed_tasks);
    let status = service_status(running, desired);

    let mut record = ResourceRecord::new(
        ResourceKey::new(Provider::Swarm, endpoint, ResourceKind::SwarmService, id),
        display_name,
        status,
    );
    record.created_at = service.created_at.as_ref().map(date_millis);
    record.labels = to_sorted_map_opt(&labels);
    record.image = service
        .spec
        .as_ref()
        .and_then(|spec| spec.task_template.as_ref())
        .and_then(|template| template.container_spec.as_ref())
        .and_then(|container| container.image.clone());
    record.raw_state = Some(format!("{running}/{desired}"));
    record.attributes = serde_json::json!({
        "running_tasks": running,
        "desired_tasks": desired,
        "completed_tasks": completed,
    });
    Some(record)
}

/// Swarm Task → 资源记录（父资源指向所属 Service）
fn task_record(endpoint: &str, task: Task) -> Option<ResourceRecord> {
    let id = task.id.clone().filter(|id| !id.is_empty())?;
    let display_name = task
        .name
        .clone()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| short_id(&id));
    let raw_state = task
        .status
        .as_ref()
        .and_then(|status| status.state)
        .map(|state| state.to_string());
    let status = task_status(raw_state.as_deref());

    let mut record = ResourceRecord::new(
        ResourceKey::new(Provider::Swarm, endpoint, ResourceKind::SwarmTask, id),
        display_name,
        status,
    );
    record.raw_state = raw_state;
    record.created_at = task.created_at.as_ref().map(date_millis);
    record.labels = to_sorted_map_opt(&task.labels);
    record.image = task
        .spec
        .as_ref()
        .and_then(|spec| spec.container_spec.as_ref())
        .and_then(|container| container.image.clone());
    if let Some(service_id) = task.service_id.as_ref().filter(|id| !id.is_empty()) {
        record.parent_uid = Some(
            ResourceKey::new(
                Provider::Swarm,
                endpoint,
                ResourceKind::SwarmService,
                service_id.clone(),
            )
            .uid(),
        );
    }
    let error = task.status.as_ref().and_then(|status| status.err.clone());
    let message = task
        .status
        .as_ref()
        .and_then(|status| status.message.clone());
    record.attributes = serde_json::json!({
        "node_id": task.node_id,
        "slot": task.slot,
        "desired_state": task.desired_state.map(|state| state.to_string()),
        "error": error,
        "message": message,
    });
    Some(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// 假 daemon 的单条响应
    #[derive(Clone)]
    struct FakeResponse {
        status: u16,
        content_type: &'static str,
        body: String,
    }

    impl FakeResponse {
        fn json(body: impl Into<String>) -> Self {
            Self {
                status: 200,
                content_type: "application/json",
                body: body.into(),
            }
        }

        fn text(body: impl Into<String>) -> Self {
            Self {
                status: 200,
                content_type: "text/plain",
                body: body.into(),
            }
        }

        fn error(status: u16, body: impl Into<String>) -> Self {
            Self {
                status,
                content_type: "application/json",
                body: body.into(),
            }
        }
    }

    fn reason(status: u16) -> &'static str {
        match status {
            200 => "OK",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            500 => "Internal Server Error",
            _ => "Unknown",
        }
    }

    /// 启动一个「按路径后缀匹配固定响应」的 HTTP/1.1 假 Docker daemon
    ///
    /// 只实现 bollard 用到的部分：读请求行 → 匹配 `/v1.x/...` 后缀 → 回一条 JSON/文本，
    /// 然后关闭连接（避免 hyper 复用已结束的连接）。
    async fn spawn_fake_docker(routes: Vec<(&'static str, FakeResponse)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("绑定端口失败");
        let addr = listener.local_addr().expect("读取本地地址失败");
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let routes = routes.clone();
                tokio::spawn(async move {
                    let mut buffer = Vec::new();
                    let mut chunk = [0_u8; 1024];
                    loop {
                        let read = socket.read(&mut chunk).await.unwrap_or(0);
                        if read == 0 {
                            return;
                        }
                        buffer.extend_from_slice(&chunk[..read]);
                        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let request = String::from_utf8_lossy(&buffer);
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let path = path.split('?').next().unwrap_or(path);
                    let response = routes
                        .iter()
                        .find(|(suffix, _)| path.ends_with(*suffix))
                        .map(|(_, response)| response.clone())
                        .unwrap_or_else(|| FakeResponse::error(404, r#"{"message":"no route"}"#));
                    let payload = response.body.as_bytes();
                    let head = format!(
                        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        response.status,
                        reason(response.status),
                        response.content_type,
                        payload.len()
                    );
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(payload).await;
                    let _ = socket.flush().await;
                });
            }
        });
        format!("tcp://{addr}")
    }

    fn version_route() -> (&'static str, FakeResponse) {
        (
            "/version",
            FakeResponse::json(
                r#"{"Version":"27.3.1","ApiVersion":"1.47","MinAPIVersion":"1.24","Os":"linux","Arch":"amd64"}"#,
            ),
        )
    }

    fn ping_route() -> (&'static str, FakeResponse) {
        ("/_ping", FakeResponse::text("OK"))
    }

    fn containers_route() -> (&'static str, FakeResponse) {
        (
            "/containers/json",
            FakeResponse::json(
                r#"[
                  {"Id":"c1aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","Names":["/web-1"],"Image":"nginx:1.27","ImageID":"sha256:img1","Command":"nginx","Created":1700000000,"Ports":[],"Labels":{"app":"web"},"State":"running","Status":"Up 2 hours"},
                  {"Id":"c2bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","Names":["/batch-1"],"Image":"busybox:1.36","Created":1700000100,"Labels":{},"State":"exited","Status":"Exited (0)"}
                ]"#,
            ),
        )
    }

    fn images_route() -> (&'static str, FakeResponse) {
        (
            "/images/json",
            FakeResponse::json(
                r#"[
                  {"Id":"sha256:img1aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","ParentId":"","RepoTags":["nginx:1.27"],"RepoDigests":[],"Created":1700000000,"Size":123456,"SharedSize":-1,"Labels":{"maintainer":"nginx"},"Containers":1}
                ]"#,
            ),
        )
    }

    fn networks_route() -> (&'static str, FakeResponse) {
        (
            "/networks",
            FakeResponse::json(
                r#"[
                  {"Name":"bridge","Id":"net1","Created":"2024-01-01T00:00:00Z","Scope":"local","Driver":"bridge","EnableIPv4":true,"EnableIPv6":false,"Internal":false,"Attachable":false,"Ingress":false,"ConfigOnly":false,"Options":{},"Labels":{}}
                ]"#,
            ),
        )
    }

    fn volumes_route() -> (&'static str, FakeResponse) {
        (
            "/volumes",
            FakeResponse::json(
                r#"{"Volumes":[{"Name":"data","Driver":"local","Mountpoint":"/var/lib/docker/volumes/data/_data","CreatedAt":"2024-01-01T00:00:00Z","Labels":{"tier":"db"},"Options":{},"Scope":"local"}],"Warnings":[]}"#,
            ),
        )
    }

    fn nodes_route() -> (&'static str, FakeResponse) {
        (
            "/nodes",
            FakeResponse::json(
                r#"[
                  {"ID":"node1","CreatedAt":"2024-01-01T00:00:00Z","Spec":{"Name":"manager-1","Labels":{"zone":"a"},"Role":"manager","Availability":"active"},"Description":{"Hostname":"m1"},"Status":{"State":"ready","Addr":"10.0.0.5"}}
                ]"#,
            ),
        )
    }

    fn services_route() -> (&'static str, FakeResponse) {
        (
            "/services",
            FakeResponse::json(
                r#"[
                  {"ID":"svc1","CreatedAt":"2024-01-01T00:00:00Z","Spec":{"Name":"web","Labels":{"app":"web"},"TaskTemplate":{"ContainerSpec":{"Image":"nginx:1.27"}}},"ServiceStatus":{"RunningTasks":1,"DesiredTasks":1,"CompletedTasks":0}}
                ]"#,
            ),
        )
    }

    fn tasks_route() -> (&'static str, FakeResponse) {
        (
            "/tasks",
            FakeResponse::json(
                r#"[
                  {"ID":"task1","CreatedAt":"2024-01-01T00:00:00Z","Name":"web.1","Labels":{},"Spec":{"ContainerSpec":{"Image":"nginx:1.27"}},"ServiceID":"svc1","NodeID":"node1","Slot":1,"Status":{"State":"running"},"DesiredState":"running"}
                ]"#,
            ),
        )
    }

    fn docker_routes() -> Vec<(&'static str, FakeResponse)> {
        vec![
            ping_route(),
            version_route(),
            containers_route(),
            images_route(),
            networks_route(),
            volumes_route(),
        ]
    }

    #[test]
    fn client_version_parses_config_values() {
        let default = client_version(None).expect("默认版本");
        assert_eq!(default.major_version, API_DEFAULT_VERSION.major_version);
        assert_eq!(default.minor_version, API_DEFAULT_VERSION.minor_version);

        let parsed = client_version(Some(" v1.43 ")).expect("解析失败");
        assert_eq!((parsed.major_version, parsed.minor_version), (1, 43));
        assert!(client_version(Some("1")).is_err(), "缺少次版本应报错");
        assert!(client_version(Some("abc.def")).is_err(), "非数字应报错");
    }

    #[tokio::test]
    async fn health_reports_server_versions() {
        let endpoint = spawn_fake_docker(docker_routes()).await;
        let connector =
            DockerConnector::connect(ConnectionConfig::docker_tcp("测试 Docker", &endpoint))
                .expect("连接器初始化失败");

        assert_eq!(connector.provider(), Provider::Docker);
        assert!(!connector.supports_watch());

        let health = connector.health().await.expect("探测失败");
        assert!(health.ok, "探测应成功: {health:?}");
        assert!(health.authenticated);
        assert!(health.authorized);
        assert_eq!(health.server_version.as_deref(), Some("27.3.1"));
        assert_eq!(health.api_version.as_deref(), Some("1.47"));
        assert_eq!(health.failure, FailureClass::None);
    }

    #[tokio::test]
    async fn unauthorized_ping_is_classified_as_auth() {
        let endpoint = spawn_fake_docker(vec![(
            "/_ping",
            FakeResponse::error(401, r#"{"message":"unauthenticated"}"#),
        )])
        .await;
        let connector =
            DockerConnector::connect(ConnectionConfig::docker_tcp("测试 Docker", &endpoint))
                .expect("连接器初始化失败");

        let health = connector.health().await.expect("探测不应返回 Err");
        assert!(!health.ok);
        assert_eq!(health.failure, FailureClass::Auth);
        assert_eq!(health.failure.code(), "auth");
        assert!(
            health
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("unauthenticated"),
            "错误原文应保留服务端消息: {:?}",
            health.error
        );
    }

    #[tokio::test]
    async fn collect_maps_docker_resources() {
        let endpoint = spawn_fake_docker(docker_routes()).await;
        let connector =
            DockerConnector::connect(ConnectionConfig::docker_tcp("测试 Docker", &endpoint))
                .expect("连接器初始化失败");

        let snapshot = connector.collect().await.expect("采集失败");
        assert!(
            snapshot.is_complete(),
            "不应有部分失败: {:?}",
            snapshot.partial_errors
        );
        assert_eq!(snapshot.count_of(ResourceKind::Container), 2);
        assert_eq!(snapshot.count_of(ResourceKind::Image), 1);
        assert_eq!(snapshot.count_of(ResourceKind::Network), 1);
        assert_eq!(snapshot.count_of(ResourceKind::Volume), 1);
        assert_eq!(snapshot.short_lived_count(), 2);

        let web = snapshot
            .resources
            .iter()
            .find(|record| record.display_name == "web-1")
            .expect("缺少容器 web-1");
        assert_eq!(web.status, ResourceStatus::Running);
        assert_eq!(web.raw_state.as_deref(), Some("running"));
        assert_eq!(web.image.as_deref(), Some("nginx:1.27"));
        assert_eq!(web.labels.get("app").map(String::as_str), Some("web"));
        assert_eq!(web.created_at, Some(1_700_000_000_000));
        assert!(
            web.uid().starts_with("docker://127.0.0.1:"),
            "{}",
            web.uid()
        );
        assert!(web.is_short_lived());

        let stopped = snapshot
            .resources
            .iter()
            .find(|record| record.display_name == "batch-1")
            .expect("缺少容器 batch-1");
        assert_eq!(stopped.status, ResourceStatus::Offline);

        let image = snapshot
            .resources
            .iter()
            .find(|record| record.key.kind == ResourceKind::Image)
            .expect("缺少镜像");
        assert_eq!(image.display_name, "nginx:1.27");
        assert_eq!(image.created_at, Some(1_700_000_000_000));

        let network = snapshot
            .resources
            .iter()
            .find(|record| record.key.kind == ResourceKind::Network)
            .expect("缺少网络");
        assert_eq!(network.display_name, "bridge");
        assert_eq!(network.created_at, Some(1_704_067_200_000));

        let volume = snapshot
            .resources
            .iter()
            .find(|record| record.key.kind == ResourceKind::Volume)
            .expect("缺少卷");
        assert_eq!(volume.display_name, "data");
        assert_eq!(volume.labels.get("tier").map(String::as_str), Some("db"));
    }

    #[tokio::test]
    async fn collect_records_partial_failures() {
        let endpoint = spawn_fake_docker(vec![
            containers_route(),
            images_route(),
            networks_route(),
            (
                "/volumes",
                FakeResponse::error(500, r#"{"message":"driver error"}"#),
            ),
        ])
        .await;
        let connector =
            DockerConnector::connect(ConnectionConfig::docker_tcp("测试 Docker", &endpoint))
                .expect("连接器初始化失败");

        let snapshot = connector.collect().await.expect("单类失败不应导致整体失败");
        assert!(!snapshot.is_complete(), "卷采集失败必须记为部分失败");
        assert_eq!(snapshot.partial_errors.len(), 1);
        assert!(
            snapshot.partial_errors[0].contains("volumes"),
            "{:?}",
            snapshot.partial_errors
        );
        // 其余资源仍应完整采到（对账器据此只会「更新」，不会误判离线）
        assert_eq!(snapshot.count_of(ResourceKind::Container), 2);
        assert_eq!(snapshot.count_of(ResourceKind::Volume), 0);
    }

    #[tokio::test]
    async fn swarm_collect_links_tasks_to_services() {
        let endpoint = spawn_fake_docker(vec![
            containers_route(),
            images_route(),
            networks_route(),
            volumes_route(),
            nodes_route(),
            services_route(),
            tasks_route(),
        ])
        .await;
        let config = ConnectionConfig::swarm_tcp("测试 Swarm", &endpoint);
        let connector = DockerConnector::connect(config).expect("连接器初始化失败");

        let snapshot = connector.collect().await.expect("采集失败");
        assert!(
            snapshot.is_complete(),
            "不应有部分失败: {:?}",
            snapshot.partial_errors
        );
        assert_eq!(snapshot.provider, Provider::Swarm);
        assert_eq!(snapshot.count_of(ResourceKind::SwarmNode), 1);
        assert_eq!(snapshot.count_of(ResourceKind::SwarmService), 1);
        assert_eq!(snapshot.count_of(ResourceKind::SwarmTask), 1);

        let node = snapshot
            .resources
            .iter()
            .find(|record| record.key.kind == ResourceKind::SwarmNode)
            .expect("缺少 Swarm 节点");
        assert_eq!(node.display_name, "manager-1");
        assert_eq!(node.status, ResourceStatus::Running);
        assert_eq!(node.attributes["role"], serde_json::json!("manager"));

        let service = snapshot
            .resources
            .iter()
            .find(|record| record.key.kind == ResourceKind::SwarmService)
            .expect("缺少 Swarm 服务");
        assert_eq!(service.display_name, "web");
        assert_eq!(service.status, ResourceStatus::Running);
        assert_eq!(service.raw_state.as_deref(), Some("1/1"));
        assert_eq!(service.image.as_deref(), Some("nginx:1.27"));

        let task = snapshot
            .resources
            .iter()
            .find(|record| record.key.kind == ResourceKind::SwarmTask)
            .expect("缺少 Swarm 任务");
        assert_eq!(task.status, ResourceStatus::Running);
        assert!(task.is_short_lived());
        let expected_parent = format!(
            "swarm://{}/swarm_service/svc1",
            connector.config().endpoint_key()
        );
        assert_eq!(task.parent_uid.as_deref(), Some(expected_parent.as_str()));
    }
}
