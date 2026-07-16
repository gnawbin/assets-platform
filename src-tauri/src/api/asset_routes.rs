//! 资产 HTTP API 路由
//!
//! 提供固定资产和无形资产的 RESTful 接口。

use axum::{extract::Path, Extension, Json};

use crate::service;
use crate::service::assets_service::{
    HardwareAssetInput, HardwareAssetView, IntangibleAssetInput, IntangibleAssetView,
};

use super::auth::UserContext;
use super::response::{ApiError, ApiResponse};

// ======================== 固定资产 ========================

/// 获取所有固定资产
pub async fn get_hardware_assets() -> Result<Json<ApiResponse<Vec<HardwareAssetView>>>, ApiError> {
    match service::assets_service::get_hardware_assets().await {
        Ok(assets) => Ok(Json(ApiResponse::success(assets))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增固定资产
pub async fn insert_hardware_asset(
    Extension(ctx): Extension<UserContext>,
    Json(input): Json<HardwareAssetInput>,
) -> Result<Json<ApiResponse<HardwareAssetView>>, ApiError> {
    match service::assets_service::insert_hardware_asset(input, ctx.user_id).await {
        Ok(asset) => Ok(Json(ApiResponse::success(asset))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 更新固定资产
pub async fn update_hardware_asset(
    Extension(ctx): Extension<UserContext>,
    Path(id): Path<String>,
    Json(input): Json<HardwareAssetInput>,
) -> Result<Json<ApiResponse<HardwareAssetView>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的资产ID"))?;

    match service::assets_service::update_hardware_asset(id, input, ctx.user_id).await {
        Ok(asset) => Ok(Json(ApiResponse::success(asset))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 删除固定资产
pub async fn delete_hardware_asset(
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的资产ID"))?;

    match service::assets_service::delete_hardware_asset(id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

// ======================== 无形资产 ========================

/// 获取所有无形资产
pub async fn get_intangible_assets() -> Result<Json<ApiResponse<Vec<IntangibleAssetView>>>, ApiError>
{
    match service::assets_service::get_intangible_assets().await {
        Ok(assets) => Ok(Json(ApiResponse::success(assets))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增无形资产
pub async fn insert_intangible_asset(
    Extension(ctx): Extension<UserContext>,
    Json(input): Json<IntangibleAssetInput>,
) -> Result<Json<ApiResponse<IntangibleAssetView>>, ApiError> {
    match service::assets_service::insert_intangible_asset(input, ctx.user_id).await {
        Ok(asset) => Ok(Json(ApiResponse::success(asset))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 更新无形资产
pub async fn update_intangible_asset(
    Extension(ctx): Extension<UserContext>,
    Path(id): Path<String>,
    Json(input): Json<IntangibleAssetInput>,
) -> Result<Json<ApiResponse<IntangibleAssetView>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的资产ID"))?;

    match service::assets_service::update_intangible_asset(id, input, ctx.user_id).await {
        Ok(asset) => Ok(Json(ApiResponse::success(asset))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 删除无形资产
pub async fn delete_intangible_asset(
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的资产ID"))?;

    match service::assets_service::delete_intangible_asset(id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}
