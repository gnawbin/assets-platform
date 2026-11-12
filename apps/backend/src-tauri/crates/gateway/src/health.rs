//! 健康检查（附录 D.9）：拆分存活与就绪，避免滚动重启时整批摘流量

use std::sync::Arc;
use std::time::Duration;

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};

use crate::state::GatewayState;

/// `readyz` 的数据库探测超时
const DB_PING_TIMEOUT: Duration = Duration::from_secs(2);

/// `GET /livez`：进程活着就 200，不查数据库
///
/// 修 B6：数据库抖动时不要让容器编排把全部实例一起重启。
pub async fn livez() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "service": "assets-gateway",
        })),
    )
}

/// `GET /readyz`：数据库可达 + 通知通道已订阅 + 不在关闭中
pub async fn readyz(State(state): State<Arc<GatewayState>>) -> impl IntoResponse {
    let db_ok = matches!(
        tokio::time::timeout(
            DB_PING_TIMEOUT,
            sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&state.pool)
        )
        .await,
        Ok(Ok(1))
    );
    let listener_ok = state.is_listener_ready();
    let draining = state.is_draining();
    let ok = db_ok && listener_ok && !draining;

    let status = if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(serde_json::json!({
            "status": if ok { "ok" } else { "degraded" },
            "db": db_ok,
            "listener": listener_ok,
            "draining": draining,
            "worker_id": state.cfg.worker_id,
            "instance_id": state.cfg.instance_id,
            "sse_connections": state.hub.len(),
        })),
    )
}
