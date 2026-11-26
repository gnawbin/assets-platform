//! 运维终端（SSH）HTTP API 路由
//!
//! 7 条 REST 挂在 `protected_routes`（`auth_middleware` + `authz_middleware` 已就绪），
//! 1 条 WebSocket 单独装配：**不套 auth 中间件**，靠一次性票据（`ssh_ticket`）鉴权，
//! 与 `/api/upload/*`、`/api/chat/stream` 的「独立 State、独立挂载」做法一致。
//!
//! 错误口径（方案 §5.3）：
//! - 票据无效 / 过期 → 401；
//! - 会话 / 主机不存在或不属于当前用户 → 404（同文案，避免枚举）；
//! - 配置 / 连接 / 认证失败 → 400。

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

use remote_shell::SshEvent;
use service::ssh_service::{
    self, SshProbeInput, SshProbeView, SshSessionTicket, SshSessionView, SshTargetInput,
    SshTargetView,
};

use super::auth::{AuthPrincipal, UserContext};
use super::response::{ApiError, ApiResponse};
use super::ssh_ticket::TicketStore;

/// WebSocket 子路由的共享状态（只有票据存储）
#[derive(Clone)]
pub struct SshRouterState {
    /// 一次性票据存储（进程级单例）
    pub tickets: &'static TicketStore,
}

/// 开会话请求
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct OpenSshSessionRequest {
    /// 目标主机 id（雪花 ID，字符串或数字均可）
    #[serde(
        alias = "targetId",
        deserialize_with = "database::models::i64_from_string"
    )]
    pub target_id: i64,
    /// 终端列数（缺省 120）
    #[serde(default)]
    pub cols: Option<u32>,
    /// 终端行数（缺省 32）
    #[serde(default)]
    pub rows: Option<u32>,
}

/// 会话列表查询参数
#[derive(Debug, Clone, Default, Deserialize, utoipa::IntoParams)]
pub struct SshSessionQuery {
    /// 是否包含其他用户的会话（仅超级管理员有效）
    #[serde(default)]
    pub all: Option<bool>,
}

/// WS 升级查询参数
#[derive(Debug, Clone, Deserialize)]
pub struct SshWsQuery {
    /// 一次性票据
    pub ticket: String,
}

/// 路径 id 解析
fn parse_id(raw: &str) -> Result<i64, ApiError> {
    raw.parse::<i64>()
        .map_err(|_| ApiError::bad_request(format!("无效的ID：{raw}")))
}

/// service 文本错误 → HTTP 错误（「不存在」统一 404）
fn map_service_error(err: String) -> ApiError {
    if err.contains("不存在") {
        ApiError::not_found(err)
    } else {
        ApiError::bad_request(err)
    }
}

// ======================== 主机 CRUD ========================

/// 主机列表（不含凭证）
#[utoipa::path(
    get,
    path = "/api/ops/ssh/targets",
    tag = "运维终端",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "SSH 主机列表", body = ApiResponse<Vec<service::ssh_service::SshTargetView>>),
        (status = 401, description = "未认证", body = ApiError),
        (status = 403, description = "权限不足", body = ApiError),
    )
)]
pub async fn list_targets() -> Result<Json<ApiResponse<Vec<SshTargetView>>>, ApiError> {
    ssh_service::list_targets()
        .await
        .map(|list| Json(ApiResponse::success(list)))
        .map_err(ApiError::internal_error)
}

/// 主机详情（不含凭证）
#[utoipa::path(
    get,
    path = "/api/ops/ssh/targets/{id}",
    tag = "运维终端",
    security(("bearer_auth" = [])),
    params(("id" = String, Path, description = "主机 ID")),
    responses(
        (status = 200, description = "SSH 主机详情", body = ApiResponse<service::ssh_service::SshTargetView>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 404, description = "主机不存在", body = ApiError),
    )
)]
pub async fn get_target(
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<SshTargetView>>, ApiError> {
    let id = parse_id(&id)?;
    ssh_service::get_target(id)
        .await
        .map(|view| Json(ApiResponse::success(view)))
        .map_err(map_service_error)
}

/// 新建主机
#[utoipa::path(
    post,
    path = "/api/ops/ssh/targets",
    tag = "运维终端",
    security(("bearer_auth" = [])),
    request_body = service::ssh_service::SshTargetInput,
    responses(
        (status = 200, description = "新建成功", body = ApiResponse<service::ssh_service::SshTargetView>),
        (status = 400, description = "参数 / 加密密钥错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 403, description = "权限不足", body = ApiError),
    )
)]
pub async fn create_target(
    Extension(ctx): Extension<UserContext>,
    Json(input): Json<SshTargetInput>,
) -> Result<Json<ApiResponse<SshTargetView>>, ApiError> {
    ssh_service::create_target(&input, Some(ctx.user_id))
        .await
        .map(|view| Json(ApiResponse::success(view)))
        .map_err(map_service_error)
}

/// 编辑主机（凭证字段缺省 = 保留原凭证）
#[utoipa::path(
    put,
    path = "/api/ops/ssh/targets/{id}",
    tag = "运维终端",
    security(("bearer_auth" = [])),
    request_body = service::ssh_service::SshTargetInput,
    params(("id" = String, Path, description = "主机 ID")),
    responses(
        (status = 200, description = "更新后的主机", body = ApiResponse<service::ssh_service::SshTargetView>),
        (status = 400, description = "参数 / 加密密钥错误", body = ApiError),
        (status = 404, description = "主机不存在", body = ApiError),
    )
)]
pub async fn update_target(
    Extension(ctx): Extension<UserContext>,
    Path(id): Path<String>,
    Json(input): Json<SshTargetInput>,
) -> Result<Json<ApiResponse<SshTargetView>>, ApiError> {
    let id = parse_id(&id)?;
    ssh_service::update_target(id, &input, Some(ctx.user_id))
        .await
        .map(|view| Json(ApiResponse::success(view)))
        .map_err(map_service_error)
}

/// 删除主机（软删；存在活动会话时 400）
#[utoipa::path(
    delete,
    path = "/api/ops/ssh/targets/{id}",
    tag = "运维终端",
    security(("bearer_auth" = [])),
    params(("id" = String, Path, description = "主机 ID")),
    responses(
        (status = 200, description = "删除成功（`data` 为 null）"),
        (status = 400, description = "存在活动会话 / 参数错误", body = ApiError),
        (status = 404, description = "主机不存在", body = ApiError),
    )
)]
pub async fn delete_target(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id = parse_id(&id)?;
    ssh_service::delete_target(id)
        .await
        .map(|()| Json(ApiResponse::success_empty()))
        .map_err(map_service_error)
}

/// 扫描已保存主机的指纹（不认证）
#[utoipa::path(
    post,
    path = "/api/ops/ssh/targets/{id}/probe",
    tag = "运维终端",
    security(("bearer_auth" = [])),
    params(("id" = String, Path, description = "主机 ID")),
    responses(
        (status = 200, description = "主机指纹", body = ApiResponse<service::ssh_service::SshProbeView>),
        (status = 400, description = "不可达 / 握手失败", body = ApiError),
        (status = 404, description = "主机不存在", body = ApiError),
    )
)]
pub async fn probe_target(
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<SshProbeView>>, ApiError> {
    let id = parse_id(&id)?;
    ssh_service::probe_target(id)
        .await
        .map(|view| Json(ApiResponse::success(view)))
        .map_err(map_service_error)
}

/// 表单未保存时扫描指纹（凭证不落库）
#[utoipa::path(
    post,
    path = "/api/ops/ssh/targets/test",
    tag = "运维终端",
    security(("bearer_auth" = [])),
    request_body = service::ssh_service::SshProbeInput,
    responses(
        (status = 200, description = "主机指纹", body = ApiResponse<service::ssh_service::SshProbeView>),
        (status = 400, description = "不可达 / 握手失败", body = ApiError),
    )
)]
pub async fn probe_draft(
    Json(input): Json<SshProbeInput>,
) -> Result<Json<ApiResponse<SshProbeView>>, ApiError> {
    ssh_service::probe_draft(&input)
        .await
        .map(|view| Json(ApiResponse::success(view)))
        .map_err(map_service_error)
}

// ======================== 会话 ========================

/// 活跃会话列表（超管可加 `?all=true` 查看全部）
#[utoipa::path(
    get,
    path = "/api/ops/ssh/sessions",
    tag = "运维终端",
    security(("bearer_auth" = [])),
    params(SshSessionQuery),
    responses(
        (status = 200, description = "活跃会话列表", body = ApiResponse<Vec<service::ssh_service::SshSessionView>>),
        (status = 401, description = "未认证", body = ApiError),
    )
)]
pub async fn list_sessions(
    Extension(ctx): Extension<UserContext>,
    principal: Option<Extension<AuthPrincipal>>,
    Query(query): Query<SshSessionQuery>,
) -> Result<Json<ApiResponse<Vec<SshSessionView>>>, ApiError> {
    // 仅超管可以查看他人的会话；普通用户始终只能看到自己的
    let is_super = principal.is_some_and(|Extension(principal)| principal.is_super_admin);
    let include_all = query.all.unwrap_or(false) && is_super;
    ssh_service::list_sessions(ctx.user_id, include_all)
        .await
        .map(|list| Json(ApiResponse::success(list)))
        .map_err(ApiError::internal_error)
}

/// 打开会话，返回 `sessionId + ticket`（票据用于随后的 WS 升级）
#[utoipa::path(
    post,
    path = "/api/ops/ssh/sessions",
    tag = "运维终端",
    security(("bearer_auth" = [])),
    request_body = OpenSshSessionRequest,
    responses(
        (status = 200, description = "会话与一次性票据", body = ApiResponse<service::ssh_service::SshSessionTicket>),
        (status = 400, description = "不可达 / 认证失败 / 超出并发上限", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "主机不存在", body = ApiError),
    )
)]
pub async fn open_session(
    State(state): State<Arc<SshRouterState>>,
    Extension(ctx): Extension<UserContext>,
    connect_info: Option<Extension<ConnectInfo<SocketAddr>>>,
    Json(request): Json<OpenSshSessionRequest>,
) -> Result<Json<ApiResponse<SshSessionTicket>>, ApiError> {
    let cols = request.cols.unwrap_or(120).clamp(1, 10_000);
    let rows = request.rows.unwrap_or(32).clamp(1, 10_000);
    let client_ip = connect_info.map(|Extension(ConnectInfo(addr))| addr.ip().to_string());
    let session_id = ssh_service::open_session(
        ctx.user_id,
        request.target_id,
        cols,
        rows,
        client_ip,
    )
    .await
    .map_err(map_service_error)?;
    let (ticket, expires_in) = state.tickets.issue(ctx.user_id, session_id);
    debug!(session_id, user_id = ctx.user_id, "SSH 会话票据已签发");
    Ok(Json(ApiResponse::success(SshSessionTicket {
        session_id: session_id.to_string(),
        ticket,
        expires_in,
    })))
}

/// 主动关闭会话（幂等）
#[utoipa::path(
    delete,
    path = "/api/ops/ssh/sessions/{sessionId}",
    tag = "运维终端",
    security(("bearer_auth" = [])),
    params(("sessionId" = String, Path, description = "会话 ID")),
    responses(
        (status = 200, description = "关闭成功（重复调用同样 200，`data` 为 null）"),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 404, description = "会话不存在 / 不属于当前用户", body = ApiError),
    )
)]
pub async fn close_session(
    Extension(ctx): Extension<UserContext>,
    Path(session_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let session_id = parse_id(&session_id)?;
    ssh_service::close_session(session_id, ctx.user_id, "client requested close")
        .await
        .map(|()| Json(ApiResponse::success_empty()))
        .map_err(map_service_error)
}

// ======================== WebSocket ========================

/// 终端全双工通道（票据鉴权，不经过 auth 中间件）
#[utoipa::path(
    get,
    path = "/api/ops/ssh/sessions/{sessionId}/ws",
    tag = "运维终端",
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
    State(state): State<Arc<SshRouterState>>,
    Path(session_id): Path<String>,
    Query(query): Query<SshWsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let session_id = parse_id(&session_id)?;
    let user_id = state
        .tickets
        .consume(&query.ticket, session_id)
        .ok_or_else(|| ApiError::unauthorized("会话票据无效或已过期"))?;
    let output = ssh_service::attach_output(session_id, user_id)
        .map_err(|_| ApiError::not_found("会话不存在"))?;
    Ok(ws.on_upgrade(move |socket| pump(socket, session_id, user_id, output)))
}

/// 客户端控制帧
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ClientControl {
    /// 窗口变化
    Resize {
        /// 列数
        cols: u32,
        /// 行数
        rows: u32,
    },
    /// 保活
    Ping,
    /// 主动断开
    Close,
}

/// 解析 Text 帧：能识别成控制帧就返回 `Some`，否则按原始输入处理
///
/// 方案 §6.2 的容错要求：个别终端库会把按键当字符串发过来，此时必须原样写进 PTY。
fn parse_control(text: &str) -> Option<ClientControl> {
    serde_json::from_str::<ClientControl>(text).ok()
}

/// 双向泵：`SshEvent` → WS 帧；WS 帧 → PTY
async fn pump(
    mut socket: WebSocket,
    session_id: i64,
    user_id: i64,
    mut output: mpsc::Receiver<SshEvent>,
) {
    loop {
        tokio::select! {
            event = output.recv() => {
                let Some(event) = event else {
                    debug!(session_id, "SSH 输出端已关闭");
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
    if let Err(err) = ssh_service::close_session(session_id, user_id, "ws disconnected").await {
        warn!(session_id, error = %err, "WS 断开后关闭 SSH 会话失败");
    }
}

/// 把 SSH 事件转成 WS 帧；返回 `false` 表示应结束泵
async fn forward_event(socket: &mut WebSocket, session_id: i64, event: SshEvent) -> bool {
    match event {
        SshEvent::Data(data) => socket.send(Message::Binary(data.into())).await.is_ok(),
        SshEvent::Exit(code) => {
            ssh_service::note_exit(session_id, code);
            send_json(socket, &ExitFrame { kind: "exit", code }).await
        }
        SshEvent::Closed(reason) => {
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
            ssh_service::write_input(session_id, user_id, data.to_vec())
                .await
                .is_ok()
        }
        Message::Text(text) => match parse_control(text.as_str()) {
            Some(ClientControl::Resize { cols, rows }) => {
                let _ = ssh_service::resize_session(session_id, user_id, cols, rows).await;
                true
            }
            Some(ClientControl::Ping) => send_json(
                socket,
                &PongFrame {
                    kind: "pong",
                },
            )
            .await,
            Some(ClientControl::Close) => false,
            // 不是 JSON 控制帧：按原始输入写进 PTY
            None => {
                ssh_service::write_input(session_id, user_id, text.as_str().as_bytes().to_vec())
                    .await
                    .is_ok()
            }
        },
        Message::Close(_) => false,
        // Ping / Pong 由协议层处理，业务不感知
        _ => true,
    }
}

/// 退出帧
#[derive(Serialize)]
struct ExitFrame {
    #[serde(rename = "type")]
    kind: &'static str,
    code: Option<u32>,
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
        let control = parse_control(r#"{"type":"resize","cols":120,"rows":32}"#).expect("resize");
        assert!(matches!(
            control,
            ClientControl::Resize { cols: 120, rows: 32 }
        ));
    }

    #[test]
    fn parses_ping_and_close() {
        assert!(matches!(parse_control(r#"{"type":"ping"}"#), Some(ClientControl::Ping)));
        assert!(matches!(parse_control(r#"{"type":"close"}"#), Some(ClientControl::Close)));
    }

    #[test]
    fn plain_text_falls_back_to_raw_input() {
        // 不是 JSON 控制帧 → 按原样写入 PTY（`ls -l`、粘贴的文本等）
        assert!(parse_control("ls -l\n").is_none());
        assert!(parse_control(r#"{"cols":1}"#).is_none());
    }

    #[test]
    fn invalid_ids_are_rejected() {
        assert!(parse_id("abc").is_err());
        assert_eq!(parse_id("123").expect("合法 id"), 123);
    }

    #[test]
    fn missing_session_maps_to_404() {
        let error = map_service_error("会话不存在".to_string());
        assert_eq!(error.code, 404);
        let error = map_service_error("主机不可达".to_string());
        assert_eq!(error.code, 400);
    }
}


