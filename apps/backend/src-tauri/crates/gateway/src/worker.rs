//! 后台任务与跨实例协调（附录 D.3 / D.8 / D.9）
//!
//! 三个常驻任务：
//! 1. 租约续约（每个实例）：续约失败即「worker id 被别人抢走」，主动退出；
//! 2. `PgListener`（每个实例）：收 `NOTIFY` 清本地缓存 / 分发 SSE 事件；
//! 3. 清理选主（每个实例，但只有一个拿到 advisory lock 的实例真正执行）。

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tracing::{info, warn};

// `close()`（主动断开 leader 独占连接）来自 `Connection` trait，必须显式引入
use sqlx::Connection as _;

use crate::coord;
use crate::state::GatewayState;

/// 清理选主的 advisory lock key（附录 D.8，与迁移锁 `MIGRATION_LOCK_KEY` 不同）
pub const CLEANUP_LOCK_KEY: i64 = 0x4757_434C_4541_4E55;

/// 选主 / 心跳轮询间隔
const LEADER_POLL: Duration = Duration::from_secs(10);

/// `PgListener` 重连退避上限（附录 D.9：重连 ≤ 30s，避免打爆数据库）
const LISTENER_MAX_BACKOFF: Duration = Duration::from_secs(30);

/// 清理间隔下限，防止配置成 0 导致空转
const MIN_CLEANUP_INTERVAL: Duration = Duration::from_secs(5);

/// 租约续约循环
pub fn spawn_lease_renewer(state: Arc<GatewayState>, shutdown: watch::Sender<bool>) {
    let interval = state.cfg.lease_renew_interval();
    let cfg = state.cfg.clone();
    let pool = state.pool.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            match coord::claim_worker(
                &pool,
                cfg.worker_id,
                &cfg.instance_id,
                cfg.worker_lease_secs,
            )
            .await
            {
                Ok(true) => tracing::debug!(
                    "worker 租约已续约: worker_id={} instance_id={}",
                    cfg.worker_id,
                    cfg.instance_id
                ),
                Ok(false) => {
                    tracing::error!(
                        "worker id 已被其它存活实例占用（worker_id={} instance_id={}），\
                         主动退出以避免重复雪花 id",
                        cfg.worker_id,
                        cfg.instance_id
                    );
                    let _ = shutdown.send(true);
                    return;
                }
                Err(e) => warn!("worker 租约续约失败（下个周期重试）: {}", e),
            }
        }
    });
}

/// `PgListener` 常驻循环：断线指数退避重连
pub fn spawn_listener(state: Arc<GatewayState>, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);
        while !*shutdown.borrow() {
            match run_listener(&state, &mut shutdown).await {
                Ok(()) => return,
                Err(e) => {
                    state.set_listener_ready(false);
                    warn!(
                        "PostgreSQL 通知通道断开，{}s 后重连（期间 SSE 仍可回放，仅实时推送延迟）: {}",
                        backoff.as_secs(),
                        e
                    );
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(backoff) => {}
                _ = shutdown.changed() => {}
            }
            backoff = (backoff * 2).min(LISTENER_MAX_BACKOFF);
        }
    });
}

/// 单次订阅生命周期：订阅成功 → 逐条分发，直到连接出错或收到关闭信号
async fn run_listener(
    state: &Arc<GatewayState>,
    shutdown: &mut watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let mut listener = sqlx::postgres::PgListener::connect_with(&state.pool).await?;
    listener.listen(&state.cfg.pg_channel).await?;
    state.set_listener_ready(true);
    info!(
        "已订阅 PostgreSQL 通知通道: {}（instance_id={}）",
        state.cfg.pg_channel, state.cfg.instance_id
    );

    loop {
        tokio::select! {
            notification = listener.recv() => {
                dispatch(state, notification?.payload()).await;
            }
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    state.set_listener_ready(false);
                    return Ok(());
                }
            }
        }
    }
}

/// 分发一条通知
///
/// 约定载荷（都带 `k` 字段区分类型）：
/// - `{"k":"authz","u":<user_id>}`：单用户权限失效
/// - `{"k":"authz_all"}`：全量权限失效
/// - `{"k":"apikey","id":<api_key_id>}`：API Key 失效
/// - `{"k":"sse","t":<tenant_id>,"id":<event_id>}`：有待推送的 SSE 事件
async fn dispatch(state: &Arc<GatewayState>, payload: &str) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        warn!("忽略无法解析的通知载荷: {}", payload);
        return;
    };
    match value.get("k").and_then(|v| v.as_str()) {
        Some("authz") => {
            if let Some(user_id) = value.get("u").and_then(|v| v.as_i64()) {
                api::auth::clear_local_user_authority(user_id);
            }
        }
        Some("authz_all") => api::auth::clear_local_authority_cache(),
        Some("apikey") => {
            if let Some(id) = value.get("id").and_then(|v| v.as_i64()) {
                service::api_key_service::clear_local_api_key(id);
            }
        }
        Some("sse") => {
            let Some(id) = value
                .get("id")
                .and_then(|v| v.as_i64())
                .filter(|id| *id > 0)
            else {
                return;
            };
            match coord::fetch_event(&state.pool, id).await {
                Ok(Some(event)) => state.hub.broadcast(event),
                Ok(None) => warn!("通知指向的 SSE 事件已过期: id={}", id),
                Err(e) => warn!("读取 SSE 事件失败: id={} error={}", id, e),
            }
        }
        _ => {}
    }
}

/// 清理选主循环（附录 D.8）
///
/// 拿到 advisory lock 的实例才执行清理；连接断开时 PostgreSQL 自动释放锁，
/// 本任务下一轮会重新抢锁，因此不需要额外的故障转移逻辑。
pub fn spawn_leader(state: Arc<GatewayState>, mut shutdown: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let interval =
            Duration::from_secs(state.cfg.cleanup_interval_secs).max(MIN_CLEANUP_INTERVAL);
        let mut leader: Option<sqlx::PgConnection> = None;
        let mut last_cleanup: Option<std::time::Instant> = None;

        loop {
            if *shutdown.borrow() {
                break;
            }

            if leader.is_none() {
                match coord::try_become_leader(&state.pool, CLEANUP_LOCK_KEY).await {
                    Ok(Some(conn)) => {
                        info!("本实例成为清理 leader（其它实例保持 standby）");
                        leader = Some(conn);
                        last_cleanup = None;
                    }
                    Ok(None) => {}
                    Err(e) => warn!("抢清理 leader 锁失败: {}", e),
                }
            }

            if let Some(conn) = leader.as_mut() {
                // 保活探测：连接断则锁已释放，回到 standby
                if sqlx::query("SELECT 1").execute(&mut *conn).await.is_err() {
                    warn!("leader 连接已断开（锁由 PostgreSQL 自动释放），回到 standby");
                    leader = None;
                } else {
                    let due = last_cleanup.is_none_or(|at| at.elapsed() >= interval);
                    if due {
                        match coord::cleanup(&state.pool, state.cfg.event_retention_secs).await {
                            Ok(removed) => info!("leader 清理完成: {} 行", removed),
                            Err(e) => warn!("leader 清理协调表失败: {}", e),
                        }
                        api::cleanup_once().await;
                        last_cleanup = Some(std::time::Instant::now());
                    }
                }
            }

            tokio::select! {
                _ = tokio::time::sleep(LEADER_POLL) => {}
                _ = shutdown.changed() => {}
            }
        }

        if let Some(mut conn) = leader.take() {
            let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
                .bind(CLEANUP_LOCK_KEY)
                .execute(&mut conn)
                .await;
            let _ = conn.close().await;
            info!("已释放清理 leader 锁");
        }
    });
}

/// 失效广播实现：把「本实例失效」用 `NOTIFY` 推给所有实例（附录 D.3）
pub struct PgInvalidator {
    channel: String,
}

impl PgInvalidator {
    /// 绑定通知通道
    pub fn new(channel: String) -> Self {
        Self { channel }
    }

    /// 异步广播：调用方（业务写路径）不等待、不受数据库抖动影响
    fn broadcast(&self, payload: String) {
        let channel = self.channel.clone();
        tokio::spawn(async move {
            let Ok(pool) = database::get_pool() else {
                return;
            };
            if let Err(e) = coord::notify(&pool, &channel, &payload).await {
                warn!("广播失效通知失败（L1 缓存 TTL 兜底）: {}", e);
            }
        });
    }

    /// 广播 API Key 失效
    pub fn notify_api_key(&self, id: i64) {
        self.broadcast(format!("{{\"k\":\"apikey\",\"id\":{}}}", id));
    }
}

impl api::auth::AuthInvalidator for PgInvalidator {
    fn on_invalidate_user(&self, user_id: i64) {
        self.broadcast(format!("{{\"k\":\"authz\",\"u\":{}}}", user_id));
    }

    fn on_clear_all(&self) {
        self.broadcast(String::from("{\"k\":\"authz_all\"}"));
    }
}

/// 安装 API Key 失效广播（附录 D.3：消除 B8 的 30s 跨实例窗口）
pub fn install_api_key_notifier(channel: &str) {
    let notifier = Arc::new(PgInvalidator::new(channel.to_string()));
    let hook = notifier.clone();
    service::api_key_service::install_api_key_invalidator(Arc::new(move |id: i64| {
        hook.notify_api_key(id);
    }));
}
