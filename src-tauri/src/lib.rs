mod api;
mod commands;
mod database;
mod service;
mod utils;

/// 加载 .env 环境变量文件
fn load_env() {
    // 尝试从当前工作目录加载 .env 文件
    match dotenvy::dotenv() {
        Ok(_) => tracing::info!("已加载 .env 环境变量文件"),
        Err(e) => {
            // 如果 .env 文件不存在，尝试从 src-tauri 目录加载
            if let Err(e2) = dotenvy::from_filename("src-tauri/.env") {
                tracing::warn!("未找到 .env 文件，将使用默认环境变量: {} / {}", e, e2);
            } else {
                tracing::info!("已从 src-tauri/.env 加载环境变量");
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 应用启动时加载 .env 环境变量
    load_env();

    // 初始化 tracing 日志系统
    if let Err(e) = utils::logging::init_tracing() {
        eprintln!("日志系统初始化失败: {}", e);
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|_app| {
            // 初始化 OpenTelemetry（需要 Tokio 运行时上下文）
            tauri::async_runtime::block_on(async {
                if let Err(e) = utils::logging::init_otel() {
                    tracing::warn!("OpenTelemetry 初始化失败: {}", e);
                }
            });

            // 应用启动时自动初始化数据库
            tauri::async_runtime::block_on(async {
                database::init_database().await.expect("数据库初始化失败");
                tracing::info!("数据库初始化完成");
            });

            // 在后台启动 HTTP API 服务（与 Tauri 共用 Tokio 运行时）
            let pool = database::get_pool().expect("获取数据库连接池失败");
            tauri::async_runtime::spawn(async move {
                api::start_http_server(pool).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // 资产类别
            commands::category_commands::get_categories,
            commands::category_commands::get_categories_parents,
            commands::category_commands::insert_category,
            commands::category_commands::update_category,
            commands::category_commands::delete_category,
            // 角色权限
            commands::role_commands::insert_role,
            commands::role_commands::get_roles,
            commands::role_commands::get_role_menu_ids,
            commands::role_commands::assign_role_menus,
            commands::role_commands::delete_role,
            commands::role_commands::get_all_menus_tree,
            // 用户管理
            commands::user_commands::login,
            commands::user_commands::get_users,
            commands::user_commands::insert_user,
            commands::user_commands::update_user,
            commands::user_commands::delete_user,
            commands::user_commands::reset_password,
            // 部门管理
            commands::department_commands::get_departments,
            commands::department_commands::insert_department,
            commands::department_commands::update_department,
            commands::department_commands::delete_department,
            // 固定资产
            commands::asset_commands::get_hardware_assets,
            commands::asset_commands::insert_hardware_asset,
            commands::asset_commands::update_hardware_asset,
            commands::asset_commands::delete_hardware_asset,
            // 无形资产
            commands::asset_commands::get_intangible_assets,
            commands::asset_commands::insert_intangible_asset,
            commands::asset_commands::update_intangible_asset,
            commands::asset_commands::delete_intangible_asset,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            tracing::error!("Tauri 应用运行出错: {}", e);
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
        let greeting = format!("Hello, {}! You've been greeted from Rust!", name);
        assert_eq!(greeting, "Hello, Alice! You've been greeted from Rust!");
    }
}
