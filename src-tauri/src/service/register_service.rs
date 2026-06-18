//! 用户注册申请 Service
//!
//! 提供用户注册申请、审核、驳回等功能。

use crate::database::models::SysUserRegister;
use crate::database::{get_read_pool, get_write_pool};
use crate::utils::password_secret::hash_password;
use crate::utils::snowflake::next_id;
use serde::Serialize;
use tracing::{error, info, warn};
use utoipa::ToSchema;

/// 注册申请响应
#[derive(Debug, Serialize, ToSchema)]
pub struct RegisterResponse {
    #[serde(serialize_with = "crate::database::models::i64_to_string")]
    pub id: i64,
    pub username: String,
    pub real_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub department_name: Option<String>,
    pub company_name: Option<String>,
    pub reason: Option<String>,
    pub status: i16,
    #[serde(serialize_with = "crate::database::models::opt_i64_to_string")]
    pub approve_by: Option<i64>,
    pub approve_time: Option<String>,
    pub approve_remark: Option<String>,
    pub created_at: Option<String>,
}

impl From<SysUserRegister> for RegisterResponse {
    fn from(r: SysUserRegister) -> Self {
        Self {
            id: r.id,
            username: r.username,
            real_name: r.real_name,
            email: r.email,
            phone: r.phone,
            department_name: r.department_name,
            company_name: r.company_name,
            reason: r.reason,
            status: r.status,
            approve_by: r.approve_by,
            approve_time: r.approve_time.map(|t| t.to_string()),
            approve_remark: r.approve_remark,
            created_at: r.created_at.map(|t| t.to_string()),
        }
    }
}

/// 用户注册申请
pub async fn register(
    username: &str,
    password: &str,
    real_name: &str,
    email: Option<&str>,
    phone: Option<&str>,
    department_name: Option<&str>,
    company_name: Option<&str>,
    reason: Option<&str>,
) -> Result<RegisterResponse, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    info!(
        "用户注册申请: username={}, real_name={}",
        username, real_name
    );

    // 检查用户名是否已存在（sys_user 表）
    let existing_user = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.sys_user WHERE username = $1 AND (deleted IS NULL OR deleted = 0)"
    )
    .bind(username)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        error!("查询用户是否存在失败: {}", e);
        format!("查询用户失败: {}", e)
    })?;

    if existing_user > 0 {
        warn!("注册失败，用户名已存在: username={}", username);
        return Err("用户名已存在".to_string());
    }

    // 检查注册表中是否已有待审核的申请
    let existing_register = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.sys_user_register WHERE username = $1 AND status = 0",
    )
    .bind(username)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        error!("查询注册申请失败: {}", e);
        format!("查询注册申请失败: {}", e)
    })?;

    if existing_register > 0 {
        warn!("注册失败，已有待审核的注册申请: username={}", username);
        return Err("该用户名已有待审核的注册申请".to_string());
    }

    // 加密密码
    let hashed_password = hash_password(password)?;

    let register = sqlx::query_as::<_, SysUserRegister>(
        r#"
        INSERT INTO public.sys_user_register (id, username, passwd, real_name, email, phone, department_name, company_name, reason, status, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 0, NOW())
        RETURNING id, username, passwd, real_name, email, phone, department_name, company_name, reason, status, approve_by, approve_time, approve_remark, created_at, updated_at
        "#
    )
    .bind(next_id() as i64)
    .bind(username)
    .bind(&hashed_password)
    .bind(real_name)
    .bind(email)
    .bind(phone)
    .bind(department_name)
    .bind(company_name)
    .bind(reason)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        error!("注册申请数据库操作失败: username={}, error={}", username, e);
        format!("注册申请失败: {}", e)
    })?;

    info!(
        "注册申请成功: id={}, username={}",
        register.id, register.username
    );
    Ok(register.into())
}

/// 获取注册申请列表
pub async fn get_registrations(status: Option<i16>) -> Result<Vec<RegisterResponse>, String> {
    let pool = get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    let registrations = if let Some(s) = status {
        sqlx::query_as::<_, SysUserRegister>(
            "SELECT id, username, passwd, real_name, email, phone, department_name, company_name, reason, status, approve_by, approve_time, approve_remark, created_at, updated_at FROM public.sys_user_register WHERE status = $1 ORDER BY created_at DESC"
        )
        .bind(s)
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询注册申请列表失败: {}", e);
            format!("查询注册申请列表失败: {}", e)
        })?
    } else {
        sqlx::query_as::<_, SysUserRegister>(
            "SELECT id, username, passwd, real_name, email, phone, department_name, company_name, reason, status, approve_by, approve_time, approve_remark, created_at, updated_at FROM public.sys_user_register ORDER BY created_at DESC"
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询注册申请列表失败: {}", e);
            format!("查询注册申请列表失败: {}", e)
        })?
    };

    let count = registrations.len();
    info!("查询注册申请列表成功: 共 {} 条记录", count);
    Ok(registrations.into_iter().map(|r| r.into()).collect())
}

/// 审核通过注册申请
///
/// 1. 更新注册申请状态为已通过
/// 2. 在 sys_user 表中创建用户
/// 3. 返回新创建的用户信息
pub async fn approve_registration(
    id: i64,
    approve_by: i64,
    tenant_id: i64,
    approve_remark: Option<&str>,
) -> Result<RegisterResponse, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    info!("审核注册申请: id={}, approve_by={}", id, approve_by);

    // 查询注册申请
    let register = sqlx::query_as::<_, SysUserRegister>(
        "SELECT id, username, passwd, real_name, email, phone, department_name, company_name, reason, status, approve_by, approve_time, approve_remark, created_at, updated_at FROM public.sys_user_register WHERE id = $1 AND status = 0"
    )
    .bind(id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        error!("查询注册申请失败: id={}, error={}", id, e);
        format!("查询注册申请失败: {}", e)
    })?
    .ok_or_else(|| {
        warn!("注册申请不存在或已处理: id={}", id);
        "注册申请不存在或已处理".to_string()
    })?;

    // 在 sys_user 表中创建用户
    let new_user_id = next_id() as i64;
    sqlx::query(
        r#"
        INSERT INTO public.sys_user (id, username, passwd, domain, real_name, email, phone, department_id, is_super_admin, status, nickname, avatar, person_id, person_code, super_user_id, tenant_id, created_by, created_at, updated_by, updated_at, deleted)
        VALUES ($1, $2, $3, NULL, $4, $5, $6, NULL, false, 1, NULL, NULL, NULL, NULL, NULL, $7, $8, NOW(), $8, NOW(), 0)
        "#
    )
    .bind(new_user_id)
    .bind(&register.username)
    .bind(&register.passwd)
    .bind(&register.real_name)
    .bind(&register.email)
    .bind(&register.phone)
    .bind(tenant_id)
    .bind(approve_by)
    .execute(&pool)
    .await
    .map_err(|e| {
        error!("审核通过-创建用户失败: username={}, error={}", register.username, e);
        format!("创建用户失败: {}", e)
    })?;

    // 更新注册申请状态
    let updated = sqlx::query_as::<_, SysUserRegister>(
        r#"
        UPDATE public.sys_user_register
        SET status = 1, approve_by = $2, approve_time = NOW(), approve_remark = $3, updated_at = NOW()
        WHERE id = $1
        RETURNING id, username, passwd, real_name, email, phone, department_name, company_name, reason, status, approve_by, approve_time, approve_remark, created_at, updated_at
        "#
    )
    .bind(id)
    .bind(approve_by)
    .bind(approve_remark)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        error!("审核通过-更新申请状态失败: id={}, error={}", id, e);
        format!("更新申请状态失败: {}", e)
    })?;

    info!(
        "审核通过注册申请成功: id={}, username={}, new_user_id={}",
        id, register.username, new_user_id
    );
    Ok(updated.into())
}

/// 驳回注册申请
pub async fn reject_registration(
    id: i64,
    approve_by: i64,
    approve_remark: Option<&str>,
) -> Result<RegisterResponse, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    info!("驳回注册申请: id={}, approve_by={}", id, approve_by);

    let updated = sqlx::query_as::<_, SysUserRegister>(
        r#"
        UPDATE public.sys_user_register
        SET status = 2, approve_by = $2, approve_time = NOW(), approve_remark = $3, updated_at = NOW()
        WHERE id = $1 AND status = 0
        RETURNING id, username, passwd, real_name, email, phone, department_name, company_name, reason, status, approve_by, approve_time, approve_remark, created_at, updated_at
        "#
    )
    .bind(id)
    .bind(approve_by)
    .bind(approve_remark)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        error!("驳回注册申请失败: id={}, error={}", id, e);
        format!("驳回注册申请失败: {}", e)
    })?
    .ok_or_else(|| {
        warn!("注册申请不存在或已处理: id={}", id);
        "注册申请不存在或已处理".to_string()
    })?;

    info!("驳回注册申请成功: id={}, username={}", id, updated.username);
    Ok(updated.into())
}
