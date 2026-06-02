use crate::database::get_pool;
use crate::database::models::SysUser;
use crate::utils::password_secret::verify_password;
use serde::Serialize;

/// 登录响应（不包含密码）
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
}

/// 用户登录
pub async fn login(username: &str, password: &str) -> Result<LoginResponse, String> {
    let pool = get_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    // 查询用户
    let user = sqlx::query_as::<_, SysUser>(
        "SELECT id, username, passwd, domain, real_name, email, phone, department_id, status, nickname, avatar, person_id, person_code, super_user_id, created_by, created_at, updated_by, updated_at, deleted FROM sys_user WHERE username = $1 AND (deleted IS NULL OR deleted = 0)"
    )
    .bind(username)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("查询用户失败: {}", e))?
    .ok_or_else(|| "用户名或密码错误".to_string())?;

    // 检查用户状态
    if user.status == 0 {
        return Err("该用户已被禁用".to_string());
    }

    // 验证密码
    let valid =
        verify_password(password, &user.passwd).map_err(|e| format!("密码验证失败: {}", e))?;

    if !valid {
        return Err("用户名或密码错误".to_string());
    }

    // 返回用户信息（不含密码）
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
    })
}
