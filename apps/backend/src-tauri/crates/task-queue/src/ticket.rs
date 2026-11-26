//! 业务进度票据（`task_ticket`）
//!
//! 为什么需要它（设计文档 §7.5 / §14.3）：
//! - `apalis.jobs` 是**全局基础设施表**（跨租户、字段是 apalis 的语义），不适合直接暴露给前端；
//! - 前端要的是「我这个资产解析到哪一步了」，因此每个租户自己有一张 `task_ticket`；
//! - 票据 id 与 apalis 任务 id 相同（都由生产者生成 UUID），两边一一对应。
//!
//! 安全约束：
//! - schema 名只可能来自服务端（`database::current_schema_name()`），这里仍做白名单校验，
//!   杜绝把用户输入拼进 SQL；
//! - `error` 写入前截断到 4KB，避免把整段堆栈塞进业务表。

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use sqlx::types::chrono::{DateTime, Utc};

use crate::error::TaskError;

/// 状态：已入队
pub const STATUS_PENDING: &str = "pending";
/// 状态：执行中
pub const STATUS_RUNNING: &str = "running";
/// 状态：已完成
pub const STATUS_COMPLETED: &str = "completed";
/// 状态：已失败（重试耗尽或不可重试）
pub const STATUS_FAILED: &str = "failed";

/// 错误摘要的长度上限（字符数）
const MAX_ERROR_CHARS: usize = 4096;

/// 进度票据（`GET /api/tasks/{taskId}` 的返回体）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TaskTicket {
    /// 任务 id（与 apalis 任务 id 相同）
    pub id: String,
    /// 任务类型：parse / vectorize
    pub task_type: String,
    /// 队列名
    pub queue: String,
    /// 关联资产 id
    pub asset_id: Option<i64>,
    /// 租户 schema
    pub tenant_schema: String,
    /// 状态：pending / running / completed / failed
    pub status: String,
    /// 阶段：queued / downloading / parsing / chunking / done
    pub stage: Option<String>,
    /// 进度百分比 0~100
    pub progress: i16,
    /// 已尝试次数
    pub attempts: i32,
    /// 最近一次失败原因（已截断）
    pub error: Option<String>,
    /// 调用链追踪 id
    pub trace_id: Option<String>,
    /// 幂等键
    pub dedupe_key: Option<String>,
    /// 发起人
    pub created_by: Option<i64>,
    /// 入队时间
    pub created_at: DateTime<Utc>,
    /// 开始执行时间
    pub started_at: Option<DateTime<Utc>>,
    /// 结束时间
    pub finished_at: Option<DateTime<Utc>>,
    /// 最近更新时间
    pub updated_at: DateTime<Utc>,
}

/// 新建票据的入参
pub struct NewTicket<'a> {
    /// 任务 id（UUID）
    pub task_id: &'a str,
    /// 任务类型：parse / vectorize
    pub task_type: &'a str,
    /// 队列名
    pub queue: &'a str,
    /// 关联资产 id
    pub asset_id: Option<i64>,
    /// 幂等键
    pub dedupe_key: Option<&'a str>,
    /// 调用链追踪 id
    pub trace_id: Option<&'a str>,
    /// 发起人
    pub created_by: Option<i64>,
}

/// 写入票据（幂等：同 id 重复写入不报错，便于重放）
pub async fn open(pool: &PgPool, schema: &str, ticket: &NewTicket<'_>) -> Result<(), TaskError> {
    let schema = sanitize_schema(schema)?;
    let sql = format!(
        "INSERT INTO {schema}.task_ticket \
         (id, task_type, queue, asset_id, tenant_schema, status, stage, progress, dedupe_key, trace_id, created_by) \
         VALUES ($1, $2, $3, $4, $5, '{STATUS_PENDING}', 'queued', 0, $6, $7, $8) \
         ON CONFLICT (id) DO NOTHING"
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(ticket.task_id)
        .bind(ticket.task_type)
        .bind(ticket.queue)
        .bind(ticket.asset_id)
        .bind(&schema)
        .bind(ticket.dedupe_key)
        .bind(ticket.trace_id)
        .bind(ticket.created_by)
        .execute(pool)
        .await
        .map_err(|e| TaskError::Transient(format!("写入任务票据失败: {e}")))?;
    Ok(())
}

/// 标记开始执行（`attempts + 1`，首次写入 `started_at`）
pub async fn mark_running(
    pool: &PgPool,
    schema: &str,
    task_id: &str,
    stage: &str,
    progress: i16,
) -> Result<(), TaskError> {
    let schema = sanitize_schema(schema)?;
    let sql = format!(
        "UPDATE {schema}.task_ticket SET \
         status = '{STATUS_RUNNING}', stage = $2, progress = $3, attempts = attempts + 1, \
         started_at = COALESCE(started_at, NOW()), updated_at = NOW() \
         WHERE id = $1"
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(task_id)
        .bind(stage)
        .bind(progress.clamp(0, 100))
        .execute(pool)
        .await
        .map_err(|e| TaskError::Transient(format!("更新任务阶段失败: {e}")))?;
    Ok(())
}

/// 推进阶段（不增加 `attempts`）
pub async fn mark_stage(
    pool: &PgPool,
    schema: &str,
    task_id: &str,
    stage: &str,
    progress: i16,
) -> Result<(), TaskError> {
    let schema = sanitize_schema(schema)?;
    let sql = format!(
        "UPDATE {schema}.task_ticket SET stage = $2, progress = $3, updated_at = NOW() WHERE id = $1"
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(task_id)
        .bind(stage)
        .bind(progress.clamp(0, 100))
        .execute(pool)
        .await
        .map_err(|e| TaskError::Transient(format!("更新任务进度失败: {e}")))?;
    Ok(())
}

/// 标记完成（进度 100、阶段 done）
pub async fn mark_done(pool: &PgPool, schema: &str, task_id: &str) -> Result<(), TaskError> {
    let schema = sanitize_schema(schema)?;
    let sql = format!(
        "UPDATE {schema}.task_ticket SET \
         status = '{STATUS_COMPLETED}', stage = 'done', progress = 100, error = NULL, \
         finished_at = NOW(), updated_at = NOW() \
         WHERE id = $1"
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| TaskError::Transient(format!("标记任务完成失败: {e}")))?;
    Ok(())
}

/// 标记失败（错误摘要截断到 4KB）
pub async fn mark_failed(
    pool: &PgPool,
    schema: &str,
    task_id: &str,
    error: &str,
) -> Result<(), TaskError> {
    let schema = sanitize_schema(schema)?;
    let summary: String = error.chars().take(MAX_ERROR_CHARS).collect();
    let sql = format!(
        "UPDATE {schema}.task_ticket SET \
         status = '{STATUS_FAILED}', error = $2, finished_at = NOW(), updated_at = NOW() \
         WHERE id = $1"
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(task_id)
        .bind(summary)
        .execute(pool)
        .await
        .map_err(|e| TaskError::Transient(format!("标记任务失败状态失败: {e}")))?;
    Ok(())
}

/// 查票据（不存在 → `None`，由 HTTP 层映射 404）
pub async fn get(
    pool: &PgPool,
    schema: &str,
    task_id: &str,
) -> Result<Option<TaskTicket>, TaskError> {
    let schema = sanitize_schema(schema)?;
    let sql = format!(
        "SELECT id, task_type, queue, asset_id, tenant_schema, status, stage, progress, attempts, \
         error, trace_id, dedupe_key, created_by, created_at, started_at, finished_at, updated_at \
         FROM {schema}.task_ticket WHERE id = $1"
    );
    sqlx::query_as::<_, TaskTicket>(sqlx::AssertSqlSafe(sql))
        .bind(task_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| TaskError::Transient(format!("查询任务票据失败: {e}")))
}

/// 列出票据（按创建时间倒序，供「任务中心」页）
///
/// - `status`：只接受 [`STATUS_PENDING`] / [`STATUS_RUNNING`] / [`STATUS_COMPLETED`] /
///   [`STATUS_FAILED`]，其它一律 `Invalid`（白名单，避免把任意文本带进比较）；
/// - `limit`：夹到 `1..=200`（默认值由调用方给，HTTP 层固定 50）。
pub async fn list(
    pool: &PgPool,
    schema: &str,
    status: Option<&str>,
    limit: i64,
) -> Result<Vec<TaskTicket>, TaskError> {
    let schema = sanitize_schema(schema)?;
    let status = match status.map(str::trim).filter(|s| !s.is_empty()) {
        None => None,
        Some(raw) => Some(known_status(raw)?),
    };
    let limit = clamp_limit(limit);
    let sql = format!(
        "SELECT id, task_type, queue, asset_id, tenant_schema, status, stage, progress, attempts, \
         error, trace_id, dedupe_key, created_by, created_at, started_at, finished_at, updated_at \
         FROM {schema}.task_ticket \
         WHERE ($1::text IS NULL OR status = $1) \
         ORDER BY created_at DESC LIMIT $2"
    );
    sqlx::query_as::<_, TaskTicket>(sqlx::AssertSqlSafe(sql))
        .bind(status)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(|e| TaskError::Transient(format!("查询任务列表失败: {e}")))
}

/// 状态白名单校验（`Status` 是后端契约，不接受前端自定义值）
fn known_status(status: &str) -> Result<&str, TaskError> {
    if [
        STATUS_PENDING,
        STATUS_RUNNING,
        STATUS_COMPLETED,
        STATUS_FAILED,
    ]
    .contains(&status)
    {
        Ok(status)
    } else {
        Err(TaskError::Invalid(format!("未知的任务状态: {status}")))
    }
}

/// 列表条数夹逼（1..=200）
fn clamp_limit(limit: i64) -> i64 {
    limit.clamp(1, 200)
}

/// 租户 schema 白名单校验（只允许 `[A-Za-z0-9_]`）
pub(crate) fn sanitize_schema(schema: &str) -> Result<String, TaskError> {
    let schema = schema.trim();
    if schema.is_empty()
        || !schema
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(TaskError::Invalid(format!("非法租户 schema: {schema}")));
    }
    Ok(schema.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_schema_accepts_tenant_names() {
        assert_eq!(sanitize_schema(" tenant_a ").expect("❌ 合法"), "tenant_a");
        assert_eq!(sanitize_schema("public").expect("❌ 合法"), "public");
    }

    #[test]
    fn test_sanitize_schema_rejects_injection() {
        for bad in [
            "",
            "   ",
            "tenant; DROP TABLE x",
            "tenant-a",
            "tenant\"b",
            "租户",
        ] {
            assert!(
                sanitize_schema(bad).is_err(),
                "❌ 应拒绝非法 schema: {bad:?}"
            );
        }
    }

    #[test]
    fn test_status_constants_match_sql() {
        assert_eq!(STATUS_PENDING, "pending");
        assert_eq!(STATUS_RUNNING, "running");
        assert_eq!(STATUS_COMPLETED, "completed");
        assert_eq!(STATUS_FAILED, "failed");
    }

    #[test]
    fn test_known_status_rejects_unknown() {
        assert!(known_status("pending").is_ok());
        assert!(known_status("completed").is_ok());
        assert!(known_status("done").is_err(), "❌ 非契约状态应拒绝");
        assert!(known_status("pending' OR 1=1").is_err());
    }

    #[test]
    fn test_clamp_limit() {
        assert_eq!(clamp_limit(0), 1);
        assert_eq!(clamp_limit(-5), 1);
        assert_eq!(clamp_limit(50), 50);
        assert_eq!(clamp_limit(999), 200);
    }
}
