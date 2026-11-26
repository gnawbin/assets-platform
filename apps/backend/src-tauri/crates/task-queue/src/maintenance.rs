//! 定时维护（`apalis-cron`）
//!
//! P2 的维护任务只做两件**业务侧**的事（不碰 apalis 内部表结构）：
//! 1. 收敛停滞票据：实例被强杀后 `task_ticket` 会永远停在 `pending` / `running`，
//!    超过阈值统一置为 `failed`，避免前端一直转圈；
//! 2. 输出队列积压指标（`Metrics::fetch_by_queue`），供日志/告警观察。
//!
//! 关于「孤儿任务回收」：apalis-postgres 1.2.0 **没有**实现 `Vacuum` / `ResumeAbandoned`
//! （`src/` 下只有 `queries/reenqueue_orphaned.rs` 的查询函数，无 trait impl），
//! 孤儿任务由 apalis-postgres 自带的 heartbeat（`Config::missed_heartbeats`）负责重新入队；
//! 这边只兜底「票据状态」这一层，不直接改 `apalis.jobs`。
//!
//! 多实例安全：所有实例都会按 cron 触发，但执行前先抢 `pg_try_advisory_lock`，
//! 抢不到说明别的实例正在跑，直接跳过（等价于 leader 执行）。

use std::str::FromStr;

use apalis::prelude::{BoxDynError, Data, Metrics, WorkerBuilder, WorkerBuilderExt, WorkerError};
use apalis_cron::{CronScheduler, Tick};
use apalis_postgres::PostgresStorage;
use cron::Schedule;
use sqlx::PgPool;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::error::TaskError;
use crate::ticket;

/// 维护任务的咨询锁 key（与网关清理 leader 的 `CLEANUP_LOCK_KEY` 不同，互不阻塞）
pub const MAINTENANCE_LOCK_KEY: i64 = 0x5154_4D41_494E_5400;

/// 缺省 cron：每天 03:30:00（六段：秒 分 时 日 月 周）
pub const DEFAULT_MAINTENANCE_CRON: &str = "0 30 3 * * *";

/// 票据停滞阈值（小时）：超过该时长仍非终态，视为实例已丢失
pub const STALE_TICKET_HOURS: i32 = 2;

/// 读取 cron 表达式（`TASK_QUEUE_MAINTENANCE_CRON`），非法值回落默认并告警
///
/// # Panics
///
/// 仅当内置常量 [`DEFAULT_MAINTENANCE_CRON`] 被改坏（不再是合法六段表达式）时 panic；
/// 该常量由单测锁定，正常运行不会触发。
#[must_use]
pub fn schedule_from_env() -> Schedule {
    let raw = std::env::var("TASK_QUEUE_MAINTENANCE_CRON").unwrap_or_default();
    match parse_schedule(&raw) {
        Ok(schedule) => schedule,
        Err(e) => {
            tracing::warn!(error = %e, "维护任务 cron 表达式无效，回落默认：{DEFAULT_MAINTENANCE_CRON}");
            Schedule::from_str(DEFAULT_MAINTENANCE_CRON)
                .expect("❌ 内置 cron 表达式必须合法（六段）")
        }
    }
}

/// 解析 cron 表达式（空串 → 默认；非法 → `TaskError::Invalid`，便于单测）
///
/// ⚠️ `cron` crate 需要 **6 段**（秒 分 时 日 月 周），写 5 段会解析失败。
///
/// # Errors
///
/// 表达式非法（段数不对 / 取值越界）时返回 [`TaskError::Invalid`]。
pub fn parse_schedule(raw: &str) -> Result<Schedule, TaskError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Schedule::from_str(DEFAULT_MAINTENANCE_CRON)
            .map_err(|e| TaskError::Invalid(format!("内置 cron 表达式非法: {e}")));
    }
    Schedule::from_str(raw)
        .map_err(|e| TaskError::Invalid(format!("cron 表达式非法（需 6 段，含秒）{raw}: {e}")))
}

/// 启动维护 cron worker（进程内一个即可；多个实例靠咨询锁互斥）
pub fn spawn(pool: PgPool, shutdown: watch::Receiver<bool>) -> JoinHandle<()> {
    let schedule = schedule_from_env();
    let worker = WorkerBuilder::new("task-queue-maintenance")
        .backend(CronScheduler::new(schedule))
        .data(pool)
        .enable_tracing()
        .build(handle_tick);

    tokio::spawn(async move {
        let mut shutdown = shutdown;
        let signal = async move {
            let _ = shutdown.changed().await;
            Ok::<(), WorkerError>(())
        };
        if let Err(e) = worker.run_until(signal).await {
            tracing::error!(error = %e, "维护 cron worker 异常退出");
        } else {
            tracing::info!("维护 cron worker 已优雅退出");
        }
    })
}

/// 一次 tick：抢锁 → 收敛票据 + 打点 → 解锁
async fn handle_tick(_tick: Tick, pool: Data<PgPool>) -> Result<(), BoxDynError> {
    let result = run_maintenance(&pool).await;
    result.map_err(TaskError::into_box)
}

/// 执行维护（抢不到锁直接返回 Ok）
async fn run_maintenance(pool: &PgPool) -> Result<(), TaskError> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| TaskError::Transient(format!("获取维护连接失败: {e}")))?;

    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(MAINTENANCE_LOCK_KEY)
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| TaskError::Transient(format!("抢维护锁失败: {e}")))?;
    if !locked {
        tracing::debug!("另一个实例正在执行任务队列维护，本轮跳过");
        return Ok(());
    }

    let outcome = async {
        let reconciled = reconcile_stale_tickets(pool).await?;
        log_queue_metrics(pool).await;
        Ok::<u64, TaskError>(reconciled)
    }
    .await;

    // 无论成功失败都要解锁（连接归还池后会话锁不会自动释放，必须显式 unlock）
    if let Err(e) = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(MAINTENANCE_LOCK_KEY)
        .execute(&mut *conn)
        .await
    {
        tracing::warn!(error = %e, "释放维护锁失败（会话回收后自动释放）");
    }

    let reconciled = outcome?;
    tracing::info!(reconciled, "任务队列维护完成");
    Ok(())
}

/// 收敛停滞票据：`pending` / `running` 超过 `STALE_TICKET_HOURS` 一律置失败
async fn reconcile_stale_tickets(pool: &PgPool) -> Result<u64, TaskError> {
    let schemas: Vec<String> = sqlx::query_scalar(
        "SELECT schema_name FROM public.sys_tenant WHERE enable = true AND schema_name IS NOT NULL",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| TaskError::Transient(format!("读取租户列表失败: {e}")))?;

    let mut total = 0u64;
    for raw in schemas {
        let Ok(schema) = ticket::sanitize_schema(&raw) else {
            continue;
        };
        let sql = format!(
            "UPDATE {schema}.task_ticket SET \
             status = 'failed', \
             error = COALESCE(error, '任务未在预期时间内完成（维护任务收敛，实例可能已下线）'), \
             finished_at = NOW(), updated_at = NOW() \
             WHERE status IN ('pending', 'running') AND updated_at < NOW() - make_interval(hours => $1)"
        );
        match sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(STALE_TICKET_HOURS)
            .execute(pool)
            .await
        {
            Ok(result) => total += result.rows_affected(),
            Err(e) => tracing::warn!(schema = %schema, error = %e, "收敛停滞票据失败"),
        }
    }
    Ok(total)
}

/// 输出队列积压（失败只告警，不影响维护）
async fn log_queue_metrics(pool: &PgPool) {
    let storage: PostgresStorage<()> = PostgresStorage::new(pool);
    match storage.fetch_by_queue().await {
        Ok(stats) => {
            for stat in stats {
                tracing::info!(metric = %stat.title, value = %stat.value, "apalis 队列指标");
            }
        }
        Err(e) => tracing::warn!(error = %e, "读取 apalis 队列指标失败"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_cron_is_six_fields_and_parsable() {
        assert_eq!(
            DEFAULT_MAINTENANCE_CRON.split_whitespace().count(),
            6,
            "❌ cron 必须是六段（含秒）"
        );
        assert!(parse_schedule(DEFAULT_MAINTENANCE_CRON).is_ok());
    }

    #[test]
    fn test_blank_cron_falls_back_to_default() {
        assert!(parse_schedule("   ").is_ok(), "❌ 空串应回落内置默认表达式");
    }

    #[test]
    fn test_invalid_cron_is_rejected() {
        assert!(parse_schedule("0 30 3 * *").is_err(), "❌ 5 段应报错");
        assert!(parse_schedule("not-a-cron").is_err());
    }

    #[test]
    fn test_lock_key_differs_from_gateway_cleanup_key() {
        // 与 gateway::worker::CLEANUP_LOCK_KEY（0x4757_434C_4541_4E55）必须不同
        assert_ne!(MAINTENANCE_LOCK_KEY, 0x4757_434C_4541_4E55);
    }
}
