//! OAuth 2.1 授权服务器（最小可用）的数据访问层
//!
//! 设计文档 §7：本模块只做**数据访问与协议数据处理**，
//! 「能不能调这个接口」等 HTTP 鉴权语义仍在 `api` crate（§16 T-1），
//! 因此 Tauri 桌面通道不会感知任何 OAuth 语义（T-8）。
//!
//! 端点与实现的对应关系：
//!
//! | 端点 | 本模块函数 |
//! |---|---|
//! | `GET/POST /oauth/authorize` | [`find_client`] + [`issue_authorization_code`] |
//! | `POST /oauth/token`（code） | [`authenticate_client`] + [`exchange_authorization_code`] |
//! | `POST /oauth/token`（refresh） | [`authenticate_client`] + [`refresh_access_token`] |
//! | `POST /oauth/introspect` | [`introspect`] |
//! | `POST /oauth/revoke` | [`revoke_token`] |
//! | 资源请求的 `at_` 令牌 | [`verify_access_token`] |
//! | `/api/oauth-clients*` | [`list_clients`] / [`create_client`] / [`update_client`] / [`delete_client`] |
//!
//! 安全要点（RFC 9700 / draft 2.1）：
//! - 授权码 / 令牌**只存 SHA-256**，一次性使用，TTL 60s；
//! - PKCE 只支持 `S256`，比较走常数时间；
//! - 刷新令牌每次轮换，检测到重放即撤销整个 `family_id`；
//! - 授权码重放（`used_at` 非空）时撤销该客户端该用户的全部未撤销令牌。

use chrono::{DateTime, Utc};
use serde::Serialize;
use tracing::{info, warn};
use uuid::Uuid;

use crate::api_key_service::normalize_scopes;
use database::models::SysOauthClient;
use utils::oauth::{
    generate_access_token, generate_authorization_code, generate_client_id, generate_client_secret,
    generate_refresh_token, hash_secret, is_valid_code_verifier, verify_pkce_s256, GeneratedSecret,
};
use utils::snowflake::next_id;

/// 授权码有效期（秒）：设计文档 §7.3 规定 60s
pub const CODE_TTL_SECS: i64 = 60;

/// 访问令牌有效期下限 / 上限 / 默认值（秒）
pub const ACCESS_TTL_MIN: i32 = 60;
pub const ACCESS_TTL_MAX: i32 = 86_400;
pub const ACCESS_TTL_DEFAULT: i32 = 900;

/// 刷新令牌有效期下限 / 上限 / 默认值（秒）
pub const REFRESH_TTL_MIN: i32 = 300;
pub const REFRESH_TTL_MAX: i32 = 7_776_000;
pub const REFRESH_TTL_DEFAULT: i32 = 2_592_000;

/// 客户端类型：confidential（服务端应用，有密钥）
pub const CLIENT_TYPE_CONFIDENTIAL: i16 = 1;

/// 客户端类型：public（桌面 / SPA，无密钥，强制 PKCE）
pub const CLIENT_TYPE_PUBLIC: i16 = 2;

/// 令牌类型标记：1 = access，2 = refresh
const TOKEN_TYPE_ACCESS: i16 = 1;
const TOKEN_TYPE_REFRESH: i16 = 2;

/// 支持的授权类型
pub const SUPPORTED_GRANT_TYPES: &[&str] = &["authorization_code", "refresh_token"];

/// OAuth 协议层错误（RFC 6749 §5.2 / RFC 7662 / RFC 7009）
///
/// 与 `api::response::ApiError` 完全隔离：`/oauth/*` 必须返回
/// `{ "error": "...", "error_description": "..." }`，否则第三方客户端库无法解析（§7.2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OauthFailure {
    /// 错误码（invalid_request / invalid_client / invalid_grant / ...）
    pub error: &'static str,
    /// 人类可读说明（不含敏感信息）
    pub description: String,
}

impl OauthFailure {
    /// 构造错误
    pub fn new(error: &'static str, description: impl Into<String>) -> Self {
        Self {
            error,
            description: description.into(),
        }
    }

    /// `invalid_request`：参数缺失 / 格式非法
    pub fn invalid_request(description: impl Into<String>) -> Self {
        Self::new("invalid_request", description)
    }

    /// `invalid_client`：客户端不存在 / 密钥错误 / 禁用
    pub fn invalid_client(description: impl Into<String>) -> Self {
        Self::new("invalid_client", description)
    }

    /// `invalid_grant`：授权码或刷新令牌无效 / 过期 / 重放
    pub fn invalid_grant(description: impl Into<String>) -> Self {
        Self::new("invalid_grant", description)
    }

    /// `invalid_scope`：scope 为空或超出客户端允许范围
    pub fn invalid_scope(description: impl Into<String>) -> Self {
        Self::new("invalid_scope", description)
    }

    /// `server_error`：数据库 / 内部异常（不得泄露细节）
    pub fn server_error(description: impl Into<String>) -> Self {
        Self::new("server_error", description)
    }
}


/// 客户端管理页列表项（**不含**密钥哈希）
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct OauthClientView {
    /// 主键（字符串，避免 JS 精度丢失）
    pub id: String,
    pub client_id: String,
    pub client_name: String,
    /// 1 = confidential，2 = public
    pub client_type: i16,
    pub redirect_uris: Vec<String>,
    pub allowed_scopes: Vec<String>,
    pub grant_types: Vec<String>,
    pub access_token_ttl: i32,
    pub refresh_token_ttl: i32,
    pub enable: bool,
    pub created_at: Option<DateTime<Utc>>,
}

impl From<&SysOauthClient> for OauthClientView {
    fn from(row: &SysOauthClient) -> Self {
        Self {
            id: row.id.to_string(),
            client_id: row.client_id.clone(),
            client_name: row.client_name.clone(),
            client_type: row.client_type,
            redirect_uris: row.redirect_uris.clone(),
            allowed_scopes: row.allowed_scopes.clone(),
            grant_types: row.grant_types.clone(),
            access_token_ttl: row.access_token_ttl,
            refresh_token_ttl: row.refresh_token_ttl,
            enable: row.enable,
            created_at: row.created_at,
        }
    }
}

/// 创建结果（`client_secret` 明文只出现这一次）
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct CreatedOauthClient {
    pub id: String,
    pub client_id: String,
    /// 客户端密钥明文；public client 为 `null`
    pub client_secret: Option<String>,
    pub client_name: String,
    pub client_type: i16,
    pub redirect_uris: Vec<String>,
    pub allowed_scopes: Vec<String>,
    pub grant_types: Vec<String>,
    pub access_token_ttl: i32,
    pub refresh_token_ttl: i32,
}

/// 创建参数
pub struct NewOauthClient<'a> {
    pub client_name: &'a str,
    /// 1 = confidential，2 = public
    pub client_type: i16,
    pub redirect_uris: &'a [String],
    pub allowed_scopes: &'a [String],
    pub grant_types: Option<&'a [String]>,
    pub access_token_ttl: Option<i32>,
    pub refresh_token_ttl: Option<i32>,
    pub tenant_id: Option<i64>,
    pub created_by: Option<i64>,
}

/// 更新参数（`None` 表示保持不变）
pub struct UpdateOauthClient<'a> {
    pub client_name: Option<&'a str>,
    pub redirect_uris: Option<&'a [String]>,
    pub allowed_scopes: Option<&'a [String]>,
    pub grant_types: Option<&'a [String]>,
    pub access_token_ttl: Option<i32>,
    pub refresh_token_ttl: Option<i32>,
    pub enable: Option<bool>,
    pub updated_by: Option<i64>,
}

/// 令牌对（响应体由 `api` 层组装，含 `Cache-Control: no-store`）
#[derive(Debug, Clone)]
pub struct OauthTokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub scopes: Vec<String>,
}

/// 令牌内省结果（RFC 7662 §2.2）
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Introspection {
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
}

impl Introspection {
    /// 无效 / 过期 / 已撤销令牌的统一响应（RFC 7662：只返回 `{"active": false}`）
    pub fn inactive() -> Self {
        Self {
            active: false,
            scope: None,
            client_id: None,
            username: None,
            token_type: None,
            exp: None,
            iat: None,
            sub: None,
        }
    }
}

/// `at_` 令牌校验结果（认证中间件用）
#[derive(Debug, Clone)]
pub struct OauthAccessAuth {
    /// `sys_oauth_token.id`
    pub token_id: i64,
    pub client_id: String,
    /// 授权用户（第三方应用代表该用户调用）
    pub user_id: Option<i64>,
    pub tenant_id: Option<i64>,
    pub scopes: Vec<String>,
    pub expires_at: DateTime<Utc>,
}

// ============================================================================
// 纯函数：scope / 回调地址 / TTL / 客户端入参校验（可单测，不碰数据库）
// ============================================================================

/// 解析 `scope` 参数（空格分隔，去重保序）
///
/// 空串返回空 Vec，由调用方决定是 `invalid_scope` 还是「沿用原 scope」。
pub fn parse_scope(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    let mut scopes: Vec<String> = Vec::new();
    for scope in raw.split_whitespace() {
        let scope = scope.trim();
        if scope.is_empty() {
            continue;
        }
        if !scopes.iter().any(|existing| existing == scope) {
            scopes.push(scope.to_string());
        }
    }
    scopes
}

/// 把 scope 列表拼回 `scope` 响应字段（空格分隔）
pub fn join_scope(scopes: &[String]) -> String {
    scopes.join(" ")
}

/// 校验 scope 非空且 ⊆ 客户端允许范围（设计文档 §7.3）
pub fn ensure_scope_allowed(
    client: &SysOauthClient,
    scopes: &[String],
) -> Result<(), OauthFailure> {
    if scopes.is_empty() {
        return Err(OauthFailure::invalid_scope("scope 不能为空"));
    }
    let outside: Vec<&str> = scopes
        .iter()
        .filter(|scope| !client.allowed_scopes.iter().any(|allowed| allowed == *scope))
        .map(String::as_str)
        .collect();
    if !outside.is_empty() {
        return Err(OauthFailure::invalid_scope(format!(
            "scope 超出客户端允许范围: {}",
            outside.join(" ")
        )));
    }
    Ok(())
}

/// 校验「请求 scope ⊆ 原始 scope」（刷新 / 换码时只能收窄）
pub fn ensure_scope_narrowed(
    requested: &[String],
    original: &[String],
) -> Result<Vec<String>, OauthFailure> {
    if requested.is_empty() {
        return Ok(original.to_vec());
    }
    let outside: Vec<&str> = requested
        .iter()
        .filter(|scope| !original.iter().any(|have| have == *scope))
        .map(String::as_str)
        .collect();
    if !outside.is_empty() {
        return Err(OauthFailure::invalid_scope(format!(
            "scope 只能收窄，不能新增: {}",
            outside.join(" ")
        )));
    }
    Ok(requested.to_vec())
}

/// 访问令牌 TTL：夹到 `[ACCESS_TTL_MIN, ACCESS_TTL_MAX]`，缺省 900s
pub fn clamp_access_token_ttl(ttl: Option<i32>) -> i32 {
    match ttl {
        Some(value) if value > 0 => value.clamp(ACCESS_TTL_MIN, ACCESS_TTL_MAX),
        Some(_) => ACCESS_TTL_DEFAULT,
        None => ACCESS_TTL_DEFAULT,
    }
}

/// 刷新令牌 TTL：夹到 `[REFRESH_TTL_MIN, REFRESH_TTL_MAX]`，缺省 30 天
pub fn clamp_refresh_token_ttl(ttl: Option<i32>) -> i32 {
    match ttl {
        Some(value) if value > 0 => value.clamp(REFRESH_TTL_MIN, REFRESH_TTL_MAX),
        Some(_) => REFRESH_TTL_DEFAULT,
        None => REFRESH_TTL_DEFAULT,
    }
}

/// 校验并规范化 `grant_types`（只允许本项目支持的两类）
pub fn normalize_grant_types(grant_types: Option<&[String]>) -> Result<Vec<String>, String> {
    let Some(grant_types) = grant_types else {
        return Ok(SUPPORTED_GRANT_TYPES
            .iter()
            .map(|value| (*value).to_string())
            .collect());
    };
    let mut normalized: Vec<String> = Vec::new();
    for grant in grant_types {
        let grant = grant.trim();
        if !SUPPORTED_GRANT_TYPES.contains(&grant) {
            return Err(format!("不支持的 grant_type: {}", grant));
        }
        if !normalized.iter().any(|existing| existing == grant) {
            normalized.push(grant.to_string());
        }
    }
    if normalized.is_empty() {
        return Err("grant_types 不能为空".to_string());
    }
    Ok(normalized)
}

/// 校验回调地址：必须 https，或 http + 回环地址；不允许 fragment / userinfo（§7.6）
pub fn validate_redirect_uri(uri: &str) -> Result<(), String> {
    let uri = uri.trim();
    if uri.is_empty() || uri.len() > 2048 {
        return Err("redirect_uri 长度非法".to_string());
    }
    if uri.contains('#') {
        return Err(format!("redirect_uri 不允许包含 fragment: {}", uri));
    }
    let Some(parsed) = split_uri(uri) else {
        return Err(format!("redirect_uri 格式非法: {}", uri));
    };
    if parsed.userinfo {
        return Err(format!("redirect_uri 不允许包含 userinfo: {}", uri));
    }
    match parsed.scheme.as_str() {
        "https" => Ok(()),
        "http" if is_loopback_host(&parsed.host) => Ok(()),
        "http" => Err(format!(
            "redirect_uri 只允许 https（http 仅限回环地址）: {}",
            uri
        )),
        other => Err(format!("redirect_uri 不支持的协议: {}", other)),
    }
}

/// 校验一组回调地址（去重 + 逐条格式校验，至少 1 条）
pub fn validate_redirect_uris(uris: &[String]) -> Result<Vec<String>, String> {
    let mut normalized: Vec<String> = Vec::new();
    for uri in uris {
        let uri = uri.trim();
        if uri.is_empty() {
            continue;
        }
        validate_redirect_uri(uri)?;
        if !normalized.iter().any(|existing| existing == uri) {
            normalized.push(uri.to_string());
        }
    }
    if normalized.is_empty() {
        return Err("redirect_uris 至少需要 1 条".to_string());
    }
    Ok(normalized)
}


/// 回调地址匹配：逐条**精确匹配**，loopback 特例忽略端口（RFC 8252 §7.3）
///
/// 只对 IP 字面量回环地址（`127.0.0.1` / `::1`）忽略端口；
/// `localhost` 按普通主机名处理，仍要求端口一致（防 DNS 重绑定）。
pub fn matches_redirect(registered: &[String], requested: &str) -> bool {
    registered
        .iter()
        .any(|uri| redirect_equals(uri, requested))
}

/// 单条回调地址比较（严格比较 + loopback 端口豁免）
fn redirect_equals(registered: &str, requested: &str) -> bool {
    if registered == requested {
        return true;
    }
    let (Some(registered_parsed), Some(requested_parsed)) =
        (split_uri(registered), split_uri(requested))
    else {
        return false;
    };
    if registered_parsed.userinfo || requested_parsed.userinfo {
        return false;
    }
    if registered_parsed.scheme != requested_parsed.scheme
        || registered_parsed.host != requested_parsed.host
        || registered_parsed.path != requested_parsed.path
    {
        return false;
    }
    // RFC 8252 §7.3：回环 IP 的端口由系统随机分配，登记时忽略端口比较
    if registered_parsed.scheme == "http" && is_loopback_host(&registered_parsed.host) {
        return true;
    }
    registered_parsed.port == requested_parsed.port
}

/// 是否为回环 IP 字面量（`127.0.0.1` / `[::1]` / `::1`）
fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "[::1]" | "::1")
}

/// 解析后的 URI（只覆盖回调地址需要的部分）
struct ParsedUri {
    scheme: String,
    host: String,
    port: Option<u16>,
    /// 路径 + 查询串（原样比较）
    path: String,
    userinfo: bool,
}

/// 极简 URI 解析：`scheme://[userinfo@]host[:port][/path][?query]`
fn split_uri(uri: &str) -> Option<ParsedUri> {
    let (scheme, rest) = uri.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return None;
    }
    let (userinfo, host_port) = match authority.split_once('@') {
        Some((_, host)) => (true, host),
        None => (false, authority),
    };
    let (host, port) = parse_host_port(host_port)?;
    Some(ParsedUri {
        scheme,
        host,
        port,
        path: path.to_string(),
        userinfo,
    })
}

/// 拆分 `host[:port]`，支持 IPv6 字面量 `[::1]:8080`
fn parse_host_port(host_port: &str) -> Option<(String, Option<u16>)> {
    if host_port.is_empty() {
        return None;
    }
    if let Some(rest) = host_port.strip_prefix('[') {
        let (host, tail) = rest.split_once(']')?;
        let host = format!("[{}]", host.to_ascii_lowercase());
        let port = match tail.strip_prefix(':') {
            Some(port) if !port.is_empty() => Some(port.parse().ok()?),
            _ => None,
        };
        return Some((host, port));
    }
    match host_port.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !port.is_empty() => {
            // `host:abc` 视为非法，避免把路径误当主机
            let port = port.parse().ok()?;
            Some((host.to_ascii_lowercase(), Some(port)))
        }
        _ => Some((host_port.to_ascii_lowercase(), None)),
    }
}


// ============================================================================
// 客户端管理（/api/oauth-clients*）
// ============================================================================

/// 生成客户端 ID / 密钥
pub struct GeneratedClient {
    pub client_id: String,
    pub secret: Option<GeneratedSecret>,
}

/// 按客户端类型生成凭据（confidential 带密钥，public 无密钥）
pub fn generate_client_credentials(client_type: i16) -> GeneratedClient {
    let client_id = generate_client_id();
    let secret = if client_type == CLIENT_TYPE_CONFIDENTIAL {
        Some(generate_client_secret())
    } else {
        None
    };
    GeneratedClient { client_id, secret }
}

/// 列出 OAuth 客户端（超管跨租户，其余只看本租户）
pub async fn list_clients(tenant_filter: Option<i64>) -> Result<Vec<OauthClientView>, String> {
    let pool = database::get_read_pool().map_err(|e| e.to_string())?;

    // 静态 SQL（非拼接）：sqlx 0.9 拒绝动态字符串，避免注入面
    let rows: Vec<SysOauthClient> = match tenant_filter {
        Some(tenant_id) => {
            sqlx::query_as(
                "SELECT id, client_id, client_name, client_secret_hash, client_type, \
                        redirect_uris, allowed_scopes, grant_types, access_token_ttl, \
                        refresh_token_ttl, tenant_id, enable, deleted, created_by, created_at, \
                        updated_by, updated_at \
                 FROM public.sys_oauth_client \
                 WHERE (deleted IS NULL OR deleted = 0) AND tenant_id = $1 \
                 ORDER BY created_at DESC NULLS LAST",
            )
            .bind(tenant_id)
            .fetch_all(&pool)
            .await
        }
        None => {
            sqlx::query_as(
                "SELECT id, client_id, client_name, client_secret_hash, client_type, \
                        redirect_uris, allowed_scopes, grant_types, access_token_ttl, \
                        refresh_token_ttl, tenant_id, enable, deleted, created_by, created_at, \
                        updated_by, updated_at \
                 FROM public.sys_oauth_client \
                 WHERE (deleted IS NULL OR deleted = 0) \
                 ORDER BY created_at DESC NULLS LAST",
            )
            .fetch_all(&pool)
            .await
        }
    }
    .map_err(|e| format!("查询 OAuth 客户端列表失败: {}", e))?;

    Ok(rows.iter().map(OauthClientView::from).collect())
}

/// 创建 OAuth 客户端（返回一次性客户端密钥）
pub async fn create_client(params: &NewOauthClient<'_>) -> Result<CreatedOauthClient, String> {
    let name = params.client_name.trim();
    if name.is_empty() {
        return Err("客户端名称不能为空".to_string());
    }
    if name.chars().count() > 100 {
        return Err("客户端名称不能超过 100 个字符".to_string());
    }
    if !matches!(
        params.client_type,
        CLIENT_TYPE_CONFIDENTIAL | CLIENT_TYPE_PUBLIC
    ) {
        return Err("client_type 只能是 1（confidential）或 2（public）".to_string());
    }

    let redirect_uris = validate_redirect_uris(params.redirect_uris)?;
    let allowed_scopes = normalize_scopes(params.allowed_scopes)?;
    let grant_types = normalize_grant_types(params.grant_types)?;
    let access_token_ttl = clamp_access_token_ttl(params.access_token_ttl);
    let refresh_token_ttl = clamp_refresh_token_ttl(params.refresh_token_ttl);

    let generated = generate_client_credentials(params.client_type);
    let pool = database::get_write_pool().map_err(|e| e.to_string())?;
    let id = next_id() as i64;

    sqlx::query(
        "INSERT INTO public.sys_oauth_client \
         (id, client_id, client_name, client_secret_hash, client_type, redirect_uris, \
          allowed_scopes, grant_types, access_token_ttl, refresh_token_ttl, tenant_id, \
          enable, deleted, created_by, created_at, updated_by, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, TRUE, 0, $12, now(), $12, now())",
    )
    .bind(id)
    .bind(&generated.client_id)
    .bind(name)
    .bind(generated.secret.as_ref().map(|secret| secret.hash.clone()))
    .bind(params.client_type)
    .bind(&redirect_uris)
    .bind(&allowed_scopes)
    .bind(&grant_types)
    .bind(access_token_ttl)
    .bind(refresh_token_ttl)
    .bind(params.tenant_id)
    .bind(params.created_by)
    .execute(&pool)
    .await
    .map_err(|e| {
        warn!("创建 OAuth 客户端失败: name={}, error={}", name, e);
        format!("创建 OAuth 客户端失败: {}", e)
    })?;

    info!(
        "OAuth 客户端已创建: id={}, client_id={}, type={}",
        id, generated.client_id, params.client_type
    );

    Ok(CreatedOauthClient {
        id: id.to_string(),
        client_id: generated.client_id,
        client_secret: generated.secret.map(|secret| secret.plaintext),
        client_name: name.to_string(),
        client_type: params.client_type,
        redirect_uris,
        allowed_scopes,
        grant_types,
        access_token_ttl,
        refresh_token_ttl,
    })
}


/// 按主键读取客户端（管理接口用，包含已禁用行）
pub async fn get_client_by_id(id: i64) -> Result<Option<SysOauthClient>, String> {
    let pool = database::get_read_pool().map_err(|e| e.to_string())?;
    sqlx::query_as(
        "SELECT id, client_id, client_name, client_secret_hash, client_type, redirect_uris, \
                allowed_scopes, grant_types, access_token_ttl, refresh_token_ttl, tenant_id, \
                enable, deleted, created_by, created_at, updated_by, updated_at \
         FROM public.sys_oauth_client WHERE id = $1 AND (deleted IS NULL OR deleted = 0)",
    )
    .bind(id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("查询 OAuth 客户端失败: {}", e))
}

/// 按 `client_id` 读取**可用**客户端（authorize / token 端点用）
pub async fn find_client(client_id: &str) -> Result<Option<SysOauthClient>, String> {
    let pool = database::get_read_pool().map_err(|e| e.to_string())?;
    sqlx::query_as(
        "SELECT id, client_id, client_name, client_secret_hash, client_type, redirect_uris, \
                allowed_scopes, grant_types, access_token_ttl, refresh_token_ttl, tenant_id, \
                enable, deleted, created_by, created_at, updated_by, updated_at \
         FROM public.sys_oauth_client \
         WHERE client_id = $1 AND enable = TRUE AND (deleted IS NULL OR deleted = 0)",
    )
    .bind(client_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("查询 OAuth 客户端失败: {}", e))
}

/// 客户端认证（`client_secret_basic` / `client_secret_post` / public client）
///
/// - confidential：必须提供密钥，且 `SHA-256(secret)` 与库中哈希常数时间相等；
/// - public：不需要密钥，但必须使用 PKCE（由 authorize / token 强制）。
pub async fn authenticate_client(
    client_id: &str,
    client_secret: Option<&str>,
) -> Result<SysOauthClient, OauthFailure> {
    let client = find_client(client_id)
        .await
        .map_err(|e| {
            warn!("查询 OAuth 客户端失败: client_id={}, error={}", client_id, e);
            OauthFailure::server_error("客户端校验服务暂时不可用")
        })?
        .ok_or_else(|| OauthFailure::invalid_client("客户端不存在或已禁用"))?;

    if client.client_type == CLIENT_TYPE_CONFIDENTIAL {
        let Some(secret) = client_secret.filter(|secret| !secret.is_empty()) else {
            return Err(OauthFailure::invalid_client("缺少 client_secret"));
        };
        let Some(stored) = client.client_secret_hash.as_deref() else {
            warn!("confidential 客户端缺少密钥哈希: client_id={}", client_id);
            return Err(OauthFailure::invalid_client("客户端配置异常"));
        };
        if !utils::oauth::constant_time_eq(&hash_secret(secret), stored) {
            warn!("OAuth 客户端密钥错误: client_id={}", client_id);
            return Err(OauthFailure::invalid_client("client_secret 错误"));
        }
    }

    Ok(client)
}

/// 客户端是否允许某 grant_type
pub fn client_allows_grant(client: &SysOauthClient, grant_type: &str) -> bool {
    client.grant_types.iter().any(|grant| grant == grant_type)
}


/// 更新 OAuth 客户端（`None` 字段保持不变）
pub async fn update_client(
    id: i64,
    params: &UpdateOauthClient<'_>,
) -> Result<OauthClientView, String> {
    let pool = database::get_write_pool().map_err(|e| e.to_string())?;

    let current = get_client_by_id(id)
        .await?
        .ok_or_else(|| "OAuth 客户端不存在".to_string())?;

    let name = match params.client_name {
        Some(name) => {
            let name = name.trim();
            if name.is_empty() {
                return Err("客户端名称不能为空".to_string());
            }
            if name.chars().count() > 100 {
                return Err("客户端名称不能超过 100 个字符".to_string());
            }
            name.to_string()
        }
        None => current.client_name.clone(),
    };

    let redirect_uris = match params.redirect_uris {
        Some(uris) => validate_redirect_uris(uris)?,
        None => current.redirect_uris.clone(),
    };
    let allowed_scopes = match params.allowed_scopes {
        Some(scopes) => normalize_scopes(scopes)?,
        None => current.allowed_scopes.clone(),
    };
    let grant_types = match params.grant_types {
        Some(grant_types) => normalize_grant_types(Some(grant_types))?,
        None => current.grant_types.clone(),
    };
    let access_token_ttl = params.access_token_ttl.map_or(current.access_token_ttl, |ttl| {
        clamp_access_token_ttl(Some(ttl))
    });
    let refresh_token_ttl = params.refresh_token_ttl.map_or(current.refresh_token_ttl, |ttl| {
        clamp_refresh_token_ttl(Some(ttl))
    });
    let enable = params.enable.unwrap_or(current.enable);

    sqlx::query(
        "UPDATE public.sys_oauth_client SET \
         client_name = $2, redirect_uris = $3, allowed_scopes = $4, grant_types = $5, \
         access_token_ttl = $6, refresh_token_ttl = $7, enable = $8, \
         updated_by = $9, updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(&name)
    .bind(&redirect_uris)
    .bind(&allowed_scopes)
    .bind(&grant_types)
    .bind(access_token_ttl)
    .bind(refresh_token_ttl)
    .bind(enable)
    .bind(params.updated_by)
    .execute(&pool)
    .await
    .map_err(|e| format!("更新 OAuth 客户端失败: {}", e))?;

    // 禁用客户端时立即撤销它已签发的全部令牌
    if !enable {
        revoke_client_tokens(&current.client_id).await?;
    }

    info!("OAuth 客户端已更新: id={}, enable={}", id, enable);

    get_client_by_id(id)
        .await?
        .map(|row| OauthClientView::from(&row))
        .ok_or_else(|| "OAuth 客户端不存在".to_string())
}

/// 删除 OAuth 客户端（软删除 + 撤销其全部令牌，行保留用于审计）
pub async fn delete_client(id: i64, updated_by: Option<i64>) -> Result<(), String> {
    let pool = database::get_write_pool().map_err(|e| e.to_string())?;

    let client = get_client_by_id(id)
        .await?
        .ok_or_else(|| "OAuth 客户端不存在".to_string())?;

    sqlx::query(
        "UPDATE public.sys_oauth_client \
         SET deleted = 1, enable = FALSE, updated_by = $2, updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(updated_by)
    .execute(&pool)
    .await
    .map_err(|e| format!("删除 OAuth 客户端失败: {}", e))?;

    revoke_client_tokens(&client.client_id).await?;
    info!(
        "OAuth 客户端已删除: id={}, client_id={}",
        id, client.client_id
    );
    Ok(())
}

/// 撤销某客户端的全部有效令牌（禁用 / 删除客户端时调用）
async fn revoke_client_tokens(client_id: &str) -> Result<(), String> {
    let pool = database::get_write_pool().map_err(|e| e.to_string())?;
    sqlx::query(
        "UPDATE public.sys_oauth_token SET revoked_at = now() \
         WHERE client_id = $1 AND revoked_at IS NULL",
    )
    .bind(client_id)
    .execute(&pool)
    .await
    .map_err(|e| format!("撤销客户端令牌失败: {}", e))?;
    Ok(())
}


// ============================================================================
// /oauth/authorize：参数校验 + 授权码签发（方案 A：Bearer + 同意页）
// ============================================================================

/// authorize 请求参数（GET query / POST body 共用）
#[derive(Debug, Clone)]
pub struct AuthorizeRequest<'a> {
    pub client_id: &'a str,
    pub redirect_uri: &'a str,
    pub response_type: &'a str,
    pub scope: &'a str,
    pub state: &'a str,
    pub code_challenge: &'a str,
    /// 缺省视为 `S256`，`plain` 一律拒绝
    pub code_challenge_method: Option<&'a str>,
}

/// 校验通过的授权请求（同意页与签发授权码共用）
#[derive(Debug, Clone)]
pub struct AuthorizationGrant {
    pub client: SysOauthClient,
    /// 规范化后的 scope（去重保序）
    pub scopes: Vec<String>,
    pub redirect_uri: String,
    pub state: String,
    pub code_challenge: String,
}

/// 校验 authorize 参数（顺序敏感：先校验，再做任何跳转，见 §7.3）
pub async fn prepare_authorization(
    req: &AuthorizeRequest<'_>,
) -> Result<AuthorizationGrant, OauthFailure> {
    // 1. client_id
    let client_id = req.client_id.trim();
    if client_id.is_empty() {
        return Err(OauthFailure::invalid_request("缺少 client_id"));
    }
    let client = find_client(client_id)
        .await
        .map_err(|e| {
            warn!("authorize 查询客户端失败: client_id={}, error={}", client_id, e);
            OauthFailure::server_error("客户端校验服务暂时不可用")
        })?
        .ok_or_else(|| OauthFailure::invalid_client("客户端不存在或已禁用"))?;

    // 2. redirect_uri 必须精确命中登记值（不匹配时不得重定向）
    let redirect_uri = req.redirect_uri.trim();
    if redirect_uri.is_empty() {
        return Err(OauthFailure::invalid_request("缺少 redirect_uri"));
    }
    if !matches_redirect(&client.redirect_uris, redirect_uri) {
        warn!(
            client_id = %client.client_id,
            "authorize 的 redirect_uri 与登记值不匹配，拒绝（不跳转）"
        );
        return Err(OauthFailure::invalid_request(
            "redirect_uri 未在客户端登记（必须精确匹配）",
        ));
    }

    // 3. response_type：必填且只支持 code
    let response_type = req.response_type.trim();
    if response_type.is_empty() {
        return Err(OauthFailure::invalid_request("缺少 response_type"));
    }
    if response_type != "code" {
        return Err(OauthFailure::new(
            "unsupported_response_type",
            "仅支持 response_type=code",
        ));
    }

    // 4. PKCE：必填 challenge，方法只允许 S256
    let code_challenge = req.code_challenge.trim();
    if code_challenge.is_empty() {
        return Err(OauthFailure::invalid_request("缺少 code_challenge（强制 PKCE）"));
    }
    if code_challenge.len() > 128 {
        return Err(OauthFailure::invalid_request("code_challenge 长度非法"));
    }
    let method = req
        .code_challenge_method
        .map(str::trim)
        .filter(|method| !method.is_empty())
        .unwrap_or("S256");
    if method != "S256" {
        return Err(OauthFailure::invalid_request(
            "code_challenge_method 只支持 S256（拒绝 plain）",
        ));
    }

    // 5. state：必填（CSRF），长度 ≤ 512
    let state = req.state.trim();
    if state.is_empty() {
        return Err(OauthFailure::invalid_request("缺少 state（CSRF 防护必需）"));
    }
    if state.len() > 512 {
        return Err(OauthFailure::invalid_request("state 长度不能超过 512"));
    }

    // 6. scope：必填且 ⊆ client.allowed_scopes
    let scopes = parse_scope(Some(req.scope));
    ensure_scope_allowed(&client, &scopes)?;

    // 7. 客户端必须允许 authorization_code
    if !client_allows_grant(&client, "authorization_code") {
        return Err(OauthFailure::new(
            "unauthorized_client",
            "该客户端不允许 authorization_code 授权",
        ));
    }

    Ok(AuthorizationGrant {
        client,
        scopes,
        redirect_uri: redirect_uri.to_string(),
        state: state.to_string(),
        code_challenge: code_challenge.to_string(),
    })
}


/// 校验用户可用（登录态由 `auth_middleware` 保证，这里只查状态位）
pub async fn ensure_user_active(user_id: i64) -> Result<(), OauthFailure> {
    let pool = database::get_read_pool().map_err(|e| {
        warn!("查询用户状态失败（连接池）: user_id={}, error={}", user_id, e);
        OauthFailure::server_error("授权服务暂时不可用")
    })?;

    let row: Option<(i16, Option<i16>)> =
        sqlx::query_as("SELECT status, deleted FROM public.sys_user WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&pool)
            .await
            .map_err(|e| {
                warn!("查询用户状态失败: user_id={}, error={}", user_id, e);
                OauthFailure::server_error("授权服务暂时不可用")
            })?;

    match row {
        Some((1, deleted)) if deleted.unwrap_or(0) == 0 => Ok(()),
        Some(_) => Err(OauthFailure::new("access_denied", "账号已被禁用，无法授权")),
        None => Err(OauthFailure::new("access_denied", "用户不存在")),
    }
}

/// 签发授权码（返回明文，只在此处出现一次）
pub async fn issue_authorization_code(
    grant: &AuthorizationGrant,
    user_id: i64,
    tenant_id: Option<i64>,
) -> Result<String, OauthFailure> {
    ensure_user_active(user_id).await?;

    // 客户端绑定租户时，只允许该租户的用户授权（防止跨租户越权）
    if let Some(client_tenant) = grant.client.tenant_id {
        if Some(client_tenant) != tenant_id {
            return Err(OauthFailure::invalid_request(
                "当前登录用户不属于该客户端绑定的租户",
            ));
        }
    }

    let code = generate_authorization_code();
    let pool = database::get_write_pool().map_err(|e| {
        warn!("签发授权码失败（连接池）: {}", e);
        OauthFailure::server_error("授权服务暂时不可用")
    })?;
    let id = next_id() as i64;

    sqlx::query(
        "INSERT INTO public.sys_oauth_code \
         (id, code_hash, client_id, user_id, tenant_id, redirect_uri, scopes, \
          code_challenge, code_challenge_method, expires_at, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'S256', \
                 now() + make_interval(secs => $9::double precision), now())",
    )
    .bind(id)
    .bind(&code.hash)
    .bind(&grant.client.client_id)
    .bind(user_id)
    .bind(tenant_id)
    .bind(&grant.redirect_uri)
    .bind(&grant.scopes)
    .bind(&grant.code_challenge)
    .bind(CODE_TTL_SECS as f64)
    .execute(&pool)
    .await
    .map_err(|e| {
        warn!(
            "保存授权码失败: client_id={}, error={}",
            grant.client.client_id, e
        );
        OauthFailure::server_error("授权服务暂时不可用")
    })?;

    info!(
        "OAuth 授权码已签发: client_id={}, user_id={}, scopes={:?}",
        grant.client.client_id, user_id, grant.scopes
    );

    Ok(code.plaintext)
}


// ============================================================================
// /oauth/token：授权码换令牌 + 刷新轮换
// ============================================================================

/// 在事务内写入一对令牌（access + refresh，同一 `family_id`）
async fn insert_token_pair(
    tx: &mut sqlx::PgConnection,
    client: &SysOauthClient,
    user_id: Option<i64>,
    tenant_id: Option<i64>,
    scopes: &[String],
    family_id: &str,
) -> Result<OauthTokenPair, OauthFailure> {
    let access = generate_access_token();
    let refresh = generate_refresh_token();
    let access_ttl = clamp_access_token_ttl(Some(client.access_token_ttl));
    let refresh_ttl = clamp_refresh_token_ttl(Some(client.refresh_token_ttl));

    let insert_sql = "INSERT INTO public.sys_oauth_token \
         (id, token_hash, token_type, client_id, user_id, tenant_id, scopes, family_id, \
          expires_at, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8::uuid, \
                 now() + make_interval(secs => $9::double precision), now())";

    sqlx::query(insert_sql)
        .bind(next_id() as i64)
        .bind(&access.hash)
        .bind(TOKEN_TYPE_ACCESS)
        .bind(&client.client_id)
        .bind(user_id)
        .bind(tenant_id)
        .bind(scopes)
        .bind(family_id)
        .bind(f64::from(access_ttl))
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            warn!("保存访问令牌失败: client_id={}, error={}", client.client_id, e);
            OauthFailure::server_error("令牌签发失败")
        })?;

    sqlx::query(insert_sql)
        .bind(next_id() as i64)
        .bind(&refresh.hash)
        .bind(TOKEN_TYPE_REFRESH)
        .bind(&client.client_id)
        .bind(user_id)
        .bind(tenant_id)
        .bind(scopes)
        .bind(family_id)
        .bind(f64::from(refresh_ttl))
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            warn!("保存刷新令牌失败: client_id={}, error={}", client.client_id, e);
            OauthFailure::server_error("令牌签发失败")
        })?;

    Ok(OauthTokenPair {
        access_token: access.plaintext,
        refresh_token: refresh.plaintext,
        expires_in: i64::from(access_ttl),
        scopes: scopes.to_vec(),
    })
}

/// 撤销某轮换族内的全部令牌（检测到刷新令牌重放时调用）
async fn revoke_token_family(
    tx: &mut sqlx::PgConnection,
    family_id: &str,
) -> Result<(), OauthFailure> {
    sqlx::query(
        "UPDATE public.sys_oauth_token SET revoked_at = now() \
         WHERE family_id = $1::uuid AND revoked_at IS NULL",
    )
    .bind(family_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        warn!("撤销令牌族失败: family_id={}, error={}", family_id, e);
        OauthFailure::server_error("令牌撤销失败")
    })?;
    Ok(())
}


/// 授权码换令牌（`grant_type=authorization_code`）
///
/// 校验顺序见 §7.4：code 一次性 → redirect_uri 完全一致 → PKCE S256 → 用户可用 → scope 收窄。
pub async fn exchange_authorization_code(
    client: &SysOauthClient,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
    requested_scope: Option<&str>,
) -> Result<OauthTokenPair, OauthFailure> {
    if !client_allows_grant(client, "authorization_code") {
        return Err(OauthFailure::new(
            "unauthorized_client",
            "该客户端不允许 authorization_code 授权",
        ));
    }
    if code.trim().is_empty() {
        return Err(OauthFailure::invalid_request("缺少 code"));
    }
    if redirect_uri.trim().is_empty() {
        return Err(OauthFailure::invalid_request("缺少 redirect_uri"));
    }
    if !is_valid_code_verifier(code_verifier) {
        return Err(OauthFailure::invalid_request(
            "code_verifier 必须是 43–128 位 [A-Za-z0-9-._~] 字符",
        ));
    }

    let code_hash = hash_secret(code.trim());
    let pool = database::get_write_pool().map_err(|e| {
        warn!("授权码换令牌失败（连接池）: {}", e);
        OauthFailure::server_error("令牌服务暂时不可用")
    })?;
    let mut tx = pool.begin().await.map_err(|e| {
        warn!("开启事务失败: {}", e);
        OauthFailure::server_error("令牌服务暂时不可用")
    })?;

    type CodeRow = (
        String,
        i64,
        Option<i64>,
        String,
        Vec<String>,
        String,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
    );
    // 行锁：同一授权码并发交换只允许一个成功
    let row: Option<CodeRow> = sqlx::query_as(
        "SELECT client_id, user_id, tenant_id, redirect_uri, scopes, code_challenge, \
                expires_at, used_at \
         FROM public.sys_oauth_code WHERE code_hash = $1 FOR UPDATE",
    )
    .bind(&code_hash)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| {
        warn!("查询授权码失败: {}", e);
        OauthFailure::server_error("令牌服务暂时不可用")
    })?;

    let Some((
        code_client_id,
        user_id,
        tenant_id,
        stored_redirect,
        scopes,
        challenge,
        expires_at,
        used_at,
    )) = row
    else {
        return Err(OauthFailure::invalid_grant("授权码无效或已过期"));
    };

    // 重放：撤销该客户端该用户的全部未撤销令牌（§7.4 第 2 点）
    if used_at.is_some() {
        warn!(
            "检测到授权码重放: client_id={}, user_id={}，撤销该用户的全部 OAuth 令牌",
            code_client_id, user_id
        );
        sqlx::query(
            "UPDATE public.sys_oauth_token SET revoked_at = now() \
             WHERE client_id = $1 AND user_id = $2 AND revoked_at IS NULL",
        )
        .bind(&code_client_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            warn!("重放处置失败: {}", e);
            OauthFailure::server_error("令牌服务暂时不可用")
        })?;
        tx.commit().await.map_err(|e| {
            warn!("提交事务失败: {}", e);
            OauthFailure::server_error("令牌服务暂时不可用")
        })?;
        return Err(OauthFailure::invalid_grant("授权码已被使用，请重新授权"));
    }

    if expires_at <= Utc::now() {
        return Err(OauthFailure::invalid_grant("授权码已过期，请重新授权"));
    }
    if code_client_id != client.client_id {
        return Err(OauthFailure::invalid_grant("授权码与客户端不匹配"));
    }
    if stored_redirect != redirect_uri.trim() {
        return Err(OauthFailure::invalid_grant("redirect_uri 与授权时不一致"));
    }

    // PKCE：失败即作废该 code，防爆破
    if !verify_pkce_s256(code_verifier, &challenge) {
        warn!(
            "PKCE 校验失败，作废授权码: client_id={}, user_id={}",
            code_client_id, user_id
        );
        sqlx::query("UPDATE public.sys_oauth_code SET used_at = now() WHERE code_hash = $1")
            .bind(&code_hash)
            .execute(&mut *tx)
            .await
            .ok();
        tx.commit().await.ok();
        return Err(OauthFailure::invalid_grant(
            "code_verifier 与 code_challenge 不匹配",
        ));
    }

    ensure_user_active(user_id).await?;

    let narrowed = ensure_scope_narrowed(&parse_scope(requested_scope), &scopes)?;

    sqlx::query("UPDATE public.sys_oauth_code SET used_at = now() WHERE code_hash = $1")
        .bind(&code_hash)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            warn!("标记授权码已使用失败: {}", e);
            OauthFailure::server_error("令牌服务暂时不可用")
        })?;

    let family_id = Uuid::new_v4().to_string();
    let pair = insert_token_pair(
        &mut tx,
        client,
        Some(user_id),
        tenant_id,
        &narrowed,
        &family_id,
    )
    .await?;

    tx.commit().await.map_err(|e| {
        warn!("提交事务失败: {}", e);
        OauthFailure::server_error("令牌服务暂时不可用")
    })?;

    info!(
        "OAuth 授权码已换取令牌: client_id={}, user_id={}, family_id={}",
        client.client_id, user_id, family_id
    );

    Ok(pair)
}


/// 刷新令牌轮换（`grant_type=refresh_token`）
///
/// - 每次刷新都轮换：旧 refresh 置 `revoked_at` + `replaced_by`，新令牌同 `family_id`；
/// - 已撤销的 refresh 再次出现视为**重放**，撤销整族（§7.4 第 3 点）；
/// - scope 只能收窄。
pub async fn refresh_access_token(
    client: &SysOauthClient,
    refresh_token: &str,
    requested_scope: Option<&str>,
) -> Result<OauthTokenPair, OauthFailure> {
    if !client_allows_grant(client, "refresh_token") {
        return Err(OauthFailure::new(
            "unauthorized_client",
            "该客户端不允许 refresh_token 授权",
        ));
    }
    let refresh_token = refresh_token.trim();
    if refresh_token.is_empty() {
        return Err(OauthFailure::invalid_request("缺少 refresh_token"));
    }

    let token_hash = hash_secret(refresh_token);
    let pool = database::get_write_pool().map_err(|e| {
        warn!("刷新令牌失败（连接池）: {}", e);
        OauthFailure::server_error("令牌服务暂时不可用")
    })?;
    let mut tx = pool.begin().await.map_err(|e| {
        warn!("开启事务失败: {}", e);
        OauthFailure::server_error("令牌服务暂时不可用")
    })?;

    type RefreshRow = (
        String,
        Option<i64>,
        Option<i64>,
        Vec<String>,
        String,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
    );
    // 行锁：同一 refresh 并发刷新只允许一个成功
    let row: Option<RefreshRow> = sqlx::query_as(
        "SELECT client_id, user_id, tenant_id, scopes, family_id::text, expires_at, revoked_at \
         FROM public.sys_oauth_token \
         WHERE token_hash = $1 AND token_type = $2 FOR UPDATE",
    )
    .bind(&token_hash)
    .bind(TOKEN_TYPE_REFRESH)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| {
        warn!("查询刷新令牌失败: {}", e);
        OauthFailure::server_error("令牌服务暂时不可用")
    })?;

    let Some((token_client_id, user_id, tenant_id, scopes, family_id, expires_at, revoked_at)) = row
    else {
        return Err(OauthFailure::invalid_grant("刷新令牌无效"));
    };

    // 重放：整族撤销
    if revoked_at.is_some() {
        warn!(
            "检测到刷新令牌重放: client_id={}, family_id={}，撤销整个令牌族",
            token_client_id, family_id
        );
        revoke_token_family(&mut tx, &family_id).await?;
        tx.commit().await.map_err(|e| {
            warn!("提交事务失败: {}", e);
            OauthFailure::server_error("令牌服务暂时不可用")
        })?;
        return Err(OauthFailure::invalid_grant(
            "刷新令牌已被使用（检测到重放），请重新授权",
        ));
    }

    if expires_at <= Utc::now() {
        return Err(OauthFailure::invalid_grant("刷新令牌已过期，请重新授权"));
    }
    if token_client_id != client.client_id {
        return Err(OauthFailure::invalid_grant("刷新令牌与客户端不匹配"));
    }
    let Some(user_id) = user_id else {
        return Err(OauthFailure::invalid_grant("刷新令牌缺少授权用户"));
    };
    ensure_user_active(user_id).await?;

    let narrowed = ensure_scope_narrowed(&parse_scope(requested_scope), &scopes)?;

    let pair = insert_token_pair(
        &mut tx,
        client,
        Some(user_id),
        tenant_id,
        &narrowed,
        &family_id,
    )
    .await?;

    // 旧 refresh 标记为已轮换，便于审计与重放检测
    sqlx::query(
        "UPDATE public.sys_oauth_token \
         SET revoked_at = now(), replaced_by = $2 \
         WHERE token_hash = $1 AND revoked_at IS NULL",
    )
    .bind(&token_hash)
    .bind(hash_secret(&pair.refresh_token))
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        warn!("标记旧刷新令牌失败: {}", e);
        OauthFailure::server_error("令牌服务暂时不可用")
    })?;

    tx.commit().await.map_err(|e| {
        warn!("提交事务失败: {}", e);
        OauthFailure::server_error("令牌服务暂时不可用")
    })?;

    info!(
        "OAuth 刷新令牌轮换成功: client_id={}, user_id={}, family_id={}",
        client.client_id, user_id, family_id
    );

    Ok(pair)
}

/// 校验 `at_` 访问令牌（认证中间件调用）
///
/// - 只接受 `token_type = 1`（access），refresh 令牌当作资源凭证一律无效（§13.2 #3）；
/// - 客户端被禁用 / 删除后令牌立即失效；
/// - 回源失败返回 `Err`，调用方按 500 处理（**不得降级为放行**，§6.8）。
pub async fn verify_access_token(raw_token: &str) -> Result<Option<OauthAccessAuth>, String> {
    let token = raw_token.trim();
    if token.is_empty() {
        return Ok(None);
    }

    let pool = database::get_read_pool().map_err(|e| e.to_string())?;
    let row: Option<(i64, String, Option<i64>, Option<i64>, Vec<String>, DateTime<Utc>)> =
        sqlx::query_as(
            "SELECT t.id, t.client_id, t.user_id, t.tenant_id, t.scopes, t.expires_at \
             FROM public.sys_oauth_token t \
             JOIN public.sys_oauth_client c \
               ON c.client_id = t.client_id \
              AND c.enable = TRUE \
              AND (c.deleted IS NULL OR c.deleted = 0) \
             WHERE t.token_hash = $1 AND t.token_type = $2 \
               AND t.revoked_at IS NULL AND t.expires_at > now()",
        )
        .bind(hash_secret(token))
        .bind(TOKEN_TYPE_ACCESS)
        .fetch_optional(&pool)
        .await
        .map_err(|e| format!("校验 OAuth 访问令牌失败: {}", e))?;

    Ok(row.map(
        |(token_id, client_id, user_id, tenant_id, scopes, expires_at)| OauthAccessAuth {
            token_id,
            client_id,
            user_id,
            tenant_id,
            scopes,
            expires_at,
        },
    ))
}


// ============================================================================
// /oauth/introspect、/oauth/revoke 与清理任务
// ============================================================================

/// 令牌内省（RFC 7662）：调用方只能内省**自己**签发的令牌
pub async fn introspect(caller_client_id: &str, raw_token: &str) -> Result<Introspection, String> {
    let token = raw_token.trim();
    if token.is_empty() {
        return Ok(Introspection::inactive());
    }

    let pool = database::get_read_pool().map_err(|e| e.to_string())?;
    type IntrospectRow = (
        i16,
        String,
        Option<i64>,
        Vec<String>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        DateTime<Utc>,
    );
    let row: Option<IntrospectRow> = sqlx::query_as(
        "SELECT token_type, client_id, user_id, scopes, expires_at, revoked_at, created_at \
         FROM public.sys_oauth_token WHERE token_hash = $1",
    )
    .bind(hash_secret(token))
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("内省令牌失败: {}", e))?;

    let Some((token_type, client_id, user_id, scopes, expires_at, revoked_at, created_at)) = row
    else {
        return Ok(Introspection::inactive());
    };

    // 令牌不属于调用方 / 已撤销 / 已过期 → 一律 inactive（不泄露存在性）
    if client_id != caller_client_id || revoked_at.is_some() || expires_at <= Utc::now() {
        return Ok(Introspection::inactive());
    }

    let username = match user_id {
        Some(user_id) => sqlx::query_scalar::<_, String>(
            "SELECT username FROM public.sys_user WHERE id = $1",
        )
        .bind(user_id)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten(),
        None => None,
    };

    Ok(Introspection {
        active: true,
        scope: Some(join_scope(&scopes)),
        client_id: Some(client_id),
        username: username.clone(),
        token_type: Some(if token_type == TOKEN_TYPE_ACCESS {
            "Bearer".to_string()
        } else {
            "refresh_token".to_string()
        }),
        exp: Some(expires_at.timestamp()),
        iat: Some(created_at.timestamp()),
        sub: user_id.map(|user_id| user_id.to_string()),
    })
}

/// 撤销令牌（RFC 7009）
///
/// - 找不到 / 不属于该客户端：静默成功（返回 200，避免探测令牌存在性）；
/// - 撤销 refresh 时同时撤销整族（含已签发的 access）。
pub async fn revoke_token(client_id: &str, raw_token: &str) -> Result<(), String> {
    let token = raw_token.trim();
    if token.is_empty() {
        return Ok(());
    }

    let pool = database::get_write_pool().map_err(|e| e.to_string())?;
    let row: Option<(String, i16, String)> = sqlx::query_as(
        "SELECT client_id, token_type, family_id::text \
         FROM public.sys_oauth_token WHERE token_hash = $1",
    )
    .bind(hash_secret(token))
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("撤销令牌失败: {}", e))?;

    let Some((token_client_id, token_type, family_id)) = row else {
        return Ok(());
    };
    if token_client_id != client_id {
        warn!(
            "拒绝跨客户端撤销令牌: caller={}, token_owner={}",
            client_id, token_client_id
        );
        return Ok(());
    }

    if token_type == TOKEN_TYPE_REFRESH {
        sqlx::query(
            "UPDATE public.sys_oauth_token SET revoked_at = now() \
             WHERE family_id = $1::uuid AND revoked_at IS NULL",
        )
        .bind(&family_id)
        .execute(&pool)
        .await
        .map_err(|e| format!("撤销令牌族失败: {}", e))?;
        info!(
            "OAuth 刷新令牌已撤销（整族）: client_id={}, family_id={}",
            client_id, family_id
        );
    } else {
        // 只撤销这一个 access 令牌：同一轮换族在刷新后仍可能保留旧的 access
        // （其 TTL 很短，靠过期自然失效），不能误伤整族
        sqlx::query(
            "UPDATE public.sys_oauth_token SET revoked_at = now() \
             WHERE token_hash = $1 AND revoked_at IS NULL",
        )
        .bind(hash_secret(token))
        .execute(&pool)
        .await
        .map_err(|e| format!("撤销访问令牌失败: {}", e))?;
        info!("OAuth 访问令牌已撤销: client_id={}", client_id);
    }

    Ok(())
}

/// 清理历史记录：授权码保留 24h、令牌过期后保留 30d（§9.2）
///
/// 单实例部署，由运维 / 定时任务调用即可；不在请求路径上自动执行。
pub async fn cleanup_expired_grants() -> Result<(u64, u64), String> {
    let pool = database::get_write_pool().map_err(|e| e.to_string())?;

    let codes = sqlx::query(
        "DELETE FROM public.sys_oauth_code WHERE expires_at < now() - interval '24 hours'",
    )
    .execute(&pool)
    .await
    .map_err(|e| format!("清理过期授权码失败: {}", e))?
    .rows_affected();

    let tokens = sqlx::query(
        "DELETE FROM public.sys_oauth_token WHERE expires_at < now() - interval '30 days'",
    )
    .execute(&pool)
    .await
    .map_err(|e| format!("清理过期令牌失败: {}", e))?
    .rows_affected();

    info!("OAuth 历史数据已清理: codes={}, tokens={}", codes, tokens);
    Ok((codes, tokens))
}


#[cfg(test)]
mod tests {
    use super::*;

    fn client(allowed_scopes: &[&str]) -> SysOauthClient {
        SysOauthClient {
            id: 1,
            client_id: "oc_test".to_string(),
            client_name: "测试客户端".to_string(),
            client_secret_hash: None,
            client_type: CLIENT_TYPE_PUBLIC,
            redirect_uris: vec!["https://app.example.com/cb".to_string()],
            allowed_scopes: allowed_scopes.iter().map(|s| s.to_string()).collect(),
            grant_types: SUPPORTED_GRANT_TYPES
                .iter()
                .map(|s| s.to_string())
                .collect(),
            access_token_ttl: ACCESS_TTL_DEFAULT,
            refresh_token_ttl: REFRESH_TTL_DEFAULT,
            tenant_id: None,
            enable: true,
            deleted: 0,
            created_by: None,
            created_at: None,
            updated_by: None,
            updated_at: None,
        }
    }

    #[test]
    fn scope_parsing_dedupes_and_ignores_blank() {
        assert_eq!(
            parse_scope(Some("asset:hardware:list   asset:* asset:hardware:list")),
            vec!["asset:hardware:list".to_string(), "asset:*".to_string()]
        );
        assert!(parse_scope(None).is_empty());
        assert!(parse_scope(Some("   ")).is_empty());
        assert_eq!(
            join_scope(&["a:b:c".to_string(), "d:e:f".to_string()]),
            "a:b:c d:e:f"
        );
    }

    #[test]
    fn scope_must_be_subset_of_client_allowlist() {
        let client = client(&["asset:hardware:list", "asset:*"]);
        assert!(ensure_scope_allowed(&client, &["asset:hardware:list".to_string()]).is_ok());
        // 空 scope 直接拒绝
        let empty = ensure_scope_allowed(&client, &[]).expect_err("空 scope 必须拒绝");
        assert_eq!(empty.error, "invalid_scope");
        // 超出允许范围（不做通配扩展：`asset:*` 只是字面量）
        let outside = ensure_scope_allowed(&client, &["process:receive:add".to_string()])
            .expect_err("超出范围必须拒绝");
        assert_eq!(outside.error, "invalid_scope");
    }

    #[test]
    fn scope_can_only_be_narrowed() {
        let original = vec![
            "asset:hardware:list".to_string(),
            "asset:hardware:add".to_string(),
        ];
        assert_eq!(
            ensure_scope_narrowed(&[], &original).expect("空表示沿用原 scope"),
            original
        );
        assert_eq!(
            ensure_scope_narrowed(&["asset:hardware:list".to_string()], &original)
                .expect("收窄应通过"),
            vec!["asset:hardware:list".to_string()]
        );
        let err = ensure_scope_narrowed(&["system:user:add".to_string()], &original)
            .expect_err("扩大 scope 必须拒绝");
        assert_eq!(err.error, "invalid_scope");
    }

    #[test]
    fn ttl_values_are_clamped() {
        assert_eq!(clamp_access_token_ttl(None), 900);
        assert_eq!(clamp_access_token_ttl(Some(1)), ACCESS_TTL_MIN);
        assert_eq!(clamp_access_token_ttl(Some(999_999)), ACCESS_TTL_MAX);
        assert_eq!(clamp_access_token_ttl(Some(0)), ACCESS_TTL_DEFAULT);

        assert_eq!(clamp_refresh_token_ttl(None), 2_592_000);
        assert_eq!(clamp_refresh_token_ttl(Some(1)), REFRESH_TTL_MIN);
        assert_eq!(clamp_refresh_token_ttl(Some(i32::MAX)), REFRESH_TTL_MAX);
    }

    #[test]
    fn grant_types_are_restricted_to_supported_set() {
        assert_eq!(
            normalize_grant_types(None).expect("缺省应返回两类"),
            vec![
                "authorization_code".to_string(),
                "refresh_token".to_string()
            ]
        );
        let custom = vec![
            "refresh_token".to_string(),
            "refresh_token".to_string(),
            "authorization_code".to_string(),
        ];
        assert_eq!(
            normalize_grant_types(Some(&custom)).expect("应去重保序"),
            vec![
                "refresh_token".to_string(),
                "authorization_code".to_string()
            ]
        );
        assert!(normalize_grant_types(Some(&["password".to_string()])).is_err());
        assert!(normalize_grant_types(Some(&[])).is_err());
    }


    #[test]
    fn redirect_uri_validation() {
        // https 任意主机都允许
        assert!(validate_redirect_uri("https://erp.example.com/oauth/cb").is_ok());
        // http 只允许回环 IP 字面量
        assert!(validate_redirect_uri("http://127.0.0.1:1456/callback").is_ok());
        assert!(validate_redirect_uri("http://[::1]:1456/callback").is_ok());
        assert!(validate_redirect_uri("http://erp.example.com/cb").is_err());
        assert!(validate_redirect_uri("http://localhost:1456/cb").is_err());
        // fragment / userinfo / 其它协议一律拒绝
        assert!(validate_redirect_uri("https://erp.example.com/cb#token").is_err());
        assert!(validate_redirect_uri("https://user@erp.example.com/cb").is_err());
        assert!(validate_redirect_uri("ftp://erp.example.com/cb").is_err());
        assert!(validate_redirect_uri("").is_err());
        // 无路径的 https 地址视为 "/"
        assert!(validate_redirect_uri("https://erp.example.com").is_ok());
    }

    #[test]
    fn redirect_uris_require_at_least_one() {
        assert!(validate_redirect_uris(&[]).is_err());
        assert!(validate_redirect_uris(&["   ".to_string()]).is_err());
        let deduped = validate_redirect_uris(&[
            "https://erp.example.com/cb".to_string(),
            "https://erp.example.com/cb".to_string(),
        ])
        .expect("应去重");
        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn redirect_matching_is_exact_with_loopback_port_exception() {
        let registered = vec!["https://erp.example.com/oauth/cb".to_string()];
        assert!(matches_redirect(
            &registered,
            "https://erp.example.com/oauth/cb"
        ));
        // 端口 / 路径 / 查询串不同都不匹配
        assert!(!matches_redirect(
            &registered,
            "https://erp.example.com:8443/oauth/cb"
        ));
        assert!(!matches_redirect(
            &registered,
            "https://erp.example.com/oauth/other"
        ));
        assert!(!matches_redirect(
            &registered,
            "https://erp.example.com/oauth/cb?x=1"
        ));
        // 协议不同不匹配
        assert!(!matches_redirect(
            &registered,
            "http://erp.example.com/oauth/cb"
        ));

        // loopback：端口由系统随机分配，登记时忽略端口（RFC 8252 §7.3）
        let loopback = vec!["http://127.0.0.1/callback".to_string()];
        assert!(matches_redirect(&loopback, "http://127.0.0.1:1456/callback"));
        assert!(matches_redirect(&loopback, "http://127.0.0.1:8080/callback"));
        assert!(!matches_redirect(&loopback, "http://127.0.0.1:8080/other"));
        // IPv6 回环同样豁免端口
        let loopback_v6 = vec!["http://[::1]/callback".to_string()];
        assert!(matches_redirect(
            &loopback_v6,
            "http://[::1]:1456/callback"
        ));
    }

    #[test]
    fn client_credentials_depend_on_type() {
        let public = generate_client_credentials(CLIENT_TYPE_PUBLIC);
        assert!(public.client_id.starts_with("oc_"));
        assert!(public.secret.is_none(), "public client 不得有密钥");

        let confidential = generate_client_credentials(CLIENT_TYPE_CONFIDENTIAL);
        let secret = confidential.secret.expect("confidential 必须有密钥");
        assert!(secret.plaintext.starts_with("ocs_"));
        assert_eq!(secret.hash.len(), 64);
    }

    #[test]
    fn grant_type_check_follows_client_config() {
        let mut client = client(&["asset:hardware:list"]);
        assert!(client_allows_grant(&client, "authorization_code"));
        client.grant_types = vec!["refresh_token".to_string()];
        assert!(!client_allows_grant(&client, "authorization_code"));
        assert!(client_allows_grant(&client, "refresh_token"));
    }
}

