//! 部门 HTTP API 路由
//!
//! 提供部门的 RESTful 接口。

use database::models::Department;
use service;
use axum::{extract::Path, extract::Query, Json};
use serde::Deserialize;

use super::response::{ApiError, ApiResponse};

/// 查询参数
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct DepartmentQuery {
    /// 租户 ID（雪花 ID 字符串），缺省使用当前登录用户的租户
    pub tenant_id: Option<String>,
}

/// 创建部门请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateDepartmentRequest {
    /// 部门名称
    pub department_name: String,
    /// 上级部门 ID（字符串或数字均可）
    #[serde(deserialize_with = "database::models::opt_i64_from_string")]
    pub parent_id: Option<i64>,
    pub description: Option<String>,
    /// 所属租户 ID（字符串或数字均可）
    #[serde(deserialize_with = "database::models::i64_from_string")]
    pub tenant_id: i64,
}

/// 更新部门请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateDepartmentRequest {
    pub department_name: String,
    #[serde(deserialize_with = "database::models::opt_i64_from_string")]
    pub parent_id: Option<i64>,
    pub description: Option<String>,
}

/// 获取所有部门
#[utoipa::path(
    get,
    path = "/api/departments",
    tag = "部门",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "部门列表", body = ApiResponse<Vec<database::models::Department>>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
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
    match service::department_service::get_departments(tenant_id).await {
        Ok(departments) => Ok(Json(ApiResponse::success(departments))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增部门
#[utoipa::path(
    post,
    path = "/api/departments",
    tag = "部门",
    security(("bearer_auth" = [])),
    request_body = CreateDepartmentRequest,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<database::models::Department>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn insert_department(
    Json(req): Json<CreateDepartmentRequest>,
) -> Result<Json<ApiResponse<Department>>, ApiError> {
    match service::department_service::insert_department(
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
#[utoipa::path(
    put,
    path = "/api/departments/{id}",
    tag = "部门",
    security(("bearer_auth" = [])),
    request_body = UpdateDepartmentRequest,
    responses(
        (status = 200, description = "更新成功", body = ApiResponse<database::models::Department>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn update_department(
    Path(id): Path<String>,
    Json(req): Json<UpdateDepartmentRequest>,
) -> Result<Json<ApiResponse<Department>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的部门ID"))?;

    match service::department_service::update_department(
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
#[utoipa::path(
    delete,
    path = "/api/departments/{id}",
    tag = "部门",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "删除成功（`data` 为 null）"),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
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
