//! PostgreSQL 协调原语（附录 D.3 – D.8）
//!
//! 本 crate 不引入 Redis：租约、票据、事件日志、限流计数全部落在 PostgreSQL，
//! 跨实例唤醒用 `LISTEN/NOTIFY`，选主用 advisory lock。
//! JSONB 列一律以 `::text` 往返，避免额外打开 sqlx 的 `json` feature。

use sqlx::PgPool;
// `close()`（主动关闭独占连接）来自 `Connection` trait，必须显式引入
use sqlx::Connection as _;
use tracing::warn;

use api::auth::AuthPrincipal;

/// SSE 事件行
#[derive(Debug, Clone)]
pub struct GatewayEvent {
    /// 事件 id（= `sys_gateway_event.id`，同时也是 SSE `id:` 字段）
    pub id: i64,
    /// 事件归属租户（跨实例分发按租户过滤）
    pub tenant_id: i64,
    /// 事件类型（SSE `event:` 字段）
    pub kind: String,
    /// 事件正文
    pub payload: serde_json::Value,
}

/// 一次性票据消费结果
#[derive(Debug, Clone)]
pub struct TicketGrant {
    /// 签发时记录的租户
    pub tenant_id: Option<i64>,
    /// 签发时记录的主体快照
    pub principal: AuthPrincipal,
}

/// 抢位 / 续约 worker id 租约
///
/// 返回 `false` 表示该 worker id 仍被其它存活实例持有：调用方必须终止启动，
/// 否则两个进程会生成重复雪花 id。
pub async fn claim_worker(
    pool: &PgPool,
    worker_id: i16,
    instance_id: &str,
    lease_secs: i64,
) -> anyhow::Result<bool> {
    let row: Option<String> = sqlx::query_scalar(
        "INSERT INTO public.sys_gateway_instance (worker_id, instance_id, expires_at)\n\
         VALUES ($1, $2, now() + make_interval(secs => $3))\n\
         ON CONFLICT (worker_id) DO UPDATE\n\
            SET instance_id = EXCLUDED.instance_id,\n\
                expires_at  = EXCLUDED.expires_at,\n\
                updated_at  = now()\n\
          WHERE public.sys_gateway_instance.instance_id = EXCLUDED.instance_id\n\
             OR public.sys_gateway_instance.expires_at < now()\n\
         RETURNING instance_id",
    )
    .bind(worker_id)
    .bind(instance_id)
    .bind(lease_secs as f64)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

/// 释放 worker 租约（优雅退出，附录 D.8 第 7 步）
pub async fn release_worker(
    pool: &PgPool,
    worker_id: i16,
    instance_id: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "DELETE FROM public.sys_gateway_instance WHERE worker_id = $1 AND instance_id = $2",
    )
    .bind(worker_id)
    .bind(instance_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// 发布 SSE 事件：写事件日志 + `NOTIFY` 唤醒所有实例
///
/// 两步放进同一事务：`NOTIFY` 在 commit 时投递，因此订阅方回表一定能读到事件，
/// 不会出现「通知先到、行还没提交」的竞态。
pub async fn publish_event(
    pool: &PgPool,
    channel: &str,
    tenant_id: i64,
    kind: &str,
    payload: &serde_json::Value,
) -> anyhow::Result<i64> {
    let payload_text = serde_json::to_string(payload)?;
    let mut tx = pool.begin().await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO public.sys_gateway_event (tenant_id, kind, payload)\n\
         VALUES ($1, $2, $3::jsonb) RETURNING id",
    )
    .bind(tenant_id)
    .bind(kind)
    .bind(&payload_text)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("SELECT pg_notify($1, $2)")
        .bind(channel)
        .bind(format!(
            "{{\"k\":\"sse\",\"t\":{},\"id\":{}}}",
            tenant_id, id
        ))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(id)
}

/// 广播认证失效通知（附录 D.3）
///
/// 失败只告警：L1 缓存的 60s TTL 是兜底路径，不会产生永久脏读。
pub async fn notify(pool: &PgPool, channel: &str, payload: &str) -> anyhow::Result<()> {
    sqlx::query("SELECT pg_notify($1, $2)")
        .bind(channel)
        .bind(payload)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| {
            warn!("广播 PostgreSQL 通知失败: {}", e);
            e.into()
        })
}

/// 读取单条事件正文（`NOTIFY` 只带 id，正文回表取，避免 8000 字节载荷上限）
pub async fn fetch_event(pool: &PgPool, id: i64) -> anyhow::Result<Option<GatewayEvent>> {
    let row: Option<(i64, i64, String, String)> = sqlx::query_as(
        "SELECT id, tenant_id, kind, payload::text FROM public.sys_gateway_event WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id, tenant_id, kind, payload)| GatewayEvent {
        id,
        tenant_id,
        kind,
        payload: serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null),
    }))
}

/// 断线回放：`Last-Event-ID` 之后、同租户、按 id 升序、最多 `limit` 条
pub async fn replay_events(
    pool: &PgPool,
    tenant_id: i64,
    after_id: i64,
    limit: i64,
) -> anyhow::Result<Vec<GatewayEvent>> {
    let rows: Vec<(i64, i64, String, String)> = sqlx::query_as(
        "SELECT id, tenant_id, kind, payload::text\n\
           FROM public.sys_gateway_event\n\
          WHERE tenant_id = $1 AND id > $2\n\
          ORDER BY id\n\
          LIMIT $3",
    )
    .bind(tenant_id)
    .bind(after_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, tenant_id, kind, payload)| GatewayEvent {
            id,
            tenant_id,
            kind,
            payload: serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null),
        })
        .collect())
}

/// 签发一次性 SSE 票据（附录 D.5 ①）
pub async fn issue_ticket(
    pool: &PgPool,
    token: &str,
    tenant_id: Option<i64>,
    principal: &AuthPrincipal,
    ttl_secs: i64,
) -> anyhow::Result<()> {
    let principal = serde_json::to_string(principal)?;
    sqlx::query(
        "INSERT INTO public.sys_gateway_sse_ticket (token, tenant_id, principal, expires_at)\n\
         VALUES ($1, $2, $3::jsonb, now() + make_interval(secs => $4))",
    )
    .bind(token)
    .bind(tenant_id)
    .bind(&principal)
    .bind(ttl_secs as f64)
    .execute(pool)
    .await?;
    Ok(())
}

/// 消费一次性票据：`DELETE ... RETURNING` 保证「原子 + 只能用一次 + 跨实例」
pub async fn consume_ticket(pool: &PgPool, token: &str) -> anyhow::Result<Option<TicketGrant>> {
    let row: Option<(Option<i64>, String)> = sqlx::query_as(
        "DELETE FROM public.sys_gateway_sse_ticket\n\
          WHERE token = $1 AND expires_at > now()\n\
          RETURNING tenant_id, principal::text",
    )
    .bind(token)
    .fetch_optional(pool)
    .await?;
    match row {
        Some((tenant_id, principal)) => {
            let principal: AuthPrincipal = serde_json::from_str(&principal)
                .map_err(|e| anyhow::anyhow!("票据主体反序列化失败: {}", e))?;
            Ok(Some(TicketGrant {
                tenant_id,
                principal,
            }))
        }
        None => Ok(None),
    }
}

/// 固定窗口共享计数：返回窗口内第几次命中（附录 D.4）
///
/// 窗口起点由数据库时钟计算，多实例天然一致，不需要额外对时。
pub async fn rate_limit_hit(
    pool: &PgPool,
    bucket_key: &str,
    window_secs: i32,
) -> anyhow::Result<i32> {
    let counter: i32 = sqlx::query_scalar(
        "INSERT INTO public.sys_gateway_rate_limit AS rl (bucket_key, window_start, counter)\n\
         VALUES ($1, to_timestamp(floor(extract(epoch FROM now()) / $2::int) * $2::int), 1)\n\
         ON CONFLICT (bucket_key, window_start) DO UPDATE SET counter = rl.counter + 1\n\
         RETURNING counter",
    )
    .bind(bucket_key)
    .bind(window_secs)
    .fetch_one(pool)
    .await?;
    Ok(counter)
}

/// leader 独占清理（附录 D.8）：返回四张协调表合计删除行数
pub async fn cleanup(pool: &PgPool, event_retention_secs: i64) -> anyhow::Result<i64> {
    let events = sqlx::query(
        "DELETE FROM public.sys_gateway_event WHERE created_at < now() - make_interval(secs => $1)",
    )
    .bind(event_retention_secs as f64)
    .execute(pool)
    .await?
    .rows_affected();
    let tickets = sqlx::query("DELETE FROM public.sys_gateway_sse_ticket WHERE expires_at < now()")
        .execute(pool)
        .await?
        .rows_affected();
    let windows = sqlx::query(
        "DELETE FROM public.sys_gateway_rate_limit WHERE window_start < now() - interval '10 minutes'",
    )
    .execute(pool)
    .await?
    .rows_affected();
    let leases = sqlx::query(
        "DELETE FROM public.sys_gateway_instance WHERE expires_at < now() - interval '5 minutes'",
    )
    .execute(pool)
    .await?
    .rows_affected();
    Ok((events + tickets + windows + leases) as i64)
}

/// 抢 leader 锁：成功返回持有锁的独占连接（连接断开 = 锁自动释放）
pub async fn try_become_leader(
    pool: &PgPool,
    key: i64,
) -> anyhow::Result<Option<sqlx::PgConnection>> {
    let mut conn = pool.acquire().await?.detach();
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(key)
        .fetch_one(&mut conn)
        .await?;
    if locked {
        Ok(Some(conn))
    } else {
        let _ = conn.close().await;
        Ok(None)
    }
}
