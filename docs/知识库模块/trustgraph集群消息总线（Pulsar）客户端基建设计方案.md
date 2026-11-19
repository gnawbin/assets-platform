# trustgraph 集群消息总线（Pulsar）客户端基建设计方案

| 项 | 值 |
|---|---|
| 文档版本 | v0.2（设计稿） |
| 日期 | 2026-11-19 |
| 适用范围 | `apps/backend/src-tauri/crates/mq`（新增）；不涉及 `api` / `service` / `gateway` / `apps/web` |
| 状态 | 待评审；v0.2 补充「多客户端 / 多实例」语义（§5.2），评审通过后进入 P0 实施 |
| 关联文档 | `docs/知识库模块/trustgraph-lite轻量级信任图谱内核设计方案.md`（内核定位与零 IO 约束）、`docs/知识库模块/README.md`（索引）、`docs/业务网关（axum）多实例部署设计方案.md`（附录 D：PostgreSQL 单一依赖模式，本方案**不与之耦合**）、`TODO.md` |

> **结论先行**：本轮只交付「Pulsar **客户端基建**」——一套与业务无关的 `mq` crate（trait + 内存实现 + 可选 Pulsar 驱动 + 配置 + 测试）。
> **不接线任何业务**：不接入 `api` / `service` / `gateway` / `bootstrap`，不修改 `trustgraph-lite`，不产生任何运行时行为变化。
> 目标形态是未来「trustgraph 图集群」的节点间消息总线，但本轮**不实现集群节点**，只把客户端层做成可独立验证的底座。

---

## 0. 一页速读

1. **`trustgraph-lite` 保持零 IO**：其 `Cargo.toml` 只有 `serde` / `serde_json` / `thiserror`，`lib.rs` 明确「不触碰任何 IO」。Pulsar 客户端因此放在**独立 crate**，内核依赖图不变。
2. **默认零影响**：`pulsar` 是 **optional 依赖**，由 feature `pulsar` 门控且默认关闭；`cargo build --workspace` 默认**不会**引入 `pulsar` 与 `prost 0.13`。
3. **驱动版本锁定 `pulsar 6.9.1`**，`default-features = false` + `tokio-rustls-runtime-ring`：与本仓「rustls(ring) 单一 TLS 栈」一致，**排除** `async-std` 与 `native-tls`。
4. **不接 gateway**：网关的多实例协调仍走 PostgreSQL（附录 D），本方案不替换、不叠加监听器。
5. **至少一次 + 幂等**：`pulsar-rs` 无事务 / 无精确一次，信封携带 `id`（雪花）与 `seq`（分片内单调），消费端按幂等处理。`id` 必须在发布前生成一次，重试 / 多客户端复用同一 `id`；多副本的 `SNOWFLAKE_WORKER_ID` 必须互异。
6. **集群语义只做「预留」**：topic 命名、分区键、`KeyShared`、`MqInitialPosition::Earliest`、`read_compacted` 写进契约，但本轮不创建 topic、不实现图事件。
7. **多客户端有明确语义**：进程内只装配一个 `Bus`（`init()` 幂等、`from_config()` 仅测试/示例）；同一 `Bus` 内按订阅名去重；跨副本用 `PULSAR_SUBSCRIPTION_SCOPE=instance|shared` 显式选择「各自全量」还是「分摊」，默认 `instance`。

---

## 1. 范围与边界

### 1.1 本轮目标

| ID | 目标 | 验收信号 |
|---|---|---|
| M1 | 交付与驱动无关的消息总线契约 | `cargo test -p mq` 通过；`mq` 依赖树**不含** `pulsar` |
| M2 | 交付可选 Pulsar 驱动 | `cargo check -p mq --features pulsar` 通过 |
| M3 | 交付可直接连 broker 自证的示例 | `docker compose --profile mq up -d` 后 `examples/publish` + `examples/subscribe` 对跑 100 条 |
| M4 | 默认零行为变化 | 未设 `PULSAR_ENABLED` 时全仓不建立任何连接；Tauri 启动路径零改动 |
| M5 | 多客户端语义可验证 | `cargo test -p mq` 覆盖 `subscription_name(instance/shared)`、重复订阅拒绝、producer 单例 |

### 1.2 非目标（本轮明确不做）

| 项 | 原因 |
|---|---|
| 接入 `api` / `service` / `gateway` / `bootstrap` | 本轮只做基建，避免出现「无人消费的孤儿代码」以外的耦合 |
| 修改 `crates/trustgraph-lite` | 内核硬约束：零 IO、零外部驱动 |
| 定义图业务事件（`node.upsert` / `edge.delete` 等取值） | 内核 `model.rs` 仍是骨架（Phase 2 未开工），事件形状此时定死会返工 |
| 创建 / 管理 topic（分区数、保留策略、ACL） | 属运维面；Pulsar Admin 是独立 REST 接口，不引入 `admin-api` 特性（会拉 `native-tls`） |
| 集群选主、分片迁移、快照压缩作业 | 属未来「集群节点」层 |
| 实现新节点 `bootstrap` 引导流程 | 属集群节点层；本轮只把「两段式」语义写进契约（§6） |
| Exactly-once / 事务型 Producer | `pulsar-rs` 不支持 |
| 与网关 `LISTEN/NOTIFY` 合并或替换 | 两者服务不同场景，不交叉 |

### 1.3 与内核文档「分布式非目标」的关系

`docs/知识库模块/trustgraph-lite轻量级信任图谱内核设计方案.md:39` 把「分布式（Pulsar / Cassandra / Neo4j）」列为非目标，`:72` 写明外部依赖「仅 serde / serde_json / thiserror」。**本方案不推翻该结论**：

| 结论 | 是否受影响 |
|---|---|
| 内核是纯内存、零 IO、单进程库 | ❌ 不受影响（`crates/trustgraph-lite` 一行不改） |
| 内核不引入消息队列依赖 | ❌ 不受影响（总线在 `crates/mq`） |
| 「分布式」不在本轮范围 | ❌ 不受影响（本轮只做客户端基建，不做集群节点） |

> 换言之：**总线是内核的外部使用者，不是内核的一部分**。未来集群节点 = 「`mq` 客户端 + `trustgraph-lite` 内核」的组合体，内核依旧不知道 Pulsar 的存在。

---

## 2. 现状盘点

| 事实 | 位置 | 对本方案的影响 |
|---|---|---|
| 内核仅 Phase 1 骨架：`model.rs` 353 B、`graph.rs` 287 B，只有 doc 注释 | `crates/trustgraph-lite/src/` | 业务事件无法定义 → 本轮只做通用信封 |
| 内核零 IO 硬约束 | `crates/trustgraph-lite/src/lib.rs:3`、`:14-17`；`Cargo.toml:9-13` | 禁止把 `pulsar` 加进内核依赖 |
| 实施计划 Phase 2–5 全部待排期 | 内核文档 `:385-390` | 集群节点排在内核之后，本轮基建正好先行 |
| 全仓无 `pulsar` / `kafka` / `amqp` 依赖 | `Cargo.lock` | 无历史包袱，可自由选型 |
| 仓库已有「trait crate + 驱动实现」先例（`llm` / `vector` / `sandbox`） | 根 `Cargo.toml:45-68` | 本方案沿用其设计语言（本轮用 feature 门控，理由见 §4.2） |
| TLS 统一 `rustls` + `ring` provider | 根 `Cargo.toml:140-146` | 决定 Pulsar 的 feature 选择 |
| 网关跨实例协调走 PostgreSQL，明确不引入 Redis | `crates/gateway/src/coord.rs:3-4` | 本方案与网关**无任何交集** |

---

## 3. 技术选型与依赖

### 3.1 依赖声明（代码阶段写入根 `Cargo.toml`）

```toml
# ---- 消息总线客户端基建（可选驱动）----
# 业务 / 内核 crate 只依赖 mq（trait + 内存实现），不接触 pulsar。
mq = {path = "crates/mq"}

# pulsar 6.9.1 的 default 会同时拉起 async-std-runtime / native-tls / auth-oauth2，
# 与全仓「rustls(ring) 单一 TLS 栈」的约定冲突 → 必须 default-features = false。
# tokio-rustls-runtime-ring：与本文件的 rustls(ring) 同源，避免双 CryptoProvider 恐慌。
# compression：LZ4/Zlib/Zstd/Snappy；telemetry：client 内部日志走 tracing。
# 不开 auth-oauth2 / admin-api：两者都会强制引入 reqwest + native-tls。
pulsar = {version = "6.9.1", default-features = false, features = [
  "tokio-rustls-runtime-ring",
  "compression",
  "telemetry",
]}
```

### 3.2 依赖兼容性核验（对照现有 `Cargo.lock`）

| pulsar 6.9.1 要求 | 本仓现状 | 结论 |
|---|---|---|
| `rustls ^0.23.27` | 0.23.40 | ✅ 同一份 |
| `tokio ^1.45` | 1.54.1 | ✅ |
| `tokio-util ^0.7.15` | 0.7.20 | ✅ |
| `futures ^0.3.31` | 0.3.34 | ✅ |
| `nom ^7.1.3` | 7.1.3 | ✅ |
| `rand ^0.8.5` | 0.8.6 | ✅ |
| `webpki-roots ^1.0` | 1.0.7 | ✅ |
| `uuid` / `pem` / `crc` / `async-channel` / `async-trait` / `bytes` / `chrono` / `serde` | 均已存在 | ✅ |
| `prost ^0.13.4` | 现有 0.11.9 + 0.14.4 | ⚠️ 新增第三份（仅编译期影响） |
| `base64 ^0.22` | 现有 0.23.1 | ⚠️ 新增第二份（仅编译期影响） |

> 两处重复版本不改变任何运行时语义，且被 feature 门控在默认构建之外。

### 3.3 已核实的客户端能力（避免按「想当然」设计）

| 类型 | 6.9.1 实际定义 |
|---|---|
| 客户端 | `Pulsar::builder(url, TokioExecutor).with_auth(..).build().await` |
| 认证 | 根模块 `pulsar::Authentication`（`name` / `data` 结构体）；token 认证无需额外 feature |
| Producer | `Pulsar::producer().with_topic().with_name().with_options(ProducerOptions).build()` |
| Producer 发送 | `send`（等 receipt）/ `send_non_blocking` / `send_all` / `send_batch` / `create_message` / `check_connection` / `close` |
| `ProducerOptions` | `batch_size` / `batch_byte_size` / `batch_timeout` / `compression` / `access_mode` / `block_queue_if_full` / `metadata` / `schema` / `routing_policy` / `encrypted`（未实现） |
| Consumer | `Pulsar::consumer().with_topic().with_subscription().with_subscription_type(SubType).with_options(ConsumerOptions).build()`；`Consumer` 实现 `Stream` |
| `ConsumerOptions` | `priority_level` / `durable` / `start_message_id` / `metadata` / `read_compacted` / `schema` / `receiver_queue_size` / `initial_position` |
| 消费动作 | `ack(&msg)` / `nack(&msg)` / `seek` / `unsubscribe` / `close` / `check_connection` |
| 订阅类型 | `pulsar::proto::command_subscribe::SubType`（Exclusive / Shared / Failover / KeyShared） |
| 起始位置 | `InitialPosition::Latest`（默认）/ `Earliest`，或 `start_message_id` 指定消息位点 |
| 死信 | `DeadLetterPolicy { max_redeliver_count: usize, dead_letter_topic: String }` |
| producer 命名 | 同一 topic 内**唯一**；同名 producer 二次连接会被 broker 拒绝（`ProducerBusy`） |
| 订阅位点 | `initial_position` 仅在订阅**首次创建**时生效；已有订阅从 cursor 继续，不回放已 ack 历史 |
| KeyShared 无 key | 未设置 message key 的消息按轮询分摊，**不保证**同 key 保序 |
| **不支持** | 事务型 Producer、Exactly-once；`ConsumerOptions` **没有** `ack_timeout` / `unacked_message_redelivery_delay`（只能配在 broker 的 namespace policy） |

### 3.4 选型结论

| 备选 | 结论 | 理由 |
|---|---|---|
| `pulsar`（pulsar-rs） | ✅ 采用 | 纯 Rust、无 C++ 依赖、StreamNative 维护、支持 key_shared / 压缩 / TLS / 压缩位点读 |
| `pulsar-client`（C++ FFI） | ❌ | 需要 C++ 工具链与预编译库，破坏跨平台构建（Windows / Android 目标） |
| Kafka / NATS / RabbitMQ | ❌ | 目标场景已确定使用 Pulsar；换协议不减少工作量 |
| Redis Streams | ❌ | 会引入第三种中间件；`Last-Event-ID` 语义与 Pulsar durable cursor 不对等 |
| 自建 PG 轮询队列 | ❌ | 集群节点间高吞吐复制不适合轮询；且会把内核重新拉回 IO |

---

## 4. 架构与 crate 布局

### 4.1 分层

```text
┌──────────────────────────────────────────────┐
│ （未来）trustgraph 集群节点 / 其它使用方      │  ← 本轮不实现
└───────────────────┬──────────────────────────┘
                    │ 只依赖 trait
┌───────────────────▼──────────────────────────┐
│ crates/mq            trait + Envelope + 配置 │  零驱动（默认）
│  ├─ feature "memory"（默认） tokio::broadcast│  单机 / 单测
│  └─ feature "pulsar"（可选） PulsarBus       │  唯一持有 pulsar 依赖
└───────────────────┬──────────────────────────┘
                    │ 可选
┌───────────────────▼──────────────────────────┐
│ crates/trustgraph-lite（纯内存、零 IO）       │  ← 一行不改
└──────────────────────────────────────────────┘
```

### 4.2 为什么是「单 crate + 双 feature」而不是 `mq` + `mq-pulsar` 两个 crate

| 考量 | 单 crate（采用） | 双 crate |
|---|---|---|
| 默认编译影响 | `pulsar` / `prost 0.13` **不进**默认构建 | 两 crate 都是 workspace 成员，`cargo build --workspace` 会编译 pulsar |
| 空壳问题 | 无 | `mq-pulsar` 若不默认编译，就只是「需要额外 feature 才非空」的壳 |
| 未来第二驱动 | 出现 Kafka 时再按 `vector-*` 模式拆，对外 API 不变 | — |
| 依赖方向约束 | 由 feature 白名单保证 | 由 crate 边界保证（更强） |

> 若评审偏好严格的 crate 边界，可切换为 `crates/mq` + `crates/mq-pulsar`，代价是默认 workspace 构建多编译一棵 protobuf 代码树；对外 API 与本文档其余内容不变。

### 4.3 文件布局

```text
crates/mq/
├── Cargo.toml          # features: default=["memory"]；P0 pulsar=[]（P1 改 dep:pulsar）
├── src/
│   ├── lib.rs          # 对外 API + crate 级文档（依赖方向 / 用法 / 风险）
│   ├── envelope.rs     # Envelope / TopicName / Receipt / Inbound
│   ├── error.rs        # MqError（thiserror）
│   ├── config.rs       # MqConfig::from_env()
│   ├── memory.rs       # feature "memory"
│   ├── retry.rs        # 指数退避纯函数
│   └── pulsar.rs       # feature "pulsar"（P1）
├── examples/
│   ├── publish.rs      # required-features = ["pulsar"]
│   └── subscribe.rs    # required-features = ["pulsar"]
└── tests/
    ├── envelope.rs     # 无 broker
    ├── config.rs       # subscription_name / producer_name 契约
    ├── memory.rs       # 多客户端语义（重复订阅 / fan-out / close）
    └── pulsar_e2e.rs   # PULSAR_TEST_URL 存在才跑（P1）
```

### 4.4 crate 清单（`crates/mq/Cargo.toml` 目标形态）

```toml
[package]
edition = "2024"
name = "mq"
version = "0.1.0"
description = "消息总线客户端基建（trait + 内存实现 + 可选 Pulsar 驱动）"

[features]
default = ["memory"]
memory = []
# P0 先占位（不引入依赖）；P1 接入驱动时改为 ["dep:pulsar"]
pulsar = []

[dependencies]
async-trait.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tokio.workspace = true
tracing.workspace = true
# P1 再登记 optional 依赖：
# pulsar = {workspace = true, optional = true}

[[example]]
name = "publish"
required-features = ["pulsar"]

[[example]]
name = "subscribe"
required-features = ["pulsar"]
```

---

## 5. 公共 API 契约

```rust
// ===== crates/mq/src/envelope.rs =====
/// 消息信封：跨版本唯一契约。
/// payload 用 JSON 而非 prost 生成类型，避免「升级 pulsar / prost 就要重新生成消息结构」的耦合。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// 幂等键：雪花 ID（与 utils::snowflake::next_id 同源）
    pub id: String,
    /// 逻辑 topic 名（不含 tenant / namespace，便于多环境复用）
    pub topic: String,
    /// 分区键 / message key：同一 key 的事件在 KeyShared 订阅下保序。
    /// 由发布方写入（未来集群复制传 `node_id`）；订阅侧不持有该字段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partition_key: Option<String>,
    /// 业务事件类型，如 "graph.node.upsert"（未来集群复制用；本轮不定义取值）
    pub kind: String,
    /// 多租户隔离（消费端据此选 schema）
    pub tenant_id: Option<i64>,
    /// 分片内单调序号（消费端幂等水位；客户端只透传，不做解释）
    pub seq: Option<i64>,
    /// 毫秒时间戳
    pub created_at: i64,
    /// 链路追踪 ID（与 tracing / OTel 对齐）
    pub trace_id: Option<String>,
    /// 信封版本；消费端按版本分支，实现平滑演进
    pub schema_version: u16,
    pub payload: serde_json::Value,
}

/// `persistent://{tenant}/{namespace}/{name}` 的构造与校验（纯函数，可单测）
pub struct TopicName { /* tenant, namespace, name */ }

impl TopicName {
    /// 逻辑名必须是 `[a-z0-9][a-z0-9._-]*`，长度 ≤ 200（Pulsar 限制内收敛）
    pub fn build(tenant: &str, namespace: &str, logical: &str) -> Result<String, MqError>;
    pub fn parse(uri: &str) -> Result<Self, MqError>;
}

/// 投递回执：broker 侧消息标识
pub struct Receipt {
    pub topic: String,
    pub message_id: String,
}

// ===== crates/mq/src/lib.rs =====
#[async_trait]
pub trait Publisher: Send + Sync {
    /// 等 broker receipt；失败按 `PULSAR_PUBLISH_RETRY_MAX` 指数退避重试
    async fn publish(&self, msg: Envelope) -> Result<Receipt, MqError>;
    /// 冲刷攒批（退出前调用）
    async fn flush(&self) -> Result<(), MqError>;
}

#[async_trait]
pub trait MessageHandler: Send + Sync {
    /// 返回 Ok → ack；Err → nack（由 broker 重投，超 `max_redeliver_count` 进 DLQ）
    async fn handle(&self, msg: Inbound) -> Result<(), MqError>;
}

/// 消费入参：信封 + broker 侧属性只读快照
pub struct Inbound {
    pub envelope: Envelope,
    pub attributes: BTreeMap<String, String>,
}

#[async_trait]
pub trait Subscription: Send + Sync {
    /// 触发退出并等待 consumer 关闭（幂等）
    async fn shutdown(&self) -> Result<(), MqError>;
}

/// 订阅作用域（来自 `PULSAR_SUBSCRIPTION_SCOPE`，见 §5.2）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionScope {
    /// 每实例独立订阅，各自全量（默认）
    Instance,
    /// 跨副本共享订阅，自动分摊
    Shared,
}

#[derive(Debug, Clone)]
pub struct MqHealth {
    pub enabled: bool,
    pub connected: bool,
    pub brokers: usize,
    /// 实例标识：多客户端 / 多副本排障定位
    pub instance_id: String,
    /// 当前订阅作用域（instance / shared，见 §5.2）
    pub subscription_scope: SubscriptionScope,
}

/// 运行态快照：多客户端 / 多订阅排障用
#[derive(Debug, Clone, Default)]
pub struct MqStats {
    /// 已建立的 producer 数（按 topic 去重）
    pub producers: usize,
    /// 活跃订阅数
    pub subscriptions: usize,
    /// 攒批中尚未收到 receipt 的消息数
    pub pending_publishes: u64,
}

#[async_trait]
pub trait Bus: Send + Sync {
    fn publisher(&self) -> Arc<dyn Publisher>;
    fn subscribe(
        &self,
        spec: SubscriptionSpec,
        handler: Arc<dyn MessageHandler>,
    ) -> Result<Arc<dyn Subscription>, MqError>;
    fn health(&self) -> MqHealth;
    fn stats(&self) -> MqStats;
    /// 优雅关闭：停所有订阅 → flush 生产者 → 关闭客户端；幂等
    async fn close(&self) -> Result<(), MqError>;
}

/// 订阅类型（mq 自持枚举：默认 `memory` feature 下不引用 pulsar 类型）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MqSubType {
    Exclusive,
    Shared,
    Failover,
    KeyShared,
}

/// 订阅首次创建时的起始位置；已有订阅从 cursor 继续，不受它影响
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MqInitialPosition {
    Latest,
    Earliest,
}

/// 订阅规格
#[derive(Debug, Clone)]
pub struct SubscriptionSpec {
    pub topic: String,
    /// 订阅名 = `MqConfig::subscription_name(purpose)`（见 §5.2）
    pub subscription: String,
    pub sub_type: MqSubType,
    pub initial_position: MqInitialPosition,
    pub read_compacted: bool,
}

// ===== 装配入口（crates/mq/src/lib.rs）=====

/// 进程级装配（幂等）：生产路径唯一入口。
/// - `PULSAR_ENABLED` 为假 → 内存实现；
/// - 为真且有 `pulsar` feature → 连接 broker；
/// - 重复调用返回已有实例（不会新建连接）。
pub async fn init(cfg: MqConfig) -> Result<Arc<dyn Bus>, MqError>;

/// 只读获取：未初始化返回 `None`（供 `/healthz` 等无状态读取，不建连）。
pub fn try_bus() -> Option<Arc<dyn Bus>>;

/// 每次调用都新建客户端：仅供测试 / examples，**生产禁止**。
/// 重复调用会带来多份连接与 producer 命名压力（见 §5.2）。
pub async fn from_config(cfg: MqConfig) -> Result<Arc<dyn Bus>, MqError>;
```

### 5.1 Pulsar 驱动实现要点

```rust
// 客户端（唯一建连点）
let client: Pulsar<_> = Pulsar::builder(&cfg.url, TokioExecutor)
    .with_auth(Authentication { name: "token".into(), data: token.into_bytes() })
    .build().await?;

// 生产者：关键路径用 send（等 receipt）；队列满阻塞而非静默丢弃
ProducerOptions {
    batch_size: Some(cfg.batch_size),
    batch_timeout: Some(Duration::from_millis(cfg.batch_delay_ms)),
    compression: cfg.compression,          // 默认 Lz4
    block_queue_if_full: true,
    ..Default::default()
}

// 消费者：KeyShared + 死信 + 可选最早位点/压缩读
ConsumerOptions {
    initial_position: cfg.initial_position,
    read_compacted: Some(cfg.read_compacted),
    receiver_queue_size: Some(cfg.receiver_queue_size),
    ..Default::default()
}
DeadLetterPolicy {
    max_redeliver_count: cfg.max_redeliver,
    dead_letter_topic: dlq_topic,
}

// 消费循环：select 同时处理消息与退出信号
loop {
    tokio::select! {
        biased;
        _ = shutdown.recv() => { consumer.close().await?; break; }
        next = consumer.try_next() => match next? {
            Some(msg) => match handler.handle(inbound(&msg)).await {
                Ok(()) => consumer.ack(&msg).await?,
                Err(e) => {
                    tracing::warn!(error = %e, "消息处理失败，nack 重投");
                    consumer.nack(&msg).await?;
                }
            },
            None => break,
        }
    }
}
```

```rust
// producer 池：同 topic 只建一次；并发首发给 OnceCell 做 single-flight
let cell = producers
    .entry(topic.clone())
    .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
    .clone();
let producer = cell
    .get_or_try_init(|| async {
        client
            .producer()
            .with_topic(&topic)
            .with_name(producer_name(&cfg, &topic)) // instance / pid / bus_seq / topic_hash
            .with_options(producer_options(&cfg))
            .build()
            .await
    })
    .await?;

// 订阅注册表：同进程同订阅名二次 subscribe 直接拒绝，
// 避免 broker 侧把消息静默分摊给两个 handler
if subscriptions.contains_key(&spec.subscription) {
    return Err(MqError::AlreadySubscribed {
        subscription: spec.subscription,
    });
}
```

约定：

1. **只用 `send`** 走关键路径（`send_non_blocking` 在攒批下返回早于落 broker，不作为「发布成功」依据）；
2. `Envelope` 实现 `SerializeMessage`（`serde_json::to_vec`）与 `DeserializeMessage`（`serde_json::from_slice`）；
3. 同时把 `trace_id` / `tenant_id` / `schema_version` 冗余写入 Pulsar `properties`，便于 broker 侧排查；
   `Envelope.partition_key` 通过 message key 写入（`MessageBuilder::with_key`），**不是** property；
4. 退出必须 `flush()` + `close()`，避免攒批丢失；
5. **producer 命名必须唯一**：`{PULSAR_PRODUCER_NAME}-{instance_id}-{pid}-{bus_seq}-{topic_hash}`。
   Pulsar 的 producer 名在 topic 内唯一，同名二次连接会被 broker 拒绝（§3.3）；
6. **producer 复用 + single-flight**：`DashMap<完整 topic, Arc<OnceCell<Producer>>>`，禁止每条消息新建 producer；
7. **订阅注册表**：同进程同订阅名二次 `subscribe` 返回 `AlreadySubscribed`；同名但类型 / 位点不同返回 `SubscriptionConflict`；
8. **关闭顺序固定**：`Bus::close()` = 停订阅（`shutdown().await`）→ `flush()` → `client.close()`，幂等，并用 `PULSAR_OP_TIMEOUT_SECS` 做超时兜底；
9. **类型映射只在驱动内**：`MqSubType` / `MqInitialPosition` 在 `pulsar.rs` 内映射为 `pulsar::proto::command_subscribe::SubType` 与 `InitialPosition`，`mq` 的公共 API 不出现 pulsar 类型。

### 5.2 多客户端与多实例语义

「多个客户端」在本方案里分四层，规则各不相同：

| 层级 | 实体 | 创建点 | 多份时的后果 | 本方案处理 |
|---|---|---|---|---|
| L1 | Pulsar 连接客户端（`pulsar::Pulsar`） | `Pulsar::builder().build()` | 每个 broker 多一条连接 + 一次 TLS 握手 | 由 L2 单例收敛，不直接暴露 |
| L2 | `Bus` 实例 | `mq::init(cfg)` / `from_config(cfg)` | 多连接、producer 命名压力、无法统一关闭 | **生产只调 `init()`**（幂等）；`from_config()` 仅供测试 / examples |
| L3 | producer / consumer | `Bus::publisher()` / `Bus::subscribe()` | producer 按 topic 复用；consumer 按订阅名在 broker 侧聚合 | producer 池 + 订阅注册表 |
| L4 | 跨进程副本 | 各进程独立 `init()` | 订阅名相同 = 分摊；不同 = 广播 | `PULSAR_SUBSCRIPTION_SCOPE` 显式选择 |

#### 5.2.1 同一 `Bus` 上多次 `subscribe()`

| 条件 | broker 行为 | 业务表现 |
|---|---|---|
| 不同 `purpose` → 不同订阅名 | 两个独立订阅 | 各自收到**全量**消息（fan-out），符合预期 |
| 同 `purpose` + `Shared` / `KeyShared` | 同一订阅下两个 consumer | 消息被**分摊**，每个 handler 只看到一部分 |
| 同 `purpose` + `Exclusive` | 第二个 consumer 被拒 | `ConsumerBusy`，`subscribe` 返回错误 |
| 同订阅名但 `sub_type` / `initial_position` 不同 | broker 拒绝（订阅类型不可变） | 报错信息不直观 |
| 同订阅名 + `earliest` | 仅订阅**首次创建**时生效 | 不回放已 ack 历史 |

> 本方案的选择：**同 `Bus` 内由订阅注册表拒绝重复订阅**（启动期报错），而不是让 broker 静默分摊一半消息。

#### 5.2.2 跨进程 / 多副本

| `PULSAR_SUBSCRIPTION_SCOPE` | 订阅名 | 适用场景 | 行为 |
|---|---|---|---|
| `instance`（默认） | `{prefix}-{instance_id}-{purpose}` | 单机、桌面端、需要每实例全量（审计 / 本地缓存） | 每副本独立订阅，**各自全量消费** |
| `shared` | `{prefix}-{purpose}` | K8s 多副本、trustgraph 集群节点 | 同名订阅自动分摊，**每条消息只被处理一次** |

约束与注意事项：

1. `shared` 只解决**跨进程分摊**，不解决「同进程多 Bus」；同进程由 `init()` 单例 + 订阅注册表保证。
2. 同机多进程必须让 `instance_id` 互异：显式 `PULSAR_INSTANCE_ID`，或使用不同的 `SNOWFLAKE_WORKER_ID`（后者本身也是雪花唯一性的前置条件）。
3. 切换 `scope` / `prefix` 会生成**新订阅名**，旧 cursor 留在 broker：新订阅按 `initial_position` 重新开始，需要引导时走 §6 的两段式流程。
4. 启动时 `info!` 打印 `instance_id` / `subscription_scope` / 解析后的订阅名，便于确认「分摊还是广播」。
5. `shared` + `KeyShared` 下新增 consumer 只接收其分到的 key 范围，**不会**补齐其他 consumer 已 ack 的历史。

```rust
// crates/mq/src/config.rs —— 订阅名唯一解析入口，调用方禁止手拼
impl MqConfig {
    pub fn subscription_name(&self, purpose: &str) -> Result<String, MqError> {
        validate_purpose(purpose)?; // ^[a-z0-9][a-z0-9._-]{0,63}$
        Ok(match self.subscription_scope {
            SubscriptionScope::Instance =>
                format!("{}-{}-{}", self.subscription_prefix, self.instance_id, purpose),
            SubscriptionScope::Shared =>
                format!("{}-{}", self.subscription_prefix, purpose),
        })
    }
}

// crates/mq/src/pulsar.rs —— 即使两个 Bus 配置完全相同也不会撞名（pid + 进程内自增）
fn producer_name(cfg: &MqConfig, full_topic: &str) -> String {
    format!(
        "{}-{}-{}-{}-{:08x}",
        cfg.producer_name,
        cfg.instance_id,
        std::process::id(),
        next_bus_seq(),          // 进程内 AtomicU64
        fnv1a(full_topic),       // 轻量 hash，不引新依赖
    )
}
```

---

## 6. 信封与 topic 规范（集群语义只做预留）

| 维度 | 约定 | 本轮状态 |
|---|---|---|
| topic 形态 | `persistent://{tenant}/{namespace}/{logical}` | 契约已定，不建 topic |
| logical 命名 | `tg.{cluster_id}.graph.{shard}`（未来） | 仅文档预留 |
| 分区键 | `node_id`（同一节点的事件保序） | `Envelope.partition_key` → message key；订阅侧不再持有 |
| 订阅类型 | `KeyShared`（多节点分摊 + 按 key 保序） | 已支持配置（`MqSubType::KeyShared`） |
| 新节点加入 | 两段式：独立 `bootstrap` 订阅（`earliest` / `read_compacted`）拉全量 → 追平后切共享订阅收增量 | 机制已写进契约，本轮不实现节点 |
| 幂等 | `id` + `seq` + 消费端水位；`id` 发布前生成一次，多副本 `worker_id` 互异 | 字段已预留 |
| 死信 | `{topic}{dlq_suffix}`，默认后缀 `.dlq` | 已支持配置 |
| 交付语义 | 至少一次（at-least-once） | 明确写入契约 |

> 本轮**不定义** `kind` 的取值集合、不定义 `payload` 结构、不创建任何 topic —— 这些等内核 Phase 2–5 完成后由《trustgraph 图集群设计方案》定义。

> ⚠️ **既有订阅不回放**：`initial_position` 只在订阅**首次创建**时生效。节点加入一条已经消费过的 `shared` 订阅，只能拿到加入之后的增量，无法补齐其他节点已 ack 的历史；历史必须走独立的 `bootstrap` 订阅（`earliest` 或压缩 topic 的 `read_compacted`）。
>
> ⚠️ **订阅名决定语义**：`instance` / `shared` 与 `{prefix}-{purpose}` 的解析规则见 §5.2；切换 scope / prefix 等于换订阅，旧 cursor 不会迁移。

---

## 7. 配置项（全部环境变量，缺省即关闭）

| 变量 | 默认 | 说明 |
|---|---|---|
| `PULSAR_ENABLED` | 未设置 = 关 | 关闭时装配内存实现，不建立连接 |
| `PULSAR_URL` | — | `pulsar://host:6650` / `pulsar+ssl://`；多 broker 逗号分隔 |
| `PULSAR_TENANT` / `PULSAR_NAMESPACE` | `public` / `default` | 拼完整 topic URI |
| `PULSAR_AUTH` | `none` | `none` / `token` |
| `PULSAR_TOKEN` | — | token 认证串（**不落日志**，与 `JWT_SECRET` 同待遇） |
| `PULSAR_TLS_CA_FILE` / `PULSAR_TLS_VERIFY_HOSTNAME` | 空 / `true` | 私有 CA 场景 |
| `PULSAR_CONNECT_TIMEOUT_SECS` / `PULSAR_OP_TIMEOUT_SECS` | `10` / `30` | 建连 / 操作超时 |
| `PULSAR_PRODUCER_NAME` | `assets` | 最终名 = `{name}-{instance_id}-{pid}-{bus_seq}-{topic_hash}`（broker 侧定位） |
| `PULSAR_INSTANCE_ID` | `{hostname}-{worker_id}` | 实例标识；同机多进程 / 容器同名时必须显式覆盖 |
| `PULSAR_BATCH_SIZE` / `PULSAR_BATCH_DELAY_MS` | `100` / `10` | 攒批 |
| `PULSAR_COMPRESSION` | `lz4` | `none` / `lz4` / `zlib` / `zstd` / `snappy` |
| `PULSAR_SUBSCRIPTION_PREFIX` | `assets` | 业务前缀（不含实例信息）；订阅名由 `MqConfig::subscription_name(purpose)` 生成 |
| `PULSAR_SUBSCRIPTION_SCOPE` | `instance` | `instance`（每实例独立全量）/ `shared`（跨副本分摊）；见 §5.2 |
| `PULSAR_SUBSCRIPTION_TYPE` | `key_shared` | `exclusive` / `shared` / `failover` / `key_shared`；仅默认值，`SubscriptionSpec` 可按 `purpose` 覆盖 |
| `PULSAR_INITIAL_POSITION` | `latest` | `latest` / `earliest` |
| `PULSAR_READ_COMPACTED` | `false` | 压缩 topic 直读「每 key 最新值」 |
| `PULSAR_RECEIVER_QUEUE_SIZE` | `1000` | 消费端预取队列 |
| `PULSAR_MAX_REDELIVER` / `PULSAR_DLQ_SUFFIX` | `5` / `.dlq` | 死信阈值与后缀 |
| `PULSAR_PUBLISH_RETRY_MAX` | `3` | 发布重试次数（指数退避） |
| `PULSAR_FAIL_MODE` | `fail_closed` | `fail_closed` 返回错误 / `fail_open` 装配 `DegradedBus` 并仅 WARN（`publish` 仍返回 `Err`，不伪造成功） |

> 订阅名解析：`instance` → `{prefix}-{instance_id}-{purpose}`；`shared` → `{prefix}-{purpose}`。启动日志会打印解析结果；切换 scope / prefix 会生成新订阅（旧 cursor 不迁移）。
>
> ⚠️ `ack_timeout` 与「未 ack 重投间隔」**不在客户端配置**（pulsar-rs 6.9.1 的 `ConsumerOptions` 无对应字段），只能在 broker 的 namespace policy 上设置。运维改环境变量不会生效。

读取方式对齐 `crates/gateway/src/config.rs`：`MqConfig::from_env()` 一次性读入，热路径不再 `std::env::var`。

---

## 8. 关键行为

| 场景 | 行为 |
|---|---|
| 建连失败 | `init()` / `from_config()` 返回 `MqError::Connect`；**不 panic、不阻塞调用方**（由使用方决定降级） |
| 断连重连 | 依赖 pulsar-rs 内建指数退避重连；`health()` 反映 `connected` |
| 发布失败 | 按 `publish_retry_max` 重试；仍失败则按 `fail_mode` 返回错误或 WARN |
| 处理失败 | nack → 重投；超过 `max_redeliver_count` → 投递到 DLQ |
| 攒批丢失风险 | 退出统一走 `Bus::close()`：停订阅 → `flush()` → `client.close()`；禁止直接 drop |
| 反压 | `receiver_queue_size` 限制预取；`block_queue_if_full = true` 让生产端阻塞而非丢弃 |
| 顺序性 | 仅 `KeyShared` + `Envelope.partition_key`（message key）保证「同 key 有序」；**不承诺全局有序** |
| 多客户端（同进程） | 生产只调 `init()`：进程内一个 `Bus`；`from_config()` 仅测试 / examples，重复调用会产生多连接 |
| 同 `Bus` 重复订阅 | 同订阅名二次 `subscribe` → `AlreadySubscribed`；同名不同类型 → `SubscriptionConflict` |
| 多副本 `shared` | 相同订阅名 + `Shared`/`KeyShared` → 自动分摊，每条消息只处理一次 |
| 多副本 `instance` | 订阅名含 `instance_id` → 各副本独立全量消费（审计 / 本地缓存） |
| 新节点加入 | 既有订阅不回放已 ack 历史；必须走独立 `bootstrap` 订阅（earliest / read_compacted）再切共享订阅 |

---

## 9. 错误模型

```rust
#[derive(Debug, thiserror::Error)]
pub enum MqError {
    #[error("消息总线未启用（PULSAR_ENABLED 未设置）")]
    Disabled,
    #[error("能力未编译：请启用 crate feature `pulsar`")]
    FeatureDisabled,
    #[error("配置非法：{0}")]
    Config(String),
    #[error("建连失败：{0}")]
    Connect(String),
    #[error("发布失败（topic={topic}）：{detail}")]
    Publish { topic: String, detail: String },
    #[error("订阅失败（topic={topic}）：{detail}")]
    Subscribe { topic: String, detail: String },
    #[error("信封序列化 / 反序列化失败：{0}")]
    Codec(String),
    #[error("topic 非法：{0}")]
    InvalidTopic(String),
    #[error("订阅已存在（同进程重复订阅）：{subscription}")]
    AlreadySubscribed { subscription: String },
    #[error("订阅配置冲突（同名但类型 / 位点不同）：{subscription}")]
    SubscriptionConflict { subscription: String },
    #[error("消息总线已关闭")]
    Closed,
}
```

约定：`mq` 的 `MqError` **不实现** `Into<api::ApiError>`（避免 trait crate 反向依赖 `api`）；未来接线时由使用方在边界处转换。

> `fail_open` 语义：建连失败不 panic，装配 `DegradedBus`（`health().connected = false`），`publish` 仍返回 `Err(MqError::Disabled)` 且只 WARN 一次；**不返回假的 `Ok`**，避免调用方误以为消息已发出。

---

## 10. 观测性

| 项 | 方案 |
|---|---|
| 日志 / 追踪 | 启用 pulsar `telemetry` feature，client 内部日志并入 `tracing`（与 `utils::logging` 同一 subscriber）；mq 自身日志统一带 `mq.instance` / `mq.subscription` / `mq.topic` 字段 |
| Span | 发布：`tracing::debug_span!("mq.publish", messaging.system="pulsar", messaging.destination=..)`；消费：`mq.consume` + `messaging.message_id` |
| 属性命名 | 对齐 OTel Messaging 语义约定，便于现有 `otel-collector-config.yaml` 直接接收 |
| 健康 | `Bus::health()` 返回 `{enabled, connected, brokers, instance_id, subscription_scope}`；`Bus::stats()` 返回 `{producers, subscriptions, pending_publishes}`；本轮**不挂到** `/healthz`（零接线），由未来使用方接入 |
| 脱敏 | `PULSAR_TOKEN` 不打印；日志只打 scheme + broker 数量，不打完整 URL 的凭据部分 |

---

## 11. 测试方案

| 层级 | 内容 | 命令 |
|---|---|---|
| 单测（无 broker） | 信封序列化 / `partition_key` 兼容（缺省字段）/ 版本兼容 / `TopicName` 解析（合法与非法：大写、空、超长、`//`、`;`）/ 退避曲线 / 内存实现投递与背压 | `cargo test -p mq` |
| 多客户端单测 | `subscription_name`（`instance` 含 `instance_id`、`shared` 不含）；`purpose` 非法值；同 `Bus` 重复订阅拒绝；同 Bus 并发首发同一 topic 只建 1 个 producer；`close()` 幂等 | `cargo test -p mq` |
| 编译验证 | Pulsar 驱动在无 broker 环境下可编译 | `cargo check -p mq --features pulsar` |
| 集成（可选） | 发布→消费→ack；nack 重投；超限进 DLQ；断连重连；优雅退出不丢消息 | `docker compose --profile mq up -d` → `PULSAR_TEST_URL=... cargo test -p mq --features pulsar --test pulsar_e2e` |
| 多客户端集成 | `shared` 两 Bus 处理计数之和 = N；`instance` 两 Bus 各收 N；两 Bus producer 名不冲突；`bootstrap` 订阅收到历史而常驻订阅只收增量 | 同上 |
| 零影响验证 | 默认依赖树不含 pulsar | `cargo tree -p mq`（不得出现 `pulsar` / `prost v0.13`） |
| 手测 | 示例对跑 100 条 + 观察 `health()` | `cargo run -p mq --features pulsar --example publish` / `subscribe` |

集成测试默认跳过（`PULSAR_TEST_URL` 不存在时直接 return），保证 CI 无 broker 也能全绿。

---

## 12. 实施计划与交付清单

### 12.1 阶段

| 阶段 | 内容 | 工作量 | 验收信号 |
|---|---|---|---|
| **P0** | `crates/mq` 骨架：`Cargo.toml`（features 门控）+ `lib.rs`（`init` / `try_bus` / `from_config` / `Bus::close`）/ `envelope.rs`（含 `partition_key`）/ `error.rs`（`AlreadySubscribed` 等）/ `config.rs`（`SubscriptionScope` / `instance_id` / `subscription_name()`）/ `memory.rs` / `retry.rs` + 根 `Cargo.toml` 登记 + `tests/envelope.rs` | 0.75 天 | `cargo test -p mq` 通过；`cargo tree -p mq` 无 pulsar；`subscription_name` 两分支单测通过 |
| **P1** | `src/pulsar.rs`（Publisher / Subscriber / producer 池 + single-flight / 订阅注册表 / 重连 / 死信 / 优雅退出 / token 认证）+ `examples/{publish,subscribe}.rs` + `tests/pulsar_e2e.rs`（含多客户端用例）+ `docker-compose.yml` 增 `pulsar`（profile `mq`，standalone） | 1.25 天 | 对跑 100 条不丢；kill 客户端后自动重连；`shared` 分摊计数 = N；`instance` 各自全量 |
| **P2** | 本文档 v0.2（多客户端 / 多实例语义）+ `docs/知识库模块/README.md` 索引 + 内核文档非目标补充引用 + `TODO.md` 条目 | 0.5 天 | 文档链接可达、条目编号不冲突 |

### 12.2 变更文件清单（代码阶段）

| 文件 | 动作 |
|---|---|
| `apps/backend/src-tauri/Cargo.toml` | 追加 `mq` 与 `pulsar`（`[workspace.dependencies]`） |
| `apps/backend/src-tauri/crates/mq/**` | 新增（见 §4.3） |
| `docker-compose.yml` | 追加 `pulsar` 服务（`profiles: ["mq"]`，默认 `up` 不启动） |
| `apps/backend/src-tauri/Cargo.lock` | 随依赖自动更新（新增 `prost 0.13` / `base64 0.22` 等） |

### 12.3 明确不改

`crates/trustgraph-lite/**`、`crates/api/**`、`crates/service/**`、`crates/gateway/**`、`crates/bootstrap/**`、`apps/backend/src-tauri/src/**`、`apps/web/**`。

---

## 13. 风险与权衡

| ID | 风险 | 影响 | 缓解 |
|---|---|---|---|
| R1 | 与内核文档「分布式非目标」被误读为已推翻 | 架构定位混淆 | §1.3 显式声明；内核文档只加引用、不删非目标 |
| R2 | `prost 0.13` 成为第三份 | 编译时间与体积增加 | `pulsar` optional + feature 默认关；`cargo tree -p mq` 纳入验收 |
| R3 | 误开 `native-tls` / `aws-lc-rs` | 双 TLS 栈、双 CryptoProvider、Windows 走 schannel | feature 白名单写死在根 `Cargo.toml` 注释里 |
| R4 | 期望客户端能配 ack 超时 / 重投间隔 | 运维调参无效 | §7 末尾显式说明只能配 broker namespace policy |
| R5 | 期望精确一次 / 事务 | 重复消费导致图数据错乱 | 至少一次 + `id` / `seq` 幂等，写入契约 |
| R6 | 内核 Phase 2–5 未开工，本轮代码短期无消费者 | 可能变「孤儿代码」 | 以 `examples` + 集成测试自证；`TODO.md` 标注「待集群节点落地」 |
| R7 | 若误把 feature 设为默认，`cargo build --workspace` 变慢 | CI 时间上升 | `default = ["memory"]`；CI 增加一条 `cargo tree` 断言 |
| R8 | 驱动升级（6.x → 7.x）破坏 API | 返工 | 驱动隔离在 `src/pulsar.rs` 单文件内，其余代码只依赖 trait |
| R9 | 集群节点实现时发现信封字段不足（如缺 `partition` 路由信息） | 需要改契约 | `schema_version` + JSON payload 允许扩展；必要时新增可选字段（不破坏旧消费端） |
| R10 | 默认 `scope=instance` 导致多副本重复消费 | 重复写图 / 重复计费 | 集群部署必须显式 `PULSAR_SUBSCRIPTION_SCOPE=shared`；启动日志打印解析后的订阅名；§5.2 表格化 |
| R11 | 同机多进程 `instance_id` 相同 → 订阅名相同 | 两个进程静默分摊消息 | 显式 `PULSAR_INSTANCE_ID`；`SNOWFLAKE_WORKER_ID` 必须互异；启动期打印 `instance_id` |
| R12 | 新节点加入既有订阅无法回放 | 图状态缺失 | 两段式引导：`bootstrap` 订阅（`earliest` / `read_compacted`）→ 追平后切共享订阅 |
| R13 | 同进程误用 `from_config` 建多个 `Bus` | 多连接 + producer 命名压力 | 生产只暴露 `init()`；`from_config` 文档标注测试专用；producer 名含 `pid` / `bus_seq` |

---

## 14. 附录 A：未采纳的备选方案

| 备选 | 未采纳原因 |
|---|---|
| 把 Pulsar 直接写进 `trustgraph-lite` | 违反内核「零 IO / 零驱动」硬约束，且让内核无法在 Tauri 中独立复用 |
| 采用 `pulsar-client`（C++ 桥接） | 需要 C++ 工具链与预编译库，破坏 Windows / Android 构建 |
| 用 Redis Streams 承载集群复制 | 引入第三种中间件；且 durable cursor / key_shared 能力不对等 |
| 先做业务事件（`graph.node.upsert`）再做客户端 | 内核 `model.rs` 仍是骨架，事件形状必返工 |
| 本轮就接 `/healthz` 与启动初始化 | 与「零接线」目标冲突，且桌面端无意义 |
| `admin-api` 特性（Pulsar Admin REST） | 强制引入 `reqwest + native-tls`；需要建 topic 时用本仓已有 reqwest(rustls) 直连 Admin REST |
| 进程内允许多个 `Bus` 并存（不做单例） | 多连接 + producer 命名压力 + 无法统一关闭；多客户端语义会退化成「每个 Bus 各自一套订阅」 |
| 多副本默认 `shared`（不分 scope） | 单机多进程调试时互相抢消息；`instance` / `shared` 必须在配置上显式可选 |

---

## 15. 附录 B：未来集群拓扑（仅预留，不实现）

```text
节点 A ──publish──► persistent://public/default/tg.{cluster}.graph.{shard}
                          │  Envelope.partition_key = node_id（message key）
                          │  subscription = {prefix}-graph-replicate
                          │    scope = shared + KeyShared（多节点分摊）
                          │  新节点引导：{prefix}-{node_id}-bootstrap（earliest）
节点 B ◄──consume─────────┘
  └─ 应用到本地 trustgraph-lite 内核（内核不感知 Pulsar）
```

待内核 Phase 2–5 完成后，另立《trustgraph 图集群设计方案》定义：事件形状、分片规则、快照压缩作业、节点加入回放、故障接管。

---

## 16. 变更记录

| 日期 | 版本 | 变更 |
|---|---|---|
| 2026-11-18 | v0.1 | 首版：确定「只做客户端基建」范围、`pulsar 6.9.1` + rustls(ring) 选型、单 crate 双 feature 布局、信封与配置契约 |
| 2026-11-19 | v0.2 | 补充「多客户端 / 多实例」语义（§5.2）：进程内 `Bus` 单例、producer 池 + 订阅注册表、`PULSAR_SUBSCRIPTION_SCOPE`；`partition_key` 移入 `Envelope`；`Bus::close` / `MqStats` / 新错误变体；修正新节点回放边界 |
