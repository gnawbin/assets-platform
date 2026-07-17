//! 注册申请 HTTP API 路由
//!
//! 提供用户注册申请、审核、驳回等 RESTful 接口。

use axum::extract::{Path, Query};
use axum::{Extension, Json};
use serde::Deserialize;

use crate::service;
use crate::service::register_service::RegisterResponse;

use super::auth;
use super::response::{ApiError, ApiResponse};

/// 注册申请请求
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    pub real_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub department_name: Option<String>,
    pub company_name: Option<String>,
    pub reason: Option<String>,
}

/// 审核请求
#[derive(Debug, Deserialize)]
pub struct ApproveRequest {
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub tenant_id: i64,
    pub approve_remark: Option<String>,
}

/// 驳回请求
#[derive(Debug, Deserialize)]
pub struct RejectRequest {
    pub approve_remark: Option<String>,
}

/// 注册申请列表查询参数
#[derive(Debug, Deserialize)]
pub struct RegistrationQuery {
    pub status: Option<i16>,
}

/// 用户注册申请（公开接口，无需认证）
pub async fn register(
    Json(req): Json<RegisterRequest>,
) -> Result<Json<ApiResponse<RegisterResponse>>, ApiError> {
    match service::register_service::register(
        &req.username,
        &req.password,
        &req.real_name,
        req.email.as_deref(),
        req.phone.as_deref(),
        req.department_name.as_deref(),
        req.company_name.as_deref(),
        req.reason.as_deref(),
    )
    .await
    {
        Ok(resp) => Ok(Json(ApiResponse::success(resp))),
        Err(e) => Err(ApiError::bad_request(e)),
    }
}

/// 获取注册申请列表
pub async fn get_registrations(
    Query(query): Query<RegistrationQuery>,
) -> Result<Json<ApiResponse<Vec<RegisterResponse>>>, ApiError> {
    match service::register_service::get_registrations(query.status).await {
        Ok(list) => Ok(Json(ApiResponse::success(list))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 审核通过注册申请
pub async fn approve_registration(
    Extension(claims): Extension<auth::Claims>,
    Path(id): Path<String>,
    Json(req): Json<ApproveRequest>,
) -> Result<Json<ApiResponse<RegisterResponse>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的申请ID"))?;

    match service::register_service::approve_registration(
        id,
        claims.sub,
        req.tenant_id,
        req.approve_remark.as_deref(),
    )
    .await
    {
        Ok(resp) => Ok(Json(ApiResponse::success(resp))),
        Err(e) => Err(ApiError::bad_request(e)),
    }
}

/// 驳回注册申请
pub async fn reject_registration(
    Extension(claims): Extension<auth::Claims>,
    Path(id): Path<String>,
    Json(req): Json<RejectRequest>,
) -> Result<Json<ApiResponse<RegisterResponse>>, ApiError> {
    let id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的申请ID"))?;

    match service::register_service::reject_registration(
        id,
        claims.sub,
        req.approve_remark.as_deref(),
    )
    .await
    {
        Ok(resp) => Ok(Json(ApiResponse::success(resp))),
        Err(e) => Err(ApiError::bad_request(e)),
    }
}
