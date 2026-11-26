//! 异步任务队列（`apalis`）：视频 / 音频 / 文档解析等耗时任务的契约与装配。
//!
//! 设计文档：[`DESIGN_DOC`]（仓库根目录 `docs/apalis异步任务队列开发文档.md`）。
//!
//! # 为什么单独成 crate
//!
//! 解析类任务的耗时是分钟级（视频抽帧 + ASR + OCR），当前实现把它们压在 HTTP 请求里
//! （`crates/service/src/doc_parser.rs` 的客户端超时被放宽到 600s），一旦客户端断开或
//! 实例重启，任务就丢失且无法重试。本 crate 负责把这类任务从「请求生命周期」搬到
//! 「持久化队列 + 常驻 worker」。
//!
//! # 依赖方向（硬约束）
//!
//! ```text
//! api / gateway / bootstrap ──依赖──► task-queue ──依赖──► parser / service / storage / database
//! ```
//!
//! - 本 crate **只依赖** `parser` / `service` / `storage` / `database` 等基础设施；
//! - **禁止**反向依赖 `api` / `gateway` / `bootstrap`（否则与现有 api ↔ service 分层冲突，
//!   且桌面形态会把 Tauri 拖进来）；
//! - 任务参数一律走序列化契约（`serde`），不跨层传 `Arc<dyn Trait>` 之类的运行时句柄。
//!
//! # 多租户红线
//!
//! worker 脱离 HTTP 请求后没有 `database::with_schema` 的 task-local 上下文，
//! `database::schema_prefix()` 会回落到 `GLOBAL_SCHEMA`。因此**每个**任务都必须在
//! `database::with_schema(task.tenant, ...)` 内执行，任务参数必须自带租户 schema 名。
//! 详见设计文档 §10.1。
//!
//! # 当前进度
//!
//! | 阶段 | 内容 | 状态 |
//! |------|------|------|
//! | P0 | crate 骨架 + 设计文档 + 根 Cargo.toml 登记 | ✅ |
//! | P1 | `ParseTask`/`VectorizeTask` 契约、`producer`（入队）、`worker`（解析 + 向量化执行）、`TASK_QUEUE_MODE` 灰度 | ✅ |
//! | P2 | `apalis-cron` 孤儿任务回收、`apalis-board` 看板、`apalis-workflow` 视频链路 | ⏳ |
//! | P3 | 每租户配额与限流、进度表 `task_ticket`、`GET /api/tasks/{id}` 路由接线 | ⏳ |
//!
//! P1 只交付**与业务无关**的能力：配置、契约、入队、worker 执行与解析结果钩子。
//! 接线（`api` 路由、`gateway`/`bootstrap` 启动、业务落库）由调用方完成，
//! 本 crate 保持「不依赖 `api` / `gateway`」的边界（见 §5.3）。

#![allow(
    clippy::missing_errors_doc,
    reason = "错误统一由 TaskError 表达，语义见设计文档 §9.6"
)]

pub mod config;
pub mod error;
pub mod job;
pub mod maintenance;
pub mod producer;
pub mod ticket;
pub mod worker;

use std::sync::Arc;

use tokio::sync::watch;
use tokio::task::JoinHandle;

pub use config::{Mode, TaskQueueConfig};
pub use error::TaskError;
pub use job::{
    CONTRACT_VERSION, DocType, ParseTask, QUEUE_PARSE, QUEUE_PARSE_VIDEO, QUEUE_VECTORIZE,
    VectorizeTask,
};
pub use producer::{push_parse, push_vectorize, setup};
pub use ticket::{NewTicket, TaskTicket};
pub use worker::{LoggingSink, ParseResultSink, TaskDeps, spawn_all};

/// 设计文档在**仓库根目录**下的相对路径（`assets-platform/docs/apalis异步任务队列开发文档.md`）。
///
/// 保留为常量而不是 `include_str!`：文档会持续更新，且不希望每次改文档都触发重编译。
pub const DESIGN_DOC: &str = "docs/apalis异步任务队列开发文档.md";

/// 一键启动（解析结果只记日志）：建 `apalis` 表 + 按配置起 worker
pub async fn start(
    pool: sqlx::PgPool,
    cfg: TaskQueueConfig,
    shutdown: watch::Receiver<bool>,
) -> Result<Vec<JoinHandle<()>>, TaskError> {
    start_with_deps(Arc::new(TaskDeps::new(pool)), cfg, shutdown).await
}

/// 一键启动（业务侧注入解析结果落库实现）
pub async fn start_with_sink(
    pool: sqlx::PgPool,
    cfg: TaskQueueConfig,
    sink: Arc<dyn ParseResultSink>,
    shutdown: watch::Receiver<bool>,
) -> Result<Vec<JoinHandle<()>>, TaskError> {
    let deps = Arc::new(TaskDeps::new(pool).with_sink(sink));
    start_with_deps(deps, cfg, shutdown).await
}

/// 启动骨架：`TASK_QUEUE_MODE=inline` 时**不建表、不起 worker**（零副作用，可随时回滚）
pub async fn start_with_deps(
    deps: Arc<TaskDeps>,
    cfg: TaskQueueConfig,
    shutdown: watch::Receiver<bool>,
) -> Result<Vec<JoinHandle<()>>, TaskError> {
    if !cfg.queue_enabled() {
        tracing::info!(
            mode = cfg.mode.as_str(),
            "任务队列未开启（inline），跳过建表与 worker 启动"
        );
        return Ok(Vec::new());
    }
    setup(&deps.pool).await?;
    let mut handles = spawn_all(deps.clone(), cfg, shutdown.clone());
    // 维护 cron（票据收敛 + 队列指标）：所有实例都起，靠咨询锁互斥
    handles.push(maintenance::spawn(deps.pool.clone(), shutdown));
    Ok(handles)
}
