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
#[derive(Debug, Deserialize)]
pub struct LinkQuery {
    #[serde(default, rename = "connectionId", alias = "connection_id")]
    pub connection_id: Option<String>,
}

/// 绑定资产的请求体
#[derive(Debug, Deserialize)]
pub struct BindRequest {
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
pub async fn providers() -> Result<Json<ApiResponse<Vec<OpsProviderOption>>>, ApiError> {
    Ok(Json(ApiResponse::success(ops_service::provider_options())))
}

/// 连接列表
pub async fn list_connections() -> Result<Json<ApiResponse<Vec<OpsConnectionView>>>, ApiError> {
    match ops_service::list_connections().await {
        Ok(list) => Ok(Json(ApiResponse::success(list))),
        Err(err) => Err(ApiError::internal_error(err)),
    }
}

/// 单条连接
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
pub async fn create_connection(
    Extension(ctx): Extension<UserContext>,
    Json(input): Json<OpsConnectionInput>,
) -> Result<Json<ApiResponse<OpsConnectionView>>, ApiError> {
    match ops_service::create_connection(input, Some(ctx.user_id)).await {
        Ok(view) => Ok(Json(ApiResponse::success(view))),
        Err(err) => Err(ApiError::bad_request(err)),
    }
}

/// 更新连接
pub async fn update_connection(
    Extension(ctx): Extension<UserContext>,
    Path(id): Path<String>,
    Json(input): Json<OpsConnectionInput>,
) -> Result<Json<ApiResponse<OpsConnectionView>>, ApiError> {
    let id = parse_id(&id)?;
    match ops_service::update_connection(id, input, Some(ctx.user_id)).await {
        Ok(view) => Ok(Json(ApiResponse::success(view))),
        Err(err) => Err(ApiError::bad_request(err)),
    }
}

/// 删除连接（软删）
pub async fn delete_connection(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    let id = parse_id(&id)?;
    match ops_service::delete_connection(id).await {
        Ok(()) => Ok(Json(ApiResponse::success(()))),
        Err(err) => Err(ApiError::internal_error(err)),
    }
}

/// 草稿探测（不落库）
pub async fn test_draft(
    Json(input): Json<OpsConnectionInput>,
) -> Result<Json<ApiResponse<OpsHealthView>>, ApiError> {
    match ops_service::test_draft(&input).await {
        Ok(view) => Ok(Json(ApiResponse::success(view))),
        Err(err) => Err(ApiError::bad_request(err)),
    }
}

/// 已存连接探测
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
pub async fn bind_link(
    Extension(ctx): Extension<UserContext>,
    Path(link_id): Path<String>,
    Json(req): Json<BindRequest>,
) -> Result<Json<ApiResponse<OpsResourceLinkView>>, ApiError> {
    let link_id = parse_id(&link_id)?;
    match ops_service::bind_link(link_id, req.asset_id, Some(ctx.user_id)).await {
        Ok(view) => Ok(Json(ApiResponse::success(view))),
        Err(err) => Err(ApiError::bad_request(err)),
    }
}
