//! 统一 API 响应格式
//!
//! 所有 HTTP API 返回统一的 JSON 结构：
//! ```json
//! {
//! "code": 0,
//! "message": "success",
//! "data": { ... }
//! }
//! ```

use axum::{
    http::{header::WWW_AUTHENTICATE, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use utoipa::ToSchema;

/// 认证域（出现在 `WWW-Authenticate` 响应头，设计文档 §6.7）
pub const AUTH_REALM: &str = "assets-platform";

/// 统一 API 响应
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiResponse<T: Serialize> {
    /// 业务码：0=成功，其它为 HTTP 语义错误码（400/401/404/500）
    pub code: i32,
    /// 提示信息，成功固定为 `success`
    pub message: String,
    /// 业务数据（无数据时不返回该字段）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T: Serialize> ApiResponse<T> {
    /// 成功响应
    pub fn success(data: T) -> Self {
        Self {
            code: 0,
            message: "success".to_string(),
            data: Some(data),
        }
    }

    /// 成功响应（无数据）
    pub fn success_empty() -> Self
    where
        T: Default,
    {
        Self {
            code: 0,
            message: "success".to_string(),
            data: None,
        }
    }
}

/// 错误响应
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiError {
    /// 错误码，与 HTTP 状态码一致（400/401/403/404/429/500）
    pub code: i32,
    /// 错误描述
    pub message: String,
    /// 权限不足（403）时返回所需权限点，便于集成方自查
    ///
    /// 仅 403 携带；其它错误不序列化该字段，保持既有 `{code,message}` 结构不变
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Vec<String>>,
}

impl ApiError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            scope: None,
        }
    }

    /// 400 参数错误
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(400, message)
    }

    /// 401 未认证
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(401, message)
    }

    /// 403 权限不足（不暴露所需权限点）
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(403, message)
    }

    /// 403 权限不足，并返回所需权限点（设计文档 §6.7）
    ///
    /// 只回显**权限点名称**，不含任何数据信息，因此不会造成信息泄露。
    pub fn forbidden_with_scope(required: &[String]) -> Self {
        Self {
            code: 403,
            message: "权限不足".to_string(),
            scope: Some(required.to_vec()),
        }
    }

    /// 404 未找到
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(404, message)
    }

    /// 409 状态冲突（如沙箱状态已变化）
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(409, message)
    }

    /// 429 请求过于频繁
    pub fn too_many_requests(message: impl Into<String>) -> Self {
        Self::new(429, message)
    }

    /// 500 服务器错误
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(500, message)
    }

    /// 503 依赖服务不可用（如沙箱未启用 / CubeAPI 不可达）
    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new(503, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.code {
            400 => StatusCode::BAD_REQUEST,
            401 => StatusCode::UNAUTHORIZED,
            403 => StatusCode::FORBIDDEN,
            404 => StatusCode::NOT_FOUND,
            409 => StatusCode::CONFLICT,
            429 => StatusCode::TOO_MANY_REQUESTS,
            503 => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        // RFC 6750：401 给出认证方案提示，403 明确所需 scope（便于集成方自查）
        let challenge = match self.code {
            401 => Some(format!("Bearer realm=\"{}\"", AUTH_REALM)),
            403 => Some(match self.scope.as_deref() {
                Some(scope) if !scope.is_empty() => format!(
                    "Bearer realm=\"{}\", error=\"insufficient_scope\", scope=\"{}\"",
                    AUTH_REALM,
                    scope.join(" ")
                ),
                _ => format!(
                    "Bearer realm=\"{}\", error=\"insufficient_scope\"",
                    AUTH_REALM
                ),
            }),
            _ => None,
        };

        let mut response = (status, Json(self)).into_response();
        if let Some(value) = challenge {
            if let Ok(value) = HeaderValue::from_str(&value) {
                response.headers_mut().insert(WWW_AUTHENTICATE, value);
            }
        }
        response
    }
}

/// 将 Result<String> 转换为 HTTP 响应
pub fn result_to_response<T, F>(
    result: Result<T, String>,
    success_fn: F,
) -> Result<Json<ApiResponse<T>>, ApiError>
where
    T: Serialize,
    F: FnOnce(T) -> Json<ApiResponse<T>>,
{
    match result {
        Ok(data) => Ok(success_fn(data)),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// 将 Result<(), String> 转换为 HTTP 响应
pub fn result_to_empty_response(
    result: Result<(), String>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    match result {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[test]
    fn forbidden_carries_scope_in_body_and_header() {
        let required = vec!["asset:hardware:list".to_string()];
        let error = ApiError::forbidden_with_scope(&required);

        // 响应体只新增 scope 字段，其余结构保持 {code,message}
        let body = serde_json::to_value(&error).expect("403 应可序列化");
        assert_eq!(body["code"], 403);
        assert_eq!(body["message"], "权限不足");
        assert_eq!(body["scope"][0], "asset:hardware:list");

        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let challenge = response
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(challenge.contains("error=\"insufficient_scope\""));
        assert!(challenge.contains("asset:hardware:list"));
    }

    #[test]
    fn unauthorized_keeps_legacy_body_and_adds_challenge() {
        let error = ApiError::unauthorized("未登录");
        let body = serde_json::to_value(&error).expect("401 应可序列化");
        assert_eq!(body["code"], 401);
        // 401 不返回 scope 字段（前端按 {code,message} 解析）
        assert!(body.get("scope").is_none());

        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let challenge = response
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert_eq!(challenge, format!("Bearer realm=\"{}\"", AUTH_REALM));
    }

    #[test]
    fn unmapped_route_403_has_no_scope_hint() {
        let response = ApiError::forbidden("该接口未配置访问权限").into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let challenge = response
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(challenge.contains("insufficient_scope"));
        assert!(!challenge.contains("scope="));
    }
}

