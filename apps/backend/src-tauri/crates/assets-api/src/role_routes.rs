//! 角色 HTTP API 路由
//!
//! 提供角色的 RESTful 接口。

use axum::{extract::Path, Extension, Json};
use serde::Deserialize;

use assets_database::models::{MantineTree, Role, SidebarMenuItem};
use assets_service;

use super::auth;
use super::response::{ApiError, ApiResponse};

/// 创建角色请求
#[derive(Debug, Deserialize)]
pub struct CreateRoleRequest {
    pub role_name: String,
    pub role_key: String,
    pub description: Option<String>,
    pub is_super_admin: Option<bool>,
    pub tenant_id: Option<String>,
}

/// 更新角色请求
#[derive(Debug, Deserialize)]
pub struct UpdateRoleRequest {
    pub role_name: String,
    pub role_key: String,
    pub description: Option<String>,
}

/// 角色列表查询参数
#[derive(Debug, Deserialize)]
pub struct RoleQueryParams {
    pub tenant_id: Option<String>,
    pub keyword: Option<String>,
}

// ======================== 角色 ========================

/// 获取所有角色
pub async fn get_roles(
    axum::extract::Query(params): axum::extract::Query<RoleQueryParams>,
) -> Result<Json<ApiResponse<Vec<Role>>>, ApiError> {
    let tenant_id = match params.tenant_id {
        Some(ref id) if !id.is_empty() => Some(
            id.parse::<i64>()
                .map_err(|_| ApiError::bad_request("无效的租户ID"))?,
        ),
        _ => None,
    };
    match assets_service::role_service::get_roles(tenant_id, params.keyword).await {
        Ok(roles) => Ok(Json(ApiResponse::success(roles))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增角色
pub async fn insert_role(
    Json(req): Json<CreateRoleRequest>,
) -> Result<Json<ApiResponse<Role>>, ApiError> {
    let tenant_id = match req.tenant_id {
        Some(ref id) if !id.is_empty() => Some(
            id.parse::<i64>()
                .map_err(|_| ApiError::bad_request("无效的租户ID"))?,
        ),
        _ => None,
    };
    match assets_service::role_service::insert_role_by_params(
        &req.role_key,
        &req.role_name,
        req.description.as_deref(),
        req.is_super_admin.unwrap_or(false),
        tenant_id,
        Some(1), // created_by
    )
    .await
    {
        Ok(role) => Ok(Json(ApiResponse::success(role))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 删除角色
pub async fn delete_role(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的角色ID"))?;

    match assets_service::role_service::delete_role(id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

// ======================== 用户角色关联 ========================

/// 分配用户角色请求
#[derive(Debug, Deserialize)]
pub struct AssignUserRolesRequest {
    #[serde(deserialize_with = "assets_database::models::vec_i64_from_string")]
    pub role_ids: Vec<i64>,
}

/// 获取用户已分配的角色 ID 列表
pub async fn get_user_role_ids(
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<Vec<i64>>>, ApiError> {
    let user_id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的用户ID"))?;

    match assets_service::role_service::get_user_role_ids(user_id).await {
        Ok(role_ids) => Ok(Json(ApiResponse::success(role_ids))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 为用户分配角色
pub async fn assign_user_roles(
    Path(id): Path<String>,
    Json(req): Json<AssignUserRolesRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let user_id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的用户ID"))?;

    match assets_service::role_service::assign_user_roles(user_id, req.role_ids).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

// ======================== 角色菜单关联 ========================

/// 分配角色菜单请求
#[derive(Debug, Deserialize)]
pub struct AssignRoleMenusRequest {
    #[serde(deserialize_with = "assets_database::models::vec_i64_from_string")]
    pub menu_ids: Vec<i64>,
}

/// 获取角色已分配的菜单 ID 列表
pub async fn get_role_menu_ids(
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<Vec<i64>>>, ApiError> {
    let role_id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的角色ID"))?;

    match assets_service::role_service::get_role_menu_ids(role_id).await {
        Ok(menu_ids) => Ok(Json(ApiResponse::success(menu_ids))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 为角色分配菜单权限
pub async fn assign_role_menus(
    Path(id): Path<String>,
    Json(req): Json<AssignRoleMenusRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let role_id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的角色ID"))?;

    match assets_service::role_service::assign_role_menus(role_id, req.menu_ids).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

// ======================== 菜单 ========================

/// 获取所有菜单树（用于权限分配）
pub async fn get_all_menus_tree() -> Result<Json<ApiResponse<Vec<MantineTree>>>, ApiError> {
    match assets_service::role_service::get_all_menus_tree().await {
        Ok(menus) => Ok(Json(ApiResponse::success(menus))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 获取侧边栏菜单（只返回目录和菜单，不返回按钮）
///
/// 根据当前登录用户的角色过滤菜单：
/// - 超级管理员：返回所有可见菜单
/// - 普通用户：只返回其角色已分配的菜单
pub async fn get_user_menus(
    Extension(claims): Extension<auth::Claims>,
) -> Result<Json<ApiResponse<Vec<SidebarMenuItem>>>, ApiError> {
    match assets_service::role_service::get_user_menus(claims.sub).await {
        Ok(menus) => Ok(Json(ApiResponse::success(menus))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}
