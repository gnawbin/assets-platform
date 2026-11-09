//! 用户 HTTP API 路由
//!
//! 提供用户的 RESTful 接口。

use axum::extract::Query;
use axum::{extract::Path, Extension, Json};
use serde::Deserialize;
use std::collections::HashMap;

use service;
use service::user_service::UserResponse;

use super::auth;
use super::response::{ApiError, ApiResponse};

/// 创建用户请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateUserRequest {
    /// 登录名（唯一）
    pub username: String,
    /// 初始密码
    pub password: String,
    /// 真实姓名
    pub real_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    #[serde(deserialize_with = "database::models::opt_i64_from_string")]
    pub department_id: Option<i64>,
    #[serde(deserialize_with = "database::models::opt_i64_from_string")]
    pub role_id: Option<i64>,
    pub nickname: Option<String>,
    pub person_id: Option<String>,
    pub person_code: Option<String>,
    #[serde(deserialize_with = "database::models::opt_i64_from_string")]
    pub super_user_id: Option<i64>,
    #[serde(deserialize_with = "database::models::opt_i64_from_string")]
    pub tenant_id: Option<i64>,
}

/// 更新用户请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateUserRequest {
    /// 真实姓名
    pub real_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    #[serde(deserialize_with = "database::models::opt_i64_from_string")]
    pub department_id: Option<i64>,
    #[serde(deserialize_with = "database::models::opt_i64_from_string")]
    pub role_id: Option<i64>,
    pub nickname: Option<String>,
    pub person_id: Option<String>,
    pub person_code: Option<String>,
    #[serde(deserialize_with = "database::models::opt_i64_from_string")]
    pub super_user_id: Option<i64>,
}

/// 重置密码请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ResetPasswordRequest {
    /// 新密码
    pub new_password: String,
}

/// 登录请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct LoginRequest {
    /// 登录名
    pub username: String,
    /// 密码
    pub password: String,
}

/// 登录响应
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct LoginResponse {
    /// JWT，后续请求放入 `Authorization: Bearer <token>`
    pub token: String,
    /// 刷新令牌，仅用于 `POST /api/auth/refresh`（每次刷新都会轮换，旧令牌立即失效）
    pub refresh_token: String,
    /// 访问令牌有效期（秒）
    pub expires_in: i64,
    /// 用户 ID（雪花 ID 字符串）
    pub user_id: String,
    pub username: String,
    pub real_name: String,
    /// 是否超级管理员
    pub is_super_admin: bool,
}

/// 用户登录
#[utoipa::path(
    post,
    path = "/api/auth/login",
    tag = "认证",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "登录成功", body = ApiResponse<LoginResponse>),
        (status = 401, description = "用户名或密码错误", body = ApiError),
    )
)]
pub async fn login(
    Json(req): Json<LoginRequest>,
) -> Result<Json<ApiResponse<LoginResponse>>, ApiError> {
    match service::user_service::login(&req.username, &req.password).await {
        Ok(resp) => {
            let login_resp = LoginResponse {
                token: resp.token.clone(),
                refresh_token: resp.refresh_token.clone(),
                expires_in: resp.expires_in,
                user_id: resp.id.to_string(),
                username: resp.username.clone(),
                real_name: resp.real_name.clone(),
                is_super_admin: resp.is_super_admin,
            };
            Ok(Json(ApiResponse::success(login_resp)))
        }
        Err(e) => Err(ApiError::unauthorized(e)),
    }
}

/// 刷新令牌请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RefreshTokenRequest {
    /// 登录（或上一次刷新）返回的 `refresh_token`
    pub refresh_token: String,
}

/// 令牌对响应
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct TokenPairResponse {
    /// 新的访问令牌，放入 `Authorization: Bearer <token>`
    pub token: String,
    /// 新的刷新令牌（旧 refresh 立即失效，请替换本地保存）
    pub refresh_token: String,
    /// 访问令牌有效期（秒）
    pub expires_in: i64,
}

/// 刷新访问令牌
///
/// 公开接口：访问令牌已过期时也必须能调用，因此不要求 `Authorization` 头。
/// 刷新成功后旧 refresh token 立即失效（轮换）；若旧 token 被重复使用，
/// 该次登录派生出的整条令牌链都会被吊销，需要重新登录。
#[utoipa::path(
    post,
    path = "/api/auth/refresh",
    tag = "认证",
    request_body = RefreshTokenRequest,
    responses(
        (status = 200, description = "刷新成功，返回新的 access / refresh 令牌（旧 refresh 立即失效）", body = ApiResponse<TokenPairResponse>),
        (status = 400, description = "请求体格式错误", body = ApiError),
        (status = 401, description = "refresh token 无效 / 已撤销 / 已过期 / 被重放 / 账号已被禁用", body = ApiError),
    )
)]
pub async fn refresh(
    Json(req): Json<RefreshTokenRequest>,
) -> Result<Json<ApiResponse<TokenPairResponse>>, ApiError> {
    match service::user_service::refresh_tokens(&req.refresh_token).await {
        Ok(pair) => Ok(Json(ApiResponse::success(TokenPairResponse {
            token: pair.token,
            refresh_token: pair.refresh_token,
            expires_in: pair.expires_in,
        }))),
        Err(e) => Err(ApiError::unauthorized(e)),
    }
}

/// 登出（撤销刷新令牌）
///
/// 公开接口且幂等：token 非法也返回成功，避免登出流程被卡住。
#[utoipa::path(
    post,
    path = "/api/auth/logout",
    tag = "认证",
    request_body = RefreshTokenRequest,
    responses(
        (status = 200, description = "已登出，refresh token 立即失效（`data` 为 null）"),
    )
)]
pub async fn logout(
    Json(req): Json<RefreshTokenRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    service::user_service::revoke_refresh_token(&req.refresh_token)
        .await
        .map_err(ApiError::internal_error)?;
    Ok(Json(ApiResponse::success(())))
}

/// 获取所有用户
#[utoipa::path(
    get,
    path = "/api/users",
    tag = "用户",
    security(("bearer_auth" = [])),
    params(
        ("tenant_id" = Option<String>, Query, description = "按租户过滤（雪花 ID 字符串）"),
        ("keyword" = Option<String>, Query, description = "按登录名 / 姓名模糊搜索"),
    ),
    responses(
        (status = 200, description = "用户列表", body = ApiResponse<Vec<service::user_service::UserResponse>>),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn get_users(
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<ApiResponse<Vec<UserResponse>>>, ApiError> {
    let tenant_id = params.get("tenant_id").and_then(|v| v.parse::<i64>().ok());
    let keyword = params.get("keyword").cloned().filter(|k| !k.is_empty());
    match service::user_service::get_users(tenant_id, keyword).await {
        Ok(users) => Ok(Json(ApiResponse::success(users))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 新增用户
#[utoipa::path(
    post,
    path = "/api/users",
    tag = "用户",
    security(("bearer_auth" = [])),
    request_body = CreateUserRequest,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<service::user_service::UserResponse>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
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
        req.tenant_id,
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
    tag = "用户",
    security(("bearer_auth" = [])),
    request_body = UpdateUserRequest,
    responses(
        (status = 200, description = "更新成功", body = ApiResponse<service::user_service::UserResponse>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
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
    tag = "用户",
    security(("bearer_auth" = [])),
    request_body = ResetPasswordRequest,
    responses(
        (status = 200, description = "重置成功（`data` 为 null）"),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
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
    tag = "用户",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "删除成功（`data` 为 null）"),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn delete_user(
    Extension(claims): Extension<auth::Claims>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的用户ID"))?;

    // 从 JWT claims 中获取当前用户信息
    let current_user_id = claims.sub;
    // 从 claims 中无法直接获取 is_super_admin，需要查询数据库
    // 通过 get_user_by_id 获取当前用户的完整信息
    match service::user_service::get_user_by_id(current_user_id).await {
        Ok(current_user) => {
            let is_super_admin = current_user.is_super_admin;
            match service::user_service::delete_user(id, current_user_id, is_super_admin).await {
                Ok(_) => Ok(Json(ApiResponse::success(()))),
                Err(e) => Err(ApiError::internal_error(e)),
            }
        }
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 获取当前登录用户信息
#[utoipa::path(
    get,
    path = "/api/users/me",
    tag = "用户",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "当前用户信息", body = ApiResponse<service::user_service::UserResponse>),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
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
