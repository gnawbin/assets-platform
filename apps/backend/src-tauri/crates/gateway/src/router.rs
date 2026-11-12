//! 应用装配：`api::build_router` + 网关自有路由 + 中间件

use std::sync::Arc;

use axum::{
    Router, middleware,
    routing::{get, post},
};
use service::llm_gateway_service::LLMRouter;
use tower_http::{compression::CompressionLayer, trace::TraceLayer};

use crate::health;
use crate::sse;
use crate::state::GatewayState;

/// 组装网关应用
///
/// 复用 `api::build_router`（业务路由 + 上传 + 对话 SSE 全部保留），在其之上叠加：
/// - `/livez`、`/readyz`（无认证，供负载均衡 / k8s 探针）
/// - `/api/auth/sse-ticket`（普通认证）与 `/api/events`（票据认证）
/// - 可选的联调端点
pub async fn build_app(
    state: Arc<GatewayState>,
    llm_router: Arc<LLMRouter>,
) -> anyhow::Result<Router> {
    let api_router = api::build_router(state.pool.clone(), Some(llm_router), None).await?;

    // 取票走普通认证（Bearer）：与 /api/* 的主体解析保持一致
    let ticket_router = Router::new()
        .route("/api/auth/sse-ticket", post(sse::issue_ticket))
        .layer(middleware::from_fn(api::auth::auth_middleware))
        .with_state(state.clone());

    // 建流走票据认证：票据本身就是凭证，因此不挂 auth_middleware
    let events_router = Router::new()
        .route("/api/events", get(sse::events))
        .with_state(state.clone());

    let probe_router = Router::new()
        .route("/livez", get(health::livez))
        .route("/readyz", get(health::readyz))
        .with_state(state.clone());

    let mut app = api_router
        .merge(ticket_router)
        .merge(events_router)
        // 分布式限流（附录 D.4）：只挂业务路由，探针不受影响
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::rate_limit::limit,
        ))
        .merge(probe_router);

    if state.cfg.dev_endpoints {
        let dev_router = Router::new()
            .route("/api/gateway/events", post(sse::publish))
            .layer(middleware::from_fn(api::auth::auth_middleware))
            .with_state(state.clone());
        app = app.merge(dev_router);
        tracing::warn!(
            "已开启 GATEWAY_DEV_ENDPOINTS：POST /api/gateway/events 可写事件，仅限联调环境"
        );
    }

    app = app.layer(TraceLayer::new_for_http());

    if state.cfg.gzip {
        // `CompressionLayer` 的默认谓词已排除 `text/event-stream`，不会破坏 SSE 帧
        app = app.layer(CompressionLayer::new());
        tracing::info!("已开启 GATEWAY_GZIP=1：普通响应压缩（SSE 自动排除）");
    }

    Ok(app)
}
