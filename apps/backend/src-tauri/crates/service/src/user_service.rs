use chrono::{DateTime, Duration, Utc};
use database::models::{SysTenant, SysUser, TenantInfo};
use database::{get_read_pool, get_write_pool};
use utils::password_secret::{hash_password, verify_password};
use utils::snowflake::next_id;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{error, info, warn};

/// JWT 声明
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// 用户ID
    pub sub: i64,
    /// 用户名
    pub username: String,
    /// 过期时间（时间戳）
    pub exp: usize,
    /// 签发时间（时间戳）
    pub iat: usize,
    /// 令牌类型：access / refresh（历史令牌没有该字段，按 access 处理）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typ: Option<String>,
    /// 刷新令牌白名单主键（雪花 ID 字符串），仅 refresh 令牌携带
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,
}

/// 登录响应（包含 JWT Token）
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    #[serde(serialize_with = "database::models::i64_to_string")]
    pub id: i64,
    pub username: String,
    pub real_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    #[serde(serialize_with = "database::models::opt_i64_to_string")]
    pub department_id: Option<i64>,
    pub is_super_admin: bool,
    pub status: i16,
    pub nickname: Option<String>,
    pub avatar: Option<String>,
    #[serde(serialize_with = "database::models::opt_i64_to_string")]
    pub tenant_id: Option<i64>,
    /// JWT Token，用于后续请求的身份验证
    pub token: String,
    /// 刷新令牌（有效期默认 30 天，每次刷新都会轮换）
    pub refresh_token: String,
    /// 访问令牌有效期（秒）
    pub expires_in: i64,
    /// 用户可访问的租户列表
    pub available_tenants: Vec<TenantInfo>,
}

// ======================== 令牌签发 / 刷新 / 撤销 ========================

/// 访问令牌默认有效期：7 天（与历史行为一致，可用 `JWT_ACCESS_TTL_SECS` 覆盖）
const DEFAULT_ACCESS_TTL_SECS: i64 = 86_400 * 7;
/// 刷新令牌默认有效期：30 天（可用 `JWT_REFRESH_TTL_SECS` 覆盖）
const DEFAULT_REFRESH_TTL_SECS: i64 = 86_400 * 30;

/// 访问令牌 / 刷新令牌对
#[derive(Debug, Clone)]
pub struct TokenPair {
    /// 访问令牌（放入 `Authorization: Bearer <token>`）
    pub token: String,
    /// 刷新令牌（仅用于 `POST /api/auth/refresh`）
    pub refresh_token: String,
    /// 访问令牌有效期（秒）
    pub expires_in: i64,
}

/// JWT 签名密钥（与 api 层校验共用 `JWT_SECRET`）
fn jwt_secret() -> String {
    std::env::var("JWT_SECRET").unwrap_or_else(|_| "assets-platform-default-secret-key".to_string())
}

/// 读取有效期配置（非法值 / 非正数回落默认值）
fn env_ttl_secs(key: &str, default: i64) -> i64 {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(default)
}

/// 当前时间戳（秒）
fn now_secs() -> Result<usize, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as usize)
        .map_err(|e| format!("获取时间戳失败: {}", e))
}

/// 签发 JWT（HS256）
fn sign_claims(claims: &Claims) -> Result<String, String> {
    encode(
        &Header::default(),
        claims,
        &EncodingKey::from_secret(jwt_secret().as_bytes()),
    )
    .map_err(|e| format!("生成令牌失败: {}", e))
}

/// 生成 access / refresh 令牌对，并把 refresh 的 jti 写入白名单
async fn issue_token_pair(
    pool: &PgPool,
    user_id: i64,
    username: &str,
    family_id: i64,
) -> Result<TokenPair, String> {
    let access_ttl = env_ttl_secs("JWT_ACCESS_TTL_SECS", DEFAULT_ACCESS_TTL_SECS);
    let refresh_ttl = env_ttl_secs("JWT_REFRESH_TTL_SECS", DEFAULT_REFRESH_TTL_SECS);
    let now = now_secs()?;

    let access_claims = Claims {
        sub: user_id,
        username: username.to_string(),
        exp: now + access_ttl as usize,
        iat: now,
        typ: Some("access".to_string()),
        jti: None,
    };
    let token = sign_claims(&access_claims)?;

    let jti = next_id() as i64;
    let refresh_claims = Claims {
        sub: user_id,
        username: username.to_string(),
        exp: now + refresh_ttl as usize,
        iat: now,
        typ: Some("refresh".to_string()),
        jti: Some(jti.to_string()),
    };
    let refresh_token = sign_claims(&refresh_claims)?;

    sqlx::query(
        "INSERT INTO public.sys_user_refresh_token (jti, user_id, family_id, expires_at) VALUES ($1, $2, $3, $4)",
    )
    .bind(jti)
    .bind(user_id)
    .bind(family_id)
    .bind(Utc::now() + Duration::seconds(refresh_ttl))
    .execute(pool)
    .await
    .map_err(|e| {
        error!("保存刷新令牌失败: user_id={}, error={}", user_id, e);
        format!("保存刷新令牌失败: {}", e)
    })?;

    Ok(TokenPair {
        token,
        refresh_token,
        expires_in: access_ttl,
    })
}

/// 刷新令牌：校验 → 轮换 → 返回新的令牌对
///
/// 安全要点：
/// - 只接受 `typ = "refresh"` 的令牌（access token 不能用来续期）；
/// - 每次刷新都轮换（旧 jti 置 `revoked_at`，新 jti 写入白名单，同 `family_id`）；
/// - 已撤销的 jti 再次出现视为**令牌重放**，直接撤销整个轮换族；
/// - 用户被禁用 / 删除后不再续期。
pub async fn refresh_tokens(refresh_token: &str) -> Result<TokenPair, String> {
    let claims = decode::<Claims>(
        refresh_token,
        &DecodingKey::from_secret(jwt_secret().as_bytes()),
        &Validation::default(),
    )
    .map_err(|e| {
        warn!("刷新令牌解析失败: {}", e);
        "刷新令牌无效，请重新登录".to_string()
    })?
    .claims;

    if claims.typ.as_deref() != Some("refresh") {
        warn!("拒绝使用非 refresh 令牌续期: sub={}", claims.sub);
        return Err("请使用刷新令牌（refresh token）".to_string());
    }

    let jti: i64 = claims
        .jti
        .as_deref()
        .ok_or_else(|| "刷新令牌缺少 jti".to_string())?
        .parse()
        .map_err(|_| "刷新令牌 jti 格式非法".to_string())?;

    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("开启事务失败: {}", e))?;

    // 1. 查白名单（行锁，避免同一令牌被并发刷新两次）
    let record = sqlx::query_as::<_, (i64, i64, Option<DateTime<Utc>>, DateTime<Utc>)>(
        "SELECT user_id, family_id, revoked_at, expires_at
           FROM public.sys_user_refresh_token
          WHERE jti = $1
            FOR UPDATE",
    )
    .bind(jti)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| format!("查询刷新令牌失败: {}", e))?;

    let Some((user_id, family_id, revoked_at, expires_at)) = record else {
        warn!("刷新令牌不在白名单中: jti={}", jti);
        return Err("刷新令牌无效，请重新登录".to_string());
    };

    if user_id != claims.sub {
        warn!(
            "刷新令牌与用户不匹配: jti={}, token_user={}, claim_user={}",
            jti, user_id, claims.sub
        );
        return Err("刷新令牌无效，请重新登录".to_string());
    }

    // 2. 重放检测：已撤销的令牌再次出现 → 整族撤销
    if revoked_at.is_some() {
        sqlx::query(
            "UPDATE public.sys_user_refresh_token
                SET revoked_at = CURRENT_TIMESTAMP
              WHERE family_id = $1 AND revoked_at IS NULL",
        )
        .bind(family_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("撤销刷新令牌族失败: {}", e))?;
        tx.commit()
            .await
            .map_err(|e| format!("提交事务失败: {}", e))?;

        warn!(
            "检测到刷新令牌重放，已撤销整个轮换族: jti={}, user_id={}, family_id={}",
            jti, user_id, family_id
        );
        return Err("刷新令牌已失效，请重新登录".to_string());
    }

    // 3. 过期检查
    if expires_at <= Utc::now() {
        sqlx::query(
            "UPDATE public.sys_user_refresh_token SET revoked_at = CURRENT_TIMESTAMP WHERE jti = $1",
        )
        .bind(jti)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("撤销过期刷新令牌失败: {}", e))?;
        tx.commit()
            .await
            .map_err(|e| format!("提交事务失败: {}", e))?;

        warn!("刷新令牌已过期: jti={}, user_id={}", jti, user_id);
        return Err("刷新令牌已过期，请重新登录".to_string());
    }

    // 4. 用户状态：禁用 / 删除后不再续期
    let user = sqlx::query_as::<_, (String, i16, Option<i16>)>(
        "SELECT username, status, deleted FROM public.sys_user WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| format!("查询用户失败: {}", e))?;

    let Some((username, status, deleted)) = user else {
        warn!("刷新令牌对应的用户不存在: user_id={}", user_id);
        return Err("用户不存在，请重新登录".to_string());
    };
    if status != 1 || deleted.unwrap_or(0) != 0 {
        sqlx::query(
            "UPDATE public.sys_user_refresh_token
                SET revoked_at = CURRENT_TIMESTAMP
              WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("撤销刷新令牌失败: {}", e))?;
        tx.commit()
            .await
            .map_err(|e| format!("提交事务失败: {}", e))?;

        warn!("账号已禁用，拒绝续期: user_id={}", user_id);
        return Err("账号已被禁用，请联系管理员".to_string());
    }

    // 5. 轮换：旧 jti 撤销 + 新 jti 落库（同族）
    let access_ttl = env_ttl_secs("JWT_ACCESS_TTL_SECS", DEFAULT_ACCESS_TTL_SECS);
    let refresh_ttl = env_ttl_secs("JWT_REFRESH_TTL_SECS", DEFAULT_REFRESH_TTL_SECS);
    let now = now_secs()?;

    let access_claims = Claims {
        sub: user_id,
        username: username.clone(),
        exp: now + access_ttl as usize,
        iat: now,
        typ: Some("access".to_string()),
        jti: None,
    };
    let token = sign_claims(&access_claims)?;

    let new_jti = next_id() as i64;
    let refresh_claims = Claims {
        sub: user_id,
        username: username.clone(),
        exp: now + refresh_ttl as usize,
        iat: now,
        typ: Some("refresh".to_string()),
        jti: Some(new_jti.to_string()),
    };
    let new_refresh_token = sign_claims(&refresh_claims)?;

    sqlx::query(
        "UPDATE public.sys_user_refresh_token
            SET revoked_at = CURRENT_TIMESTAMP, replaced_by = $2
          WHERE jti = $1",
    )
    .bind(jti)
    .bind(new_jti)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("撤销旧刷新令牌失败: {}", e))?;

    sqlx::query(
        "INSERT INTO public.sys_user_refresh_token (jti, user_id, family_id, expires_at) VALUES ($1, $2, $3, $4)",
    )
    .bind(new_jti)
    .bind(user_id)
    .bind(family_id)
    .bind(Utc::now() + Duration::seconds(refresh_ttl))
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("保存新刷新令牌失败: {}", e))?;

    // 6. 顺手清理 7 天前已过期的历史记录
    sqlx::query(
        "DELETE FROM public.sys_user_refresh_token WHERE expires_at < CURRENT_TIMESTAMP - INTERVAL '7 days'",
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("清理过期刷新令牌失败: {}", e))?;

    tx.commit()
        .await
        .map_err(|e| format!("提交事务失败: {}", e))?;

    info!(
        "刷新令牌轮换成功: user_id={}, old_jti={}, new_jti={}",
        user_id, jti, new_jti
    );

    Ok(TokenPair {
        token,
        refresh_token: new_refresh_token,
        expires_in: access_ttl,
    })
}

/// 撤销单个刷新令牌（登出；幂等：令牌非法也视为成功）
pub async fn revoke_refresh_token(refresh_token: &str) -> Result<(), String> {
    let Ok(decoded) = decode::<Claims>(
        refresh_token,
        &DecodingKey::from_secret(jwt_secret().as_bytes()),
        &Validation::default(),
    ) else {
        warn!("登出时刷新令牌解析失败，忽略");
        return Ok(());
    };

    let Some(jti) = decoded
        .claims
        .jti
        .as_deref()
        .and_then(|raw| raw.parse::<i64>().ok())
    else {
        return Ok(());
    };

    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    sqlx::query(
        "UPDATE public.sys_user_refresh_token
            SET revoked_at = CURRENT_TIMESTAMP
          WHERE jti = $1 AND revoked_at IS NULL",
    )
    .bind(jti)
    .execute(&pool)
    .await
    .map_err(|e| {
        error!("撤销刷新令牌失败: jti={}, error={}", jti, e);
        format!("撤销刷新令牌失败: {}", e)
    })?;

    info!("已撤销刷新令牌: jti={}", jti);
    Ok(())
}

/// 撤销某用户全部未失效的刷新令牌（改密 / 禁用 / 删除时调用）
pub async fn revoke_all_user_refresh_tokens(user_id: i64) -> Result<u64, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let result = sqlx::query(
        "UPDATE public.sys_user_refresh_token
            SET revoked_at = CURRENT_TIMESTAMP
          WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .map_err(|e| {
        error!("吊销用户刷新令牌失败: user_id={}, error={}", user_id, e);
        format!("吊销用户刷新令牌失败: {}", e)
    })?;

    if result.rows_affected() > 0 {
        info!(
            "已吊销用户全部刷新令牌: user_id={}, count={}",
            user_id,
            result.rows_affected()
        );
    }
    Ok(result.rows_affected())
}

/// 用户列表响应（不包含密码）
/// 用户列表 / 当前用户响应（HTTP 文档 schema 由 crates/apidoc 汇总）
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UserResponse {
    #[serde(serialize_with = "database::models::i64_to_string")]
    pub id: i64,
    pub username: String,
    pub real_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    #[serde(serialize_with = "database::models::opt_i64_to_string")]
    pub department_id: Option<i64>,
    pub is_super_admin: bool,
    pub status: i16,
    pub nickname: Option<String>,
    pub avatar: Option<String>,
    pub person_id: Option<String>,
    pub person_code: Option<String>,
    #[serde(serialize_with = "database::models::opt_i64_to_string")]
    pub super_user_id: Option<i64>,
    #[serde(serialize_with = "database::models::opt_i64_to_string")]
    pub tenant_id: Option<i64>,
    /// 机构名称
    pub tenant_name: Option<String>,
    #[serde(serialize_with = "database::models::opt_i64_to_string")]
    pub created_by: Option<i64>,
    pub created_at: Option<String>,
    #[serde(serialize_with = "database::models::opt_i64_to_string")]
    pub updated_by: Option<i64>,
    pub updated_at: Option<String>,
}

impl From<SysUser> for UserResponse {
    fn from(u: SysUser) -> Self {
        Self {
            id: u.id,
            username: u.username,
            real_name: u.real_name,
            email: u.email,
            phone: u.phone,
            department_id: u.department_id,
            is_super_admin: u.is_super_admin,
            status: u.status,
            nickname: u.nickname,
            avatar: u.avatar,
            person_id: u.person_id,
            person_code: u.person_code,
            super_user_id: u.super_user_id,
            tenant_id: u.tenant_id,
            tenant_name: None,
            created_by: u.created_by,
            created_at: u.created_at.map(|t| t.to_string()),
            updated_by: u.updated_by,
            updated_at: u.updated_at.map(|t| t.to_string()),
        }
    }
}

/// 查询用户可访问的租户列表
///
/// 所有用户统一通过 sys_user_tenant 关联表查询，
/// 包括超级管理员也需要在 sys_user_tenant 中有记录。
async fn get_available_tenants(pool: &PgPool, user: &SysUser) -> Result<Vec<TenantInfo>, String> {
    let tenants: Vec<SysTenant> = if user.is_super_admin {
        // 超级管理员直接返回所有启用租户，不依赖 sys_user_tenant 关联表
        sqlx::query_as::<_, SysTenant>(
            "SELECT id, tenant_name, parent_id, is_leaf, schema_name, enable, create_at, updated_at
             FROM public.sys_tenant WHERE enable = true
             ORDER BY id ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| {
            error!("查询租户列表失败: {}", e);
            format!("查询租户列表失败: {}", e)
        })?
    } else {
        // 普通用户从 sys_user_tenant 关联表查询
        sqlx::query_as::<_, SysTenant>(
            "SELECT t.id, t.tenant_name, t.parent_id, t.is_leaf, t.schema_name, t.enable, t.create_at, t.updated_at
             FROM public.sys_user_tenant ut
             JOIN public.sys_tenant t ON t.id = ut.tenant_id
             WHERE ut.user_id = $1 AND t.enable = true
             ORDER BY t.id ASC"
        )
        .bind(user.id)
        .fetch_all(pool)
        .await
        .map_err(|e| {
            error!("查询用户租户列表失败: {}", e);
            format!("查询用户租户列表失败: {}", e)
        })?
    };

    let current_tenant_id = user.tenant_id;
    Ok(tenants
        .into_iter()
        .map(|t| TenantInfo {
            id: t.id,
            tenant_name: t.tenant_name,
            schema_name: t.schema_name,
            is_current: Some(t.id) == current_tenant_id,
        })
        .collect())
}

/// 用户登录
pub async fn login(username: &str, password: &str) -> Result<LoginResponse, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    info!("用户登录尝试: username={}", username);

    // 所有用户都存储在 public.sys_user 中，统一从 public schema 查询
    let user = sqlx::query_as::<_, SysUser>(
        "SELECT id, username, passwd, domain, real_name, email, phone, department_id, is_super_admin, status, nickname, avatar, person_id, person_code, super_user_id, tenant_id, created_by, created_at, updated_by, updated_at, deleted FROM public.sys_user WHERE username = $1 AND (deleted IS NULL OR deleted = 0)"
    )
    .bind(username)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        error!("查询用户失败: username={}, error={}", username, e);
        format!("查询用户失败: {}", e)
    })?
    .ok_or_else(|| {
        warn!("登录失败，用户不存在: username={}", username);
        "用户名或密码错误".to_string()
    })?;

    // 检查用户状态
    if user.status == 0 {
        warn!(
            "登录失败，用户已被禁用: username={}, id={}",
            username, user.id
        );
        return Err("该用户已被禁用".to_string());
    }

    // 验证密码
    let valid =
        verify_password(password, &user.passwd).map_err(|e| format!("密码验证失败: {}", e))?;

    if !valid {
        warn!("登录失败，密码错误: username={}", username);
        return Err("用户名或密码错误".to_string());
    }

    // 查询用户可访问的租户列表
    let available_tenants = get_available_tenants(&pool, &user).await?;

    // 更新 USER_TENANT_CACHE（用户当前选中租户）
    // 同时更新 GLOBAL_SCHEMA（Tauri 模式使用）
    let cache = database::postgres::get_user_tenant_cache();
    if let Some(tenant_id) = user.tenant_id {
        cache.insert(user.id, tenant_id);
        // 查询并设置全局 schema
        if let Some(tenant) = available_tenants.iter().find(|t| t.id == tenant_id) {
            if let Some(ref sn) = tenant.schema_name {
                database::set_global_schema(sn.clone());
            }
        }
        info!(
            "用户 '{}' 已设置当前租户: tenant_id={}",
            username, tenant_id
        );
    } else if user.is_super_admin && !available_tenants.is_empty() {
        // 超级管理员没有 tenant_id，默认选中第一个可用租户
        let first_tenant = &available_tenants[0];
        cache.insert(user.id, first_tenant.id);
        // 设置全局 schema
        if let Some(ref sn) = first_tenant.schema_name {
            database::set_global_schema(sn.clone());
        }
        info!(
            "超级管理员 '{}' 已默认选中第一个租户: tenant_id={}",
            username, first_tenant.id
        );
    }

    info!(
        "用户登录成功: username={}, id={}, real_name={}",
        username, user.id, user.real_name
    );

    // 签发访问令牌 + 刷新令牌（刷新令牌写入白名单，供 /api/auth/refresh 轮换）
    let family_id = next_id() as i64;
    let token_pair = issue_token_pair(&pool, user.id, &user.username, family_id).await?;

    // 返回用户信息（不含密码）+ JWT Token + available_tenants
    Ok(LoginResponse {
        id: user.id,
        username: user.username,
        real_name: user.real_name,
        email: user.email,
        phone: user.phone,
        department_id: user.department_id,
        is_super_admin: user.is_super_admin,
        status: user.status,
        nickname: user.nickname,
        avatar: user.avatar,
        tenant_id: user.tenant_id,
        token: token_pair.token,
        refresh_token: token_pair.refresh_token,
        expires_in: token_pair.expires_in,
        available_tenants,
    })
}

/// 用于查询用户列表时携带机构名称的中间结构
#[derive(Debug)]
struct UserWithTenant {
    user: SysUser,
    tenant_name: Option<String>,
}

impl sqlx::FromRow<'_, sqlx::postgres::PgRow> for UserWithTenant {
    fn from_row(row: &sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        let user = SysUser::from_row(row)?;
        let tenant_name: Option<String> = row.try_get("tenant_name")?;
        Ok(UserWithTenant { user, tenant_name })
    }
}

/// 获取用户列表
///
/// 如果 tenant_id 为 Some，则只查询该机构下的用户；
/// 如果为 None（超级管理员），则查询所有机构的用户。
/// keyword 可选，用于按用户名或真实姓名模糊搜索。
pub async fn get_users(
    tenant_id: Option<i64>,
    keyword: Option<String>,
) -> Result<Vec<UserResponse>, String> {
    let pool = get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    // 使用 LEFT JOIN 查询用户及其所属机构名称
    let rows = if let Some(tid) = tenant_id {
        if let Some(ref kw) = keyword {
            sqlx::query_as::<_, UserWithTenant>(
                r#"
                SELECT u.id, u.username, u.passwd, u.domain, u.real_name, u.email, u.phone, u.department_id, u.is_super_admin, u.status, u.nickname, u.avatar, u.person_id, u.person_code, u.super_user_id, u.tenant_id, u.created_by, u.created_at, u.updated_by, u.updated_at, u.deleted,
                       t.tenant_name
                FROM public.sys_user u
                LEFT JOIN public.sys_tenant t ON u.tenant_id = t.id
                WHERE (u.deleted IS NULL OR u.deleted = 0) AND u.tenant_id = $1
                  AND (u.username ILIKE '%' || $2 || '%' OR u.real_name ILIKE '%' || $2 || '%')
                ORDER BY u.id ASC
                "#,
            )
            .bind(tid)
            .bind(kw)
            .fetch_all(&pool)
            .await
            .map_err(|e| {
                error!("查询用户列表失败: {}", e);
                format!("查询用户列表失败: {}", e)
            })?
        } else {
            sqlx::query_as::<_, UserWithTenant>(
                r#"
                SELECT u.id, u.username, u.passwd, u.domain, u.real_name, u.email, u.phone, u.department_id, u.is_super_admin, u.status, u.nickname, u.avatar, u.person_id, u.person_code, u.super_user_id, u.tenant_id, u.created_by, u.created_at, u.updated_by, u.updated_at, u.deleted,
                       t.tenant_name
                FROM public.sys_user u
                LEFT JOIN public.sys_tenant t ON u.tenant_id = t.id
                WHERE (u.deleted IS NULL OR u.deleted = 0) AND u.tenant_id = $1
                ORDER BY u.id ASC
                "#,
            )
            .bind(tid)
            .fetch_all(&pool)
            .await
            .map_err(|e| {
                error!("查询用户列表失败: {}", e);
                format!("查询用户列表失败: {}", e)
            })?
        }
    } else {
        if let Some(ref kw) = keyword {
            sqlx::query_as::<_, UserWithTenant>(
                r#"
                SELECT u.id, u.username, u.passwd, u.domain, u.real_name, u.email, u.phone, u.department_id, u.is_super_admin, u.status, u.nickname, u.avatar, u.person_id, u.person_code, u.super_user_id, u.tenant_id, u.created_by, u.created_at, u.updated_by, u.updated_at, u.deleted,
                       t.tenant_name
                FROM public.sys_user u
                LEFT JOIN public.sys_tenant t ON u.tenant_id = t.id
                WHERE (u.deleted IS NULL OR u.deleted = 0)
                  AND (u.username ILIKE '%' || $1 || '%' OR u.real_name ILIKE '%' || $1 || '%')
                ORDER BY u.id ASC
                "#,
            )
            .bind(kw)
            .fetch_all(&pool)
            .await
            .map_err(|e| {
                error!("查询用户列表失败: {}", e);
                format!("查询用户列表失败: {}", e)
            })?
        } else {
            sqlx::query_as::<_, UserWithTenant>(
                r#"
                SELECT u.id, u.username, u.passwd, u.domain, u.real_name, u.email, u.phone, u.department_id, u.is_super_admin, u.status, u.nickname, u.avatar, u.person_id, u.person_code, u.super_user_id, u.tenant_id, u.created_by, u.created_at, u.updated_by, u.updated_at, u.deleted,
                       t.tenant_name
                FROM public.sys_user u
                LEFT JOIN public.sys_tenant t ON u.tenant_id = t.id
                WHERE u.deleted IS NULL OR u.deleted = 0
                ORDER BY u.id ASC
                "#,
            )
            .fetch_all(&pool)
            .await
            .map_err(|e| {
                error!("查询用户列表失败: {}", e);
                format!("查询用户列表失败: {}", e)
            })?
        }
    };

    let count = rows.len();
    info!("查询用户列表成功: 共 {} 条记录", count);
    Ok(rows
        .into_iter()
        .map(|row| {
            let mut resp: UserResponse = row.user.into();
            resp.tenant_name = row.tenant_name;
            resp
        })
        .collect())
}

/// 新增用户
pub async fn insert_user(
    username: &str,
    password: &str,
    real_name: &str,
    email: Option<&str>,
    phone: Option<&str>,
    department_id: Option<i64>,
    status: i16,
    nickname: Option<&str>,
    person_id: Option<&str>,
    person_code: Option<&str>,
    super_user_id: Option<i64>,
    tenant_id: Option<i64>,
    created_by: Option<i64>,
) -> Result<UserResponse, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    info!("新增用户: username={}, real_name={}", username, real_name);

    // sys_user 是公共表，始终从 public schema 查询
    // 检查用户名是否已存在
    let existing = sqlx::query_as::<_, SysUser>(
        "SELECT id, username, passwd, domain, real_name, email, phone, department_id, is_super_admin, status, nickname, avatar, person_id, person_code, super_user_id, tenant_id, created_by, created_at, updated_by, updated_at, deleted FROM public.sys_user WHERE username = $1 AND (deleted IS NULL OR deleted = 0)"
    )
    .bind(username)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        error!("查询用户是否存在失败: username={}, error={}", username, e);
        format!("查询用户失败: {}", e)
    })?;

    if existing.is_some() {
        warn!("新增用户失败，用户名已存在: username={}", username);
        return Err("用户名已存在".to_string());
    }

    // 加密密码
    let hashed_password = hash_password(password)?;

    // 确定 tenant_id：如果未指定，保持为 null（超级管理员不属于任何机构）
    let final_tenant_id = tenant_id;

    let user = sqlx::query_as::<_, SysUser>(
        r#"
        INSERT INTO public.sys_user (id, username, passwd, domain, real_name, email, phone, department_id, is_super_admin, status, nickname, avatar, person_id, person_code, super_user_id, tenant_id, created_by, created_at, updated_by, updated_at, deleted)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, NOW(), $18, NOW(), 0)
        RETURNING id, username, passwd, domain, real_name, email, phone, department_id, is_super_admin, status, nickname, avatar, person_id, person_code, super_user_id, tenant_id, created_by, created_at, updated_by, updated_at, deleted
        "#,
    )
    .bind(next_id() as i64)
    .bind(username)
    .bind(&hashed_password)
    .bind(Option::<String>::None) // domain
    .bind(real_name)
    .bind(email)
    .bind(phone)
    .bind(department_id)
    .bind(false) // is_super_admin: 默认非超级管理员
    .bind(status)
    .bind(nickname)
    .bind(Option::<String>::None) // avatar
    .bind(person_id)
    .bind(person_code)
    .bind(super_user_id)
    .bind(final_tenant_id)
    .bind(created_by)
    .bind(created_by) // updated_by
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        error!("新增用户数据库操作失败: username={}, error={}", username, e);
        format!("新增用户失败: {}", e)
    })?;

    // 如果是超级管理员，自动分配所有启用的租户
    if user.is_super_admin {
        info!("超级管理员 '{}' 创建时自动分配所有租户...", user.username);
        let all_tenants: Vec<(i64,)> = sqlx::query_as::<_, (i64,)>(
            "SELECT id FROM public.sys_tenant WHERE enable = true ORDER BY id ASC",
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询租户列表失败: {}", e);
            format!("查询租户列表失败: {}", e)
        })?;

        let tenant_ids: Vec<i64> = all_tenants.into_iter().map(|(id,)| id).collect();
        if !tenant_ids.is_empty() {
            crate::tenant_service::assign_user_tenants(
                user.id,
                &tenant_ids,
                created_by.unwrap_or(user.id),
            )
            .await?;
            info!(
                "超级管理员 '{}' 已分配 {} 个租户",
                user.username,
                tenant_ids.len()
            );
        }
    }

    info!("新增用户成功: id={}, username={}", user.id, user.username);
    Ok(user.into())
}

/// 更新用户信息
pub async fn update_user(
    id: i64,
    username: &str,
    real_name: &str,
    email: Option<&str>,
    phone: Option<&str>,
    department_id: Option<i64>,
    status: i16,
    nickname: Option<&str>,
    person_id: Option<&str>,
    person_code: Option<&str>,
    super_user_id: Option<i64>,
    updated_by: Option<i64>,
) -> Result<UserResponse, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    info!("更新用户信息: id={}, username={}", id, username);

    // sys_user 是公共表，始终从 public schema 操作
    let user = sqlx::query_as::<_, SysUser>(
        r#"
        UPDATE public.sys_user
        SET username = $2, real_name = $3, email = $4, phone = $5, department_id = $6, status = $7, nickname = $8, person_id = $9, person_code = $10, super_user_id = $11, updated_by = $12, updated_at = NOW()
        WHERE id = $1 AND (deleted IS NULL OR deleted = 0)
        RETURNING id, username, passwd, domain, real_name, email, phone, department_id, is_super_admin, status, nickname, avatar, person_id, person_code, super_user_id, tenant_id, created_by, created_at, updated_by, updated_at, deleted
        "#,
    )
    .bind(id)
    .bind(username)
    .bind(real_name)
    .bind(email)
    .bind(phone)
    .bind(department_id)
    .bind(status)
    .bind(nickname)
    .bind(person_id)
    .bind(person_code)
    .bind(super_user_id)
    .bind(updated_by)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        error!("更新用户失败: id={}, error={}", id, e);
        format!("更新用户失败: {}", e)
    })?;

    // 禁用用户时立即吊销其全部刷新令牌（已签发的访问令牌到期后自然失效）
    if status != 1 {
        if let Err(e) = revoke_all_user_refresh_tokens(id).await {
            warn!("禁用用户后吊销刷新令牌失败: id={}, error={}", id, e);
        }
    }

    info!("更新用户成功: id={}, username={}", id, username);
    Ok(user.into())
}

/// 删除用户（软删除）
///
/// 权限校验：
/// - 超级管理员不能被任何人删除（包括超级管理员自己）
/// - 非超级管理员只能删除自己所在机构的用户
pub async fn delete_user(
    id: i64,
    current_user_id: i64,
    is_super_admin: bool,
) -> Result<(), String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    info!(
        "删除用户: id={}, current_user_id={}, is_super_admin={}",
        id, current_user_id, is_super_admin
    );

    // 先查询目标用户
    let target_user = sqlx::query_as::<_, SysUser>(
        "SELECT id, username, passwd, domain, real_name, email, phone, department_id, is_super_admin, status, nickname, avatar, person_id, person_code, super_user_id, tenant_id, created_by, created_at, updated_by, updated_at, deleted FROM public.sys_user WHERE id = $1 AND (deleted IS NULL OR deleted = 0)"
    )
    .bind(id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        error!("查询目标用户失败: id={}, error={}", id, e);
        format!("查询用户失败: {}", e)
    })?
    .ok_or_else(|| {
        warn!("要删除的用户不存在: id={}", id);
        "用户不存在".to_string()
    })?;

    // 超级管理员不能被任何人删除
    if target_user.is_super_admin {
        warn!(
            "禁止删除超级管理员: id={}, username={}",
            id, target_user.username
        );
        return Err("超级管理员不能被删除".to_string());
    }

    // 非超级管理员只能删除自己所在机构的用户
    if !is_super_admin {
        // 查询当前用户的信息以获取其 tenant_id
        let current_user = sqlx::query_as::<_, SysUser>(
            "SELECT id, username, passwd, domain, real_name, email, phone, department_id, is_super_admin, status, nickname, avatar, person_id, person_code, super_user_id, tenant_id, created_by, created_at, updated_by, updated_at, deleted FROM public.sys_user WHERE id = $1 AND (deleted IS NULL OR deleted = 0)"
        )
        .bind(current_user_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| {
            error!("查询当前用户失败: id={}, error={}", current_user_id, e);
            format!("查询当前用户失败: {}", e)
        })?
        .ok_or_else(|| {
            warn!("当前用户不存在: id={}", current_user_id);
            "当前用户不存在".to_string()
        })?;

        if current_user.tenant_id != target_user.tenant_id {
            warn!(
                "非超级管理员跨机构删除被拒绝: current_user_tenant={:?}, target_user_tenant={:?}",
                current_user.tenant_id, target_user.tenant_id
            );
            return Err("只能删除本机构的用户".to_string());
        }
    }

    // sys_user 是公共表，始终从 public schema 操作
    sqlx::query("UPDATE public.sys_user SET deleted = 1, updated_at = NOW() WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("删除用户失败: id={}, error={}", id, e);
            format!("删除用户失败: {}", e)
        })?;

    // 删除用户后吊销其全部刷新令牌
    if let Err(e) = revoke_all_user_refresh_tokens(id).await {
        warn!("删除用户后吊销刷新令牌失败: id={}, error={}", id, e);
    }

    info!("删除用户成功: id={}", id);
    Ok(())
}

/// 根据用户ID获取用户信息
pub async fn get_user_by_id(id: i64) -> Result<UserResponse, String> {
    let pool = get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    // sys_user 是公共表，始终从 public schema 查询
    let user = sqlx::query_as::<_, SysUser>(
        "SELECT id, username, passwd, domain, real_name, email, phone, department_id, is_super_admin, status, nickname, avatar, person_id, person_code, super_user_id, tenant_id, created_by, created_at, updated_by, updated_at, deleted FROM public.sys_user WHERE id = $1 AND (deleted IS NULL OR deleted = 0)"
    )
    .bind(id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        error!("查询用户失败: id={}, error={}", id, e);
        format!("查询用户失败: {}", e)
    })?
    .ok_or_else(|| {
        warn!("用户不存在: id={}", id);
        "用户不存在".to_string()
    })?;

    info!(
        "获取用户信息成功: id={}, username={}",
        user.id, user.username
    );
    Ok(user.into())
}

/// 重置密码
pub async fn reset_password(id: i64, new_password: &str) -> Result<(), String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    info!("重置用户密码: id={}", id);

    let hashed_password = hash_password(new_password)?;

    // sys_user 是公共表，始终从 public schema 操作
    sqlx::query("UPDATE public.sys_user SET passwd = $2, updated_at = NOW() WHERE id = $1")
        .bind(id)
        .bind(&hashed_password)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("重置密码失败: id={}, error={}", id, e);
            format!("重置密码失败: {}", e)
        })?;

    // 密码变更后吊销全部刷新令牌，强制重新登录
    if let Err(e) = revoke_all_user_refresh_tokens(id).await {
        warn!("重置密码后吊销刷新令牌失败: id={}, error={}", id, e);
    }

    info!("重置用户密码成功: id={}", id);
    Ok(())
}
