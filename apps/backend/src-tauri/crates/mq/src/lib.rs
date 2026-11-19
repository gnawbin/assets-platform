//! 消息总线客户端基建（与驱动无关的契约 + 内存实现 + 可选 Pulsar 驱动）。
//!
//! 本 crate 只提供客户端层，不含任何业务事件定义，也不被 `api` / `service` /
//! `gateway` / `bootstrap` 接线；`trustgraph-lite` 依旧保持零 IO。
//!
//! # 多客户端语义（设计文档 §5.2）
//! - **进程内**：生产路径只调 [`init`]（幂等），进程内只有一个 [`Bus`]；
//!   [`from_config`] 每次新建客户端，仅供测试 / examples。
//! - **同一 `Bus`**：同订阅名二次 `subscribe` 返回 [`MqError::AlreadySubscribed`]；
//!   同名但类型 / 位点不同返回 [`MqError::SubscriptionConflict`]。
//! - **跨副本**：`MqConfig::subscription_scope` 决定订阅名是否带 `instance_id`：
//!   `Instance`（默认）各自全量，`Shared` 自动分摊。
//!
//! # 默认零影响
//! `pulsar` feature 默认关闭；`cargo test -p mq` 只编译 `memory` 实现，
//! 不会引入 `pulsar` / `prost`。
//!
//! ```no_run
//! # async fn demo() -> Result<(), mq::MqError> {
//! let cfg = mq::MqConfig::from_env()?;
//! let bus = mq::init(cfg).await?;
//! let publisher = bus.publisher();
//! # let _ = (bus, publisher);
//! # Ok(())
//! # }
//! ```

#![allow(
    clippy::missing_errors_doc,
    reason = "所有错误统一由 MqError 表达，设计文档 §9 已有完整说明"
)]

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use tokio::sync::Mutex;

pub mod config;
pub mod envelope;
pub mod error;
#[cfg(feature = "memory")]
pub mod memory;
pub mod retry;

pub use config::{
    AuthMode, Compression, FailMode, MqConfig, MqInitialPosition, MqSubType, SubscriptionScope,
};
pub use envelope::{Envelope, Inbound, Receipt, TopicName};
pub use error::{MqError, MqResult};

/// 生产者：一个实例可向多个逻辑 topic 发布（内部按 topic 池化 producer）
#[async_trait]
pub trait Publisher: Send + Sync {
    /// 等 broker receipt；失败按 `PULSAR_PUBLISH_RETRY_MAX` 指数退避重试
    async fn publish(&self, msg: Envelope) -> Result<Receipt, MqError>;

    /// 冲刷攒批（退出前调用）
    async fn flush(&self) -> Result<(), MqError>;
}

/// 消费处理器：`Ok` → ack；`Err` → nack（Pulsar 由 broker 重投，超限进 DLQ）
#[async_trait]
pub trait MessageHandler: Send + Sync {
    async fn handle(&self, msg: Inbound) -> Result<(), MqError>;
}

/// 订阅句柄：关闭需要显式调用（避免 drop 时做 IO）
#[async_trait]
pub trait Subscription: Send + Sync {
    /// 触发退出并等待 consumer 关闭（幂等）
    async fn shutdown(&self) -> Result<(), MqError>;
}

/// 健康快照（多客户端 / 多副本排障定位）
#[derive(Debug, Clone)]
pub struct MqHealth {
    /// `PULSAR_ENABLED` 是否为真
    pub enabled: bool,
    /// 是否已建立 / 保持连接（内存实现恒为 `!closed`）
    pub connected: bool,
    /// 已知 broker 数（内存实现为 0）
    pub brokers: usize,
    /// 实例标识（`PULSAR_INSTANCE_ID`）
    pub instance_id: String,
    /// 订阅作用域（`instance` / `shared`）
    pub subscription_scope: SubscriptionScope,
}

/// 运行态快照：多客户端 / 多订阅排障用
#[derive(Debug, Clone, Default)]
pub struct MqStats {
    /// 已建立的 producer 数（按 topic 去重；内存实现为已知 topic 数）
    pub producers: usize,
    /// 活跃订阅数
    pub subscriptions: usize,
    /// 攒批中尚未收到 receipt 的消息数（内存实现恒为 0）
    pub pending_publishes: u64,
}

/// 消息总线契约
#[async_trait]
pub trait Bus: Send + Sync {
    /// 取得生产者句柄（同一 `Bus` 内可被多个任务共享）
    fn publisher(&self) -> Arc<dyn Publisher>;

    /// 创建订阅；同 `Bus` 内订阅名重复会被本地拒绝
    fn subscribe(
        &self,
        spec: SubscriptionSpec,
        handler: Arc<dyn MessageHandler>,
    ) -> Result<Arc<dyn Subscription>, MqError>;

    /// 健康快照
    fn health(&self) -> MqHealth;

    /// 运行态快照
    fn stats(&self) -> MqStats;

    /// 优雅关闭：停所有订阅 → flush 生产者 → 关闭客户端；幂等
    async fn close(&self) -> Result<(), MqError>;
}

/// 订阅规格
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionSpec {
    /// 逻辑 topic 名（不含 tenant / namespace）
    pub topic: String,
    /// 订阅名 = `MqConfig::subscription_name(purpose)`（禁止调用方手拼）
    pub subscription: String,
    pub sub_type: MqSubType,
    pub initial_position: MqInitialPosition,
    pub read_compacted: bool,
}

/// 进程级单例（生产路径唯一入口）
static BUS: OnceLock<Arc<dyn Bus>> = OnceLock::new();
/// 异步初始化锁：`OnceLock` 本身不支持 async init
static INIT_LOCK: Mutex<()> = Mutex::const_new(());

/// 进程级装配（幂等）：生产路径唯一入口。
///
/// - `PULSAR_ENABLED` 为假 → 内存实现；
/// - 为真时连接 broker；当前 P0 尚未接入驱动，`fail_closed` 返回
///   [`MqError::FeatureDisabled`]，`fail_open` 装配内部 `DegradedBus`；
/// - 重复调用返回已有实例（不会新建连接）。
pub async fn init(cfg: MqConfig) -> Result<Arc<dyn Bus>, MqError> {
    if let Some(bus) = BUS.get() {
        return Ok(Arc::clone(bus));
    }

    let _guard = INIT_LOCK.lock().await;
    if let Some(bus) = BUS.get() {
        return Ok(Arc::clone(bus));
    }

    let bus = from_config(cfg).await?;
    if BUS.set(Arc::clone(&bus)).is_err() {
        return BUS
            .get()
            .cloned()
            .ok_or_else(|| MqError::Config("消息总线初始化竞态".to_string()));
    }
    Ok(bus)
}

/// 只读获取：未初始化返回 `None`（供 `/healthz` 等无状态读取，不建连）。
pub fn try_bus() -> Option<Arc<dyn Bus>> {
    BUS.get().cloned()
}

/// 每次调用都新建客户端：仅供测试 / examples，**生产禁止**。
///
/// 重复调用会带来多份连接与 producer 命名压力（见设计文档 §5.2）。
#[allow(
    clippy::unused_async,
    reason = "P1 接入 Pulsar 驱动后需要 await 建连；保持签名稳定"
)]
pub async fn from_config(cfg: MqConfig) -> Result<Arc<dyn Bus>, MqError> {
    if cfg.enabled {
        // P0 只交付契约与内存实现；P1 在此分支接入 pulsar::PulsarBus::connect(cfg)。
        return match cfg.fail_mode {
            FailMode::FailOpen => {
                tracing::warn!(
                    instance = %cfg.instance_id,
                    "PULSAR_ENABLED=true 但 Pulsar 驱动尚未接入，装配 DegradedBus（fail_open）"
                );
                Ok(Arc::new(DegradedBus::new(cfg)))
            }
            FailMode::FailClosed => Err(MqError::FeatureDisabled),
        };
    }

    #[cfg(feature = "memory")]
    {
        tracing::info!(
            instance = %cfg.instance_id,
            scope = ?cfg.subscription_scope,
            "消息总线使用内存实现（PULSAR_ENABLED=false）"
        );
        Ok(Arc::new(memory::MemoryBus::new(cfg)))
    }
    #[cfg(not(feature = "memory"))]
    {
        Err(MqError::FeatureDisabled)
    }
}

/// 建连失败且 `fail_open` 时的降级实现：不 panic、不伪造成功。
struct DegradedBus {
    cfg: MqConfig,
    /// 直接以 `dyn` 形态持有：向上转型只在构造处发生一次。
    /// `Arc<T> as Arc<dyn Trait>` 不是 Rust 的 cast 语法（rustc 靠 unsize 隐式
    /// coercion 才接受），rust-analyzer 会报 `non-primitive cast`，故不用 `as`。
    publisher: Arc<dyn Publisher>,
}

impl DegradedBus {
    fn new(cfg: MqConfig) -> Self {
        Self {
            cfg,
            // 结构体字段已声明为 `Arc<dyn Publisher>`，此处由 unsize coercion 完成上转型
            publisher: Arc::new(DegradedPublisher),
        }
    }
}

#[async_trait]
impl Bus for DegradedBus {
    fn publisher(&self) -> Arc<dyn Publisher> {
        // 字段本身就是 `Arc<dyn Publisher>`：无需转型 / coercion
        Arc::clone(&self.publisher)
    }

    fn subscribe(
        &self,
        _spec: SubscriptionSpec,
        _handler: Arc<dyn MessageHandler>,
    ) -> Result<Arc<dyn Subscription>, MqError> {
        Err(MqError::Disabled)
    }

    fn health(&self) -> MqHealth {
        MqHealth {
            enabled: self.cfg.enabled,
            connected: false,
            brokers: 0,
            instance_id: self.cfg.instance_id.clone(),
            subscription_scope: self.cfg.subscription_scope,
        }
    }

    fn stats(&self) -> MqStats {
        MqStats::default()
    }

    async fn close(&self) -> Result<(), MqError> {
        Ok(())
    }
}

struct DegradedPublisher;

#[async_trait]
impl Publisher for DegradedPublisher {
    async fn publish(&self, _msg: Envelope) -> Result<Receipt, MqError> {
        Err(MqError::Disabled)
    }

    async fn flush(&self) -> Result<(), MqError> {
        Ok(())
    }
}
