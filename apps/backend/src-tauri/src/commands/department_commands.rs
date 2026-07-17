//! 部门管理 Command
//!
//! 对应 lib.rs 中的 get_departments / insert_department / update_department / delete_department

use crate::database::models::Department;
use crate::service;

/// 获取所有部门列表
#[tauri::command]
pub async fn get_departments(tenant_id: Option<String>) -> Result<Vec<Department>, String> {
    let tenant_id: Option<i64> = tenant_id
        .map(|id| {
            id.parse::<i64>()
                .map_err(|e| format!("无效的租户ID: {}", e))
        })
        .transpose()?;
    service::department_service::get_departments(tenant_id).await
}

/// 新增部门
#[tauri::command]
pub async fn insert_department(
    department_name: String,
    parent_id: Option<String>,
    description: Option<String>,
    currentUserId: Option<String>,
    tenant_id: Option<String>,
) -> Result<Department, String> {
    let parent_id: Option<i64> = parent_id
        .map(|id| {
            id.parse::<i64>()
                .map_err(|e| format!("无效的父部门ID: {}", e))
        })
        .transpose()?;
    let tenant_id: i64 = tenant_id
        .ok_or_else(|| "缺少租户ID参数".to_string())?
        .parse()
        .map_err(|e| format!("无效的租户ID: {}", e))?;
    service::department_service::insert_department(
        &department_name,
        parent_id,
        description.as_deref(),
        currentUserId.and_then(|id| id.parse().ok()),
        tenant_id,
    )
    .await
}

/// 更新部门
#[tauri::command]
pub async fn update_department(
    id: String,
    department_name: String,
    parent_id: Option<String>,
    description: Option<String>,
    currentUserId: Option<String>,
) -> Result<Department, String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的部门ID: {}", e))?;
    let parent_id: Option<i64> = parent_id
        .map(|id| {
            id.parse::<i64>()
                .map_err(|e| format!("无效的父部门ID: {}", e))
        })
        .transpose()?;
    service::department_service::update_department(
        id,
        &department_name,
        parent_id,
        description.as_deref(),
        currentUserId.and_then(|id| id.parse().ok()),
    )
    .await
}

/// 删除部门（软删除）
#[tauri::command]
pub async fn delete_department(id: String) -> Result<(), String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的部门ID: {}", e))?;
    service::department_service::delete_department(id).await
}
