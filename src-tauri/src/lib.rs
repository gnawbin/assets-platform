mod database;

use database::dual_database::DualDatabaseManager;
use database::models::AssetCategory;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// 获取所有资产类别列表
#[tauri::command]
async fn get_categories() -> Result<Vec<AssetCategory>, String> {
    let pool = DualDatabaseManager::public_pool();
    let categories = sqlx::query_as::<_, AssetCategory>(
        "SELECT id, category_name, asset_type, parent_id, sort, description, created_by, created_at, updated_by, updated_at FROM asset_categories ORDER BY sort ASC"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("查询资产类别失败: {}", e))?;

    Ok(categories)
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
            // 应用启动时自动初始化公开数据库
            tauri::async_runtime::block_on(async {
                database::init_public_database()
                    .await
                    .expect("公开数据库初始化失败");
                println!("公开数据库初始化完成");
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![greet, get_categories])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
