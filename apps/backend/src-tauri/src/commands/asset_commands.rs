//! 资产管理 Command
//!
//! 对应 lib.rs 中的固定资产 + 无形资产管理相关 command

use assets_service;
use assets_service::assets_service::{
    HardwareAssetInput, HardwareAssetView, IntangibleAssetInput, IntangibleAssetView,
};

// ======================== 固定资产管理 ========================

/// 获取所有固定资产列表
#[tauri::command]
pub async fn get_hardware_assets() -> Result<Vec<HardwareAssetView>, String> {
    assets_service::assets_service::get_hardware_assets().await
}

/// 新增固定资产
#[tauri::command]
pub async fn insert_hardware_asset(
    input: HardwareAssetInput,
    currentUserId: Option<String>,
) -> Result<HardwareAssetView, String> {
    let user_id = currentUserId.and_then(|id| id.parse().ok()).unwrap_or(1);
    assets_service::assets_service::insert_hardware_asset(input, user_id).await
}

/// 修改固定资产
#[tauri::command]
pub async fn update_hardware_asset(
    id: String,
    input: HardwareAssetInput,
    currentUserId: Option<String>,
) -> Result<HardwareAssetView, String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的资产ID: {}", e))?;
    let user_id = currentUserId.and_then(|id| id.parse().ok()).unwrap_or(1);
    assets_service::assets_service::update_hardware_asset(id, input, user_id).await
}

/// 删除固定资产（软删除）
#[tauri::command]
pub async fn delete_hardware_asset(id: String) -> Result<(), String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的资产ID: {}", e))?;
    assets_service::assets_service::delete_hardware_asset(id).await
}

// ======================== 无形资产管理 ========================

/// 获取所有无形资产列表
#[tauri::command]
pub async fn get_intangible_assets() -> Result<Vec<IntangibleAssetView>, String> {
    assets_service::assets_service::get_intangible_assets().await
}

/// 新增无形资产
#[tauri::command]
pub async fn insert_intangible_asset(
    input: IntangibleAssetInput,
    currentUserId: Option<String>,
) -> Result<IntangibleAssetView, String> {
    let user_id = currentUserId.and_then(|id| id.parse().ok()).unwrap_or(1);
    assets_service::assets_service::insert_intangible_asset(input, user_id).await
}

/// 修改无形资产
#[tauri::command]
pub async fn update_intangible_asset(
    id: String,
    input: IntangibleAssetInput,
    currentUserId: Option<String>,
) -> Result<IntangibleAssetView, String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的资产ID: {}", e))?;
    let user_id = currentUserId.and_then(|id| id.parse().ok()).unwrap_or(1);
    assets_service::assets_service::update_intangible_asset(id, input, user_id).await
}

/// 删除无形资产（软删除）
#[tauri::command]
pub async fn delete_intangible_asset(id: String) -> Result<(), String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的资产ID: {}", e))?;
    assets_service::assets_service::delete_intangible_asset(id).await
}
