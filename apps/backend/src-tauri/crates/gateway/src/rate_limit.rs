//! 分布式限流（附录 D.4：PostgreSQL 固定窗口计数，跨实例总额准确）
//!
//! 默认只限制「带凭证」的流量（`Authorization` 存在）：这类调用方是 API Key /
//! OAuth 令牌，最需要防止单 key 打满全集群；匿名请求交给上游网关 / WAF。
//! `GATEWAY_RATE_LIMIT_ENFORCE_GLOBAL=1` 时对所有请求生效（按对端 IP 计数）。
//!
//! 计数在 PostgreSQL 中用单语句 UPSERT 完成，再多实例也只有一个全局总额。
//! `GATEWAY_RATE_LIMIT_FAIL_OPEN=1`（默认）时，计数失败直接放行——限流是保护
//! 手段，不应成为单点故障（修 B8 里 fail-closed 的老问题）。

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use axum::{
    extract::{ConnectInfo, Request, State},
    http::header::AUTHORIZATION,
    middleware::Next,
    response::Response,
};

use api::response::ApiError;

use crate::coord;
use crate::state::GatewayState;

/// 固定窗口长度（秒）
const WINDOW_SECS: i32 = 60;

/// 每个桶在窗口内的请求上限
const LIMIT_PER_WINDOW: i32 = 600;

/// 限流中间件
pub async fn limit(
    State(state): State<Arc<GatewayState>>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    if !state.cfg.rate_limit_enforce_global && !req.headers().contains_key(AUTHORIZATION) {
        return Ok(next.run(req).await);
    }

    let bucket_key = bucket_key(&req);
    match coord::rate_limit_hit(&state.pool, &bucket_key, WINDOW_SECS).await {
        Ok(counter) if counter > LIMIT_PER_WINDOW => Err(ApiError::too_many_requests(format!(
            "请求过于频繁（{} 次/{}s 上限），请稍后重试",
            LIMIT_PER_WINDOW, WINDOW_SECS
        ))),
        Ok(_) => Ok(next.run(req).await),
        Err(e) => {
            if state.cfg.rate_limit_fail_open {
                tracing::warn!("限流计数失败，按 fail-open 放行: {}", e);
                Ok(next.run(req).await)
            } else {
                Err(ApiError::service_unavailable("限流服务暂时不可用"))
            }
        }
    }
}

/// 构造计数桶键
///
/// - 有凭证：`tok:<凭证哈希>`——按令牌限流，等价于按「调用方身份」限流，
///   且不会把原始密钥写进数据库；
/// - 无凭证：`ip:<对端地址>`——仅在 `GATEWAY_RATE_LIMIT_ENFORCE_GLOBAL=1` 时走到。
fn bucket_key(req: &Request) -> String {
    if let Some(token) = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        token.hash(&mut hasher);
        return format!("tok:{:016x}", hasher.finish());
    }

    match req.extensions().get::<ConnectInfo<std::net::SocketAddr>>() {
        Some(ConnectInfo(addr)) => format!("ip:{}", addr.ip()),
        None => String::from("ip:unknown"),
    }
}
