# apalis 异步任务队列开发文档

| 项 | 内容 |
|---|---|
| 文档版本 | V1.0（设计稿） |
| 最后更新 | 2026-11-26 |
| 适用代码 | `apps/backend/src-tauri`（Rust workspace，edition 2024，`members = ["crates/*"]`） |
| 目标 crate | `crates/task-queue`（P0 已建骨架：`Cargo.toml` / `src/lib.rs` / `README.md`） |
| apalis 基线 | `apalis 1.2.0`、`apalis-postgres 1.2.0`、`apalis-cron 1.2.0`、`apalis-workflow 1.2.0`、`apalis-board 1.3.0` |
| 当前状态 | **P1/P2/P3 已实现**：契约+worker、`task_ticket` 进度表与 HTTP 接口、`apalis-cron` 维护、bootstrap/gateway 接线；`apalis-board` 因上游不兼容改为独立镜像，`apalis-workflow` 分阶段链路延后（见 §11 / §12 / §13） |
| 相关文档 | `docs/文档解析后向量化数据缓存方案.md`、`docs/Rust原生多模态解析+pgvector多模态RAG设计文档_V1.7整合版.md`、`docs/业务网关（axum）多实例部署设计方案.md` |

---

## 1. 概述

### 1.1 要解决的问题

资产平台里已经有一批「分钟级」任务被压在 HTTP 请求里执行。它们的共同特征是：耗时不
可控、依赖外部进程（ffmpeg / ffprobe / Python 侧车）、失败后需要重试、且与客户端连接
生命周期强耦合：

| 场景 | 典型耗时 | 现状代码 | 现状问题 |
|---|---|---|---|
| 视频解析（抽帧 → ASR → OCR → 汇总） | 1~20 分钟 | `service/src/doc_parser.rs` 内同步调用，`reqwest` 超时被放宽到 **600s**（第 73~80 行） | 网关/反代先超时；客户端断开后结果无人接收；实例重启即丢 |
| 聊天附件解析（视频/音频/图片/文档） | 十秒~分钟级 | `service/src/conversation_service.rs:311-341`，请求内「下载 S3 → 解析 → 帧转 data URL」 | 一条消息最坏占用一个连接 10 分钟，且无法重试 |
| 大文档解析 + 分块向量化 | 秒~分钟级 | `service/src/rag_service.rs:159` `chunk_and_vectorize` 在写入路径内串行 | 用户保存即阻塞；中途失败留下半截分片 |
| 后台清理（OAuth / 会话 / 审计） | 秒级但需定时 | `api/src/lib.rs:944` `tokio::spawn` + `sleep`；网关侧靠 advisory lock 选主 | 重启丢任务、无重试、无进度、无重放 |

### 1.2 判定标准：什么样的任务必须入队

满足**任意一条**即应走队列，而不是留在请求内：

1. p95 耗时 > 3s，或耗时随文件大小线性增长且无上界；
2. 需要拉起外部进程（ffmpeg / ffprobe / Python `apps/doc-parser`）或独占 CPU/GPU；
3. 失败后需要自动重试、断点续跑、或人工重放（例如 ASR 模型 OOM）；
4. 需要跨实例分摊（多副本 gateway），或必须与请求生命周期解耦（客户端断连不影响结果）。

### 1.3 非目标（明确边界，避免重复造轮子）

| 不做 | 交给谁 |
|---|---|
| 实时 token 流式输出 | `crates/gateway/src/sse.rs` + `llm-openai` / `llm-anthropic` 流式响应 |
| 审批 / 会签 / 人工节点的业务流程 | `crates/workflow`（BPMN + adk-graph） |
| 进程内事件广播（缓存失效、SSE 分发） | `crates/mq` + `gateway/src/worker.rs` 的 `LISTEN/NOTIFY` |
| 定时容器/主机采集 | `crates/ops` / `crates/ops-exec` 现有调度 |
| 分布式锁 / 选主 | PostgreSQL advisory lock（`gateway/src/coord.rs`），本方案不替换 |

### 1.4 术语

| 术语 | 含义 |
|---|---|
| 任务（Task / Job） | 一条持久化记录（`apalis.jobs` 一行），含参数、状态、重试次数、执行结果 |
| 队列（Queue / Queue name） | 任务的逻辑分组，如 `parse`、`vectorize`、`maintenance` |
| worker | 常驻消费者，按队列拉取任务并执行 |
| backend / storage | apalis 中的任务存储实现，本方案用 `apalis-postgres` 的 `PostgresStorage` |
| 生产者侧 | `api` / Tauri command，只做 `push`，不执行解析 |
| 消费侧 | `crates/task-queue` 中装配的 worker，跑在 gateway 或桌面进程内 |
| tenant（租户 schema） | 任务参数必须携带 schema 名，worker 用它恢复 `database::with_schema` 上下文 |

---

## 2. 现状与差距

### 2.1 现有基础设施能力矩阵

| 能力 | `tokio::spawn`（`api/src/lib.rs:944`） | `crates/mq` | `crates/workflow` | `apalis` |
|---|---|---|---|---|
| 任务持久化 | ❌ 进程内，重启即丢 | ❌ 内存实现；Pulsar 仅事件流 | ⚠️ 依赖业务表，非任务表 | ✅ PG 表 + 事务 |
| 自动重试 / 退避 | ❌ 手写 | ❌ | ⚠️ 业务自定义 | ✅ `RetryPolicy` + `AbortError` / `RetryAfterError` |
| 超时 / 限流 / 熔断 | ❌ | ❌ | ❌ | ✅ tower layer |
| 跨实例分摊 | ❌ 每个实例都跑 | ⚠️ `Shared` 订阅可分摊 | ❌ | ✅ `FOR UPDATE SKIP LOCKED` |
| 长任务锁续期 | ❌ | ❌ | ❌ | ✅ heartbeat + `long_running()` |
| 孤儿任务回收 | ❌ | ❌ | ❌ | ✅ `ResumeAbandoned` / `Vacuum` |
| 进度 / 结果查询 | ❌ | ❌ | ⚠️ 业务表 | ✅ `FetchById` + `WaitForCompletion` |
| 看板 / 人工重放 | ❌ | ❌ | ❌ | ✅ `apalis-board` |
| 定时任务 | ⚠️ 手写 `sleep` 循环 | ❌ | ⚠️ cron 节点 | ✅ `apalis-cron` |

### 2.2 为什么选 PostgreSQL 后端，而不是 Redis

1. **现有架构的硬约束**：`crates/gateway/src/lib.rs:1-10` 明确「多实例四件事全部用
   PostgreSQL 实现，不引入 Redis」；`apalis-postgres` 与 `apalis-redis` 是平级可选项，
   选前者不增加新的运维组件。
2. **版本天然对齐**：根 `Cargo.toml` 已锁定 `sqlx = 0.9.1`，`apalis-postgres 1.2.0`
   依赖 `sqlx ^0.9`，不会解析出两份 sqlx（避免重复编译与 trait 不兼容）。
3. **事务一致性**：业务写库与入队可以用同一个连接池/事务完成，避免「业务成功但任务丢失」。
4. **代价**：轮询会比 Redis 多几条轻量查询；`PostgresStorageFactory` 支持共享
   `LISTEN/NOTIFY`（`with_pubsub()`）降低空轮询开销，P1 视压测结果决定是否启用。

### 2.3 结论

新增 `crates/task-queue` 作为**唯一**的任务契约与 worker 装配层；`api` / Tauri 只做
生产者（`push`），`gateway` / 桌面负责启动 worker。`crates/mq` 与 `crates/workflow` 保持
职责不变。

---

## 3. apalis 生态与版本

### 3.1 版本基线（2026-11 docs.rs 实测）

| crate | 版本 | 作用 | 本期是否引入 |
|---|---|---|---|
| `apalis` | 1.2.0 | 门面 crate：re-export `apalis-core`，提供 `prelude` 与常用 layer | P1 |
| `apalis-core` | 1.2.0 | `Backend` / `TaskSink` / `WorkerBuilder` / `Monitor` / `layers` | 间接（由 `apalis` re-export） |
| `apalis-postgres` | 1.2.0 | `PostgresStorage` / `PostgresStorageFactory`（`LISTEN/NOTIFY` 共享轮询） | P1 |
| `apalis-cron` | 1.2.0 | `CronScheduler`（基于 `cron` crate，六段表达式） | P2 |
| `apalis-workflow` | 1.2.0 | `SteppedFlow`（顺序）/ `GraphFlow`（DAG） | P2 |
| `apalis-board` | 1.3.0 | 看板总入口（`axum` / `actix` / `salvo` / `web` 四个 feature） | P2 |
| `apalis-board-api` | 1.3.0 | axum/actix/salvo 路由 + SSE 日志 | 间接 |
| `apalis-board-types` | 1.3.0 | 看板 DTO（`RawTask` 等） | 间接 |
| `apalis-board-web` | 1.3.0 | Leptos 写的 UI（由 `apalis-board` 内嵌） | 间接 |

> 你手里那份「README 表格」（`apalis-board-types` / `apalis-board-api` / `apalis-board-web`
> 平铺、`apalis-core 🔁 📅`）是 **1.0 时代**的列表，仍然成立但已不是入口；现在统一从
> `apalis` 1.2 + `apalis-board` 1.3 引入。

### 3.2 1.0 → 1.2 关键变更（照抄老资料会踩的坑）

| 旧写法 | 1.2 现状 | 影响 |
|---|---|---|
| `apalis-sql`（feature `postgres`） | 拆成独立的 `apalis-postgres` / `apalis-mysql` / `apalis-sqlite` | Cargo.toml 不能再写 `apalis = { features = ["postgres"] }` |
| `CronStream` | `CronScheduler`（`apalis-cron`） | 定时任务示例需重写 |
| `WorkerBuilder::concurrency(n)` | **不存在**；并发靠「同一 backend 起 N 个 worker」或 `ParallelizeExt::parallelize` | 并发模型章节按此设计 |
| `push` 直接塞进 `Storage` | 统一走 `TaskSink`：`push` / `push_task` / `push_bulk` / `push_all` | `push(args)` 走 `Into<Args>`；要保留 `max_attempts` 等元数据必须用 `push_task` |
| 迁移在 `public` | 1.0 起 `apalis-postgres` 的迁移与历史表放在 **`apalis` schema**（`apalis._sqlx_migrations`） | 备份、权限、排查路径都要改，见 §14 |
| 内置 `pgcrypto` | 1.2 不再使用 | 老库升级需一次性 `ALTER TABLE public._sqlx_migrations SET SCHEMA apalis` |

### 3.3 核心抽象速查

| 抽象 | 位置 | 用途 |
|---|---|---|
| `Backend` / `BackendConfig` | `apalis_core::backend` | 任务来源与队列配置（`queue` / `batch_size`） |
| `TaskSink<Args, Kind>` | `apalis_core::backend` | 生产者入口，`Kind = Durable｜Ephemeral` |
| `WorkerBuilder` | `apalis_core::worker::builder` | 固有方法仅 `new` / `backend` / `data` / `layer` / `chain` / `build` |
| 扩展 trait | `apalis_core::worker::ext` | `EventListenerExt`(`on_event`)、`LongRunningExt`(`long_running`)、`ParallelizeExt`、`CircuitBreaker`、`AcknowledgementExt` |
| `Monitor` | `apalis_core::monitor` | 多 worker 统一注册 / 优雅关闭 / 指标 |
| 错误语义 | `apalis-core` 文档「Error Handling」 | `AbortError`（不可重试）/ `RetryAfterError`（延时重试）/ `DeferredError`（立即重试） |

### 3.4 feature 选用

```toml
apalis = { version = "1.2.0" }        # 默认已含 tracing / retry / timeout / limit / catch-panic / config
apalis-postgres = { version = "1.2.0", default-features = false, features = ["migrate", "tokio-comp", "task-tracking"] }
apalis-cron = { version = "1.2.0" }
apalis-workflow = { version = "1.2.0" }
apalis-board = { version = "1.3.0", default-features = false, features = ["axum"] }   # 严禁用默认特性
```

- **`apalis-board` 必须 `default-features = false`**：它同时支持 `actix` / `salvo`，默认特性
  会把两套 Web 框架拖进依赖树，与现有 axum 网关冲突。
- `apalis-postgres` 的 `migrate` 是默认特性，但显式声明更安全（后续若裁剪默认特性不会静默失效）。
- `task-tracking` 开启后才能在看板里看到细粒度 worker / task 明细。

---

## 4. 总体架构

```
┌──────────────────────── 生产者（请求侧，毫秒级返回） ─────────────────────────┐
│  axum handler / Tauri command                                                │
│      │  1. 业务校验 + 落业务表（同一事务，asset.status = parsing）             │
│      │  2. task_queue::producer::push(ParseTask { tenant, asset_id, .. })     │
│      ▼                                                                       │
│   TaskSink::push_task(TaskBuilder::new(task).max_attempts(3).build())         │
└───────────────────────────────────┬──────────────────────────────────────────┘
                                    │ INSERT INTO apalis.jobs (queue='parse', Pending)
                                    ▼
                     ┌──────────────────────────────────┐
                     │  PostgreSQL（单一依赖，不引 Redis）│
                     │   apalis.jobs / apalis.workers    │
                     │   业务表：asset / document_chunk … │
                     └──────┬────────────────────┬──────┘
     FOR UPDATE SKIP LOCKED     │                    │  LISTEN/NOTIFY（进度 / 失效广播）
                ┌───────────────▼───┐                │
                │ worker（每实例 N 个）│                │
   ┌────────────┴───────────┬───────┴────────────────┐│
   │ gateway 实例 A          │ gateway 实例 B          ││
   │ WorkerBuilder::new(     │ WorkerBuilder::new(     ││
   │   "parse-{instance}")   │   "parse-{instance}")   ││
   └────────────┬────────────┴────────────┬───────────┘│
                ▼                          ▼            │
      database::with_schema(task.tenant)                │
                │                                       │
      ┌─────────┴──────────┬──────────────┬────────────┴──────┐
      ▼                    ▼              ▼                   ▼
  storage::s3        parser::parse_file  DocParserClient   rag_service::chunk_and_vectorize
  （下载 / 预签名）    （本地 PDF/Office） （Python 侧车：视频/ASR/OCR） （分块入库）
      └────────────────────┴──────────────┴───────────────────┘
                                    │
                          回写业务表 + 进度事件
```

### 4.1 请求时序（视频解析）

```
Client              api handler                 PostgreSQL              worker
  │  POST /api/knowledge/parse │                   │                      │
  ├───────────────────────────►│                   │                      │
  │                            │ 落 asset(parsing)  │                      │
  │                            ├──────────────────►│                      │
  │                            │ push ParseTask     │                      │
  │                            ├──────────────────►│ jobs(queue=parse)    │
  │  202 { taskId, assetId }   │◄──────────────────┤                      │
  │◄───────────────────────────┤                   │                      │
  │  GET /api/tasks/{taskId}   │                   │ SKIP LOCKED 取任务    │
  │  （或 SSE 订阅进度）        │                   │◄─────────────────────┤
  │                            │                   │ with_schema 执行解析  │
  │                            │                   │◄─────────────────────┤
  │                            │                   │ 回写文本/分片 + 进度   │
  │                            │                   │◄─────────────────────┤
  │  200 { status: completed } │                   │ ack                  │
```

### 4.2 队列划分

| 队列 | 内容 | 并发基线 | 备注 |
|---|---|---|---|
| `parse` | 文档 / 图片 / 音频解析 | 每实例 1~2 | 与视频隔离，避免长任务饿死短任务 |
| `parse-video` | 视频（下载 → 抽帧 → ASR → 汇总） | 每实例 1 | 可独立扩容 / 独立限流 |
| `vectorize` | 分块 + embedding + 入库 | 每实例 2 | 幂等（按 `chunk_index` upsert），可安全重试 |
| `maintenance` | 清理、一致性校验、索引重建 | 每实例 1 | 与 `apalis-cron` 配合，且只由 leader 注册 |

### 4.3 任务状态流转

```
Pending ──► Running ──► Completed
   ▲            │
   │            ├──► Failed   （重试次数耗尽，等人工重放 / 修复后 ResumeById）
   │            └──► Retrying （按 RetryAfter 退避后回到 Pending）
   └───────────────────────────┘
        ResumeAbandoned / Vacuum：实例崩溃后回收孤儿任务
```

> `apalis-postgres` 自带 heartbeat 与孤儿任务重新入队；P2 阶段用 `apalis-cron` 每天扫一次
> `ResumeAbandoned`，兜住「进程被杀且租约已过期」的极端情况。

---

## 5. crates/task-queue 设计

### 5.1 目标目录结构（P1/P2 完成后）

```
crates/task-queue/
├── Cargo.toml            # 版本来源：根 [workspace.dependencies]
├── README.md             # 索引 + 快速上手（已更新到 P1）
└── src/
    ├── lib.rs            # 模块文档 + 再导出 + start/start_with_sink/start_with_deps
    ├── config.rs         # TaskQueueConfig::from_env + Mode（inline/queue/queue_fail_open）      ✅
    ├── error.rs          # TaskError → apalis 的「可重试 / AbortError」映射                    ✅
    ├── job.rs            # ParseTask / VectorizeTask 契约、DocType 路由、幂等键、参数校验        ✅
    ├── producer.rs       # setup（建 apalis schema）+ push_parse / push_vectorize              ✅
    ├── worker.rs         # spawn_all、解析/向量化执行体、with_schema 包裹、ParseResultSink、信号量 ✅
    ├── flow.rs           # P2：apalis-workflow 视频链路（未创建）
    └── board.rs          # P2：apalis-board 子路由（未创建）
```

### 5.2 模块职责

| 模块 | 职责 | 允许依赖 | 明确不做 |
|---|---|---|---|
| `config.rs` | 读环境变量、给默认值、集中做「开关语义」 | `std` | 不读数据库、不建连接池 |
| `job.rs` | 任务参数契约与版本演进 | `serde` / `chrono` | 不含业务逻辑，不查库 |
| `producer.rs` | 入队（`TaskSink`）+ 失败降级 | `apalis*` / `sqlx` | 不执行解析、不写业务表 |
| `worker.rs` | 装配 worker、恢复租户上下文、错误分类 | `apalis*` / `parser` / `service` / `storage` / `database` | 不做 HTTP 响应、不碰 `axum` |
| `flow.rs` | 多阶段解析链路（视频） | `apalis-workflow` | 不替代 `crates/workflow` 的业务流 |
| `board.rs` | 看板子路由装配 | `apalis-board`(axum) | 不引入 actix/salvo |

### 5.3 依赖方向

```
            ┌──────────────┐
            │ api / gateway│  （只管 push 与启动 worker）
            └──────┬───────┘
                   │ 依赖
            ┌──────▼───────┐
            │  task-queue  │
            └──┬───┬───┬───┘
               │   │   └────────────► storage（S3 下载 / 预签名）
               │   └────────────────► service（doc_parser 客户端、rag_service）
               └────────────────────► database（with_schema / 连接池）、parser
```

**禁止**：`task-queue → api / gateway / bootstrap`。原因有两条：

1. `api` 已经是「路由 + 鉴权 + 响应形状」层，worker 不需要 HTTP 语义；
2. 桌面形态（Tauri）不能把 `gateway`（axum 多实例协调）拖进二进制。

### 5.4 与现有模块的边界

| 现有模块 | 谁调谁 | 说明 |
|---|---|---|
| `crates/parser` | `task-queue` → `parser` | 本地 PDF/Office 解析，仍走 `spawn_blocking` |
| `service::doc_parser` | `task-queue` → `service` | 视频/音频走 Python 侧车（`DOC_PARSER_BACKEND` 语义不变） |
| `storage::s3` | `task-queue` → `storage` | 任务只存 `s3://bucket/key`，worker 内下载到临时目录（RAII 清理） |
| `service::rag_service` | `task-queue` → `service` | 分块/向量化仍复用现有实现，不重写 |
| `crates/mq` | 不直接依赖 | 进度事件可按既有 `NOTIFY` 通道广播，避免多引入一层抽象 |
| `crates/workflow` | 不依赖 | BPMN 业务流与本方案的队列是正交的（§12.4） |

---

## 6. Cargo 接入

### 6.1 根 `apps/backend/src-tauri/Cargo.toml`

P0 已在「内部业务 crate」段落登记（第 78~80 行）：

```toml
# 异步任务队列（apalis）：视频 / 音频 / 文档解析等耗时任务。
# P0 只交付骨架与 docs/apalis异步任务队列开发文档.md，依赖在 P1 打开。
task-queue = {path = "crates/task-queue"}
```

P1 再在 `[workspace.dependencies]` 中追加第三方依赖（**唯一版本来源**，成员禁止写 version）：

```toml
# ---- 异步任务队列（apalis）----
# 只选 PostgreSQL 后端：与 gateway「单一 PG 依赖」策略一致，且 sqlx 0.9 与工作区同版本。
apalis = "1.2.0"
# migrate：启动时建 apalis schema；tokio-comp：sqlx 运行时（不可省）；
# task-tracking：看板需要细粒度 worker/task 明细。
apalis-postgres = {version = "1.2.0", default-features = false, features = [
  "migrate",
  "tokio-comp",
  "task-tracking",
]}
apalis-cron = "1.2.0"
apalis-workflow = "1.2.0"
# 严禁默认特性：默认会拉 actix-web / salvo，与现有 axum 网关冲突。
apalis-board = {version = "1.3.0", default-features = false, features = ["axum"]}
```

### 6.2 成员 `crates/task-queue/Cargo.toml`

```toml
[package]
description = "异步任务队列（apalis）：视频 / 音频 / 文档解析等耗时任务的契约与装配"
edition = "2024"
name = "task-queue"
version = "0.1.0"

[lints]
workspace = true

[dependencies]
apalis.workspace = true
apalis-postgres.workspace = true
database.workspace = true
parser.workspace = true
serde.workspace = true
serde_json.workspace = true
service.workspace = true
sqlx.workspace = true
storage.workspace = true
thiserror.workspace = true
tokio.workspace = true
tracing.workspace = true
uuid.workspace = true
```

### 6.3 三条硬约束（来自根 Cargo.toml 顶部注释）

1. `[workspace.dependencies]` **不允许 `optional`** —— 需要 optional 的依赖只能在成员 crate
   本地声明（例如 `parser` 的 `image`），因此 apalis 相关依赖一律非 optional，靠 feature 裁剪；
2. 成员的 `features` 只能在本表基础上**追加**，不能削减；
3. `default-features` 默认只在根定义一次；只有两个 crate 需求相反时才在成员侧覆盖
   （edition 2024 / Cargo 1.99+ 支持）。

> 版本实测提醒：`apalis-board 1.3.0` 与 `apalis 1.2.0` 的大版本不同轨，P2 引入前先跑一次
> `cargo tree -i apalis-core` 确认只有一份 `apalis-core`；若出现两份，优先降 `apalis-board`
> 到与 `apalis-core 1.2` 兼容的版本，**不要**用 `[patch]` 硬拼。

---

## 7. 任务契约

### 7.1 公共信封字段（所有任务类型共享）

| 字段 | 类型 | 必填 | 作用 |
|---|---|---|---|
| `tenant` | `String` | ✅ | 租户 schema 名，worker 用它恢复 `database::with_schema`；**空值直接判失败** |
| `dedupe_key` | `Option<String>` | — | 幂等键：同文件重复提交只跑一次（§7.5） |
| `trace_id` | `Option<String>` | — | 与前端请求的 trace/request id 对齐，便于串联日志 |
| `version` | `u8` | ✅ | 契约版本，默认 1；只增字段不改语义（§7.6） |

### 7.2 `ParseTask`（P1 首个任务类型）

```rust
// crates/task-queue/src/job.rs（示意，P1 实现）
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocType {
    Pdf,
    Office,
    Image,
    Audio,
    Video,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseTask {
    /// 租户 schema，worker 内 database::with_schema 的唯一依据（禁止为空）
    pub tenant: String,
    /// 业务主键：知识资产 id
    pub asset_id: i64,
    /// 对象存储引用（s3://bucket/key）；禁止本机绝对路径 / 临时目录 / 长期凭证
    pub storage_url: String,
    pub file_name: String,
    /// 路由依据：video/audio 进 parse-video 队列，其余进 parse
    pub doc_type: DocType,
    /// 解析选项，语义沿用 Python `/parse` 的 options（skip_index / frame_interval 等）
    #[serde(default)]
    pub options: serde_json::Value,
    /// 幂等键：sha256(tenant + asset_id + file_md5 + options)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    /// 契约版本
    #[serde(default = "default_version")]
    pub version: u8,
}

fn default_version() -> u8 {
    1
}
```

### 7.3 `VectorizeTask`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorizeTask {
    pub tenant: String,
    pub asset_id: i64,
    /// 解析产出的文本（视频场景可能很大 → 超过 64KB 时改为只传 result_ref）
    pub text: String,
    pub title: String,
    pub okf_type: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub tree_node_id: Option<i64>,
    pub dedupe_key: Option<String>,
    pub trace_id: Option<String>,
    #[serde(default = "default_version")]
    pub version: u8,
}
```

### 7.4 `TaskBuilder` 元数据规范（已实现，见 `src/producer.rs`）

| 元数据 | `parse` / `parse-video` | `vectorize` | 说明 |
|---|---|---|---|
| `task_id` | `uuid v4` 字符串 | 同左 | 业务侧 task id，与 `TaskId::from_string(..)` 一致，便于日志/进度串联 |
| `idempotency_key` | `parse:{tenant}:{asset_id}:{md5}` | `vectorize:{tenant}:{asset_id}` | 去重键，落 `apalis` 任务的幂等列 |
| `max_attempts` | 文档 3 / 音视频 2 | 5 | 解析多为 IO / 外部进程抖动，重试有效；embedding 失败多为限流 |
| `priority` | 文档 2 / 音视频 1 | 2 | 保证短任务先出结果 |
| `run_after` | 未使用 | 未使用 | 需要削峰时由调用方显式设置（`TaskBuilder::run_after`） |

> ⚠️ 实测注意事项（apalis 1.2.0）：
> - `TaskBuilder` **没有** `timeout()` 方法 —— 任务超时要用 worker 层（`apalis::layers` 的超时 layer）
>   或任务体内 `tokio::time::timeout` 自行实现；
> - `WorkerBuilder` **没有** `concurrency()` —— 并发靠「同一队列起 N 个 worker」（本 crate 已按此实现）；
> - `TaskSink::push(args)` 会重新生成元数据，**要保留 `max_attempts` / `idempotency_key` 必须用
>   `push_task(task)`**。

### 7.5 幂等与去重

1. 生产者计算 `dedupe_key = sha256(tenant + asset_id + file_md5 + options_json)`；
2. 入队前查业务表 `task_ticket`：存在「同 dedupe_key 且状态 ∈ {Pending, Running, Retrying}」→
   直接返回既有 `task_id`，不再入队；
3. worker 侧保持**业务幂等**：解析结果按 `asset_id` upsert，分片按 `(asset_id, chunk_index)`
   upsert，因此即使重复执行也不会产生脏数据；
4. 失败态（`Failed`）不参与去重，允许重新提交（人工修复文件后重试）。

### 7.6 契约演进规则

- **只增不改**：新增字段一律 `#[serde(default)]`，禁止改字段名/语义/类型；
- worker 必须能反序列化 **N-1 版本**的旧任务（线上 `apalis.jobs` 里可能积压）；
- 破坏性变更走新任务类型（`ParseTaskV2`）或新队列，不做「原地升级」；
- 每次改契约同步更新本节与 `crates/task-queue/README.md` 的字段表。

---

## 8. 生产者侧

### 8.1 落点（已接线）

| 入口 | 位置 | 现状 |
|---|---|---|
| HTTP（新增） | `crates/api/src/task_routes.rs` | `POST /api/tasks/parse` → `202`；`GET /api/tasks/{taskId}` → 读 `task_ticket` ✅ |
| HTTP（已有） | `crates/api/src/knowledge_routes.rs` / `upload_routes.rs` | 仍走原有同步语义，**未改**（避免影响现有前端联调） |
| 桌面 | `apps/backend/src-tauri`（`bootstrap::serve_http`） | 与 HTTP 同生命周期启动 worker（`inline` 时零副作用）✅ |
| 多实例 | `crates/gateway/src/lib.rs` `serve()` | 启动 worker、退出时优雅停止 ✅ |
| 内部 | `service::rag_service` 的向量化 | 大批量导入可改用 `push_vectorize`（已提供，调用方按需切换） |

### 8.2 入队封装（已实现，见 `src/producer.rs`）

```rust
// crates/task-queue/src/producer.rs（节选）
pub async fn push_parse(
    pool: &PgPool,
    task: ParseTask,
    cfg: &TaskQueueConfig,
) -> Result<String, TaskError> {
    task.validate()?;
    if !cfg.queue_enabled() {
        return Err(TaskError::Invalid("任务队列未开启（TASK_QUEUE_MODE=inline）".into()));
    }
    if task.doc_type.is_heavy() && !cfg.video_enabled {
        return Err(TaskError::Invalid("视频/音频队列未开启".into()));
    }

    // 幂等键 / 队列 / 重试次数都必须在 move 之前算好
    let idempotency_key = task.idempotency_key();
    let queue = task.queue();                                   // parse | parse-video
    let max_attempts = if task.doc_type.is_heavy() { 2 } else { 3 };
    let priority = if task.doc_type.is_heavy() { 1 } else { 2 };

    let mut storage: PostgresStorage<ParseTask> = PostgresStorage::new(pool)
        .with_config(Config::default().queue(queue).batch_size(cfg.batch_size));

    let task_id = uuid::Uuid::new_v4().to_string();
    let built = TaskBuilder::new(task)
        .task_id(TaskId::from_string(task_id.clone()))
        .idempotency_key(idempotency_key)
        .max_attempts(max_attempts)
        .priority(priority)
        .build();

    storage
        .push_task(built)
        .await
        .map_err(|e| TaskError::Transient(format!("入队失败（queue={queue}）: {e}")))?;

    Ok(task_id)
}
```

> ⚠️ `TaskSink::push_task` 返回 `Result<(), TaskSinkError<_>>`（**不返回任务 id**），
> 所以业务 task id 必须由我们先生成（`uuid::Uuid::new_v4()`）并写进 `TaskBuilder::task_id(..)`，
> 这样 `apalis` 表与业务侧日志/进度才能对上。

### 8.3 HTTP 语义

| 场景 | 状态码 | 响应体 |
|---|---|---|
| 入队成功 | `202 Accepted` | `{ taskId, assetId, status: "pending" }` |
| 幂等命中（同 dedupe_key 已有活跃任务） | `200 OK` | `{ taskId: 既有 id, status: "pending" }` |
| 队列不可用且不允许降级 | `503` | `{ code: "TASK_QUEUE_UNAVAILABLE" }` |
| 队列不可用但允许降级 | `202` | 走 inline 同步执行（仅 `TASK_QUEUE_MODE=inline` 或 `fail_open`） |

### 8.4 进度查询（已实现）

```text
POST /api/tasks/parse          → 202 { taskId, assetId, status: "pending" }
GET  /api/tasks?status=&limit= → 200 [ { taskId, taskType, queue, assetId, status, stage,
                                        progress, attempts, error?, createdAt, startedAt?, finishedAt? } ]
GET  /api/tasks/{taskId}       → 200 同上单条
```

前端入口：
- **任务中心页** `/tasks`（`apps/web/src/app/tasks/page.tsx`）：列表 + 状态筛选 + 5s 自动刷新 + 进度条 + 展开看错误；
- **知识资产页**「解析」按钮（`apps/web/src/app/knowledge-asset/page.tsx`）：对已绑定文件的资产一键入队；
- 菜单：`sys_menu` id=240（页面）+ 241~243（权限点），超管立即可见，普通角色需在「设置 → 权限」勾选。

数据来源：租户 `task_ticket`（`task_queue::ticket::{get,list}`）。字段与实现：

| 字段 | 来源 | 说明 |
|---|---|---|
| `status` | `pending` / `running` / `completed` / `failed` | worker 在开始 / 完成 / 失败时更新 |
| `stage` | `queued` / `downloading` / `parsing` / `chunking` / `done` | 每个阶段推进一次（`Progress::stage`） |
| `progress` | 0~100 | 10 / 20 / 40 / 80 / 100 的粗粒度 |
| `attempts` | `attempts + 1` | 每次 `mark_running` 自增，便于判断是否在重试 |
| `error` | 最近一次失败原因 | 截断 4KB，不含凭证 |

> 运维排查仍可看 `apalis.jobs`（任务/重试/耗时），但**前端只读 `task_ticket`** —— 避免把
> 基础设施表结构暴露成对外契约。

### 8.5 与 SSE / `NOTIFY` 的对接

- worker 每完成一个阶段执行 `SELECT pg_notify('<channel>', payload)`（复用 gateway 既有通道
  命名规则，见 `crates/gateway/src/worker.rs`）；
- 前端订阅既有 SSE 端点即可，**不新增一套推送机制**；
- 推送 payload 只含 `{taskId, status, progress}`，不含文件内容与路径。

### 8.6 降级

| 开关 | 行为 |
|---|---|
| `TASK_QUEUE_MODE=inline`（默认） | 保持现状：请求内同步解析，`push` 分支不生效 |
| `TASK_QUEUE_MODE=queue` | 正常入队；库/队列异常直接 503（宁可失败也不退化成 10 分钟长连接） |
| `TASK_QUEUE_MODE=queue_fail_open` | 入队失败时回退 inline，并打 `WARN` + 计数（用于灰度首周） |

---

## 9. 消费者侧（worker）

### 9.1 启动位置

| 形态 | 启动点 | 说明 |
|---|---|---|
| 网关（多实例） | `crates/gateway/src/lib.rs` `serve()` 第 4 步之后 | 已有 `shutdown_tx/rx`（`watch::channel`），worker 直接复用；`instance_id` 用作 worker 名前缀 |
| 无头 server bin | `bootstrap::serve_http` 之前 | 与网关共用 `task_queue::worker::spawn_all(pool, deps, shutdown_rx)` |
| 桌面（Tauri） | `bootstrap` 初始化完成后 | **并发固定 1**，且不进 `parse-video` 队列（避免与 UI 抢 CPU） |

### 9.2 装配示意（已实现，见 `src/worker.rs`）

```rust
// task_queue::start(...) 内部：按队列起 N 个 worker
let backend = PostgresStorage::new(&deps.pool)
    .with_config(Config::default().queue(queue).batch_size(batch_size));
let worker = WorkerBuilder::new(format!("{queue}-{}-{index}", cfg.instance_id))
    .backend(backend)
    .data(deps)              // → handler 参数 Data<Arc<TaskDeps>>
    .long_running()          // LongRunningExt：长任务心跳/锁续期
    .enable_tracing()        // WorkerBuilderExt：事件写入 tracing
    .build(handle_parse);    // handle_parse(ParseTask, Data<Arc<TaskDeps>>, WorkerContext)

tokio::spawn(async move {
    let signal = async move { let _ = shutdown.changed().await; Ok::<(), WorkerError>(()) };
    if let Err(e) = worker.run_until(signal).await { tracing::error!(error = %e, "worker 退出"); }
});
```

> ⚠️ 实测（apalis 1.2.0）：`run_until` 的语义是「signal 完成 → 调用 `ctx.stop()` 优雅停止」，
> 所以 signal 返回 `Ok(())` 即可，不需要在 signal 里手动 `stop`；`on_event` / `long_running` /
> `enable_tracing` 分别来自 `EventListenerExt` / `LongRunningExt` / `WorkerBuilderExt`，
> 都要 `use apalis::prelude::*;`。

### 9.3 并发模型

`apalis` 没有「单 worker 并发上限」的固有方法，实际吞吐 = **worker 数量 × `batch_size`**：

| 队列 | worker 数（每实例） | `batch_size` | 依据 |
|---|---|---|---|
| `parse` | `TASK_QUEUE_PARSE_CONCURRENCY`（默认 2） | 1 | 单任务持一个连接 + 一个临时目录 |
| `parse-video` | 1 | 1 | ffmpeg + ASR 吃满 CPU/内存 |
| `vectorize` | 2 | 4 | embedding 走网络，批量更划算 |
| `maintenance` | 1 | 1 | 低优先级 |

公式（进程级）：`并发任务数 ≈ Σ(worker 数 × batch_size)`，必须同时满足：

1. ≤ `sqlx` 连接池上限的 50%（`crates/database` 的连接池配置），否则会与 HTTP 请求抢连接；
2. 视频任务并发 ≤ `min(CPU 核数 / 2, TASK_QUEUE_VIDEO_CONCURRENCY)`。

### 9.4 长任务：`long_running()`

视频解析可能持续 20 分钟，远超默认锁/可见性超时。必须同时做两件事：

1. `WorkerBuilder::long_running()`（或 `long_running_with(LongRunningConfig)`）—— 由 apalis 负责
   任务锁续期，避免被别的实例判定为孤儿并重复领取；
2. 任务级 `timeout` 兜底（§7.4），超时后走 `RetryAfterError` 而不是挂死。

### 9.5 CPU 密集与外部进程

| 场景 | 做法 | 原因 |
|---|---|---|
| PDF / Office 解析 | 复用 `parser::parse_file`（内部已 `spawn_blocking`） | 不要在 async 上下文里直接做 CPU 密集解析 |
| 视频抽帧 / ASR / OCR | `service::doc_parser::DocParserClient::parse_file`（Python 侧车 HTTP） | 侧车进程独立限流、独立崩溃域 |
| ffmpeg 子进程 | 进程级 `Semaphore`（默认 1）限制并发 | 防止 N 个实例把机器打满内存 |
| 大文件 | 只下载到任务私有临时目录（RAII 清理），完成后立即删除 | 磁盘水位可控 |

```rust
// 任务体内务必整个包在 with_schema 里（见 §10.1）
let text = database::with_schema(task.tenant.clone(), async {
    let result = client.parse_file(&local_path, &task.options).await?;
    Ok::<_, TaskError>(result.raw_text)
}).await?;
```

### 9.6 错误分类

| 情形 | 返回语义 | 是否重试 |
|---|---|---|
| 文件不存在 / 扩展名不支持 / 租户为空 | `AbortError` | ❌ 立即 Failed，不再重试 |
| S3 超时、连接重置、ffmpeg 偶发非 0 | `RetryAfterError`（指数退避） | ✅ |
| ASR 模型 OOM、embedding 限流 | `DeferredError` 或 `RetryAfterError` | ✅ |
| 代码 panic | 由 `catch-panic` 特性转成错误（apalis 默认开启） | ✅ 按 attempts |

```rust
// crates/task-queue/src/error.rs（示意）
#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("任务参数非法: {0}")]
    Invalid(String),                 // → AbortError
    #[error("依赖暂不可用: {0}")]
    Transient(String),              // → RetryAfterError
    #[error("解析失败: {0}")]
    Fatal(String),                  // → AbortError（已重试耗尽或不可恢复）
}
```

### 9.7 优雅关闭

- 复用 `gateway::serve` 的 `shutdown_tx/rx`：收到 SIGTERM → 停止领取新任务 → 等在跑的任务
  到超时（`GATEWAY_DRAIN_SECS`，默认 15s）→ 退出；
- 进程内秒级强杀时，任务停在 `Running`，由 apalis-postgres 的 heartbeat 超时后重新入队；
- 关闭期间**不要**释放 `worker_id` 租约之外的东西（`coord::release_worker` 逻辑不变）。

### 9.8 桌面形态的额外约束

1. 并发固定 1，队列只开 `parse`（不开 `parse-video`，视频仍走 inline 或提示用户用服务端）；
2. worker 启动失败只 `WARN`，不阻断窗口启动（与 `bootstrap` 现有约定一致）；
3. Tauri 退出必须 `join` worker 或直接 abort（桌面不保证优雅关闭）。

### 9.9 P1 已实现的行为清单（`src/worker.rs`）

| 行为 | 实现 |
|---|---|
| 队列划分 | `parse`（文档/图片）、`parse-video`（音视频，仅 `TASK_QUEUE_VIDEO_ENABLED=1` 时启动）、`vectorize` |
| 并发 | `parse` = `TASK_QUEUE_PARSE_CONCURRENCY`（1~8）；`parse-video` = 1；`vectorize` = 2 |
| 重任务闸门 | 进程级 `Semaphore(1)`：音视频任务串行，压住 ffmpeg + 模型的内存峰值 |
| 租户上下文 | 任务体 100% 包在 `database::with_schema(task.tenant, ..)`（§10.1 红线） |
| 文件落地 | `s3://` 下载到任务私有 `TempDir`（`Drop` 递归删除）；`file://`/绝对路径仅桌面调试 |
| 解析后端 | `service::doc_parser::DocParserClient`，`DOC_PARSER_BACKEND=local｜remote` 语义不变 |
| 向量化 | `skip_index=false` 时复用 `service::rag_service::RAGRetriever::chunk_and_vectorize` |
| 结果落库 | `ParseResultSink` 钩子（默认 `LoggingSink` 只记日志，业务侧注入实现） |
| 错误语义 | `Transient` → 可重试；`Invalid`/`Fatal` → `AbortError`（不重试） |
| 优雅关闭 | `spawn_all` 返回 `JoinHandle`；`shutdown` 置位后 `run_until` 停止领取新任务 |

---

## 10. 多租户与多实例

### 10.1 红线：`database::with_schema` 必须包裹任务体

`crates/database/src/lib.rs` 的租户上下文是 **task-local**（`CURRENT_SCHEMA`），由 HTTP 的
`auth_middleware` 设置；worker 不在请求里，`schema_prefix()` 会回落到 `GLOBAL_SCHEMA`
（桌面默认 `public`，服务端是最后登录的租户）。**不包裹 = 跨租户写库**。

```rust
// ❌ 错误：worker 里直接写库，落到 GLOBAL_SCHEMA
async fn handle_parse(task: ParseTask, ctx: Data<Arc<TaskDeps>>) -> Result<(), TaskError> {
    let prefix = database::schema_prefix();      // 可能是 ""（public）或别的租户
    sqlx::query(&format!("UPDATE {prefix}knowledge_asset SET ...")).execute(&ctx.pool).await?;
    Ok(())
}

// ✅ 正确：整个任务体（含所有子查询、HTTP 回调、向量化）都在 with_schema 内
async fn handle_parse(task: ParseTask, ctx: Data<Arc<TaskDeps>>) -> Result<(), TaskError> {
    if task.tenant.trim().is_empty() {
        return Err(TaskError::Invalid("tenant 为空".into()));   // AbortError
    }
    database::with_schema(task.tenant.clone(), async move {
        // 下载 → 解析 → 回写 → 向量化，全部在这里
        Ok::<_, TaskError>(())
    })
    .await
}
```

配套约束：

1. `tenant` 必须由**服务端**从鉴权上下文写入任务（`api` 的 `UserContext.schema`），
   绝不接受请求体里的 `tenant`（与 `knowledge_asset_routes.rs` 里「不采信客户端 `createdBy`」同源）；
2. `with_schema` 内的所有 `spawn` 子任务都会**丢失** task-local —— 如果必须并发，用
   `database::with_schema` 各自包一层，或干脆串行；
3. `apalis` 自己的表（`apalis.jobs` / `apalis.workers`）**不按租户分 schema**，属于基础设施表；
   租户隔离只体现在任务参数与业务表上。

### 10.2 worker 命名与雪花租约

| 约束 | 做法 | 理由 |
|---|---|---|
| worker 名全局唯一 | `"{queue}-{instance_id}-{n}"` | 看板按 worker 聚合并发/心跳；重名会互相覆盖 |
| 不与雪花冲突 | 复用 `gateway::config` 的 `instance_id`（已是 `SNOWFLAKE_WORKER_ID` 的语义载体） | 现有租约机制保证一实例一 worker_id，无需第二套 |
| 桌面 | `"parse-desktop-0"` | 固定后缀，便于排查 |

> `apalis.jobs` 的 `id` 用的是它自己的 `TaskId`（ULID/UUID，取决于 feature），
> **不要**拿雪花 id 当任务 id；业务与任务的关联走 `task_ticket`（§7.5）。

### 10.3 队列分摊与公平性

- `PostgresStorage` 用 `FOR UPDATE SKIP LOCKED` 领取任务，多实例天然分摊，无需额外选主；
- 同一队列内按 `priority` + `run_at` 排序：视频任务 `priority=1`，短文档 `priority=2`，
  避免「一个大视频堵住整条队列」；
- `batch_size > 1` 时单个 worker 一次拿多条，会降低公平性 —— 因此 `parse` / `parse-video`
  一律 `batch_size = 1`；
- 队列积压监控见 §13.4（阈值触发扩容或限流上传接口）。

### 10.4 定时任务的单实例约束（重要）

`apalis-cron` 的 `CronScheduler` 是**每个 worker 各算各的**：N 个实例注册同一个 cron，
就是 N 倍触发。两种正确做法（P2 二选一）：

| 方案 | 做法 | 适用 |
|---|---|---|
| A. leader 注册（推荐） | 只在拿到 `CLEANUP_LOCK_KEY`（`gateway/src/worker.rs:21`）的实例上 `spawn` cron worker | 与现有选主机制一致，零新增依赖 |
| B. cron 持久化 + 唯一约束 | `CronScheduler::new(schedule).pipe_to(PostgresStorage::new(&pool))`，任务体按「当天日期」做幂等 | 需要 cron 结果留痕时 |

---

## 11. 定时任务（apalis-cron）

### 11.1 适用场景（已实现，见 `src/maintenance.rs`）

| 任务 | 频率 | 说明 |
|---|---|---|
| 收敛停滞票据 | 每天 03:30（可配） | `task_ticket` 里 `pending`/`running` 超过 2 小时 → 置 `failed`，前端不会永远转圈 |
| 队列指标打点 | 同上 | `Metrics::fetch_by_queue()` 输出各队列统计（积压/完成/失败），供日志告警 |
| 失败任务日报 | — | 页面/日志已可直接看到，未单独实现 |

> ⚠️ **实测修正**：`apalis-postgres 1.2.0` **没有**实现 `Vacuum` / `ResumeAbandoned`
> （`src/` 下只有 `queries/reenqueue_orphaned.rs` 的查询函数，没有 trait impl），因此
> §3.2 / §11.1 早期写的「用 Vacuum / ResumeAbandoned 回收孤儿任务」**不可用**。
> 孤儿任务由 apalis-postgres 自带 heartbeat（`Config::missed_heartbeats`，默认 30s × 10）
> 重新入队；维护 cron 只兜底「票据状态」，不改 `apalis.jobs`。

### 11.2 实现方式（`src/maintenance.rs`）

```rust
// 1) 表达式：TASK_QUEUE_MAINTENANCE_CRON，非法值回落 "0 30 3 * * *"（六段，含秒）
// 2) worker：CronScheduler::new(schedule) + data(PgPool) + enable_tracing()
// 3) 每次 tick：抢 pg_try_advisory_lock(MAINTENANCE_LOCK_KEY) → 干活 → 显式 unlock
//    抢不到说明别的实例在跑，直接跳过（等价 leader，且不依赖 gateway 选主）
// 4) 干活内容：reconcile_stale_tickets() + log_queue_metrics()
```

### 11.3 多实例与幂等

| 关注点 | 做法 |
|---|---|
| 多实例重复触发 | `pg_try_advisory_lock`：抢不到直接返回（连接必须显式 unlock，归还池不会自动释放会话锁） |
| 票据收敛幂等 | `WHERE status IN ('pending','running') AND updated_at < NOW() - 2h`，重复执行只影响同一批 |
| 与 gateway 清理任务互不影响 | 锁 key 不同（`MAINTENANCE_LOCK_KEY` ≠ `CLEANUP_LOCK_KEY`），单测锁定这一点 |

### 11.4 注意事项

1. 时区：`cron` 按进程本地时区计算，部署时用 `TZ=Asia/Shanghai` 统一，不要依赖宿主机默认值；
2. 与 `gateway` 的 `spawn_leader` **不必**绑定：咨询锁已保证单实例执行，少一层耦合；
3. cron 任务本身要幂等（同一天重复执行不产生重复数据）—— 票据收敛天然满足。

---

## 12. 工作流（apalis-workflow）

### 12.1 何时该用 workflow，而不是一个「大 job」

满足下列任意两条才上 workflow，否则保持单一任务（简单、好排障）：

| 判据 | 单一任务 | workflow |
|---|---|---|
| 阶段数 | 1~2 | ≥3 且有数据依赖 |
| 中间产物 | 不需要留存 | 需要留存（帧、ASR 文本、OCR 结果）或需要断点续跑 |
| 并行度 | 无 | 抽帧 OCR 与 ASR 可并行 |
| 失败定位 | 整体重跑可接受 | 需要知道「卡在哪一步」 |

### 12.2 视频解析链路（`SteppedFlow`，顺序版）

```rust
use apalis::prelude::*;
use apalis_workflow::SteppedFlow;

let flow = SteppedFlow::new("video-parse")
    .and_then(|task: ParseTask| async move { download(&task).await })          // → LocalFile
    .and_then(|file: LocalFile| async move { probe(&file).await })             // → MediaInfo
    .and_then(|info: MediaInfo| async move { extract_frames(&info).await })    // → Vec<Frame>
    .and_then(|frames: Vec<Frame>| async move { ocr(&frames).await })          // → String
    .and_then(|ocr_text: String| async move { transcribe(&ocr_text).await })   // → String
    .and_then(|text: String| async move { persist_and_vectorize(&text).await }); // → ()

let worker = WorkerBuilder::new(format!("video-{instance_id}"))
    .backend(PostgresStorage::new(&pool).with_config(Config::default().queue("parse-video")))
    .data(deps.clone())
    .long_running()
    .build(flow);
```

> ⚠️ 示意：`SteppedFlow` 的每一步类型必须严格衔接（上一步输出 = 下一步输入），
> 编译器会检查。真实实现里 `download` 的入参需要用 `WorkflowSink` 的方式推入，而不是直接
> `push_task`。

### 12.3 并行版（`GraphFlow`，DAG）

```rust
use apalis_workflow::{GraphFlow, WorkflowSink};

let flow = GraphFlow::new("video-parse-dag");
let frames = flow.add_task(extract_frames);        // 输出 Vec<Frame>
let asr    = flow.add_task(transcribe_audio);      // 输出 String（独立分支）
let ocr    = flow.add_task(ocr_frames);            // 输出 String（独立分支）
flow.add_task(merge_and_persist).depends_on((&frames, &asr, &ocr));  // 顺序敏感
flow.validate()?;                                   // 图结构校验，失败直接 panic/err
```

并行收益：`transcribe_audio`（分钟级）与 `ocr_frames`（分钟级）重叠执行，视频总耗时接近
`max(ASR, OCR)` 而非两者之和。

### 12.4 与 `crates/workflow`（BPMN）的边界

| 维度 | `crates/workflow`（BPMN/adk-graph） | `apalis-workflow` |
|---|---|---|
| 面向 | 业务人员编排（审批、会签、人工节点） | 工程内部数据管道（解析、转码、向量化） |
| 存储 | 业务表（流程实例/任务） | `apalis.jobs`（任务表） |
| 触发 | 用户/表单/事件 | 队列、代码、cron |
| 是否引入本方案 | ❌ 不改 | ✅ P2 引入 |

两者**不互相替代**：BPMN 里如果要「调用视频解析」，正确做法是 BPMN 服务节点 push 一个
`ParseTask`，而不是把解析步骤画进 BPMN。

### 12.5 P2 落地结论（重要）

**`apalis-workflow` 分阶段视频链路本期未落地**，原因不是「没时间」，而是语义成本：

1. `SteppedFlow` 的**每一步都是独立的持久化任务**，worker 执行每一步时都没有上一步的内存状态，
   因此每一步都必须从参数里恢复 `database::with_schema(task.tenant, ..)` 与重试语义；
2. 视频链路的中间产物（帧、ASR 原文）如果只放在内存里会随步骤边界丢失，必须落对象存储并传引用，
   相当于把「单任务内存流水线」重写成「跨任务状态机」；
3. 当前单任务链路（下载 → 解析（含抽帧/ASR/OCR）→ 向量化）已经能正确完成业务目标，
   分阶段的主要收益是「中间产物可观测 + 断点续跑」，属于 P4 的收益区间。

落地时的最小步骤：

```
VideoState { task: ParseTask, local_path, text, chunks }   // 每一步都整体传递，tenant 跟着走
step1 download  → set local_path
step2 parse     → set text（内部 DocParserClient 已覆盖视频抽取）
step3 vectorize → 调 rag_service + ParseResultSink.on_parsed
```

每步开头统一 `database::with_schema(state.task.tenant.clone(), ..)`，并把 `task.task_id` 用于票据阶段推进。

---

## 13. 可观测性

### 13.1 日志与 tracing

| 要求 | 做法 |
|---|---|
| 任务 span 可串联 | worker 侧 `enable_tracing()`；任务体用 `tracing::info_span!("parse", asset_id, tenant, task_id)` |
| 关键阶段打点 | `downloading` / `parsing` / `chunking` / `vectorizing` / `done`，每步带耗时 |
| 不打印敏感信息 | 不打印 `storage_url` 全量（只打印 bucket + key 前 8 字符）、不打印文件内容、不打印令牌 |

### 13.2 指标（建议自建，不依赖 apalis feature）

| 指标 | 类型 | 说明 |
|---|---|---|
| `task_queue_backlog{queue}` | Gauge | `SELECT count(*) FROM apalis.jobs WHERE queue=$1 AND status='Pending'` |
| `task_queue_wait_seconds{queue}` | Histogram | `run_at`/入队时间 → 开始执行 |
| `task_queue_duration_seconds{kind,doc_type}` | Histogram | 执行耗时 |
| `task_queue_failures_total{kind,error_class}` | Counter | `Abort` / `Retryable` / `Fatal` 分类 |
| `task_queue_workers{alive}` | Gauge | 看板/心跳视图 |

> `apalis` 提供 `prometheus` / `opentelemetry` feature，但项目已用 OTel（`opentelemetry 0.33`），
> 建议走自建指标（复用现有 meter），避免两套埋点体系。

### 13.3 看板（apalis-board）—— 上游不兼容，改用独立部署

**实测结论（2026-11）**：`apalis-board 1.3.0` 的 axum 集成要求后端实现
`ListWorkers`（`RegisterRoute` 的 where 子句里写死），而 `apalis-postgres 1.2.0` 只实现了
`ListQueues` / `ListTasks` / `ListAllTasks` / `Metrics` / `FetchById`，**没有 `ListWorkers`**
→ 挂到网关子路由在编译期就不成立。

可行方案（按推荐顺序）：

1. **官方独立镜像**：以 `apalis-dev/apalis-board` 仓库 README 的镜像名 / 环境变量为准，
   指向同一个 `DATABASE_URL`，只监听内网或 `127.0.0.1`，通过堡垒机访问；
2. **自建只读视图**（零依赖，立即可用）：直接查 `apalis.jobs` 与业务 `task_ticket`：

```sql
-- 队列积压（运维排障）
SELECT queue, status, count(*) FROM apalis.jobs GROUP BY 1, 2 ORDER BY 1, 2;
-- 最近失败任务（按租户）
SELECT tenant_schema, id, asset_id, stage, attempts, left(error, 120)
FROM tenant_a.task_ticket WHERE status = 'failed' ORDER BY updated_at DESC LIMIT 50;
```

3. 等 `apalis-postgres` 补齐 `ListWorkers` 后再评估挂载；届时**必须**叠加现有鉴权中间件
   （`api::auth`）+ 仅管理员可见，**不要**暴露到公网。

### 13.4 告警阈值（建议初值）

| 告警 | 条件 | 处置 |
|---|---|---|
| 队列积压 | `backlog{parse} > 50` 持续 10 分钟 | 扩 worker / 限流上传 |
| 等待过长 | `p95(wait) > 300s` | 同上 + 检查是否被视频任务长期占用 |
| 失败率 | 5 分钟内 `failures/total > 10%` | 查 `AbortError` 原因（多为格式不支持/文件损坏） |
| 无心跳 | `workers{alive} == 0` 且 `backlog > 0` | worker 装配失败或被关闭信号误杀 |
| 长任务堆积 | `Running` 超过 `timeout` 仍未结束 | 检查 ffmpeg 僵尸进程 / ASR 挂死 |

---

## 14. 数据库与迁移

### 14.1 `apalis` 独立 schema

`apalis-postgres 1.x` 把表与迁移历史放在 **`apalis`** schema（不是 `public`，也不是租户 schema）：

| 对象 | 说明 |
|---|---|
| `apalis.jobs`（名称以迁移为准） | 任务行：队列、状态、参数、重试次数、时间戳 |
| `apalis.workers` | worker 注册与心跳 |
| `apalis._sqlx_migrations` | apalis 自己的迁移历史 |

核对命令：

```sql
-- 首次 setup 后执行，确认对象落在 apalis schema
\dt apalis.*
SELECT * FROM apalis._sqlx_migrations ORDER BY version DESC LIMIT 3;
```

### 14.2 `PostgresStorage::setup` 调用点与顺序

```rust
// crates/bootstrap/src/lib.rs（P1 改造示意）
pub async fn init_task_queue(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    // 1. 业务迁移已完成（database::init_database()），再建 apalis 表
    apalis_postgres::PostgresStorage::setup(pool).await?;   // 幂等
    tracing::info!("apalis 任务表就绪（schema=apalis）");
    Ok(())
}
```

| 约束 | 说明 |
|---|---|
| 必须在 `database::init_database()` **之后** | 业务表要先就绪，且连接池已建立 |
| 幂等 | 可重复调用；桌面/Serve/网关三条启动路径都调一次 |
| 失败策略 | 无头形态：**直接退出**（宁可少一个实例，不带病接任务）；桌面形态：`WARN` 后禁用队列（回落 inline） |
| 多实例并发启动 | `sqlx` 迁移自带锁；无需额外 advisory lock |

### 14.3 与业务表、租户 schema 的关系

| 表 | schema | 租户隔离 | 备注 |
|---|---|---|---|
| `apalis.jobs` / `apalis.workers` | `apalis` | ❌（全局） | 基础设施表，含所有租户的任务；隔离靠 payload 的 `tenant` + worker 的 `with_schema` |
| `task_ticket`（新增业务表） | 租户 schema | ✅ | 进度/去重/前端查询都看它，必须按租户隔离 |
| `knowledge_asset` / `document_chunk` | 租户 schema | ✅ | worker 在 `with_schema` 内写入 |

> 新表 `task_ticket` 需要同时加进 `public` 与各租户 schema 的建表脚本（参照
> `database/src/postgres.rs` 现有租户建表流程），否则切换租户后会 `relation does not exist`。

### 14.4 权限最小化

应用账号需要：

- `CREATE ON DATABASE`（首次 `setup` 建 `apalis` schema）；
- `apalis` schema 下建表 / 建索引 / `NOTIFY` 权限。

生产建议：**DBA 预建 schema 与表**，应用侧关闭自动迁移（不调用 `setup`，或用独立
`migrate` bin 跑一次），避免运行时账号持有 DDL 权限。

### 14.5 备份与恢复

- `pg_dump` 需包含 `apalis` schema；沿用现有备份脚本时确认没有 `--schema=public`、
  `--schema=<tenant>` 这种白名单；
- 恢复后任务表与业务表可能不一致（例如任务已「完成」但业务行回滚）：以业务表为准，
  把 `Running/Pending` 的孤儿任务用 `ResumeAbandoned` 重新入队即可；
- `apalis.jobs` 会持续增长，必须定期 `Vacuum`（§11.1），否则索引膨胀影响领取性能。

### 14.6 老库升级

本项目是**全新引入**，不存在 1.0 之前的 `apalis` 历史表，因此 §3.2 里那条
`ALTER TABLE public._sqlx_migrations SET SCHEMA apalis` 的迁移**不需要执行**。
只有当某个环境曾手工装过 `apalis-sql` 才需要按官方升级说明处理。

---

## 15. 容量与性能

### 15.1 并发公式

```
单实例并发任务数 = Σ over queues ( worker 数 × batch_size )
约束：
  1. 并发任务数 × 平均连接占用 ≤ 连接池上限 × 0.5
  2. 视频并发 ≤ min(CPU 核数 / 2, TASK_QUEUE_VIDEO_CONCURRENCY)
  3. 临时目录峰值 = 视频并发 × 单文件大小上限（默认 2GB）≤ 磁盘剩余空间 × 0.3
```

### 15.2 资源画像（单任务峰值，实测后修订）

| 类型 | CPU | 内存 | 磁盘（临时） | 主要瓶颈 |
|---|---|---|---|---|
| PDF（文本版） | 1 核 | < 300MB | ≈ 文件大小 | CPU（`pdf_oxide`） |
| Office（docx/xlsx） | 1 核 | < 500MB | ≈ 文件大小 | CPU |
| 图片 + OCR | 1~2 核 | 1~2GB | 小 | 模型加载 |
| 音频（ASR） | 2 核 | 2~4GB | ≈ 文件大小 | 模型推理 |
| 视频（抽帧 + ASR + OCR） | 2~4 核 | 4~8GB | 2× 文件大小（帧） | ffmpeg + 模型 |

> 结论：**视频与文档必须分队列**，否则一个 30 分钟视频会让同实例的文档解析全部等待。

### 15.3 payload 大小

| 内容 | 放哪 |
|---|---|
| 文件本体 | ❌ 不入库；`s3://bucket/key` 引用 |
| 解析出的长文本（> 64KB） | 写对象存储，payload 放 `result_ref`；短文本可直接放 `text`（`VectorizeTask`） |
| 帧图片 | 写对象存储或临时目录（任务结束即删），payload 只放引用 |
| 错误堆栈 | 存 `task_ticket.error`（截断到 4KB），不写进 `apalis.jobs` 的 meta |

### 15.4 压测建议

| 场景 | 指标 | 通过标准 |
|---|---|---|
| 50 个 100MB PDF 并发上传 | 入队 p99 延迟、`parse` 队列清空时间 | 入队 p99 < 100ms；无 5xx |
| 1 个 30 分钟 1080p 视频 | 端到端耗时、内存峰值 | 不 OOM、`Running` 状态心跳不断 |
| 网关 2 实例 + 100 任务 | 任务是否重复执行 | 重复执行数为 0（`SKIP LOCKED` 生效） |
| 强杀实例（`kill -9`） | 孤儿回收时间 | ≤ heartbeat 超时 + 1 个 cron 周期 |
| 500 任务积压 | 看板可用性、DB 连接占用 | 看板加载 < 3s；HTTP 请求不受影响 |

---

## 16. 安全

### 16.1 payload 红线

| 禁止 | 原因 | 正确做法 |
|---|---|---|
| 本机绝对路径（`/tmp/x.mp4`、`C:\...`） | 任务可能被别的实例领取，路径不存在；也会泄露目录结构 | 只放 `s3://bucket/key` |
| 长期凭证（AK/SK、DB 密码、JWT） | `apalis.jobs` 是明文表，任何有库权限的人都能看 | worker 从环境变量/凭据链取；临时访问用预签名 URL |
| 文件内容本身 | payload 膨胀、备份变慢 | 只放引用（`result_ref`） |
| 客户端传入的 `tenant` | 越权：可把任务投到别的租户 schema | 服务端从 `UserContext.schema` 覆盖写入 |

### 16.2 看板暴露面

1. 默认**不**对公网暴露；只监听内网或 `127.0.0.1`，通过堡垒机/端口转发访问；
2. 若必须挂在网关上，路径固定为 `/admin/jobs`，并叠加：
   - 现有 `api::auth` 鉴权中间件；
   - 角色校验（仅 `SUPER_ADMIN` / 运维角色）；
   - 独立限流（`crates/gateway/src/rate_limit.rs`）；
3. 看板能「重放 / 删除任务」，属于**写操作**，必须进 `sys_oper_log` 审计。

### 16.3 越权与重放

| 风险 | 缓解 |
|---|---|
| 用别人的 `taskId` 查进度 | 查询接口校验 `task_ticket.tenant == 当前用户 schema` |
| 枚举任务 id | 任务 id 用 ULID/UUID（不可枚举），业务 ticket 用 UUID，不用自增 |
| 重复提交消耗资源 | `dedupe_key` + 活跃态去重（§7.5）；上传接口本身限流 |
| 恶意超大文件 | 入队前校验大小上限（视频 2GB / 文档 200MB），超限直接拒绝而非入队 |

### 16.4 文件处理安全（worker 侧）

```text
1. 下载到任务私有目录（临时目录 + RAII 清理），文件名由服务端生成，绝不用原始文件名拼路径
2. 校验扩展名与实际 magic（parser::detect_file_type + 文件头字节）
3. 解压类格式（docx/xlsx/zip）限制解压后总大小与文件数（zip bomb）
4. 外部进程（ffmpeg）用参数数组而非 shell 字符串，避免命令注入
5. 任务结束（成功/失败）都删除临时目录；失败任务保留不超过 N 小时（可配）
```

---

## 17. 灰度与回滚

### 17.1 开关矩阵

| 环境变量 | 取值 | 默认 | 作用 |
|---|---|---|---|
| `TASK_QUEUE_MODE` | `inline` / `queue` / `queue_fail_open` | `inline` | 总开关；默认与改造前**完全一致** ✅ |
| `TASK_QUEUE_PARSE_CONCURRENCY` | 1~8 | 2 | `parse` 队列每实例 worker 数 ✅ |
| `TASK_QUEUE_BATCH_SIZE` | 1~16 | 1 | 领取批大小（`parse-video` 恒为 1，`vectorize` 恒为 4）✅ |
| `TASK_QUEUE_VIDEO_ENABLED` | `0` / `1` | `0` | 是否启动 `parse-video` 队列（桌面恒为 0）✅ |
| `TASK_QUEUE_INSTANCE_ID` | 任意非空串 | `SNOWFLAKE_WORKER_ID` 或 `local` | worker 命名与日志定位 ✅ |
| `TASK_QUEUE_BOARD_ENABLED` | `0` / `1` | `0` | 看板子路由开关（**上游不兼容，当前无实现**，见 §13.3） |
| `TASK_QUEUE_MAINTENANCE_CRON` | 六段 cron | `0 30 3 * * *` | 维护任务触发时间（票据收敛 + 队列指标）✅ |
| `TASK_QUEUE_TASK_TIMEOUT_SECS` | 60~7200 | 1800 | 单任务超时（P4：待接 worker 超时 layer） |

> 命名与语义沿用现有约定：全大写 + `_SECS` 后缀、非法值回落默认（参考
> `service::doc_parser::parse_backend_value` 的容错写法）。

### 17.2 上线顺序

```
① 迁移：DBA 预建 apalis schema + 新表 task_ticket（或应用启动 setup）
② 发版（TASK_QUEUE_MODE 仍为 inline）：验证启动、迁移、无行为变化
③ 打开 queue_fail_open，对 1 个租户灰度（观察 1 天：失败率、积压、耗时）
④ 全量 queue，关闭 fail_open
⑤ 打开看板（内网）与 cron（仅 leader）
⑥ 视频走队列（TASK_QUEUE_VIDEO_ENABLED=1）
```

### 17.3 回滚

| 触发条件 | 回滚动作 | 影响 |
|---|---|---|
| 入队失败率高 / worker 崩溃 | `TASK_QUEUE_MODE=inline` 重启 | 恢复旧同步行为；已入队任务留在表中，稍后消化或人工重放 |
| 数据串租户（红线告警） | 立即 `inline` + 停 worker + 按 `tenant` 审计修复 | 必须复盘 §10.1 |
| 数据库压力过大（连接/CPU） | 降 `TASK_QUEUE_PARSE_CONCURRENCY` 至 1，或临时停 worker | 队列积压增加，但 HTTP 不受影响 |
| 看板异常 | `TASK_QUEUE_BOARD_ENABLED=0` | 无业务影响 |

回滚**不需要**改代码，也**不需要**回滚数据库迁移（`apalis` 表可空置）。

---

## 18. 里程碑与验收

### 18.1 里程碑

| 阶段 | 范围 | 产出 | 验收标准 |
|---|---|---|---|
| **P0（本次）** | 文档 + crate 骨架 + 根 Cargo.toml 登记 | 本文档、`crates/task-queue/{Cargo.toml,README.md,src/lib.rs}` | `cargo metadata` 正常；`cargo check -p task-queue` 通过；零依赖、零行为变化 |
| **P1（已完成）** | `ParseTask`/`VectorizeTask` 契约、`producer`、`worker`（解析 + 向量化）、`TASK_QUEUE_MODE` 灰度 | `crates/task-queue/src/{config,job,producer,worker,error}.rs` | `cargo check/test -p task-queue` 通过；`inline` 时零副作用；音视频独立队列 |
| **P2（已完成）** | `apalis-cron` 维护（票据收敛 + 队列指标 + 咨询锁）；board / workflow 兼容性核实 | `crates/task-queue/src/maintenance.rs` | cron 六段表达式校验；多实例只跑一次；board 不兼容有实测证据（§13.3）；workflow 结论见 §12.5 |
| **P3（已完成）** | `task_ticket` 表、`GET/POST /api/tasks*`、`BusinessParseSink`、bootstrap/gateway 接线 | `database/src/sql/task_queue_migration.sql`、`api/src/{task_routes,task_sink}.rs`、`bootstrap`、`gateway` | `cargo check -p api -p bootstrap -p gateway` 通过；`inline` 时不建表不起 worker |
| P4（候选） | `apalis-workflow` 分阶段链路（中间产物落对象存储）、任务超时 layer、看板（等上游 `ListWorkers`） | — | 按收益排期 |

### 18.2 P0 验收清单（本次已交付）

- [x] `crates/task-queue/Cargo.toml`：edition 2024、`[lints] workspace = true`、零依赖
- [x] `crates/task-queue/src/lib.rs`：模块文档（职责 / 依赖方向 / 多租户红线 / 进度表）
- [x] `crates/task-queue/README.md`：索引 + 现状证据 + 5 步快速上手 + 进度表
- [x] 根 `Cargo.toml` 登记 `task-queue = {path = "crates/task-queue"}`
- [x] 本文档：架构、契约、生产/消费、多租户、定时、工作流、可观测、迁移、容量、安全、灰度

### 18.3 P2/P3 验收清单

**P2（已交付）**

- [x] `cron` 六段表达式解析 + 非法值回落（单测覆盖 5 段报错）
- [x] 维护任务抢 `pg_try_advisory_lock`，抢不到直接跳过（多实例只跑一次）
- [x] 锁 key 与 `gateway::worker::CLEANUP_LOCK_KEY` 不同（单测锁定）
- [x] 票据收敛：`pending`/`running` 超 2 小时置 `failed`（按租户 schema 逐个执行，schema 白名单校验）
- [x] 队列指标：`Metrics::fetch_by_queue()` 打点
- [x] `apalis-board` 兼容性实测：`ListWorkers` 缺失 → 挂载不可行，改用独立镜像 / 只读 SQL（§13.3）

**P3（已交付）**

- [x] `task_ticket` 建表 SQL + 新租户初始化 + 存量租户启动迁移（`migrate_all_tenant_task_queue_tables`）
- [x] `POST /api/tasks/parse` → `202 { taskId, assetId, status }`；参数非法 400、队列不可用 503
- [x] `GET /api/tasks/{taskId}` → 读 `task_ticket`；不存在 404
- [x] `GET /api/tasks?status=&limit=` 列表（状态白名单 + limit 夹逼 1..=200）
- [x] **任务中心页** `/tasks`（列表 / 筛选 / 5s 自动刷新 / 进度条 / 展开错误）
- [x] **知识资产页「解析」按钮**（未绑定文件禁用；成功提示 taskId）
- [x] 菜单种子 id=240~243（`public_initial_data.sql` + `public_migration.sql`，幂等）
- [x] 前端 `httpAdapter` 3 条映射 + `taskService` 契约测试（`pnpm exec jest` 通过）
- [x] 租户只从 `database::current_schema_name()` 取，请求体不采信
- [x] authz 权限点登记：`POST` → `knowledge:edit`、`GET` → `knowledge:list`
- [x] `BusinessParseSink`：仅当资产正文为空时回填，不覆盖人工内容
- [x] `bootstrap::TaskQueueRuntime` + `serve_http` / `gateway::serve` 同生命周期启动与优雅关闭
- [x] 启动失败只降级（HTTP / 网关继续提供，解析走同步路径）
- [ ] 真实 DB 联调（需要 PostgreSQL + pgvector 环境；本机无库，未跑集成用例）
- [ ] 长视频不重复领取 / `kill -9` 回收（依赖真实实例与心跳窗口，未跑）

### 18.4 风险表

| 风险 | 可能性 | 影响 | 对策 |
|---|---|---|---|
| `apalis-board` 1.3 与 `apalis-core` 1.2 版本不兼容 | 中 | P2 看板推迟 | P2 前先跑 `cargo tree -i apalis-core`；不兼容则先用独立进程镜像 |
| 多租户上下文丢失导致串租户 | 中 | **严重** | `with_schema` 强制包裹 + 评审 + 灰度期比对各租户写入量 |
| 视频任务拖垮实例内存 | 中 | 高 | 独立队列 + 并发 1 + 临时目录水位监控 + 单文件大小上限 |
| `apalis.jobs` 膨胀 | 高（长期） | 中 | 定期 `Vacuum` + 保留期（建议完成态保留 7 天） |
| 桌面形态 worker 抢 UI 线程 | 中 | 中 | 桌面并发固定 1、只开 `parse`、失败不阻断启动 |
| 依赖引入导致构建时间上升 | 中 | 低 | P1 只引 `apalis` + `apalis-postgres`，cron/workflow/board 延后 |

---

## 19. 参考链接

| 主题 | 链接 |
|---|---|
| apalis 主文档 | <https://docs.rs/apalis/latest/apalis/> |
| apalis-core（Worker / Backend / TaskSink / Monitor） | <https://docs.rs/apalis-core/latest/apalis_core/> |
| apalis-postgres（PostgresStorage / 迁移） | <https://docs.rs/apalis-postgres/latest/apalis_postgres/> |
| apalis-cron（CronScheduler） | <https://docs.rs/apalis-cron/latest/apalis_cron/> |
| apalis-workflow（SteppedFlow / GraphFlow） | <https://docs.rs/apalis-workflow/latest/apalis_workflow/> |
| apalis-board（axum / actix / salvo / web） | <https://docs.rs/apalis-board/latest/apalis_board/> |

仓库内相关设计：

- `docs/文档解析后向量化数据缓存方案.md`
- `docs/Rust原生多模态解析+pgvector多模态RAG设计文档_V1.7整合版.md`
- `docs/业务网关（axum）多实例部署设计方案.md`
- `docs/大文件分片上传与断点续传设计.md`
- `crates/task-queue/README.md`（本方案的索引与快速上手）

---

## 附录 A：P1 需要同步改动的文件清单

| 文件 | 改动 |
|---|---|
| `apps/backend/src-tauri/Cargo.toml` | `[workspace.dependencies]` 追加 apalis 四件套（§6.1） |
| `crates/task-queue/Cargo.toml` | 打开依赖（§6.2） |
| `crates/task-queue/src/{config,job,error,producer,worker}.rs` | 新增（§5.1） |
| `crates/bootstrap/src/lib.rs` | 新增 `init_task_queue()` 并在启动流程调用（§14.2） |
| `crates/gateway/src/lib.rs` | `serve()` 中启动 worker + 关闭信号接通（§9.1） |
| `crates/api/src/knowledge_routes.rs` / `upload_routes.rs` | 解析接口改为 push + `202`（§8.1） |
| `crates/api/src/lib.rs` | 注册 `GET /api/tasks/{taskId}`，并进 apidoc |
| `crates/database/src/postgres.rs` | 新增 `task_ticket` 建表（public + 租户 schema） |
| `docs/学习进度.md` | 登记本方案 P0 状态（如该文件用于进度跟踪） |

## 附录 C：P2/P3 实际改动清单（已交付）

| 文件 | 改动 |
|---|---|
| `crates/database/src/sql/task_queue_migration.sql` | 新增：`task_ticket` 表 + 索引 + 注释（`{schema}` 占位） |
| `crates/database/src/postgres.rs` | 新增 `migrate_tenant_task_queue_tables` / `migrate_all_tenant_task_queue_tables`，并在 `init_tenant_tables` 与启动流程（11.4）调用 |
| `crates/task-queue/src/ticket.rs` | 新增：票据 `open` / `mark_running` / `mark_stage` / `mark_done` / `mark_failed` / `get` + schema 白名单校验 |
| `crates/task-queue/src/maintenance.rs` | 新增：`apalis-cron` 维护（票据收敛 + `Metrics` 打点 + 咨询锁互斥） |
| `crates/task-queue/src/{job,producer,worker}.rs` | `ParseTask`/`VectorizeTask` 增加 `task_id`；入队写票据；worker 按阶段回写票据 |
| `crates/api/src/task_routes.rs` | 新增：`POST /api/tasks/parse`（202）、`GET /api/tasks/{taskId}` |
| `crates/api/src/task_sink.rs` | 新增：`BusinessParseSink`（正文为空时回填 `knowledge_asset.content`） |
| `crates/api/src/lib.rs` | 注册 `/api/tasks/*` 路由与两个新模块 |
| `crates/api/src/authz.rs` | 新增两条权限点映射（`knowledge:edit` / `knowledge:list`） |
| `crates/bootstrap/src/lib.rs` | 新增 `TaskQueueRuntime`，`serve_http` 内与 HTTP 同生命周期启动/关闭 |
| `crates/gateway/src/lib.rs` | `serve()` 内启动 worker，退出时优雅停止 |
| `crates/task-queue/src/ticket.rs` | 追加 `list()`（状态白名单 + limit 夹逼）供任务中心页使用 |
| `crates/api/src/task_routes.rs` | 追加 `GET /api/tasks`；`assetId` 入参/出参字符串化（防 JS 精度丢失）；`TaskItem` 输出 DTO |
| `apps/web/src/services/taskService.ts` | 新增：`listParseTasks` / `getParseTask` / `pushParseTask` + 状态/阶段展示映射 |
| `apps/web/src/utils/adapters/httpAdapter.ts` | 登记 `push_parse_task` / `list_parse_tasks` / `get_parse_task` 三条映射 |
| `apps/web/src/app/tasks/page.tsx` | 新增：任务中心页（列表 + 筛选 + 自动刷新 + 进度条 + 展开错误） |
| `apps/web/src/app/knowledge-asset/page.tsx` | 资产卡片新增「解析」按钮（未绑定文件时禁用） |
| `apps/web/src/tests/services/taskService.test.ts` | 新增：路由映射 / query / body / 展示映射契约测试 |
| `crates/database/src/sql/public_{initial_data,migration}.sql` | 新增菜单 id=240（页面）与 241~243（权限点），幂等 |
| 根 `Cargo.toml` | 新增 `apalis-cron` / `cron`（`apalis-workflow` / `apalis-board` 经核实不可用，未引入） |

## 附录 D：上游能力核实记录（2026-11，apalis 1.2.0 / board 1.3.0）

| 事实 | 证据 | 影响 |
|---|---|---|
| `PostgresStorage` 实现 `ListQueues` / `ListTasks` / `ListAllTasks` / `Metrics` / `FetchById` | `apalis-postgres-1.2.0/src/queries/*.rs` 的 `impl` | 队列指标与看板的部分能力可用 |
| `PostgresStorage` **未实现** `ListWorkers` | 全 crate 无 `impl … ListWorkers` | `apalis-board` 的 axum `RegisterRoute` 编译不通过 → 看板改独立部署 |
| `PostgresStorage` **未实现** `Vacuum` / `ResumeAbandoned` | 只有 `queries/reenqueue_orphaned.rs` 查询函数 | 孤儿回收依赖 heartbeat；维护 cron 只收敛票据 |
| `TaskBuilder` 无 `timeout`、`WorkerBuilder` 无 `concurrency` | `apalis-core-1.2.0/src/task/builder.rs` / `worker/builder.rs` | 超时需 worker layer，并发用多 worker |
| `run_until(signal)` 在 signal 完成时自动 `ctx.stop()` | `apalis-core-1.2.0/src/worker/mod.rs:245` | 关闭信号返回 `Ok(())` 即可 |

## 附录 B：常见问题（FAQ）

**Q1：已经有 `crates/mq` 了，为什么还要 apalis？**
`mq` 是事件总线（发布/订阅，进程内或 Pulsar），消息消费完即丢，没有「待办任务」概念；
apalis 是任务表 + 状态机 + 重试 + 看板。二者可共存：apalis 负责「必须完成」，mq 负责
「通知一下」。

**Q2：为什么不直接 `tokio::spawn` + 重试循环？**
重启即丢、无法跨实例分摊、没有进度与人工重放、重试/超时/限流都要自己写。自研一套的成本
远高于接入 apalis 的改造成本。

**Q3：解析为什么不用 Rust 一次性重写，还要 Python 侧车？**
`crates/parser` 已覆盖 PDF/Office（`LOCAL_SUPPORTED_EXTS`），视频/音频/OCR 仍在
`apps/doc-parser`。apalis 与「解析实现选型」正交：任务里调 `DocParserClient` 即可，
将来 `parser` 补齐视频能力只换实现，不动队列。

**Q4：任务参数里为什么一定要带 `tenant`？**
见 §10.1，这是 worker 里唯一的租户上下文来源；缺了就会写错库。

**Q5：能不能用 `apalis-redis`？**
可以但要新增 Redis 依赖，与网关「单一 PG 依赖」的既定策略冲突；只有压测证实 PG 轮询
成为瓶颈时才考虑，届时需单独写选型说明。

**Q6：cron 表达式为什么是 6 段？**
`apalis-cron` 用 `cron` crate，格式为「秒 分 时 日 月 周」，例如 `0 30 3 * * *` 是每天
03:30:00；写 5 段会解析失败。

**Q7：任务失败了怎么人工重放？**
短期：看板里 `ResumeById`；长期：业务侧「重新解析」按钮 → 新 `dedupe_key`（清空失败态）
重新 `push`，复用同一条 `task_ticket`。











