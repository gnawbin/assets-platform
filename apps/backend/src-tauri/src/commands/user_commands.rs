//! 用户管理 Command
//!
//! 对应 lib.rs 中的 login / get_users / insert_user / update_user / delete_user / reset_password

use crate::service;
use crate::service::user_service::{LoginResponse, UserResponse};

/// 用户登录
#[tauri::command]
pub async fn login(username: String, password: String) -> Result<LoginResponse, String> {
    service::user_service::login(&username, &password).await
}

/// 获取用户列表
///
/// 如果 tenant_id 为 Some，则只查询该机构下的用户；
/// 如果为 None（超级管理员），则查询所有机构的用户。
/// keyword 可选，用于按用户名或真实姓名模糊搜索。
#[tauri::command]
pub async fn get_users(
    tenant_id: Option<i64>,
    keyword: Option<String>,
) -> Result<Vec<UserResponse>, String> {
    service::user_service::get_users(tenant_id, keyword).await
}

/// 新增用户
#[tauri::command]
pub async fn insert_user(
    username: String,
    password: String,
    real_name: String,
    email: Option<String>,
    phone: Option<String>,
    department_id: Option<i64>,
    status: i16,
    nickname: Option<String>,
    person_id: Option<String>,
    person_code: Option<String>,
    super_user_id: Option<i64>,
    tenant_id: Option<i64>,
    created_by: Option<i64>,
) -> Result<UserResponse, String> {
    service::user_service::insert_user(
        &username,
        &password,
        &real_name,
        email.as_deref(),
        phone.as_deref(),
        department_id,
        status,
        nickname.as_deref(),
        person_id.as_deref(),
        person_code.as_deref(),
        super_user_id,
        tenant_id,
        created_by,
    )
    .await
}

/// 更新用户信息
#[tauri::command]
pub async fn update_user(
    id: String,
    username: String,
    real_name: String,
    email: Option<String>,
    phone: Option<String>,
    department_id: Option<i64>,
    status: i16,
    nickname: Option<String>,
    person_id: Option<String>,
    person_code: Option<String>,
    super_user_id: Option<i64>,
    updated_by: Option<i64>,
) -> Result<UserResponse, String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的用户ID: {}", e))?;
    service::user_service::update_user(
        id,
        &username,
        &real_name,
        email.as_deref(),
        phone.as_deref(),
        department_id,
        status,
        nickname.as_deref(),
        person_id.as_deref(),
        person_code.as_deref(),
        super_user_id,
        updated_by,
    )
    .await
}

/// 删除用户（软删除）
///
/// 权限校验：
/// - 超级管理员不能被任何人删除（包括超级管理员自己）
/// - 非超级管理员只能删除自己所在机构的用户
#[tauri::command]
pub async fn delete_user(
    id: String,
    current_user_id: i64,
    is_super_admin: bool,
) -> Result<(), String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的用户ID: {}", e))?;
    service::user_service::delete_user(id, current_user_id, is_super_admin).await
}

/// 获取当前登录用户信息（从 JWT token 解析用户ID）
#[tauri::command]
pub async fn get_current_user(token: String) -> Result<UserResponse, String> {
    use jsonwebtoken::{decode, DecodingKey, Validation};

    let jwt_secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "assets-platform-default-secret-key".to_string());

    let token_data = decode::<service::user_service::Claims>(
        &token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|e| format!("Token 验证失败: {}", e))?;

    let user_id = token_data.claims.sub;
    service::user_service::get_user_by_id(user_id).await
}

/// 重置密码
#[tauri::command]
pub async fn reset_password(id: String, new_password: String) -> Result<(), String> {
    let id: i64 = id.parse().map_err(|e| format!("无效的用户ID: {}", e))?;
    service::user_service::reset_password(id, &new_password).await
}
