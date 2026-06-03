//! 资产类别管理 Command
//!
//! 对应 lib.rs 中的 get_categories / get_categories_parents / insert_category / update_category / delete_category

use crate::database::models::AssetCategory;
use crate::service;

/// 获取所有资产类别列表
#[tauri::command]
pub async fn get_categories() -> Result<Vec<AssetCategory>, String> {
    service::assets_categories_service::get_categories().await
}

/// 获取所有资产类别列表
#[tauri::command]
pub async fn get_categories_parents() -> Result<Vec<AssetCategory>, String> {
    service::assets_categories_service::get_super_categories().await
}

/// 新增资产类别
#[tauri::command]
pub async fn insert_category(category: AssetCategory) -> Result<AssetCategory, String> {
    service::assets_categories_service::insert_category(&category).await
}

/// 更新资产类别
#[tauri::command]
pub async fn update_category(category: AssetCategory) -> Result<AssetCategory, String> {
    service::assets_categories_service::update_category(&category).await
}

/// 删除资产类别
#[tauri::command]
pub async fn delete_category(id: String) -> Result<(), String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    service::assets_categories_service::delete_category(id).await
}
