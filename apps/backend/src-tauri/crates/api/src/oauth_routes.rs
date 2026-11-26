//! OAuth 2.1 授权服务器端点（设计文档 §7）
//!
//! | 端点 | 认证 | 说明 |
//! |---|---|---|
//! | `GET /oauth/authorize` | Bearer JWT | 参数校验 + 返回同意信息（方案 A） |
//! | `POST /oauth/authorize` | Bearer JWT | 用户同意 / 拒绝，签发授权码 |
//! | `POST /oauth/token` | 客户端 | `authorization_code` / `refresh_token` |
//! | `POST /oauth/introspect` | 客户端 | RFC 7662 令牌内省 |
//! | `POST /oauth/revoke` | 客户端 | RFC 7009 撤销 |
//! | `GET /.well-known/oauth-authorization-server` | 无 | RFC 8414 元数据 |
//! | `/api/oauth-clients*` | Bearer JWT + perms | 客户端注册管理（不走动态注册） |
//!
//! **方案 A（Bearer + 同意页）说明**（§7.3）：本项目只有 Bearer JWT，没有会话
//! Cookie，因此 `GET /oauth/authorize` **不是**标准浏览器重定向流程 —— 它由已完成
//! 登录的内部应用携带 JWT 调用，服务端返回同意信息；用户在应用内确认后，
//! 应用再以 `POST /oauth/authorize` 提交决定并拿到 `redirect_to`。
//! 接外部第三方前需要补方案 B（`sys_user_session` + httpOnly Cookie）。
//!
//! **默认不挂载**（§16 T-7）：只有显式设置 `OAUTH_ENABLED=true`（或
//! `OAUTH_ALLOW_INSECURE_HTTP=true`）时，`create_oauth_routes()` 才会返回路由，
//! 因此桌面端不配置任何 `OAUTH_*` 变量也不会暴露 OAuth 端点。

use std::fmt::Write as _;

use axum::{
    extract::Query,
    http::{header::AUTHORIZATION, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Extension, Form, Json,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use serde::{Deserialize, Serialize};
use tracing::warn;

use service::oauth_service::{
    self, CreatedOauthClient, Introspection, NewOauthClient, OauthClientView, OauthTokenPair,
    UpdateOauthClient, ACCESS_TTL_DEFAULT, CLIENT_TYPE_CONFIDENTIAL, CLIENT_TYPE_PUBLIC,
    REFRESH_TTL_DEFAULT, SUPPORTED_GRANT_TYPES,
};

use super::auth::{AuthPrincipal, PrincipalKind};
use super::authz;
use super::oauth_error::OauthError;
use super::response::{ApiError, ApiResponse};

/// OAuth 端点开关（缺省关闭，见模块文档）
pub fn is_enabled() -> bool {
    fn truthy(value: &str) -> bool {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    }
    if let Ok(value) = std::env::var("OAUTH_ENABLED") {
        return truthy(&value);
    }
    std::env::var("OAUTH_ALLOW_INSECURE_HTTP")
        .map(|value| truthy(&value))
        .unwrap_or(false)
}

/// 令牌签发方标识（出现在 `iss` 与元数据里）
///
/// 未配置 `OAUTH_ISSUER` 时回落到 `http://localhost:<API_PORT>`；生产环境应显式配置。
pub fn oauth_issuer() -> String {
    if let Ok(issuer) = std::env::var("OAUTH_ISSUER") {
        let issuer = issuer.trim().trim_end_matches('/').to_string();
        if !issuer.is_empty() {
            return issuer;
        }
    }
    let port = std::env::var("API_PORT").unwrap_or_else(|_| "3001".to_string());
    format!("http://localhost:{}", port)
}

/// 明文传输告警（§8.2 补偿控制 2）：启动日志打印 WARN 横幅
pub fn warn_if_insecure_transport() {
    if !is_enabled() {
        return;
    }
    let issuer = oauth_issuer();
    if !issuer.starts_with("http://") {
        return;
    }
    if std::env::var("OAUTH_ALLOW_INSECURE_HTTP").is_ok() {
        warn!(
            issuer = %issuer,
            "OAUTH_ALLOW_INSECURE_HTTP 已启用：OAuth 端点运行在明文 HTTP 下，仅限内网 / 开发环境，\
             非生产可用（设计文档 §8.2）"
        );
    } else {
        warn!(
            issuer = %issuer,
            "OAuth 授权服务器 issuer 为明文 HTTP：建议由反向代理终止 TLS 并设置 OAUTH_ISSUER=https://..."
        );
    }
}

/// 当前进程 OAuth 端点的权限点前缀（供自检 / 文档使用）
pub const OAUTH_CLIENT_PERMS_PREFIX: &str = "system:oauthclient:";

/// 禁止写入 OAuth 客户端 allowed_scopes 的前缀（防提权，§6.6）
pub const OAUTH_FORBIDDEN_SCOPE_PREFIXES: &[&str] = &["system:", "upload:"];

/// URL 查询参数编码（unreserved 之外全部百分号编码）
fn encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(*byte as char);
            }
            other => {
                let _ = write!(&mut encoded, "%{:02X}", other);
            }
        }
    }
    encoded
}

/// `application/x-www-form-urlencoded` 解码（RFC 6749 §2.3.1 Basic 凭据用）
fn decode_component(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let high = (bytes[index + 1] as char).to_digit(16);
                let low = (bytes[index + 2] as char).to_digit(16);
                if let (Some(high), Some(low)) = (high, low) {
                    decoded.push((high * 16 + low) as u8);
                    index += 3;
                    continue;
                }
                decoded.push(bytes[index]);
            }
            b'+' => decoded.push(b' '),
            other => decoded.push(other),
        }
        index += 1;
    }
    String::from_utf8_lossy(&decoded).to_string()
}

/// 把参数追加到回调地址（保留已有查询串）
fn append_query(url: &str, params: &[(&str, &str)]) -> String {
    let separator = if url.contains('?') { '&' } else { '?' };
    let mut result = String::with_capacity(url.len() + 64);
    result.push_str(url);
    result.push(separator);
    for (index, (key, value)) in params.iter().enumerate() {
        if index > 0 {
            result.push('&');
        }
        let _ = write!(
            &mut result,
            "{}={}",
            encode_component(key),
            encode_component(value)
        );
    }
    result
}

// ============================================================================
// 客户端认证与令牌响应
// ============================================================================

/// 解析 `Authorization: Basic ...`（RFC 6749 §2.3.1）
///
/// 非 Basic 方案（例如 Bearer）返回 `Ok(None)`，交由调用方按「未提供客户端凭据」处理。
fn basic_credentials(headers: &HeaderMap) -> Result<Option<(String, String)>, OauthError> {
    let Some(raw) = headers.get(AUTHORIZATION) else {
        return Ok(None);
    };
    let raw = raw
        .to_str()
        .map_err(|_| OauthError::invalid_request("Authorization 头不是合法字符串"))?;
    let Some(encoded) = raw
        .strip_prefix("Basic ")
        .or_else(|| raw.strip_prefix("basic "))
    else {
        return Ok(None);
    };

    let decoded = BASE64_STANDARD
        .decode(encoded.trim())
        .map_err(|_| OauthError::invalid_client("client_secret_basic 编码非法"))?;
    let text = String::from_utf8(decoded)
        .map_err(|_| OauthError::invalid_client("client_secret_basic 编码非法"))?;
    let Some((client_id, client_secret)) = text.split_once(':') else {
        return Err(OauthError::invalid_client(
            "client_secret_basic 格式应为 client_id:client_secret",
        ));
    };
    Ok(Some((
        decode_component(client_id),
        decode_component(client_secret),
    )))
}

/// 从请求中取出客户端凭据并认证（Basic 优先，其次表单字段）
async fn authenticate_request_client(
    headers: &HeaderMap,
    form_client_id: Option<&str>,
    form_client_secret: Option<&str>,
) -> Result<database::models::SysOauthClient, OauthError> {
    let basic = basic_credentials(headers)?;

    // RFC 6749 §2.3：不得同时使用多种认证方式
    if basic.is_some() && (form_client_id.is_some() || form_client_secret.is_some()) {
        return Err(OauthError::invalid_request(
            "不得同时使用 client_secret_basic 与请求体凭据",
        ));
    }

    let (client_id, client_secret) = match basic {
        Some((client_id, secret)) => (client_id, Some(secret)),
        None => (
            form_client_id.unwrap_or_default().trim().to_string(),
            form_client_secret.map(str::to_string),
        ),
    };

    if client_id.is_empty() {
        return Err(OauthError::invalid_client("缺少 client_id"));
    }

    oauth_service::authenticate_client(&client_id, client_secret.as_deref())
        .await
        .map_err(OauthError::from_failure)
}

/// 令牌响应体（RFC 6749 §5.1）
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct OauthTokenResponse {
    pub access_token: String,
    /// 固定为 `Bearer`
    pub token_type: String,
    /// 访问令牌有效期（秒）
    pub expires_in: i64,
    pub refresh_token: String,
    /// 空格分隔的授权范围
    pub scope: String,
}

/// 组装令牌响应（`Cache-Control: no-store` / `Pragma: no-cache`，§7.4）
fn token_response(pair: OauthTokenPair) -> Response {
    let body = OauthTokenResponse {
        access_token: pair.access_token,
        token_type: "Bearer".to_string(),
        expires_in: pair.expires_in,
        refresh_token: pair.refresh_token,
        scope: oauth_service::join_scope(&pair.scopes),
    };
    let mut response = Json(body).into_response();
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
    headers.insert(
        axum::http::header::PRAGMA,
        HeaderValue::from_static("no-cache"),
    );
    headers.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    response
}


// ============================================================================
// /oauth/authorize（方案 A：Bearer JWT + 同意页）
// ============================================================================

/// `GET /oauth/authorize` 查询参数（全部 Option，由 `prepare_authorization` 统一校验）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AuthorizeQuery {
    pub client_id: Option<String>,
    pub redirect_uri: Option<String>,
    pub response_type: Option<String>,
    pub scope: Option<String>,
    pub state: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
}

/// 同意页展示信息（前端据此渲染确认弹窗）
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct AuthorizeConsent {
    pub client_id: String,
    /// 客户端名称（同意页标题）
    pub client_name: String,
    pub redirect_uri: String,
    /// 原样回传的 CSRF 随机串
    pub state: String,
    /// 申请的 scope 列表（逐条展示）
    pub scopes: Vec<String>,
    /// 空格分隔的 scope（POST 时原样回传）
    pub scope: String,
    /// 固定为 `S256`
    pub code_challenge_method: String,
    /// 授权服务器标识
    pub issuer: String,
}

/// `POST /oauth/authorize` 请求体（用户在同意页的最终决定）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AuthorizeDecisionRequest {
    pub client_id: String,
    pub redirect_uri: String,
    /// 缺省 `code`
    #[serde(default)]
    pub response_type: Option<String>,
    pub scope: String,
    pub state: String,
    pub code_challenge: String,
    pub code_challenge_method: Option<String>,
    /// `true` = 同意，`false` = 拒绝（返回 `access_denied`）
    pub approve: bool,
}

/// `POST /oauth/authorize` 响应：客户端据此跳转到 `redirect_to`
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct AuthorizeDecisionResponse {
    /// 带 `code` / `state` / `iss` 的完整回调地址（302 由调用方完成）
    pub redirect_to: String,
}

/// 端点要求**用户**身份（API Key / OAuth 令牌一律拒绝，§7.3 方案 A）
fn require_end_user(principal: &AuthPrincipal) -> Result<i64, OauthError> {
    if principal.kind != PrincipalKind::User {
        warn!(
            subject = %principal.subject,
            "非用户身份调用了 /oauth/authorize，拒绝"
        );
        return Err(OauthError::access_denied(
            "该端点需要用户登录态（方案 A 仅支持已登录的内部应用）",
        ));
    }
    principal
        .user_id
        .ok_or_else(|| OauthError::access_denied("缺少用户身份"))
}

/// 校验 authorize 参数并返回同意信息（**不签发**授权码）
#[utoipa::path(
    get,
    path = "/oauth/authorize",
    tag = "OAuth 2.1",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "同意页信息（需用户确认）", body = AuthorizeConsent),
        (status = 400, description = "参数非法", body = super::oauth_error::OauthErrorBody),
        (status = 401, description = "未认证"),
    )
)]
pub async fn authorize_consent(
    Extension(principal): Extension<AuthPrincipal>,
    Query(query): Query<AuthorizeQuery>,
) -> Result<Json<AuthorizeConsent>, OauthError> {
    require_end_user(&principal)?;

    let request = oauth_service::AuthorizeRequest {
        client_id: query.client_id.as_deref().unwrap_or_default(),
        redirect_uri: query.redirect_uri.as_deref().unwrap_or_default(),
        response_type: query.response_type.as_deref().unwrap_or_default(),
        scope: query.scope.as_deref().unwrap_or_default(),
        state: query.state.as_deref().unwrap_or_default(),
        code_challenge: query.code_challenge.as_deref().unwrap_or_default(),
        code_challenge_method: query.code_challenge_method.as_deref(),
    };

    let grant = oauth_service::prepare_authorization(&request)
        .await
        .map_err(OauthError::from_failure)?;

    Ok(Json(AuthorizeConsent {
        client_id: grant.client.client_id.clone(),
        client_name: grant.client.client_name.clone(),
        redirect_uri: grant.redirect_uri.clone(),
        state: grant.state.clone(),
        scope: oauth_service::join_scope(&grant.scopes),
        scopes: grant.scopes.clone(),
        code_challenge_method: "S256".to_string(),
        issuer: oauth_issuer(),
    }))
}

/// 用户同意 / 拒绝授权，同意时签发一次性授权码并返回回调地址
#[utoipa::path(
    post,
    path = "/oauth/authorize",
    tag = "OAuth 2.1",
    security(("bearer_auth" = [])),
    request_body = AuthorizeDecisionRequest,
    responses(
        (status = 200, description = "授权码已签发，返回带 code/state/iss 的回调地址", body = AuthorizeDecisionResponse),
        (status = 400, description = "参数非法 / 用户拒绝授权", body = super::oauth_error::OauthErrorBody),
        (status = 401, description = "未认证"),
    )
)]
pub async fn authorize_decision(
    Extension(principal): Extension<AuthPrincipal>,
    Json(body): Json<AuthorizeDecisionRequest>,
) -> Result<Json<AuthorizeDecisionResponse>, OauthError> {
    let user_id = require_end_user(&principal)?;

    if !body.approve {
        return Err(OauthError::access_denied("用户拒绝了授权请求"));
    }

    let request = oauth_service::AuthorizeRequest {
        client_id: &body.client_id,
        redirect_uri: &body.redirect_uri,
        response_type: body.response_type.as_deref().unwrap_or_default(),
        scope: &body.scope,
        state: &body.state,
        code_challenge: &body.code_challenge,
        code_challenge_method: body.code_challenge_method.as_deref(),
    };

    let grant = oauth_service::prepare_authorization(&request)
        .await
        .map_err(OauthError::from_failure)?;

    let code = oauth_service::issue_authorization_code(&grant, user_id, principal.tenant_id)
        .await
        .map_err(OauthError::from_failure)?;

    let issuer = oauth_issuer();
    let redirect_to = append_query(
        &grant.redirect_uri,
        &[("code", &code), ("state", &grant.state), ("iss", &issuer)],
    );

    Ok(Json(AuthorizeDecisionResponse { redirect_to }))
}


// ============================================================================
// /oauth/token、/oauth/introspect、/oauth/revoke、元数据
// ============================================================================

/// `/oauth/token` 表单（`application/x-www-form-urlencoded`，§7.4）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TokenRequest {
    pub grant_type: Option<String>,
    pub code: Option<String>,
    pub redirect_uri: Option<String>,
    pub code_verifier: Option<String>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

/// 令牌端点：授权码交换 / 刷新轮换
#[utoipa::path(
    post,
    path = "/oauth/token",
    tag = "OAuth 2.1",
    responses(
        (status = 200, description = "令牌对（`Cache-Control: no-store`）", body = OauthTokenResponse),
        (status = 400, description = "invalid_request / invalid_grant / invalid_scope / unsupported_grant_type", body = super::oauth_error::OauthErrorBody),
        (status = 401, description = "invalid_client", body = super::oauth_error::OauthErrorBody),
    )
)]
pub async fn token(
    headers: HeaderMap,
    Form(form): Form<TokenRequest>,
) -> Result<Response, OauthError> {
    let client = authenticate_request_client(
        &headers,
        form.client_id.as_deref(),
        form.client_secret.as_deref(),
    )
    .await?;

    let grant_type = form.grant_type.as_deref().unwrap_or_default().trim();
    match grant_type {
        "authorization_code" => {
            let pair = oauth_service::exchange_authorization_code(
                &client,
                form.code.as_deref().unwrap_or_default(),
                form.redirect_uri.as_deref().unwrap_or_default(),
                form.code_verifier.as_deref().unwrap_or_default(),
                form.scope.as_deref(),
            )
            .await
            .map_err(OauthError::from_failure)?;
            Ok(token_response(pair))
        }
        "refresh_token" => {
            let pair = oauth_service::refresh_access_token(
                &client,
                form.refresh_token.as_deref().unwrap_or_default(),
                form.scope.as_deref(),
            )
            .await
            .map_err(OauthError::from_failure)?;
            Ok(token_response(pair))
        }
        other => Err(OauthError::unsupported_grant_type(format!(
            "不支持的 grant_type: {}（仅支持 {}）",
            if other.is_empty() { "<空>" } else { other },
            SUPPORTED_GRANT_TYPES.join(" / ")
        ))),
    }
}

/// `/oauth/introspect` 表单（RFC 7662）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct IntrospectRequest {
    pub token: Option<String>,
    pub token_type_hint: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

/// 令牌内省：只允许 confidential 客户端（资源服务）调用
#[utoipa::path(
    post,
    path = "/oauth/introspect",
    tag = "OAuth 2.1",
    responses(
        (status = 200, description = "内省结果（无效令牌返回 active=false）", body = Introspection),
        (status = 401, description = "invalid_client", body = super::oauth_error::OauthErrorBody),
    )
)]
pub async fn introspect(
    headers: HeaderMap,
    Form(form): Form<IntrospectRequest>,
) -> Result<Json<Introspection>, OauthError> {
    let client = authenticate_request_client(
        &headers,
        form.client_id.as_deref(),
        form.client_secret.as_deref(),
    )
    .await?;

    if client.client_type != CLIENT_TYPE_CONFIDENTIAL {
        return Err(OauthError::invalid_client(
            "内省端点仅限 confidential 客户端（资源服务）调用",
        ));
    }

    let introspection = oauth_service::introspect(
        &client.client_id,
        form.token.as_deref().unwrap_or_default(),
    )
    .await
    .map_err(OauthError::server_error)?;

    Ok(Json(introspection))
}

/// `/oauth/revoke` 表单（RFC 7009）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RevokeRequest {
    pub token: Option<String>,
    pub token_type_hint: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

/// 撤销令牌：无论令牌是否存在都返回 200（RFC 7009 §2.2）
#[utoipa::path(
    post,
    path = "/oauth/revoke",
    tag = "OAuth 2.1",
    responses(
        (status = 200, description = "已处理（不区分令牌是否存在）"),
        (status = 401, description = "invalid_client", body = super::oauth_error::OauthErrorBody),
    )
)]
pub async fn revoke(
    headers: HeaderMap,
    Form(form): Form<RevokeRequest>,
) -> Result<StatusCode, OauthError> {
    let client = authenticate_request_client(
        &headers,
        form.client_id.as_deref(),
        form.client_secret.as_deref(),
    )
    .await?;

    oauth_service::revoke_token(
        &client.client_id,
        form.token.as_deref().unwrap_or_default(),
    )
    .await
    .map_err(OauthError::server_error)?;

    Ok(StatusCode::OK)
}

/// 授权服务器元数据（RFC 8414）
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct OauthMetadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub introspection_endpoint: String,
    pub revocation_endpoint: String,
    pub response_types_supported: Vec<String>,
    pub grant_types_supported: Vec<String>,
    pub code_challenge_methods_supported: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
}

/// `GET /.well-known/oauth-authorization-server`
#[utoipa::path(
    get,
    path = "/.well-known/oauth-authorization-server",
    tag = "OAuth 2.1",
    responses(
        (status = 200, description = "授权服务器元数据（RFC 8414）", body = OauthMetadata),
    )
)]
pub async fn metadata() -> Json<OauthMetadata> {
    let issuer = oauth_issuer();
    Json(OauthMetadata {
        authorization_endpoint: format!("{}/oauth/authorize", issuer),
        token_endpoint: format!("{}/oauth/token", issuer),
        introspection_endpoint: format!("{}/oauth/introspect", issuer),
        revocation_endpoint: format!("{}/oauth/revoke", issuer),
        response_types_supported: vec!["code".to_string()],
        grant_types_supported: SUPPORTED_GRANT_TYPES
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        // 设计文档 §7.3：只支持 S256，显式不宣告 plain
        code_challenge_methods_supported: vec!["S256".to_string()],
        token_endpoint_auth_methods_supported: vec![
            "client_secret_basic".to_string(),
            "client_secret_post".to_string(),
            "none".to_string(),
        ],
        issuer,
    })
}

/// 公开的 OAuth 端点（不经过 `auth_middleware`，客户端自己认证）
///
/// 只在 [`is_enabled()`] 为真时由调用方挂载（§16 T-7：缺省不暴露）。
///
/// 泛型 `S` 是 axum 的 state 类型：调用方（`api::create_router`）的受保护路由
/// 使用 `Arc<PgPool>` 作为 state，这里保持同类型才能 `merge`。
pub fn public_oauth_routes<S>() -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    use axum::routing::{get, post};

    axum::Router::new()
        .route("/oauth/token", post(token))
        .route("/oauth/introspect", post(introspect))
        .route("/oauth/revoke", post(revoke))
        .route("/.well-known/oauth-authorization-server", get(metadata))
}


// ============================================================================
// 客户端注册管理（/api/oauth-clients*，JWT + system:oauthclient:*）
// ============================================================================

/// 创建客户端请求
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateOauthClientRequest {
    pub client_name: String,
    /// 1 = confidential（有密钥），2 = public（无密钥、强制 PKCE）；缺省 2
    pub client_type: Option<i16>,
    /// 至少 1 条；https，或 http + 回环地址
    pub redirect_uris: Vec<String>,
    /// 允许申请的 scope（必须 ⊆ 创建者权限，且不能含 system:* / upload:*）
    pub allowed_scopes: Vec<String>,
    pub grant_types: Option<Vec<String>>,
    /// 访问令牌有效期（秒，默认 900）
    pub access_token_ttl: Option<i32>,
    /// 刷新令牌有效期（秒，默认 30 天）
    pub refresh_token_ttl: Option<i32>,
}

/// 更新客户端请求（`None` = 保持不变）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateOauthClientRequest {
    pub client_name: Option<String>,
    pub redirect_uris: Option<Vec<String>>,
    pub allowed_scopes: Option<Vec<String>>,
    pub grant_types: Option<Vec<String>>,
    pub access_token_ttl: Option<i32>,
    pub refresh_token_ttl: Option<i32>,
    /// `false` = 禁用（同时撤销该客户端全部令牌）
    pub enable: Option<bool>,
}

/// 管理面：必须是用户身份 JWT（API Key / OAuth 令牌一律 403）
fn ensure_user_principal(principal: &AuthPrincipal) -> Result<(), ApiError> {
    if principal.kind != PrincipalKind::User {
        warn!(
            subject = %principal.subject,
            "非用户身份调用 OAuth 客户端管理接口，拒绝"
        );
        return Err(ApiError::forbidden(
            "OAuth 客户端管理仅允许管理员 JWT 调用",
        ));
    }
    Ok(())
}

/// 防提权：allowed_scopes 不得包含管理面（`system:*`）与上传流（`upload:*`）
fn ensure_scope_policy(principal: &AuthPrincipal, scopes: &[String]) -> Result<(), ApiError> {
    for scope in scopes {
        if OAUTH_FORBIDDEN_SCOPE_PREFIXES
            .iter()
            .any(|prefix| scope.starts_with(prefix))
        {
            return Err(ApiError::bad_request(format!(
                "scope 不允许包含管理面权限点: {}",
                scope
            )));
        }
    }
    ensure_scope_subset(principal, scopes)
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

/// 解析路径中的客户端主键
fn parse_client_id(id: &str) -> Result<i64, ApiError> {
    id.trim()
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("客户端 ID 非法"))
}

/// 租户隔离：非超管只能操作本租户的客户端（越权一律 404，不泄露存在性）
async fn ensure_client_in_tenant(
    principal: &AuthPrincipal,
    id: i64,
) -> Result<database::models::SysOauthClient, ApiError> {
    let client = oauth_service::get_client_by_id(id)
        .await
        .map_err(ApiError::internal_error)?
        .ok_or_else(|| ApiError::not_found("OAuth 客户端不存在"))?;

    if !principal.is_super_admin && client.tenant_id != principal.tenant_id {
        return Err(ApiError::not_found("OAuth 客户端不存在"));
    }
    Ok(client)
}


/// OAuth 客户端列表（超管看全部，其余只看本租户）
#[utoipa::path(
    get,
    path = "/api/oauth-clients",
    tag = "OAuth 2.1",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "客户端列表（不含密钥哈希）", body = ApiResponse<Vec<OauthClientView>>),
        (status = 401, description = "未认证", body = ApiError),
        (status = 403, description = "权限不足", body = ApiError),
    )
)]
pub async fn list_oauth_clients(
    Extension(principal): Extension<AuthPrincipal>,
) -> Result<Json<ApiResponse<Vec<OauthClientView>>>, ApiError> {
    ensure_user_principal(&principal)?;

    let tenant_filter = if principal.is_super_admin {
        None
    } else {
        match principal.tenant_id {
            Some(tenant_id) => Some(tenant_id),
            None => return Ok(Json(ApiResponse::success(Vec::new()))),
        }
    };

    let clients = oauth_service::list_clients(tenant_filter)
        .await
        .map_err(ApiError::internal_error)?;

    Ok(Json(ApiResponse::success(clients)))
}

/// 创建 OAuth 客户端（`client_secret` 明文只返回一次）
#[utoipa::path(
    post,
    path = "/api/oauth-clients",
    tag = "OAuth 2.1",
    security(("bearer_auth" = [])),
    request_body = CreateOauthClientRequest,
    responses(
        (status = 200, description = "创建成功（含一次性客户端密钥）", body = ApiResponse<CreatedOauthClient>),
        (status = 400, description = "参数错误 / scope 超范围", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 403, description = "权限不足", body = ApiError),
    )
)]
pub async fn create_oauth_client(
    Extension(principal): Extension<AuthPrincipal>,
    Json(req): Json<CreateOauthClientRequest>,
) -> Result<Json<ApiResponse<CreatedOauthClient>>, ApiError> {
    ensure_user_principal(&principal)?;
    ensure_scope_policy(&principal, &req.allowed_scopes)?;

    let created = oauth_service::create_client(&NewOauthClient {
        client_name: &req.client_name,
        client_type: req.client_type.unwrap_or(CLIENT_TYPE_PUBLIC),
        redirect_uris: &req.redirect_uris,
        allowed_scopes: &req.allowed_scopes,
        grant_types: req.grant_types.as_deref(),
        access_token_ttl: Some(req.access_token_ttl.unwrap_or(ACCESS_TTL_DEFAULT)),
        refresh_token_ttl: Some(req.refresh_token_ttl.unwrap_or(REFRESH_TTL_DEFAULT)),
        tenant_id: principal.tenant_id,
        created_by: principal.user_id,
    })
    .await
    .map_err(ApiError::bad_request)?;

    Ok(Json(ApiResponse::success(created)))
}

/// 更新 OAuth 客户端（禁用会同时撤销其全部令牌）
#[utoipa::path(
    put,
    path = "/api/oauth-clients/{id}",
    tag = "OAuth 2.1",
    security(("bearer_auth" = [])),
    request_body = UpdateOauthClientRequest,
    responses(
        (status = 200, description = "更新后的客户端", body = ApiResponse<OauthClientView>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 403, description = "权限不足", body = ApiError),
        (status = 404, description = "客户端不存在", body = ApiError),
    )
)]
pub async fn update_oauth_client(
    Extension(principal): Extension<AuthPrincipal>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<UpdateOauthClientRequest>,
) -> Result<Json<ApiResponse<OauthClientView>>, ApiError> {
    ensure_user_principal(&principal)?;
    let id = parse_client_id(&id)?;
    ensure_client_in_tenant(&principal, id).await?;

    if let Some(scopes) = req.allowed_scopes.as_deref() {
        ensure_scope_policy(&principal, scopes)?;
    }

    let updated = oauth_service::update_client(
        id,
        &UpdateOauthClient {
            client_name: req.client_name.as_deref(),
            redirect_uris: req.redirect_uris.as_deref(),
            allowed_scopes: req.allowed_scopes.as_deref(),
            grant_types: req.grant_types.as_deref(),
            access_token_ttl: req.access_token_ttl,
            refresh_token_ttl: req.refresh_token_ttl,
            enable: req.enable,
            updated_by: principal.user_id,
        },
    )
    .await
    .map_err(ApiError::bad_request)?;

    Ok(Json(ApiResponse::success(updated)))
}

/// 删除 OAuth 客户端（软删除 + 撤销其全部令牌）
#[utoipa::path(
    delete,
    path = "/api/oauth-clients/{id}",
    tag = "OAuth 2.1",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "已删除（`data` 为 null）"),
        (status = 401, description = "未认证", body = ApiError),
        (status = 403, description = "权限不足", body = ApiError),
        (status = 404, description = "客户端不存在", body = ApiError),
    )
)]
pub async fn delete_oauth_client(
    Extension(principal): Extension<AuthPrincipal>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    ensure_user_principal(&principal)?;
    let id = parse_client_id(&id)?;
    ensure_client_in_tenant(&principal, id).await?;

    oauth_service::delete_client(id, principal.user_id)
        .await
        .map_err(ApiError::bad_request)?;

    Ok(Json(ApiResponse::success(())))
}


#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue as HttpHeaderValue;

    fn basic_header(client_id: &str, secret: &str) -> HeaderMap {
        let raw = format!("{}:{}", client_id, secret);
        let encoded = BASE64_STANDARD.encode(raw.as_bytes());
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HttpHeaderValue::from_str(&format!("Basic {}", encoded)).expect("头部应合法"),
        );
        headers
    }

    #[test]
    fn query_encoding_escapes_reserved_bytes() {
        assert_eq!(encode_component("a:b c/d?e&f"), "a%3Ab%20c%2Fd%3Fe%26f");
        // unreserved 字符不转义
        assert_eq!(encode_component("Az09-._~"), "Az09-._~");
        // 中文按 UTF-8 逐字节编码
        assert_eq!(encode_component("资"), "%E8%B5%84");
    }

    #[test]
    fn append_query_keeps_existing_query_string() {
        assert_eq!(
            append_query("https://app.example.com/cb", &[("code", "abc"), ("state", "x y")]),
            "https://app.example.com/cb?code=abc&state=x%20y"
        );
        assert_eq!(
            append_query("https://app.example.com/cb?a=1", &[("code", "abc")]),
            "https://app.example.com/cb?a=1&code=abc"
        );
    }

    #[test]
    fn basic_credentials_are_percent_decoded() {
        let headers = basic_header("oc_test", "sec%20ret");
        let (client_id, secret) = basic_credentials(&headers)
            .expect("Basic 头应解析成功")
            .expect("应取到凭据");
        assert_eq!(client_id, "oc_test");
        // RFC 6749 §2.3.1：Basic 里的凭据先 form-urlencoded 再 base64
        assert_eq!(secret, "sec ret");
    }

    #[test]
    fn bearer_header_is_not_treated_as_client_credentials() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HttpHeaderValue::from_static("Bearer eyJhbGciOiJIUzI1NiJ9.payload.sig"),
        );
        assert!(
            basic_credentials(&headers)
                .expect("不应报错")
                .is_none(),
            "Bearer 头不应被当成 client_secret_basic"
        );
    }

    #[test]
    fn malformed_basic_header_is_invalid_client() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HttpHeaderValue::from_static("Basic not-base64!!"),
        );
        let error = basic_credentials(&headers).expect_err("非法 base64 必须报错");
        assert_eq!(error.body.error, "invalid_client");
    }

    #[tokio::test]
    async fn metadata_advertises_code_and_s256_only() {
        let metadata = metadata().await.0;
        assert_eq!(metadata.response_types_supported, vec!["code".to_string()]);
        assert_eq!(
            metadata.code_challenge_methods_supported,
            vec!["S256".to_string()],
            "不得宣告 plain（§7.3）"
        );
        assert!(metadata
            .grant_types_supported
            .contains(&"authorization_code".to_string()));
        assert!(metadata
            .grant_types_supported
            .contains(&"refresh_token".to_string()));
        assert!(metadata.token_endpoint.ends_with("/oauth/token"));
        assert!(metadata
            .authorization_endpoint
            .ends_with("/oauth/authorize"));
    }
}

