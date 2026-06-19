//! 角色权限管理 Command
//!
//! 对应 lib.rs 中的 insert_role / get_roles / get_role_menu_ids / assign_role_menus / delete_role / get_all_menus_tree

use crate::database::models::{MantineTree, Role, SidebarMenuItem};
use crate::service;
use tracing::info;

/// 新增角色
#[tauri::command]
pub async fn insert_role(
    role_key: String,
    role_name: String,
    description: Option<String>,
    tenant_id: String,
) -> Result<Role, String> {
    let tid: i64 = tenant_id
        .parse()
        .map_err(|e| format!("无效的租户ID: {}", e))?;
    service::role_service::insert_role_by_params(
        &role_key,
        &role_name,
        description.as_deref(),
        false, // 新增角色默认非超级管理员
        Some(tid),
        Some(1), // created_by
    )
    .await
}

/// 获取所有角色列表（支持按租户筛选和关键词搜索）
#[tauri::command]
pub async fn get_roles(
    tenant_id: Option<String>,
    keyword: Option<String>,
) -> Result<Vec<Role>, String> {
    info!(
        "[DEBUG] get_roles called: tenant_id={:?}, keyword={:?}",
        tenant_id, keyword
    );
    let tid = match tenant_id {
        Some(ref id) if !id.is_empty() => Some(
            id.parse::<i64>()
                .map_err(|e| format!("无效的租户ID: {}", e))?,
        ),
        _ => None,
    };
    let result = service::role_service::get_roles(tid, keyword).await;
    info!(
        "[DEBUG] get_roles result: {:?}",
        result.as_ref().map(|r| r.len())
    );
    result
}

/// 获取指定角色已分配的菜单权限ID列表
#[tauri::command]
pub async fn get_role_menu_ids(role_id: String) -> Result<Vec<i64>, String> {
    let role_id: i64 = role_id
        .parse()
        .map_err(|e| format!("无效的角色ID: {}", e))?;
    service::role_service::get_role_menu_ids(role_id).await
}

/// 为角色分配菜单权限
#[tauri::command]
pub async fn assign_role_menus(role_id: String, menu_ids: Vec<String>) -> Result<(), String> {
    let role_id: i64 = role_id
        .parse()
        .map_err(|e| format!("无效的角色ID: {}", e))?;
    let menu_ids: Vec<i64> = menu_ids
        .into_iter()
        .map(|id| {
            id.parse::<i64>()
                .map_err(|e| format!("无效的菜单ID: {}", e))
        })
        .collect::<Result<Vec<i64>, String>>()?;
    service::role_service::assign_role_menus(role_id, menu_ids).await
}

/// 删除角色
#[tauri::command]
pub async fn delete_role(role_id: String) -> Result<(), String> {
    let role_id: i64 = role_id
        .parse()
        .map_err(|e| format!("无效的角色ID: {}", e))?;
    service::role_service::delete_role(role_id).await
}

/// 获取所有菜单树（用于权限分配）
#[tauri::command]
pub async fn get_all_menus_tree() -> Result<Vec<MantineTree>, String> {
    service::role_service::get_all_menus_tree().await
}

/// 获取侧边栏菜单（只返回目录和菜单，不返回按钮）
///
/// 根据用户角色过滤菜单：
/// - 超级管理员：返回所有可见菜单
/// - 普通用户：只返回其角色已分配的菜单
#[tauri::command]
pub async fn get_user_menus(user_id: Option<String>) -> Result<Vec<SidebarMenuItem>, String> {
    match user_id {
        Some(uid) => {
            let user_id: i64 = uid.parse().map_err(|e| format!("无效的用户ID: {}", e))?;
            service::role_service::get_user_menus(user_id).await
        }
        None => Ok(Vec::new()), // 未登录时返回空菜单
    }
}

/// 获取用户已分配的角色 ID 列表
#[tauri::command]
pub async fn get_user_role_ids(id: String) -> Result<Vec<i64>, String> {
    let user_id: i64 = id.parse().map_err(|e| format!("无效的用户ID: {}", e))?;
    service::role_service::get_user_role_ids(user_id).await
}

/// 为用户分配角色
#[tauri::command]
pub async fn assign_user_roles(id: String, role_ids: Vec<String>) -> Result<(), String> {
    let user_id: i64 = id.parse().map_err(|e| format!("无效的用户ID: {}", e))?;
    let role_ids: Vec<i64> = role_ids
        .into_iter()
        .map(|rid| {
            rid.parse::<i64>()
                .map_err(|e| format!("无效的角色ID: {}", e))
        })
        .collect::<Result<Vec<i64>, String>>()?;
    service::role_service::assign_user_roles(user_id, role_ids).await
}
