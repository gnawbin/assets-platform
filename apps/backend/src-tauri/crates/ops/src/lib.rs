//! 运维采集：Docker / Swarm / Kubernetes **只读**连接器
//!
//! 本 crate 是《IT资产与运维排查一体化系统落地方案》§3 采集发现引擎的连接层：
//! 只负责把远端资源归一化成 [`model::ResourceRecord`]，不做落库、不做调度、
//! 不做启停（对账器 / 调度器 / 操作器见后续迭代）。
//!
//! # 特性开关（默认全关，避免拖慢 Tauri 主构建）
//! - `docker`：[`connector::docker::DockerConnector`]，bollard 实现，覆盖单机 Docker 与 Swarm；
//! - `k8s`：[`connector::k8s::K8sConnector`]，kube-rs 实现，只走 APIServer。
//!
//! ```bash
//! cargo test -p ops --features docker,k8s
//! RUST_LOG=ops=debug cargo run -p ops --example ops_probe --features docker,k8s
//! ```
//!
//! # 只读纪律（方案 §6）
//! 连接器只调用 ping / version / list / get 类接口：不 exec、不启停、不删除；
//! 不直连 K8s 节点的 `docker.sock`；ConfigMap / Secret 只取元数据，绝不取内容。
//!
//! # 加密 provider
//! `rustls` 的 CryptoProvider 全仓统一为 `ring`（kube 的 `ring` feature 与 bollard
//! `ssl` 内部一致），因此调用方无需在启动时 `install_default`；若将来引入其他
//! provider，务必在应用启动处显式安装，避免 "no process-level CryptoProvider" 恐慌。
//!
//! # 安全约定
//! 连接凭证（Token、客户端证书、kubeconfig）只在本 crate 内使用，既不 `Debug`
//! 打印也不落日志；[`connector::AuthKind`] 的 `Debug` 实现已对敏感字段做掩码。

pub mod connector;
pub mod error;
pub mod model;
pub mod serde_util;

pub use connector::{
    AuthKind, ConnectionConfig, ConnectionHealth, ConnectionTier, Connector, ConnectorRegistry,
};
pub use error::{FailureClass, OpsError, OpsResult};
pub use model::{
    CategoryScope, Provider, ResourceKey, ResourceKind, ResourceRecord, ResourceSnapshot,
    ResourceStatus,
};
