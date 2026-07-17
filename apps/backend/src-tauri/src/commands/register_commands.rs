//! 注册申请 Command
//!
//! 提供用户注册申请、审核、驳回等 Tauri Command。

use crate::service;
use crate::service::register_service::RegisterResponse;

/// 用户注册申请
#[tauri::command]
pub async fn register(
    username: String,
    password: String,
    real_name: String,
    email: Option<String>,
    phone: Option<String>,
    department_name: Option<String>,
    company_name: Option<String>,
    reason: Option<String>,
) -> Result<RegisterResponse, String> {
    service::register_service::register(
        &username,
        &password,
        &real_name,
        email.as_deref(),
        phone.as_deref(),
        department_name.as_deref(),
        company_name.as_deref(),
        reason.as_deref(),
    )
    .await
}

/// 获取注册申请列表
#[tauri::command]
pub async fn get_registrations(status: Option<i16>) -> Result<Vec<RegisterResponse>, String> {
    service::register_service::get_registrations(status).await
}

/// 审核通过注册申请
#[tauri::command]
pub async fn approve_registration(
    id: String,
    approve_by: i64,
    tenant_id: i64,
    approve_remark: Option<String>,
) -> Result<RegisterResponse, String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的申请ID: {}", e))?;
    service::register_service::approve_registration(
        id,
        approve_by,
        tenant_id,
        approve_remark.as_deref(),
    )
    .await
}

/// 驳回注册申请
#[tauri::command]
pub async fn reject_registration(
    id: String,
    approve_by: i64,
    approve_remark: Option<String>,
) -> Result<RegisterResponse, String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的申请ID: {}", e))?;
    service::register_service::reject_registration(id, approve_by, approve_remark.as_deref()).await
}
