mod database;
mod service;
mod utils;
use database::models::AssetCategory;

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
            insert_category
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
