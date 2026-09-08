//! 流程管理 Command
//!
//! 对应 lib.rs 中的流程管理相关 command

use assets_service;
use assets_service::process_service::{
    AssetPurchaseInput, AssetPurchaseUpdateInput, AssetReceiveInput, AssetReceiveUpdateInput,
    AssetRepairInput, AssetRepairUpdateInput, AssetReturnInput, AssetReturnUpdateInput,
    AssetScrapInput, AssetScrapUpdateInput, AssetTransferInput, AssetTransferUpdateInput,
};

use assets_database::models::{
    AssetPurchase, AssetReceive, AssetRepair, AssetReturn, AssetScrap, AssetTransfer,
};

// ======================== 领用管理 ========================

#[tauri::command]
pub async fn get_receives() -> Result<Vec<AssetReceive>, String> {
    assets_service::process_service::get_receives().await
}

#[tauri::command]
pub async fn insert_receive(input: AssetReceiveInput) -> Result<AssetReceive, String> {
    assets_service::process_service::insert_receive(input).await
}

#[tauri::command]
pub async fn update_receive(
    id: String,
    input: AssetReceiveUpdateInput,
) -> Result<AssetReceive, String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    assets_service::process_service::update_receive(id, input).await
}

#[tauri::command]
pub async fn delete_receive(id: String) -> Result<(), String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    assets_service::process_service::delete_receive(id).await
}

// ======================== 归还管理 ========================

#[tauri::command]
pub async fn get_returns() -> Result<Vec<AssetReturn>, String> {
    assets_service::process_service::get_returns().await
}

#[tauri::command]
pub async fn insert_return(input: AssetReturnInput) -> Result<AssetReturn, String> {
    assets_service::process_service::insert_return(input).await
}

#[tauri::command]
pub async fn update_return(
    id: String,
    input: AssetReturnUpdateInput,
) -> Result<AssetReturn, String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    assets_service::process_service::update_return(id, input).await
}

#[tauri::command]
pub async fn delete_return(id: String) -> Result<(), String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    assets_service::process_service::delete_return(id).await
}

// ======================== 调拨管理 ========================

#[tauri::command]
pub async fn get_transfers() -> Result<Vec<AssetTransfer>, String> {
    assets_service::process_service::get_transfers().await
}

#[tauri::command]
pub async fn insert_transfer(input: AssetTransferInput) -> Result<AssetTransfer, String> {
    assets_service::process_service::insert_transfer(input).await
}

#[tauri::command]
pub async fn update_transfer(
    id: String,
    input: AssetTransferUpdateInput,
) -> Result<AssetTransfer, String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    assets_service::process_service::update_transfer(id, input).await
}

#[tauri::command]
pub async fn delete_transfer(id: String) -> Result<(), String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    assets_service::process_service::delete_transfer(id).await
}

// ======================== 维修管理 ========================

#[tauri::command]
pub async fn get_repairs() -> Result<Vec<AssetRepair>, String> {
    assets_service::process_service::get_repairs().await
}

#[tauri::command]
pub async fn insert_repair(input: AssetRepairInput) -> Result<AssetRepair, String> {
    assets_service::process_service::insert_repair(input).await
}

#[tauri::command]
pub async fn update_repair(
    id: String,
    input: AssetRepairUpdateInput,
) -> Result<AssetRepair, String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    assets_service::process_service::update_repair(id, input).await
}

#[tauri::command]
pub async fn delete_repair(id: String) -> Result<(), String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    assets_service::process_service::delete_repair(id).await
}

// ======================== 报废管理 ========================

#[tauri::command]
pub async fn get_scraps() -> Result<Vec<AssetScrap>, String> {
    assets_service::process_service::get_scraps().await
}

#[tauri::command]
pub async fn insert_scrap(input: AssetScrapInput) -> Result<AssetScrap, String> {
    assets_service::process_service::insert_scrap(input).await
}

#[tauri::command]
pub async fn update_scrap(id: String, input: AssetScrapUpdateInput) -> Result<AssetScrap, String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    assets_service::process_service::update_scrap(id, input).await
}

#[tauri::command]
pub async fn delete_scrap(id: String) -> Result<(), String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    assets_service::process_service::delete_scrap(id).await
}

// ======================== 采购管理 ========================

#[tauri::command]
pub async fn get_purchases() -> Result<Vec<AssetPurchase>, String> {
    assets_service::process_service::get_purchases().await
}

#[tauri::command]
pub async fn insert_purchase(input: AssetPurchaseInput) -> Result<AssetPurchase, String> {
    assets_service::process_service::insert_purchase(input).await
}

#[tauri::command]
pub async fn update_purchase(
    id: String,
    input: AssetPurchaseUpdateInput,
) -> Result<AssetPurchase, String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    assets_service::process_service::update_purchase(id, input).await
}

#[tauri::command]
pub async fn delete_purchase(id: String) -> Result<(), String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    assets_service::process_service::delete_purchase(id).await
}
