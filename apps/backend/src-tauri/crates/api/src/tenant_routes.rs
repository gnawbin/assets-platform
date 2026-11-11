//! 租户 HTTP API 路由
//!
//! 提供租户的 RESTful 接口。

use axum::{extract::Path, Extension, Json};
use serde::Deserialize;

use database::models::TenantInfo;
use service;
use service::tenant_service::TenantResponse;

use super::auth;
use super::response::{ApiError, ApiResponse};

/// 创建租户请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateTenantRequest {
    /// 租户名称
    pub tenant_name: String,
    /// 父租户 ID（雪花 ID 字符串）
    pub parent_id: Option<String>,
    /// 是否叶子租户
    pub is_leaf: bool,
    /// 独立 schema 名称（缺省自动生成）
    pub schema_name: Option<String>,
    /// 是否启用
    pub enable: bool,
}

/// 更新租户请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateTenantRequest {
    pub tenant_name: String,
    pub enable: bool,
}

/// 获取所有租户
#[utoipa::path(
    get,
    path = "/api/tenants",
    tag = "租户",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "租户列表", body = ApiResponse<Vec<service::tenant_service::TenantResponse>>),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
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
    tag = "租户",
    security(("bearer_auth" = [])),
    request_body = CreateTenantRequest,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<service::tenant_service::TenantResponse>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
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
    tag = "租户",
    security(("bearer_auth" = [])),
    request_body = UpdateTenantRequest,
    responses(
        (status = 200, description = "更新成功", body = ApiResponse<service::tenant_service::TenantResponse>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
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
    tag = "租户",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "已禁用（`data` 为 null）"),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
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

/// 切换租户请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SwitchTenantRequest {
    /// 目标租户 ID（雪花 ID 字符串）
    pub tenant_id: String,
}

/// 切换租户
#[utoipa::path(
    post,
    path = "/api/tenants/switch",
    tag = "租户",
    security(("bearer_auth" = [])),
    request_body = SwitchTenantRequest,
    responses(
        (status = 200, description = "切换成功，返回当前租户信息", body = ApiResponse<database::models::TenantInfo>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn switch_tenant(
    Extension(ctx): Extension<auth::UserContext>,
    Json(req): Json<SwitchTenantRequest>,
) -> Result<Json<ApiResponse<TenantInfo>>, ApiError> {
    let tenant_id: i64 = req
        .tenant_id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的租户ID"))?;

    // 同 `login`：`tenant_service::switch_tenant` 会写 `GLOBAL_SCHEMA`（Tauri 专用），
    // HTTP 侧调用后立即恢复快照，避免污染桌面会话（设计文档 §16 T-3）。
    let schema_snapshot = database::global_schema_name();
    let result = service::tenant_service::switch_tenant(ctx.user_id, tenant_id).await;
    database::set_global_schema(schema_snapshot);

    match result {
        Ok(info) => Ok(Json(ApiResponse::success(info))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 分配租户请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AssignTenantsRequest {
    /// 用户 ID（雪花 ID 字符串）
    pub user_id: String,
    /// 可访问的租户 ID 列表（雪花 ID 字符串）
    pub tenant_ids: Vec<String>,
}

/// 为用户分配租户（覆盖式）
#[utoipa::path(
    post,
    path = "/api/tenants/assign",
    tag = "租户",
    security(("bearer_auth" = [])),
    request_body = AssignTenantsRequest,
    responses(
        (status = 200, description = "分配成功（`data` 为 null）"),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
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
#[utoipa::path(
    get,
    path = "/api/users/{user_id}/tenants",
    tag = "租户",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "用户可访问的租户", body = ApiResponse<Vec<database::models::TenantInfo>>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
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
