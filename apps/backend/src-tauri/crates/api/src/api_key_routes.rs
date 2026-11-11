//! API Key 管理接口（设计文档 §5.2 / §10.1）
//!
//! 全部接口要求 **JWT 用户身份 + 管理员权限点**：
//!
//! | 方法 | 路径 | perms |
//! |---|---|---|
//! | POST | `/api/api-keys` | `system:apikey:add` |
//! | GET | `/api/api-keys` | `system:apikey:list` |
//! | POST | `/api/api-keys/{id}/rotate` | `system:apikey:edit` |
//! | DELETE | `/api/api-keys/{id}` | `system:apikey:delete` |
//!
//! 权限点判定在 `authz::authz_middleware` 完成（这些路径全部登记为 `system:*`，
//! 因此 API Key 自身**永远调不进来**，避免「用泄露的 key 给自己续命/扩权」）。
//!
//! 本模块只额外做两件事（都属于 HTTP 层的授权语义，不下沉到 service）：
//! 1. **子集约束**：`scopes ⊆ 创建者当前权限点并集`；
//! 2. **防自举**：禁止把管理面权限点写进 key 的 scopes。

use axum::{extract::Path, Extension, Json};
use serde::Deserialize;

use service::api_key_service::{self, ApiKeyView, CreatedApiKey, NewApiKey};

use super::auth::{AuthPrincipal, PrincipalKind};
use super::authz;
use super::response::{ApiError, ApiResponse};

/// 创建 API Key 请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateApiKeyRequest {
    /// 名称（给人看，例如「ERP 集成」）
    pub name: String,
    /// 用途说明
    pub description: Option<String>,
    /// 授权范围（至少 1 个、最多 50 个，必须 ⊆ 创建者权限）
    pub scopes: Vec<String>,
    /// 有效天数（默认 90，最长 365）
    pub expires_in_days: Option<i64>,
    /// 每分钟请求上限（默认 600）
    pub rate_limit_per_min: Option<i32>,
}

/// 轮换 API Key 请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RotateApiKeyRequest {
    /// 新 key 的有效天数（默认 90，最长 365）
    pub expires_in_days: Option<i64>,
}

/// 禁止写入 API Key scopes 的权限点前缀（防提权 / 防自举，设计文档 §6.6）
pub const FORBIDDEN_SCOPE_PREFIXES: &[&str] = &[
    "system:apikey:",
    "system:oauthclient:",
    "system:tenant:",
    "system:role:",
    "system:user:add",
    "system:user:edit",
    "system:user:delete",
    "system:user:reset",
    "system:user:assign",
];

/// 管理面约束：必须是用户身份的 JWT（API Key 一律 403）
fn ensure_user_principal(principal: &AuthPrincipal) -> Result<(), ApiError> {
    if principal.kind != PrincipalKind::User {
        tracing::warn!(
            subject = %principal.subject,
            "API Key 调用 API Key 管理接口，拒绝"
        );
        return Err(ApiError::forbidden("API Key 不可调用 API Key 管理接口"));
    }
    Ok(())
}

/// 防自举：scopes 不得包含高危管理点
fn ensure_scopes_not_privileged(scopes: &[String]) -> Result<(), ApiError> {
    for scope in scopes {
        if FORBIDDEN_SCOPE_PREFIXES
            .iter()
            .any(|prefix| scope.starts_with(prefix))
        {
            return Err(ApiError::bad_request(format!(
                "scope 不允许包含高危管理点: {}",
                scope
            )));
        }
    }
    Ok(())
}

/// 子集约束：scopes ⊆ 创建者当前权限点并集（超管直接放行）
fn ensure_scope_subset(principal: &AuthPrincipal, scopes: &[String]) -> Result<(), ApiError> {
    if principal.is_super_admin {
        return Ok(());
    }

    let missing: Vec<String> = scopes
        .iter()
        .filter(|need| {
            !principal
                .scopes
                .iter()
                .any(|have| authz::matches_scope(have, need))
        })
        .cloned()
        .collect();

    if missing.is_empty() {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "scope 超出创建者权限: {}",
            missing.join(", ")
        )))
    }
}

/// 解析路径中的 API Key ID
fn parse_api_key_id(id: &str) -> Result<i64, ApiError> {
    id.parse::<i64>()
        .map_err(|_| ApiError::bad_request("无效的 API Key ID"))
}

/// 非超管只能操作本租户的 key
async fn ensure_key_in_tenant(principal: &AuthPrincipal, id: i64) -> Result<(), ApiError> {
    if principal.is_super_admin {
        return Ok(());
    }

    let Some(tenant_id) = principal.tenant_id else {
        // 未绑定租户的账号看不到任何 key（fail-closed）
        return Err(ApiError::not_found("API Key 不存在"));
    };

    let keys = api_key_service::list_api_keys(Some(tenant_id))
        .await
        .map_err(ApiError::internal_error)?;

    if keys.iter().any(|key| key.id == id.to_string()) {
        Ok(())
    } else {
        // 不用 403，避免暴露其它租户 key 的存在性
        Err(ApiError::not_found("API Key 不存在"))
    }
}

/// 创建 API Key（返回一次性明文）
#[utoipa::path(
    post,
    path = "/api/api-keys",
    tag = "API Key",
    security(("bearer_auth" = [])),
    request_body = CreateApiKeyRequest,
    responses(
        (status = 200, description = "创建成功，api_key 字段只返回这一次", body = ApiResponse<CreatedApiKey>),
        (status = 400, description = "参数错误 / scope 超出创建者权限", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 403, description = "权限不足", body = ApiError),
    )
)]
pub async fn create_api_key(
    Extension(principal): Extension<AuthPrincipal>,
    Json(req): Json<CreateApiKeyRequest>,
) -> Result<Json<ApiResponse<CreatedApiKey>>, ApiError> {
    ensure_user_principal(&principal)?;

    let scopes = api_key_service::normalize_scopes(&req.scopes).map_err(ApiError::bad_request)?;
    ensure_scopes_not_privileged(&scopes)?;
    ensure_scope_subset(&principal, &scopes)?;

    let created = api_key_service::create_api_key(&NewApiKey {
        tenant_id: principal.tenant_id,
        name: &req.name,
        description: req.description.as_deref(),
        scopes: &scopes,
        rate_limit_per_min: api_key_service::clamp_rate_limit(req.rate_limit_per_min),
        expires_in_days: api_key_service::clamp_expires_in_days(req.expires_in_days),
        created_by: principal.user_id,
    })
    .await
    .map_err(ApiError::bad_request)?;

    Ok(Json(ApiResponse::success(created)))
}

/// API Key 列表（超管看全部，其余只看本租户）
#[utoipa::path(
    get,
    path = "/api/api-keys",
    tag = "API Key",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "API Key 列表（不含明文与哈希）", body = ApiResponse<Vec<ApiKeyView>>),
        (status = 401, description = "未认证", body = ApiError),
        (status = 403, description = "权限不足", body = ApiError),
    )
)]
pub async fn list_api_keys(
    Extension(principal): Extension<AuthPrincipal>,
) -> Result<Json<ApiResponse<Vec<ApiKeyView>>>, ApiError> {
    ensure_user_principal(&principal)?;

    let tenant_filter = if principal.is_super_admin {
        None
    } else {
        match principal.tenant_id {
            Some(tenant_id) => Some(tenant_id),
            // 未绑定租户：返回空列表，而不是把 public 全局 key 暴露出去
            None => return Ok(Json(ApiResponse::success(Vec::<ApiKeyView>::new()))),
        }
    };

    let keys = api_key_service::list_api_keys(tenant_filter)
        .await
        .map_err(ApiError::internal_error)?;

    Ok(Json(ApiResponse::success(keys)))
}

/// 轮换 API Key（旧 key 24 小时后过期）
#[utoipa::path(
    post,
    path = "/api/api-keys/{id}/rotate",
    tag = "API Key",
    security(("bearer_auth" = [])),
    request_body = RotateApiKeyRequest,
    responses(
        (status = 200, description = "轮换成功，返回新的一次性明文", body = ApiResponse<CreatedApiKey>),
        (status = 400, description = "参数错误 / key 已吊销", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 403, description = "权限不足", body = ApiError),
        (status = 404, description = "API Key 不存在", body = ApiError),
    )
)]
pub async fn rotate_api_key(
    Extension(principal): Extension<AuthPrincipal>,
    Path(id): Path<String>,
    Json(req): Json<RotateApiKeyRequest>,
) -> Result<Json<ApiResponse<CreatedApiKey>>, ApiError> {
    ensure_user_principal(&principal)?;
    let id = parse_api_key_id(&id)?;
    ensure_key_in_tenant(&principal, id).await?;

    let created = api_key_service::rotate_api_key(id, req.expires_in_days, principal.user_id)
        .await
        .map_err(ApiError::bad_request)?;

    Ok(Json(ApiResponse::success(created)))
}

/// 吊销 API Key（立即生效）
#[utoipa::path(
    delete,
    path = "/api/api-keys/{id}",
    tag = "API Key",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "已吊销（`data` 为 null）"),
        (status = 400, description = "参数错误 / 已吊销", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 403, description = "权限不足", body = ApiError),
        (status = 404, description = "API Key 不存在", body = ApiError),
    )
)]
pub async fn revoke_api_key(
    Extension(principal): Extension<AuthPrincipal>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    ensure_user_principal(&principal)?;
    let id = parse_api_key_id(&id)?;
    ensure_key_in_tenant(&principal, id).await?;

    api_key_service::revoke_api_key(id, principal.user_id)
        .await
        .map_err(ApiError::bad_request)?;

    Ok(Json(ApiResponse::success(())))
}

