//! 资产 HTTP API 路由
//!
//! 提供固定资产和无形资产的 RESTful 接口。

use axum::{extract::Path, Json};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::service;
use crate::service::assets_service::{
    HardwareAssetInput, HardwareAssetView, IntangibleAssetInput, IntangibleAssetView,
};

use super::response::{ApiError, ApiResponse};

// ======================== 固定资产 ========================

/// 获取所有固定资产
#[utoipa::path(
    get,
    path = "/api/assets/hardware",
    tag = "固定资产",
    responses(
        (status = 200, description = "获取成功", body = ApiResponse<Vec<HardwareAssetView>>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_hardware_assets() -> Result<Json<ApiResponse<Vec<HardwareAssetView>>>, ApiError> {
    match service::assets_service::get_hardware_assets().await {
        Ok(assets) => Ok(Json(ApiResponse::success(assets))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增固定资产
#[utoipa::path(
    post,
    path = "/api/assets/hardware",
    tag = "固定资产",
    request_body = HardwareAssetInput,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<HardwareAssetView>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn insert_hardware_asset(
    Json(input): Json<HardwareAssetInput>,
) -> Result<Json<ApiResponse<HardwareAssetView>>, ApiError> {
    match service::assets_service::insert_hardware_asset(input).await {
        Ok(asset) => Ok(Json(ApiResponse::success(asset))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 更新固定资产
#[utoipa::path(
    put,
    path = "/api/assets/hardware/{id}",
    tag = "固定资产",
    request_body = HardwareAssetInput,
    responses(
        (status = 200, description = "更新成功", body = ApiResponse<HardwareAssetView>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn update_hardware_asset(
    Path(id): Path<String>,
    Json(input): Json<HardwareAssetInput>,
) -> Result<Json<ApiResponse<HardwareAssetView>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的资产ID"))?;

    match service::assets_service::update_hardware_asset(id, input).await {
        Ok(asset) => Ok(Json(ApiResponse::success(asset))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 删除固定资产
#[utoipa::path(
    delete,
    path = "/api/assets/hardware/{id}",
    tag = "固定资产",
    responses(
        (status = 200, description = "删除成功"),
        (status = 500, description = "服务器错误"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
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
#[utoipa::path(
    get,
    path = "/api/assets/intangible",
    tag = "无形资产",
    responses(
        (status = 200, description = "获取成功", body = ApiResponse<Vec<IntangibleAssetView>>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_intangible_assets() -> Result<Json<ApiResponse<Vec<IntangibleAssetView>>>, ApiError>
{
    match service::assets_service::get_intangible_assets().await {
        Ok(assets) => Ok(Json(ApiResponse::success(assets))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增无形资产
#[utoipa::path(
    post,
    path = "/api/assets/intangible",
    tag = "无形资产",
    request_body = IntangibleAssetInput,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<IntangibleAssetView>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn insert_intangible_asset(
    Json(input): Json<IntangibleAssetInput>,
) -> Result<Json<ApiResponse<IntangibleAssetView>>, ApiError> {
    match service::assets_service::insert_intangible_asset(input).await {
        Ok(asset) => Ok(Json(ApiResponse::success(asset))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 更新无形资产
#[utoipa::path(
    put,
    path = "/api/assets/intangible/{id}",
    tag = "无形资产",
    request_body = IntangibleAssetInput,
    responses(
        (status = 200, description = "更新成功", body = ApiResponse<IntangibleAssetView>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn update_intangible_asset(
    Path(id): Path<String>,
    Json(input): Json<IntangibleAssetInput>,
) -> Result<Json<ApiResponse<IntangibleAssetView>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的资产ID"))?;

    match service::assets_service::update_intangible_asset(id, input).await {
        Ok(asset) => Ok(Json(ApiResponse::success(asset))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 删除无形资产
#[utoipa::path(
    delete,
    path = "/api/assets/intangible/{id}",
    tag = "无形资产",
    responses(
        (status = 200, description = "删除成功"),
        (status = 500, description = "服务器错误"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
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
