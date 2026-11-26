//! 运维采集 Command
//!
//! 对应 service::ops_service 的 11 个入口；本层只做参数解析与委托，零业务逻辑。

use service::container_exec_service::{
    self, OpsTerminalSessionTicket, TerminalTarget,
};
use service::container_probe_service::{
    self, OpsContainerLogsView, OpsContainerStatsView, OpsPodLogsView, OpsPodMetricsView,
};
use service::ops_service::{
    self, OpsCollectResult, OpsConnectionInput, OpsConnectionView, OpsHealthView,
    OpsProviderOption, OpsResourceLinkView,
};

fn parse_id(raw: &str, label: &str) -> Result<i64, String> {
    raw.parse::<i64>()
        .map_err(|err| format!("无效的{label}ID: {err}"))
}

/// 来源 / 认证方式下拉数据
#[tauri::command]
pub async fn get_ops_providers() -> Result<Vec<OpsProviderOption>, String> {
    Ok(ops_service::provider_options())
}

/// 连接列表
#[tauri::command]
pub async fn get_ops_connections() -> Result<Vec<OpsConnectionView>, String> {
    ops_service::list_connections().await
}

/// 单条连接
#[tauri::command]
pub async fn get_ops_connection(id: String) -> Result<OpsConnectionView, String> {
    ops_service::get_connection(parse_id(&id, "连接")?).await
}

/// 新建连接
#[tauri::command]
pub async fn create_ops_connection(
    input: OpsConnectionInput,
    currentUserId: Option<String>,
) -> Result<OpsConnectionView, String> {
    let user_id = currentUserId.and_then(|value| value.parse().ok());
    ops_service::create_connection(input, user_id).await
}

/// 更新连接
#[tauri::command]
pub async fn update_ops_connection(
    id: String,
    input: OpsConnectionInput,
    currentUserId: Option<String>,
) -> Result<OpsConnectionView, String> {
    let user_id = currentUserId.and_then(|value| value.parse().ok());
    ops_service::update_connection(parse_id(&id, "连接")?, input, user_id).await
}

/// 删除连接（软删）
#[tauri::command]
pub async fn delete_ops_connection(id: String) -> Result<(), String> {
    ops_service::delete_connection(parse_id(&id, "连接")?).await
}

/// 草稿探测（不落库）
#[tauri::command]
pub async fn test_ops_connection_draft(
    input: OpsConnectionInput,
) -> Result<OpsHealthView, String> {
    ops_service::test_draft(&input).await
}

/// 已存连接探测
#[tauri::command]
pub async fn test_ops_connection(id: String) -> Result<OpsHealthView, String> {
    ops_service::test_connection(parse_id(&id, "连接")?).await
}

/// 立即采集一次
#[tauri::command]
pub async fn collect_ops_resources(id: String) -> Result<OpsCollectResult, String> {
    ops_service::collect_now(parse_id(&id, "连接")?).await
}

/// 资源清单（可按连接过滤）
#[tauri::command]
pub async fn get_ops_links(
    connectionId: Option<String>,
) -> Result<Vec<OpsResourceLinkView>, String> {
    let connection_id = match connectionId {
        Some(raw) if !raw.trim().is_empty() => Some(parse_id(raw.trim(), "连接")?),
        _ => None,
    };
    ops_service::list_links(connection_id).await
}

// ======================== 容器日志 / 性能 / 终端 ========================

/// Docker 容器日志
#[tauri::command]
pub async fn get_ops_container_logs(
    connectionId: String,
    container: String,
    lines: Option<u32>,
    timestamps: Option<bool>,
) -> Result<OpsContainerLogsView, String> {
    container_probe_service::container_logs(
        parse_id(&connectionId, "连接")?,
        &container,
        lines,
        timestamps.unwrap_or(false),
    )
    .await
    .map_err(|err| err.into_message())
}

/// Docker 容器性能
#[tauri::command]
pub async fn get_ops_container_stats(
    connectionId: String,
    container: String,
    twoSamples: Option<bool>,
) -> Result<OpsContainerStatsView, String> {
    container_probe_service::container_stats(
        parse_id(&connectionId, "连接")?,
        &container,
        twoSamples.unwrap_or(false),
    )
    .await
    .map_err(|err| err.into_message())
}

/// K8s Pod 日志
#[tauri::command]
pub async fn get_ops_pod_logs(
    connectionId: String,
    namespace: String,
    pod: String,
    container: Option<String>,
    previous: Option<bool>,
    lines: Option<u32>,
    timestamps: Option<bool>,
) -> Result<OpsPodLogsView, String> {
    container_probe_service::pod_logs(
        parse_id(&connectionId, "连接")?,
        &namespace,
        &pod,
        container.as_deref(),
        previous.unwrap_or(false),
        lines,
        timestamps.unwrap_or(false),
    )
    .await
    .map_err(|err| err.into_message())
}

/// K8s Pod 指标
#[tauri::command]
pub async fn get_ops_pod_metrics(
    connectionId: String,
    namespace: String,
    pod: String,
) -> Result<OpsPodMetricsView, String> {
    container_probe_service::pod_metrics(
        parse_id(&connectionId, "连接")?,
        &namespace,
        &pod,
    )
    .await
    .map_err(|err| err.into_message())
}

/// 打开容器终端会话（票据用于随后的 WS 升级）
#[tauri::command]
pub async fn open_ops_terminal_session(
    connectionId: String,
    target: TerminalTarget,
    shell: Option<String>,
    cols: Option<u32>,
    rows: Option<u32>,
    currentUserId: Option<String>,
) -> Result<OpsTerminalSessionTicket, String> {
    let user_id = currentUserId
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);
    let session_id = container_exec_service::open_session(
        user_id,
        parse_id(&connectionId, "连接")?,
        &target,
        shell.as_deref(),
        cols.unwrap_or(120).clamp(1, 1000),
        rows.unwrap_or(32).clamp(1, 1000),
        None,
    )
    .await?;
    // Tauri 通道不承载 WS 票据，票据只在本地 HTTP 模式签发；这里返回空票据让调用方走 HTTP
    Ok(OpsTerminalSessionTicket {
        session_id: session_id.to_string(),
        ticket: String::new(),
        expires_in: 0,
    })
}

/// 关闭容器终端会话（幂等）
#[tauri::command]
pub async fn close_ops_terminal_session(
    sessionId: String,
    currentUserId: Option<String>,
) -> Result<(), String> {
    let user_id = currentUserId
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);
    container_exec_service::close_session(
        parse_id(&sessionId, "会话")?,
        user_id,
        "client requested close",
    )
    .await
}

/// 把观测资源归档到已有资产
#[tauri::command]
pub async fn bind_ops_link(
    linkId: String,
    assetId: String,
    currentUserId: Option<String>,
) -> Result<OpsResourceLinkView, String> {
    let user_id = currentUserId.and_then(|value| value.parse().ok());
    ops_service::bind_link(
        parse_id(&linkId, "资源关联")?,
        parse_id(&assetId, "资产")?,
        user_id,
    )
    .await
}
