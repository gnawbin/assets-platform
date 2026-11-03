//! 归一化资源模型
//!
//! 采集产物不直接映射数据库：Docker 容器、Swarm Task、K8s Pod 先统一成
//! [`ResourceRecord`]，再由对账器（reconciler，后续迭代）落 `assets` 台账。
//!
//! 三个关键约定（对应方案 §2.3 / §3.1）：
//! - **唯一身份**：[`ResourceKey::uid`] = `provider + endpoint + kind + namespace + name`，
//!   资源销毁只置「已下线」，绝不物理删除；
//! - **短时动态资产**：[`ResourceKind::is_short_lived`] 为真的资源（容器 / Pod / Swarm Task）
//!   归入 `is_short_lived=1` 子分类，禁止录入采购、折旧、维保等财务字段；
//! - **时间精度**：所有时间戳统一为 epoch 毫秒并以字符串序列化（见 [`crate::serde_util`]）。

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{OpsError, OpsResult};
use crate::serde_util::{i64_to_string, opt_i64_to_string};

/// 当前 epoch 毫秒
///
/// 采集观测时间统一用它，避免为 ops 引入 chrono 的 clock 特性。
pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .unwrap_or_default()
}

/// 采集来源大类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// 单机 Docker
    Docker,
    /// Docker Swarm 集群（只连 Manager）
    Swarm,
    /// Kubernetes 集群（只连 APIServer）
    ///
    /// `rename` 与 [`Provider::as_str`] 保持一致（`k8s`），同时保留 `kubernetes`
    /// 作为反序列化别名，兼容既有落库数据与旧接口调用方。
    #[serde(rename = "k8s", alias = "kubernetes")]
    Kubernetes,
    /// Redis（原生 / Redis Stack，含 RediSearch、RedisJSON 等模块）
    #[serde(rename = "redis")]
    Redis,
    /// Valkey（Redis 7.2 分叉，RESP 协议与命令兼容，需独立编码以便统计授权差异）
    #[serde(rename = "valkey")]
    Valkey,
    /// PostgreSQL + pgvector（**向量归档权威库**，`vector_role=archive`）
    #[serde(rename = "pgvector")]
    PgVector,
    /// LanceDB（本地目录 / 对象存储的向量副本，**可重建**，`vector_role=replica`）
    #[serde(rename = "lancedb")]
    LanceDb,
}

impl Provider {
    /// 全部取值（界面下拉、批量探测用）
    pub const ALL: [Self; 7] = [
        Self::Docker,
        Self::Swarm,
        Self::Kubernetes,
        Self::Redis,
        Self::Valkey,
        Self::PgVector,
        Self::LanceDb,
    ];

    /// 落库 / uid 用的稳定编码
    ///
    /// 与 `#[serde(rename)]` 必须逐字一致（有 `serde_name_equals_as_str` 单测守护）：
    /// 落库、`resource_uid`、HTTP 接口、前端下拉全部依赖它。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Docker => "docker",
            Self::Swarm => "swarm",
            Self::Kubernetes => "k8s",
            Self::Redis => "redis",
            Self::Valkey => "valkey",
            Self::PgVector => "pgvector",
            Self::LanceDb => "lancedb",
        }
    }

    /// 中文标签
    pub const fn label(self) -> &'static str {
        match self {
            Self::Docker => "Docker 单机",
            Self::Swarm => "Docker Swarm",
            Self::Kubernetes => "Kubernetes",
            Self::Redis => "Redis",
            Self::Valkey => "Valkey",
            Self::PgVector => "PostgreSQL + pgvector",
            Self::LanceDb => "LanceDB",
        }
    }

    /// 对应 `asset_category.category_scope`
    ///
    /// 容器 / 集群编排工具属于 **IT 逻辑资产**；缓存与向量库属于 **数据资产**
    /// （方案 §2.1：数据资产按数据实体而非宿主机计量）。
    pub const fn category_scope(self) -> CategoryScope {
        match self {
            Self::Docker | Self::Swarm | Self::Kubernetes => CategoryScope::Logical,
            Self::Redis | Self::Valkey | Self::PgVector | Self::LanceDb => CategoryScope::Data,
        }
    }

    /// 是否为「数据存储」类来源（缓存 / 向量库）
    ///
    /// 采集侧据此决定超时预算与「不做 DDL / 不写数据」的硬约束提示。
    pub const fn is_data_store(self) -> bool {
        matches!(
            self,
            Self::Redis | Self::Valkey | Self::PgVector | Self::LanceDb
        )
    }

    /// 向量库在采集链路中的角色
    ///
    /// - `archive`：pgvector，权威副本，**不可**由采集侧重建；
    /// - `replica`：LanceDB，可由 pgvector 重放重建，允许漂移（见 `attributes.replica_drift`）。
    pub const fn vector_role(self) -> Option<&'static str> {
        match self {
            Self::PgVector => Some("archive"),
            Self::LanceDb => Some("replica"),
            _ => None,
        }
    }
}

impl FromStr for Provider {
    type Err = OpsError;

    fn from_str(value: &str) -> OpsResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "docker" => Ok(Self::Docker),
            "swarm" => Ok(Self::Swarm),
            "k8s" | "kubernetes" => Ok(Self::Kubernetes),
            // 别名只为「人手输入 / 旧配置」兼容，落库一律用 `as_str`
            "redis" | "redis_stack" => Ok(Self::Redis),
            "valkey" => Ok(Self::Valkey),
            "pgvector" | "postgres" | "postgresql" | "pg" => Ok(Self::PgVector),
            "lancedb" | "lance" => Ok(Self::LanceDb),
            other => Err(OpsError::Parse(format!("未知采集来源：{other}"))),
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 资产大类（`asset_category.category_scope`，见方案 §2.1 的 5 大一级分类）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CategoryScope {
    /// 实物硬件资产（财务固定资产）
    Physical,
    /// 软件资产
    Software,
    /// IT 逻辑资产（虚拟化 / Docker / Swarm / K8s）
    Logical,
    /// 云租赁资产
    Cloud,
    /// 数据资产
    Data,
}

/// 归一化资源类型
///
/// Docker / Swarm / K8s 的名称冲突（Node、Service）在此显式区分，
/// 避免下游按 kind 做 match 时出现歧义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    // ---- Docker 单机 ----
    /// 容器
    Container,
    /// 镜像
    Image,
    /// 网络
    Network,
    /// 存储卷
    Volume,
    // ---- Docker Swarm ----
    /// Swarm 节点
    SwarmNode,
    /// Swarm 服务
    SwarmService,
    /// Swarm 任务（短生命周期）
    SwarmTask,
    // ---- Kubernetes ----
    /// 命名空间
    Namespace,
    /// K8s 节点
    ClusterNode,
    /// Deployment
    Deployment,
    /// DaemonSet
    DaemonSet,
    /// StatefulSet
    StatefulSet,
    /// Job（短生命周期）
    Job,
    /// CronJob
    CronJob,
    /// Pod（短生命周期）
    Pod,
    /// K8s Service
    KubeService,
    /// Ingress
    Ingress,
    /// PersistentVolume
    PersistentVolume,
    /// PersistentVolumeClaim
    PersistentVolumeClaim,
    /// ConfigMap（只采元数据）
    ConfigMap,
    /// Secret（只采元数据，绝不取内容）
    Secret,
    // ---- Redis / Valkey（缓存与检索） ----
    /// 缓存实例（独立实例或 Redis Cluster 中的单个节点）
    CacheInstance,
    /// 逻辑库（db0 / db1 ...，带 keyspace 键数与内存占用）
    CacheKeyspace,
    /// 已加载模块（`MODULE LIST`：search / ReJSON / bf ...）
    CacheModule,
    /// 检索索引（RediSearch `FT._LIST`）
    SearchIndex,
    // ---- 向量库（pgvector 权威 / LanceDB 副本） ----
    /// 向量表（pgvector `schema.table`、LanceDB `table`）
    VectorTable,
    /// 向量索引（pgvector `pg_index`、LanceDB 的 `list_indices`）
    VectorIndex,
}

impl ResourceKind {
    /// 全部取值（自检用，保证 `as_str` / `FromStr` 覆盖一致）
    pub const ALL: [Self; 27] = [
        Self::Container,
        Self::Image,
        Self::Network,
        Self::Volume,
        Self::SwarmNode,
        Self::SwarmService,
        Self::SwarmTask,
        Self::Namespace,
        Self::ClusterNode,
        Self::Deployment,
        Self::DaemonSet,
        Self::StatefulSet,
        Self::Job,
        Self::CronJob,
        Self::Pod,
        Self::KubeService,
        Self::Ingress,
        Self::PersistentVolume,
        Self::PersistentVolumeClaim,
        Self::ConfigMap,
        Self::Secret,
        Self::CacheInstance,
        Self::CacheKeyspace,
        Self::CacheModule,
        Self::SearchIndex,
        Self::VectorTable,
        Self::VectorIndex,
    ];

    /// 落库 / uid 用的稳定编码
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Container => "container",
            Self::Image => "image",
            Self::Network => "network",
            Self::Volume => "volume",
            Self::SwarmNode => "swarm_node",
            Self::SwarmService => "swarm_service",
            Self::SwarmTask => "swarm_task",
            Self::Namespace => "namespace",
            Self::ClusterNode => "cluster_node",
            Self::Deployment => "deployment",
            Self::DaemonSet => "daemonset",
            Self::StatefulSet => "statefulset",
            Self::Job => "job",
            Self::CronJob => "cronjob",
            Self::Pod => "pod",
            Self::KubeService => "kube_service",
            Self::Ingress => "ingress",
            Self::PersistentVolume => "persistent_volume",
            Self::PersistentVolumeClaim => "persistent_volume_claim",
            Self::ConfigMap => "configmap",
            Self::Secret => "secret",
            Self::CacheInstance => "cache_instance",
            Self::CacheKeyspace => "cache_keyspace",
            Self::CacheModule => "cache_module",
            Self::SearchIndex => "search_index",
            Self::VectorTable => "vector_table",
            Self::VectorIndex => "vector_index",
        }
    }

    /// 中文标签
    pub const fn label(self) -> &'static str {
        match self {
            Self::Container => "容器",
            Self::Image => "镜像",
            Self::Network => "网络",
            Self::Volume => "存储卷",
            Self::SwarmNode => "Swarm 节点",
            Self::SwarmService => "Swarm 服务",
            Self::SwarmTask => "Swarm 任务",
            Self::Namespace => "命名空间",
            Self::ClusterNode => "K8s 节点",
            Self::Deployment => "Deployment",
            Self::DaemonSet => "DaemonSet",
            Self::StatefulSet => "StatefulSet",
            Self::Job => "Job",
            Self::CronJob => "CronJob",
            Self::Pod => "Pod",
            Self::KubeService => "Service",
            Self::Ingress => "Ingress",
            Self::PersistentVolume => "PV",
            Self::PersistentVolumeClaim => "PVC",
            Self::ConfigMap => "ConfigMap",
            Self::Secret => "Secret",
            Self::CacheInstance => "缓存实例",
            Self::CacheKeyspace => "逻辑库",
            Self::CacheModule => "已加载模块",
            Self::SearchIndex => "检索索引",
            Self::VectorTable => "向量表",
            Self::VectorIndex => "向量索引",
        }
    }

    /// 是否为「短时动态资产」（方案 §2.1：`is_short_lived=1`）
    pub const fn is_short_lived(self) -> bool {
        matches!(
            self,
            Self::Container | Self::SwarmTask | Self::Pod | Self::Job
        )
    }

    /// 是否命名空间级资源（K8s 之外恒为 false）
    pub const fn is_namespaced(self) -> bool {
        matches!(
            self,
            Self::Deployment
                | Self::DaemonSet
                | Self::StatefulSet
                | Self::Job
                | Self::CronJob
                | Self::Pod
                | Self::KubeService
                | Self::Ingress
                | Self::PersistentVolumeClaim
                | Self::ConfigMap
                | Self::Secret
        )
    }

    /// 所属来源**族**（同族来源共享资源类型）
    ///
    /// Redis / Valkey 共用同一批缓存类型，此处以族代表 `Redis` 表示；
    /// pgvector / LanceDB 共用向量表与索引类型，以 `PgVector` 表示。
    /// 需要「精确归属」判断时用 [`Self::supports`]。
    pub const fn family(self) -> Provider {
        match self {
            Self::Container | Self::Image | Self::Network | Self::Volume => Provider::Docker,
            Self::SwarmNode | Self::SwarmService | Self::SwarmTask => Provider::Swarm,
            Self::CacheInstance | Self::CacheKeyspace | Self::CacheModule | Self::SearchIndex => {
                Provider::Redis
            }
            Self::VectorTable | Self::VectorIndex => Provider::PgVector,
            _ => Provider::Kubernetes,
        }
    }

    /// 该资源类型能否由指定来源采集
    ///
    /// 对账器只用它做「来源 → 类型」白名单校验，避免把 Valkey 采集出的缓存类型
    /// 误判成「Redis 专有」，也避免把 LanceDB 副本类型误判成 pgvector 权威数据。
    pub const fn supports(self, provider: Provider) -> bool {
        match provider {
            Provider::Docker => matches!(
                self,
                Self::Container | Self::Image | Self::Network | Self::Volume
            ),
            Provider::Swarm => matches!(
                self,
                Self::SwarmNode | Self::SwarmService | Self::SwarmTask
            ),
            Provider::Redis | Provider::Valkey => matches!(
                self,
                Self::CacheInstance | Self::CacheKeyspace | Self::CacheModule | Self::SearchIndex
            ),
            Provider::PgVector | Provider::LanceDb => {
                matches!(self, Self::VectorTable | Self::VectorIndex)
            }
            Provider::Kubernetes => matches!(
                self,
                Self::Namespace
                    | Self::ClusterNode
                    | Self::Deployment
                    | Self::DaemonSet
                    | Self::StatefulSet
                    | Self::Job
                    | Self::CronJob
                    | Self::Pod
                    | Self::KubeService
                    | Self::Ingress
                    | Self::PersistentVolume
                    | Self::PersistentVolumeClaim
                    | Self::ConfigMap
                    | Self::Secret
            ),
        }
    }
}

impl FromStr for ResourceKind {
    type Err = OpsError;

    fn from_str(value: &str) -> OpsResult<Self> {
        let normalized = value.trim().to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == normalized)
            .ok_or_else(|| OpsError::Parse(format!("未知资源类型：{value}")))
    }
}

impl fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 资产状态（方案 §2.3 的 8 态）
///
/// 前 8 个取值与 `assets.status` 的目标枚举一一对应；[`Self::Unknown`] 不属于业务状态，
/// 仅表示「连接器本次无法判定」（例如容器处于 `restarting`），落库时按原状态保留。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceStatus {
    /// 正常运行
    Running,
    /// 维修中
    UnderRepair,
    /// 待变更
    PendingChange,
    /// 待下线
    PendingOffline,
    /// 已下线（采集发现资源消失也置为此态，不物理删除）
    Offline,
    /// 已报废
    Scrapped,
    /// 告警屏蔽
    AlarmMuted,
    /// 冻结
    Frozen,
    /// 无法判定（非业务状态）
    Unknown,
}

impl ResourceStatus {
    /// 落库 / 接口用的稳定编码
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::UnderRepair => "under_repair",
            Self::PendingChange => "pending_change",
            Self::PendingOffline => "pending_offline",
            Self::Offline => "offline",
            Self::Scrapped => "scrapped",
            Self::AlarmMuted => "alarm_muted",
            Self::Frozen => "frozen",
            Self::Unknown => "unknown",
        }
    }

    /// 中文标签（与方案 §2.3 的 8 态文案一致）
    pub const fn label(self) -> &'static str {
        match self {
            Self::Running => "正常运行",
            Self::UnderRepair => "维修中",
            Self::PendingChange => "待变更",
            Self::PendingOffline => "待下线",
            Self::Offline => "已下线",
            Self::Scrapped => "已报废",
            Self::AlarmMuted => "告警屏蔽",
            Self::Frozen => "冻结",
            Self::Unknown => "未知",
        }
    }

    /// 兼容存量数值状态（`assets.status`：0=正常 1=借用 2=维修 3=报废 4=过期）
    ///
    /// 迁移脚本与采集侧共用同一张映射表，避免两处口径不一致。
    pub const fn from_legacy_code(code: i16) -> Self {
        match code {
            0 => Self::Running,
            // 借用：仍在使用中，业务上等同正常运行
            1 => Self::Running,
            2 => Self::UnderRepair,
            3 => Self::Scrapped,
            // 过期：进入待下线处理流程
            4 => Self::PendingOffline,
            _ => Self::Unknown,
        }
    }
}

impl fmt::Display for ResourceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// 资源唯一键
///
/// `uid` 形如 `docker://10.0.0.5:2375/container/3f2c...`（K8s 会多一层命名空间）：
/// `{provider}://{endpoint}/{kind}[/{namespace}]/{name}`。
/// `endpoint` 必须是**去掉 scheme 与末尾 `/`** 的 `host[:port]`（见
/// [`crate::connector::ConnectionConfig::endpoint_key`]），否则 uid 无法反解。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResourceKey {
    /// 采集来源
    pub provider: Provider,
    /// 去掉 scheme 的端点，如 `10.0.0.5:2375`、`local`、`10.0.0.10:6443`
    pub endpoint: String,
    /// 资源类型
    pub kind: ResourceKind,
    /// 命名空间（K8s 命名空间级资源必填；其它为 None）
    pub namespace: Option<String>,
    /// 资源标识（容器 ID / 镜像 ID / K8s uid 或 name）
    pub name: String,
}

impl ResourceKey {
    /// 构造不带命名空间的键
    pub fn new(
        provider: Provider,
        endpoint: impl Into<String>,
        kind: ResourceKind,
        name: impl Into<String>,
    ) -> Self {
        Self {
            provider,
            endpoint: endpoint.into(),
            kind,
            namespace: None,
            name: name.into(),
        }
    }

    /// 追加命名空间
    #[must_use]
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// 生成落 `assets.resource_uid` 的唯一字符串
    pub fn uid(&self) -> String {
        let mut uid = String::with_capacity(self.endpoint.len() + self.name.len() + 32);
        uid.push_str(self.provider.as_str());
        uid.push_str("://");
        uid.push_str(&self.endpoint);
        uid.push('/');
        uid.push_str(self.kind.as_str());
        uid.push('/');
        if let Some(namespace) = &self.namespace {
            uid.push_str(namespace);
            uid.push('/');
        }
        uid.push_str(&self.name);
        uid
    }

    /// 反解 [`Self::uid`]，用于「按 uid 反查连接器」的排查链路
    pub fn parse(uid: &str) -> OpsResult<Self> {
        let (provider, rest) = uid
            .split_once("://")
            .ok_or_else(|| OpsError::Parse(format!("资源 uid 缺少来源前缀：{uid}")))?;
        let provider = Provider::from_str(provider)?;
        let mut segments = rest.split('/');
        let endpoint = segments
            .next()
            .filter(|segment| !segment.is_empty())
            .ok_or_else(|| OpsError::Parse(format!("资源 uid 缺少端点：{uid}")))?;
        let kind = segments
            .next()
            .ok_or_else(|| OpsError::Parse(format!("资源 uid 缺少类型：{uid}")))?;
        let kind = ResourceKind::from_str(kind)?;
        let parts: Vec<&str> = segments.collect();
        let (namespace, name) = match parts.as_slice() {
            [name] => (None, *name),
            [namespace, name] => (Some((*namespace).to_string()), *name),
            _ => return Err(OpsError::Parse(format!("资源 uid 段数非法：{uid}"))),
        };
        if name.is_empty() {
            return Err(OpsError::Parse(format!("资源 uid 缺少资源名：{uid}")));
        }
        Ok(Self {
            provider,
            endpoint: endpoint.to_string(),
            kind,
            namespace,
            name: name.to_string(),
        })
    }
}

/// 单条采集结果
///
/// 字段刻意保持「原始 + 归一化」双份：`status` 供台账展示，
/// `raw_state` / `attributes` 供排查时对照厂商控制台原文。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRecord {
    /// 唯一键（落库时取 `key.uid()`）
    pub key: ResourceKey,
    /// 展示名（容器名 / 工作负载名 / 节点名）
    pub display_name: String,
    /// 归一化状态
    pub status: ResourceStatus,
    /// 原始状态（Docker `state`、K8s `phase`），未识别时原样保留
    pub raw_state: Option<String>,
    /// 父资源 uid（容器 → Swarm Service、Pod → Node、Task → Service）
    pub parent_uid: Option<String>,
    /// 镜像 / 版本
    pub image: Option<String>,
    /// 资源创建时间（epoch 毫秒）
    #[serde(
        serialize_with = "opt_i64_to_string",
        deserialize_with = "crate::serde_util::opt_i64_from_string"
    )]
    pub created_at: Option<i64>,
    /// 本次观测时间（epoch 毫秒），对应 `assets.last_seen_at`
    #[serde(
        serialize_with = "i64_to_string",
        deserialize_with = "crate::serde_util::i64_from_string"
    )]
    pub observed_at: i64,
    /// 标签（Docker / K8s labels），用于业务归属与链路溯源
    pub labels: BTreeMap<String, String>,
    /// 其余关键属性（端口、容量、副本数、镜像摘要 ...）
    pub attributes: serde_json::Value,
}

impl ResourceRecord {
    /// 以最小必填字段构造，`observed_at` 默认取当前时间
    pub fn new(key: ResourceKey, display_name: impl Into<String>, status: ResourceStatus) -> Self {
        Self {
            key,
            display_name: display_name.into(),
            status,
            raw_state: None,
            parent_uid: None,
            image: None,
            created_at: None,
            observed_at: now_millis(),
            labels: BTreeMap::new(),
            attributes: serde_json::Value::Null,
        }
    }

    /// 设置原始状态
    #[must_use]
    pub fn with_raw_state(mut self, raw_state: impl Into<String>) -> Self {
        self.raw_state = Some(raw_state.into());
        self
    }

    /// 设置父资源 uid
    #[must_use]
    pub fn with_parent_uid(mut self, parent_uid: impl Into<String>) -> Self {
        self.parent_uid = Some(parent_uid.into());
        self
    }

    /// 设置镜像
    #[must_use]
    pub fn with_image(mut self, image: impl Into<String>) -> Self {
        self.image = Some(image.into());
        self
    }

    /// 设置创建时间（epoch 毫秒）
    #[must_use]
    pub fn with_created_at(mut self, created_at: i64) -> Self {
        self.created_at = Some(created_at);
        self
    }

    /// 设置标签（Docker / K8s labels、Redis keyspace 前缀等）
    #[must_use]
    pub fn with_labels(mut self, labels: BTreeMap<String, String>) -> Self {
        self.labels = labels;
        self
    }

    /// 设置扩展属性
    ///
    /// 约定：必须是 JSON 对象（`serde_json::json!({...})`）。数据存储类连接器
    /// 用它承载 `vector_role` / `replica_drift` / `dimensions` 等排查字段。
    #[must_use]
    pub fn with_attributes(mut self, attributes: serde_json::Value) -> Self {
        self.attributes = attributes;
        self
    }

    /// 是否短时动态资产（容器 / Pod / Swarm Task / Job）
    pub const fn is_short_lived(&self) -> bool {
        self.key.kind.is_short_lived()
    }

    /// 资源唯一字符串（`assets.resource_uid`）
    pub fn uid(&self) -> String {
        self.key.uid()
    }
}

/// 一次全量采集的快照
///
/// 对账器以「快照 vs 上次快照」做 diff；`partial_errors` 非空说明本次采集不完整，
/// 上游只能「新增/更新」，不得据此把缺失资源判为「已下线」（方案 §2.3）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceSnapshot {
    /// 采集来源
    pub provider: Provider,
    /// 采集端点（带 scheme 的原样端点，便于回显）
    pub endpoint: String,
    /// 采集完成时间（epoch 毫秒）
    #[serde(
        serialize_with = "i64_to_string",
        deserialize_with = "crate::serde_util::i64_from_string"
    )]
    pub collected_at: i64,
    /// 采集耗时（毫秒）
    pub elapsed_ms: u64,
    /// 归一化资源
    pub resources: Vec<ResourceRecord>,
    /// 非致命错误（如某命名空间无权限），不中断整体采集
    pub partial_errors: Vec<String>,
}

impl ResourceSnapshot {
    /// 创建空快照
    pub fn new(provider: Provider, endpoint: impl Into<String>, elapsed_ms: u64) -> Self {
        Self {
            provider,
            endpoint: endpoint.into(),
            collected_at: now_millis(),
            elapsed_ms,
            resources: Vec::new(),
            partial_errors: Vec::new(),
        }
    }

    /// 追加一条资源
    pub fn push(&mut self, record: ResourceRecord) {
        self.resources.push(record);
    }

    /// 追加非致命错误
    pub fn push_error(&mut self, message: impl Into<String>) {
        self.partial_errors.push(message.into());
    }

    /// 采集是否完整（无部分失败）
    pub fn is_complete(&self) -> bool {
        self.partial_errors.is_empty()
    }

    /// 各类型资源数量
    pub fn counts_by_kind(&self) -> BTreeMap<ResourceKind, usize> {
        let mut counts = BTreeMap::new();
        for record in &self.resources {
            *counts.entry(record.key.kind).or_insert(0) += 1;
        }
        counts
    }

    /// 指定类型的资源数量
    pub fn count_of(&self, kind: ResourceKind) -> usize {
        self.resources
            .iter()
            .filter(|record| record.key.kind == kind)
            .count()
    }

    /// 短时动态资源数量（容器 / Pod / Swarm Task / Job）
    pub fn short_lived_count(&self) -> usize {
        self.resources
            .iter()
            .filter(|record| record.is_short_lived())
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_uid_round_trips() {
        let key = ResourceKey::new(
            Provider::Docker,
            "10.0.0.5:2375",
            ResourceKind::Container,
            "3f2c8b1d",
        );
        let uid = key.uid();
        assert_eq!(uid, "docker://10.0.0.5:2375/container/3f2c8b1d");
        assert_eq!(ResourceKey::parse(&uid).expect("反解失败"), key);
    }

    #[test]
    fn k8s_uid_keeps_namespace() {
        let key = ResourceKey::new(
            Provider::Kubernetes,
            "10.0.0.10:6443",
            ResourceKind::Pod,
            "api-0",
        )
        .with_namespace("prod");
        let uid = key.uid();
        assert_eq!(uid, "k8s://10.0.0.10:6443/pod/prod/api-0");
        let parsed = ResourceKey::parse(&uid).expect("反解失败");
        assert_eq!(parsed, key);
        assert_eq!(parsed.namespace.as_deref(), Some("prod"));
    }

    #[test]
    fn local_socket_uid_uses_local_endpoint() {
        let key = ResourceKey::new(Provider::Docker, "local", ResourceKind::Image, "sha256:aa");
        assert_eq!(key.uid(), "docker://local/image/sha256:aa");
        assert_eq!(ResourceKey::parse(&key.uid()).expect("反解失败"), key);
    }

    #[test]
    fn malformed_uid_is_rejected() {
        for uid in [
            "container/abc",
            "docker:///container/abc",
            "docker://host/unknown/abc",
            "docker://host",
            "docker://host/container/a/b/c",
        ] {
            assert!(ResourceKey::parse(uid).is_err(), "非法 uid 应当报错: {uid}");
        }
    }

    #[test]
    fn legacy_status_codes_are_mapped() {
        assert_eq!(ResourceStatus::from_legacy_code(0), ResourceStatus::Running);
        assert_eq!(ResourceStatus::from_legacy_code(1), ResourceStatus::Running);
        assert_eq!(
            ResourceStatus::from_legacy_code(2),
            ResourceStatus::UnderRepair
        );
        assert_eq!(
            ResourceStatus::from_legacy_code(3),
            ResourceStatus::Scrapped
        );
        assert_eq!(
            ResourceStatus::from_legacy_code(4),
            ResourceStatus::PendingOffline
        );
        assert_eq!(ResourceStatus::from_legacy_code(9), ResourceStatus::Unknown);
        assert_eq!(ResourceStatus::Running.label(), "正常运行");
        assert_eq!(ResourceStatus::Offline.as_str(), "offline");
    }

    #[test]
    fn kind_codes_are_unique_and_parseable() {
        let mut seen = std::collections::HashSet::new();
        for kind in ResourceKind::ALL {
            assert!(
                seen.insert(kind.as_str()),
                "资源类型编码重复: {}",
                kind.as_str()
            );
            assert_eq!(
                ResourceKind::from_str(kind.as_str()).expect("反解失败"),
                kind
            );
            assert!(!kind.label().is_empty());
        }
        assert_eq!(ResourceKind::ALL.len(), 27);
        assert!(ResourceKind::Pod.is_namespaced());
        assert!(!ResourceKind::ClusterNode.is_namespaced());
        assert!(!ResourceKind::Image.is_namespaced());
    }

    #[test]
    fn short_lived_set_matches_plan() {
        let short_lived: Vec<&str> = ResourceKind::ALL
            .into_iter()
            .filter(|kind| kind.is_short_lived())
            .map(ResourceKind::as_str)
            .collect();
        assert_eq!(short_lived, vec!["container", "swarm_task", "job", "pod"]);
    }

    #[test]
    fn provider_aliases_and_scope() {
        assert_eq!(
            "kubernetes".parse::<Provider>().expect("解析失败"),
            Provider::Kubernetes
        );
        assert_eq!(
            "K8S".parse::<Provider>().expect("解析失败"),
            Provider::Kubernetes
        );
        assert!("swarm_cluster".parse::<Provider>().is_err());
        // 数据存储来源的别名（兼容旧配置与人工录入）
        for alias in ["redis", "redis_stack"] {
            assert_eq!(alias.parse::<Provider>().expect("解析失败"), Provider::Redis);
        }
        for alias in ["pgvector", "postgres", "postgresql", "pg"] {
            assert_eq!(
                alias.parse::<Provider>().expect("解析失败"),
                Provider::PgVector
            );
        }
        for alias in ["lancedb", "lance"] {
            assert_eq!(
                alias.parse::<Provider>().expect("解析失败"),
                Provider::LanceDb
            );
        }
        assert_eq!(
            "valkey".parse::<Provider>().expect("解析失败"),
            Provider::Valkey
        );
    }

    #[test]
    fn provider_scope_splits_logical_and_data() {
        for provider in [Provider::Docker, Provider::Swarm, Provider::Kubernetes] {
            assert_eq!(provider.category_scope(), CategoryScope::Logical);
            assert_eq!(provider.category_scope().as_str(), "logical");
            assert!(!provider.is_data_store());
            assert_eq!(provider.vector_role(), None);
        }
        for provider in [
            Provider::Redis,
            Provider::Valkey,
            Provider::PgVector,
            Provider::LanceDb,
        ] {
            assert_eq!(provider.category_scope(), CategoryScope::Data);
            assert_eq!(provider.category_scope().as_str(), "data");
            assert!(provider.is_data_store());
        }
        assert_eq!(Provider::PgVector.vector_role(), Some("archive"));
        assert_eq!(Provider::LanceDb.vector_role(), Some("replica"));
        for provider in Provider::ALL {
            assert_eq!(provider.to_string(), provider.as_str());
        }
        assert_eq!(Provider::ALL.len(), 7);
    }

    #[test]
    fn serde_name_equals_as_str() {
        for provider in Provider::ALL {
            let json = serde_json::to_value(provider).expect("序列化失败");
            assert_eq!(
                json.as_str(),
                Some(provider.as_str()),
                "{} 的 serde 名与 as_str 不一致，会导致落库编码与接口编码分叉",
                provider.as_str()
            );
        }
    }

    #[test]
    fn kind_family_and_supports_cover_new_sources() {
        for kind in ResourceKind::ALL {
            assert!(
                kind.supports(kind.family()),
                "{} 应被同族来源支持",
                kind.as_str()
            );
        }
        for kind in [ResourceKind::CacheInstance, ResourceKind::SearchIndex] {
            assert!(kind.supports(Provider::Redis));
            assert!(kind.supports(Provider::Valkey));
            assert!(!kind.supports(Provider::PgVector));
        }
        for kind in [ResourceKind::VectorTable, ResourceKind::VectorIndex] {
            assert!(kind.supports(Provider::PgVector));
            assert!(kind.supports(Provider::LanceDb));
            assert!(!kind.supports(Provider::Redis));
        }
        // 数据存储类资源是长期资产：不得进入短时动态资产子分类
        assert!(!ResourceKind::CacheInstance.is_short_lived());
        assert!(!ResourceKind::VectorTable.is_short_lived());
        assert!(!ResourceKind::VectorTable.is_namespaced());
    }

    #[test]
    fn snapshot_counts_by_kind_and_short_lived() {
        let mut snapshot = ResourceSnapshot::new(Provider::Docker, "tcp://127.0.0.1:2375", 12);
        for name in ["c1", "c2"] {
            snapshot.push(ResourceRecord::new(
                ResourceKey::new(
                    Provider::Docker,
                    "127.0.0.1:2375",
                    ResourceKind::Container,
                    name,
                ),
                format!("web-{name}"),
                ResourceStatus::Running,
            ));
        }
        snapshot.push(ResourceRecord::new(
            ResourceKey::new(
                Provider::Docker,
                "127.0.0.1:2375",
                ResourceKind::Image,
                "sha256:1",
            ),
            "nginx:latest",
            ResourceStatus::Running,
        ));
        assert_eq!(snapshot.count_of(ResourceKind::Container), 2);
        assert_eq!(snapshot.count_of(ResourceKind::Image), 1);
        assert_eq!(snapshot.short_lived_count(), 2);
        assert_eq!(
            snapshot
                .counts_by_kind()
                .get(&ResourceKind::Container)
                .copied(),
            Some(2)
        );
        assert!(snapshot.is_complete());
        snapshot.push_error("namespace kube-system: forbidden");
        assert!(!snapshot.is_complete());
        assert_eq!(snapshot.partial_errors.len(), 1);
    }

    #[test]
    fn record_serializes_timestamps_as_strings() {
        let record = ResourceRecord::new(
            ResourceKey::new(Provider::Docker, "local", ResourceKind::Container, "c1"),
            "web-1",
            ResourceStatus::Running,
        )
        .with_created_at(1_700_000_000_000)
        .with_image("nginx:1.27")
        .with_raw_state("running")
        .with_parent_uid("swarm://local/swarm_service/svc1");

        let json = serde_json::to_value(&record).expect("序列化失败");
        assert_eq!(json["created_at"], serde_json::json!("1700000000000"));
        assert!(
            json["observed_at"].is_string(),
            "epoch 毫秒必须以字符串输出"
        );
        assert_eq!(json["status"], serde_json::json!("running"));
        assert_eq!(json["key"]["kind"], serde_json::json!("container"));
        assert_eq!(record.uid(), "docker://local/container/c1");
        assert!(record.is_short_lived());
    }
}

impl CategoryScope {
    /// 落库编码
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Physical => "physical",
            Self::Software => "software",
            Self::Logical => "logical",
            Self::Cloud => "cloud",
            Self::Data => "data",
        }
    }
}
