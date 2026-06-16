//! 流程管理 HTTP API 路由
//!
//! 提供资产领用、归还、调拨、维修、报废、采购的 RESTful 接口。

use axum::{extract::Path, Json};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::service::process_service::{
    AssetPurchaseInput, AssetPurchaseUpdateInput, AssetReceiveInput, AssetReceiveUpdateInput,
    AssetRepairInput, AssetRepairUpdateInput, AssetReturnInput, AssetReturnUpdateInput,
    AssetScrapInput, AssetScrapUpdateInput, AssetTransferInput, AssetTransferUpdateInput,
};

use crate::database::models::{
    AssetPurchase, AssetReceive, AssetRepair, AssetReturn, AssetScrap, AssetTransfer,
};

use super::response::{ApiError, ApiResponse};

// ======================== 领用管理 ========================

/// 获取所有领用记录
#[utoipa::path(
    get,
    path = "/api/process/receive",
    tag = "流程管理-领用",
    responses(
        (status = 200, description = "获取成功", body = ApiResponse<Vec<AssetReceive>>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_receives() -> Result<Json<ApiResponse<Vec<AssetReceive>>>, ApiError> {
    match crate::service::process_service::get_receives().await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增领用记录
#[utoipa::path(
    post,
    path = "/api/process/receive",
    tag = "流程管理-领用",
    request_body = AssetReceiveInput,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<AssetReceive>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn insert_receive(
    Json(input): Json<AssetReceiveInput>,
) -> Result<Json<ApiResponse<AssetReceive>>, ApiError> {
    match crate::service::process_service::insert_receive(input).await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 更新领用记录
#[utoipa::path(
    put,
    path = "/api/process/receive/{id}",
    tag = "流程管理-领用",
    request_body = AssetReceiveUpdateInput,
    responses(
        (status = 200, description = "更新成功", body = ApiResponse<AssetReceive>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn update_receive(
    Path(id): Path<String>,
    Json(input): Json<AssetReceiveUpdateInput>,
) -> Result<Json<ApiResponse<AssetReceive>>, ApiError> {
    let id: i64 = id.parse().map_err(|_| ApiError::bad_request("无效的ID"))?;
    match crate::service::process_service::update_receive(id, input).await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 删除领用记录
#[utoipa::path(
    delete,
    path = "/api/process/receive/{id}",
    tag = "流程管理-领用",
    responses(
        (status = 200, description = "删除成功"),
        (status = 500, description = "服务器错误"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn delete_receive(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id.parse().map_err(|_| ApiError::bad_request("无效的ID"))?;
    match crate::service::process_service::delete_receive(id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

// ======================== 归还管理 ========================

/// 获取所有归还记录
#[utoipa::path(
    get,
    path = "/api/process/return",
    tag = "流程管理-归还",
    responses(
        (status = 200, description = "获取成功", body = ApiResponse<Vec<AssetReturn>>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_returns() -> Result<Json<ApiResponse<Vec<AssetReturn>>>, ApiError> {
    match crate::service::process_service::get_returns().await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增归还记录
#[utoipa::path(
    post,
    path = "/api/process/return",
    tag = "流程管理-归还",
    request_body = AssetReturnInput,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<AssetReturn>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn insert_return(
    Json(input): Json<AssetReturnInput>,
) -> Result<Json<ApiResponse<AssetReturn>>, ApiError> {
    match crate::service::process_service::insert_return(input).await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 更新归还记录
#[utoipa::path(
    put,
    path = "/api/process/return/{id}",
    tag = "流程管理-归还",
    request_body = AssetReturnUpdateInput,
    responses(
        (status = 200, description = "更新成功", body = ApiResponse<AssetReturn>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn update_return(
    Path(id): Path<String>,
    Json(input): Json<AssetReturnUpdateInput>,
) -> Result<Json<ApiResponse<AssetReturn>>, ApiError> {
    let id: i64 = id.parse().map_err(|_| ApiError::bad_request("无效的ID"))?;
    match crate::service::process_service::update_return(id, input).await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 删除归还记录
#[utoipa::path(
    delete,
    path = "/api/process/return/{id}",
    tag = "流程管理-归还",
    responses(
        (status = 200, description = "删除成功"),
        (status = 500, description = "服务器错误"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn delete_return(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id.parse().map_err(|_| ApiError::bad_request("无效的ID"))?;
    match crate::service::process_service::delete_return(id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

// ======================== 调拨管理 ========================

/// 获取所有调拨记录
#[utoipa::path(
    get,
    path = "/api/process/transfer",
    tag = "流程管理-调拨",
    responses(
        (status = 200, description = "获取成功", body = ApiResponse<Vec<AssetTransfer>>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_transfers() -> Result<Json<ApiResponse<Vec<AssetTransfer>>>, ApiError> {
    match crate::service::process_service::get_transfers().await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增调拨记录
#[utoipa::path(
    post,
    path = "/api/process/transfer",
    tag = "流程管理-调拨",
    request_body = AssetTransferInput,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<AssetTransfer>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn insert_transfer(
    Json(input): Json<AssetTransferInput>,
) -> Result<Json<ApiResponse<AssetTransfer>>, ApiError> {
    match crate::service::process_service::insert_transfer(input).await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 更新调拨记录
#[utoipa::path(
    put,
    path = "/api/process/transfer/{id}",
    tag = "流程管理-调拨",
    request_body = AssetTransferUpdateInput,
    responses(
        (status = 200, description = "更新成功", body = ApiResponse<AssetTransfer>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn update_transfer(
    Path(id): Path<String>,
    Json(input): Json<AssetTransferUpdateInput>,
) -> Result<Json<ApiResponse<AssetTransfer>>, ApiError> {
    let id: i64 = id.parse().map_err(|_| ApiError::bad_request("无效的ID"))?;
    match crate::service::process_service::update_transfer(id, input).await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 删除调拨记录
#[utoipa::path(
    delete,
    path = "/api/process/transfer/{id}",
    tag = "流程管理-调拨",
    responses(
        (status = 200, description = "删除成功"),
        (status = 500, description = "服务器错误"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn delete_transfer(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id.parse().map_err(|_| ApiError::bad_request("无效的ID"))?;
    match crate::service::process_service::delete_transfer(id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

// ======================== 维修管理 ========================

/// 获取所有维修记录
#[utoipa::path(
    get,
    path = "/api/process/repair",
    tag = "流程管理-维修",
    responses(
        (status = 200, description = "获取成功", body = ApiResponse<Vec<AssetRepair>>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_repairs() -> Result<Json<ApiResponse<Vec<AssetRepair>>>, ApiError> {
    match crate::service::process_service::get_repairs().await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增维修记录
#[utoipa::path(
    post,
    path = "/api/process/repair",
    tag = "流程管理-维修",
    request_body = AssetRepairInput,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<AssetRepair>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn insert_repair(
    Json(input): Json<AssetRepairInput>,
) -> Result<Json<ApiResponse<AssetRepair>>, ApiError> {
    match crate::service::process_service::insert_repair(input).await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 更新维修记录
#[utoipa::path(
    put,
    path = "/api/process/repair/{id}",
    tag = "流程管理-维修",
    request_body = AssetRepairUpdateInput,
    responses(
        (status = 200, description = "更新成功", body = ApiResponse<AssetRepair>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn update_repair(
    Path(id): Path<String>,
    Json(input): Json<AssetRepairUpdateInput>,
) -> Result<Json<ApiResponse<AssetRepair>>, ApiError> {
    let id: i64 = id.parse().map_err(|_| ApiError::bad_request("无效的ID"))?;
    match crate::service::process_service::update_repair(id, input).await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 删除维修记录
#[utoipa::path(
    delete,
    path = "/api/process/repair/{id}",
    tag = "流程管理-维修",
    responses(
        (status = 200, description = "删除成功"),
        (status = 500, description = "服务器错误"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn delete_repair(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id.parse().map_err(|_| ApiError::bad_request("无效的ID"))?;
    match crate::service::process_service::delete_repair(id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

// ======================== 报废管理 ========================

/// 获取所有报废记录
#[utoipa::path(
    get,
    path = "/api/process/scrap",
    tag = "流程管理-报废",
    responses(
        (status = 200, description = "获取成功", body = ApiResponse<Vec<AssetScrap>>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_scraps() -> Result<Json<ApiResponse<Vec<AssetScrap>>>, ApiError> {
    match crate::service::process_service::get_scraps().await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增报废记录
#[utoipa::path(
    post,
    path = "/api/process/scrap",
    tag = "流程管理-报废",
    request_body = AssetScrapInput,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<AssetScrap>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn insert_scrap(
    Json(input): Json<AssetScrapInput>,
) -> Result<Json<ApiResponse<AssetScrap>>, ApiError> {
    match crate::service::process_service::insert_scrap(input).await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 更新报废记录
#[utoipa::path(
    put,
    path = "/api/process/scrap/{id}",
    tag = "流程管理-报废",
    request_body = AssetScrapUpdateInput,
    responses(
        (status = 200, description = "更新成功", body = ApiResponse<AssetScrap>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn update_scrap(
    Path(id): Path<String>,
    Json(input): Json<AssetScrapUpdateInput>,
) -> Result<Json<ApiResponse<AssetScrap>>, ApiError> {
    let id: i64 = id.parse().map_err(|_| ApiError::bad_request("无效的ID"))?;
    match crate::service::process_service::update_scrap(id, input).await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 删除报废记录
#[utoipa::path(
    delete,
    path = "/api/process/scrap/{id}",
    tag = "流程管理-报废",
    responses(
        (status = 200, description = "删除成功"),
        (status = 500, description = "服务器错误"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn delete_scrap(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id.parse().map_err(|_| ApiError::bad_request("无效的ID"))?;
    match crate::service::process_service::delete_scrap(id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

// ======================== 采购管理 ========================

/// 获取所有采购记录
#[utoipa::path(
    get,
    path = "/api/process/purchase",
    tag = "流程管理-采购",
    responses(
        (status = 200, description = "获取成功", body = ApiResponse<Vec<AssetPurchase>>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_purchases() -> Result<Json<ApiResponse<Vec<AssetPurchase>>>, ApiError> {
    match crate::service::process_service::get_purchases().await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增采购记录
#[utoipa::path(
    post,
    path = "/api/process/purchase",
    tag = "流程管理-采购",
    request_body = AssetPurchaseInput,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<AssetPurchase>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn insert_purchase(
    Json(input): Json<AssetPurchaseInput>,
) -> Result<Json<ApiResponse<AssetPurchase>>, ApiError> {
    match crate::service::process_service::insert_purchase(input).await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 更新采购记录
#[utoipa::path(
    put,
    path = "/api/process/purchase/{id}",
    tag = "流程管理-采购",
    request_body = AssetPurchaseUpdateInput,
    responses(
        (status = 200, description = "更新成功", body = ApiResponse<AssetPurchase>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn update_purchase(
    Path(id): Path<String>,
    Json(input): Json<AssetPurchaseUpdateInput>,
) -> Result<Json<ApiResponse<AssetPurchase>>, ApiError> {
    let id: i64 = id.parse().map_err(|_| ApiError::bad_request("无效的ID"))?;
    match crate::service::process_service::update_purchase(id, input).await {
        Ok(data) => Ok(Json(ApiResponse::success(data))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 删除采购记录
#[utoipa::path(
    delete,
    path = "/api/process/purchase/{id}",
    tag = "流程管理-采购",
    responses(
        (status = 200, description = "删除成功"),
        (status = 500, description = "服务器错误"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn delete_purchase(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id.parse().map_err(|_| ApiError::bad_request("无效的ID"))?;
    match crate::service::process_service::delete_purchase(id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}
