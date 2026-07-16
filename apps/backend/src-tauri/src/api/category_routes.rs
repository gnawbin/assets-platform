//! 资产分类 HTTP API 路由
//!
//! 提供资产分类的 RESTful 接口。

use axum::{extract::Path, Json};
use serde::Deserialize;

use crate::database::models::AssetCategory;
use crate::service;

use super::response::{ApiError, ApiResponse};

/// 创建分类请求
#[derive(Debug, Deserialize)]
pub struct CreateCategoryRequest {
    pub category_name: String,
    pub asset_type: String,
    #[serde(deserialize_with = "crate::database::models::opt_i64_from_string")]
    pub parent_id: Option<i64>,
    pub sort: i16,
    pub description: Option<String>,
}

/// 更新分类请求
#[derive(Debug, Deserialize)]
pub struct UpdateCategoryRequest {
    pub category_name: String,
    pub asset_type: String,
    #[serde(deserialize_with = "crate::database::models::opt_i64_from_string")]
    pub parent_id: Option<i64>,
    pub sort: i16,
    pub description: Option<String>,
}

/// 获取所有资产分类
pub async fn get_categories() -> Result<Json<ApiResponse<Vec<AssetCategory>>>, ApiError> {
    match service::assets_categories_service::get_categories().await {
        Ok(categories) => Ok(Json(ApiResponse::success(categories))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 获取顶级分类
pub async fn get_categories_parents() -> Result<Json<ApiResponse<Vec<AssetCategory>>>, ApiError> {
    match service::assets_categories_service::get_super_categories().await {
        Ok(categories) => Ok(Json(ApiResponse::success(categories))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增资产分类
pub async fn insert_category(
    Json(req): Json<CreateCategoryRequest>,
) -> Result<Json<ApiResponse<AssetCategory>>, ApiError> {
    let category = AssetCategory {
        id: 0, // service 层会生成新 ID
        category_name: req.category_name,
        asset_type: req.asset_type,
        parent_id: req.parent_id,
        sort: req.sort,
        description: req.description,
        created_by: Some(1),
        created_at: None,
        updated_by: Some(1),
        updated_at: None,
        deleted: Some(0),
    };

    match service::assets_categories_service::insert_category(&category).await {
        Ok(cat) => Ok(Json(ApiResponse::success(cat))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 更新资产分类
pub async fn update_category(
    Path(id): Path<String>,
    Json(req): Json<UpdateCategoryRequest>,
) -> Result<Json<ApiResponse<AssetCategory>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的分类ID"))?;

    let category = AssetCategory {
        id,
        category_name: req.category_name,
        asset_type: req.asset_type,
        parent_id: req.parent_id,
        sort: req.sort,
        description: req.description,
        created_by: Some(1),
        created_at: None,
        updated_by: Some(1),
        updated_at: None,
        deleted: Some(0),
    };

    match service::assets_categories_service::update_category(&category).await {
        Ok(cat) => Ok(Json(ApiResponse::success(cat))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 删除资产分类
pub async fn delete_category(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的分类ID"))?;

    match service::assets_categories_service::delete_category(id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}
