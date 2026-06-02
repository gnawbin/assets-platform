mod database;
mod service;
mod utils;
use database::models::{AssetCategory, Department, MantineTree, Role};
use service::user_service::{LoginResponse, UserResponse};

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// 获取所有资产类别列表
#[tauri::command]
async fn get_categories() -> Result<Vec<AssetCategory>, String> {
    service::assets_categories_service::get_categories().await
}
/// 获取所有资产类别列表
#[tauri::command]
async fn get_categories_parents() -> Result<Vec<AssetCategory>, String> {
    service::assets_categories_service::get_super_categories().await
}
/// 新增资产类别
#[tauri::command]
async fn insert_category(category: AssetCategory) -> Result<AssetCategory, String> {
    service::assets_categories_service::insert_category(&category).await
}

// ======================== 角色权限管理 ========================

/// 新增角色
#[tauri::command]
async fn insert_role(role: Role) -> Result<Role, String> {
    service::role_service::insert_role(&role).await
}

/// 获取所有角色列表
#[tauri::command]
async fn get_roles() -> Result<Vec<Role>, String> {
    service::role_service::get_roles().await
}

/// 获取指定角色已分配的菜单权限ID列表
#[tauri::command]
async fn get_role_menu_ids(role_id: i64) -> Result<Vec<i64>, String> {
    service::role_service::get_role_menu_ids(role_id).await
}

/// 为角色分配菜单权限
#[tauri::command]
async fn assign_role_menus(role_id: i64, menu_ids: Vec<i64>) -> Result<(), String> {
    service::role_service::assign_role_menus(role_id, menu_ids).await
}

/// 删除角色
#[tauri::command]
async fn delete_role(role_id: i64) -> Result<(), String> {
    service::role_service::delete_role(role_id).await
}

/// 获取所有菜单树（用于权限分配）
#[tauri::command]
async fn get_all_menus_tree() -> Result<Vec<MantineTree>, String> {
    service::role_service::get_all_menus_tree().await
}

// ======================== 用户登录 ========================

/// 用户登录
#[tauri::command]
async fn login(username: String, password: String) -> Result<LoginResponse, String> {
    service::user_service::login(&username, &password).await
}

// ======================== 用户管理 ========================

/// 获取所有用户列表
#[tauri::command]
async fn get_users() -> Result<Vec<UserResponse>, String> {
    service::user_service::get_users().await
}

/// 新增用户
#[tauri::command]
async fn insert_user(
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
        created_by,
    )
    .await
}

/// 更新用户信息
#[tauri::command]
async fn update_user(
    id: i64,
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
#[tauri::command]
async fn delete_user(id: i64) -> Result<(), String> {
    service::user_service::delete_user(id).await
}

/// 重置密码
#[tauri::command]
async fn reset_password(id: i64, new_password: String) -> Result<(), String> {
    service::user_service::reset_password(id, &new_password).await
}

// ======================== 部门管理 ========================

/// 获取所有部门列表
#[tauri::command]
async fn get_departments() -> Result<Vec<Department>, String> {
    service::department_service::get_departments().await
}

/// 新增部门
#[tauri::command]
async fn insert_department(
    department_name: String,
    parent_id: Option<i64>,
    description: Option<String>,
    created_by: Option<i64>,
) -> Result<Department, String> {
    service::department_service::insert_department(
        &department_name,
        parent_id,
        description.as_deref(),
        created_by,
    )
    .await
}

/// 更新部门
#[tauri::command]
async fn update_department(
    id: i64,
    department_name: String,
    parent_id: Option<i64>,
    description: Option<String>,
    updated_by: Option<i64>,
) -> Result<Department, String> {
    service::department_service::update_department(
        id,
        &department_name,
        parent_id,
        description.as_deref(),
        updated_by,
    )
    .await
}

/// 删除部门（软删除）
#[tauri::command]
async fn delete_department(id: i64) -> Result<(), String> {
    service::department_service::delete_department(id).await
}

/// 加载 .env 环境变量文件
fn load_env() {
    // 尝试从当前工作目录加载 .env 文件
    match dotenvy::dotenv() {
        Ok(_) => println!("已加载 .env 环境变量文件"),
        Err(e) => {
            // 如果 .env 文件不存在，尝试从 src-tauri 目录加载
            if let Err(e2) = dotenvy::from_filename("src-tauri/.env") {
                println!("未找到 .env 文件，将使用默认环境变量: {} / {}", e, e2);
            } else {
                println!("已从 src-tauri/.env 加载环境变量");
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 应用启动时加载 .env 环境变量
    load_env();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|_app| {
            // 应用启动时自动初始化数据库
            tauri::async_runtime::block_on(async {
                database::init_database().await.expect("数据库初始化失败");
                println!("数据库初始化完成");
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            get_categories,
            get_categories_parents,
            insert_category,
            insert_role,
            get_roles,
            get_role_menu_ids,
            assign_role_menus,
            delete_role,
            get_all_menus_tree,
            login,
            get_users,
            insert_user,
            update_user,
            delete_user,
            reset_password,
            get_departments,
            insert_department,
            update_department,
            delete_department,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("error while running tauri application: {}", e);
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use database::models::AssetCategory;

    #[test]
    fn test_greet() {
        let name = "Alice";
        let greeting = greet(name);
        assert_eq!(greeting, "Hello, Alice! You've been greeted from Rust!");
    }
}
