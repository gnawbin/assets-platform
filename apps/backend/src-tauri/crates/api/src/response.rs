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
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

/// 统一 API 响应
#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub code: i32,
    pub message: String,
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
#[derive(Debug, Serialize)]
pub struct ApiError {
    pub code: i32,
    pub message: String,
}

impl ApiError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
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

    /// 404 未找到
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(404, message)
    }

    /// 500 服务器错误
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(500, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.code {
            400 => StatusCode::BAD_REQUEST,
            401 => StatusCode::UNAUTHORIZED,
            404 => StatusCode::NOT_FOUND,
            500 => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        (status, Json(self)).into_response()
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
