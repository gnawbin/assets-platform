//! 租户管理 Command
//!
//! 对应 lib.rs 中的 get_tenants / insert_tenant / update_tenant / delete_tenant

use crate::service;
use crate::service::tenant_service::TenantResponse;

/// 获取所有租户列表
#[tauri::command]
pub async fn get_tenants() -> Result<Vec<TenantResponse>, String> {
    service::tenant_service::get_tenants().await
}

/// 新增租户
#[tauri::command]
pub async fn insert_tenant(
    tenant_name: String,
    parent_id: Option<String>,
    is_leaf: bool,
    schema_name: Option<String>,
    enable: bool,
    created_by: Option<i64>,
) -> Result<TenantResponse, String> {
    let parent_id: Option<i64> = match parent_id {
        Some(s) if !s.is_empty() => Some(s.parse().map_err(|e| format!("无效的父租户ID: {}", e))?),
        _ => None,
    };
    service::tenant_service::insert_tenant(
        &tenant_name,
        parent_id,
        is_leaf,
        schema_name.as_deref(),
        enable,
        created_by,
    )
    .await
}

/// 更新租户信息
#[tauri::command]
pub async fn update_tenant(
    id: String,
    tenant_name: String,
    enable: bool,
) -> Result<TenantResponse, String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的租户ID: {}", e))?;
    service::tenant_service::update_tenant(id, &tenant_name, enable).await
}

/// 删除租户（禁用租户）
#[tauri::command]
pub async fn delete_tenant(id: String) -> Result<(), String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的租户ID: {}", e))?;
    service::tenant_service::delete_tenant(id).await
}

/// 切换租户 schema（需要 user_id）
///
/// 前端选择租户时调用，切换到对应租户的 schema。
/// tenant_id = 1 表示默认租户（public schema）。
#[tauri::command]
pub async fn switch_tenant(user_id: String, tenant_id: String) -> Result<String, String> {
    let user_id: i64 = user_id
        .parse()
        .map_err(|e| format!("无效的用户ID: {}", e))?;
    let tenant_id: i64 = tenant_id
        .parse()
        .map_err(|e| format!("无效的租户ID: {}", e))?;
    let info = service::tenant_service::switch_tenant(user_id, tenant_id).await?;
    Ok(info.schema_name.unwrap_or_default())
}
