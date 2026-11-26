# task-queue（apalis 异步任务队列）

把「分钟级」的解析类任务从 HTTP 请求生命周期里搬出来：视频抽帧 + ASR + OCR、音频转写、
大文档解析、批量向量化、离线导出等。持久化在 PostgreSQL，跨实例分摊，失败可重试、
可重放、可看板观测。

> 正式设计文档：**[`docs/apalis异步任务队列开发文档.md`](../../../../../docs/apalis异步任务队列开发文档.md)**
> （本文只做索引与快速上手，冲突时以设计文档为准）

## 1. 为什么需要它

现有实现把解析压在请求里，问题不是「慢」，而是「丢了就没了」：

| 位置 | 现状 | 后果 |
|---|---|---|
| `crates/service/src/doc_parser.rs` | `DocParserClient` 超时放宽到 **600s**，在请求内同步调用 | 网关/代理先超时；客户端断开后解析结果无人接收 |
| `crates/service/src/conversation_service.rs` | 聊天附件（含视频）在请求内 `download_object` → `parse_file` → 帧转 data URL | 一条消息最坏占用一个 HTTP 连接 10 分钟 |
| `crates/api/src/lib.rs` | 后台清理用 `tokio::spawn` + `sleep` | 重启即丢，无重试、无进度 |
| `crates/mq` | 进程内 pub/sub（可选 Pulsar）契约 | 只做事件广播，**不持久化**、不重放 |
| `crates/workflow` | BPMN / adk-graph 业务流（审批、会签） | 不是任务队列，不解决「谁执行、失败怎么办」 |

`apalis` 补的正是这一层：`apalis.jobs` 表 + worker + 重试/超时/限流/长任务锁续期 + 看板。

## 2. 依赖方向（硬约束）

```
api / gateway / bootstrap ──依赖──► task-queue ──依赖──► parser / service / storage / database
```

- 任务参数只走 `serde` 序列化契约，不跨进程传运行时句柄；
- 禁止 `task-queue → api / gateway` 反向依赖（会与既有 `api ↔ service` 分层打架，桌面形态还会拖进 Tauri）。

## 3. 快速上手（P1/P3 已实现）

```rust
use std::sync::Arc;
use task_queue::{ParseTask, TaskQueueConfig, push_parse};
use tokio::sync::watch;

// 1) 启动：TASK_QUEUE_MODE=queue 时建 apalis 表 + task_ticket + 起 worker / 维护 cron
let cfg = TaskQueueConfig::from_env();
let (shutdown_tx, shutdown_rx) = watch::channel(false);
let workers = task_queue::start(pool.clone(), cfg.clone(), shutdown_rx).await?;

// 2) 生产：入队后立即返回 task_id（同时写 task_ticket），HTTP 响应 202
let mut task = ParseTask::new(tenant_schema, asset_id, "s3://bucket/key.mp4", file_name);
task.title = title;
task.options = serde_json::json!({ "skip_index": false });
let task_id = push_parse(&pool, task, &cfg, Some(user_id)).await?;

// 3) 查进度：GET /api/tasks/{taskId}  → 读 task_ticket（status / stage / progress）
// 4) 关闭：置 true → worker 停止领取新任务并优雅退出
let _ = shutdown_tx.send(true);
```

三条不可省略的约束：

1. **`database::with_schema(task.tenant, ...)` 包裹任务体** —— worker 内部已经做了，但业务侧
   实现 `ParseResultSink` 落库时不要自己再拼 schema（见设计文档 §10.1）；
2. **payload 只放引用不放字节**（`s3://bucket/key`），禁止本机绝对路径与长期凭证；
3. **默认 `TASK_QUEUE_MODE=inline`** —— 未显式开启时行为与改造前完全一致，可一键回滚。

## 4. 已实现能力

| 模块 | 内容 |
|---|---|
| `config` | `TaskQueueConfig::from_env()`、`Mode::{Inline, Queue, QueueFailOpen}`，非法值回落默认 |
| `job` | `ParseTask` / `VectorizeTask` 契约、`DocType` 路由（音视频独立队列）、幂等键、参数校验 |
| `producer` | `setup()`（建 apalis schema）、`push_parse()` / `push_vectorize()`（task_id + 幂等键 + 写票据） |
| `worker` | `spawn_all()`：`long_running()` + `enable_tracing()` + `run_until` 优雅关闭；解析/向量化执行体；`ParseResultSink` 钩子；重任务信号量；临时目录 RAII；**阶段进度回写票据** |
| `ticket` | `task_ticket` 读写：`open` / `mark_running` / `mark_stage` / `mark_done` / `mark_failed` / `get`（schema 白名单校验） |
| `maintenance` | `apalis-cron` 定时维护：收敛停滞票据 + 输出队列指标，`pg_try_advisory_lock` 保证单实例执行 |
| `error` | `TaskError::{Invalid, Transient, Fatal}` → apalis 的「不可重试 / 可重试」语义 |

接线（P3）：

| 位置 | 内容 |
|---|---|
| `crates/api` | `POST /api/tasks/parse`（202 + taskId）、`GET /api/tasks`（列表）、`GET /api/tasks/{taskId}`、`BusinessParseSink`（正文为空时回填）、authz 权限点登记 |
| `crates/bootstrap` | `TaskQueueRuntime::start/shutdown`，`serve_http` 内与 HTTP 同生命周期启动/关闭（失败只降级） |
| `crates/gateway` | 多实例启动 worker + 关闭时优雅停止 |
| `crates/database` | `task_ticket` 建表 SQL（新租户 + 存量租户启动迁移）+ 菜单种子（任务中心页 + 权限点） |
| `apps/web` | 任务中心页 `/tasks`、知识资产页「解析」按钮、`taskService` + `httpAdapter` 映射 |

测试：`cargo test -p task-queue`（纯单元测试，不需要数据库）。

## 5. 尚未落地（有明确原因）

| 项 | 状态 | 原因 |
|---|---|---|
| `apalis-board` 看板 | ❌ 阻塞 | `apalis-board 1.3` 的 axum `RegisterRoute` 要求后端实现 `ListWorkers`，而 `apalis-postgres 1.2.0` **未实现**（只有 `ListQueues` / `ListTasks` / `ListAllTasks` / `Metrics` / `FetchById`）→ 编译期不成立；改用官方独立镜像或等上游补齐 |
| `apalis-workflow` 视频分阶段链路 | ⏳ 延后 | `SteppedFlow` 的每一步是**独立持久化任务**，需要每步从参数重建 `with_schema` 租户上下文与重试语义；本期先保证单任务链路（下载→解析→向量化）正确，分阶段链路等 P4 |
| `Vacuum` / `ResumeAbandoned` | ❌ 不存在 | apalis-postgres 1.2 未实现这两个 trait；孤儿任务由后端 heartbeat（`Config::missed_heartbeats`）负责，维护 cron 只兜底票据状态 |

## 6. 当前进度

| 阶段 | 内容 | 状态 |
|---|---|---|
| P0 | crate 骨架 + 设计文档 + 根 `Cargo.toml` 登记 | ✅ |
| P1 | 配置 / 契约 / 入队 / worker / 解析 + 向量化 / 错误分类 / 单元测试 | ✅ |
| P2 | 定时维护（票据收敛 + 队列指标 + 咨询锁）；board/workflow 结论已核实 | ✅（board/workflow 见 §5） |
| P3 | `task_ticket` 表、api 路由 + 业务落库钩子、bootstrap/gateway 接线 | ✅ |
