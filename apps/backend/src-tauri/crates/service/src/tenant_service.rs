//! 租户管理 Service
//!
//! 提供租户的增删改查功能，新增租户时自动创建对应的 PostgreSQL schema
//! 并初始化业务表结构和默认数据。

use database::models::{SysTenant, TenantInfo};
use database::{get_read_pool, get_write_pool};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{error, info, warn};
use utils::snowflake::next_id;

/// 租户名称最大长度（与 public.sys_tenant.tenant_name 的 VARCHAR(100) 保持一致）
const MAX_TENANT_NAME_LEN: usize = 100;

/// schema 名称最大长度（与 public.sys_tenant.schema_name 的 VARCHAR(50) 保持一致）
const MAX_SCHEMA_NAME_LEN: usize = 50;

/// 禁止用作租户 schema 的系统 schema
const RESERVED_SCHEMA_NAMES: [&str; 4] = ["public", "information_schema", "pg_catalog", "pg_toast"];

/// PostgreSQL 保留字（未加引号时会导致 SQL 语法错误，禁止作为 schema 名称）
const RESERVED_SQL_KEYWORDS: &str =
    "all analyse analyze and any array as asc asymmetric both case cast \
check collate collation column concurrently constraint create cross current_catalog current_date \
current_role current_schema current_time current_timestamp current_user default deferrable desc \
distinct do else end except false fetch for foreign freeze from full grant group having ilike in \
initially inner intersect into is isnull join lateral leading left like limit localtime \
localtimestamp natural not notnull null offset on only or order outer overlaps placing primary \
references returning right select session_user similar some symmetric table tablesample then to \
trailing true union unique user using variadic verbose when where window with";

/// 判断名称是否为 PostgreSQL 保留字
fn is_reserved_keyword(name: &str) -> bool {
    RESERVED_SQL_KEYWORDS
        .split_whitespace()
        .any(|keyword| keyword == name)
}

/// 校验并规范化租户名称
fn normalize_tenant_name(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("租户名称不能为空".to_string());
    }
    if trimmed.chars().count() > MAX_TENANT_NAME_LEN {
        return Err(format!(
            "租户名称长度不能超过 {} 个字符",
            MAX_TENANT_NAME_LEN
        ));
    }
    Ok(trimmed.to_string())
}

/// 校验并规范化 schema 名称
///
/// schema 名称会被拼接进 DDL（CREATE SCHEMA / 初始化脚本），必须严格限制字符集；
/// 同时统一转为小写，避免 PostgreSQL 对未加引号标识符的大小写折叠导致名称不一致。
fn normalize_schema_name(raw: &str) -> Result<String, String> {
    let normalized = raw.trim().to_ascii_lowercase();

    if normalized.is_empty() {
        return Err("schema 名称不能为空".to_string());
    }
    if normalized.chars().count() > MAX_SCHEMA_NAME_LEN {
        return Err(format!(
            "schema 名称长度不能超过 {} 个字符",
            MAX_SCHEMA_NAME_LEN
        ));
    }

    let starts_with_letter = normalized
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false);
    let valid_chars = normalized
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_');

    if !starts_with_letter || !valid_chars {
        return Err("schema 名称必须以字母开头，且只能包含字母、数字和下划线".to_string());
    }

    if normalized.starts_with("pg_")
        || RESERVED_SCHEMA_NAMES.contains(&normalized.as_str())
        || is_reserved_keyword(&normalized)
    {
        return Err(format!(
            "schema 名称 '{}' 为系统保留名称或 SQL 保留字，禁止使用",
            normalized
        ));
    }

    Ok(normalized)
}

/// 为标识符加双引号，防止保留字/特殊字符导致 SQL 语法错误
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// 判断数据库错误是否为唯一约束冲突
fn is_unique_violation(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .map(|db_err| db_err.is_unique_violation())
        .unwrap_or(false)
}

/// 回滚时尽力删除本次创建的 schema，失败仅记录日志
async fn drop_schema_best_effort(pool: &PgPool, schema: &str) {
    let sql = format!("DROP SCHEMA IF EXISTS {} CASCADE", quote_ident(schema));
    match sqlx::query(sqlx::AssertSqlSafe(sql)).execute(pool).await {
        Ok(_) => warn!("回滚成功，已删除 schema '{}'", schema),
        Err(e) => error!("回滚删除 schema '{}' 失败，请手动清理: {}", schema, e),
    }
}

/// 清理指定租户相关的内存缓存（schema 缓存 + 用户当前租户缓存）
fn evict_tenant_cache(tenant_id: i64) {
    database::postgres::get_schema_cache().remove(&tenant_id);
    database::postgres::get_user_tenant_cache().retain(|_, current| *current != tenant_id);
}

/// 租户 schema 可用性检查结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TenantSchemaState {
    /// schema 与业务表均已就绪
    Ready,
    /// schema 不存在（可能被误删，或创建过程从未成功）
    Missing,
    /// schema 存在但没有任何表（建表过程中断 / 表被清空）
    Empty,
}

/// 检查租户 schema 是否仍然可用
///
/// 只判断 schema 是否存在并不充分：`CREATE SCHEMA` 成功但建表失败会残留一个空 schema，
/// 此时放行切换会让后续业务查询持续报 `relation does not exist`，
/// 比在切换时就提示重建更难定位，因此在切换入口一并校验 schema 内是否已有业务表。
async fn inspect_tenant_schema(
    pool: &PgPool,
    schema: &str,
) -> Result<TenantSchemaState, sqlx::Error> {
    let (schema_exists, has_table) = sqlx::query_as::<_, (bool, bool)>(
        "SELECT
             EXISTS(SELECT 1 FROM pg_namespace n WHERE n.nspname = $1),
             EXISTS(
                 SELECT 1
                 FROM pg_class c
                 JOIN pg_namespace n ON n.oid = c.relnamespace
                 WHERE n.nspname = $1 AND c.relkind IN ('r', 'p')
             )",
    )
    .bind(schema)
    .fetch_one(pool)
    .await?;

    Ok(if !schema_exists {
        TenantSchemaState::Missing
    } else if !has_table {
        TenantSchemaState::Empty
    } else {
        TenantSchemaState::Ready
    })
}

/// 清理已失效租户 schema 的内存缓存
///
/// SCHEMA_CACHE 会在应用启动预加载（`preload_schema_cache`）或上次切换成功时写入，
/// 若 schema 已被删除却保留缓存，auth 中间件会继续把请求路由到不存在的 schema，
/// 因此切换失败时必须把缓存一起清掉，而不是只返回错误。
fn invalidate_broken_tenant_cache(tenant_id: i64, schema_name: &str) {
    evict_tenant_cache(tenant_id);

    // Tauri 模式不经过 auth 中间件，GLOBAL_SCHEMA 可能仍指向该失效 schema，需回退到 public
    if database::current_schema_name() == schema_name {
        database::set_global_schema("public".to_string());
        warn!(
            "租户 schema '{}' 已失效，全局 schema 已回退到 public",
            schema_name
        );
    }
}

/// 租户响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantResponse {
    #[serde(serialize_with = "database::models::i64_to_string")]
    pub id: i64,
    pub tenant_name: String,
    #[serde(
        serialize_with = "database::models::opt_i64_to_string",
        deserialize_with = "database::models::opt_i64_from_string"
    )]
    pub parent_id: Option<i64>,
    pub is_leaf: bool,
    pub schema_name: Option<String>,
    pub enable: bool,
    pub create_at: Option<String>,
    pub updated_at: Option<String>,
}

impl From<SysTenant> for TenantResponse {
    fn from(t: SysTenant) -> Self {
        Self {
            id: t.id,
            tenant_name: t.tenant_name,
            parent_id: t.parent_id,
            is_leaf: t.is_leaf,
            schema_name: t.schema_name,
            enable: t.enable,
            create_at: t.create_at.map(|t| t.to_string()),
            updated_at: t.updated_at.map(|t| t.to_string()),
        }
    }
}

/// 获取所有租户列表
pub async fn get_tenants() -> Result<Vec<TenantResponse>, String> {
    let pool = get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    let tenants = sqlx::query_as::<_, SysTenant>(
        "SELECT id, tenant_name, parent_id, is_leaf, schema_name, enable, create_at, updated_at FROM public.sys_tenant ORDER BY id ASC"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        error!("查询租户列表失败: {}", e);
        format!("查询租户列表失败: {}", e)
    })?;

    let count = tenants.len();
    info!("查询租户列表成功: 共 {} 条记录", count);
    Ok(tenants.into_iter().map(|t| t.into()).collect())
}

/// 新增租户
///
/// 1. 校验租户名称、schema 名称，并检查名称/schema/管理员账号是否冲突
/// 2. 如果是末级节点（is_leaf=true），创建 PostgreSQL schema 并初始化表结构与默认数据
/// 3. 在事务中写入租户记录、初始管理员、角色关联以及超级管理员绑定
/// 4. 任一步骤失败都会回滚事务并清理已创建的 schema，避免残留半成品租户
pub async fn insert_tenant(
    tenant_name: &str,
    parent_id: Option<i64>,
    is_leaf: bool,
    schema_name: Option<&str>,
    enable: bool,
    created_by: Option<i64>,
) -> Result<TenantResponse, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    let tenant_name = normalize_tenant_name(tenant_name)?;

    // 仅末级节点拥有独立 schema；分组节点即使传了 schema 也忽略，避免出现“有 schema 但未创建”的脏数据
    let schema_name = if is_leaf {
        Some(normalize_schema_name(schema_name.ok_or_else(|| {
            "末级租户必须指定 schema 名称".to_string()
        })?)?)
    } else {
        if schema_name.is_some() {
            warn!(
                "分组节点不需要独立 schema，已忽略传入的 schema_name: tenant_name={}",
                tenant_name
            );
        }
        None
    };

    info!(
        "新增租户: tenant_name={}, parent_id={:?}, is_leaf={}, schema_name={:?}",
        tenant_name, parent_id, is_leaf, schema_name
    );

    // 校验父节点：必须存在且处于启用状态
    if let Some(pid) = parent_id {
        let parent_enabled =
            sqlx::query_scalar::<_, bool>("SELECT enable FROM public.sys_tenant WHERE id = $1")
                .bind(pid)
                .fetch_optional(&pool)
                .await
                .map_err(|e| {
                    error!("查询父租户失败: parent_id={}, error={}", pid, e);
                    format!("查询父租户失败: {}", e)
                })?
                .ok_or_else(|| format!("父租户不存在: id={}", pid))?;

        if !parent_enabled {
            warn!("新增租户失败，父租户已禁用: parent_id={}", pid);
            return Err(format!("父租户已禁用，无法新增子租户: id={}", pid));
        }
    }

    // 管理员账号由 schema 名称推导，需提前确认未被占用，避免事务中途失败
    let admin_username = schema_name.as_ref().map(|sn| format!("admin_{}", sn));

    // 检查 schema 名称是否被现有租户占用，以及数据库中是否已存在同名 schema
    if let Some(sn) = schema_name.as_deref() {
        let schema_registered = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM public.sys_tenant WHERE schema_name = $1)",
        )
        .bind(sn)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!(
                "查询 schema 是否被占用失败: schema_name={}, error={}",
                sn, e
            );
            format!("查询租户失败: {}", e)
        })?;

        if schema_registered {
            warn!("新增租户失败，schema 名称已存在: schema_name={}", sn);
            return Err("schema 名称已存在".to_string());
        }

        // 数据库里已存在同名 schema 时禁止复用，否则会把其他租户/历史残留的数据暴露给新租户
        let schema_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM pg_namespace WHERE nspname = $1)",
        )
        .bind(sn)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!(
                "查询数据库 schema 是否存在失败: schema_name={}, error={}",
                sn, e
            );
            format!("查询 schema 失败: {}", e)
        })?;

        if schema_exists {
            warn!(
                "新增租户失败，数据库中已存在同名 schema: schema_name={}",
                sn
            );
            return Err(format!("schema '{}' 已存在，请更换名称或先手动清理", sn));
        }

        if let Some(username) = admin_username.as_deref() {
            let user_exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM public.sys_user WHERE username = $1)",
            )
            .bind(username)
            .fetch_one(&pool)
            .await
            .map_err(|e| {
                error!(
                    "查询管理员账号是否存在失败: username={}, error={}",
                    username, e
                );
                format!("查询管理员账号失败: {}", e)
            })?;

            if user_exists {
                warn!("新增租户失败，管理员账号已存在: username={}", username);
                return Err(format!(
                    "管理员账号 '{}' 已存在（可能残留在 sys_user 中），请先清理",
                    username
                ));
            }
        }
    }

    // 检查 tenant_name 是否已存在
    let name_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM public.sys_tenant WHERE tenant_name = $1)",
    )
    .bind(&tenant_name)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        error!(
            "查询租户名称是否存在失败: tenant_name={}, error={}",
            tenant_name, e
        );
        format!("查询租户失败: {}", e)
    })?;

    if name_exists {
        warn!("新增租户失败，租户名称已存在: tenant_name={}", tenant_name);
        return Err("租户名称已存在".to_string());
    }

    // 密码加密放在事务之前，避免长时间占用数据库事务
    let admin_password_hash = if is_leaf {
        let default_password =
            std::env::var("DEFAULT_ADMIN_PASSWORD").unwrap_or_else(|_| "admin123".to_string());
        // 复用 utils 中统一封装的 Argon2 实现（与 user_service/register_service 保持一致）
        Some(utils::password_secret::hash_password(&default_password)?)
    } else {
        None
    };

    let id = next_id() as i64;

    // 先创建 schema 并初始化表/数据：失败时直接清理 schema，此时尚未写入任何 sys_tenant 记录
    if let Some(sn) = schema_name.as_deref() {
        info!("正在创建租户 schema '{}'...", sn);
        let create_schema_sql = format!("CREATE SCHEMA {}", quote_ident(sn));
        if let Err(e) = sqlx::query(sqlx::AssertSqlSafe(create_schema_sql))
            .execute(&pool)
            .await
        {
            error!("创建 schema '{}' 失败: {}", sn, e);
            return Err(format!("创建 schema '{}' 失败: {}", sn, e));
        }

        info!("正在初始化租户 '{}' 表结构...", sn);
        if let Err(e) = database::postgres::init_tenant_tables(&pool, sn).await {
            error!("初始化租户 '{}' 表结构失败: {}", sn, e);
            drop_schema_best_effort(&pool, sn).await;
            return Err(format!("初始化租户表结构失败: {}", e));
        }

        info!("正在初始化租户 '{}' 默认数据...", sn);
        if let Err(e) = database::postgres::init_tenant_default_data(&pool, sn).await {
            error!("初始化租户 '{}' 默认数据失败: {}", sn, e);
            drop_schema_best_effort(&pool, sn).await;
            return Err(format!("初始化租户默认数据失败: {}", e));
        }
    }

    let params = NewTenantParams {
        id,
        tenant_name: &tenant_name,
        parent_id,
        is_leaf,
        schema_name: schema_name.as_deref(),
        admin_username: admin_username.as_deref(),
        enable,
        created_by,
    };

    let tenant = match persist_new_tenant(&pool, &params, admin_password_hash.as_deref()).await {
        Ok(tenant) => tenant,
        Err(e) => {
            // 事务通常已回滚，需要清理已创建的 schema；但若提交阶段发生网络异常，
            // 事务可能已在服务端提交，必须先确认租户记录是否存在，避免误删有效数据
            let persisted = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM public.sys_tenant WHERE id = $1)",
            )
            .bind(id)
            .fetch_one(&pool)
            .await;

            match persisted {
                Ok(true) => {
                    warn!(
                        "新增租户返回错误但记录已写入，保留 schema 以便人工确认: tenant_id={}, error={}",
                        id, e
                    );
                }
                Ok(false) => {
                    if let Some(sn) = schema_name.as_deref() {
                        drop_schema_best_effort(&pool, sn).await;
                    }
                }
                Err(check_err) => {
                    error!(
                        "无法确认租户是否已写入，跳过 schema 清理: tenant_id={}, error={}",
                        id, check_err
                    );
                }
            }

            return Err(e);
        }
    };

    if tenant.enable {
        if let Some(sn) = tenant.schema_name.as_ref() {
            database::postgres::get_schema_cache().insert(id, sn.clone());
        }
    }

    info!("新增租户成功: id={}, tenant_name={}", id, tenant_name);
    Ok(tenant.into())
}

/// persist_new_tenant 的入参
struct NewTenantParams<'a> {
    id: i64,
    tenant_name: &'a str,
    parent_id: Option<i64>,
    is_leaf: bool,
    schema_name: Option<&'a str>,
    admin_username: Option<&'a str>,
    enable: bool,
    created_by: Option<i64>,
}

/// 在事务中写入租户记录及其关联数据
///
/// 调用前 schema 必须已经创建并初始化完成；本函数返回错误时事务会自动回滚，
/// 由调用方负责清理已创建的 schema。
async fn persist_new_tenant(
    pool: &PgPool,
    params: &NewTenantParams<'_>,
    admin_password_hash: Option<&str>,
) -> Result<SysTenant, String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("开启事务失败: {}", e))?;

    // 1. 插入租户记录
    let tenant = sqlx::query_as::<_, SysTenant>(
        r#"
        INSERT INTO public.sys_tenant (id, tenant_name, parent_id, is_leaf, schema_name, enable, create_at)
        VALUES ($1, $2, $3, $4, $5, $6, NOW())
        RETURNING id, tenant_name, parent_id, is_leaf, schema_name, enable, create_at, updated_at
        "#,
    )
    .bind(params.id)
    .bind(params.tenant_name)
    .bind(params.parent_id)
    .bind(params.is_leaf)
    .bind(params.schema_name)
    .bind(params.enable)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        if is_unique_violation(&e) {
            error!(
                "新增租户唯一约束冲突: tenant_name={}, schema_name={:?}, error={}",
                params.tenant_name, params.schema_name, e
            );
            "租户名称或 schema 名称已存在".to_string()
        } else {
            error!(
                "新增租户数据库操作失败: tenant_name={}, error={}",
                params.tenant_name, e
            );
            format!("新增租户失败: {}", e)
        }
    })?;

    // 2. 有子节点后父节点不再属于末级节点
    if let Some(pid) = params.parent_id {
        sqlx::query(
            "UPDATE public.sys_tenant
             SET is_leaf = false, updated_at = NOW()
             WHERE id = $1 AND is_leaf = true",
        )
        .bind(pid)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            error!("更新父租户节点类型失败: parent_id={}, error={}", pid, e);
            format!("更新父租户失败: {}", e)
        })?;
    }

    // 3. 末级节点：创建初始管理员、租户级 admin 角色及相关关联
    if params.is_leaf {
        let username = params
            .admin_username
            .ok_or_else(|| "末级租户缺少管理员账号信息".to_string())?;
        let hash = admin_password_hash.ok_or_else(|| "末级租户缺少管理员密码".to_string())?;
        create_tenant_admin(&mut tx, params, username, hash).await?;
    }

    // 4. 新增租户后，自动为所有超级管理员绑定该租户
    let super_admins: Vec<(i64,)> = sqlx::query_as::<_, (i64,)>(
        "SELECT id FROM public.sys_user WHERE is_super_admin = true AND (deleted IS NULL OR deleted = 0)",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| {
        error!("查询超级管理员列表失败: {}", e);
        format!("查询超级管理员列表失败: {}", e)
    })?;

    for (sa_id,) in &super_admins {
        sqlx::query(
            "INSERT INTO public.sys_user_tenant (id, user_id, tenant_id, created_by)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (user_id, tenant_id) DO NOTHING",
        )
        .bind(next_id() as i64)
        .bind(sa_id)
        .bind(params.id)
        .bind(params.created_by)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            error!("为超级管理员绑定租户失败: sa_id={}, error={}", sa_id, e);
            format!("为超级管理员绑定租户失败: {}", e)
        })?;
    }

    tx.commit()
        .await
        .map_err(|e| format!("提交事务失败: {}", e))?;

    if !super_admins.is_empty() {
        info!(
            "已为 {} 个超级管理员自动绑定新租户: tenant_id={}",
            super_admins.len(),
            params.id
        );
    }

    Ok(tenant)
}

/// 创建租户初始管理员账号、租户级 admin 角色以及用户-角色/用户-租户关联
async fn create_tenant_admin(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    params: &NewTenantParams<'_>,
    username: &str,
    password_hash: &str,
) -> Result<(), String> {
    info!("正在为租户 '{}' 创建初始管理员账号...", params.tenant_name);

    let admin_user_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO public.sys_user
            (id, username, passwd, real_name, is_super_admin, tenant_id, created_by, created_at, status, deleted)
        VALUES ($1, $2, $3, $4, false, $5, $6, NOW(), 1, 0)
        RETURNING id
        "#,
    )
    .bind(next_id() as i64)
    .bind(username)
    .bind(password_hash)
    .bind(params.tenant_name)
    .bind(params.id)
    .bind(params.created_by)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| {
        error!(
            "为租户 '{}' 创建管理员账号失败: username={}, error={}",
            params.tenant_name, username, e
        );
        if is_unique_violation(&e) {
            format!("管理员账号 '{}' 已存在", username)
        } else {
            format!("创建管理员账号失败: {}", e)
        }
    })?;

    // 每个租户使用独立的 admin 角色，避免复用全局超级管理员角色（role_id = 1）
    let existing_role_id = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM public.sys_role
         WHERE role_key = 'admin' AND tenant_id = $1 AND (deleted IS NULL OR deleted = 0)
         ORDER BY id ASC LIMIT 1",
    )
    .bind(params.id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| {
        error!(
            "查询租户管理员角色失败: tenant_id={}, error={}",
            params.id, e
        );
        format!("查询租户管理员角色失败: {}", e)
    })?;

    let admin_role_id = match existing_role_id {
        Some(role_id) => role_id,
        None => {
            let role_id = next_id() as i64;
            sqlx::query(
                r#"
                INSERT INTO public.sys_role
                    (id, role_key, role_name, description, is_super_admin, tenant_id, created_by, created_at, deleted)
                VALUES ($1, 'admin', $2, $3, false, $4, $5, NOW(), 0)
                "#,
            )
            .bind(role_id)
            .bind(format!("{}管理员", params.tenant_name))
            .bind(format!("租户 {} 的默认管理员角色", params.tenant_name))
            .bind(params.id)
            .bind(params.created_by)
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                error!(
                    "为租户 '{}' 创建管理员角色失败: {}",
                    params.tenant_name, e
                );
                format!("创建管理员角色失败: {}", e)
            })?;

            copy_super_admin_menus(tx, role_id, params.created_by).await?;
            role_id
        }
    };

    // 绑定用户角色
    sqlx::query(
        "INSERT INTO public.sys_user_role (id, user_id, role_id, created_by, created_at, deleted)
         VALUES ($1, $2, $3, $4, NOW(), 0)",
    )
    .bind(next_id() as i64)
    .bind(admin_user_id)
    .bind(admin_role_id)
    .bind(params.created_by)
    .execute(&mut **tx)
    .await
    .map_err(|e| {
        error!(
            "为租户 '{}' 创建用户角色关联失败: user_id={}, role_id={}, error={}",
            params.tenant_name, admin_user_id, admin_role_id, e
        );
        format!("创建用户角色关联失败: {}", e)
    })?;

    // 管理员必须绑定自己的租户，否则登录后可用租户列表为空
    sqlx::query(
        "INSERT INTO public.sys_user_tenant (id, user_id, tenant_id, created_by)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (user_id, tenant_id) DO NOTHING",
    )
    .bind(next_id() as i64)
    .bind(admin_user_id)
    .bind(params.id)
    .bind(params.created_by)
    .execute(&mut **tx)
    .await
    .map_err(|e| {
        error!(
            "为租户管理员绑定租户失败: user_id={}, tenant_id={}, error={}",
            admin_user_id, params.id, e
        );
        format!("为租户管理员绑定租户失败: {}", e)
    })?;

    info!(
        "租户 '{}' 初始管理员创建成功: username={}, role_id={}",
        params.tenant_name, username, admin_role_id
    );
    Ok(())
}

/// 复制全局管理员角色（role_id = 1）的菜单授权到租户管理员角色
async fn copy_super_admin_menus(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    role_id: i64,
    created_by: Option<i64>,
) -> Result<(), String> {
    let menu_ids = sqlx::query_scalar::<_, i64>(
        "SELECT DISTINCT menu_id FROM public.sys_role_menu
         WHERE role_id = 1 AND (deleted IS NULL OR deleted = 0)",
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| {
        error!("查询全局管理员菜单授权失败: {}", e);
        format!("查询管理员菜单权限失败: {}", e)
    })?;

    for menu_id in menu_ids {
        sqlx::query(
            "INSERT INTO public.sys_role_menu (id, role_id, menu_id, created_by, created_at, deleted)
             VALUES ($1, $2, $3, $4, NOW(), 0)",
        )
        .bind(next_id() as i64)
        .bind(role_id)
        .bind(menu_id)
        .bind(created_by)
        .execute(&mut **tx)
        .await
        .map_err(|e| {
            error!(
                "复制租户管理员菜单授权失败: role_id={}, menu_id={}, error={}",
                role_id, menu_id, e
            );
            format!("创建管理员菜单权限失败: {}", e)
        })?;
    }

    Ok(())
}

/// 更新租户信息
pub async fn update_tenant(
    id: i64,
    tenant_name: &str,
    enable: bool,
) -> Result<TenantResponse, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    let tenant_name = normalize_tenant_name(tenant_name)?;

    info!(
        "更新租户信息: id={}, tenant_name={}, enable={}",
        id, tenant_name, enable
    );

    // tenant_name 没有数据库唯一约束，必须在应用层拦截重名
    let name_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM public.sys_tenant WHERE tenant_name = $1 AND id <> $2)",
    )
    .bind(&tenant_name)
    .bind(id)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        error!(
            "查询租户名称是否存在失败: tenant_name={}, error={}",
            tenant_name, e
        );
        format!("查询租户失败: {}", e)
    })?;

    if name_exists {
        warn!("更新租户失败，租户名称已存在: tenant_name={}", tenant_name);
        return Err("租户名称已存在".to_string());
    }

    let tenant = sqlx::query_as::<_, SysTenant>(
        r#"
        UPDATE public.sys_tenant
        SET tenant_name = $2, enable = $3, updated_at = NOW()
        WHERE id = $1
        RETURNING id, tenant_name, parent_id, is_leaf, schema_name, enable, create_at, updated_at
        "#,
    )
    .bind(id)
    .bind(&tenant_name)
    .bind(enable)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        error!("更新租户失败: id={}, error={}", id, e);
        format!("更新租户失败: {}", e)
    })?
    .ok_or_else(|| format!("租户不存在: id={}", id))?;

    if tenant.enable {
        if let Some(sn) = tenant.schema_name.as_ref() {
            database::postgres::get_schema_cache().insert(id, sn.clone());
        }
    } else {
        // 禁用后必须清理缓存，否则 auth 中间件仍会从缓存中取到该租户的 schema
        evict_tenant_cache(id);
    }

    info!(
        "更新租户成功: id={}, tenant_name={}",
        id, tenant.tenant_name
    );
    Ok(tenant.into())
}

/// 删除租户（软删除 - 禁用租户）
pub async fn delete_tenant(id: i64) -> Result<(), String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    info!("删除租户: id={}", id);

    // 不允许删除默认租户（id=1）
    if id == 1 {
        warn!("禁止删除默认租户: id={}", id);
        return Err("不能删除默认租户".to_string());
    }

    // 存在启用中的子组织时不允许禁用，否则会出现“父节点已删除、子节点仍启用”的断链状态
    let active_children = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.sys_tenant WHERE parent_id = $1 AND enable = true",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        error!("查询子组织失败: id={}, error={}", id, e);
        format!("查询子组织失败: {}", e)
    })?;

    if active_children > 0 {
        warn!(
            "删除租户失败，存在启用的子组织: id={}, count={}",
            id, active_children
        );
        return Err("该组织下存在启用的子组织，请先删除子组织".to_string());
    }

    // 软删除：将租户设为禁用状态
    let result = sqlx::query(
        "UPDATE public.sys_tenant SET enable = false, updated_at = NOW() WHERE id = $1 AND enable = true",
    )
    .bind(id)
    .execute(&pool)
    .await
    .map_err(|e| {
        error!("删除租户失败: id={}, error={}", id, e);
        format!("删除租户失败: {}", e)
    })?;

    if result.rows_affected() == 0 {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM public.sys_tenant WHERE id = $1)",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("查询租户失败: {}", e))?;

        if !exists {
            return Err(format!("租户不存在: id={}", id));
        }
        info!("租户已处于禁用状态，无需重复删除: id={}", id);
    }

    // 清理缓存，避免已禁用租户的 schema 仍被 auth 中间件命中
    evict_tenant_cache(id);

    info!("删除租户成功: id={}", id);
    Ok(())
}

/// 切换租户 schema（带权限校验）
///
/// 1. 校验用户状态及租户访问权限
/// 2. 校验目标租户已启用、拥有独立 schema，且 schema 结构完整（未被删除/清空）
/// 3. 更新 SCHEMA_CACHE / USER_TENANT_CACHE，并返回租户信息
///
/// schema 不可用时会同时清理失效缓存并回退全局 schema，避免后续请求继续命中坏 schema。
pub async fn switch_tenant(user_id: i64, tenant_id: i64) -> Result<TenantInfo, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    info!("切换租户: user_id={}, tenant_id={}", user_id, tenant_id);

    // 1. 检查用户是否是超级管理员（已软删除的用户不允许切换）
    let is_super_admin = sqlx::query_scalar::<_, bool>(
        "SELECT is_super_admin FROM public.sys_user
         WHERE id = $1 AND (deleted IS NULL OR deleted = 0)",
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("查询用户失败: {}", e))?
    .ok_or_else(|| "用户不存在或已被删除".to_string())?;

    // 2. 校验权限
    if !is_super_admin {
        let has_access = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM public.sys_user_tenant WHERE user_id = $1 AND tenant_id = $2)"
        )
        .bind(user_id)
        .bind(tenant_id)
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("查询权限失败: {}", e))?;

        if !has_access {
            warn!(
                "用户无权访问该租户: user_id={}, tenant_id={}",
                user_id, tenant_id
            );
            return Err("无权访问该租户".to_string());
        }
    }

    // 3. 直接读库获取租户信息，避免命中已失效的 schema 缓存
    let tenant = sqlx::query_as::<_, SysTenant>(
        "SELECT id, tenant_name, parent_id, is_leaf, schema_name, enable, create_at, updated_at
         FROM public.sys_tenant WHERE id = $1",
    )
    .bind(tenant_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("查询租户信息失败: {}", e))?
    .ok_or_else(|| format!("租户不存在: id={}", tenant_id))?;

    if !tenant.enable {
        warn!("切换租户失败，租户已禁用: tenant_id={}", tenant_id);
        return Err("租户已禁用".to_string());
    }

    let schema_name = tenant
        .schema_name
        .clone()
        .ok_or_else(|| "该组织为分组节点，未分配独立 schema，无法切换".to_string())?;

    let schema_state = inspect_tenant_schema(&pool, &schema_name)
        .await
        .map_err(|e| {
            error!(
                "查询租户 schema 状态失败: tenant_id={}, schema_name={}, error={}",
                tenant_id, schema_name, e
            );
            format!("查询租户 schema 失败: {}", e)
        })?;

    // schema 不存在或只剩空壳时都视为不可用：此时放行切换，前端只会拿到一堆
    // “relation does not exist”，不如在这里给出明确的重建提示
    let unusable_reason = match schema_state {
        TenantSchemaState::Ready => None,
        TenantSchemaState::Missing => Some("不存在"),
        TenantSchemaState::Empty => Some("结构不完整（缺少业务表）"),
    };

    if let Some(reason) = unusable_reason {
        error!(
            "切换租户失败，schema {}: tenant_id={}, schema_name={}",
            reason, tenant_id, schema_name
        );
        // 只返回错误而不清缓存，auth 中间件会继续按失效 schema 拼接 SQL
        invalidate_broken_tenant_cache(tenant_id, &schema_name);
        return Err(format!(
            "租户 schema '{}' {}，请联系管理员重建",
            schema_name, reason
        ));
    }

    // 4. 校验通过后再更新缓存
    database::postgres::get_schema_cache().insert(tenant_id, schema_name.clone());
    database::postgres::get_user_tenant_cache().insert(user_id, tenant_id);

    // 5. 更新 GLOBAL_SCHEMA（Tauri 模式使用，不经过 HTTP 中间件）
    database::set_global_schema(schema_name);

    info!("切换租户成功: user_id={}, tenant_id={}", user_id, tenant_id);
    Ok(TenantInfo {
        id: tenant.id,
        tenant_name: tenant.tenant_name,
        schema_name: tenant.schema_name,
        is_current: true,
    })
}

/// 为用户分配租户（覆盖式）
///
/// 事务：删除旧关联 → 插入新关联；重复的 tenant_id 会被自动去重
pub async fn assign_user_tenants(
    user_id: i64,
    tenant_ids: &[i64],
    current_user_id: i64,
) -> Result<(), String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    info!(
        "为用户分配租户: user_id={}, tenant_ids={:?}",
        user_id, tenant_ids
    );

    // 目标用户必须存在
    let user_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM public.sys_user WHERE id = $1 AND (deleted IS NULL OR deleted = 0))",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .map_err(|e| format!("查询用户失败: {}", e))?;

    if !user_exists {
        return Err(format!("用户不存在: id={}", user_id));
    }

    // 去重（保持顺序），避免触发 sys_user_tenant 的 UNIQUE(user_id, tenant_id)
    let mut unique_tenant_ids: Vec<i64> = Vec::with_capacity(tenant_ids.len());
    for tid in tenant_ids {
        if !unique_tenant_ids.contains(tid) {
            unique_tenant_ids.push(*tid);
        }
    }

    // 校验租户均存在且已启用
    if !unique_tenant_ids.is_empty() {
        let valid_tenant_ids: Vec<i64> = sqlx::query_scalar(
            "SELECT id FROM public.sys_tenant WHERE id = ANY($1) AND enable = true",
        )
        .bind(&unique_tenant_ids)
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!(
                "查询租户列表失败: tenant_ids={:?}, error={}",
                unique_tenant_ids, e
            );
            format!("查询租户列表失败: {}", e)
        })?;

        let invalid_ids: Vec<i64> = unique_tenant_ids
            .iter()
            .copied()
            .filter(|tid| !valid_tenant_ids.contains(tid))
            .collect();

        if !invalid_ids.is_empty() {
            warn!(
                "为用户分配租户失败，存在无效租户: user_id={}, invalid_ids={:?}",
                user_id, invalid_ids
            );
            return Err(format!("以下租户不存在或已禁用: {:?}", invalid_ids));
        }
    }

    // 事务：删除旧关联 → 插入新关联
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("开启事务失败: {}", e))?;

    sqlx::query("DELETE FROM public.sys_user_tenant WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("删除旧关联失败: {}", e))?;

    for tid in &unique_tenant_ids {
        sqlx::query(
            "INSERT INTO public.sys_user_tenant (id, user_id, tenant_id, created_by)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (user_id, tenant_id) DO NOTHING",
        )
        .bind(utils::snowflake::next_id() as i64)
        .bind(user_id)
        .bind(tid)
        .bind(current_user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("插入关联失败: {}", e))?;
    }

    tx.commit()
        .await
        .map_err(|e| format!("提交事务失败: {}", e))?;

    // 用户当前选中的租户被移除时清理缓存，避免继续使用已撤销的 schema
    let current_cached_tenant = database::postgres::get_user_tenant_cache()
        .get(&user_id)
        .map(|tid| *tid);

    if let Some(current_tenant) = current_cached_tenant {
        if !unique_tenant_ids.contains(&current_tenant) {
            database::postgres::get_user_tenant_cache().remove(&user_id);
            warn!(
                "用户当前租户已被移除，已清理缓存: user_id={}, tenant_id={}",
                user_id, current_tenant
            );
        }
    }

    info!(
        "为用户分配租户成功: user_id={}, count={}",
        user_id,
        unique_tenant_ids.len()
    );
    Ok(())
}

/// 获取用户可访问的租户列表
pub async fn get_user_tenants(user_id: i64) -> Result<Vec<TenantInfo>, String> {
    let pool = get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    // 先判断用户是否是超级管理员，同时取出用户所属组织作为当前租户的兜底值
    let (is_super_admin, primary_tenant_id) = sqlx::query_as::<_, (bool, Option<i64>)>(
        "SELECT is_super_admin, tenant_id FROM public.sys_user
         WHERE id = $1 AND (deleted IS NULL OR deleted = 0)",
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("查询用户失败: {}", e))?
    .ok_or_else(|| format!("用户不存在: id={}", user_id))?;

    let tenants: Vec<SysTenant> = if is_super_admin {
        sqlx::query_as::<_, SysTenant>(
            "SELECT id, tenant_name, parent_id, is_leaf, schema_name, enable, create_at, updated_at
             FROM public.sys_tenant WHERE enable = true ORDER BY id ASC",
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("查询租户列表失败: {}", e))?
    } else {
        sqlx::query_as::<_, SysTenant>(
            "SELECT t.id, t.tenant_name, t.parent_id, t.is_leaf, t.schema_name, t.enable, t.create_at, t.updated_at
             FROM public.sys_user_tenant ut
             JOIN public.sys_tenant t ON t.id = ut.tenant_id
             WHERE ut.user_id = $1 AND t.enable = true
             ORDER BY t.id ASC"
        )
        .bind(user_id)
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("查询用户租户列表失败: {}", e))?
    };

    // 当前租户：优先取缓存，未命中时回退到用户所属组织
    let current_cached_tenant = database::postgres::get_user_tenant_cache()
        .get(&user_id)
        .map(|tid| *tid);
    let current_tenant_id = current_cached_tenant.or(primary_tenant_id);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_tenant_name() {
        assert_eq!(normalize_tenant_name("  华东厂区 ").unwrap(), "华东厂区");
        assert!(normalize_tenant_name("").is_err());
        assert!(normalize_tenant_name("   ").is_err());
        assert!(normalize_tenant_name(&"a".repeat(MAX_TENANT_NAME_LEN)).is_ok());
        assert!(normalize_tenant_name(&"a".repeat(MAX_TENANT_NAME_LEN + 1)).is_err());
    }

    #[test]
    fn test_normalize_schema_name_valid() {
        assert_eq!(normalize_schema_name(" Factory_A1 ").unwrap(), "factory_a1");
        assert_eq!(normalize_schema_name("tenant_b").unwrap(), "tenant_b");
        assert_eq!(normalize_schema_name("a").unwrap(), "a");
        assert_eq!(
            normalize_schema_name(&"a".repeat(MAX_SCHEMA_NAME_LEN)).unwrap(),
            "a".repeat(MAX_SCHEMA_NAME_LEN)
        );
    }

    #[test]
    fn test_normalize_schema_name_invalid() {
        assert!(normalize_schema_name("").is_err());
        assert!(normalize_schema_name("   ").is_err());
        assert!(normalize_schema_name("1tenant").is_err());
        assert!(normalize_schema_name("_tenant").is_err());
        assert!(normalize_schema_name("tenant-a").is_err());
        assert!(normalize_schema_name("tenant a").is_err());
        assert!(normalize_schema_name("租户").is_err());
        assert!(normalize_schema_name(&"a".repeat(MAX_SCHEMA_NAME_LEN + 1)).is_err());
        // 注入尝试必须被字符集校验拦截
        assert!(normalize_schema_name("x; DROP SCHEMA public CASCADE; --").is_err());
    }

    #[test]
    fn test_normalize_schema_name_reserved() {
        for name in [
            "public",
            "PUBLIC",
            "information_schema",
            "pg_catalog",
            "pg_toast",
        ] {
            assert!(
                normalize_schema_name(name).is_err(),
                "{} 是系统保留名称，应被拒绝",
                name
            );
        }

        assert!(normalize_schema_name("pg_temp_1").is_err());
        assert!(normalize_schema_name("pg_catalog_backup").is_err());
        // SQL 保留字会导致未加引号的 DDL 语法错误
        assert!(normalize_schema_name("order").is_err());
        assert!(normalize_schema_name("USER").is_err());
        assert!(normalize_schema_name("check").is_err());
        assert!(normalize_schema_name("with").is_err());
        // 包含 pg_ 但不是前缀的名称不应被误伤
        assert!(normalize_schema_name("tenant_pg_1").is_ok());
        // 普通名称不应被误伤
        assert!(normalize_schema_name("orders").is_ok());
    }

    #[test]
    fn test_quote_ident() {
        assert_eq!(quote_ident("factory_a"), "\"factory_a\"");
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
    }
}
