//! 租户 HTTP API 路由
//!
//! 提供租户的 RESTful 接口。

use axum::{extract::Path, Extension, Json};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::service;
use crate::service::tenant_service::TenantResponse;

use super::auth;
use super::response::{ApiError, ApiResponse};

/// 创建租户请求
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateTenantRequest {
    pub tenant_name: String,
    pub parent_id: Option<String>,
    pub is_leaf: bool,
    pub schema_name: Option<String>,
    pub enable: bool,
}

/// 更新租户请求
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateTenantRequest {
    pub tenant_name: String,
    pub enable: bool,
}

/// 获取所有租户
#[utoipa::path(
    get,
    path = "/api/tenants",
    tag = "租户管理",
    responses(
        (status = 200, description = "获取成功", body = ApiResponse<Vec<TenantResponse>>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_tenants() -> Result<Json<ApiResponse<Vec<TenantResponse>>>, ApiError> {
    match service::tenant_service::get_tenants().await {
        Ok(tenants) => Ok(Json(ApiResponse::success(tenants))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增租户
#[utoipa::path(
    post,
    path = "/api/tenants",
    tag = "租户管理",
    request_body = CreateTenantRequest,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<TenantResponse>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn insert_tenant(
    Extension(claims): Extension<auth::Claims>,
    Json(req): Json<CreateTenantRequest>,
) -> Result<Json<ApiResponse<TenantResponse>>, ApiError> {
    let parent_id: Option<i64> = match &req.parent_id {
        Some(s) if !s.is_empty() => Some(
            s.parse()
                .map_err(|_| ApiError::bad_request("无效的父租户ID"))?,
        ),
        _ => None,
    };
    match service::tenant_service::insert_tenant(
        &req.tenant_name,
        parent_id,
        req.is_leaf,
        req.schema_name.as_deref(),
        req.enable,
        Some(claims.sub),
    )
    .await
    {
        Ok(tenant) => Ok(Json(ApiResponse::success(tenant))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 更新租户
#[utoipa::path(
    put,
    path = "/api/tenants/{id}",
    tag = "租户管理",
    request_body = UpdateTenantRequest,
    responses(
        (status = 200, description = "更新成功", body = ApiResponse<TenantResponse>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn update_tenant(
    Path(id): Path<String>,
    Json(req): Json<UpdateTenantRequest>,
) -> Result<Json<ApiResponse<TenantResponse>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的租户ID"))?;

    match service::tenant_service::update_tenant(id, &req.tenant_name, req.enable).await {
        Ok(tenant) => Ok(Json(ApiResponse::success(tenant))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 删除租户（禁用租户）
#[utoipa::path(
    delete,
    path = "/api/tenants/{id}",
    tag = "租户管理",
    responses(
        (status = 200, description = "删除成功"),
        (status = 500, description = "服务器错误"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn delete_tenant(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的租户ID"))?;

    match service::tenant_service::delete_tenant(id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}
