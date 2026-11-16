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
    /// 签发方（Phase 3；历史令牌没有该字段）
    #[serde(default)]
    pub iss: Option<String>,
    /// 受众（Phase 3；历史令牌没有该字段）
    #[serde(default)]
    pub aud: Option<String>,
}

/// 用户上下文（注入到每个请求的 Extension 中）
#[derive(Debug, Clone)]
pub struct UserContext {
    pub user_id: i64,
    pub username: String,
    pub schema_name: String,
}

/// 主体类型（设计文档 §5.4）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// 只清本实例的 L1 授权快照（供 NOTIFY listener 调用，避免再次广播形成回环）
pub fn clear_local_user_authority(user_id: i64) {
    authority_cache().remove(&user_id);
}

/// 清除某个用户的授权缓存（改角色 / 改角色菜单 / 禁用 / 登出后调用）
///
/// 签名与改造前一致，现有调用点无需改动：先清本实例 L1，再触发跨实例广播。
pub fn invalidate_user_authority(user_id: i64) {
    clear_local_user_authority(user_id);
    if let Some(invalidator) = auth_invalidator() {
        invalidator.on_invalidate_user(user_id);
    }
}

/// 清空全部用户授权缓存（权限配置批量变更后调用）
pub fn clear_user_authority_cache() {
    clear_local_authority_cache();
    if let Some(invalidator) = auth_invalidator() {
        invalidator.on_clear_all();
    }
}

/// 只清本实例的全部授权快照（供 NOTIFY listener 调用）
pub fn clear_local_authority_cache() {
    authority_cache().clear();
}

/// 认证失效广播的注入点（设计文档附录 D.3）
///
/// `api` crate 不感知 Redis / PostgreSQL：真正的广播实现由进程入口注入。
/// 桌面形态不注入，[`invalidate_user_authority`] 的行为与改造前完全一致。
pub trait AuthInvalidator: Send + Sync {
    /// 某用户权限失效（改角色 / 改角色菜单 / 禁用 / 登出）
    fn on_invalidate_user(&self, user_id: i64);
    /// 全量失效（权限配置批量变更）
    fn on_clear_all(&self);
}

static AUTH_INVALIDATOR: OnceLock<std::sync::Arc<dyn AuthInvalidator>> = OnceLock::new();

/// 安装失效广播器（幂等，仅启动期调用一次）
pub fn install_auth_invalidator(invalidator: std::sync::Arc<dyn AuthInvalidator>) {
    let _ = AUTH_INVALIDATOR.set(invalidator);
}

/// 当前失效广播器（未安装时返回 `None`，桌面形态即如此）
fn auth_invalidator() -> Option<&'static std::sync::Arc<dyn AuthInvalidator>> {
    AUTH_INVALIDATOR.get()
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

/// 构造 JWT 校验参数（与签发侧共用 `iss` / `aud` 配置）
///
/// **必须显式设置 audience**：`jsonwebtoken` 11 的 `Validation::default()` 是
/// `validate_aud = true` 且 `aud = None`，此时**只要令牌带 `aud` 声明就返回
/// `InvalidAudience`**（见 `validation.rs` 的 `(TryParse::Parsed(_), None)` 分支）。
/// 签发侧自 Phase 3 起固定写 `aud`，因此用默认校验会让所有 HTTP 请求 401
/// 「无效的认证令牌」。
///
/// 兼容说明：`jsonwebtoken` 仅在令牌**存在** `aud` / `iss` 时才比对，
/// 历史令牌（无这两个声明）依旧放行；严格模式由
/// [`service::user_service::check_token_scope`] 的 `JWT_STRICT_ISS_AUD` 控制。
fn jwt_validation() -> Validation {
    let mut validation = Validation::default();
    validation.set_audience(&[service::user_service::jwt_audience()]);
    validation.set_issuer(&[service::user_service::jwt_issuer()]);
    validation
}

/// 从请求中提取并验证 JWT token
pub fn verify_token(token: &str) -> Result<Claims, ApiError> {
    let jwt_secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "assets-platform-default-secret-key".to_string());

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &jwt_validation(),
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

    // Phase 3：iss / aud 校验（存在即校验；JWT_STRICT_ISS_AUD=true 时强制存在）
    if let Err(e) =
        service::user_service::check_token_scope(claims.iss.as_deref(), claims.aud.as_deref())
    {
        tracing::warn!("JWT scope 校验失败: sub={}, error={}", claims.sub, e);
        return Err(ApiError::unauthorized(e));
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
/// 1. 拒绝 URL 参数传令牌（400，`?access_token=` 之类直接拒绝，避免令牌进日志）
/// 2. 从 `Authorization: Bearer <token>` 提取凭证，按**前缀**分流（设计文档 §5.1）：
///    - `ak_*` → API Key：格式校验 → 哈希查表 → 服务主体（`PrincipalKind::Service`，
///      无 `user_id`；真正查表在 `service::api_key_service::verify_api_key`）
///    - `at_*` → OAuth 访问令牌：哈希查表（`oauth_service::verify_access_token`）→
///      `PrincipalKind::OAuth`，**不注入** `UserContext` / `Claims`，`authz` 始终 enforce
///    - `rt_*` → **一律 401**：刷新令牌只能用于 `/oauth/token`（设计文档 §13.2 #3），
///      出现在资源路径上属误用，文案直接点明用途
///    - 其余（`eyJ...` 及历史无前缀令牌）→ JWT：验签 → 用户主体，
///      同时注入 `UserContext` / `Claims`
/// 3. 注入 [`AuthPrincipal`]（三种主体都有）
/// 4. 通过 `with_schema` 设置 task_local 供 service 层使用
/// 5. 之后由 `authz_middleware` 做 L2 功能权限判定
///
/// 分流逻辑本体在 [`parse_credential`]，单测见本文件 `mod tests` 的 A-1…A-5。
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

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::encode;

    /// 与签发侧（`user_service::generate_token`）同形的声明，含 `iss` / `aud`
    fn claims_with_scope() -> Claims {
        Claims {
            sub: 1,
            username: "admin".to_string(),
            exp: jsonwebtoken::get_current_timestamp() as usize + 3600,
            iat: jsonwebtoken::get_current_timestamp() as usize,
            typ: Some("access".to_string()),
            jti: None,
            iss: Some(service::user_service::jwt_issuer()),
            aud: Some(service::user_service::jwt_audience()),
        }
    }

    fn sign(claims: &Claims) -> String {
        encode(
            &jsonwebtoken::Header::default(),
            claims,
            &jsonwebtoken::EncodingKey::from_secret(b"assets-platform-default-secret-key"),
        )
        .expect("测试令牌应可签发")
    }

    /// 回归用例：签发侧带 `aud`，校验必须显式设置 audience，
    /// 否则 `jsonwebtoken` 11 会返回 `InvalidAudience` 导致全站 401。
    #[test]
    fn token_with_audience_passes_verification() {
        let token = sign(&claims_with_scope());
        let claims = verify_token(&token).expect("带 iss/aud 的访问令牌应通过校验");
        assert_eq!(claims.sub, 1);
        assert_eq!(
            claims.aud.as_deref(),
            Some(service::user_service::jwt_audience().as_str())
        );
    }

    /// 历史令牌（无 `iss` / `aud`）按宽容模式继续放行，避免升级后强制重登
    #[test]
    fn legacy_token_without_scope_still_passes() {
        let mut claims = claims_with_scope();
        claims.iss = None;
        claims.aud = None;
        let token = sign(&claims);
        assert!(verify_token(&token).is_ok());
    }

    /// 受众不匹配必须被拒绝（校验参数确实生效，而不是形同虚设）
    #[test]
    fn mismatched_audience_is_rejected() {
        let mut claims = claims_with_scope();
        claims.aud = Some("some-other-service".to_string());
        let token = sign(&claims);
        assert!(verify_token(&token).is_err());
    }

    /// 非访问令牌（refresh）不得当作 access 使用
    #[test]
    fn refresh_token_is_rejected() {
        let mut claims = claims_with_scope();
        claims.typ = Some("refresh".to_string());
        let token = sign(&claims);
        assert!(verify_token(&token).is_err());
    }

    /// 校验参数与签发参数一致（防止有人改回 `Validation::default()`）
    #[test]
    fn validation_scope_matches_signing_scope() {
        let validation = jwt_validation();
        let audience = validation.aud.expect("audience 必须显式设置");
        assert!(audience.contains(&service::user_service::jwt_audience()));
        let issuer = validation.iss.expect("issuer 必须显式设置");
        assert!(issuer.contains(&service::user_service::jwt_issuer()));
    }

    // ==================== 凭证前缀分流（OAuth2.1 Bearer，A-1…A-5）====================

    /// A-1：`at_` 访问令牌必须分流为 [`Credential::AccessToken`]
    ///
    /// 这是「第三方系统 → 业务接口」整条链路的总开关：一旦分流错，
    /// `at_` 会被当成 JWT 去验签（必然失败 → 全站 401），或者被当成 API Key 查表。
    #[test]
    fn access_token_prefix_is_routed_to_oauth() {
        let token = utils::oauth::generate_access_token().plaintext;
        assert!(token.starts_with("at_"));
        match parse_credential(&token) {
            Ok(Credential::AccessToken(value)) => assert_eq!(value, token),
            other => panic!("at_ 前缀应解析为 AccessToken，实际: {other:?}"),
        }
    }

    /// A-2：刷新令牌出现在资源请求里必须 401，且文案点明「刷新令牌」
    ///
    /// `rt_` 只能用于 `/oauth/token`（设计文档 §13.2 #3），错误文案要能让集成方自查。
    #[test]
    fn refresh_token_prefix_is_rejected_with_actionable_message() {
        let token = utils::oauth::generate_refresh_token().plaintext;
        assert!(token.starts_with("rt_"));

        let err = parse_credential(&token).expect_err("刷新令牌不得用于资源请求");
        assert_eq!(err.code, 401);
        assert!(
            err.message.contains("刷新令牌"),
            "错误文案应提示刷新令牌用途，实际: {}",
            err.message
        );
    }

    /// A-3：格式合法的 `ak_` 密钥分流为 [`Credential::ApiKey`]
    #[test]
    fn api_key_prefix_is_routed_to_api_key() {
        let key = utils::api_key::generate_api_key(utils::api_key::ApiKeyEnv::Live).plaintext;
        match parse_credential(&key) {
            Ok(Credential::ApiKey(value)) => assert_eq!(value, key),
            other => panic!("ak_ 前缀应解析为 ApiKey，实际: {other:?}"),
        }
    }

    /// A-3 反向：`ak_` 前缀但格式非法 → 401，**不得**降级为 JWT 继续验签
    #[test]
    fn malformed_api_key_is_rejected_instead_of_falling_back_to_jwt() {
        let err = parse_credential("ak_live_tooshort_tooshort").expect_err("格式非法应拒绝");
        assert_eq!(err.code, 401);
        assert_eq!(err.message, "凭证无效或已过期");
    }

    /// A-4：空 / 纯空白令牌 → 401「认证令牌为空」
    #[test]
    fn empty_credential_is_rejected() {
        for raw in ["", "   ", "\t\n"] {
            let err = parse_credential(raw).expect_err("空令牌应拒绝");
            assert_eq!(err.code, 401);
            assert!(
                err.message.contains("认证令牌为空"),
                "实际文案: {}",
                err.message
            );
        }
    }

    /// A-5：无前缀的 JWT 仍然走原有分支（回归：别把 JWT 误判成 OAuth 令牌）
    #[test]
    fn jwt_is_routed_to_jwt_branch() {
        let token = sign(&claims_with_scope());
        assert!(token.starts_with("eyJ"), "JWT 应以 eyJ 开头");
        match parse_credential(&token) {
            Ok(Credential::Jwt(value)) => assert_eq!(value, token),
            other => panic!("JWT 应走 Jwt 分支，实际: {other:?}"),
        }
    }
}

