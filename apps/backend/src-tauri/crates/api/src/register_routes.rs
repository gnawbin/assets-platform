//! 注册申请 HTTP API 路由
//!
//! 提供用户注册申请、审核、驳回等 RESTful 接口。

use axum::extract::{Path, Query};
use axum::{Extension, Json};
use serde::Deserialize;

use service;
use service::register_service::RegisterResponse;

use super::auth;
use super::response::{ApiError, ApiResponse};

/// 注册申请请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RegisterRequest {
    /// 登录名（唯一）
    pub username: String,
    /// 密码
    pub password: String,
    /// 真实姓名
    pub real_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    /// 部门名称（自由填写，审核时人工确认）
    pub department_name: Option<String>,
    /// 公司 / 单位名称
    pub company_name: Option<String>,
    /// 申请理由
    pub reason: Option<String>,
}

/// 审核请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ApproveRequest {
    /// 分配到的租户 ID（雪花 ID，字符串或数字均可）
    #[serde(deserialize_with = "database::models::i64_from_string")]
    pub tenant_id: i64,
    /// 审核备注
    pub approve_remark: Option<String>,
}

/// 驳回请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RejectRequest {
    /// 驳回原因
    pub approve_remark: Option<String>,
}

/// 注册申请列表查询参数
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct RegistrationQuery {
    /// 申请状态（0=待审核 1=通过 2=驳回），缺省返回全部
    pub status: Option<i16>,
}

/// 用户注册申请（公开接口，无需认证）
#[utoipa::path(
    post,
    path = "/api/auth/register",
    tag = "认证",
    request_body = RegisterRequest,
    responses(
        (status = 200, description = "提交成功，等待管理员审核", body = ApiResponse<service::register_service::RegisterResponse>),
        (status = 400, description = "参数错误或登录名已存在", body = ApiError),
    )
)]
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
#[utoipa::path(
    get,
    path = "/api/auth/registrations",
    tag = "注册审核",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "申请列表", body = ApiResponse<Vec<service::register_service::RegisterResponse>>),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn get_registrations(
    Query(query): Query<RegistrationQuery>,
) -> Result<Json<ApiResponse<Vec<RegisterResponse>>>, ApiError> {
    match service::register_service::get_registrations(query.status).await {
        Ok(list) => Ok(Json(ApiResponse::success(list))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 审核通过注册申请
#[utoipa::path(
    post,
    path = "/api/auth/registrations/{id}/approve",
    tag = "注册审核",
    security(("bearer_auth" = [])),
    request_body = ApproveRequest,
    responses(
        (status = 200, description = "审核通过", body = ApiResponse<service::register_service::RegisterResponse>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
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
#[utoipa::path(
    post,
    path = "/api/auth/registrations/{id}/reject",
    tag = "注册审核",
    security(("bearer_auth" = [])),
    request_body = RejectRequest,
    responses(
        (status = 200, description = "已驳回", body = ApiResponse<service::register_service::RegisterResponse>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
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
