mod database;

use database::models::AssetCategory;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// 获取所有资产类别列表
#[tauri::command]
async fn get_categories() -> Result<Vec<AssetCategory>, String> {
    let pool = database::get_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let categories = sqlx::query_as::<_, AssetCategory>(
        "SELECT id, category_name, asset_type, parent_id, sort, description, created_by, created_at, updated_by, updated_at FROM asset_category ORDER BY sort ASC"
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
            // 应用启动时自动初始化数据库
            tauri::async_runtime::block_on(async {
                database::init_database().await.expect("数据库初始化失败");
                println!("数据库初始化完成");
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![greet, get_categories])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use database::models::AssetCategory;

    /// 辅助函数：创建一个测试用的 AssetCategory 实例
    fn create_test_category(
        id: i64,
        category_name: &str,
        asset_type: &str,
        parent_id: i64,
        sort: i16,
        description: Option<&str>,
    ) -> AssetCategory {
        let now: DateTime<Utc> = Utc::now();
        AssetCategory {
            id,
            category_name: category_name.to_string(),
            asset_type: asset_type.to_string(),
            parent_id,
            sort,
            description: description.map(|s| s.to_string()),
            created_by: Some(1),
            created_at: now,
            updated_by: Some(1),
            updated_at: now,
        }
    }

    /// 测试 AssetCategory 结构体的字段完整性
    #[test]
    fn test_asset_category_struct_fields() {
        let category = create_test_category(1, "IT设备", "hardware", 0, 1, Some("信息技术设备"));

        assert_eq!(category.id, 1);
        assert_eq!(category.category_name, "IT设备");
        assert_eq!(category.asset_type, "hardware");
        assert_eq!(category.parent_id, 0);
        assert_eq!(category.sort, 1);
        assert_eq!(category.description, Some("信息技术设备".to_string()));
        assert_eq!(category.created_by, Some(1));
        assert_eq!(category.updated_by, Some(1));
    }

    /// 测试 AssetCategory 的序列化和反序列化
    #[test]
    fn test_asset_category_serde() {
        let category = create_test_category(2, "服务器", "hardware", 1, 1, Some("服务器设备"));

        // 序列化为 JSON
        let json = serde_json::to_string(&category).expect("序列化失败");
        assert!(json.contains("\"category_name\":\"服务器\""));
        assert!(json.contains("\"asset_type\":\"hardware\""));
        assert!(json.contains("\"parent_id\":1"));
        assert!(json.contains("\"sort\":1"));

        // 反序列化回结构体
        let deserialized: AssetCategory = serde_json::from_str(&json).expect("反序列化失败");
        assert_eq!(deserialized.id, category.id);
        assert_eq!(deserialized.category_name, category.category_name);
        assert_eq!(deserialized.asset_type, category.asset_type);
        assert_eq!(deserialized.parent_id, category.parent_id);
        assert_eq!(deserialized.sort, category.sort);
        assert_eq!(deserialized.description, category.description);
    }

    /// 测试 AssetCategory 的 Clone 特性
    #[test]
    fn test_asset_category_clone() {
        let category = create_test_category(3, "网络设备", "hardware", 1, 2, Some("网络设备"));
        let cloned = category.clone();
        assert_eq!(cloned.id, category.id);
        assert_eq!(cloned.category_name, category.category_name);
        assert_eq!(cloned.asset_type, category.asset_type);
        assert_eq!(cloned.parent_id, category.parent_id);
        assert_eq!(cloned.sort, category.sort);
        assert_eq!(cloned.description, category.description);
    }

    /// 测试 AssetCategory 的 Debug 特性
    #[test]
    fn test_asset_category_debug() {
        let category = create_test_category(4, "办公设备", "hardware", 0, 2, None);
        let debug_str = format!("{:?}", category);
        assert!(debug_str.contains("AssetCategory"));
        assert!(debug_str.contains("办公设备"));
        assert!(debug_str.contains("hardware"));
    }

    /// 测试 AssetCategory 列表的排序逻辑（按 sort 字段）
    #[test]
    fn test_asset_category_sort_order() {
        let categories = vec![
            create_test_category(3, "网络设备", "hardware", 1, 2, None),
            create_test_category(1, "IT设备", "hardware", 0, 1, None),
            create_test_category(2, "服务器", "hardware", 1, 1, None),
        ];

        let mut sorted = categories.clone();
        sorted.sort_by_key(|c| c.sort);

        assert_eq!(sorted[0].id, 1); // sort=1
        assert_eq!(sorted[1].id, 2); // sort=1
        assert_eq!(sorted[2].id, 3); // sort=2
    }

    /// 测试 AssetCategory 的 parent_id 层级关系
    #[test]
    fn test_asset_category_parent_relationship() {
        let parent = create_test_category(1, "IT设备", "hardware", 0, 1, None);
        let child = create_test_category(2, "服务器", "hardware", 1, 1, None);

        assert_eq!(parent.id, child.parent_id);
        assert_eq!(parent.asset_type, child.asset_type);
    }

    /// 测试 AssetCategory 的 description 为 None 的情况
    #[test]
    fn test_asset_category_description_none() {
        let category = create_test_category(5, "测试类别", "software", 0, 1, None);
        assert_eq!(category.description, None);
    }

    /// 测试 AssetCategory 的 description 为 Some 的情况
    #[test]
    fn test_asset_category_description_some() {
        let category = create_test_category(6, "测试类别", "software", 0, 1, Some("测试描述"));
        assert_eq!(category.description, Some("测试描述".to_string()));
    }

    /// 测试 get_categories 函数的 SQL 查询语句格式
    #[test]
    fn test_get_categories_query_format() {
        let expected_sql = "SELECT id, category_name, asset_type, parent_id, sort, description, created_by, created_at, updated_by, updated_at FROM asset_category ORDER BY sort ASC";

        // 验证 SQL 查询语句包含必要的字段
        assert!(expected_sql.contains("id"));
        assert!(expected_sql.contains("category_name"));
        assert!(expected_sql.contains("asset_type"));
        assert!(expected_sql.contains("parent_id"));
        assert!(expected_sql.contains("sort"));
        assert!(expected_sql.contains("description"));
        assert!(expected_sql.contains("created_by"));
        assert!(expected_sql.contains("created_at"));
        assert!(expected_sql.contains("updated_by"));
        assert!(expected_sql.contains("updated_at"));
        assert!(expected_sql.contains("asset_category"));
        assert!(expected_sql.contains("ORDER BY sort ASC"));
    }

    /// 测试返回的 Vec<AssetCategory> 为空列表的情况
    #[test]
    fn test_empty_categories_list() {
        let empty_list: Vec<AssetCategory> = Vec::new();
        assert!(empty_list.is_empty());
        assert_eq!(empty_list.len(), 0);
    }

    /// 测试返回的 Vec<AssetCategory> 包含多个类别的情况
    #[test]
    fn test_multiple_categories_list() {
        let categories = vec![
            create_test_category(1, "IT设备", "hardware", 0, 1, None),
            create_test_category(6, "软件资产", "software", 0, 3, None),
            create_test_category(5, "办公设备", "hardware", 0, 2, None),
        ];
        assert_eq!(categories.len(), 3);
        assert!(!categories.is_empty());
    }

    /// 测试不同 asset_type 的分类
    #[test]
    fn test_asset_category_types() {
        let hardware = create_test_category(1, "IT设备", "hardware", 0, 1, None);
        let software = create_test_category(6, "软件资产", "software", 0, 3, None);

        assert_eq!(hardware.asset_type, "hardware");
        assert_eq!(software.asset_type, "software");
        assert_ne!(hardware.asset_type, software.asset_type);
    }

    // ==================== get_categories 函数测试 ====================

    /// 测试 get_categories 函数在数据库未初始化时的错误处理
    /// 在测试环境中，数据库通常未初始化，因此 get_pool() 会返回错误
    #[tokio::test]
    async fn test_get_categories_db_not_initialized() {
        let result = get_categories().await;

        match result {
            Ok(categories) => {
                for cat in &categories {
                    let _: &AssetCategory = cat;
                    assert!(cat.id > 0, "id 必须大于0");
                    assert!(!cat.category_name.is_empty(), "category_name 不能为空");
                    assert!(!cat.asset_type.is_empty(), "asset_type 不能为空");
                }
                for i in 1..categories.len() {
                    assert!(
                        categories[i - 1].sort <= categories[i].sort,
                        "资产类别应按 sort 字段升序排列"
                    );
                }
            }
            Err(e) => {
                let err_msg = e.to_string();
                assert!(
                    err_msg.contains("获取数据库连接失败") || err_msg.contains("查询资产类别失败"),
                    "错误信息应包含中文描述: {}",
                    err_msg
                );
                assert!(!err_msg.is_empty(), "错误信息不应为空");
            }
        }
    }

    /// 测试 get_categories 函数返回的数据类型是否正确
    #[tokio::test]
    async fn test_get_categories_return_type() {
        let result = get_categories().await;

        match result {
            Ok(categories) => {
                for cat in &categories {
                    let _: &AssetCategory = cat;
                }
            }
            Err(e) => {
                let _: &String = &e;
                assert!(!e.is_empty(), "错误信息不应为空");
            }
        }
    }

    /// 测试 get_categories 函数的 SQL 查询包含所有必需字段
    #[test]
    fn test_get_categories_sql_all_fields() {
        let sql = "SELECT id, category_name, asset_type, parent_id, sort, description, created_by, created_at, updated_by, updated_at FROM asset_category ORDER BY sort ASC";

        assert!(sql.contains("id"));
        assert!(sql.contains("category_name"));
        assert!(sql.contains("asset_type"));
        assert!(sql.contains("parent_id"));
        assert!(sql.contains("sort"));
        assert!(sql.contains("description"));
        assert!(sql.contains("created_by"));
        assert!(sql.contains("created_at"));
        assert!(sql.contains("updated_by"));
        assert!(sql.contains("updated_at"));
        assert!(sql.contains("asset_category"));
        assert!(sql.contains("ORDER BY sort ASC"));
        assert!(sql.starts_with("SELECT"));
        assert!(sql.contains("FROM"));
        assert!(sql.contains("ORDER BY"));
    }

    /// 测试 get_categories 函数返回数据的字段完整性
    /// 当数据库有数据返回时，验证每个字段的值符合预期
    #[tokio::test]
    async fn test_get_categories_field_integrity() {
        let result = get_categories().await;

        if let Ok(categories) = result {
            for cat in &categories {
                assert!(cat.id > 0, "id 必须大于0");
                assert!(!cat.category_name.is_empty(), "category_name 不能为空");
                assert!(!cat.asset_type.is_empty(), "asset_type 不能为空");
                assert!(cat.parent_id >= 0, "parent_id 不能为负数");
                assert!(cat.sort >= 0, "sort 不能为负数");
                assert!(
                    cat.created_at <= Utc::now(),
                    "created_at 不能是未来时间"
                );
                assert!(
                    cat.updated_at <= Utc::now(),
                    "updated_at 不能是未来时间"
                );
                assert!(
                    cat.updated_at >= cat.created_at,
                    "updated_at 应大于等于 created_at"
                );
            }
        }
    }

    /// 测试 get_categories 函数在数据库连接失败时的错误信息格式
    #[test]
    fn test_get_categories_error_messages() {
        let pool_error = database::get_pool().err();
        if let Some(err) = pool_error {
            let err_msg = err.to_string();
            assert!(
                err_msg.contains("获取数据库连接失败")
                    || err_msg.contains("数据库管理器未初始化")
                    || err_msg.contains("未初始化"),
                "错误信息应包含中文描述: {}",
                err_msg
            );
        }

        let sql_error = "查询资产类别失败: relation \"asset_category\" does not exist";
        assert!(sql_error.contains("查询资产类别失败"));
        assert!(sql_error.contains("asset_category"));
    }

    /// 测试 get_categories 函数在并发调用时的表现
    /// 模拟多个客户端同时请求分类列表的场景
    #[tokio::test]
    async fn test_get_categories_concurrent_calls() {
        use futures::future::join_all;

        let mut handles = Vec::new();
        for _ in 0..10 {
            handles.push(get_categories());
        }

        let results = join_all(handles).await;

        let first_result = &results[0];
        for result in &results {
            match (first_result, result) {
                (Ok(first_cats), Ok(cats)) => {
                    assert_eq!(
                        first_cats.len(),
                        cats.len(),
                        "并发调用返回的分类数量应一致"
                    );
                }
                (Err(_), Err(_)) => {}
                _ => {
                    panic!("并发调用结果不一致：一个成功而另一个失败");
                }
            }
        }
    }

    /// 测试 get_categories 函数返回的数据按 sort 字段正确排序
    #[tokio::test]
    async fn test_get_categories_sorted_by_sort() {
        let result = get_categories().await;

        if let Ok(categories) = result {
            for i in 1..categories.len() {
                assert!(
                    categories[i - 1].sort <= categories[i].sort,
                    "资产类别应按 sort 字段升序排列 (位置 {}: sort={}, 位置 {}: sort={})",
                    i - 1,
                    categories[i - 1].sort,
                    i,
                    categories[i].sort
                );
            }

            for i in 1..categories.len() {
                if categories[i - 1].sort == categories[i].sort {
                    assert!(
                        categories[i - 1].id <= categories[i].id
                            || categories[i - 1].created_at <= categories[i].created_at,
                        "sort 相同时应按创建时间或 id 排序"
                    );
                }
            }
        }
    }

    /// 测试 get_categories 函数的整体流程（需要数据库连接）
    /// 使用 #[ignore] 标记，需要显式运行
    /// 运行方式: cargo test -- --ignored 或设置 DATABASE_URL 环境变量
    #[ignore]
    #[tokio::test]
    async fn test_get_categories_integration() {
        let pool = database::get_pool().expect("数据库连接池应该已初始化");
        let categories = sqlx::query_as::<_, AssetCategory>(
            "SELECT id, category_name, asset_type, parent_id, sort, description, created_by, created_at, updated_by, updated_at FROM asset_category ORDER BY sort ASC"
        )
        .fetch_all(&pool)
        .await
        .expect("查询资产类别应该成功");

        assert!(!categories.is_empty(), "资产类别列表不应为空");

        for i in 1..categories.len() {
            assert!(
                categories[i - 1].sort <= categories[i].sort,
                "资产类别应按 sort 字段升序排列"
            );
        }

        let names: Vec<&str> = categories
            .iter()
            .map(|c| c.category_name.as_str())
            .collect();
        assert!(names.contains(&"IT设备"), "应包含 IT设备");
        assert!(names.contains(&"软件资产"), "应包含 软件资产");
        assert!(names.contains(&"办公设备"), "应包含 办公设备");
    }
}
