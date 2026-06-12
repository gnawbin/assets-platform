//! JWT 认证中间件
//!
//! 提供 HTTP API 的 JWT token 验证中间件。
//! 从 Authorization header 中提取 Bearer token，验证后将用户信息存入请求扩展。

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

use super::response::ApiError;

/// JWT 声明（与 user_service 中的 Claims 保持一致）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,
    pub username: String,
    pub exp: usize,
    pub iat: usize,
}

/// 从请求中提取并验证 JWT token
pub fn verify_token(token: &str) -> Result<Claims, ApiError> {
    let jwt_secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "assets-platform-default-secret-key".to_string());

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|e| {
        tracing::warn!("JWT 验证失败: {}", e);
        ApiError::unauthorized("无效的认证令牌")
    })?;

    Ok(token_data.claims)
}

/// 认证中间件
///
/// 从 Authorization header 提取 Bearer token 并验证。
/// 验证通过后将用户信息存入请求扩展。
pub async fn auth_middleware(mut req: Request, next: Next) -> Result<Response, ApiError> {
    // 从请求头中获取 Authorization
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::unauthorized("缺少 Authorization 请求头"))?;

    // 提取 Bearer token
    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| ApiError::unauthorized("Authorization 格式错误，应为 Bearer <token>"))?;

    // 验证 token
    let claims = verify_token(token)?;

    // 将用户信息存入请求扩展
    req.extensions_mut().insert(claims);

    Ok(next.run(req).await)
}

/// 不需要认证的路由列表
pub const PUBLIC_ROUTES: &[&str] = &["/api/auth/login", "/api/swagger-ui", "/api/openapi.json"];
