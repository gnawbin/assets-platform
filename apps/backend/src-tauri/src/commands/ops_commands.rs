//! 运维采集 Command
//!
//! 对应 service::ops_service 的 11 个入口；本层只做参数解析与委托，零业务逻辑。

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
