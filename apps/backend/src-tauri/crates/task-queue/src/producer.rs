//! 生产者：唯一的入队入口
//!
//! 两条纪律（设计文档 §8.2 / §8.6）：
//! 1. **只入队，不执行**：解析、下载、向量化全部在 worker 里做；
//! 2. **失败要显式**：队列未开启 / 部署未启用视频队列 / 参数非法 → 直接返回错误，
//!    由调用方决定是 503 还是回退到 inline（`Mode::fail_open` 的判断在调用方）。

use apalis::prelude::{TaskBuilder, TaskId, TaskSink};
use apalis_postgres::{Config, PostgresStorage};
use sqlx::PgPool;

use crate::config::TaskQueueConfig;
use crate::error::TaskError;
use crate::job::{ParseTask, VectorizeTask};
use crate::ticket;

/// 建 `apalis` schema 与表（幂等）
///
/// 必须在 `database::init_database()` **之后**调用（业务表先就绪、连接池已建立）。
pub async fn setup(pool: &PgPool) -> Result<(), TaskError> {
    PostgresStorage::<()>::setup(pool)
        .await
        .map_err(|e| TaskError::Transient(format!("apalis 迁移失败: {e}")))
}

/// 入队解析任务，返回业务侧 task id（UUID 字符串，同时作为 `apalis` 的任务 id）
///
/// 返回的 id 可直接用于 `GET /api/tasks/{id}` 的进度查询与日志串联。
pub async fn push_parse(
    pool: &PgPool,
    task: ParseTask,
    cfg: &TaskQueueConfig,
    created_by: Option<i64>,
) -> Result<String, TaskError> {
    task.validate()?;

    if !cfg.queue_enabled() {
        return Err(TaskError::Invalid(format!(
            "任务队列未开启（TASK_QUEUE_MODE={}）",
            cfg.mode.as_str()
        )));
    }
    if task.doc_type.is_heavy() && !cfg.video_enabled {
        return Err(TaskError::Invalid(
            "视频/音频队列未开启（TASK_QUEUE_VIDEO_ENABLED=0）".to_string(),
        ));
    }

    // 幂等键要在 move 之前算好
    let mut task = task;
    let idempotency_key = task.idempotency_key();
    let queue = task.queue();
    // 重任务（视频/音频）失败代价高、重试噪声大，只重试 1 次；文档允许 2 次
    let max_attempts = if task.doc_type.is_heavy() { 2 } else { 3 };
    let priority = if task.doc_type.is_heavy() { 1 } else { 2 };

    let task_id = uuid::Uuid::new_v4().to_string();
    task.task_id = Some(task_id.clone());

    // 业务进度票据：写失败只告警（不影响入队），worker 侧按缺省值继续
    let tenant = task.tenant.clone();
    if let Err(e) = ticket::open(
        pool,
        &tenant,
        &ticket::NewTicket {
            task_id: &task_id,
            task_type: "parse",
            queue,
            asset_id: Some(task.asset_id),
            dedupe_key: Some(&idempotency_key),
            trace_id: task.trace_id.as_deref(),
            created_by,
        },
    )
    .await
    {
        tracing::warn!(error = %e, "写入任务票据失败（继续入队）");
    }

    let mut storage: PostgresStorage<ParseTask> = PostgresStorage::new(pool)
        .with_config(Config::default().queue(queue).batch_size(cfg.batch_size));

    let built = TaskBuilder::new(task)
        .task_id(TaskId::from_string(task_id.clone()))
        .idempotency_key(idempotency_key)
        .max_attempts(max_attempts)
        .priority(priority)
        .build();

    if let Err(e) = storage.push_task(built).await {
        let detail = format!("入队失败（queue={queue}）: {e}");
        if let Err(ticket_err) = ticket::mark_failed(pool, &tenant, &task_id, &detail).await {
            tracing::warn!(error = %ticket_err, "标记任务票据失败状态失败");
        }
        return Err(TaskError::Transient(detail));
    }

    tracing::info!(
        task_id = %task_id,
        queue,
        max_attempts = max_attempts as u64,
        "解析任务已入队"
    );
    Ok(task_id)
}

/// 入队向量化任务，返回业务侧 task id
pub async fn push_vectorize(
    pool: &PgPool,
    task: VectorizeTask,
    cfg: &TaskQueueConfig,
    created_by: Option<i64>,
) -> Result<String, TaskError> {
    task.validate()?;

    if !cfg.queue_enabled() {
        return Err(TaskError::Invalid(format!(
            "任务队列未开启（TASK_QUEUE_MODE={}）",
            cfg.mode.as_str()
        )));
    }

    let mut task = task;
    let idempotency_key = task.idempotency_key();
    let queue = task.queue();

    let task_id = uuid::Uuid::new_v4().to_string();
    task.task_id = Some(task_id.clone());

    let tenant = task.tenant.clone();
    if let Err(e) = ticket::open(
        pool,
        &tenant,
        &ticket::NewTicket {
            task_id: &task_id,
            task_type: "vectorize",
            queue,
            asset_id: Some(task.asset_id),
            dedupe_key: Some(&idempotency_key),
            trace_id: task.trace_id.as_deref(),
            created_by,
        },
    )
    .await
    {
        tracing::warn!(error = %e, "写入任务票据失败（继续入队）");
    }

    let mut storage: PostgresStorage<VectorizeTask> = PostgresStorage::new(pool)
        .with_config(Config::default().queue(queue).batch_size(cfg.batch_size));

    let built = TaskBuilder::new(task)
        .task_id(TaskId::from_string(task_id.clone()))
        .idempotency_key(idempotency_key)
        .max_attempts(5)
        .priority(2)
        .build();

    if let Err(e) = storage.push_task(built).await {
        let detail = format!("入队失败（queue={queue}）: {e}");
        if let Err(ticket_err) = ticket::mark_failed(pool, &tenant, &task_id, &detail).await {
            tracing::warn!(error = %ticket_err, "标记任务票据失败状态失败");
        }
        return Err(TaskError::Transient(detail));
    }

    tracing::info!(task_id = %task_id, queue, "向量化任务已入队");
    Ok(task_id)
}
