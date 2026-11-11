//! OAuth 2.1 协议错误响应（设计文档 §7.2）
//!
//! `/oauth/*` 端点**不得**复用 [`super::response::ApiError`] 的 `{code,message}` 结构，
//! 否则第三方 OAuth 客户端库无法解析。本模块输出 RFC 6749 §5.2 的
//! `{ "error": "...", "error_description": "..." }`，并统一带上：
//!
//! - `Cache-Control: no-store` / `Pragma: no-cache`（令牌相关响应禁止缓存）
//! - `WWW-Authenticate: Basic realm="..."`（401 `invalid_client`）
//! - `X-Content-Type-Options: nosniff`

use axum::{
    http::{
        header::{CACHE_CONTROL, PRAGMA, WWW_AUTHENTICATE},
        HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use service::oauth_service::OauthFailure;

use super::response::AUTH_REALM;

/// 错误响应体（RFC 6749 §5.2）
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct OauthErrorBody {
    /// 错误码（invalid_request / invalid_client / invalid_grant / ...）
    pub error: String,
    /// 人类可读说明（不含敏感信息）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
}

/// OAuth 协议错误
#[derive(Debug, Clone)]
pub struct OauthError {
    pub status: StatusCode,
    pub body: OauthErrorBody,
    /// `WWW-Authenticate` 头（仅 401 使用）
    challenge: Option<String>,
}

impl OauthError {
    /// 构造错误（按错误码决定 HTTP 状态）
    pub fn new(error: impl Into<String>, description: impl Into<String>) -> Self {
        let error = error.into();
        let status = status_for(&error);
        let challenge = if error == "invalid_client" {
            Some(format!("Basic realm=\"{}\"", AUTH_REALM))
        } else if error == "invalid_token" {
            Some(format!(
                "Bearer realm=\"{}\", error=\"invalid_token\"",
                AUTH_REALM
            ))
        } else {
            None
        };
        Self {
            status,
            body: OauthErrorBody {
                error,
                error_description: Some(description.into()),
            },
            challenge,
        }
    }

    /// 由 `service::oauth_service::OauthFailure` 转换
    pub fn from_failure(failure: OauthFailure) -> Self {
        Self::new(failure.error, failure.description)
    }

    /// `invalid_request`
    pub fn invalid_request(description: impl Into<String>) -> Self {
        Self::new("invalid_request", description)
    }

    /// `invalid_client`（401，带 Basic challenge）
    pub fn invalid_client(description: impl Into<String>) -> Self {
        Self::new("invalid_client", description)
    }

    /// `server_error`
    pub fn server_error(description: impl Into<String>) -> Self {
        Self::new("server_error", description)
    }

    /// `unsupported_grant_type`
    pub fn unsupported_grant_type(description: impl Into<String>) -> Self {
        Self::new("unsupported_grant_type", description)
    }

    /// `access_denied`（用户拒绝授权）
    pub fn access_denied(description: impl Into<String>) -> Self {
        Self::new("access_denied", description)
    }

    /// `invalid_grant`
    pub fn invalid_grant(description: impl Into<String>) -> Self {
        Self::new("invalid_grant", description)
    }

    /// `invalid_scope`
    pub fn invalid_scope(description: impl Into<String>) -> Self {
        Self::new("invalid_scope", description)
    }
}

/// 错误码 → HTTP 状态（RFC 6749 §5.2 / RFC 6750 §3.1）
fn status_for(error: &str) -> StatusCode {
    match error {
        "invalid_client" | "invalid_token" => StatusCode::UNAUTHORIZED,
        "server_error" => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::BAD_REQUEST,
    }
}

impl IntoResponse for OauthError {
    fn into_response(self) -> Response {
        let mut response = (self.status, Json(self.body)).into_response();
        let headers = response.headers_mut();
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
        headers.insert(PRAGMA, HeaderValue::from_static("no-cache"));
        headers.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
        if let Some(challenge) = self.challenge {
            if let Ok(value) = HeaderValue::from_str(&challenge) {
                headers.insert(WWW_AUTHENTICATE, value);
            }
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn body_json(response: Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("响应体应可读取");
        serde_json::from_slice(&bytes).expect("响应体应是 JSON")
    }

    #[tokio::test]
    async fn invalid_request_is_400_without_challenge() {
        let response = OauthError::invalid_request("缺少 client_id").into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response
                .headers()
                .get(CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("no-store")
        );
        assert!(response.headers().get(WWW_AUTHENTICATE).is_none());
        let body = body_json(response).await;
        assert_eq!(body["error"], "invalid_request");
        assert_eq!(body["error_description"], "缺少 client_id");
    }

    #[tokio::test]
    async fn invalid_client_is_401_with_basic_challenge() {
        let response = OauthError::invalid_client("client_secret 错误").into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let challenge = response
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(challenge.starts_with("Basic realm="));
        let body = body_json(response).await;
        assert_eq!(body["error"], "invalid_client");
    }

    #[tokio::test]
    async fn server_error_is_500_and_never_leaks_details() {
        let failure = OauthFailure::server_error("令牌服务暂时不可用");
        let response = OauthError::from_failure(failure).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = body_json(response).await;
        assert_eq!(body["error"], "server_error");
    }

    #[tokio::test]
    async fn unsupported_grant_type_is_400() {
        let response =
            OauthError::unsupported_grant_type("仅支持 authorization_code").into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = body_json(response).await;
        assert_eq!(body["error"], "unsupported_grant_type");
    }
}
