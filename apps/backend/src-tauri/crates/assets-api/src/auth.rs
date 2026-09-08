//! JWT 认证中间件
//!
//! 提供 HTTP API 的 JWT token 验证中间件。
//! 从 Authorization header 中提取 Bearer token，验证后将用户信息存入请求扩展。
//! 同时根据用户当前选中租户设置 task_local schema。

use super::response::ApiError;
use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

/// JWT 声明（与 user_service 中的 Claims 保持一致）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,
    pub username: String,
    pub exp: usize,
    pub iat: usize,
}

/// 用户上下文（注入到每个请求的 Extension 中）
#[derive(Debug, Clone)]
pub struct UserContext {
    pub user_id: i64,
    pub username: String,
    pub schema_name: String,
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

/// 查询用户当前选中的 schema 名称
///
/// 1. 先查 USER_TENANT_CACHE（内存缓存，每次登录/切换时设置）
/// 2. 缓存未命中时，从数据库兜底查询用户的 tenant_id → schema_name
/// 3. 查询到后写入缓存
/// 4. 如果用户没有关联任何租户，返回 "public"
async fn resolve_user_schema(user_id: i64) -> String {
    // 1. 查 USER_TENANT_CACHE 获取用户当前选中租户 ID
    let user_cache = assets_database::postgres::get_user_tenant_cache();
    if let Some(entry) = user_cache.get(&user_id) {
        let tenant_id = *entry;
        // 查 SCHEMA_CACHE 获取 schema 名称
        let schema_cache = assets_database::postgres::get_schema_cache();
        if let Some(schema) = schema_cache.get(&tenant_id) {
            return schema.clone();
        }
    }

    // 2. 缓存未命中，从数据库兜底查询
    if let Ok(pool) = assets_database::get_read_pool() {
        // 查询用户的 tenant_id
        let tenant_id: Option<i64> = sqlx::query_scalar(
            "SELECT tenant_id FROM public.sys_user WHERE id = $1 AND (deleted IS NULL OR deleted = 0)"
        )
        .bind(user_id)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten();

        if let Some(tid) = tenant_id {
            // 查询 schema_name
            let schema: Option<String> = sqlx::query_scalar(
                "SELECT schema_name FROM public.sys_tenant WHERE id = $1 AND enable = true",
            )
            .bind(tid)
            .fetch_optional(&pool)
            .await
            .ok()
            .flatten();

            if let Some(sn) = schema {
                // 写入缓存
                user_cache.insert(user_id, tid);
                assets_database::postgres::get_schema_cache().insert(tid, sn.clone());
                return sn;
            }
        }
    }

    // 4. 没有任何租户关联，返回 public
    "public".to_string()
}

/// 认证中间件
///
/// 1. 从 Authorization header 提取 Bearer token 并验证
/// 2. 查 USER_TENANT_CACHE 获取用户当前选中租户的 schema
/// 3. 注入 `UserContext` 到请求 Extension
/// 4. 通过 `with_schema` 设置 task_local 供 service 层使用
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

    // 解析用户当前 schema（async 调用，从缓存或数据库获取）
    let schema_name: String = resolve_user_schema(claims.sub).await;

    // 同时更新 GLOBAL_SCHEMA（Tauri 模式不经过此中间件，但以防后端同时存在 HTTP 调用）
    assets_database::set_global_schema(schema_name.clone());

    // 注入 UserContext（新）
    let ctx = UserContext {
        user_id: claims.sub,
        username: claims.username.clone(),
        schema_name: schema_name.clone(),
    };
    req.extensions_mut().insert(ctx);

    // 仍注入 Claims（兼容旧路由，提供 claims.sub 作为 user_id）
    req.extensions_mut().insert(claims);

    // 通过 with_schema 设置 task_local，使 service 层的 schema_prefix() 能读取
    let response = assets_database::with_schema(schema_name, async { next.run(req).await }).await;

    Ok(response)
}

/// 不需要认证的路由列表
pub const PUBLIC_ROUTES: &[&str] = &["/api/auth/login"];
