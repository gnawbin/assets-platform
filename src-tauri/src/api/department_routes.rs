//! 部门 HTTP API 路由
//!
//! 提供部门的 RESTful 接口。

use axum::{extract::Path, Json};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::database::models::Department;
use crate::service;

use super::response::{ApiError, ApiResponse};

/// 创建部门请求
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateDepartmentRequest {
    pub department_name: String,
    pub parent_id: Option<String>,
    pub description: Option<String>,
}

/// 更新部门请求
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateDepartmentRequest {
    pub department_name: String,
    pub parent_id: Option<String>,
    pub description: Option<String>,
}

/// 获取所有部门
#[utoipa::path(
    get,
    path = "/api/departments",
    tag = "部门管理",
    responses(
        (status = 200, description = "获取成功", body = ApiResponse<Vec<Department>>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_departments() -> Result<Json<ApiResponse<Vec<Department>>>, ApiError> {
    match service::department_service::get_departments().await {
        Ok(departments) => Ok(Json(ApiResponse::success(departments))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增部门
#[utoipa::path(
    post,
    path = "/api/departments",
    tag = "部门管理",
    request_body = CreateDepartmentRequest,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<Department>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn insert_department(
    Json(req): Json<CreateDepartmentRequest>,
) -> Result<Json<ApiResponse<Department>>, ApiError> {
    let parent_id: Option<i64> = req
        .parent_id
        .map(|id| id.parse::<i64>())
        .transpose()
        .map_err(|_| ApiError::bad_request("无效的父部门ID"))?;

    match service::department_service::insert_department(
        &req.department_name,
        parent_id,
        req.description.as_deref(),
        Some(1),
    )
    .await
    {
        Ok(dept) => Ok(Json(ApiResponse::success(dept))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 更新部门
#[utoipa::path(
    put,
    path = "/api/departments/{id}",
    tag = "部门管理",
    request_body = UpdateDepartmentRequest,
    responses(
        (status = 200, description = "更新成功", body = ApiResponse<Department>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn update_department(
    Path(id): Path<String>,
    Json(req): Json<UpdateDepartmentRequest>,
) -> Result<Json<ApiResponse<Department>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的部门ID"))?;

    let parent_id: Option<i64> = req
        .parent_id
        .map(|id| id.parse::<i64>())
        .transpose()
        .map_err(|_| ApiError::bad_request("无效的父部门ID"))?;

    match service::department_service::update_department(
        id,
        &req.department_name,
        parent_id,
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
#[utoipa::path(
    delete,
    path = "/api/departments/{id}",
    tag = "部门管理",
    responses(
        (status = 200, description = "删除成功"),
        (status = 500, description = "服务器错误"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn delete_department(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的部门ID"))?;

    match service::department_service::delete_department(id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}
