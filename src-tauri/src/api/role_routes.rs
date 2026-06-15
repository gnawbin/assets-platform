//! 角色 HTTP API 路由
//!
//! 提供角色的 RESTful 接口。

use axum::{extract::Path, Json};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::database::models::{MantineTree, Role, SidebarMenuItem};
use crate::service;

use super::response::{ApiError, ApiResponse};

/// 创建角色请求
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateRoleRequest {
    pub role_name: String,
    pub role_key: String,
    pub description: Option<String>,
}

/// 更新角色请求
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateRoleRequest {
    pub role_name: String,
    pub role_key: String,
    pub description: Option<String>,
}

// ======================== 角色 ========================

/// 获取所有角色
#[utoipa::path(
    get,
    path = "/api/roles",
    tag = "角色管理",
    responses(
        (status = 200, description = "获取成功", body = ApiResponse<Vec<Role>>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_roles() -> Result<Json<ApiResponse<Vec<Role>>>, ApiError> {
    match service::role_service::get_roles().await {
        Ok(roles) => Ok(Json(ApiResponse::success(roles))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增角色
#[utoipa::path(
    post,
    path = "/api/roles",
    tag = "角色管理",
    request_body = CreateRoleRequest,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<Role>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn insert_role(
    Json(req): Json<CreateRoleRequest>,
) -> Result<Json<ApiResponse<Role>>, ApiError> {
    // 通过 service 层创建角色，路由层不直接构造 Role 对象
    match service::role_service::insert_role_by_params(
        &req.role_key,
        &req.role_name,
        req.description.as_deref(),
        Some(1), // created_by
    )
    .await
    {
        Ok(role) => Ok(Json(ApiResponse::success(role))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 删除角色
#[utoipa::path(
    delete,
    path = "/api/roles/{id}",
    tag = "角色管理",
    responses(
        (status = 200, description = "删除成功"),
        (status = 500, description = "服务器错误"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn delete_role(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的角色ID"))?;

    match service::role_service::delete_role(id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

// ======================== 用户角色关联 ========================

/// 分配用户角色请求
#[derive(Debug, Deserialize, ToSchema)]
pub struct AssignUserRolesRequest {
    pub role_ids: Vec<i64>,
}

/// 获取用户已分配的角色 ID 列表
#[utoipa::path(
    get,
    path = "/api/users/{id}/roles",
    tag = "角色管理",
    responses(
        (status = 200, description = "获取成功", body = ApiResponse<Vec<i64>>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_user_role_ids(
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<Vec<i64>>>, ApiError> {
    let user_id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的用户ID"))?;

    match service::role_service::get_user_role_ids(user_id).await {
        Ok(role_ids) => Ok(Json(ApiResponse::success(role_ids))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 为用户分配角色
#[utoipa::path(
    post,
    path = "/api/users/{id}/roles",
    tag = "角色管理",
    request_body = AssignUserRolesRequest,
    responses(
        (status = 200, description = "分配成功"),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn assign_user_roles(
    Path(id): Path<String>,
    Json(req): Json<AssignUserRolesRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let user_id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的用户ID"))?;

    match service::role_service::assign_user_roles(user_id, req.role_ids).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

// ======================== 角色菜单关联 ========================

/// 分配角色菜单请求
#[derive(Debug, Deserialize, ToSchema)]
pub struct AssignRoleMenusRequest {
    pub menu_ids: Vec<i64>,
}

/// 获取角色已分配的菜单 ID 列表
#[utoipa::path(
    get,
    path = "/api/roles/{id}/menus",
    tag = "角色管理",
    responses(
        (status = 200, description = "获取成功", body = ApiResponse<Vec<i64>>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_role_menu_ids(
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<Vec<i64>>>, ApiError> {
    let role_id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的角色ID"))?;

    match service::role_service::get_role_menu_ids(role_id).await {
        Ok(menu_ids) => Ok(Json(ApiResponse::success(menu_ids))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 为角色分配菜单权限
#[utoipa::path(
    post,
    path = "/api/roles/{id}/menus",
    tag = "角色管理",
    request_body = AssignRoleMenusRequest,
    responses(
        (status = 200, description = "分配成功"),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn assign_role_menus(
    Path(id): Path<String>,
    Json(req): Json<AssignRoleMenusRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let role_id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的角色ID"))?;

    match service::role_service::assign_role_menus(role_id, req.menu_ids).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

// ======================== 菜单 ========================

/// 获取所有菜单树（用于权限分配）
#[utoipa::path(
    get,
    path = "/api/menus/tree",
    tag = "菜单管理",
    responses(
        (status = 200, description = "获取成功"),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_all_menus_tree() -> Result<Json<ApiResponse<Vec<MantineTree>>>, ApiError> {
    match service::role_service::get_all_menus_tree().await {
        Ok(menus) => Ok(Json(ApiResponse::success(menus))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 获取侧边栏菜单（只返回目录和菜单，不返回按钮）
#[utoipa::path(
    get,
    path = "/api/menus/user",
    tag = "菜单管理",
    responses(
        (status = 200, description = "获取成功"),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_user_menus() -> Result<Json<ApiResponse<Vec<SidebarMenuItem>>>, ApiError> {
    match service::role_service::get_user_menus().await {
        Ok(menus) => Ok(Json(ApiResponse::success(menus))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}
