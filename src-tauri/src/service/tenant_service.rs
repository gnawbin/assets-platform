//! 租户管理 Service
//!
//! 提供租户的增删改查功能，新增租户时自动创建对应的 PostgreSQL schema
//! 并初始化业务表结构和默认数据。

use crate::database::models::SysTenant;
use crate::database::{get_read_pool, get_write_pool};
use crate::utils::snowflake::next_id;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use utoipa::ToSchema;

/// 租户响应
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TenantResponse {
    #[serde(serialize_with = "crate::database::models::i64_to_string")]
    pub id: i64,
    pub tenant_name: String,
    #[serde(
        serialize_with = "crate::database::models::opt_i64_to_string",
        deserialize_with = "crate::database::models::opt_i64_from_string"
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
/// 1. 在 sys_tenant 表插入记录
/// 2. 如果是末级节点（is_leaf=true），自动创建对应的 PostgreSQL schema
/// 3. 在新 schema 中初始化业务表结构
/// 4. 在新 schema 中初始化默认数据
pub async fn insert_tenant(
    tenant_name: &str,
    parent_id: Option<i64>,
    is_leaf: bool,
    schema_name: Option<&str>,
    enable: bool,
    created_by: Option<i64>,
) -> Result<TenantResponse, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    info!(
        "新增租户: tenant_name={}, parent_id={:?}, is_leaf={}, schema_name={:?}",
        tenant_name, parent_id, is_leaf, schema_name
    );

    // 如果是末级节点，schema_name 必填
    if is_leaf && schema_name.is_none() {
        return Err("末级租户必须指定 schema 名称".to_string());
    }

    // 检查 schema_name 是否已存在（仅末级节点需要）
    if let Some(sn) = schema_name {
        let existing = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM public.sys_tenant WHERE schema_name = $1",
        )
        .bind(sn)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("查询租户是否存在失败: schema_name={}, error={}", sn, e);
            format!("查询租户失败: {}", e)
        })?;

        if existing > 0 {
            warn!("新增租户失败，schema 名称已存在: schema_name={}", sn);
            return Err("schema 名称已存在".to_string());
        }
    }

    // 检查 tenant_name 是否已存在
    let name_exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.sys_tenant WHERE tenant_name = $1",
    )
    .bind(tenant_name)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        error!(
            "查询租户名称是否存在失败: tenant_name={}, error={}",
            tenant_name, e
        );
        format!("查询租户失败: {}", e)
    })?;

    if name_exists > 0 {
        warn!("新增租户失败，租户名称已存在: tenant_name={}", tenant_name);
        return Err("租户名称已存在".to_string());
    }

    let id = next_id() as i64;

    // 插入租户记录
    let tenant = sqlx::query_as::<_, SysTenant>(
        r#"
        INSERT INTO public.sys_tenant (id, tenant_name, parent_id, is_leaf, schema_name, enable, create_at)
        VALUES ($1, $2, $3, $4, $5, $6, NOW())
        RETURNING id, tenant_name, parent_id, is_leaf, schema_name, enable, create_at, updated_at
        "#,
    )
    .bind(id)
    .bind(tenant_name)
    .bind(parent_id)
    .bind(is_leaf)
    .bind(schema_name)
    .bind(enable)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        error!(
            "新增租户数据库操作失败: tenant_name={}, error={}",
            tenant_name, e
        );
        format!("新增租户失败: {}", e)
    })?;

    // 仅末级节点才创建 schema 和初始化数据
    if is_leaf {
        let sn = schema_name.unwrap();

        // 创建租户 schema
        info!("正在创建租户 schema '{}'...", sn);
        let create_schema_sql = format!("CREATE SCHEMA IF NOT EXISTS {}", sn);
        sqlx::query(&create_schema_sql)
            .execute(&pool)
            .await
            .map_err(|e| {
                error!("创建 schema '{}' 失败: {}", sn, e);
                format!("创建 schema '{}' 失败: {}", sn, e)
            })?;

        // 初始化租户 schema 表结构
        info!("正在初始化租户 '{}' 表结构...", sn);
        crate::database::postgres::init_tenant_tables(&pool, sn)
            .await
            .map_err(|e| {
                error!("初始化租户 '{}' 表结构失败: {}", sn, e);
                format!("初始化租户表结构失败: {}", e)
            })?;

        // 初始化租户 schema 默认数据
        info!("正在初始化租户 '{}' 默认数据...", sn);
        crate::database::postgres::init_tenant_default_data(&pool, sn)
            .await
            .map_err(|e| {
                error!("初始化租户 '{}' 默认数据失败: {}", sn, e);
                format!("初始化租户默认数据失败: {}", e)
            })?;

        // 创建属于该租户的初始管理员用户（写入 public.sys_user）
        info!("正在为租户 '{}' 创建初始管理员账号...", tenant_name);
        let admin_user_id = next_id() as i64;
        let admin_username = format!("admin_{}", sn);

        // 使用 argon2 加密默认密码
        use argon2::password_hash::SaltString;
        use argon2::{Argon2, PasswordHasher};
        let default_password =
            std::env::var("DEFAULT_ADMIN_PASSWORD").unwrap_or_else(|_| "admin123".to_string());
        let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
        let argon2 = Argon2::default();
        let hash = argon2
            .hash_password(default_password.as_bytes(), &salt)
            .map_err(|e| format!("密码加密失败: {}", e))?
            .to_string();

        sqlx::query(
            r#"
            INSERT INTO public.sys_user (id, username, passwd, real_name, is_super_admin, tenant_id, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, NOW())
            ON CONFLICT (username) DO NOTHING
            "#,
        )
        .bind(admin_user_id)
        .bind(&admin_username)
        .bind(&hash)
        .bind(tenant_name)
        .bind(false) // is_super_admin: 普通租户管理员不是超级管理员
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("为租户 '{}' 创建管理员账号失败: {}", tenant_name, e);
            format!("创建管理员账号失败: {}", e)
        })?;

        // 在 public.sys_user_role 中创建用户角色关联（admin 用户 → admin 角色）
        info!("正在为租户 '{}' 创建用户角色关联...", tenant_name);
        let user_role_id = next_id() as i64;
        sqlx::query(
            r#"
            INSERT INTO public.sys_user_role (id, user_id, role_id, created_by, created_at, deleted)
            SELECT $1, $2, 1, 1, NOW(), 0
            WHERE NOT EXISTS (
                SELECT 1 FROM public.sys_user_role WHERE user_id = $2 AND role_id = 1
            )
            "#,
        )
        .bind(user_role_id)
        .bind(admin_user_id)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("为租户 '{}' 创建用户角色关联失败: {}", tenant_name, e);
            format!("创建用户角色关联失败: {}", e)
        })?;
    }

    info!("新增租户成功: id={}, tenant_name={}", id, tenant_name);
    Ok(tenant.into())
}

/// 更新租户信息
pub async fn update_tenant(
    id: i64,
    tenant_name: &str,
    enable: bool,
) -> Result<TenantResponse, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    info!("更新租户信息: id={}, tenant_name={}", id, tenant_name);

    let tenant = sqlx::query_as::<_, SysTenant>(
        r#"
        UPDATE public.sys_tenant
        SET tenant_name = $2, enable = $3, updated_at = NOW()
        WHERE id = $1
        RETURNING id, tenant_name, parent_id, is_leaf, schema_name, enable, create_at, updated_at
        "#,
    )
    .bind(id)
    .bind(tenant_name)
    .bind(enable)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        error!("更新租户失败: id={}, error={}", id, e);
        format!("更新租户失败: {}", e)
    })?;

    info!("更新租户成功: id={}, tenant_name={}", id, tenant_name);
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

    // 软删除：将租户设为禁用状态
    sqlx::query("UPDATE public.sys_tenant SET enable = false WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("删除租户失败: id={}, error={}", id, e);
            format!("删除租户失败: {}", e)
        })?;

    info!("删除租户成功: id={}", id);
    Ok(())
}

/// 切换租户 schema
///
/// 根据租户ID切换到对应的 schema。
/// 如果 tenant_id = 1（默认租户），切换到 public schema。
pub async fn switch_tenant(tenant_id: i64) -> Result<String, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    info!("切换租户: tenant_id={}", tenant_id);

    if tenant_id == 1 {
        // 默认租户，切换到 public
        crate::database::postgres::set_current_schema("public");
        info!("已切换到 public schema（默认租户）");
        Ok("public".to_string())
    } else {
        // 查询租户的 schema 名称
        let schema_name = sqlx::query_scalar::<_, String>(
            "SELECT schema_name FROM public.sys_tenant WHERE id = $1 AND enable = true",
        )
        .bind(tenant_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| {
            error!("查询租户 schema 失败: tenant_id={}, error={}", tenant_id, e);
            format!("查询租户信息失败: {}", e)
        })?
        .ok_or_else(|| {
            warn!("租户不存在或已禁用: tenant_id={}", tenant_id);
            "租户不存在或已禁用".to_string()
        })?;

        crate::database::postgres::set_current_schema(&schema_name);
        info!("已切换到租户 schema: {}", schema_name);
        Ok(schema_name)
    }
}
