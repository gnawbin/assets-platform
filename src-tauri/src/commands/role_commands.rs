//! 角色权限管理 Command
//!
//! 对应 lib.rs 中的 insert_role / get_roles / get_role_menu_ids / assign_role_menus / delete_role / get_all_menus_tree

use crate::database::models::{MantineTree, Role, SidebarMenuItem};
use crate::service;

/// 新增角色
#[tauri::command]
pub async fn insert_role(role: Role) -> Result<Role, String> {
    service::role_service::insert_role(&role).await
}

/// 获取所有角色列表
#[tauri::command]
pub async fn get_roles() -> Result<Vec<Role>, String> {
    service::role_service::get_roles().await
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
#[tauri::command]
pub async fn get_user_menus() -> Result<Vec<SidebarMenuItem>, String> {
    service::role_service::get_user_menus().await
}
