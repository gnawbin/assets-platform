//! Kubernetes 只读连接器（kube-rs 实现）
//!
//! 只走 APIServer（方案 §6）：**绝不**直连节点 `docker.sock`，不 exec、不 delete/patch；
//! ConfigMap / Secret 只采元数据（键数量、类型），不读取任何 Data 内容。
//!
//! 认证入口（[`AuthKind`]）：
//! - `KubeconfigFile`：读取本机 / 挂载的 kubeconfig；
//! - `KubeconfigInline`：前端上传的 kubeconfig 内容；
//! - `Direct`：server + Token / 客户端证书，内部合成最小 kubeconfig 再交给 kube 解析
//!   （kube 3.x 的 `AuthInfo.token` 是 `SecretBox`，手工拼 `Config` 反而更易出错）。

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine as _;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::api::batch::v1::{CronJob, Job};
use k8s_openapi::api::core::v1::{
    ConfigMap, Namespace, Node, PersistentVolumeClaim, Pod, Secret, Service,
};
use k8s_openapi::api::networking::v1::Ingress;
use kube::api::{Api, ListParams};
use kube::config::{KubeConfigOptions, Kubeconfig};
use kube::{Client, Config, Resource};
use serde::de::DeserializeOwned;

use super::{AuthKind, ConnectionConfig, ConnectionHealth, Connector};
use crate::error::{FailureClass, OpsError, OpsResult};
use crate::model::{
    Provider, ResourceKey, ResourceKind, ResourceRecord, ResourceSnapshot, ResourceStatus,
};

/// Kubernetes 只读连接器
pub struct K8sConnector {
    client: Client,
    config: ConnectionConfig,
}

impl K8sConnector {
    /// 建立连接（解析 kubeconfig 后回填真实 `server` 到 `config.endpoint`）
    pub async fn connect(config: ConnectionConfig) -> OpsResult<Self> {
        if config.provider != Provider::Kubernetes {
            return Err(OpsError::Config(
                "K8s 连接器只接受 provider=k8s 的配置".to_string(),
            ));
        }
        config.validate()?;
        config.ensure_feature()?;
        let kube_config = build_config(&config).await?;
        let endpoint = kube_config.cluster_url.to_string();
        let client =
            Client::try_from(kube_config).map_err(|err| map_kube_error("创建客户端", err))?;
        let mut config = config;
        config.endpoint = endpoint;
        Ok(Self { client, config })
    }

    /// 用现成的 [`Client`] 构造（单测 / 复用已有连接时使用）
    pub fn with_client(client: Client, config: ConnectionConfig) -> Self {
        Self { client, config }
    }

    /// 底层 kube 客户端
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// 限定采集的命名空间
    pub fn namespace(&self) -> Option<&str> {
        self.config.namespace.as_deref()
    }
}

/// 依据认证方式构建 kube [`Config`]，并把超时预算写进去
async fn build_config(config: &ConnectionConfig) -> OpsResult<Config> {
    let mut kube_config = match &config.auth {
        AuthKind::KubeconfigFile { path, context } => {
            let yaml = tokio::fs::read_to_string(path).await.map_err(|err| {
                OpsError::Config(format!("读取 kubeconfig 失败（{path}）：{err}"))
            })?;
            let kubeconfig = Kubeconfig::from_yaml(&yaml).map_err(|err| {
                OpsError::Config(format!("解析 kubeconfig 失败（{path}）：{err}"))
            })?;
            from_kubeconfig(kubeconfig, context.clone()).await?
        }
        AuthKind::KubeconfigInline { yaml, context } => {
            let kubeconfig = Kubeconfig::from_yaml(yaml)
                .map_err(|err| OpsError::Config(format!("解析 kubeconfig 内容失败：{err}")))?;
            from_kubeconfig(kubeconfig, context.clone()).await?
        }
        AuthKind::Direct {
            token,
            ca_pem,
            client_cert_pem,
            client_key_pem,
            insecure_skip_tls_verify,
        } => {
            let yaml = direct_kubeconfig_yaml(
                &config.endpoint,
                token.as_deref(),
                ca_pem.as_deref(),
                client_cert_pem.as_deref(),
                client_key_pem.as_deref(),
                *insecure_skip_tls_verify,
                config.namespace.as_deref(),
            );
            let kubeconfig = Kubeconfig::from_yaml(&yaml)
                .map_err(|err| OpsError::Config(format!("内部 kubeconfig 生成失败：{err}")))?;
            from_kubeconfig(kubeconfig, None).await?
        }
        other => {
            return Err(OpsError::Config(format!(
                "Kubernetes 不支持 {} 认证方式",
                other.describe()
            )));
        }
    };
    kube_config.connect_timeout = Some(config.timeout);
    kube_config.read_timeout = Some(config.timeout);
    kube_config.write_timeout = Some(config.timeout);
    Ok(kube_config)
}

/// `Kubeconfig` → kube `Config`（指定 context 时按 context 取集群与用户）
async fn from_kubeconfig(kubeconfig: Kubeconfig, context: Option<String>) -> OpsResult<Config> {
    let options = KubeConfigOptions {
        context,
        cluster: None,
        user: None,
    };
    Config::from_custom_kubeconfig(kubeconfig, &options)
        .await
        .map_err(|err| OpsError::Config(format!("kubeconfig 无法用于连接：{err}")))
}

/// 合成「直连」用的最小 kubeconfig
///
/// Token / 客户端证书都通过 `*-data`（base64）内联，不落盘、不写临时文件。
/// `insecure_skip_tls_verify=true` 时不写 CA，交给 rustls 跳过校验（界面需审计提示）。
fn direct_kubeconfig_yaml(
    server: &str,
    token: Option<&str>,
    ca_pem: Option<&str>,
    client_cert_pem: Option<&str>,
    client_key_pem: Option<&str>,
    insecure_skip_tls_verify: bool,
    namespace: Option<&str>,
) -> String {
    const NAME: &str = "ops-collector";
    let mut yaml = String::new();
    yaml.push_str("apiVersion: v1\nkind: Config\ncurrent-context: ");
    yaml.push_str(NAME);
    yaml.push_str("\nclusters:\n  - name: ");
    yaml.push_str(NAME);
    yaml.push_str("\n    cluster:\n      server: \"");
    yaml.push_str(&yaml_escape(server));
    yaml.push_str("\"\n");
    if insecure_skip_tls_verify {
        yaml.push_str("      insecure-skip-tls-verify: true\n");
    } else if let Some(ca) = ca_pem.filter(|value| !value.trim().is_empty()) {
        yaml.push_str("      certificate-authority-data: ");
        yaml.push_str(&base64_pem(ca));
        yaml.push('\n');
    }

    yaml.push_str("users:\n  - name: ");
    yaml.push_str(NAME);
    yaml.push_str("\n    user:");
    let mut has_credential = false;
    if let Some(token) = token.filter(|value| !value.trim().is_empty()) {
        yaml.push_str("\n      token: \"");
        yaml.push_str(&yaml_escape(token));
        yaml.push_str("\"\n");
        has_credential = true;
    }
    let cert = client_cert_pem.filter(|value| !value.trim().is_empty());
    let key = client_key_pem.filter(|value| !value.trim().is_empty());
    if let (Some(cert), Some(key)) = (cert, key) {
        yaml.push_str("\n      client-certificate-data: ");
        yaml.push_str(&base64_pem(cert));
        yaml.push_str("\n      client-key-data: ");
        yaml.push_str(&base64_pem(key));
        yaml.push('\n');
        has_credential = true;
    }
    if !has_credential {
        // 空映射：交给 kube 走匿名认证（随后会被 API Server 判 401）
        yaml.push_str(" {}\n");
    }

    yaml.push_str("contexts:\n  - name: ");
    yaml.push_str(NAME);
    yaml.push_str("\n    context:\n      cluster: ");
    yaml.push_str(NAME);
    yaml.push_str("\n      user: ");
    yaml.push_str(NAME);
    yaml.push('\n');
    if let Some(namespace) = namespace.filter(|value| !value.trim().is_empty()) {
        yaml.push_str("      namespace: \"");
        yaml.push_str(&yaml_escape(namespace));
        yaml.push_str("\"\n");
    }
    yaml
}

/// YAML 双引号字符串转义（只处理反斜杠与双引号，凭证里不会出现控制字符）
fn yaml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// PEM 原文 → base64（kubeconfig 的 `*-data` 字段格式）
fn base64_pem(pem: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(pem.as_bytes())
}

/// kube 错误 → 统一错误（401/403 必须单独归类，界面提示完全不同）
fn map_kube_error(action: &str, err: kube::Error) -> OpsError {
    match err {
        kube::Error::Api(status) => match status.code {
            401 => OpsError::Auth(status.message),
            403 => OpsError::Forbidden(status.message),
            code => OpsError::Api(format!("{action} 失败：HTTP {code} {}", status.message)),
        },
        kube::Error::InferConfig(err) => OpsError::Config(format!("kubeconfig 推断失败：{err}")),
        kube::Error::InferKubeconfig(err) => {
            OpsError::Config(format!("kubeconfig 解析失败：{err}"))
        }
        other => classify_kube_transport(format!("{action} 失败：{other}")),
    }
}

/// 按消息关键字归类 kube 传输层错误
fn classify_kube_transport(message: String) -> OpsError {
    let lowered = message.to_ascii_lowercase();
    if lowered.contains("certificate")
        || lowered.contains("tls")
        || lowered.contains("x509")
        || lowered.contains("rustls")
    {
        return OpsError::Tls(message);
    }
    if lowered.contains("timed out") || lowered.contains("timeout") {
        return OpsError::Timeout(Duration::from_secs(1));
    }
    if lowered.contains("serialize") || lowered.contains("deserialize") || lowered.contains("json")
    {
        return OpsError::Parse(message);
    }
    OpsError::Connect(message)
}

/// 单类资源失败时：认证/权限/TLS/配置直接失败，其余记为部分失败（不得据此判定离线）
fn absorb_kind_error(
    snapshot: &mut ResourceSnapshot,
    action: &str,
    err: kube::Error,
) -> OpsResult<()> {
    let mapped = map_kube_error(action, err);
    if matches!(
        mapped.classify(),
        FailureClass::Auth | FailureClass::Forbidden | FailureClass::Config | FailureClass::Tls
    ) {
        return Err(mapped);
    }
    tracing::warn!(action, error = %mapped, "K8s 采集部分失败");
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
            "{} 共 {total} 个，仅保留前 {max_items} 个（可调大 max_items / OPS_MAX_ITEMS）",
            kind.label()
        ));
    }
}

/// 毫秒耗时（溢出时饱和）
fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Pod 阶段 → 状态
fn pod_status(phase: Option<&str>) -> ResourceStatus {
    match phase {
        Some("Running") => ResourceStatus::Running,
        Some("Succeeded" | "Failed") => ResourceStatus::Offline,
        _ => ResourceStatus::Unknown,
    }
}

/// 工作负载副本健康度（Deployment / StatefulSet / DaemonSet 共用）
fn workload_status(desired: i32, ready: i32) -> ResourceStatus {
    if desired <= 0 {
        ResourceStatus::Unknown
    } else if ready >= desired {
        ResourceStatus::Running
    } else if ready == 0 {
        ResourceStatus::Offline
    } else {
        ResourceStatus::Unknown
    }
}

/// Job 健康度：有活跃 Pod → 正常；有成功记录 → 已下线（完成即退场）；其余无法判定
fn job_status(active: i32, succeeded: i32, failed: i32) -> ResourceStatus {
    if active > 0 {
        ResourceStatus::Running
    } else if succeeded > 0 || failed > 0 {
        ResourceStatus::Offline
    } else {
        ResourceStatus::Unknown
    }
}

/// PVC 阶段 → 状态
fn pvc_status(phase: Option<&str>) -> ResourceStatus {
    match phase {
        Some("Bound") => ResourceStatus::Running,
        Some("Lost") => ResourceStatus::Offline,
        _ => ResourceStatus::Unknown,
    }
}

/// 命名空间阶段 → 状态
fn namespace_status(phase: Option<&str>) -> ResourceStatus {
    match phase {
        Some("Active") => ResourceStatus::Running,
        Some("Terminating") => ResourceStatus::PendingOffline,
        _ => ResourceStatus::Unknown,
    }
}

#[async_trait]
impl Connector for K8sConnector {
    fn config(&self) -> &ConnectionConfig {
        &self.config
    }

    /// 本连接是否支持 watch（本轮统一 false，调度器仍走轮询）
    fn supports_watch(&self) -> bool {
        false
    }

    async fn health(&self) -> OpsResult<ConnectionHealth> {
        let started = Instant::now();
        let mut health = ConnectionHealth::new(Provider::Kubernetes, self.config.endpoint.clone());

        match self.client.apiserver_version().await {
            Ok(info) => {
                health.server_version =
                    Some(info.git_version.clone()).filter(|value| !value.is_empty());
                let major = info.major.trim();
                let minor = info.minor.trim();
                if !major.is_empty() || !minor.is_empty() {
                    health.api_version = Some(format!("{major}.{minor}"));
                }
                health.authenticated = true;
            }
            Err(err) => {
                return Ok(
                    health.with_failure(&map_kube_error("GET /version", err), elapsed_ms(started))
                );
            }
        }

        // 权限探测：配置了命名空间就探命名空间内 Pod（RBAC 可能只给单命名空间只读）
        let authorized = match self.namespace() {
            Some(namespace) => {
                let api: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
                api.list(&ListParams::default().limit(1))
                    .await
                    .map(|_| ())
                    .map_err(|err| map_kube_error("GET pods", err))
            }
            None => {
                let api: Api<Namespace> = Api::all(self.client.clone());
                api.list(&ListParams::default().limit(1))
                    .await
                    .map(|_| ())
                    .map_err(|err| map_kube_error("GET /api/v1/namespaces", err))
            }
        };
        match authorized {
            Ok(()) => health.authorized = true,
            Err(mapped)
                if matches!(
                    mapped.classify(),
                    FailureClass::Auth | FailureClass::Forbidden
                ) =>
            {
                return Ok(health.with_failure(&mapped, elapsed_ms(started)));
            }
            Err(mapped) => health.warn(format!("{mapped}")),
        }

        health.ok = health.authorized;
        health.latency_ms = elapsed_ms(started);
        Ok(health)
    }

    async fn collect(&self) -> OpsResult<ResourceSnapshot> {
        let started = Instant::now();
        let endpoint = self.config.endpoint_key();
        let mut snapshot =
            ResourceSnapshot::new(Provider::Kubernetes, self.config.endpoint.clone(), 0);
        let namespace = self.namespace().map(str::to_string);

        // 集群级资源只在「全集群采集」时拉取：命名空间受限的凭证没有这些权限
        if namespace.is_none() {
            let api: Api<Namespace> = Api::all(self.client.clone());
            self.collect_kind(
                api,
                ResourceKind::Namespace,
                &endpoint,
                &mut snapshot,
                decorate_namespace,
            )
            .await?;
            let api: Api<Node> = Api::all(self.client.clone());
            self.collect_kind(
                api,
                ResourceKind::ClusterNode,
                &endpoint,
                &mut snapshot,
                decorate_node,
            )
            .await?;
        }

        macro_rules! collect_scoped {
            ($ty:ty, $kind:expr, $decorate:path) => {{
                let api: Api<$ty> = match &namespace {
                    Some(namespace) => Api::namespaced(self.client.clone(), namespace.as_str()),
                    None => Api::all(self.client.clone()),
                };
                self.collect_kind(api, $kind, &endpoint, &mut snapshot, $decorate)
                    .await?;
            }};
        }

        collect_scoped!(Deployment, ResourceKind::Deployment, decorate_deployment);
        collect_scoped!(StatefulSet, ResourceKind::StatefulSet, decorate_statefulset);
        collect_scoped!(DaemonSet, ResourceKind::DaemonSet, decorate_daemonset);
        collect_scoped!(CronJob, ResourceKind::CronJob, decorate_cronjob);
        collect_scoped!(Job, ResourceKind::Job, decorate_job);
        collect_scoped!(Pod, ResourceKind::Pod, decorate_pod);
        collect_scoped!(Service, ResourceKind::KubeService, decorate_service);
        collect_scoped!(Ingress, ResourceKind::Ingress, decorate_ingress);
        collect_scoped!(
            PersistentVolumeClaim,
            ResourceKind::PersistentVolumeClaim,
            decorate_pvc
        );
        collect_scoped!(ConfigMap, ResourceKind::ConfigMap, decorate_configmap);
        collect_scoped!(Secret, ResourceKind::Secret, decorate_secret);

        snapshot.elapsed_ms = elapsed_ms(started);
        tracing::debug!(
            total = snapshot.resources.len(),
            partial_errors = snapshot.partial_errors.len(),
            "K8s 采集完成"
        );
        Ok(snapshot)
    }
}

impl K8sConnector {
    /// 通用「列出并归一化」流程（所有 K8s 资源类型共用）
    async fn collect_kind<T, F>(
        &self,
        api: Api<T>,
        kind: ResourceKind,
        endpoint: &str,
        snapshot: &mut ResourceSnapshot,
        decorate: F,
    ) -> OpsResult<()>
    where
        T: Resource<DynamicType = ()> + DeserializeOwned + Clone + std::fmt::Debug,
        F: Fn(&T, ResourceRecord) -> ResourceRecord,
    {
        let action = format!("GET {} 列表", kind.as_str());
        let list = match api.list(&ListParams::default()).await {
            Ok(list) => list,
            Err(err) => return absorb_kind_error(snapshot, &action, err),
        };
        let total = list.items.len();
        for item in list.items.into_iter().take(self.config.max_items) {
            let meta = item.meta().clone();
            let Some(name) = meta.name.clone().filter(|name| !name.is_empty()) else {
                continue;
            };
            let mut key = ResourceKey::new(Provider::Kubernetes, endpoint, kind, name.clone());
            if kind.is_namespaced() {
                if let Some(namespace) = meta.namespace.clone().filter(|ns| !ns.is_empty()) {
                    key = key.with_namespace(namespace);
                }
            }
            let mut record = ResourceRecord::new(key, name, ResourceStatus::Running);
            record.created_at = meta
                .creation_timestamp
                .as_ref()
                .map(|time| time.0.as_millisecond());
            record.labels = meta.labels.clone().unwrap_or_default();
            snapshot.push(decorate(&item, record));
        }
        absorb_truncation(snapshot, kind, total, self.config.max_items);
        Ok(())
    }
}

/// 命名空间 → 资源记录
fn decorate_namespace(object: &Namespace, mut record: ResourceRecord) -> ResourceRecord {
    let phase = object
        .status
        .as_ref()
        .and_then(|status| status.phase.clone());
    record.status = namespace_status(phase.as_deref());
    record.raw_state = phase;
    record
}

/// 节点 → 资源记录（Ready 条件决定是否「正常运行」）
fn decorate_node(object: &Node, mut record: ResourceRecord) -> ResourceRecord {
    let status = object.status.as_ref();
    let ready = status
        .and_then(|status| status.conditions.as_ref())
        .and_then(|conditions| {
            conditions
                .iter()
                .find(|condition| condition.type_ == "Ready")
        })
        .map(|condition| condition.status == "True")
        .unwrap_or(false);
    record.status = if ready {
        ResourceStatus::Running
    } else {
        ResourceStatus::Offline
    };
    record.raw_state = Some(if ready { "Ready" } else { "NotReady" }.to_string());

    let info = status.and_then(|status| status.node_info.as_ref());
    let capacity: Option<BTreeMap<String, String>> = status
        .and_then(|status| status.capacity.as_ref())
        .map(|capacity| {
            capacity
                .iter()
                .map(|(key, quantity)| (key.clone(), quantity.0.clone()))
                .collect()
        });
    record.attributes = serde_json::json!({
        "kubelet_version": info.map(|info| info.kubelet_version.clone()),
        "os_image": info.map(|info| info.os_image.clone()),
        "container_runtime_version": info.map(|info| info.container_runtime_version.clone()),
        "architecture": info.map(|info| info.architecture.clone()),
        "capacity": capacity,
    });
    record
}

/// Deployment → 资源记录
fn decorate_deployment(object: &Deployment, mut record: ResourceRecord) -> ResourceRecord {
    let desired = object
        .spec
        .as_ref()
        .and_then(|spec| spec.replicas)
        .unwrap_or(0);
    let status = object.status.as_ref();
    let ready = status.and_then(|status| status.ready_replicas).unwrap_or(0);
    record.status = workload_status(desired, ready);
    record.raw_state = Some(format!("{ready}/{desired}"));
    record.attributes = serde_json::json!({
        "desired_replicas": desired,
        "ready_replicas": ready,
        "available_replicas": status.and_then(|status| status.available_replicas),
        "updated_replicas": status.and_then(|status| status.updated_replicas),
    });
    record
}

/// StatefulSet → 资源记录
fn decorate_statefulset(object: &StatefulSet, mut record: ResourceRecord) -> ResourceRecord {
    let desired = object
        .spec
        .as_ref()
        .and_then(|spec| spec.replicas)
        .unwrap_or(0);
    let status = object.status.as_ref();
    let ready = status.and_then(|status| status.ready_replicas).unwrap_or(0);
    let updated = status.and_then(|status| status.updated_replicas);
    record.status = workload_status(desired, ready);
    record.raw_state = Some(format!("{ready}/{desired}"));
    record.attributes = serde_json::json!({
        "desired_replicas": desired,
        "ready_replicas": ready,
        "current_replicas": status.and_then(|status| status.current_replicas),
        "updated_replicas": updated,
    });
    record
}

/// DaemonSet → 资源记录
fn decorate_daemonset(object: &DaemonSet, mut record: ResourceRecord) -> ResourceRecord {
    let status = object.status.as_ref();
    let desired = status
        .map(|status| status.desired_number_scheduled)
        .unwrap_or(0);
    let ready = status.map(|status| status.number_ready).unwrap_or(0);
    record.status = workload_status(desired, ready);
    record.raw_state = Some(format!("{ready}/{desired}"));
    record.attributes = serde_json::json!({
        "desired_number_scheduled": desired,
        "number_ready": ready,
        "number_available": status.and_then(|status| status.number_available),
        "number_misscheduled": status.map(|status| status.number_misscheduled),
    });
    record
}

/// CronJob → 资源记录（`suspend=true` 映射为「冻结」）
fn decorate_cronjob(object: &CronJob, mut record: ResourceRecord) -> ResourceRecord {
    let spec = object.spec.as_ref();
    let suspend = spec.and_then(|spec| spec.suspend).unwrap_or(false);
    record.status = if suspend {
        ResourceStatus::Frozen
    } else {
        ResourceStatus::Running
    };
    record.raw_state = Some(if suspend { "Suspended" } else { "Scheduled" }.to_string());
    record.attributes = serde_json::json!({
        "schedule": spec.map(|spec| spec.schedule.clone()),
        "suspend": suspend,
        "concurrency_policy": spec.and_then(|spec| spec.concurrency_policy.clone()),
        "time_zone": spec.and_then(|spec| spec.time_zone.clone()),
    });
    record
}

/// Job → 资源记录（短时动态资产）
fn decorate_job(object: &Job, mut record: ResourceRecord) -> ResourceRecord {
    let status = object.status.as_ref();
    let active = status.and_then(|status| status.active).unwrap_or(0);
    let succeeded = status.and_then(|status| status.succeeded).unwrap_or(0);
    let failed = status.and_then(|status| status.failed).unwrap_or(0);
    record.status = job_status(active, succeeded, failed);
    record.raw_state = Some(format!(
        "active={active},succeeded={succeeded},failed={failed}"
    ));
    record.attributes = serde_json::json!({
        "active": active,
        "succeeded": succeeded,
        "failed": failed,
    });
    record
}

/// Pod → 资源记录（短时动态资产；父资源指向所在节点）
fn decorate_pod(object: &Pod, mut record: ResourceRecord) -> ResourceRecord {
    let phase = object
        .status
        .as_ref()
        .and_then(|status| status.phase.clone());
    record.status = pod_status(phase.as_deref());
    record.raw_state = phase;

    let node_name = object
        .spec
        .as_ref()
        .and_then(|spec| spec.node_name.clone())
        .filter(|name| !name.is_empty());
    if let Some(node_name) = &node_name {
        let endpoint = record.key.endpoint.clone();
        record.parent_uid = Some(
            ResourceKey::new(
                Provider::Kubernetes,
                endpoint,
                ResourceKind::ClusterNode,
                node_name.clone(),
            )
            .uid(),
        );
    }

    let restarts: i32 = object
        .status
        .as_ref()
        .and_then(|status| status.container_statuses.as_ref())
        .map(|statuses| {
            statuses
                .iter()
                .map(|status| status.restart_count)
                .sum::<i32>()
        })
        .unwrap_or(0);
    record.attributes = serde_json::json!({
        "node_name": node_name,
        "pod_ip": object.status.as_ref().and_then(|status| status.pod_ip.clone()),
        "qos_class": object.status.as_ref().and_then(|status| status.qos_class.clone()),
        "restart_count": restarts,
        "container_count": object.spec.as_ref().map(|spec| spec.containers.len()),
    });
    record
}

/// Service → 资源记录
fn decorate_service(object: &Service, mut record: ResourceRecord) -> ResourceRecord {
    let spec = object.spec.as_ref();
    let ports: Option<Vec<String>> = spec.and_then(|spec| spec.ports.as_ref()).map(|ports| {
        ports
            .iter()
            .map(|port| match port.protocol.clone() {
                Some(protocol) => format!("{}/{}", port.port, protocol),
                None => port.port.to_string(),
            })
            .collect()
    });
    record.attributes = serde_json::json!({
        "type": spec.and_then(|spec| spec.type_.clone()),
        "cluster_ip": spec.and_then(|spec| spec.cluster_ip.clone()),
        "ports": ports,
        "external_name": spec.and_then(|spec| spec.external_name.clone()),
    });
    record
}

/// Ingress → 资源记录
fn decorate_ingress(object: &Ingress, mut record: ResourceRecord) -> ResourceRecord {
    let spec = object.spec.as_ref();
    let hosts: Vec<String> = spec
        .and_then(|spec| spec.rules.as_ref())
        .map(|rules| rules.iter().filter_map(|rule| rule.host.clone()).collect())
        .unwrap_or_default();
    let tls_hosts: Vec<String> = spec
        .and_then(|spec| spec.tls.as_ref())
        .map(|tls| {
            tls.iter()
                .flat_map(|entry| entry.hosts.clone().unwrap_or_default())
                .collect()
        })
        .unwrap_or_default();
    record.attributes = serde_json::json!({
        "hosts": hosts,
        "tls_hosts": tls_hosts,
        "ingress_class_name": spec.and_then(|spec| spec.ingress_class_name.clone()),
    });
    record
}

/// PVC → 资源记录
fn decorate_pvc(object: &PersistentVolumeClaim, mut record: ResourceRecord) -> ResourceRecord {
    let phase = object
        .status
        .as_ref()
        .and_then(|status| status.phase.clone());
    record.status = pvc_status(phase.as_deref());
    record.raw_state = phase;

    let spec = object.spec.as_ref();
    let requested = spec
        .and_then(|spec| spec.resources.as_ref())
        .and_then(|resources| resources.requests.as_ref())
        .and_then(|requests| requests.get("storage"))
        .map(|quantity| quantity.0.clone());
    record.attributes = serde_json::json!({
        "storage_class_name": spec.and_then(|spec| spec.storage_class_name.clone()),
        "access_modes": spec.and_then(|spec| spec.access_modes.clone()),
        "volume_name": spec.and_then(|spec| spec.volume_name.clone()),
        "requested_storage": requested,
        "capacity": object.status.as_ref().and_then(|status| status.capacity.as_ref()).and_then(|capacity| capacity.get("storage")).map(|quantity| quantity.0.clone()),
    });
    record
}

/// ConfigMap → 资源记录（只保留键名，绝不取 Data 内容）
fn decorate_configmap(object: &ConfigMap, mut record: ResourceRecord) -> ResourceRecord {
    let data_keys: Vec<String> = object
        .data
        .as_ref()
        .map(|data| data.keys().cloned().collect())
        .unwrap_or_default();
    record.attributes = serde_json::json!({
        "data_keys": data_keys,
        "binary_data_count": object.binary_data.as_ref().map(BTreeMap::len),
        "immutable": object.immutable,
    });
    record
}

/// Secret → 资源记录（只保留键名与类型，绝不取 Data 内容）
fn decorate_secret(object: &Secret, mut record: ResourceRecord) -> ResourceRecord {
    let data_keys: Vec<String> = object
        .data
        .as_ref()
        .map(|data| data.keys().cloned().collect())
        .unwrap_or_default();
    record.attributes = serde_json::json!({
        "type": object.type_.clone(),
        "data_keys": data_keys,
        "immutable": object.immutable,
    });
    record
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inline_yaml() -> String {
        r#"apiVersion: v1
kind: Config
current-context: ops
clusters:
  - name: ops
    cluster:
      server: "https://10.0.0.10:6443"
users:
  - name: ops
    user:
      token: "t0ken"
contexts:
  - name: ops
    context:
      cluster: ops
      user: ops
      namespace: "platform"
"#
        .to_string()
    }

    #[test]
    fn direct_yaml_round_trips_through_kubeconfig() {
        let pem = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n";
        let yaml = direct_kubeconfig_yaml(
            "https://10.0.0.10:6443",
            Some("t0ken"),
            Some(pem),
            None,
            None,
            false,
            Some("platform"),
        );
        let kubeconfig = Kubeconfig::from_yaml(&yaml).expect("合成的 kubeconfig 必须可解析");

        let cluster = kubeconfig
            .clusters
            .first()
            .and_then(|named| named.cluster.as_ref())
            .expect("缺少 cluster");
        assert_eq!(cluster.server.as_deref(), Some("https://10.0.0.10:6443"));
        assert!(!cluster.insecure_skip_tls_verify.unwrap_or(false));
        let ca = cluster
            .certificate_authority_data
            .clone()
            .expect("缺少 CA 数据");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(ca)
            .expect("CA 必须是合法 base64");
        assert_eq!(String::from_utf8(decoded).expect("CA 必须是 UTF-8"), pem);

        let user = kubeconfig
            .auth_infos
            .first()
            .and_then(|named| named.auth_info.as_ref())
            .expect("缺少 user");
        assert!(user.token.is_some(), "Token 必须写进 kubeconfig");

        let context = kubeconfig
            .contexts
            .first()
            .and_then(|named| named.context.as_ref())
            .expect("缺少 context");
        assert_eq!(context.cluster, "ops-collector");
        assert_eq!(context.namespace.as_deref(), Some("platform"));
    }

    #[test]
    fn insecure_flag_skips_ca_and_marks_certificate_check() {
        let yaml =
            direct_kubeconfig_yaml("https://10.0.0.10:6443", None, None, None, None, true, None);
        let kubeconfig = Kubeconfig::from_yaml(&yaml).expect("kubeconfig 必须可解析");
        let cluster = kubeconfig
            .clusters
            .first()
            .and_then(|named| named.cluster.as_ref())
            .expect("缺少 cluster");
        assert_eq!(cluster.insecure_skip_tls_verify, Some(true));
        assert!(cluster.certificate_authority_data.is_none());

        // 无凭证时写空映射，交给 kube 走匿名认证（随后由 API Server 判 401）
        let user = kubeconfig
            .auth_infos
            .first()
            .and_then(|named| named.auth_info.as_ref())
            .expect("缺少 user");
        assert!(user.token.is_none());
    }

    #[test]
    fn yaml_escape_only_touches_quotes_and_backslash() {
        assert_eq!(yaml_escape("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(yaml_escape("plain-token"), "plain-token");
    }

    #[tokio::test]
    async fn build_config_maps_server_and_timeout() {
        let config = ConnectionConfig::new(
            "测试集群",
            Provider::Kubernetes,
            "10.0.0.10:6443",
            AuthKind::KubeconfigInline {
                yaml: inline_yaml(),
                context: Some("ops".to_string()),
            },
        )
        .with_timeout_secs(20);

        let kube_config = build_config(&config).await.expect("构建 kube 配置失败");
        assert_eq!(kube_config.cluster_url.scheme_str(), Some("https"));
        assert_eq!(kube_config.cluster_url.host(), Some("10.0.0.10"));
        assert_eq!(kube_config.cluster_url.port_u16(), Some(6443));
        assert_eq!(kube_config.default_namespace, "platform");
        assert_eq!(kube_config.read_timeout, Some(Duration::from_secs(20)));
        assert_eq!(kube_config.connect_timeout, Some(Duration::from_secs(20)));
        assert!(!kube_config.accept_invalid_certs);
    }

    #[tokio::test]
    async fn connect_reports_malformed_kubeconfig_as_config_error() {
        let config = ConnectionConfig::new(
            "坏配置",
            Provider::Kubernetes,
            "",
            AuthKind::KubeconfigInline {
                yaml: "clusters: [".to_string(),
                context: None,
            },
        );
        let Err(err) = K8sConnector::connect(config).await else {
            panic!("非法 kubeconfig 必须失败");
        };
        assert_eq!(err.classify(), FailureClass::Config);
    }

    #[tokio::test]
    async fn connect_rejects_non_kubernetes_provider() {
        let Err(err) = K8sConnector::connect(ConnectionConfig::local_docker("本机")).await else {
            panic!("provider 不匹配必须失败");
        };
        assert_eq!(err.classify(), FailureClass::Config);
    }

    #[test]
    fn status_mappers_follow_plan() {
        assert_eq!(pod_status(Some("Running")), ResourceStatus::Running);
        assert_eq!(pod_status(Some("Failed")), ResourceStatus::Offline);
        assert_eq!(pod_status(None), ResourceStatus::Unknown);
        assert_eq!(workload_status(2, 2), ResourceStatus::Running);
        assert_eq!(workload_status(2, 0), ResourceStatus::Offline);
        assert_eq!(workload_status(2, 1), ResourceStatus::Unknown);
        assert_eq!(workload_status(0, 0), ResourceStatus::Unknown);
        assert_eq!(job_status(1, 0, 0), ResourceStatus::Running);
        assert_eq!(job_status(0, 1, 0), ResourceStatus::Offline);
        assert_eq!(job_status(0, 0, 0), ResourceStatus::Unknown);
        assert_eq!(pvc_status(Some("Bound")), ResourceStatus::Running);
        assert_eq!(pvc_status(Some("Lost")), ResourceStatus::Offline);
        assert_eq!(
            namespace_status(Some("Terminating")),
            ResourceStatus::PendingOffline
        );
    }
}
