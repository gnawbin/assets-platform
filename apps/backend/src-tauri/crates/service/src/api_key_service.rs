//! API Key 管理（外部系统接入凭证）
//!
//! 职责边界（设计文档 §16 T-1）：
//! - 本模块只做**数据访问**：表行读写、哈希等值查表、权限点查询；
//! - 「能不能调这个接口」「scope 是否超出创建者权限」等**判定**一律放在
//!   `api` crate（`authz.rs` / `api_key_routes.rs`），
//!   因此 Tauri 桌面通道（直接调 service）不会带上任何 HTTP 鉴权语义。
//!
//! 安全约定（设计文档 §5.2 / §6.8）：
//! - 只存 `SHA-256(完整 key)` 的 hex，支持等值查找且无时序侧信道；
//! - 明文只在创建 / 轮换的返回值里出现一次；
//! - 校验结果进 `DashMap` 短缓存（30s），吊销 / 轮换立即清除；
//! - `last_used_at` 写入节流 ≥ 60s，避免每请求一条 UPDATE。

use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::Serialize;
use tracing::{info, warn};

use database::models::SysApiKey;
use utils::api_key::{generate_api_key, hash_api_key, ApiKeyEnv};
use utils::snowflake::next_id;

/// 校验结果缓存 TTL（设计文档 §6.8）
const VERIFY_CACHE_TTL: Duration = Duration::from_secs(30);

/// `last_used_at` 写入节流窗口（设计文档 §5.2）
const LAST_USED_THROTTLE: Duration = Duration::from_secs(60);

/// scopes 数量上限（设计文档 §6.6）
pub const MAX_SCOPES: usize = 50;

/// 默认有效期（天）
pub const DEFAULT_EXPIRES_DAYS: i64 = 90;

/// 最长有效期（天）
pub const MAX_EXPIRES_DAYS: i64 = 365;

/// 默认每分钟请求上限
pub const DEFAULT_RATE_LIMIT_PER_MIN: i32 = 600;

/// 校验命中缓存：`key_hash` → (写入时刻, 认证信息)
fn verify_cache() -> &'static DashMap<String, (Instant, ApiKeyAuth)> {
    static CACHE: OnceLock<DashMap<String, (Instant, ApiKeyAuth)>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

/// `last_used_at` 写入节流：`api_key_id` → 上次写入时刻
fn last_used_throttle() -> &'static DashMap<i64, Instant> {
    static THROTTLE: OnceLock<DashMap<i64, Instant>> = OnceLock::new();
    THROTTLE.get_or_init(DashMap::new)
}

/// 认证中间件需要的 API Key 信息（不含哈希）
#[derive(Debug, Clone)]
pub struct ApiKeyAuth {
    /// 主键（雪花 ID）
    pub id: i64,
    /// 绑定租户（空表示 public）
    pub tenant_id: Option<i64>,
    /// 授权范围
    pub scopes: Vec<String>,
    /// 每分钟请求上限
    pub rate_limit_per_min: i32,
    /// 定位段（日志 / 审计用，不是密钥）
    pub key_prefix: String,
    /// 过期时间（仅用于管理页告警）
    pub expires_at: Option<DateTime<Utc>>,
}

/// 管理页列表项（只含非敏感字段）
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ApiKeyView {
    /// 主键（字符串，避免 JS 精度丢失）
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// 定位段
    pub key_prefix: String,
    pub scopes: Vec<String>,
    pub rate_limit_per_min: i32,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub enable: bool,
    pub created_at: Option<DateTime<Utc>>,
}

/// 创建 / 轮换结果（`api_key` 明文只出现这一次）
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct CreatedApiKey {
    pub id: String,
    /// 完整明文密钥：`ak_<env>_<prefix16>_<secret43>`
    pub api_key: String,
    pub key_prefix: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// 创建参数
pub struct NewApiKey<'a> {
    pub tenant_id: Option<i64>,
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub scopes: &'a [String],
    pub rate_limit_per_min: i32,
    pub expires_in_days: i64,
    pub created_by: Option<i64>,
}

/// 用户的「授权快照」：是否超管 + 权限点并集
///
/// 由 `api` crate 侧做 60s 缓存与判定（设计文档 §6.8）。
#[derive(Debug, Clone, Default)]
pub struct UserAuthority {
    pub is_super_admin: bool,
    pub scopes: Vec<String>,
}

impl From<SysApiKey> for ApiKeyView {
    fn from(row: SysApiKey) -> Self {
        Self {
            id: row.id.to_string(),
            name: row.name,
            description: row.description,
            key_prefix: row.key_prefix,
            scopes: row.scopes,
            rate_limit_per_min: row.rate_limit_per_min,
            expires_at: row.expires_at,
            last_used_at: row.last_used_at,
            revoked_at: row.revoked_at,
            enable: row.enable,
            created_at: row.created_at,
        }
    }
}

impl From<&SysApiKey> for ApiKeyAuth {
    fn from(row: &SysApiKey) -> Self {
        Self {
            id: row.id,
            tenant_id: row.tenant_id,
            scopes: row.scopes.clone(),
            rate_limit_per_min: row.rate_limit_per_min,
            key_prefix: row.key_prefix.clone(),
            expires_at: row.expires_at,
        }
    }
}

/// API Key 环境标签：`API_KEY_ENV=test` 时生成 `ak_test_*`，缺省 `ak_live_*`
///
/// 仅影响展示与分类，不参与校验；新变量缺省即安全（T-7）。
fn api_key_env() -> ApiKeyEnv {
    match std::env::var("API_KEY_ENV") {
        Ok(value) => ApiKeyEnv::parse(value.trim()).unwrap_or(ApiKeyEnv::Live),
        Err(_) => ApiKeyEnv::Live,
    }
}

/// 规范化并校验 scopes（去重 + 格式 + 数量）
///
/// 只做**数据合法性**校验；「是否超出创建者权限」「是否含高危管理点」
/// 属于授权判定，由 `api::api_key_routes` 负责（T-1）。
pub fn normalize_scopes(scopes: &[String]) -> Result<Vec<String>, String> {
    let mut normalized: Vec<String> = Vec::with_capacity(scopes.len());
    for scope in scopes {
        let scope = scope.trim();
        if scope.is_empty() {
            return Err("scope 不能为空字符串".to_string());
        }
        // 权限点字符集：小写字母、数字、冒号、通配 *（`asset:hardware:list` / `asset:*`）
        let valid = scope
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b':' || b == b'*');
        if !valid {
            return Err(format!(
                "scope 格式非法：{}（只允许小写字母、数字、冒号和通配符 *）",
                scope
            ));
        }
        if !scope.contains(':') {
            return Err(format!(
                "scope 格式非法：{}（至少形如 模块:资源:动作）",
                scope
            ));
        }
        if !normalized.iter().any(|existing| existing == scope) {
            normalized.push(scope.to_string());
        }
    }

    if normalized.is_empty() {
        return Err("scopes 不能为空（至少 1 个权限点）".to_string());
    }
    if normalized.len() > MAX_SCOPES {
        return Err(format!("scopes 数量不能超过 {} 个", MAX_SCOPES));
    }
    Ok(normalized)
}

/// 把有效期天数夹到 `[1, MAX_EXPIRES_DAYS]`
pub fn clamp_expires_in_days(days: Option<i64>) -> i64 {
    match days {
        Some(d) if d > 0 => d.min(MAX_EXPIRES_DAYS),
        Some(_) => 1,
        None => DEFAULT_EXPIRES_DAYS,
    }
}

/// 把速率上限夹到合理区间（1..=100_000，缺省 600）
pub fn clamp_rate_limit(rate_limit_per_min: Option<i32>) -> i32 {
    match rate_limit_per_min {
        Some(n) if n > 0 => n.min(100_000),
        _ => DEFAULT_RATE_LIMIT_PER_MIN,
    }
}

/// 创建 API Key（返回一次性明文）
pub async fn create_api_key(params: &NewApiKey<'_>) -> Result<CreatedApiKey, String> {
    let scopes = normalize_scopes(params.scopes)?;
    let pool = database::get_write_pool().map_err(|e| e.to_string())?;

    let name = params.name.trim();
    if name.is_empty() {
        return Err("名称不能为空".to_string());
    }

    let generated = generate_api_key(api_key_env());
    let id = next_id() as i64;
    let expires_in_days = clamp_expires_in_days(Some(params.expires_in_days));
    let rate_limit_per_min = clamp_rate_limit(Some(params.rate_limit_per_min));

    let expires_at: Option<DateTime<Utc>> = sqlx::query_scalar(
        r#"
        INSERT INTO public.sys_api_key
            (id, tenant_id, name, description, key_prefix, key_hash, scopes,
             rate_limit_per_min, expires_at, created_by, created_at, updated_by, updated_at,
             enable, deleted)
        VALUES
            ($1, $2, $3, $4, $5, $6, $7, $8,
             now() + make_interval(days => $9::int), $10, now(), $10, now(), TRUE, 0)
        RETURNING expires_at
        "#,
    )
    .bind(id)
    .bind(params.tenant_id)
    .bind(name)
    .bind(params.description)
    .bind(&generated.key_prefix)
    .bind(&generated.key_hash)
    .bind(&scopes)
    .bind(rate_limit_per_min)
    .bind(expires_in_days as i32)
    .bind(params.created_by)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        warn!("创建 API Key 失败: name={}, error={}", name, e);
        format!("创建 API Key 失败: {}", e)
    })?;

    info!(
        "API Key 已创建: id={}, prefix={}, scopes={:?}",
        id, generated.key_prefix, scopes
    );

    Ok(CreatedApiKey {
        id: id.to_string(),
        api_key: generated.plaintext,
        key_prefix: generated.key_prefix,
        scopes,
        expires_at,
    })
}

/// 列出 API Key
///
/// `tenant_filter = Some(t)` 时只返回该租户的 key；`None` 表示跨租户（超管）。
/// 只返回未软删除的行，包含已吊销（`revoked_at` 非空）用于审计展示。
pub async fn list_api_keys(tenant_filter: Option<i64>) -> Result<Vec<ApiKeyView>, String> {
    let pool = database::get_read_pool().map_err(|e| e.to_string())?;

    let rows: Vec<SysApiKey> = match tenant_filter {
        Some(tenant_id) => {
            sqlx::query_as(
                r#"
                SELECT id, tenant_id, name, description, key_prefix, key_hash, scopes,
                       rate_limit_per_min, expires_at, last_used_at, revoked_at,
                       created_by, created_at, updated_by, updated_at, enable, deleted
                FROM public.sys_api_key
                WHERE (deleted IS NULL OR deleted = 0)
                  AND tenant_id = $1
                ORDER BY created_at DESC NULLS LAST
                "#,
            )
            .bind(tenant_id)
            .fetch_all(&pool)
            .await
        }
        None => {
            sqlx::query_as(
                r#"
                SELECT id, tenant_id, name, description, key_prefix, key_hash, scopes,
                       rate_limit_per_min, expires_at, last_used_at, revoked_at,
                       created_by, created_at, updated_by, updated_at, enable, deleted
                FROM public.sys_api_key
                WHERE (deleted IS NULL OR deleted = 0)
                ORDER BY created_at DESC NULLS LAST
                "#,
            )
            .fetch_all(&pool)
            .await
        }
    }
    .map_err(|e| format!("查询 API Key 列表失败: {}", e))?;

    Ok(rows.into_iter().map(ApiKeyView::from).collect())
}



/// 轮换 API Key
///
/// 语义（设计文档 §5.2）：新 key 立即生效，旧 key 的 `expires_at` 提前到
/// `min(原过期时间, now() + 24h)`，留出切换窗口后自然失效（行保留用于审计）。
pub async fn rotate_api_key(
    id: i64,
    expires_in_days: Option<i64>,
    updated_by: Option<i64>,
) -> Result<CreatedApiKey, String> {
    let pool = database::get_write_pool().map_err(|e| e.to_string())?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("开启事务失败: {}", e))?;

    // 行锁：避免并发轮换产生两条同时有效的新 key
    let old: Option<SysApiKey> = sqlx::query_as(
        r#"
        SELECT id, tenant_id, name, description, key_prefix, key_hash, scopes,
               rate_limit_per_min, expires_at, last_used_at, revoked_at,
               created_by, created_at, updated_by, updated_at, enable, deleted
        FROM public.sys_api_key
        WHERE id = $1 AND (deleted IS NULL OR deleted = 0)
        FOR UPDATE
        "#,
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| format!("查询 API Key 失败: {}", e))?;

    let Some(old) = old else {
        return Err("API Key 不存在".to_string());
    };
    if old.revoked_at.is_some() || !old.enable {
        return Err("API Key 已吊销，请重新创建".to_string());
    }

    let generated = generate_api_key(api_key_env());
    let new_id = next_id() as i64;
    let days = clamp_expires_in_days(expires_in_days);

    let expires_at: Option<DateTime<Utc>> = sqlx::query_scalar(
        r#"
        INSERT INTO public.sys_api_key
            (id, tenant_id, name, description, key_prefix, key_hash, scopes,
             rate_limit_per_min, expires_at, created_by, created_at, updated_by, updated_at,
             enable, deleted)
        VALUES
            ($1, $2, $3, $4, $5, $6, $7, $8,
             now() + make_interval(days => $9::int), $10, now(), $10, now(), TRUE, 0)
        RETURNING expires_at
        "#,
    )
    .bind(new_id)
    .bind(old.tenant_id)
    .bind(&old.name)
    .bind(&old.description)
    .bind(&generated.key_prefix)
    .bind(&generated.key_hash)
    .bind(&old.scopes)
    .bind(old.rate_limit_per_min)
    .bind(days as i32)
    .bind(updated_by.or(old.created_by))
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| format!("轮换 API Key 失败: {}", e))?;

    // 旧 key 提前失效：最迟 24 小时后过期（原过期时间更早则保持不变）
    sqlx::query(
        r#"
        UPDATE public.sys_api_key
        SET expires_at = LEAST(
                COALESCE(expires_at, now() + interval '24 hours'),
                now() + interval '24 hours'
            ),
            updated_by = $2,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(updated_by)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("旧 API Key 提前失效失败: {}", e))?;

    tx.commit()
        .await
        .map_err(|e| format!("提交事务失败: {}", e))?;

    invalidate_api_key(id);
    info!(
        "API Key 已轮换: old_id={}, new_id={}, prefix={}",
        id, new_id, generated.key_prefix
    );

    Ok(CreatedApiKey {
        id: new_id.to_string(),
        api_key: generated.plaintext,
        key_prefix: generated.key_prefix,
        scopes: old.scopes,
        expires_at,
    })
}

/// 吊销 API Key（写 `revoked_at`，行保留用于审计）
///
/// 立即清掉校验缓存，保证 1s 内生效（设计文档 §6.8 的撤销延迟承诺）。
pub async fn revoke_api_key(id: i64, updated_by: Option<i64>) -> Result<(), String> {
    let pool = database::get_write_pool().map_err(|e| e.to_string())?;

    let result = sqlx::query(
        r#"
        UPDATE public.sys_api_key
        SET revoked_at = now(), enable = FALSE, updated_by = $2, updated_at = now()
        WHERE id = $1 AND (deleted IS NULL OR deleted = 0) AND revoked_at IS NULL
        "#,
    )
    .bind(id)
    .bind(updated_by)
    .execute(&pool)
    .await
    .map_err(|e| format!("吊销 API Key 失败: {}", e))?;

    if result.rows_affected() == 0 {
        return Err("API Key 不存在或已吊销".to_string());
    }

    invalidate_api_key(id);
    info!("API Key 已吊销: id={}", id);
    Ok(())
}

/// 校验 API Key（中间件调用）
///
/// - 命中缓存直接返回（30s TTL）
/// - 未命中回源 DB；**回源失败返回 `Err`，不得降级为放行**（设计文档 §6.8）
pub async fn verify_api_key(raw_key: &str) -> Result<Option<ApiKeyAuth>, String> {
    let key_hash = hash_api_key(raw_key);

    let fresh = verify_cache()
        .get(&key_hash)
        .filter(|entry| entry.0.elapsed() < VERIFY_CACHE_TTL)
        .map(|entry| entry.1.clone());
    if let Some(auth) = fresh {
        return Ok(Some(auth));
    }
    verify_cache().remove(&key_hash);

    let pool = database::get_read_pool().map_err(|e| e.to_string())?;
    let row: Option<SysApiKey> = sqlx::query_as(
        r#"
        SELECT id, tenant_id, name, description, key_prefix, key_hash, scopes,
               rate_limit_per_min, expires_at, last_used_at, revoked_at,
               created_by, created_at, updated_by, updated_at, enable, deleted
        FROM public.sys_api_key
        WHERE key_hash = $1
          AND enable = TRUE
          AND revoked_at IS NULL
          AND (expires_at IS NULL OR expires_at > now())
          AND (deleted IS NULL OR deleted = 0)
        "#,
    )
    .bind(&key_hash)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("校验 API Key 失败: {}", e))?;

    let Some(row) = row else {
        return Ok(None);
    };

    let auth = ApiKeyAuth::from(&row);
    verify_cache().insert(key_hash, (Instant::now(), auth.clone()));
    touch_last_used(row.id);
    Ok(Some(auth))
}

/// 更新 `last_used_at`（节流 ≥ 60s，异步写入，不阻塞请求）
fn touch_last_used(id: i64) {
    let throttled = last_used_throttle()
        .get(&id)
        .map(|entry| entry.elapsed() < LAST_USED_THROTTLE)
        .unwrap_or(false);
    if throttled {
        return;
    }
    last_used_throttle().insert(id, Instant::now());

    let Ok(pool) = database::get_write_pool() else {
        return;
    };
    tokio::spawn(async move {
        if let Err(e) =
            sqlx::query("UPDATE public.sys_api_key SET last_used_at = now() WHERE id = $1")
                .bind(id)
                .execute(&pool)
                .await
        {
            warn!("更新 API Key last_used_at 失败: id={}, error={}", id, e);
        }
    });
}

/// 清除单个 API Key 的校验缓存（吊销 / 轮换后立即调用）
pub fn invalidate_api_key(id: i64) {
    clear_local_api_key(id);
    if let Some(hook) = api_key_invalidator() {
        hook(id);
    }
}

/// 只清本实例的校验缓存（供 NOTIFY listener 调用，避免回环广播）
pub fn clear_local_api_key(id: i64) {
    verify_cache().retain(|_, (_, auth)| auth.id != id);
    last_used_throttle().remove(&id);
}

/// API Key 失效广播的注入点（设计文档附录 D.3，修 B8：30s 跨实例窗口）
///
/// 与 `api::auth::install_auth_invalidator` 同构：`service` 不感知传输方式，
/// 由进程入口注入「发 NOTIFY」的实现；桌面形态不注入，行为与改造前一致。
static API_KEY_INVALIDATOR: OnceLock<std::sync::Arc<dyn Fn(i64) + Send + Sync>> = OnceLock::new();

/// 安装失效广播器（幂等，仅启动期调用一次）
pub fn install_api_key_invalidator(f: std::sync::Arc<dyn Fn(i64) + Send + Sync>) {
    let _ = API_KEY_INVALIDATOR.set(f);
}

/// 当前失效广播器（未安装时返回 `None`）
fn api_key_invalidator() -> Option<&'static std::sync::Arc<dyn Fn(i64) + Send + Sync>> {
    API_KEY_INVALIDATOR.get()
}

/// 清空全部校验缓存（权限配置变更后由管理端调用）
pub fn clear_verify_cache() {
    verify_cache().clear();
}

/// 读取用户的授权快照（是否超管 + 权限点并集）
///
/// 只做数据查询，判定（子集校验、路由放行）在 `api` crate 完成。
pub async fn load_user_authority(user_id: i64) -> Result<UserAuthority, String> {
    let pool = database::get_read_pool().map_err(|e| e.to_string())?;

    let user_super_admin: Option<bool> = sqlx::query_scalar(
        "SELECT is_super_admin FROM public.sys_user
         WHERE id = $1 AND (deleted IS NULL OR deleted = 0)",
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("查询用户超管标记失败: {}", e))?;

    let Some(user_super_admin) = user_super_admin else {
        // 用户不存在 / 已删除：返回空授权（fail-closed）
        return Ok(UserAuthority::default());
    };

    let role_super_admin: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM public.sys_user_role ur
            JOIN public.sys_role r
              ON r.id = ur.role_id AND (r.deleted IS NULL OR r.deleted = 0)
            WHERE ur.user_id = $1
              AND (ur.deleted IS NULL OR ur.deleted = 0)
              AND r.is_super_admin
        )
        "#,
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap_or(false);

    let scopes: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT DISTINCT m.perms
        FROM public.sys_user_role ur
        JOIN public.sys_role r
          ON r.id = ur.role_id AND (r.deleted IS NULL OR r.deleted = 0)
        JOIN public.sys_role_menu rm
          ON rm.role_id = ur.role_id AND (rm.deleted IS NULL OR rm.deleted = 0)
        JOIN public.sys_menu m
          ON m.id = rm.menu_id AND (m.deleted IS NULL OR m.deleted = 0)
        WHERE ur.user_id = $1
          AND (ur.deleted IS NULL OR ur.deleted = 0)
          AND m.perms IS NOT NULL
        "#,
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("查询用户权限点失败: {}", e))?;

    Ok(UserAuthority {
        is_super_admin: user_super_admin || role_super_admin,
        scopes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_scopes_dedupes_and_keeps_order() {
        let input = vec![
            "asset:hardware:list".to_string(),
            "asset:hardware:list".to_string(),
            "asset:*".to_string(),
        ];
        let scopes = normalize_scopes(&input).expect("合法 scopes 应通过");
        assert_eq!(scopes, vec!["asset:hardware:list", "asset:*"]);
    }

    #[test]
    fn normalize_scopes_requires_at_least_one() {
        assert!(normalize_scopes(&[]).is_err());
        assert!(normalize_scopes(&["".to_string()]).is_err());
    }

    #[test]
    fn normalize_scopes_rejects_bad_format() {
        // 大写 / 空格 / 中文 / 缺冒号 一律拒绝
        assert!(normalize_scopes(&["Asset:hardware:list".to_string()]).is_err());
        assert!(normalize_scopes(&["asset hardware list".to_string()]).is_err());
        assert!(normalize_scopes(&["资产:list".to_string()]).is_err());
        assert!(normalize_scopes(&["list".to_string()]).is_err());
    }

    #[test]
    fn normalize_scopes_enforces_upper_bound() {
        let many: Vec<String> = (0..=MAX_SCOPES)
            .map(|index| format!("asset:resource{}:list", index))
            .collect();
        assert!(normalize_scopes(&many).is_err());
    }

    #[test]
    fn expires_and_rate_limit_are_clamped() {
        assert_eq!(clamp_expires_in_days(None), DEFAULT_EXPIRES_DAYS);
        assert_eq!(clamp_expires_in_days(Some(0)), 1);
        assert_eq!(clamp_expires_in_days(Some(-5)), 1);
        assert_eq!(clamp_expires_in_days(Some(10_000)), MAX_EXPIRES_DAYS);

        assert_eq!(
            clamp_rate_limit(None),
            DEFAULT_RATE_LIMIT_PER_MIN,
            "缺省 600 次/分钟"
        );
        assert_eq!(clamp_rate_limit(Some(0)), DEFAULT_RATE_LIMIT_PER_MIN);
        assert_eq!(clamp_rate_limit(Some(120)), 120);
        assert_eq!(clamp_rate_limit(Some(1_000_000)), 100_000);
    }
}

