//! 网关共享状态

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sqlx::PgPool;

use crate::config::GatewayConfig;
use crate::sse::SseHub;

/// 全进程共享状态：请求处理与后台任务都只拿这一个 `Arc`
pub struct GatewayState {
    /// 启动期读取一次的配置
    pub cfg: Arc<GatewayConfig>,
    /// 主库连接池（协调表 / 票据 / 事件都在主库，避免只读副本延迟）
    pub pool: PgPool,
    /// 本实例 SSE 连接表
    pub hub: Arc<SseHub>,
    /// `PgListener` 是否已订阅成功（`/readyz` 依据之一，附录 D.9）
    listener_ready: AtomicBool,
    /// 是否已进入优雅关闭（关闭期间的 `/readyz` 返回 503，让流量先撤走）
    draining: AtomicBool,
}

impl GatewayState {
    /// 创建状态
    pub fn new(cfg: Arc<GatewayConfig>, pool: PgPool) -> Self {
        let hub = Arc::new(SseHub::new(cfg.max_sse_per_user));
        Self {
            cfg,
            pool,
            hub,
            listener_ready: AtomicBool::new(false),
            draining: AtomicBool::new(false),
        }
    }

    /// 标记 `LISTEN` 通道状态
    pub fn set_listener_ready(&self, ready: bool) {
        self.listener_ready.store(ready, Ordering::Relaxed);
    }

    /// 是否已订阅通知通道
    pub fn is_listener_ready(&self) -> bool {
        self.listener_ready.load(Ordering::Relaxed)
    }

    /// 标记进入优雅关闭
    pub fn set_draining(&self, draining: bool) {
        self.draining.store(draining, Ordering::Relaxed);
    }

    /// 是否正在优雅关闭
    pub fn is_draining(&self) -> bool {
        self.draining.load(Ordering::Relaxed)
    }
}
