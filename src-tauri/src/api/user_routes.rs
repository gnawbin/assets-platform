//! 用户 HTTP API 路由
//!
//! 提供用户的 RESTful 接口。

use axum::{extract::Path, Extension, Json};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::service;
use crate::service::user_service::UserResponse;

use super::auth;
use super::response::{ApiError, ApiResponse};

/// 创建用户请求
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub real_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub department_id: Option<i64>,
    pub role_id: Option<i64>,
    pub nickname: Option<String>,
    pub person_id: Option<String>,
    pub person_code: Option<String>,
    pub super_user_id: Option<i64>,
}

/// 更新用户请求
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateUserRequest {
    pub real_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub department_id: Option<i64>,
    pub role_id: Option<i64>,
    pub nickname: Option<String>,
    pub person_id: Option<String>,
    pub person_code: Option<String>,
    pub super_user_id: Option<i64>,
}

/// 重置密码请求
#[derive(Debug, Deserialize, ToSchema)]
pub struct ResetPasswordRequest {
    pub new_password: String,
}

/// 登录请求
#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// 登录响应
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct LoginResponse {
    pub token: String,
    pub user_id: String,
    pub username: String,
    pub real_name: String,
}

/// 用户登录
#[utoipa::path(
    post,
    path = "/api/auth/login",
    tag = "认证",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "登录成功", body = ApiResponse<LoginResponse>),
        (status = 401, description = "认证失败", body = ApiError),
    ),
)]
pub async fn login(
    Json(req): Json<LoginRequest>,
) -> Result<Json<ApiResponse<LoginResponse>>, ApiError> {
    match service::user_service::login(&req.username, &req.password).await {
        Ok(resp) => {
            let login_resp = LoginResponse {
                token: resp.token.clone(),
                user_id: resp.id.to_string(),
                username: resp.username.clone(),
                real_name: resp.real_name.clone(),
            };
            Ok(Json(ApiResponse::success(login_resp)))
        }
        Err(e) => Err(ApiError::unauthorized(e)),
    }
}

/// 获取所有用户
#[utoipa::path(
    get,
    path = "/api/users",
    tag = "用户管理",
    responses(
        (status = 200, description = "获取成功", body = ApiResponse<Vec<UserResponse>>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_users() -> Result<Json<ApiResponse<Vec<UserResponse>>>, ApiError> {
    match service::user_service::get_users().await {
        Ok(users) => Ok(Json(ApiResponse::success(users))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增用户
#[utoipa::path(
    post,
    path = "/api/users",
    tag = "用户管理",
    request_body = CreateUserRequest,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<UserResponse>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn insert_user(
    Extension(claims): Extension<auth::Claims>,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<ApiResponse<UserResponse>>, ApiError> {
    match service::user_service::insert_user(
        &req.username,
        &req.password,
        &req.real_name,
        req.email.as_deref(),
        req.phone.as_deref(),
        req.department_id,
        1, // status: 启用
        req.nickname.as_deref(),
        req.person_id.as_deref(),
        req.person_code.as_deref(),
        req.super_user_id,
        Some(claims.sub), // created_by 从 JWT 中提取
    )
    .await
    {
        Ok(user) => Ok(Json(ApiResponse::success(user))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 更新用户
#[utoipa::path(
    put,
    path = "/api/users/{id}",
    tag = "用户管理",
    request_body = UpdateUserRequest,
    responses(
        (status = 200, description = "更新成功", body = ApiResponse<UserResponse>),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn update_user(
    Extension(claims): Extension<auth::Claims>,
    Path(id): Path<String>,
    Json(req): Json<UpdateUserRequest>,
) -> Result<Json<ApiResponse<UserResponse>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的用户ID"))?;

    match service::user_service::update_user(
        id,
        &req.real_name, // username 复用 real_name
        &req.real_name,
        req.email.as_deref(),
        req.phone.as_deref(),
        req.department_id,
        1, // status
        req.nickname.as_deref(),
        req.person_id.as_deref(),
        req.person_code.as_deref(),
        req.super_user_id,
        Some(claims.sub), // updated_by 从 JWT 中提取
    )
    .await
    {
        Ok(user) => Ok(Json(ApiResponse::success(user))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 重置密码
#[utoipa::path(
    post,
    path = "/api/users/{id}/reset-password",
    tag = "用户管理",
    request_body = ResetPasswordRequest,
    responses(
        (status = 200, description = "重置成功"),
        (status = 500, description = "服务器错误", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn reset_password(
    Path(id): Path<String>,
    Json(req): Json<ResetPasswordRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的用户ID"))?;

    match service::user_service::reset_password(id, &req.new_password).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 删除用户
#[utoipa::path(
    delete,
    path = "/api/users/{id}",
    tag = "用户管理",
    responses(
        (status = 200, description = "删除成功"),
        (status = 500, description = "服务器错误"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn delete_user(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的用户ID"))?;

    match service::user_service::delete_user(id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 获取当前登录用户信息
#[utoipa::path(
    get,
    path = "/api/users/me",
    tag = "用户管理",
    responses(
        (status = 200, description = "获取成功", body = ApiResponse<UserResponse>),
        (status = 401, description = "认证失败", body = ApiError),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_current_user(
    Extension(claims): Extension<auth::Claims>,
) -> Result<Json<ApiResponse<UserResponse>>, ApiError> {
    match service::user_service::get_user_by_id(claims.sub).await {
        Ok(user) => Ok(Json(ApiResponse::success(user))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}
