//! 角色 HTTP API 路由
//!
//! 提供角色的 RESTful 接口。

use axum::{extract::Path, Json};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::database::models::{MantineTree, Role, SidebarMenuItem};
use crate::service;
use crate::utils::snowflake::next_id;

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
    let role = Role {
        id: next_id() as i64,
        role_key: req.role_key,
        role_name: req.role_name,
        description: req.description,
        created_by: Some(1),
        created_at: None,
        updated_by: Some(1),
        updated_at: None,
        deleted: Some(0),
    };

    match service::role_service::insert_role(&role).await {
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
