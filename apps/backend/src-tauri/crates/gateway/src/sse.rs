//! SSE：票据签发 + 建流 + 跨实例分发（附录 D.5 – D.7）
//!
//! 流程（`EventSource` 无法带自定义 Header，因此必须「Bearer 换一次性票据」）：
//! 1. `POST /api/auth/sse-ticket`（普通认证）→ 拿票据；
//! 2. `GET /api/events?ticket=...` → 消费票据建流；
//! 3. 断线后用浏览器自动带上的 `Last-Event-ID` 回放增量。

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use axum::{
    Extension, Json,
    extract::{Query, State},
    http::HeaderMap,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use dashmap::DashMap;
use futures::{Stream, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{info, warn};

use api::auth::{AuthPrincipal, PrincipalKind};
use api::response::ApiError;

use crate::coord::{self, GatewayEvent};
use crate::state::GatewayState;

/// 单连接事件缓冲：满了丢最旧（避免慢客户端拖垮进程内存）
const CONN_BUFFER: usize = 256;

/// 一帧待发送数据
#[derive(Debug, Clone)]
pub struct SseMessage {
    /// SSE `id:` 字段（`hello` 帧没有 id）
    pub id: Option<i64>,
    /// SSE `event:` 字段
    pub event: String,
    /// SSE `data:` 字段（JSON 文本）
    pub data: String,
}

impl SseMessage {
    fn into_event(self) -> Event {
        let mut event = Event::default();
        if let Some(id) = self.id {
            event = event.id(id.to_string());
        }
        event.event(self.event).data(self.data)
    }
}

/// 本实例的一条 SSE 连接
struct Conn {
    /// 订阅者（用户 id，用于每用户连接数上限）
    user_id: i64,
    /// 订阅者租户（跨实例分发按租户过滤）
    tenant_id: i64,
    /// 事件出口
    tx: mpsc::Sender<SseMessage>,
    /// 水位：`id <= since` 的事件不再投递（回放已经覆盖）
    since: Arc<AtomicI64>,
}

/// 本实例 SSE 连接表
pub struct SseHub {
    conns: DashMap<u64, Conn>,
    next_conn_id: AtomicU64,
    max_per_user: usize,
}

impl SseHub {
    /// 创建连接表
    pub fn new(max_per_user: usize) -> Self {
        Self {
            conns: DashMap::new(),
            next_conn_id: AtomicU64::new(1),
            max_per_user: max_per_user.max(1),
        }
    }

    /// 当前连接数
    pub fn len(&self) -> usize {
        self.conns.len()
    }

    /// 是否没有活跃连接
    pub fn is_empty(&self) -> bool {
        self.conns.is_empty()
    }

    fn user_conn_count(&self, user_id: i64) -> usize {
        self.conns
            .iter()
            .filter(|entry| entry.user_id == user_id)
            .count()
    }

    /// 注册连接；超过每用户上限返回 `None`（调用方回 429）
    fn register(
        &self,
        user_id: i64,
        tenant_id: i64,
        since: i64,
    ) -> Option<(u64, mpsc::Receiver<SseMessage>, Arc<AtomicI64>)> {
        if self.user_conn_count(user_id) >= self.max_per_user {
            return None;
        }
        let (tx, rx) = mpsc::channel(CONN_BUFFER);
        let conn_id = self.next_conn_id.fetch_add(1, Ordering::Relaxed);
        let watermark = Arc::new(AtomicI64::new(since));
        self.conns.insert(
            conn_id,
            Conn {
                user_id,
                tenant_id,
                // 发送端只留在连接表里：连接断开 -> 记录移除 -> 通道关闭 -> 流结束
                tx,
                since: watermark.clone(),
            },
        );
        Some((conn_id, rx, watermark))
    }

    fn unregister(&self, conn_id: u64) {
        self.conns.remove(&conn_id);
    }

    /// 把一条事件投给本实例上同租户的连接
    pub fn broadcast(&self, event: GatewayEvent) {
        let data = serde_json::to_string(&event.payload).unwrap_or_else(|_| String::from("{}"));
        for mut entry in self.conns.iter_mut() {
            let conn = entry.value_mut();
            if conn.tenant_id != event.tenant_id {
                continue;
            }
            if conn.since.load(Ordering::Relaxed) >= event.id {
                continue;
            }
            let msg = SseMessage {
                id: Some(event.id),
                event: event.kind.clone(),
                data: data.clone(),
            };
            if conn.tx.try_send(msg).is_err() {
                warn!(
                    "SSE 连接缓冲已满，丢弃事件: conn={} event_id={}",
                    entry.key(),
                    event.id
                );
            }
        }
    }

    /// 优雅退出：给所有连接发 `shutdown` 帧并建议重连延迟（附录 D.8 第 2 步）
    pub fn broadcast_shutdown(&self, retry_after_ms: u64) {
        let data = format!(
            "{{\"reason\":\"restart\",\"retry_after_ms\":{}}}",
            retry_after_ms
        );
        for entry in self.conns.iter() {
            let _ = entry.tx.try_send(SseMessage {
                id: None,
                event: String::from("shutdown"),
                data: data.clone(),
            });
        }
    }
}

/// 连接注销守卫：连接断开（客户端关页 / 网络断）时自动清表
struct ConnGuard {
    hub: Arc<SseHub>,
    conn_id: u64,
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.hub.unregister(self.conn_id);
    }
}

/// `GET /api/events?ticket=...` 的查询参数
#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    /// 一次性票据（`EventSource` 无法带 Header，因此走查询串）
    pub ticket: Option<String>,
}

/// 签发一次性 SSE 票据（附录 D.5 ①）
///
/// 走普通认证（Bearer / API Key），无请求体：租户直接取主体上的绑定租户。
/// 票据 TTL 默认 30s 且只能用一次：即使被日志或 Referer 泄漏，窗口也极小。
pub async fn issue_ticket(
    State(state): State<Arc<GatewayState>>,
    Extension(principal): Extension<AuthPrincipal>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if principal.kind != PrincipalKind::User || principal.user_id.is_none() {
        return Err(ApiError::forbidden(
            "SSE 只服务用户态主体；API Key / OAuth 服务身份请使用轮询接口",
        ));
    }
    let tenant_id = principal.tenant_id.unwrap_or_default();
    let token = uuid::Uuid::new_v4().simple().to_string();
    let ttl = state.cfg.ticket_ttl_secs;
    coord::issue_ticket(&state.pool, &token, Some(tenant_id), &principal, ttl)
        .await
        .map_err(|e| ApiError::internal_error(format!("签发 SSE 票据失败: {}", e)))?;
    Ok(Json(serde_json::json!({
        "ticket": token,
        "expires_in": ttl,
    })))
}

/// `GET /api/events?ticket=...`：消费票据 → 回放 → 实时（附录 D.5 ②）
pub async fn events(
    State(state): State<Arc<GatewayState>>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if state.is_draining() {
        return Err(ApiError::service_unavailable(
            "网关正在重启，请稍后重连（客户端会自动重试）",
        ));
    }
    let Some(token) = query.ticket.filter(|token| !token.trim().is_empty()) else {
        return Err(ApiError::unauthorized(
            "缺少 SSE 票据：请先调用 POST /api/auth/sse-ticket",
        ));
    };
    let grant = coord::consume_ticket(&state.pool, token.trim())
        .await
        .map_err(|e| ApiError::internal_error(format!("校验 SSE 票据失败: {}", e)))?
        .ok_or_else(|| ApiError::unauthorized("SSE 票据无效、已使用或已过期"))?;

    let Some(user_id) = grant.principal.user_id else {
        return Err(ApiError::forbidden("SSE 只服务用户态主体"));
    };
    let tenant_id = grant
        .tenant_id
        .or(grant.principal.tenant_id)
        .unwrap_or_default();

    // 浏览器重连时自动带 `Last-Event-ID`；非法值按「从最新开始」处理
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|id| *id > 0)
        .unwrap_or_default();

    let Some((conn_id, rx, watermark)) = state.hub.register(user_id, tenant_id, last_event_id)
    else {
        return Err(ApiError::too_many_requests(
            "SSE 连接数超过上限，请关闭多余标签页后重试",
        ));
    };

    // 先注册再回放：回放期间产生的新事件会落进通道（并按水位去重），不会丢
    let replay = coord::replay_events(
        &state.pool,
        tenant_id,
        last_event_id,
        state.cfg.replay_max_events,
    )
    .await
    .unwrap_or_else(|e| {
        warn!("回放 SSE 事件失败（先以降级模式建流）: {}", e);
        Vec::new()
    });

    let cursor = replay.last().map(|event| event.id).unwrap_or(last_event_id);
    watermark.fetch_max(cursor, Ordering::Relaxed);

    let gap = replay
        .first()
        .is_some_and(|first| first.id > last_event_id + 1);

    let mut frames: Vec<SseMessage> = Vec::with_capacity(replay.len() + 1);
    frames.push(SseMessage {
        id: None,
        event: String::from("hello"),
        data: serde_json::json!({
            "cursor": cursor,
            "replayed": replay.len(),
            "gap": gap,
        })
        .to_string(),
    });
    frames.extend(replay.into_iter().map(|event| SseMessage {
        id: Some(event.id),
        event: event.kind,
        data: serde_json::to_string(&event.payload).unwrap_or_else(|_| String::from("{}")),
    }));

    info!(
        "SSE 连接建立: user_id={} tenant_id={} conn={} since={} replayed={} gap={}",
        user_id,
        tenant_id,
        conn_id,
        last_event_id,
        frames.len() - 1,
        gap
    );

    let initial = futures::stream::iter(
        frames
            .into_iter()
            .map(|frame| Ok::<Event, Infallible>(frame.into_event())),
    );
    let live = futures::stream::unfold(
        (
            rx,
            cursor,
            ConnGuard {
                hub: state.hub.clone(),
                conn_id,
            },
        ),
        |(mut rx, last, guard)| async move {
            loop {
                let msg = rx.recv().await?;
                if let Some(id) = msg.id {
                    if id <= last {
                        continue;
                    }
                    return Some((Ok::<Event, Infallible>(msg.into_event()), (rx, id, guard)));
                }
                return Some((Ok::<Event, Infallible>(msg.into_event()), (rx, last, guard)));
            }
        },
    );

    let stream: Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> =
        Box::pin(initial.chain(live));
    let body = Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(state.cfg.keepalive())
            .text("keep-alive"),
    );
    Ok(body.into_response())
}

/// `POST /api/gateway/events` 的请求体（仅联调端点使用）
#[derive(Debug, Deserialize)]
pub struct PublishRequest {
    /// 目标租户
    pub tenant_id: i64,
    /// 事件类型
    pub kind: String,
    /// 事件正文
    pub payload: serde_json::Value,
}

/// 发布测试事件（仅 `GATEWAY_DEV_ENDPOINTS=1` 时挂载）
///
/// 多实例验收依赖它：在实例 A 上发布，实例 B 的连接应当同样收到。
pub async fn publish(
    State(state): State<Arc<GatewayState>>,
    Json(req): Json<PublishRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let id = coord::publish_event(
        &state.pool,
        &state.cfg.pg_channel,
        req.tenant_id,
        &req.kind,
        &req.payload,
    )
    .await
    .map_err(|e| ApiError::internal_error(format!("发布 SSE 事件失败: {}", e)))?;
    Ok(Json(serde_json::json!({
        "id": id,
        "connections": state.hub.len(),
    })))
}
