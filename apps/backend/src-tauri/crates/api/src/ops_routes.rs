//! 运维采集 HTTP API 路由
//!
//! 全部挂在 `protected_routes`（`auth_middleware` 已提供 JWT + 租户 schema 注入）。
//! 探测失败不返回 5xx：返回 `ApiResponse<OpsHealthView>` 且 `ok=false`，与 `ConnectionHealth`
//! 的设计意图一致；只有配置类错误才映射为 400。

use axum::extract::{Path, Query};
use axum::{Extension, Json};
use serde::Deserialize;

use service::ops_service::{
    self, OpsCollectResult, OpsConnectionInput, OpsConnectionView, OpsHealthView,
    OpsProviderOption, OpsResourceLinkView,
};

use super::auth::UserContext;
use super::response::{ApiError, ApiResponse};

/// 资源清单查询参数（`connectionId` 来自前端 args，兼容下划线写法）
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct LinkQuery {
    /// 连接 ID（雪花 ID 字符串），缺省返回全部连接下的资源
    #[serde(default, rename = "connectionId", alias = "connection_id")]
    pub connection_id: Option<String>,
}

/// 绑定资产的请求体
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct BindRequest {
    /// 目标资产 ID（雪花 ID，字符串或数字均可）
    #[serde(
        alias = "assetId",
        deserialize_with = "database::models::i64_from_string"
    )]
    pub asset_id: i64,
}

fn parse_id(raw: &str) -> Result<i64, ApiError> {
    raw.parse::<i64>()
        .map_err(|_| ApiError::bad_request(format!("无效的ID：{raw}")))
}

/// 来源 / 认证方式下拉数据
#[utoipa::path(
    get,
    path = "/api/ops/providers",
    tag = "运维采集",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "可选来源与认证方式", body = ApiResponse<Vec<service::ops_service::OpsProviderOption>>),
        (status = 401, description = "未认证", body = ApiError),
    )
)]
pub async fn providers() -> Result<Json<ApiResponse<Vec<OpsProviderOption>>>, ApiError> {
    Ok(Json(ApiResponse::success(ops_service::provider_options())))
}

/// 连接列表
#[utoipa::path(
    get,
    path = "/api/ops/connections",
    tag = "运维采集",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "连接列表", body = ApiResponse<Vec<service::ops_service::OpsConnectionView>>),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn list_connections() -> Result<Json<ApiResponse<Vec<OpsConnectionView>>>, ApiError> {
    match ops_service::list_connections().await {
        Ok(list) => Ok(Json(ApiResponse::success(list))),
        Err(err) => Err(ApiError::internal_error(err)),
    }
}

/// 单条连接
#[utoipa::path(
    get,
    path = "/api/ops/connections/{id}",
    tag = "运维采集",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "连接详情", body = ApiResponse<service::ops_service::OpsConnectionView>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "连接不存在", body = ApiError),
    )
)]
pub async fn get_connection(
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<OpsConnectionView>>, ApiError> {
    let id = parse_id(&id)?;
    match ops_service::get_connection(id).await {
        Ok(view) => Ok(Json(ApiResponse::success(view))),
        Err(err) => Err(ApiError::not_found(err)),
    }
}

/// 新建连接
#[utoipa::path(
    post,
    path = "/api/ops/connections",
    tag = "运维采集",
    security(("bearer_auth" = [])),
    request_body = service::ops_service::OpsConnectionInput,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<service::ops_service::OpsConnectionView>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn create_connection(
    user_ctx: Option<Extension<UserContext>>,
    Json(input): Json<OpsConnectionInput>,
) -> Result<Json<ApiResponse<OpsConnectionView>>, ApiError> {
    // 服务身份（API Key）没有用户上下文（无 `user_id`）：`created_by` 记 NULL，
    // 而不是因缺少 `Extension<UserContext>` 直接 500（设计文档 §6.6「采集上报」模板
    // 本来就允许 API Key 调用运维写接口）。
    let user_id = user_ctx.map(|Extension(ctx)| ctx.user_id);
    match ops_service::create_connection(input, user_id).await {
        Ok(view) => Ok(Json(ApiResponse::success(view))),
        Err(err) => Err(ApiError::bad_request(err)),
    }
}

/// 更新连接
#[utoipa::path(
    put,
    path = "/api/ops/connections/{id}",
    tag = "运维采集",
    security(("bearer_auth" = [])),
    request_body = service::ops_service::OpsConnectionInput,
    responses(
        (status = 200, description = "更新成功", body = ApiResponse<service::ops_service::OpsConnectionView>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn update_connection(
    user_ctx: Option<Extension<UserContext>>,
    Path(id): Path<String>,
    Json(input): Json<OpsConnectionInput>,
) -> Result<Json<ApiResponse<OpsConnectionView>>, ApiError> {
    let id = parse_id(&id)?;
    // 同上：API Key 场景 `updated_by` 为 NULL
    let user_id = user_ctx.map(|Extension(ctx)| ctx.user_id);
    match ops_service::update_connection(id, input, user_id).await {
        Ok(view) => Ok(Json(ApiResponse::success(view))),
        Err(err) => Err(ApiError::bad_request(err)),
    }
}

/// 删除连接（软删）
#[utoipa::path(
    delete,
    path = "/api/ops/connections/{id}",
    tag = "运维采集",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "删除成功（`data` 为 null）"),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn delete_connection(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id = parse_id(&id)?;
    match ops_service::delete_connection(id).await {
        Ok(()) => Ok(Json(ApiResponse::success(()))),
        Err(err) => Err(ApiError::internal_error(err)),
    }
}

/// 草稿探测（不落库）
#[utoipa::path(
    post,
    path = "/api/ops/connections/test",
    tag = "运维采集",
    security(("bearer_auth" = [])),
    request_body = service::ops_service::OpsConnectionInput,
    responses(
        (status = 200, description = "探测结果（`ok=false` 表示不可达，仍返回 200）", body = ApiResponse<service::ops_service::OpsHealthView>),
        (status = 400, description = "配置错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
    )
)]
pub async fn test_draft(
    Json(input): Json<OpsConnectionInput>,
) -> Result<Json<ApiResponse<OpsHealthView>>, ApiError> {
    match ops_service::test_draft(&input).await {
        Ok(view) => Ok(Json(ApiResponse::success(view))),
        Err(err) => Err(ApiError::bad_request(err)),
    }
}

/// 已存连接探测
#[utoipa::path(
    post,
    path = "/api/ops/connections/{id}/test",
    tag = "运维采集",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "探测结果（`ok=false` 表示不可达，仍返回 200）", body = ApiResponse<service::ops_service::OpsHealthView>),
        (status = 400, description = "配置错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
    )
)]
pub async fn test_connection(
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<OpsHealthView>>, ApiError> {
    let id = parse_id(&id)?;
    match ops_service::test_connection(id).await {
        Ok(view) => Ok(Json(ApiResponse::success(view))),
        Err(err) => Err(ApiError::bad_request(err)),
    }
}

/// 立即采集一次
#[utoipa::path(
    post,
    path = "/api/ops/connections/{id}/collect",
    tag = "运维采集",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "本次采集结果", body = ApiResponse<service::ops_service::OpsCollectResult>),
        (status = 400, description = "配置或采集错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
    )
)]
pub async fn collect_now(
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<OpsCollectResult>>, ApiError> {
    let id = parse_id(&id)?;
    match ops_service::collect_now(id).await {
        Ok(result) => Ok(Json(ApiResponse::success(result))),
        Err(err) => Err(ApiError::bad_request(err)),
    }
}

/// 资源清单
#[utoipa::path(
    get,
    path = "/api/ops/links",
    tag = "运维采集",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "观测到的资源清单", body = ApiResponse<Vec<service::ops_service::OpsResourceLinkView>>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn list_links(
    Query(query): Query<LinkQuery>,
) -> Result<Json<ApiResponse<Vec<OpsResourceLinkView>>>, ApiError> {
    let connection_id = match query.connection_id {
        Some(raw) if !raw.trim().is_empty() => Some(parse_id(raw.trim())?),
        _ => None,
    };
    match ops_service::list_links(connection_id).await {
        Ok(list) => Ok(Json(ApiResponse::success(list))),
        Err(err) => Err(ApiError::internal_error(err)),
    }
}

/// 把观测资源归档到已有资产
#[utoipa::path(
    post,
    path = "/api/ops/links/{linkId}/bind",
    tag = "运维采集",
    security(("bearer_auth" = [])),
    request_body = BindRequest,
    responses(
        (status = 200, description = "绑定后的资源条目", body = ApiResponse<service::ops_service::OpsResourceLinkView>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn bind_link(
    user_ctx: Option<Extension<UserContext>>,
    Path(link_id): Path<String>,
    Json(req): Json<BindRequest>,
) -> Result<Json<ApiResponse<OpsResourceLinkView>>, ApiError> {
    let link_id = parse_id(&link_id)?;
    // `ops:link:bind` 在「采集上报」模板内，API Key 必须能用：无用户上下文时 `updated_by` 记 NULL
    let user_id = user_ctx.map(|Extension(ctx)| ctx.user_id);
    match ops_service::bind_link(link_id, req.asset_id, user_id).await {
        Ok(view) => Ok(Json(ApiResponse::success(view))),
        Err(err) => Err(ApiError::bad_request(err)),
    }
}
