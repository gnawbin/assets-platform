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
    pub id: i64,
    pub username: String,
    pub real_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub department_id: Option<i64>,
    pub status: i16,
    pub nickname: Option<String>,
    pub avatar: Option<String>,
    /// JWT Token，用于后续请求的身份验证
    pub token: String,
}

/// 用户列表响应（不包含密码）
#[derive(Debug, Serialize, ToSchema)]
pub struct UserResponse {
    pub id: i64,
    pub username: String,
    pub real_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub department_id: Option<i64>,
    pub status: i16,
    pub nickname: Option<String>,
    pub avatar: Option<String>,
    pub person_id: Option<String>,
    pub person_code: Option<String>,
    pub super_user_id: Option<i64>,
    pub created_by: Option<i64>,
    pub created_at: Option<String>,
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
            status: u.status,
            nickname: u.nickname,
            avatar: u.avatar,
            person_id: u.person_id,
            person_code: u.person_code,
            super_user_id: u.super_user_id,
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

    // 查询用户
    let user = sqlx::query_as::<_, SysUser>(
        "SELECT id, username, passwd, domain, real_name, email, phone, department_id, status, nickname, avatar, person_id, person_code, super_user_id, created_by, created_at, updated_by, updated_at, deleted FROM sys_user WHERE username = $1 AND (deleted IS NULL OR deleted = 0)"
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
        status: user.status,
        nickname: user.nickname,
        avatar: user.avatar,
        token,
    })
}

/// 获取所有用户列表
pub async fn get_users() -> Result<Vec<UserResponse>, String> {
    let pool = get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
    let users = sqlx::query_as::<_, SysUser>(
        "SELECT id, username, passwd, domain, real_name, email, phone, department_id, status, nickname, avatar, person_id, person_code, super_user_id, created_by, created_at, updated_by, updated_at, deleted FROM sys_user WHERE deleted IS NULL OR deleted = 0 ORDER BY id ASC"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        error!("查询用户列表失败: {}", e);
        format!("查询用户列表失败: {}", e)
    })?;

    let count = users.len();
    info!("查询用户列表成功: 共 {} 条记录", count);
    Ok(users.into_iter().map(|u| u.into()).collect())
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
    created_by: Option<i64>,
) -> Result<UserResponse, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    info!("新增用户: username={}, real_name={}", username, real_name);

    // 检查用户名是否已存在
    let existing = sqlx::query_as::<_, SysUser>(
        "SELECT id, username, passwd, domain, real_name, email, phone, department_id, status, nickname, avatar, person_id, person_code, super_user_id, created_by, created_at, updated_by, updated_at, deleted FROM sys_user WHERE username = $1 AND (deleted IS NULL OR deleted = 0)"
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

    let user = sqlx::query_as::<_, SysUser>(
        r#"
        INSERT INTO sys_user (id, username, passwd, domain, real_name, email, phone, department_id, status, nickname, avatar, person_id, person_code, super_user_id, created_by, created_at, updated_by, updated_at, deleted)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, NOW(), $16, NOW(), 0)
        RETURNING id, username, passwd, domain, real_name, email, phone, department_id, status, nickname, avatar, person_id, person_code, super_user_id, created_by, created_at, updated_by, updated_at, deleted
        "#
    )
    .bind(next_id() as i64)
    .bind(username)
    .bind(&hashed_password)
    .bind(Option::<String>::None) // domain
    .bind(real_name)
    .bind(email)
    .bind(phone)
    .bind(department_id)
    .bind(status)
    .bind(nickname)
    .bind(Option::<String>::None) // avatar
    .bind(person_id)
    .bind(person_code)
    .bind(super_user_id)
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

    let user = sqlx::query_as::<_, SysUser>(
        r#"
        UPDATE sys_user
        SET username = $2, real_name = $3, email = $4, phone = $5, department_id = $6, status = $7, nickname = $8, person_id = $9, person_code = $10, super_user_id = $11, updated_by = $12, updated_at = NOW()
        WHERE id = $1 AND (deleted IS NULL OR deleted = 0)
        RETURNING id, username, passwd, domain, real_name, email, phone, department_id, status, nickname, avatar, person_id, person_code, super_user_id, created_by, created_at, updated_by, updated_at, deleted
        "#
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
pub async fn delete_user(id: i64) -> Result<(), String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    info!("删除用户: id={}", id);

    sqlx::query("UPDATE sys_user SET deleted = 1, updated_at = NOW() WHERE id = $1")
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

    let user = sqlx::query_as::<_, SysUser>(
        "SELECT id, username, passwd, domain, real_name, email, phone, department_id, status, nickname, avatar, person_id, person_code, super_user_id, created_by, created_at, updated_by, updated_at, deleted FROM sys_user WHERE id = $1 AND (deleted IS NULL OR deleted = 0)"
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

    sqlx::query("UPDATE sys_user SET passwd = $2, updated_at = NOW() WHERE id = $1")
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
