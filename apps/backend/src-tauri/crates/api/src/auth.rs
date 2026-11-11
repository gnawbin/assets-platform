//! JWT 认证中间件
//!
//! 提供 HTTP API 的凭证验证中间件：
//! 从 `Authorization: Bearer <token>` 中提取凭证，按前缀分流为
//! **JWT 访问令牌**（`eyJ...`）、**外部系统 API Key**（`ak_...`）或
//! **OAuth 访问令牌**（`at_...`，Phase 2），
//! 验证后注入 [`UserContext`] / [`Claims`]（JWT）与 [`AuthPrincipal`]（三者）。
//! 同时根据用户当前选中租户设置 task_local schema。

use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

use dashmap::DashMap;

use super::response::ApiError;
use axum::{extract::Request, middleware::Next, response::Response};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

use service::api_key_service::UserAuthority;

/// JWT 声明（与 user_service 中的 Claims 保持一致）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,
    pub username: String,
    pub exp: usize,
    pub iat: usize,
    /// 令牌类型：access / refresh（历史令牌没有该字段，按 access 处理）
    #[serde(default)]
    pub typ: Option<String>,
    /// 刷新令牌白名单主键（雪花 ID 字符串），仅 refresh 令牌携带
    #[serde(default)]
    pub jti: Option<String>,
}

/// 用户上下文（注入到每个请求的 Extension 中）
#[derive(Debug, Clone)]
pub struct UserContext {
    pub user_id: i64,
    pub username: String,
    pub schema_name: String,
}

/// 主体类型（设计文档 §5.4）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalKind {
    /// 登录用户（JWT）
    User,
    /// 服务身份（外部系统 API Key）
    Service,
    /// OAuth 访问令牌（Phase 2）
    OAuth,
}

/// 统一主体（**新增** extension，老 handler 不感知，设计文档 §5.5）
///
/// 与 [`UserContext`] 的关系：
/// - JWT 请求同时注入 `UserContext` / `Claims` / `AuthPrincipal`，老 handler 照常工作；
/// - API Key 请求**只**注入 `AuthPrincipal`（服务身份没有 `user_id`），
///   因此需要 `claims.sub` 的接口对它一律 403（由 `authz` 层拦截）。
#[derive(Debug, Clone)]
pub struct AuthPrincipal {
    /// 主体类型
    pub kind: PrincipalKind,
    /// 主体标识：`u:<user_id>` / `k:<api_key_id>`
    pub subject: String,
    /// 用户 ID（服务身份为 `None`）
    pub user_id: Option<i64>,
    /// API Key 主键（用户身份为 `None`）
    pub api_key_id: Option<i64>,
    /// 绑定租户
    pub tenant_id: Option<i64>,
    /// 请求实际落库的 schema（task_local 已按此设置）
    pub schema_name: String,
    /// 可用权限点（用户 = 角色权限并集；API Key = key 的 scopes）
    pub scopes: Vec<String>,
    /// 是否超级管理员（只有 JWT 用户可能为 true）
    pub is_super_admin: bool,
    /// 每分钟请求上限（仅 API Key 有）
    pub rate_limit_per_min: Option<i32>,
}

impl AuthPrincipal {
    /// 是否满足某路由的全部权限点要求（设计文档 §6.4）
    ///
    /// - 超管直接通过（`is_super_admin` 只可能由 JWT 路径设置）
    /// - `required` 为空表示「仅需登录」
    pub fn allows(&self, required: &[String]) -> bool {
        if self.is_super_admin {
            return true;
        }
        required
            .iter()
            .all(|need| self.scopes.iter().any(|have| super::authz::matches_scope(have, need)))
    }
}

/// 凭证类型（设计文档 §5.1：无需解码即可分流）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    /// JWT 访问令牌（`eyJ...`）
    Jwt(String),
    /// 外部系统 API Key（`ak_<env>_<prefix16>_<secret43>`）
    ApiKey(String),
    /// OAuth 2.1 访问令牌（`at_<43>`，Phase 2，设计文档 §7.5）
    AccessToken(String),
}

/// 用户授权快照缓存 TTL（设计文档 §6.8：用户 perms 并集 60s）
const AUTHORITY_CACHE_TTL: Duration = Duration::from_secs(60);

/// 用户授权快照缓存：`user_id` → (写入时刻, 授权快照)
fn authority_cache() -> &'static DashMap<i64, (Instant, UserAuthority)> {
    static CACHE: OnceLock<DashMap<i64, (Instant, UserAuthority)>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

/// 清除某个用户的授权缓存（改角色 / 改角色菜单 / 禁用 / 登出后调用）
pub fn invalidate_user_authority(user_id: i64) {
    authority_cache().remove(&user_id);
}

/// 清空全部用户授权缓存（权限配置批量变更后调用）
pub fn clear_user_authority_cache() {
    authority_cache().clear();
}

/// 读取用户授权快照（带 60s 缓存，回源失败按「无权限」处理）
pub async fn user_authority(user_id: i64) -> UserAuthority {
    let cached = authority_cache()
        .get(&user_id)
        .filter(|entry| entry.0.elapsed() < AUTHORITY_CACHE_TTL)
        .map(|entry| entry.1.clone());
    if let Some(authority) = cached {
        return authority;
    }

    match service::api_key_service::load_user_authority(user_id).await {
        Ok(authority) => {
            authority_cache().insert(user_id, (Instant::now(), authority.clone()));
            authority
        }
        Err(e) => {
            // 回源失败不得降级为放行（§6.8），这里返回空授权 → 后续 403
            tracing::error!("查询用户权限失败，按无权限处理: user_id={}, error={}", user_id, e);
            UserAuthority::default()
        }
    }
}

/// 按前缀解析凭证类型
///
/// API Key 只做**格式**校验（段数 / 环境 / 字符集），真正的哈希查表在
/// [`service::api_key_service::verify_api_key`]。
pub fn parse_credential(token: &str) -> Result<Credential, ApiError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(ApiError::unauthorized("认证令牌为空"));
    }

    if utils::api_key::is_api_key_token(token) {
        if utils::api_key::parse_api_key_prefix(token).is_none() {
            tracing::warn!("API Key 格式非法（不输出完整值）");
            return Err(ApiError::unauthorized("凭证无效或已过期"));
        }
        return Ok(Credential::ApiKey(token.to_string()));
    }

    // OAuth 访问令牌（Phase 2）：`at_` 前缀，哈希查表在 oauth_service
    if utils::oauth::is_access_token(token) {
        return Ok(Credential::AccessToken(token.to_string()));
    }

    // refresh 令牌只能用于 /oauth/token，出现在资源请求里一律拒绝（§13.2 #3）
    if utils::oauth::is_refresh_token(token) {
        tracing::warn!("拒绝把 OAuth 刷新令牌当作访问令牌使用");
        return Err(ApiError::unauthorized(
            "刷新令牌只能用于 /oauth/token，请使用访问令牌",
        ));
    }

    Ok(Credential::Jwt(token.to_string()))
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

    // refresh token 只能用于 /api/auth/refresh，不能当作访问令牌使用
    // （历史令牌没有 typ 字段，按 access 处理，避免升级后强制全员重新登录）
    let claims = token_data.claims;
    if claims.typ.as_deref().unwrap_or("access") != "access" {
        tracing::warn!("拒绝使用非访问令牌调用接口: sub={}", claims.sub);
        return Err(ApiError::unauthorized(
            "请使用访问令牌（access token），刷新令牌仅可用于 /api/auth/refresh",
        ));
    }

    Ok(claims)
}

/// 查询用户当前选中的 schema 名称
///
/// 1. 先查 USER_TENANT_CACHE（内存缓存，每次登录/切换时设置）
/// 2. 缓存未命中时，从数据库兜底查询用户的 tenant_id → schema_name
/// 3. 查询到后写入缓存
/// 4. 如果用户没有关联任何租户，返回 "public"
async fn resolve_user_schema(user_id: i64) -> String {
    // 1. 查 USER_TENANT_CACHE 获取用户当前选中租户 ID
    let user_cache = database::postgres::get_user_tenant_cache();
    if let Some(entry) = user_cache.get(&user_id) {
        let tenant_id = *entry;
        // 查 SCHEMA_CACHE 获取 schema 名称
        let schema_cache = database::postgres::get_schema_cache();
        if let Some(schema) = schema_cache.get(&tenant_id) {
            return schema.clone();
        }
    }

    // 2. 缓存未命中，从数据库兜底查询
    if let Ok(pool) = database::get_read_pool() {
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
                database::postgres::get_schema_cache().insert(tid, sn.clone());
                return sn;
            }
        }
    }

    // 4. 没有任何租户关联，返回 public
    "public".to_string()
}

/// 拒绝通过 URL 参数传递令牌（RFC 6750 §2.3 / 设计文档 §6.7 → 400）
fn reject_token_in_query(req: &Request) -> Result<(), ApiError> {
    let Some(query) = req.uri().query() else {
        return Ok(());
    };
    const FORBIDDEN_PARAMS: [&str; 4] = ["access_token", "id_token", "token", "api_key"];
    for param in FORBIDDEN_PARAMS {
        let pattern = format!("{}=", param);
        let hit = query
            .split('&')
            .any(|pair| pair.to_ascii_lowercase().starts_with(&pattern));
        if hit {
            tracing::warn!(
                route = %req.uri().path(),
                param,
                "拒绝通过 URL 参数传递令牌（客户端应改用 Authorization 请求头）"
            );
            return Err(ApiError::bad_request("令牌不得通过 URL 参数传递"));
        }
    }
    Ok(())
}

/// 从 `Authorization` 请求头提取 Bearer 凭证
fn extract_bearer(req: &Request) -> Result<String, ApiError> {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::unauthorized("缺少 Authorization 请求头"))?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| ApiError::unauthorized("Authorization 格式错误，应为 Bearer <token>"))?;

    Ok(token.trim().to_string())
}

/// 按租户 ID 解析 schema 名称（API Key 绑定租户用）
async fn resolve_tenant_schema(tenant_id: Option<i64>) -> String {
    let Some(tenant_id) = tenant_id else {
        return "public".to_string();
    };

    if let Some(schema) = database::postgres::get_schema_cache().get(&tenant_id) {
        return schema.clone();
    }

    if let Ok(pool) = database::get_read_pool() {
        let schema: Option<Option<String>> = sqlx::query_scalar(
            "SELECT schema_name FROM public.sys_tenant WHERE id = $1 AND enable = true",
        )
        .bind(tenant_id)
        .fetch_optional(&pool)
        .await
        .ok();

        if let Some(Some(schema)) = schema {
            database::postgres::get_schema_cache()
                .insert(tenant_id, schema.clone());
            return schema;
        }
    }

    "public".to_string()
}

/// 认证中间件
///
/// 1. 拒绝 URL 参数传令牌（400）
/// 2. 从 `Authorization: Bearer <token>` 提取凭证并按前缀分流（§5.1）
///    - `ak_*` → API Key：哈希查表 → 服务主体（无 `user_id`）
///    - 其它 → JWT：验签 → 用户主体（同时注入 `UserContext` / `Claims`）
/// 3. 注入 [`AuthPrincipal`]（两种主体都有）
/// 4. 通过 `with_schema` 设置 task_local 供 service 层使用
/// 5. 之后由 `authz_middleware` 做 L2 功能权限判定
pub async fn auth_middleware(mut req: Request, next: Next) -> Result<Response, ApiError> {
    reject_token_in_query(&req)?;

    let token = extract_bearer(&req)?;
    let credential = parse_credential(&token)?;

    match credential {
        Credential::Jwt(token) => {
            let claims = verify_token(&token)?;

            // 解析用户当前 schema（async 调用，从缓存或数据库获取）
            let schema_name: String = resolve_user_schema(claims.sub).await;
            let authority = user_authority(claims.sub).await;
            let tenant_id = database::postgres::get_user_tenant_cache()
                .get(&claims.sub)
                .map(|entry| *entry);

            // 注意：此处**不**更新 `database::GLOBAL_SCHEMA`。
            //
            // `GLOBAL_SCHEMA` 是 Tauri 命令模式的 schema 来源（启动时由 `bootstrap` 设置默认租户、
            // 由 `switch_tenant` 命令切换）。HTTP 请求在同一进程内改它会直接污染桌面会话，
            // 属改造前遗留的串租户 bug（设计文档 §16 T-3 / §13.4 的 RT-3、RT-4）。
            // HTTP 一律只走下面的 task_local（`with_schema`）。

            let principal = AuthPrincipal {
                kind: PrincipalKind::User,
                subject: format!("u:{}", claims.sub),
                user_id: Some(claims.sub),
                api_key_id: None,
                tenant_id,
                schema_name: schema_name.clone(),
                scopes: authority.scopes,
                is_super_admin: authority.is_super_admin,
                rate_limit_per_min: None,
            };

            // 注入 UserContext（老 handler 全部依赖，不能去掉，§5.5）
            let ctx = UserContext {
                user_id: claims.sub,
                username: claims.username.clone(),
                schema_name: schema_name.clone(),
            };
            req.extensions_mut().insert(ctx);

            // 仍注入 Claims（兼容旧路由，提供 claims.sub 作为 user_id）
            req.extensions_mut().insert(claims);

            // 新增统一主体（新代码使用，老代码不感知）
            req.extensions_mut().insert(principal);

            // 通过 with_schema 设置 task_local，使 service 层的 schema_prefix() 能读取
            let response =
                database::with_schema(schema_name, async { next.run(req).await }).await;

            Ok(response)
        }
        Credential::ApiKey(raw_key) => {
            let auth = service::api_key_service::verify_api_key(&raw_key)
                .await
                .map_err(|e| {
                    // 回源失败不得降级为放行（§6.8）：返回 500 而不是放过去
                    tracing::error!("API Key 校验失败（回源异常）: {}", e);
                    ApiError::internal_error("凭证校验服务暂时不可用")
                })?
                .ok_or_else(|| {
                    tracing::warn!("API Key 无效 / 过期 / 已吊销（不输出完整值）");
                    ApiError::unauthorized("凭证无效或已过期")
                })?;

            let schema_name = resolve_tenant_schema(auth.tenant_id).await;

            // 只注入 AuthPrincipal：服务身份没有 user_id，
            // 需要 claims.sub 的接口由 authz 层一律拒绝（§6.6）。
            let principal = AuthPrincipal {
                kind: PrincipalKind::Service,
                subject: format!("k:{}", auth.id),
                user_id: None,
                api_key_id: Some(auth.id),
                tenant_id: auth.tenant_id,
                schema_name: schema_name.clone(),
                scopes: auth.scopes,
                is_super_admin: false,
                rate_limit_per_min: Some(auth.rate_limit_per_min),
            };
            req.extensions_mut().insert(principal);

            tracing::debug!(
                prefix = %auth.key_prefix,
                tenant_id = ?auth.tenant_id,
                route = %req.uri().path(),
                "API Key 鉴权通过"
            );

            let response =
                database::with_schema(schema_name, async { next.run(req).await }).await;

            Ok(response)
        }
        Credential::AccessToken(raw_token) => {
            // OAuth 2.1 访问令牌（Phase 2）：哈希查表 + 客户端启用状态校验。
            // 回源失败同样不得降级为放行（§6.8）。
            let auth = service::oauth_service::verify_access_token(&raw_token)
                .await
                .map_err(|e| {
                    tracing::error!("OAuth 访问令牌校验失败（回源异常）: {}", e);
                    ApiError::internal_error("凭证校验服务暂时不可用")
                })?
                .ok_or_else(|| {
                    tracing::warn!("OAuth 访问令牌无效 / 过期 / 已撤销（不输出完整值）");
                    ApiError::unauthorized("凭证无效或已过期")
                })?;

            let schema_name = resolve_tenant_schema(auth.tenant_id).await;

            // 主体标识格式见 §5.4：`u:<user_id>@client:<client_id>`
            let subject = match auth.user_id {
                Some(user_id) => format!("u:{}@client:{}", user_id, auth.client_id),
                None => format!("client:{}", auth.client_id),
            };

            // 只注入 AuthPrincipal：OAuth 令牌没有 username，
            // 需要 `UserContext` / `Claims` 的接口由 authz 层一律 403。
            let principal = AuthPrincipal {
                kind: PrincipalKind::OAuth,
                subject,
                user_id: auth.user_id,
                api_key_id: None,
                tenant_id: auth.tenant_id,
                schema_name: schema_name.clone(),
                scopes: auth.scopes,
                is_super_admin: false,
                rate_limit_per_min: None,
            };
            req.extensions_mut().insert(principal);

            tracing::debug!(
                client_id = %auth.client_id,
                token_id = auth.token_id,
                tenant_id = ?auth.tenant_id,
                route = %req.uri().path(),
                "OAuth 访问令牌鉴权通过"
            );

            let response =
                database::with_schema(schema_name, async { next.run(req).await }).await;

            Ok(response)
        }
    }
}

/// 不需要认证的路由列表
pub const PUBLIC_ROUTES: &[&str] = &["/api/auth/login"];
