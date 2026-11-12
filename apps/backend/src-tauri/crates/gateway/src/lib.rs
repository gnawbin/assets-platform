//! 业务网关（多实例，PostgreSQL 单一依赖模式，设计文档附录 D）
//!
//! 为什么要有它：桌面形态（Tauri 内嵌 `api` + 单个 `server`）无法水平扩展，
//! 而「多实例」需要解决四件事，本 crate 全部用 PostgreSQL 实现，不引入 Redis：
//! 1. 雪花 worker id 租约（`sys_gateway_instance` + 续约）；
//! 2. 缓存失效广播（`LISTEN/NOTIFY`）；
//! 3. SSE 跨实例分发与断线续传（`sys_gateway_event` + 一次性票据）；
//! 4. 分布式限流与 leader 独占清理（`sys_gateway_rate_limit` + advisory lock）。
//!
//! 设计取舍见附录 D.12：不做代理（v1 嵌在 `api` 路由器内），不做 gRPC。

pub mod config;
pub mod coord;
pub mod health;
pub mod rate_limit;
pub mod router;
pub mod sse;
pub mod state;
pub mod worker;

use std::sync::Arc;
use std::time::Duration;

use service::llm_gateway_service::LLMRouter;
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

pub use config::GatewayConfig;
pub use state::GatewayState;

/// 优雅关闭时给客户端建议的重连延迟（毫秒）
const SHUTDOWN_RETRY_AFTER_MS: u64 = 1000;

/// 优雅关闭默认宽限（秒）：超时后强制断开剩余连接（大多是长连 SSE）
const DEFAULT_DRAIN_SECS: u64 = 15;

/// 网关主流程：抢租约 → 注入广播 → 起后台任务 → 装配路由 → 监听 → 收尾
pub async fn serve(
    cfg: GatewayConfig,
    pool: sqlx::PgPool,
    llm_router: Arc<LLMRouter>,
) -> anyhow::Result<()> {
    // 1. 协调表（幂等）：覆盖「老库升级」与「只跑网关不跑 server」两种场景
    database::postgres::migrate_gateway_tables(&pool).await?;

    // 2. worker id 租约：抢不到直接失败退出，绝不带病启动（宁可少一个实例）
    if !coord::claim_worker(
        &pool,
        cfg.worker_id,
        &cfg.instance_id,
        cfg.worker_lease_secs,
    )
    .await?
    {
        anyhow::bail!(
            "worker id 已被其它存活实例占用：worker_id={} instance_id={}（换 SNOWFLAKE_WORKER_ID，或等待租约过期）",
            cfg.worker_id,
            cfg.instance_id
        );
    }
    utils::snowflake::init_with(cfg.worker_id as u64);
    info!(
        "worker 租约获取成功: worker_id={} instance_id={} lease={}s",
        cfg.worker_id, cfg.instance_id, cfg.worker_lease_secs
    );

    // 3. 失效广播注入：此后本实例的写操作会把失效推给其它实例（附录 D.3）
    api::auth::install_auth_invalidator(Arc::new(worker::PgInvalidator::new(
        cfg.pg_channel.clone(),
    )));
    worker::install_api_key_notifier(&cfg.pg_channel);

    // 4. 共享状态 + 三个常驻后台任务
    let drain_secs = std::env::var("GATEWAY_DRAIN_SECS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_DRAIN_SECS);
    let state = Arc::new(GatewayState::new(Arc::new(cfg.clone()), pool.clone()));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    worker::spawn_lease_renewer(state.clone(), shutdown_tx.clone());
    worker::spawn_listener(state.clone(), shutdown_rx.clone());
    worker::spawn_leader(state.clone(), shutdown_rx.clone());

    // 5. 装配路由并监听
    let app = router::build_app(state.clone(), llm_router).await?;
    let addr: std::net::SocketAddr = format!("{}:{}", cfg.bind, cfg.port)
        .parse()
        .map_err(|e| anyhow::anyhow!("网关监听地址非法（{}:{}）: {}", cfg.bind, cfg.port, e))?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("绑定 {} 失败: {}", addr, e))?;
    info!(
        "assets-gateway 监听 http://{}（instance_id={} worker_id={}）",
        addr, cfg.instance_id, cfg.worker_id
    );

    // 6. 关闭协调：信号 → 标记 draining + 通知 SSE 客户端；超时强制断开
    let (drain_tx, mut drain_rx) = mpsc::channel::<()>(1);
    let mut graceful_rx = shutdown_rx.clone();
    let shutdown_signal = async move {
        tokio::select! {
            _ = wait_signal() => info!("收到退出信号"),
            _ = graceful_rx.changed() => info!("内部请求关闭（租约丢失或任务异常）"),
        }
        let _ = drain_tx.send(()).await;
    };

    let drain_state = state.clone();
    let drain = async move {
        let _ = drain_rx.recv().await;
        drain_state.set_draining(true);
        warn!(
            "优雅关闭：通知 {} 个 SSE 连接重连，最多等待 {}s",
            drain_state.hub.len(),
            drain_secs
        );
        drain_state.hub.broadcast_shutdown(SHUTDOWN_RETRY_AFTER_MS);
        tokio::time::sleep(Duration::from_secs(drain_secs)).await;
    };

    let serve = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal);

    tokio::select! {
        result = serve => {
            result.map_err(|e| anyhow::anyhow!("网关服务运行出错: {}", e))?;
        }
        _ = drain => {
            warn!("优雅关闭超过 {}s，强制断开剩余连接", drain_secs);
        }
    }

    // 7. 收尾：停后台任务 + 释放租约（附录 D.8 第 7 步）
    let _ = shutdown_tx.send(true);
    if let Err(e) = coord::release_worker(&pool, cfg.worker_id, &cfg.instance_id).await {
        warn!(
            "释放 worker 租约失败（{}s 后自动过期）: {}",
            cfg.worker_lease_secs, e
        );
    }
    info!("assets-gateway 已退出（worker_id={}）", cfg.worker_id);
    Ok(())
}

/// 等待 SIGTERM / SIGINT
async fn wait_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
