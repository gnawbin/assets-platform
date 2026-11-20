//! 容器日志 / 性能 / 终端 HTTP API 路由
//!
//! 4 条只读 GET 与 2 条会话 REST 挂在 `protected_routes`（auth + authz 已就绪）；
//! 1 条 WebSocket 单独装配：**不套 auth 中间件**，靠一次性票据鉴权（与 SSH 终端同构）。
//!
//! 错误口径（实施方案 §3.4）：
//! - 连接 / 容器 / Pod / 会话不存在 → 404；
//! - 镜像无 shell、参数错误、未启用终端 → 400；
//! - 上游超时 → 504。

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::response::Response;
use axum::{Extension, Json};
use futures::SinkExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use service::container_exec_service::{
    self, ExecEvent, OpsTerminalSessionTicket, TerminalTarget,
};
use service::container_probe_service::{
    self, ContainerProbeError, OpsContainerLogsView, OpsContainerStatsView, OpsPodLogsView,
    OpsPodMetricsView,
};

use super::auth::UserContext;
use super::response::{ApiError, ApiResponse};
use super::ssh_ticket::TicketStore;

/// WebSocket 子路由的共享状态（只有票据存储）
#[derive(Clone)]
pub struct ContainerRouterState {
    /// 一次性票据存储（与 SSH 共用进程级单例即可：票据绑定 session_id）
    pub tickets: &'static TicketStore,
}

/// 容器日志查询参数
#[derive(Debug, Clone, Default, Deserialize, utoipa::IntoParams)]
pub struct ContainerLogsQuery {
    /// 末尾行数（默认 200，夹到 1..=2000）
    #[serde(default)]
    pub lines: Option<u32>,
    /// 是否让 daemon 加 RFC3339 时间戳
    #[serde(default)]
    pub timestamps: Option<bool>,
}

/// 容器性能查询参数
#[derive(Debug, Clone, Default, Deserialize, utoipa::IntoParams)]
pub struct ContainerStatsQuery {
    /// 是否取两条样本计算 CPU%（默认 false：one-shot）
    #[serde(default, rename = "twoSamples", alias = "two_samples")]
    pub two_samples: Option<bool>,
}

/// Pod 日志查询参数
#[derive(Debug, Clone, Default, Deserialize, utoipa::IntoParams)]
pub struct PodLogsQuery {
    /// 容器名（多容器 Pod 必填）
    #[serde(default)]
    pub container: Option<String>,
    /// 是否读取上一次运行日志
    #[serde(default)]
    pub previous: Option<bool>,
    /// 末尾行数（默认 200）
    #[serde(default)]
    pub lines: Option<u32>,
    /// 是否让 kubelet 加时间戳
    #[serde(default)]
    pub timestamps: Option<bool>,
}

/// 打开终端会话请求
#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenContainerSessionRequest {
    /// 采集连接 ID（雪花 ID，字符串或数字均可）
    #[serde(deserialize_with = "database::models::i64_from_string")]
    pub connection_id: i64,
    /// 终端目标
    pub target: TerminalTarget,
    /// shell（必须命中后端白名单）
    #[serde(default)]
    pub shell: Option<String>,
    /// 列数（缺省 120）
    #[serde(default)]
    pub cols: Option<u32>,
    /// 行数（缺省 32）
    #[serde(default)]
    pub rows: Option<u32>,
}

/// WS 升级查询参数
#[derive(Debug, Clone, Deserialize)]
pub struct ContainerWsQuery {
    /// 一次性票据
    pub ticket: String,
}

fn parse_id(raw: &str) -> Result<i64, ApiError> {
    raw.parse::<i64>()
        .map_err(|_| ApiError::bad_request(format!("无效的ID：{raw}")))
}

/// 只读探测错误 → HTTP 错误
fn map_probe_error(err: ContainerProbeError) -> ApiError {
    match err {
        ContainerProbeError::NotFound(message) => ApiError::not_found(message),
        ContainerProbeError::BadRequest(message) => ApiError::bad_request(message),
        ContainerProbeError::Timeout(message) => ApiError::new(504, message),
        ContainerProbeError::Internal(message) => ApiError::internal_error(message),
    }
}

/// 终端 service 文本错误 → HTTP 错误（「不存在」统一 404，其余 400）
fn map_exec_error(err: String) -> ApiError {
    if err.contains("不存在") {
        ApiError::not_found(err)
    } else {
        ApiError::bad_request(err)
    }
}

// ======================== 只读：日志 / 性能 ========================

/// Docker 容器日志
#[utoipa::path(
    get,
    path = "/api/ops/connections/{connectionId}/containers/{container}/logs",
    tag = "容器运维",
    security(("bearer_auth" = [])),
    params(
        ("connectionId" = String, Path, description = "连接 ID"),
        ("container" = String, Path, description = "容器 ID 或名称"),
        ContainerLogsQuery,
    ),
    responses(
        (status = 200, description = "容器日志", body = ApiResponse<service::container_probe_service::OpsContainerLogsView>),
        (status = 400, description = "参数 / Docker API 错误", body = ApiError),
        (status = 404, description = "连接或容器不存在", body = ApiError),
        (status = 504, description = "上游超时", body = ApiError),
    )
)]
pub async fn container_logs(
    Path((connection_id, container)): Path<(String, String)>,
    Query(query): Query<ContainerLogsQuery>,
) -> Result<Json<ApiResponse<OpsContainerLogsView>>, ApiError> {
    let connection_id = parse_id(&connection_id)?;
    container_probe_service::container_logs(
        connection_id,
        &container,
        query.lines,
        query.timestamps.unwrap_or(false),
    )
    .await
    .map(|view| Json(ApiResponse::success(view)))
    .map_err(map_probe_error)
}

/// Docker 容器性能
#[utoipa::path(
    get,
    path = "/api/ops/connections/{connectionId}/containers/{container}/stats",
    tag = "容器运维",
    security(("bearer_auth" = [])),
    params(
        ("connectionId" = String, Path, description = "连接 ID"),
        ("container" = String, Path, description = "容器 ID 或名称"),
        ContainerStatsQuery,
    ),
    responses(
        (status = 200, description = "容器性能快照", body = ApiResponse<service::container_probe_service::OpsContainerStatsView>),
        (status = 400, description = "参数 / Docker API 错误（含容器未运行）", body = ApiError),
        (status = 404, description = "连接或容器不存在", body = ApiError),
        (status = 504, description = "上游超时", body = ApiError),
    )
)]
pub async fn container_stats(
    Path((connection_id, container)): Path<(String, String)>,
    Query(query): Query<ContainerStatsQuery>,
) -> Result<Json<ApiResponse<OpsContainerStatsView>>, ApiError> {
    let connection_id = parse_id(&connection_id)?;
    container_probe_service::container_stats(
        connection_id,
        &container,
        query.two_samples.unwrap_or(false),
    )
    .await
    .map(|view| Json(ApiResponse::success(view)))
    .map_err(map_probe_error)
}

/// K8s Pod 日志
#[utoipa::path(
    get,
    path = "/api/ops/connections/{connectionId}/namespaces/{namespace}/pods/{pod}/logs",
    tag = "容器运维",
    security(("bearer_auth" = [])),
    params(
        ("connectionId" = String, Path, description = "连接 ID"),
        ("namespace" = String, Path, description = "命名空间"),
        ("pod" = String, Path, description = "Pod 名"),
        PodLogsQuery,
    ),
    responses(
        (status = 200, description = "Pod 日志", body = ApiResponse<service::container_probe_service::OpsPodLogsView>),
        (status = 400, description = "参数 / APIServer 错误", body = ApiError),
        (status = 404, description = "连接 / Pod / 容器不存在", body = ApiError),
        (status = 504, description = "上游超时", body = ApiError),
    )
)]
pub async fn pod_logs(
    Path((connection_id, namespace, pod)): Path<(String, String, String)>,
    Query(query): Query<PodLogsQuery>,
) -> Result<Json<ApiResponse<OpsPodLogsView>>, ApiError> {
    let connection_id = parse_id(&connection_id)?;
    container_probe_service::pod_logs(
        connection_id,
        &namespace,
        &pod,
        query.container.as_deref(),
        query.previous.unwrap_or(false),
        query.lines,
        query.timestamps.unwrap_or(false),
    )
    .await
    .map(|view| Json(ApiResponse::success(view)))
    .map_err(map_probe_error)
}

/// K8s Pod 指标（metrics-server）
#[utoipa::path(
    get,
    path = "/api/ops/connections/{connectionId}/namespaces/{namespace}/pods/{pod}/metrics",
    tag = "容器运维",
    security(("bearer_auth" = [])),
    params(
        ("connectionId" = String, Path, description = "连接 ID"),
        ("namespace" = String, Path, description = "命名空间"),
        ("pod" = String, Path, description = "Pod 名"),
    ),
    responses(
        (status = 200, description = "Pod 指标", body = ApiResponse<service::container_probe_service::OpsPodMetricsView>),
        (status = 400, description = "参数 / metrics-server 缺失或无权", body = ApiError),
        (status = 404, description = "连接或 Pod 不存在", body = ApiError),
        (status = 504, description = "上游超时", body = ApiError),
    )
)]
pub async fn pod_metrics(
    Path((connection_id, namespace, pod)): Path<(String, String, String)>,
) -> Result<Json<ApiResponse<OpsPodMetricsView>>, ApiError> {
    let connection_id = parse_id(&connection_id)?;
    container_probe_service::pod_metrics(connection_id, &namespace, &pod)
        .await
        .map(|view| Json(ApiResponse::success(view)))
        .map_err(map_probe_error)
}


// ======================== 终端会话 ========================

/// 打开终端会话，返回 `sessionId + ticket`
#[utoipa::path(
    post,
    path = "/api/ops/terminal/sessions",
    tag = "容器运维",
    security(("bearer_auth" = [])),
    request_body = OpenContainerSessionRequest,
    responses(
        (status = 200, description = "会话与一次性票据", body = ApiResponse<service::container_exec_service::OpsTerminalSessionTicket>),
        (status = 400, description = "未启用 / 无 shell / 参数错误 / 超出并发上限", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "连接 / 容器 / Pod 不存在", body = ApiError),
    )
)]
pub async fn open_session(
    State(state): State<Arc<ContainerRouterState>>,
    Extension(ctx): Extension<UserContext>,
    connect_info: Option<Extension<ConnectInfo<SocketAddr>>>,
    Json(request): Json<OpenContainerSessionRequest>,
) -> Result<Json<ApiResponse<OpsTerminalSessionTicket>>, ApiError> {
    let cols = request.cols.unwrap_or(120).clamp(1, 1000);
    let rows = request.rows.unwrap_or(32).clamp(1, 1000);
    let client_ip = connect_info.map(|Extension(ConnectInfo(addr))| addr.ip().to_string());
    let session_id = container_exec_service::open_session(
        ctx.user_id,
        request.connection_id,
        &request.target,
        request.shell.as_deref(),
        cols,
        rows,
        client_ip,
    )
    .await
    .map_err(map_exec_error)?;
    let (ticket, expires_in) = state.tickets.issue(ctx.user_id, session_id);
    debug!(
        session_id,
        user_id = ctx.user_id,
        "容器终端会话票据已签发"
    );
    Ok(Json(ApiResponse::success(OpsTerminalSessionTicket {
        session_id: session_id.to_string(),
        ticket,
        expires_in,
    })))
}

/// 主动关闭会话（幂等）
#[utoipa::path(
    delete,
    path = "/api/ops/terminal/sessions/{sessionId}",
    tag = "容器运维",
    security(("bearer_auth" = [])),
    params(("sessionId" = String, Path, description = "会话 ID")),
    responses(
        (status = 200, description = "关闭成功（重复调用同样 200）"),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 404, description = "会话不存在 / 不属于当前用户", body = ApiError),
    )
)]
pub async fn close_session(
    Extension(ctx): Extension<UserContext>,
    Path(session_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let session_id = parse_id(&session_id)?;
    container_exec_service::close_session(session_id, ctx.user_id, "client requested close")
        .await
        .map(|()| Json(ApiResponse::success_empty()))
        .map_err(map_exec_error)
}

/// 终端全双工通道（票据鉴权，不经过 auth 中间件）
#[utoipa::path(
    get,
    path = "/api/ops/terminal/sessions/{sessionId}/ws",
    tag = "容器运维",
    params(
        ("sessionId" = String, Path, description = "会话 ID"),
        ("ticket" = String, Query, description = "一次性票据"),
    ),
    responses(
        (status = 101, description = "协议切换成功"),
        (status = 401, description = "票据无效或已过期", body = ApiError),
        (status = 404, description = "会话不存在", body = ApiError),
    )
)]
pub async fn session_ws(
    State(state): State<Arc<ContainerRouterState>>,
    Path(session_id): Path<String>,
    Query(query): Query<ContainerWsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let session_id = parse_id(&session_id)?;
    let user_id = state
        .tickets
        .consume(&query.ticket, session_id)
        .ok_or_else(|| ApiError::unauthorized("会话票据无效或已过期"))?;
    let output = container_exec_service::attach_output(session_id, user_id)
        .map_err(|_| ApiError::not_found("会话不存在"))?;
    Ok(ws.on_upgrade(move |socket| pump(socket, session_id, user_id, output)))
}


/// 客户端控制帧（与 SSH 终端完全一致）
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ClientControl {
    Resize { cols: u32, rows: u32 },
    Ping,
    Close,
}

/// 解析 Text 帧：能识别成控制帧就返回 `Some`，否则按原始输入处理
fn parse_control(text: &str) -> Option<ClientControl> {
    serde_json::from_str::<ClientControl>(text).ok()
}

/// 双向泵：`ExecEvent` → WS 帧；WS 帧 → 容器 stdin
async fn pump(
    mut socket: WebSocket,
    session_id: i64,
    user_id: i64,
    mut output: mpsc::Receiver<ExecEvent>,
) {
    loop {
        tokio::select! {
            event = output.recv() => {
                let Some(event) = event else {
                    debug!(session_id, "容器终端输出端已关闭");
                    break;
                };
                if !forward_event(&mut socket, session_id, event).await {
                    break;
                }
            }
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else {
                    break;
                };
                if !handle_client_message(&mut socket, session_id, user_id, message).await {
                    break;
                }
            }
        }
    }
    let _ = socket.close().await;
    // 幂等：前端 DELETE 与 WS 断线双触发都安全
    if let Err(err) =
        container_exec_service::close_session(session_id, user_id, "ws disconnected").await
    {
        warn!(session_id, error = %err, "WS 断开后关闭容器终端会话失败");
    }
}

/// 把终端事件转成 WS 帧；返回 `false` 表示应结束泵
async fn forward_event(socket: &mut WebSocket, session_id: i64, event: ExecEvent) -> bool {
    match event {
        ExecEvent::Data(data) => socket.send(Message::Binary(data.into())).await.is_ok(),
        ExecEvent::Exit(code) => {
            container_exec_service::note_exit(session_id, code);
            send_json(socket, &ExitFrame { kind: "exit", code }).await
        }
        ExecEvent::Closed(reason) => {
            let _ = send_json(
                socket,
                &ClosedFrame {
                    kind: "closed",
                    reason: &reason,
                },
            )
            .await;
            false
        }
    }
}

/// 处理客户端帧；返回 `false` 表示应结束泵
async fn handle_client_message(
    socket: &mut WebSocket,
    session_id: i64,
    user_id: i64,
    message: Message,
) -> bool {
    match message {
        Message::Binary(data) => {
            container_exec_service::write_input(session_id, user_id, data.to_vec())
                .await
                .is_ok()
        }
        Message::Text(text) => match parse_control(text.as_str()) {
            Some(ClientControl::Resize { cols, rows }) => {
                let _ =
                    container_exec_service::resize_session(session_id, user_id, cols, rows).await;
                true
            }
            Some(ClientControl::Ping) => send_json(socket, &PongFrame { kind: "pong" }).await,
            Some(ClientControl::Close) => false,
            // 不是 JSON 控制帧：按原始输入写进容器 stdin
            None => {
                container_exec_service::write_input(
                    session_id,
                    user_id,
                    text.as_str().as_bytes().to_vec(),
                )
                .await
                .is_ok()
            }
        },
        Message::Close(_) => false,
        _ => true,
    }
}

/// 退出帧
#[derive(Serialize)]
struct ExitFrame {
    #[serde(rename = "type")]
    kind: &'static str,
    code: Option<i32>,
}

/// 关闭帧
#[derive(Serialize)]
struct ClosedFrame<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    reason: &'a str,
}

/// pong 帧
#[derive(Serialize)]
struct PongFrame {
    #[serde(rename = "type")]
    kind: &'static str,
}

/// 发送 JSON 文本帧
async fn send_json<T: Serialize>(socket: &mut WebSocket, payload: &T) -> bool {
    let Ok(text) = serde_json::to_string(payload) else {
        return false;
    };
    socket.send(Message::Text(text.into())).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_resize_control_frame() {
        let control =
            parse_control(r#"{"type":"resize","cols":120,"rows":32}"#).expect("resize");
        assert!(matches!(
            control,
            ClientControl::Resize { cols: 120, rows: 32 }
        ));
    }

    #[test]
    fn plain_text_falls_back_to_raw_input() {
        assert!(parse_control("ls -l\n").is_none());
        assert!(parse_control(r#"{"cols":1}"#).is_none());
    }

    #[test]
    fn missing_shell_maps_to_400() {
        let error = map_exec_error(
            "该镜像不含 shell（scratch/distroless），请改用日志/性能或 debug sidecar".to_string(),
        );
        assert_eq!(error.code, 400);
    }

    #[test]
    fn missing_container_maps_to_404() {
        assert_eq!(map_exec_error("容器不存在：web".to_string()).code, 404);
    }

    #[test]
    fn probe_errors_map_to_expected_status() {
        assert_eq!(
            map_probe_error(ContainerProbeError::NotFound("x".into())).code,
            404
        );
        assert_eq!(
            map_probe_error(ContainerProbeError::Timeout("x".into())).code,
            504
        );
        assert_eq!(
            map_probe_error(ContainerProbeError::BadRequest("x".into())).code,
            400
        );
    }
}

