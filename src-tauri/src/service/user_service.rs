use crate::database::models::SysUser;
use crate::database::{get_read_pool, get_write_pool};
use crate::utils::password_secret::{hash_password, verify_password};
use crate::utils::snowflake::next_id;
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use utoipa::ToSchema;

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
}

/// 登录响应（包含 JWT Token）
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    #[serde(serialize_with = "crate::database::models::i64_to_string")]
    pub id: i64,
    pub username: String,
    pub real_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    #[serde(serialize_with = "crate::database::models::opt_i64_to_string")]
    pub department_id: Option<i64>,
    pub is_super_admin: bool,
    pub status: i16,
    pub nickname: Option<String>,
    pub avatar: Option<String>,
    #[serde(serialize_with = "crate::database::models::opt_i64_to_string")]
    pub tenant_id: Option<i64>,
    /// JWT Token，用于后续请求的身份验证
    pub token: String,
}

/// 用户列表响应（不包含密码）
#[derive(Debug, Serialize, ToSchema)]
pub struct UserResponse {
    #[serde(serialize_with = "crate::database::models::i64_to_string")]
    pub id: i64,
    pub username: String,
    pub real_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    #[serde(serialize_with = "crate::database::models::opt_i64_to_string")]
    pub department_id: Option<i64>,
    pub is_super_admin: bool,
    pub status: i16,
    pub nickname: Option<String>,
    pub avatar: Option<String>,
    pub person_id: Option<String>,
    pub person_code: Option<String>,
    #[serde(serialize_with = "crate::database::models::opt_i64_to_string")]
    pub super_user_id: Option<i64>,
    #[serde(serialize_with = "crate::database::models::opt_i64_to_string")]
    pub tenant_id: Option<i64>,
    /// 机构名称
    pub tenant_name: Option<String>,
    #[serde(serialize_with = "crate::database::models::opt_i64_to_string")]
    pub created_by: Option<i64>,
    pub created_at: Option<String>,
    #[serde(serialize_with = "crate::database::models::opt_i64_to_string")]
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

    // 登录成功后，根据用户所属租户自动切换 schema
    // 如果 tenant_id = 1（默认租户），保持 public schema
    // 否则切换到对应租户的 schema
    if let Some(tenant_id) = user.tenant_id {
        if tenant_id != 1 {
            let schema: Option<String> = sqlx::query_scalar(
                "SELECT schema_name FROM public.sys_tenant WHERE id = $1 AND enable = true",
            )
            .bind(tenant_id)
            .fetch_optional(&pool)
            .await
            .map_err(|e| {
                error!("查询租户 schema 失败: tenant_id={}, error={}", tenant_id, e);
                format!("查询租户信息失败: {}", e)
            })?;

            if let Some(schema_name) = schema {
                crate::database::postgres::set_current_schema(&schema_name);
                info!("用户 '{}' 已切换到租户 schema: {}", username, schema_name);
            }
        } else {
            // 默认租户（id=1），切换到 public
            crate::database::postgres::set_current_schema("public");
            info!("用户 '{}' 使用 public schema（默认租户）", username);
        }
    } else {
        // 没有 tenant_id，使用 public
        crate::database::postgres::set_current_schema("public");
    }

    info!(
        "用户登录成功: username={}, id={}, real_name={}",
        username, user.id, user.real_name
    );

    // 生成 JWT Token
    let jwt_secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "assets-platform-default-secret-key".to_string());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("获取时间戳失败: {}", e))?
        .as_secs() as usize;
    let exp = now + 86400 * 7; // 7天过期

    let claims = Claims {
        sub: user.id,
        username: user.username.clone(),
        exp,
        iat: now,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes()),
    )
    .map_err(|e| format!("生成Token失败: {}", e))?;

    // 返回用户信息（不含密码）+ JWT Token
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
        token,
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

    info!("重置用户密码成功: id={}", id);
    Ok(())
}
