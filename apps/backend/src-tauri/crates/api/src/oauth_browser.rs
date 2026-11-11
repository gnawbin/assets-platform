//! OAuth 2.1 授权端点的**浏览器流程**（方案 B，设计文档 §7.3）
//!
//! 同一个 URL `/oauth/authorize` 同时服务两种调用方，按请求特征分流：
//!
//! | 请求 | 分流依据 | 结果 |
//! |---|---|---|
//! | `GET` + `Authorization: Bearer <JWT>` | 有 Bearer 头 | JSON（方案 A，内部应用） |
//! | `GET` + 会话 Cookie | 无 Bearer 头 | HTML 同意页（方案 B，浏览器） |
//! | `POST` + `Content-Type: application/json` | JSON 体 | JSON（方案 A） |
//! | `POST` + 表单体 | 其它 | 校验 CSRF → 302 回调（方案 B） |
//!
//! 之所以把两条流程合并在一个路径上，是因为 OAuth 2.1 规定授权端点的标准形态是
//! 「浏览器 GET → 同意 → 302」，第三方客户端只会用这一种；而内部应用仍需要
//! 纯 JSON 的调用方式。分流放在 handler 内部而不是路由层，避免为同一路径注册两次
//! 路由（axum 会 panic）。
//!
//! 安全基线：
//! - 会话令牌只存 SHA-256，Cookie 为 `HttpOnly; SameSite=Lax`；
//! - 同意页表单校验双提交 CSRF（`ap_csrf` Cookie vs 隐藏域，常数时间比较）；
//! - 所有 HTML 插值经 `oauth_session::html_escape`，并附 `CSP` / `X-Frame-Options`。

use axum::{
    body::Bytes,
    extract::Query,
    http::{
        header::{self, AUTHORIZATION, CONTENT_TYPE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
    Extension, Json,
};

use service::oauth_service;
use service::session_service::{self, UserSession};

use super::auth::{self, AuthPrincipal, Credential, PrincipalKind};
use super::oauth_error::OauthError;
use super::oauth_routes::{AuthorizeDecisionRequest, AuthorizeQuery};
use super::oauth_session::{
    self, clear_cookie, clear_csrf_cookie, csrf_matches, parse_form_body, read_cookie,
    render_consent_page, render_error_page, CSRF_COOKIE_NAME, SESSION_COOKIE_NAME,
};

/// 同意页 / 错误页统一的安全头（无内联脚本，允许内联样式）
const HTML_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; img-src 'self'; \
                        form-action 'self'; base-uri 'none'";

/// 构造 HTML 响应（含安全头与 `no-store`）
fn html_response(status: StatusCode, html: String, cookies: Vec<String>) -> Response {
    let mut response = (status, html).into_response();
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(HTML_CSP),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    for cookie in cookies {
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            headers.append(header::SET_COOKIE, value);
        }
    }
    response
}

/// 302 跳转到客户端回调地址（禁止缓存，避免授权码泄露给中间缓存）
fn redirect_response(location: &str) -> Response {
    let Ok(value) = HeaderValue::from_str(location) else {
        return html_response(
            StatusCode::BAD_REQUEST,
            render_error_page("跳转地址非法", "回调地址包含非法字符"),
            Vec::new(),
        );
    };
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert(header::LOCATION, value);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache"),
    );
    response
}

/// 是否按 JSON（方案 A）处理：显式 JSON 体，或带了 Authorization 头
fn wants_json(headers: &HeaderMap, has_json_body: bool) -> bool {
    has_json_body || headers.contains_key(AUTHORIZATION)
}

/// 查询串编码（unreserved 之外全部百分号编码）
fn urlencode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(*byte as char)
            }
            other => encoded.push_str(&format!("%{:02X}", other)),
        }
    }
    encoded
}

/// 构造「拒绝授权」的回调地址（`error=access_denied`，保留 `state`）
pub fn deny_redirect(redirect_uri: &str, state: &str) -> String {
    let separator = if redirect_uri.contains('?') { '&' } else { '?' };
    if state.is_empty() {
        format!("{}{}error=access_denied", redirect_uri, separator)
    } else {
        format!(
            "{}{}error=access_denied&state={}",
            redirect_uri,
            separator,
            urlencode(state)
        )
    }
}

/// 方案 A：从 `Authorization` 头解析用户主体（只接受用户 JWT）
async fn bearer_principal(headers: &HeaderMap) -> Result<AuthPrincipal, OauthError> {
    let raw = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| OauthError::invalid_request("缺少 Authorization 请求头"))?;
    let token = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))
        .ok_or_else(|| OauthError::invalid_request("Authorization 格式应为 Bearer <token>"))?;

    match auth::parse_credential(token.trim()) {
        Ok(Credential::Jwt(jwt)) => {
            let claims = auth::verify_token(&jwt)
                .map_err(|_| OauthError::new("invalid_token", "访问令牌无效或已过期"))?;
            let authority = auth::user_authority(claims.sub).await;
            let tenant_id = database::postgres::get_user_tenant_cache()
                .get(&claims.sub)
                .map(|entry| *entry);
            Ok(AuthPrincipal {
                kind: PrincipalKind::User,
                subject: format!("u:{}", claims.sub),
                user_id: Some(claims.sub),
                api_key_id: None,
                tenant_id,
                schema_name: "public".to_string(),
                scopes: authority.scopes,
                is_super_admin: authority.is_super_admin,
                rate_limit_per_min: None,
            })
        }
        Ok(_) => Err(OauthError::access_denied(
            "该端点需要用户登录态（API Key / OAuth 令牌不可用于授权码流程）",
        )),
        Err(_) => Err(OauthError::new("invalid_token", "凭证无效或已过期")),
    }
}

/// 方案 B：从 Cookie 读取并校验会话（同时确认账号仍可用）
async fn cookie_session(headers: &HeaderMap) -> Result<Option<UserSession>, OauthError> {
    let Some(raw) = read_cookie(headers, SESSION_COOKIE_NAME) else {
        return Ok(None);
    };
    let session = session_service::find_session(&raw)
        .await
        .map_err(|e| {
            tracing::warn!("读取浏览器会话失败: {}", e);
            OauthError::server_error("会话校验服务暂时不可用")
        })?;
    let Some(session) = session else {
        return Ok(None);
    };
    oauth_service::ensure_user_active(session.user_id)
        .await
        .map_err(OauthError::from_failure)?;
    Ok(Some(session))
}

/// 把 `AuthorizeQuery` 转成同意页隐藏域用的参数表
fn consent_params(query: &AuthorizeQuery) -> std::collections::HashMap<String, String> {
    let mut params = std::collections::HashMap::new();
    let mut insert = |key: &str, value: &Option<String>| {
        if let Some(value) = value {
            if !value.is_empty() {
                params.insert(key.to_string(), value.clone());
            }
        }
    };
    insert("response_type", &query.response_type);
    insert("client_id", &query.client_id);
    insert("redirect_uri", &query.redirect_uri);
    insert("scope", &query.scope);
    insert("state", &query.state);
    insert("code_challenge", &query.code_challenge);
    insert("code_challenge_method", &query.code_challenge_method);
    params
}

/// 从 `OauthError` 里取安全的人类可读描述（不含堆栈 / SQL）
fn describe(error: &OauthError) -> String {
    error
        .body
        .error_description
        .clone()
        .unwrap_or_else(|| error.body.error.clone())
}

/// `GET /oauth/authorize`
///
/// - 带 `Authorization` → 方案 A：返回 JSON 同意信息（需登录的内部应用）
/// - 无 `Authorization` → 方案 B：用会话 Cookie 渲染 HTML 同意页
pub async fn authorize_page(headers: HeaderMap, Query(query): Query<AuthorizeQuery>) -> Response {
    if wants_json(&headers, false) {
        let principal = match bearer_principal(&headers).await {
            Ok(principal) => principal,
            Err(error) => return error.into_response(),
        };
        return super::oauth_routes::authorize_consent(Extension(principal), Query(query))
            .await
            .into_response();
    }

    // ---------- 方案 B：必须有浏览器会话 ----------
    let session = match cookie_session(&headers).await {
        Ok(Some(session)) => session,
        Ok(None) => {
            return html_response(
                StatusCode::UNAUTHORIZED,
                render_error_page(
                    "需要登录",
                    "请先在浏览器中登录本系统，然后再重新打开该授权链接。",
                ),
                vec![clear_cookie(SESSION_COOKIE_NAME)],
            );
        }
        Err(error) => {
            return html_response(
                error.status,
                render_error_page("授权请求失败", &describe(&error)),
                Vec::new(),
            );
        }
    };

    let request = oauth_service::AuthorizeRequest {
        client_id: query.client_id.as_deref().unwrap_or_default(),
        redirect_uri: query.redirect_uri.as_deref().unwrap_or_default(),
        response_type: query.response_type.as_deref().unwrap_or_default(),
        scope: query.scope.as_deref().unwrap_or_default(),
        state: query.state.as_deref().unwrap_or_default(),
        code_challenge: query.code_challenge.as_deref().unwrap_or_default(),
        code_challenge_method: query.code_challenge_method.as_deref(),
    };

    let grant = match oauth_service::prepare_authorization(&request).await {
        Ok(grant) => grant,
        Err(failure) => {
            let error = OauthError::from_failure(failure);
            return html_response(
                error.status,
                render_error_page("授权请求不合法", &describe(&error)),
                Vec::new(),
            );
        }
    };

    // 双提交 CSRF：CSRF 令牌必须与**本会话**绑定；旧 Cookie 失配时现场重签并回写
    let mut cookies = Vec::new();
    let csrf_token = match read_cookie(&headers, CSRF_COOKIE_NAME) {
        Some(token)
            if utils::oauth::constant_time_eq(
                &utils::oauth::hash_secret(&token),
                &session.csrf_hash,
            ) =>
        {
            token
        }
        _ => {
            let token = utils::oauth::generate_csrf_token();
            if let Err(e) = session_service::rotate_csrf_token(session.id, &token).await {
                tracing::warn!("重签会话 CSRF 令牌失败: {}", e);
            }
            cookies.push(oauth_session::build_csrf_cookie(
                &token,
                session_service::session_ttl_secs(),
            ));
            token
        }
    };

    tracing::info!(
        client_id = %grant.client.client_id,
        user_id = session.user_id,
        "渲染 OAuth 同意页（浏览器流程）"
    );

    html_response(
        StatusCode::OK,
        render_consent_page(
            &grant.client.client_name,
            &grant.client.client_id,
            &grant.scopes,
            &consent_params(&query),
            &csrf_token,
        ),
        cookies,
    )
}

/// 从表单构造 `AuthorizeDecisionRequest`
fn decision_from_form(
    form: &std::collections::HashMap<String, String>,
) -> AuthorizeDecisionRequest {
    AuthorizeDecisionRequest {
        client_id: form.get("client_id").cloned().unwrap_or_default(),
        redirect_uri: form.get("redirect_uri").cloned().unwrap_or_default(),
        response_type: form.get("response_type").cloned(),
        scope: form.get("scope").cloned().unwrap_or_default(),
        state: form.get("state").cloned().unwrap_or_default(),
        code_challenge: form.get("code_challenge").cloned().unwrap_or_default(),
        code_challenge_method: form.get("code_challenge_method").cloned(),
        approve: form.get("decision").map(String::as_str) == Some("approve"),
    }
}

/// `POST /oauth/authorize`
///
/// JSON 体 → 方案 A（Bearer JWT，返回 `redirect_to`）；
/// 表单体 → 方案 B（会话 Cookie + CSRF 双提交，直接 302 回调）。
pub async fn authorize_submit(headers: HeaderMap, body: Bytes) -> Response {
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_json = content_type.contains("application/json");
    let text = String::from_utf8_lossy(&body).into_owned();

    if wants_json(&headers, is_json) {
        let principal = match bearer_principal(&headers).await {
            Ok(principal) => principal,
            Err(error) => return error.into_response(),
        };
        let request: AuthorizeDecisionRequest = match serde_json::from_str(&text) {
            Ok(request) => request,
            Err(_) => {
                return OauthError::invalid_request("请求体不是合法的授权决定 JSON")
                    .into_response()
            }
        };
        return super::oauth_routes::authorize_decision(Extension(principal), Json(request))
            .await
            .into_response();
    }

    // ---------- 方案 B：表单 + 会话 Cookie ----------
    let form = parse_form_body(&text);
    let csrf_cookie = read_cookie(&headers, CSRF_COOKIE_NAME);
    if !csrf_matches(csrf_cookie.as_deref(), form.get("csrf_token").map(String::as_str)) {
        tracing::warn!("同意页 CSRF 校验失败，拒绝提交");
        return html_response(
            StatusCode::FORBIDDEN,
            render_error_page(
                "请求已过期",
                "页面已过期或来源不可信，请重新打开授权链接。",
            ),
            vec![clear_csrf_cookie()],
        );
    }

    let session = match cookie_session(&headers).await {
        Ok(Some(session)) => session,
        Ok(None) => {
            return html_response(
                StatusCode::UNAUTHORIZED,
                render_error_page("会话已失效", "登录状态已过期，请重新登录后再授权。"),
                vec![clear_cookie(SESSION_COOKIE_NAME)],
            );
        }
        Err(error) => {
            return html_response(
                error.status,
                render_error_page("授权请求失败", &describe(&error)),
                Vec::new(),
            );
        }
    };

    // 第二道 CSRF 校验：令牌必须属于**本会话**（防 Cookie 注入 / 跨会话复用）
    let csrf_form_token = form
        .get("csrf_token")
        .map(String::as_str)
        .unwrap_or_default();
    if !utils::oauth::constant_time_eq(&utils::oauth::hash_secret(csrf_form_token), &session.csrf_hash)
    {
        tracing::warn!(
            user_id = session.user_id,
            "同意页 CSRF 令牌与会话不匹配，拒绝提交"
        );
        return html_response(
            StatusCode::FORBIDDEN,
            render_error_page("请求已过期", "页面已过期，请重新打开授权链接。"),
            Vec::new(),
        );
    }

    let decision = decision_from_form(&form);

    // 先校验参数（含 redirect_uri 精确匹配），校验通过后才可以跳转
    let request = oauth_service::AuthorizeRequest {
        client_id: &decision.client_id,
        redirect_uri: &decision.redirect_uri,
        response_type: decision.response_type.as_deref().unwrap_or_default(),
        scope: &decision.scope,
        state: &decision.state,
        code_challenge: &decision.code_challenge,
        code_challenge_method: decision.code_challenge_method.as_deref(),
    };
    let grant = match oauth_service::prepare_authorization(&request).await {
        Ok(grant) => grant,
        Err(failure) => {
            let error = OauthError::from_failure(failure);
            return html_response(
                error.status,
                render_error_page("授权请求不合法", &describe(&error)),
                Vec::new(),
            );
        }
    };

    if !decision.approve {
        tracing::info!(
            client_id = %grant.client.client_id,
            user_id = session.user_id,
            "用户在同意页拒绝授权"
        );
        return redirect_response(&deny_redirect(&grant.redirect_uri, &grant.state));
    }

    let principal = AuthPrincipal {
        kind: PrincipalKind::User,
        subject: format!("u:{}", session.user_id),
        user_id: Some(session.user_id),
        api_key_id: None,
        tenant_id: session.tenant_id,
        schema_name: "public".to_string(),
        scopes: Vec::new(),
        is_super_admin: false,
        rate_limit_per_min: None,
    };

    match super::oauth_routes::authorize_decision(Extension(principal), Json(decision)).await {
        Ok(Json(response)) => redirect_response(&response.redirect_to),
        Err(error) => html_response(
            error.status,
            render_error_page("授权失败", &describe(&error)),
            Vec::new(),
        ),
    }
}

/// 登录成功后下发会话 Cookie（会话签发失败**不能**影响登录本身）
///
/// 只有启用 OAuth（`OAUTH_ENABLED` / `OAUTH_ALLOW_INSECURE_HTTP`）时才签发：
/// 未启用时端点不存在，写会话只会留下无用的数据库行（§16 T-7 缺省即安全）。
pub async fn issue_session_cookies(
    headers: &HeaderMap,
    user_id: i64,
    tenant_id: Option<i64>,
) -> Vec<String> {
    if !super::oauth_routes::is_enabled() {
        return Vec::new();
    }
    let ip = super::security::client_ip_from(headers, None).map(|ip| ip.to_string());
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok());
    let ttl = session_service::session_ttl_secs();

    match session_service::create_session(user_id, tenant_id, ip.as_deref(), user_agent).await {
        Ok(created) => vec![
            oauth_session::build_session_cookie(&created.token, ttl),
            oauth_session::build_csrf_cookie(&created.csrf_token, ttl),
        ],
        Err(e) => {
            tracing::warn!(
                "登录成功但签发浏览器会话失败（OAuth 方案 B 暂不可用）: user_id={}, error={}",
                user_id,
                e
            );
            Vec::new()
        }
    }
}

/// `GET /api/auth/session` 的响应体
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct SessionInfo {
    /// 用户 ID（雪花 ID 字符串）
    pub user_id: String,
    /// 会话绑定租户
    pub tenant_id: Option<String>,
    /// 会话过期时间（RFC 3339）
    pub expires_at: String,
}

/// `GET /api/auth/session`：用会话 Cookie 查询当前登录用户（供前端判断是否可走浏览器授权）
pub async fn session_me(headers: HeaderMap) -> Response {
    use super::response::{ApiError, ApiResponse};

    match cookie_session(&headers).await {
        Ok(Some(session)) => Json(ApiResponse::success(SessionInfo {
            user_id: session.user_id.to_string(),
            tenant_id: session.tenant_id.map(|id| id.to_string()),
            expires_at: session.expires_at.to_rfc3339(),
        }))
        .into_response(),
        Ok(None) => ApiError::unauthorized("浏览器会话不存在或已过期").into_response(),
        Err(error) => (
            error.status,
            Json(ApiError::internal_error(describe(&error))),
        )
            .into_response(),
    }
}

/// `POST /api/auth/session/logout`：撤销会话并清理 Cookie（幂等）
pub async fn session_logout(headers: HeaderMap) -> Response {
    use super::response::ApiResponse;

    if let Some(raw) = read_cookie(&headers, SESSION_COOKIE_NAME) {
        if let Err(e) = session_service::revoke_session(&raw).await {
            tracing::warn!("撤销浏览器会话失败: {}", e);
        }
    }

    let mut response = Json(ApiResponse::success(())).into_response();
    let response_headers = response.headers_mut();
    for cookie in [clear_cookie(SESSION_COOKIE_NAME), clear_csrf_cookie()] {
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response_headers.append(header::SET_COOKIE, value);
        }
    }
    response
}

/// 浏览器授权流程 + 会话查询路由（公开，不经过 `auth_middleware`）
///
/// 挂载条件：`oauth_routes::is_enabled()`（§16 T-7 缺省不暴露）。
/// `/oauth/authorize` 内部对 Bearer / Cookie 两种调用方分流，见模块文档。
pub fn browser_routes<S>() -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    use axum::routing::{get, post};

    axum::Router::new()
        .route(
            "/oauth/authorize",
            get(authorize_page).post(authorize_submit),
        )
        .route("/api/auth/session", get(session_me))
        .route("/api/auth/session/logout", post(session_logout))
        .layer(axum::middleware::from_fn(super::security::trusted_cidr_guard))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_map(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            let name: header::HeaderName = name.parse().expect("合法头名");
            headers.insert(name, HeaderValue::from_str(value).expect("合法头值"));
        }
        headers
    }

    #[test]
    fn deny_redirect_preserves_existing_query_and_encodes_state() {
        assert_eq!(
            deny_redirect("http://localhost:5173/cb", "a b&c"),
            "http://localhost:5173/cb?error=access_denied&state=a%20b%26c"
        );
        assert_eq!(
            deny_redirect("http://localhost:5173/cb?x=1", "s"),
            "http://localhost:5173/cb?x=1&error=access_denied&state=s"
        );
        assert_eq!(
            deny_redirect("http://localhost:5173/cb", ""),
            "http://localhost:5173/cb?error=access_denied"
        );
    }

    #[test]
    fn decision_from_form_maps_approve_and_deny() {
        let mut form = std::collections::HashMap::new();
        form.insert("client_id".to_string(), "app_1".to_string());
        form.insert("redirect_uri".to_string(), "http://localhost/cb".to_string());
        form.insert("scope".to_string(), "read write".to_string());
        form.insert("state".to_string(), "xyz".to_string());
        form.insert("code_challenge".to_string(), "abc".to_string());
        form.insert("decision".to_string(), "approve".to_string());

        let approve = decision_from_form(&form);
        assert!(approve.approve);
        assert_eq!(approve.client_id, "app_1");
        assert_eq!(approve.response_type, None);

        form.insert("decision".to_string(), "deny".to_string());
        assert!(!decision_from_form(&form).approve);

        // 缺失字段不得 panic，全部退化为空串 / false
        let empty = decision_from_form(&std::collections::HashMap::new());
        assert!(!empty.approve);
        assert!(empty.client_id.is_empty());
        assert!(empty.code_challenge.is_empty());
    }

    #[test]
    fn authorize_path_dispatches_on_authorization_header_or_json_body() {
        let plain = header_map(&[]);
        assert!(!wants_json(&plain, false));
        assert!(wants_json(&plain, true));

        let bearer = header_map(&[("authorization", "Bearer abc")]);
        assert!(wants_json(&bearer, false));
    }

    #[test]
    fn consent_params_skip_empty_values() {
        let query = AuthorizeQuery {
            client_id: Some("app_1".to_string()),
            redirect_uri: Some(String::new()),
            response_type: Some("code".to_string()),
            scope: Some("read".to_string()),
            state: Some("s".to_string()),
            code_challenge: Some("c".to_string()),
            code_challenge_method: None,
        };
        let params = consent_params(&query);
        assert_eq!(params.get("client_id").map(String::as_str), Some("app_1"));
        assert_eq!(params.get("response_type").map(String::as_str), Some("code"));
        // 空值 / None 都不写入隐藏域
        assert!(!params.contains_key("redirect_uri"));
        assert!(!params.contains_key("code_challenge_method"));
    }

    #[test]
    fn html_response_sets_security_headers_and_cookies() {
        let response = html_response(
            StatusCode::OK,
            "<html></html>".to_string(),
            vec!["ap_csrf=abc; Path=/".to_string()],
        );
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/html; charset=utf-8")
        );
        assert_eq!(
            response
                .headers()
                .get("x-frame-options")
                .and_then(|value| value.to_str().ok()),
            Some("DENY")
        );
        assert!(response.headers().get("content-security-policy").is_some());
        assert!(response.headers().get(header::SET_COOKIE).is_some());
    }
}





