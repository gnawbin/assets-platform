//! LLM 厂商 / 模型 / 用户偏好 HTTP API 路由
//!
//! 对应原 Tauri 命令：`get_llm_providers` / `get_llm_provider` / `create_llm_provider` /
//! `update_llm_provider` / `delete_llm_provider` / `get_llm_models` / `create_llm_model` /
//! `update_llm_model` / `delete_llm_model` / `fetch_llm_models` / `get_user_llm_setting` /
//! `save_user_llm_setting`。
//!
//! # 请求形状约定（全 HTTP 化统一原则）
//!
//! 前端 `httpAdapter.extractParams()` 只把**路径占位符**对应的参数从请求体里剥掉，
//! 其余参数原样作为 JSON body（GET / DELETE 则作为 query string）发送。
//!
//! | 命令 | 路由 | 路径参数 | body / query |
//! | --- | --- | --- | --- |
//! | `get_llm_providers()` | `GET /api/llm/providers` | — | — |
//! | `get_llm_provider(id)` | `GET /api/llm/providers/{id}` | `id` | — |
//! | `create_llm_provider(providerCode, providerName, …)` | `POST /api/llm/providers` | — | `{providerCode, providerName, baseUrl?, apiKey?, weight?, isLocal?, createdBy?, currentUserId?}` |
//! | `update_llm_provider(id, …)` | `PUT /api/llm/providers/{id}` | `id` | `{providerCode?, providerName?, baseUrl?, apiKey?, weight?, enable?}` |
//! | `delete_llm_provider(id)` | `DELETE /api/llm/providers/{id}` | `id` | — |
//! | `fetch_llm_models(providerId)` | `POST /api/llm/providers/{providerId}/fetch-models` | `providerId` | — |
//! | `get_llm_models(providerId?)` | `GET /api/llm/models` | — | `?providerId` |
//! | `create_llm_model(providerId, modelCode, …)` | `POST /api/llm/models` | — | `{providerId, modelCode, modelName, modelType, contextWindow?, temperatureDefault?, maxTokensDefault?, enable?}` |
//! | `update_llm_model(id, …)` | `PUT /api/llm/models/{id}` | `id` | `{modelCode?, modelName?, modelType?, contextWindow?, temperatureDefault?, maxTokensDefault?, enable?}` |
//! | `delete_llm_model(id)` | `DELETE /api/llm/models/{id}` | `id` | — |
//! | `get_user_llm_setting(userId)` | `GET /api/llm/user-setting` | — | `?userId`（仅比对告警，服务端用令牌 `sub`） |
//! | `save_user_llm_setting(userId, …)` | `POST /api/llm/user-setting` | — | `{userId?, defaultProviderId?, defaultChatModelId?, defaultEmbedModelId?, customTemp?, customMaxToken?}` |
//!
//! # 安全口径
//!
//! | 关注点 | 结论 |
//! | --- | --- |
//! | API Key 脱敏 | 厂商响应统一返回 `API_KEY_MASK`（`******`），**永不**回传密文或明文；前端编辑时不会再回填旧 Key |
//! | 回传脱敏值 | 更新时 `apiKey == "******"` 视为「未修改」，不会被当成新 Key 覆盖 |
//! | 审计人 | `created_by` 取令牌 `sub`；body 的 `createdBy` / `currentUserId` 仅比对告警 |
//! | 用户偏好归属 | `get/save_user_llm_setting` 一律使用令牌 `sub`，body/query 的 `userId` 不采信（防越权改他人偏好） |
//! | 不存在语义 | `LLM_PROVIDER_NOT_FOUND` / `LLM_MODEL_NOT_FOUND` → **404**；重复删除同样 404 |
//! | 密钥加密 | 桌面命令与 HTTP 路由共用 `build_provider_entity()`（`utils::crypto::encrypt_api_key`） |

use axum::extract::{Path, Query};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use database::models::{LlmModel, LlmProvider, UserLLmSetting};
use service::llm_provider_service::{self, API_KEY_MASK};

use super::auth::UserContext;
use super::response::{ApiError, ApiResponse};

// ======================== 响应视图 ========================

/// 厂商视图（**API Key 已脱敏**，不含 `secret_key` / `extra_config`）
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ProviderView {
    /// 厂商 ID（字符串形式的 i64，与桌面命令一致）
    pub id: String,
    /// 厂商编码（如 openai / deepseek）
    pub provider_code: String,
    /// 展示名称
    pub provider_name: String,
    /// OpenAI 兼容 Base URL
    pub base_url: Option<String>,
    /// API Key —— 固定为脱敏占位符 `******`（未配置时 `null`）
    pub api_key: Option<String>,
    /// 负载权重
    pub weight: Option<i32>,
    /// 是否本地模型
    pub is_local: bool,
    /// 是否启用
    pub enable: bool,
}

impl From<LlmProvider> for ProviderView {
    fn from(p: LlmProvider) -> Self {
        Self {
            id: p.id.to_string(),
            provider_code: p.provider_code,
            provider_name: p.provider_name,
            base_url: p.base_url,
            // 永不回传密文 / 明文，只给固定占位符
            api_key: p
                .api_key
                .filter(|key| !key.is_empty())
                .map(|_| API_KEY_MASK.to_string()),
            weight: p.weight,
            is_local: p.is_local,
            enable: p.enable,
        }
    }
}

// ======================== 请求 DTO ========================

/// 创建厂商请求（与 `create_llm_provider` 命令参数同名）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateProviderRequest {
    /// 厂商编码
    #[serde(rename = "providerCode")]
    pub provider_code: String,
    /// 展示名称
    #[serde(rename = "providerName")]
    pub provider_name: String,
    /// OpenAI 兼容 Base URL
    #[serde(rename = "baseUrl", default)]
    pub base_url: Option<String>,
    /// API Key 明文（服务端加密后入库）
    #[serde(rename = "apiKey", default)]
    pub api_key: Option<String>,
    /// 权重
    #[serde(default)]
    pub weight: Option<i32>,
    /// 是否本地模型
    #[serde(rename = "isLocal", default)]
    pub is_local: Option<bool>,
    /// 创建人 —— 客户端可伪造，服务端以令牌 `sub` 为准
    #[serde(rename = "createdBy", default)]
    pub created_by: Option<String>,
    /// 兼容桌面命令参数名（同上，忽略）
    #[serde(rename = "currentUserId", default)]
    pub current_user_id: Option<String>,
}

/// 更新厂商请求（`id` 走路径）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateProviderRequest {
    /// 厂商编码（缺省不改）
    #[serde(rename = "providerCode", default)]
    pub provider_code: Option<String>,
    /// 展示名称（缺省不改）
    #[serde(rename = "providerName", default)]
    pub provider_name: Option<String>,
    /// Base URL（缺省不改）
    #[serde(rename = "baseUrl", default)]
    pub base_url: Option<String>,
    /// API Key 明文；缺省 / 空串 / 脱敏占位符 `******` 均视为「不修改」
    #[serde(rename = "apiKey", default)]
    pub api_key: Option<String>,
    /// 权重（缺省不改）
    #[serde(default)]
    pub weight: Option<i32>,
    /// 是否启用（缺省不改）
    #[serde(default)]
    pub enable: Option<bool>,
}

/// 模型列表查询参数（GET）
#[derive(Debug, Deserialize)]
pub struct ListModelsQuery {
    /// 厂商 ID（缺省返回全部）
    #[serde(rename = "providerId", default)]
    pub provider_id: Option<String>,
}

/// 创建模型请求（与 `create_llm_model` 命令参数同名）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateModelRequest {
    /// 所属厂商 ID
    #[serde(rename = "providerId")]
    pub provider_id: String,
    /// 模型编码
    #[serde(rename = "modelCode")]
    pub model_code: String,
    /// 模型名称
    #[serde(rename = "modelName")]
    pub model_name: String,
    /// 类型：chat / embedding / vision
    #[serde(rename = "modelType")]
    pub model_type: String,
    /// 上下文窗口
    #[serde(rename = "contextWindow", default)]
    pub context_window: Option<i32>,
    /// 默认温度
    #[serde(rename = "temperatureDefault", default)]
    pub temperature_default: Option<f64>,
    /// 默认最大 token
    #[serde(rename = "maxTokensDefault", default)]
    pub max_tokens_default: Option<i32>,
    /// 是否启用（缺省 true）
    #[serde(default)]
    pub enable: Option<bool>,
}

/// 更新模型请求（`id` 走路径，字段全部可选）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateModelRequest {
    /// 模型编码（缺省不改）
    #[serde(rename = "modelCode", default)]
    pub model_code: Option<String>,
    /// 模型名称（缺省不改）
    #[serde(rename = "modelName", default)]
    pub model_name: Option<String>,
    /// 类型（缺省不改）
    #[serde(rename = "modelType", default)]
    pub model_type: Option<String>,
    /// 上下文窗口（缺省不改）
    #[serde(rename = "contextWindow", default)]
    pub context_window: Option<i32>,
    /// 默认温度（缺省不改）
    #[serde(rename = "temperatureDefault", default)]
    pub temperature_default: Option<f64>,
    /// 默认最大 token（缺省不改）
    #[serde(rename = "maxTokensDefault", default)]
    pub max_tokens_default: Option<i32>,
    /// 是否启用（缺省不改）
    #[serde(default)]
    pub enable: Option<bool>,
}

/// 用户偏好查询参数（GET）——`userId` 仅比对告警
#[derive(Debug, Deserialize)]
pub struct UserSettingQuery {
    /// 用户 ID —— 客户端可伪造，服务端以令牌 `sub` 为准
    #[serde(rename = "userId", default)]
    pub user_id: Option<String>,
}

/// 保存用户偏好请求（字段与命令参数同名；`userId` 仅比对告警）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SaveUserSettingRequest {
    /// 用户 ID —— 客户端可伪造，服务端以令牌 `sub` 为准
    #[serde(rename = "userId", default)]
    pub user_id: Option<String>,
    /// 默认厂商
    #[serde(rename = "defaultProviderId", default)]
    pub default_provider_id: Option<String>,
    /// 默认对话模型
    #[serde(rename = "defaultChatModelId", default)]
    pub default_chat_model_id: Option<String>,
    /// 默认向量化模型
    #[serde(rename = "defaultEmbedModelId", default)]
    pub default_embed_model_id: Option<String>,
    /// 自定义温度
    #[serde(rename = "customTemp", default)]
    pub custom_temp: Option<f64>,
    /// 自定义最大 token
    #[serde(rename = "customMaxToken", default)]
    pub custom_max_token: Option<i32>,
}


// ======================== 参数与错误处理 ========================

/// 服务层错误 → HTTP 语义
///
/// `LLM_*_NOT_FOUND`（含「未找到」）→ **404**；解析类错误（`无效的…`）→ **400**；
/// 其余（数据库 / 上游厂商报错）→ 500。
fn map_service_error(message: String) -> ApiError {
    if message.contains("未找到") {
        ApiError::not_found(message)
    } else if message.starts_with("无效的") {
        ApiError::bad_request(message)
    } else {
        ApiError::internal_error(message)
    }
}

/// 解析字符串形式的 i64 ID（失败 → 400，文案与桌面命令一致）
fn parse_id(raw: &str, label: &str) -> Result<i64, ApiError> {
    raw.trim()
        .parse::<i64>()
        .map_err(|e| ApiError::bad_request(format!("无效的{}: {}", label, e)))
}

/// 解析可选的字符串 ID：空串 / 非法值视为未指定（与桌面命令 `.parse().ok()` 一致）
fn parse_opt_id(raw: Option<&str>) -> Option<i64> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
}

/// 解析必填的可选 ID（桌面命令对非空非法值报 400）
fn parse_opt_id_strict(raw: Option<&str>, label: &str) -> Result<Option<i64>, ApiError> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s
            .parse::<i64>()
            .map(Some)
            .map_err(|e| ApiError::bad_request(format!("无效的{}: {}", label, e))),
        None => Ok(None),
    }
}

/// 以令牌 `sub` 为准解析调用者身份（body/query 的 `userId` 仅供比对告警）
fn resolve_user_id(
    token_user_id: Option<i64>,
    body_user_id: Option<&str>,
) -> Result<i64, ApiError> {
    let token_user_id = token_user_id.ok_or_else(|| ApiError::unauthorized("未认证"))?;

    if let Some(body_id) = parse_opt_id(body_user_id) {
        if body_id != token_user_id {
            warn!(
                "[HTTP] 请求体 userId={} 与令牌 sub={} 不一致，以令牌为准（防越权）",
                body_id, token_user_id
            );
        }
    }

    Ok(token_user_id)
}

/// 从请求上下文取令牌身份（`None` 表示中间件未注入）
fn token_user_id(user_ctx: &Option<Extension<UserContext>>) -> Option<i64> {
    user_ctx.as_ref().map(|Extension(ctx)| ctx.user_id)
}

/// API Key 归一化：空串 / 脱敏占位符 → `None`（表示「不修改」）
///
/// 前端编辑表单不会回填旧 Key，但为兼容「把列表响应原样回传」的旧前端，
/// 这里把 `******` 也当作未修改，避免把占位符当成新 Key 加密入库。
fn normalize_api_key(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty() && *s != API_KEY_MASK)
}

// ======================== 厂商 handler ========================

/// GET /api/llm/providers - 厂商列表（API Key 已脱敏）
#[utoipa::path(
    get,
    path = "/api/llm/providers",
    tag = "LLM 管理",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "厂商数组（`api_key` 固定为 `******`）"),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn list_providers(
    user_ctx: Option<Extension<UserContext>>,
) -> Result<Json<ApiResponse<Vec<ProviderView>>>, ApiError> {
    let user_id = resolve_user_id(token_user_id(&user_ctx), None)?;
    info!("[HTTP] GET /api/llm/providers user_id={}", user_id);

    let list = llm_provider_service::get_providers()
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(
        list.into_iter().map(ProviderView::from).collect(),
    )))
}

/// GET /api/llm/providers/{id} - 单个厂商（API Key 已脱敏）
#[utoipa::path(
    get,
    path = "/api/llm/providers/{id}",
    tag = "LLM 管理",
    security(("bearer_auth" = [])),
    params(("id" = String, Path, description = "厂商 ID")),
    responses(
        (status = 200, description = "厂商详情（`api_key` 固定为 `******`）"),
        (status = 400, description = "ID 非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "厂商不存在或已删除", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn get_provider(
    Path(id): Path<String>,
    user_ctx: Option<Extension<UserContext>>,
) -> Result<Json<ApiResponse<ProviderView>>, ApiError> {
    let user_id = resolve_user_id(token_user_id(&user_ctx), None)?;
    let pid = parse_id(&id, "ID")?;
    info!("[HTTP] GET /api/llm/providers/{} user_id={}", pid, user_id);

    let provider = llm_provider_service::get_provider(pid)
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(ProviderView::from(provider))))
}

/// POST /api/llm/providers - 新建厂商（API Key 加密入库）
#[utoipa::path(
    post,
    path = "/api/llm/providers",
    tag = "LLM 管理",
    security(("bearer_auth" = [])),
    request_body = CreateProviderRequest,
    responses(
        (status = 200, description = "创建成功（`api_key` 已脱敏）"),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn create_provider(
    user_ctx: Option<Extension<UserContext>>,
    Json(req): Json<CreateProviderRequest>,
) -> Result<Json<ApiResponse<ProviderView>>, ApiError> {
    // 两个字段都只是兼容旧前端，服务端只认令牌
    let claimed = req.created_by.as_deref().or(req.current_user_id.as_deref());
    let user_id = resolve_user_id(token_user_id(&user_ctx), claimed)?;
    info!(
        "[HTTP] POST /api/llm/providers user_id={} code={}",
        user_id, req.provider_code
    );

    let entity = llm_provider_service::build_provider_entity(
        req.provider_code,
        req.provider_name,
        req.base_url,
        req.api_key,
        req.weight,
        req.is_local,
        Some(user_id),
    );

    let created = llm_provider_service::create_provider(&entity)
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(ProviderView::from(created))))
}


/// PUT /api/llm/providers/{id} - 更新厂商
///
/// `apiKey` 缺省 / 空串 / `******` 均表示「不修改」；新 Key 会先加密再入库。
#[utoipa::path(
    put,
    path = "/api/llm/providers/{id}",
    tag = "LLM 管理",
    security(("bearer_auth" = [])),
    params(("id" = String, Path, description = "厂商 ID")),
    request_body = UpdateProviderRequest,
    responses(
        (status = 200, description = "更新成功（`api_key` 已脱敏）"),
        (status = 400, description = "ID 非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "厂商不存在或已删除", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn update_provider(
    Path(id): Path<String>,
    user_ctx: Option<Extension<UserContext>>,
    Json(req): Json<UpdateProviderRequest>,
) -> Result<Json<ApiResponse<ProviderView>>, ApiError> {
    let user_id = resolve_user_id(token_user_id(&user_ctx), None)?;
    let pid = parse_id(&id, "ID")?;
    info!("[HTTP] PUT /api/llm/providers/{} user_id={}", pid, user_id);

    // 先取现有记录：缺失 → 404；未传的字段沿用原值（与桌面命令一致）
    let existing = llm_provider_service::get_provider(pid)
        .await
        .map_err(map_service_error)?;

    let code = req.provider_code.unwrap_or(existing.provider_code);
    let name = req.provider_name.unwrap_or(existing.provider_name);

    // 只有「真正的新 Key」才加密；占位符 / 空串 / 缺省 → None（不改）
    let encrypted_key = normalize_api_key(req.api_key.as_deref()).map(|raw| {
        utils::crypto::encrypt_api_key(raw).unwrap_or_else(|_| raw.to_string())
    });

    let updated = llm_provider_service::update_provider(
        pid,
        &code,
        &name,
        req.base_url.as_deref(),
        encrypted_key.as_deref(),
        req.weight,
        req.enable,
    )
    .await
    .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(ProviderView::from(updated))))
}

/// DELETE /api/llm/providers/{id} - 删除厂商（软删除，幂等 404）
#[utoipa::path(
    delete,
    path = "/api/llm/providers/{id}",
    tag = "LLM 管理",
    security(("bearer_auth" = [])),
    params(("id" = String, Path, description = "厂商 ID")),
    responses(
        (status = 200, description = "删除成功"),
        (status = 400, description = "ID 非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "厂商不存在或已删除", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn delete_provider(
    Path(id): Path<String>,
    user_ctx: Option<Extension<UserContext>>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let user_id = resolve_user_id(token_user_id(&user_ctx), None)?;
    let pid = parse_id(&id, "ID")?;
    info!("[HTTP] DELETE /api/llm/providers/{} user_id={}", pid, user_id);

    llm_provider_service::delete_provider(pid)
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(())))
}

/// POST /api/llm/providers/{providerId}/fetch-models - 拉取厂商模型并入库
///
/// 上游厂商报错（鉴权失败 / 网络不可达）→ 500，文案原样透传便于排查。
#[utoipa::path(
    post,
    path = "/api/llm/providers/{providerId}/fetch-models",
    tag = "LLM 管理",
    security(("bearer_auth" = [])),
    params(("providerId" = String, Path, description = "厂商 ID")),
    responses(
        (status = 200, description = "拉取并保存后的模型数组"),
        (status = 400, description = "厂商 ID 非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "厂商不存在或已删除", body = ApiError),
        (status = 500, description = "上游厂商报错 / 未配置 API Key", body = ApiError),
    )
)]
pub async fn fetch_models(
    Path(providerId): Path<String>,
    user_ctx: Option<Extension<UserContext>>,
) -> Result<Json<ApiResponse<Vec<LlmModel>>>, ApiError> {
    let user_id = resolve_user_id(token_user_id(&user_ctx), None)?;
    let pid = parse_id(&providerId, "厂商ID")?;
    info!(
        "[HTTP] POST /api/llm/providers/{}/fetch-models user_id={}",
        pid, user_id
    );

    let models = llm_provider_service::fetch_models_from_api(pid)
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(models)))
}


// ======================== 模型 handler ========================

/// GET /api/llm/models - 模型列表（可按 `providerId` 过滤）
#[utoipa::path(
    get,
    path = "/api/llm/models",
    tag = "LLM 管理",
    security(("bearer_auth" = [])),
    params(("providerId" = Option<String>, Query, description = "厂商 ID（缺省返回全部）")),
    responses(
        (status = 200, description = "模型数组（与命令返回同形）"),
        (status = 400, description = "厂商 ID 非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn list_models(
    user_ctx: Option<Extension<UserContext>>,
    Query(q): Query<ListModelsQuery>,
) -> Result<Json<ApiResponse<Vec<LlmModel>>>, ApiError> {
    let user_id = resolve_user_id(token_user_id(&user_ctx), None)?;
    let provider_id = parse_opt_id_strict(q.provider_id.as_deref(), "ID")?;
    info!(
        "[HTTP] GET /api/llm/models user_id={} provider_id={:?}",
        user_id, provider_id
    );

    let list = llm_provider_service::get_models(provider_id)
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(list)))
}

/// POST /api/llm/models - 新建模型（同名模型按 `provider_id + model_code` 覆盖更新）
#[utoipa::path(
    post,
    path = "/api/llm/models",
    tag = "LLM 管理",
    security(("bearer_auth" = [])),
    request_body = CreateModelRequest,
    responses(
        (status = 200, description = "创建 / 覆盖成功"),
        (status = 400, description = "厂商 ID 非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn create_model(
    user_ctx: Option<Extension<UserContext>>,
    Json(req): Json<CreateModelRequest>,
) -> Result<Json<ApiResponse<LlmModel>>, ApiError> {
    let user_id = resolve_user_id(token_user_id(&user_ctx), None)?;
    let pid = parse_id(&req.provider_id, "厂商ID")?;
    info!(
        "[HTTP] POST /api/llm/models user_id={} provider_id={} code={}",
        user_id, pid, req.model_code
    );

    let created = llm_provider_service::create_model(
        pid,
        &req.model_code,
        &req.model_name,
        &req.model_type,
        req.context_window,
        req.temperature_default,
        req.max_tokens_default,
        req.enable,
    )
    .await
    .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(created)))
}

/// PUT /api/llm/models/{id} - 更新模型
#[utoipa::path(
    put,
    path = "/api/llm/models/{id}",
    tag = "LLM 管理",
    security(("bearer_auth" = [])),
    params(("id" = String, Path, description = "模型 ID")),
    request_body = UpdateModelRequest,
    responses(
        (status = 200, description = "更新成功"),
        (status = 400, description = "ID 非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "模型不存在或已删除", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn update_model(
    Path(id): Path<String>,
    user_ctx: Option<Extension<UserContext>>,
    Json(req): Json<UpdateModelRequest>,
) -> Result<Json<ApiResponse<LlmModel>>, ApiError> {
    let user_id = resolve_user_id(token_user_id(&user_ctx), None)?;
    let mid = parse_id(&id, "模型ID")?;
    info!("[HTTP] PUT /api/llm/models/{} user_id={}", mid, user_id);

    let updated = llm_provider_service::update_model(
        mid,
        req.model_code.as_deref(),
        req.model_name.as_deref(),
        req.model_type.as_deref(),
        req.context_window,
        req.temperature_default,
        req.max_tokens_default,
        req.enable,
    )
    .await
    .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(updated)))
}

/// DELETE /api/llm/models/{id} - 删除模型（软删除，幂等 404）
#[utoipa::path(
    delete,
    path = "/api/llm/models/{id}",
    tag = "LLM 管理",
    security(("bearer_auth" = [])),
    params(("id" = String, Path, description = "模型 ID")),
    responses(
        (status = 200, description = "删除成功"),
        (status = 400, description = "ID 非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "模型不存在或已删除", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn delete_model(
    Path(id): Path<String>,
    user_ctx: Option<Extension<UserContext>>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let user_id = resolve_user_id(token_user_id(&user_ctx), None)?;
    let mid = parse_id(&id, "模型ID")?;
    info!("[HTTP] DELETE /api/llm/models/{} user_id={}", mid, user_id);

    llm_provider_service::delete_model(mid)
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(())))
}


// ======================== 用户偏好 handler ========================

/// GET /api/llm/user-setting - 当前用户的模型偏好
///
/// query 里的 `userId` 仅比对告警；实际查询用令牌 `sub`。
#[utoipa::path(
    get,
    path = "/api/llm/user-setting",
    tag = "LLM 管理",
    security(("bearer_auth" = [])),
    params(("userId" = Option<String>, Query, description = "用户 ID（仅兼容旧前端，服务端以令牌为准）")),
    responses(
        (status = 200, description = "用户偏好；未配置时 `data` 为 null"),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn get_user_setting(
    user_ctx: Option<Extension<UserContext>>,
    Query(q): Query<UserSettingQuery>,
) -> Result<Json<ApiResponse<Option<UserLLmSetting>>>, ApiError> {
    let user_id = resolve_user_id(token_user_id(&user_ctx), q.user_id.as_deref())?;
    info!("[HTTP] GET /api/llm/user-setting user_id={}", user_id);

    let setting = llm_provider_service::get_user_llm_setting(user_id)
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(setting)))
}

/// POST /api/llm/user-setting - 保存当前用户的模型偏好（UPSERT）
///
/// body 里的 `userId` 不采信：只能写自己的偏好。
#[utoipa::path(
    post,
    path = "/api/llm/user-setting",
    tag = "LLM 管理",
    security(("bearer_auth" = [])),
    request_body = SaveUserSettingRequest,
    responses(
        (status = 200, description = "保存成功，返回最新偏好"),
        (status = 400, description = "厂商 / 模型 ID 非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn save_user_setting(
    user_ctx: Option<Extension<UserContext>>,
    Json(req): Json<SaveUserSettingRequest>,
) -> Result<Json<ApiResponse<UserLLmSetting>>, ApiError> {
    let user_id = resolve_user_id(token_user_id(&user_ctx), req.user_id.as_deref())?;
    let default_provider_id =
        parse_opt_id_strict(req.default_provider_id.as_deref(), "厂商ID")?;
    let default_chat_model_id =
        parse_opt_id_strict(req.default_chat_model_id.as_deref(), "模型ID")?;
    let default_embed_model_id =
        parse_opt_id_strict(req.default_embed_model_id.as_deref(), "模型ID")?;
    info!(
        "[HTTP] POST /api/llm/user-setting user_id={} provider={:?}",
        user_id, default_provider_id
    );

    let setting = UserLLmSetting {
        id: 0,
        user_id,
        default_provider_id,
        default_chat_model_id,
        default_embed_model_id,
        custom_temp: req.custom_temp,
        custom_max_token: req.custom_max_token,
        created_at: None,
        updated_at: None,
        deleted: 0,
    };

    let saved = llm_provider_service::upsert_user_llm_setting(&setting)
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(saved)))
}


#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Uri;
    use database::models::LlmProvider;

    fn provider_with_key(key: Option<&str>) -> LlmProvider {
        LlmProvider {
            id: 42,
            provider_code: "openai".to_string(),
            provider_name: "OpenAI".to_string(),
            base_url: Some("https://api.openai.com/v1".to_string()),
            api_key: key.map(|k| k.to_string()),
            secret_key: Some("should-not-leak".to_string()),
            extra_config: Some(serde_json::json!({"region": "cn"})),
            weight: Some(10),
            is_local: false,
            enable: true,
            created_by: Some(1),
            created_at: None,
            updated_at: None,
            deleted: 0,
        }
    }

    /// 厂商响应必须脱敏：不出现密文/明文，也不出现 secret_key / extra_config
    #[test]
    fn provider_view_masks_api_key_and_drops_secrets() {
        let view = ProviderView::from(provider_with_key(Some("encrypted-ciphertext")));
        assert_eq!(view.api_key.as_deref(), Some(API_KEY_MASK));
        assert_eq!(view.id, "42", "ID 以字符串返回（与桌面命令一致）");

        let json = serde_json::to_value(&view).expect("视图应可序列化");
        let text = json.to_string();
        assert!(!text.contains("encrypted-ciphertext"), "密文不得出现在响应里");
        assert!(!text.contains("should-not-leak"), "secret_key 不得出现在响应里");
        assert!(!text.contains("region"), "extra_config 不得出现在响应里");

        // 未配置 Key → null；空串同样视为未配置
        assert!(ProviderView::from(provider_with_key(None)).api_key.is_none());
        assert!(ProviderView::from(provider_with_key(Some(""))).api_key.is_none());
    }

    /// 回传脱敏占位符 / 空串 / 缺省 → 视为「不修改」，不会覆盖真实 Key
    #[test]
    fn masked_api_key_is_treated_as_unchanged() {
        assert_eq!(normalize_api_key(None), None);
        assert_eq!(normalize_api_key(Some("")), None);
        assert_eq!(normalize_api_key(Some("   ")), None);
        assert_eq!(normalize_api_key(Some(API_KEY_MASK)), None);
        assert_eq!(normalize_api_key(Some("sk-new-key")), Some("sk-new-key"));
    }

    /// 404 / 400 / 500 映射
    #[test]
    fn service_errors_map_to_http_codes() {
        use service::llm_provider_service::{LLM_MODEL_NOT_FOUND, LLM_PROVIDER_NOT_FOUND};

        assert_eq!(map_service_error(LLM_PROVIDER_NOT_FOUND.to_string()).code, 404);
        assert_eq!(map_service_error(LLM_MODEL_NOT_FOUND.to_string()).code, 404);
        assert_eq!(map_service_error("无效的厂商ID: x".to_string()).code, 400);
        // 上游厂商报错（鉴权失败等）不是 4xx
        assert_eq!(map_service_error("API 返回的模型列表为空".to_string()).code, 500);
    }

    /// 令牌缺失 → 401；body 的 userId 不能覆盖令牌身份
    #[test]
    fn user_id_comes_from_token_only() {
        assert_eq!(
            resolve_user_id(None, Some("7")).expect_err("无令牌必须拒绝").code,
            401
        );
        assert_eq!(resolve_user_id(Some(5), None).expect("缺省正常"), 5);
        assert_eq!(resolve_user_id(Some(5), Some("6")).expect("不一致按令牌"), 5);
        assert_eq!(resolve_user_id(Some(5), Some("abc")).expect("非法按令牌"), 5);
    }

    /// 可选 ID 解析：非空非法值 → 400（与命令一致）
    #[test]
    fn optional_ids_are_strict() {
        assert_eq!(parse_opt_id_strict(Some("3"), "ID").expect("合法"), Some(3));
        assert_eq!(parse_opt_id_strict(Some(""), "ID").expect("空串"), None);
        assert_eq!(parse_opt_id_strict(None, "ID").expect("缺省"), None);
        assert_eq!(
            parse_opt_id_strict(Some("abc"), "厂商ID")
                .expect_err("非法应报错")
                .code,
            400
        );
    }

    /// GET 参数使用 camelCase（httpAdapter 原样发 query）
    #[test]
    fn query_params_are_camel_case() {
        let uri = Uri::from_static("/api/llm/models?providerId=3");
        let Query(q) = Query::<ListModelsQuery>::try_from_uri(&uri).expect("应能解析 query");
        assert_eq!(q.provider_id.as_deref(), Some("3"));

        let uri = Uri::from_static("/api/llm/user-setting?userId=9");
        let Query(q) = Query::<UserSettingQuery>::try_from_uri(&uri).expect("应能解析 query");
        assert_eq!(q.user_id.as_deref(), Some("9"));
    }

    /// 前端 body 必须能原样反序列化（未知字段忽略）
    #[test]
    fn bodies_match_command_arguments() {
        let req: CreateProviderRequest = serde_json::from_str(
            r#"{"providerCode":"openai","providerName":"OpenAI","baseUrl":"https://api.openai.com/v1",
                "apiKey":"sk-x","weight":10,"isLocal":false,"createdBy":"7","currentUserId":"7","unknown":1}"#,
        )
        .expect("创建厂商 body 应可反序列化");
        assert_eq!(req.provider_code, "openai");
        assert_eq!(req.api_key.as_deref(), Some("sk-x"));
        assert_eq!(req.created_by.as_deref(), Some("7"));
        assert_eq!(req.weight, Some(10));

        let req: UpdateProviderRequest =
            serde_json::from_str(r#"{"providerName":"改名","enable":false}"#)
                .expect("更新厂商 body 应可反序列化");
        assert_eq!(req.provider_name.as_deref(), Some("改名"));
        assert_eq!(req.enable, Some(false));
        assert!(req.api_key.is_none());

        let req: CreateModelRequest = serde_json::from_str(
            r#"{"providerId":"3","modelCode":"gpt-4o","modelName":"GPT-4o","modelType":"chat",
                "contextWindow":128000,"temperatureDefault":0.7,"maxTokensDefault":4096}"#,
        )
        .expect("创建模型 body 应可反序列化");
        assert_eq!(req.provider_id, "3");
        assert_eq!(req.context_window, Some(128000));
        assert_eq!(req.enable, None);

        let req: UpdateModelRequest =
            serde_json::from_str(r#"{"modelType":"vision","enable":true}"#)
                .expect("更新模型 body 应可反序列化");
        assert_eq!(req.model_type.as_deref(), Some("vision"));
        assert_eq!(req.enable, Some(true));

        let req: SaveUserSettingRequest = serde_json::from_str(
            r#"{"userId":"1","defaultProviderId":"3","defaultChatModelId":"4","customTemp":0.5}"#,
        )
        .expect("保存偏好 body 应可反序列化");
        assert_eq!(req.default_provider_id.as_deref(), Some("3"));
        assert_eq!(req.default_chat_model_id.as_deref(), Some("4"));
        assert!(req.default_embed_model_id.is_none());
        assert_eq!(req.custom_temp, Some(0.5));
    }

    /// 12 条路由可同时注册（路径占位符与前端参数名一致）
    #[test]
    fn llm_router_builds() {
        use axum::routing::{get, post, put};
        use axum::Router;

        let router: Router = Router::new()
            .route(
                "/api/llm/providers",
                get(list_providers).post(create_provider),
            )
            .route(
                "/api/llm/providers/{id}",
                get(get_provider).put(update_provider).delete(delete_provider),
            )
            .route(
                "/api/llm/providers/{providerId}/fetch-models",
                post(fetch_models),
            )
            .route("/api/llm/models", get(list_models).post(create_model))
            .route("/api/llm/models/{id}", put(update_model).delete(delete_model))
            .route(
                "/api/llm/user-setting",
                get(get_user_setting).post(save_user_setting),
            );

        let _ = router;
    }
}

