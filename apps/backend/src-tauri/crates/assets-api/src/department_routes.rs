//! 部门 HTTP API 路由
//!
//! 提供部门的 RESTful 接口。

use assets_database::models::Department;
use assets_service;
use axum::{extract::Path, extract::Query, Json};
use serde::Deserialize;

use super::response::{ApiError, ApiResponse};

/// 查询参数
#[derive(Debug, Deserialize)]
pub struct DepartmentQuery {
    pub tenant_id: Option<String>,
}

/// 创建部门请求
#[derive(Debug, Deserialize)]
pub struct CreateDepartmentRequest {
    pub department_name: String,
    #[serde(deserialize_with = "assets_database::models::opt_i64_from_string")]
    pub parent_id: Option<i64>,
    pub description: Option<String>,
    #[serde(deserialize_with = "assets_database::models::i64_from_string")]
    pub tenant_id: i64,
}

/// 更新部门请求
#[derive(Debug, Deserialize)]
pub struct UpdateDepartmentRequest {
    pub department_name: String,
    #[serde(deserialize_with = "assets_database::models::opt_i64_from_string")]
    pub parent_id: Option<i64>,
    pub description: Option<String>,
}

/// 获取所有部门
pub async fn get_departments(
    Query(query): Query<DepartmentQuery>,
) -> Result<Json<ApiResponse<Vec<Department>>>, ApiError> {
    let tenant_id: Option<i64> = query
        .tenant_id
        .map(|id| {
            id.parse::<i64>()
                .map_err(|_| ApiError::bad_request("无效的租户ID"))
        })
        .transpose()?;
    match assets_service::department_service::get_departments(tenant_id).await {
        Ok(departments) => Ok(Json(ApiResponse::success(departments))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增部门
pub async fn insert_department(
    Json(req): Json<CreateDepartmentRequest>,
) -> Result<Json<ApiResponse<Department>>, ApiError> {
    match assets_service::department_service::insert_department(
        &req.department_name,
        req.parent_id,
        req.description.as_deref(),
        Some(1),
        req.tenant_id,
    )
    .await
    {
        Ok(dept) => Ok(Json(ApiResponse::success(dept))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 更新部门
pub async fn update_department(
    Path(id): Path<String>,
    Json(req): Json<UpdateDepartmentRequest>,
) -> Result<Json<ApiResponse<Department>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的部门ID"))?;

    match assets_service::department_service::update_department(
        id,
        &req.department_name,
        req.parent_id,
        req.description.as_deref(),
        Some(1),
    )
    .await
    {
        Ok(dept) => Ok(Json(ApiResponse::success(dept))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 删除部门
pub async fn delete_department(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的部门ID"))?;

    match assets_service::department_service::delete_department(id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}
