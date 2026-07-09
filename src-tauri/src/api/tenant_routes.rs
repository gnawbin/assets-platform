//! 租户 HTTP API 路由
//!
//! 提供租户的 RESTful 接口。

use axum::{extract::Path, Extension, Json};
use serde::Deserialize;

use crate::database::models::TenantInfo;
use crate::service;
use crate::service::tenant_service::TenantResponse;

use super::auth;
use super::response::{ApiError, ApiResponse};

/// 创建租户请求
#[derive(Debug, Deserialize)]
pub struct CreateTenantRequest {
    pub tenant_name: String,
    pub parent_id: Option<String>,
    pub is_leaf: bool,
    pub schema_name: Option<String>,
    pub enable: bool,
}

/// 更新租户请求
#[derive(Debug, Deserialize)]
pub struct UpdateTenantRequest {
    pub tenant_name: String,
    pub enable: bool,
}

/// 获取所有租户
pub async fn get_tenants() -> Result<Json<ApiResponse<Vec<TenantResponse>>>, ApiError> {
    match service::tenant_service::get_tenants().await {
        Ok(tenants) => Ok(Json(ApiResponse::success(tenants))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增租户
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
pub async fn delete_tenant(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的租户ID"))?;

    match service::tenant_service::delete_tenant(id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 切换租户请求
#[derive(Debug, Deserialize)]
pub struct SwitchTenantRequest {
    pub tenant_id: String,
}

/// 切换租户
pub async fn switch_tenant(
    Extension(ctx): Extension<auth::UserContext>,
    Json(req): Json<SwitchTenantRequest>,
) -> Result<Json<ApiResponse<TenantInfo>>, ApiError> {
    let tenant_id: i64 = req
        .tenant_id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的租户ID"))?;

    match service::tenant_service::switch_tenant(ctx.user_id, tenant_id).await {
        Ok(info) => Ok(Json(ApiResponse::success(info))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 分配租户请求
#[derive(Debug, Deserialize)]
pub struct AssignTenantsRequest {
    pub user_id: String,
    pub tenant_ids: Vec<String>,
}

/// 为用户分配租户（覆盖式）
pub async fn assign_tenants(
    Extension(ctx): Extension<auth::UserContext>,
    Json(req): Json<AssignTenantsRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let user_id: i64 = req
        .user_id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的用户ID"))?;
    let tenant_ids: Result<Vec<i64>, _> = req
        .tenant_ids
        .iter()
        .map(|s| s.parse().map_err(|_| ApiError::bad_request("无效的租户ID")))
        .collect();
    let tenant_ids = tenant_ids?;

    match service::tenant_service::assign_user_tenants(user_id, &tenant_ids, ctx.user_id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 获取用户可访问的租户列表
pub async fn get_user_tenants(
    Path(user_id): Path<String>,
) -> Result<Json<ApiResponse<Vec<TenantInfo>>>, ApiError> {
    let user_id: i64 = user_id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的用户ID"))?;

    match service::tenant_service::get_user_tenants(user_id).await {
        Ok(tenants) => Ok(Json(ApiResponse::success(tenants))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}
