mod api;
mod commands;
mod database;
mod service;
mod utils;

/// 加载 .env TOML 配置文件，将值设置为环境变量
fn load_env() {
    // 尝试多个路径查找 .env.toml 文件
    // 1. 当前工作目录（可能是项目根目录或 src-tauri 目录）
    // 2. src-tauri/.env.toml（相对于当前工作目录）
    // 3. 项目根目录（通过 CARGO_MANIFEST_DIR 定位）
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root_dir = manifest_dir.parent().unwrap_or(manifest_dir);
    let root_env = root_dir.join(".env.toml");

    let content = std::fs::read_to_string(".env.toml")
        .or_else(|_| std::fs::read_to_string("src-tauri/.env.toml"))
        .or_else(|_| std::fs::read_to_string(&root_env));

    match content {
        Ok(text) => match text.parse::<toml::Table>() {
            Ok(table) => {
                for (section, values) in &table {
                    if let Some(sub_table) = values.as_table() {
                        // 特殊处理 postgres 段的 read_replicas 数组
                        if section == "postgres" {
                            if let Some(toml::Value::Array(replicas)) =
                                sub_table.get("read_replicas")
                            {
                                let hosts_str: Vec<String> = replicas
                                    .iter()
                                    .filter_map(|v| {
                                        let table = v.as_table()?;
                                        let host = table.get("host")?.as_str()?;
                                        let port = table
                                            .get("port")
                                            .and_then(|p| p.as_integer())
                                            .unwrap_or(5432);
                                        let weight = table
                                            .get("weight")
                                            .and_then(|w| w.as_integer())
                                            .unwrap_or(1);
                                        Some(format!("{}:{}:{}", host, port, weight))
                                    })
                                    .collect();
                                if !hosts_str.is_empty() {
                                    let pg_read_hosts = hosts_str.join(",");
                                    std::env::set_var("PG_READ_HOSTS", &pg_read_hosts);
                                    std::env::set_var("POSTGRES_READ_HOSTS", &pg_read_hosts);
                                }
                            }
                        }

                        for (key, value) in sub_table {
                            let env_value = match value {
                                toml::Value::String(s) => s.clone(),
                                toml::Value::Integer(i) => i.to_string(),
                                toml::Value::Float(f) => f.to_string(),
                                toml::Value::Boolean(b) => b.to_string(),
                                _ => continue,
                            };
                            // 生成标准环境变量名：SECTION_KEY
                            let env_key =
                                format!("{}_{}", section.to_uppercase(), key.to_uppercase());
                            std::env::set_var(&env_key, &env_value);

                            // 兼容旧版环境变量名（postgres 段映射为 PG_ 前缀）
                            if section == "postgres" {
                                let legacy_key = format!("PG_{}", key.to_uppercase());
                                std::env::set_var(&legacy_key, &env_value);
                            } else if section == "auth" && key == "jwt_secret" {
                                std::env::set_var("JWT_SECRET", &env_value);
                            }
                        }
                    }
                }
                tracing::info!("已加载 .env TOML 配置文件");
            }
            Err(e) => {
                tracing::warn!(".env 文件解析失败: {}", e);
            }
        },
        Err(e) => {
            tracing::warn!("未找到 .env 文件，将使用默认环境变量: {}", e);
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
            commands::role_commands::get_user_menus,
            commands::role_commands::get_user_role_ids,
            commands::role_commands::assign_user_roles,
            // 用户管理
            commands::user_commands::login,
            commands::user_commands::get_users,
            commands::user_commands::insert_user,
            commands::user_commands::update_user,
            commands::user_commands::delete_user,
            commands::user_commands::get_current_user,
            commands::user_commands::reset_password,
            // 租户管理
            commands::tenant_commands::get_tenants,
            commands::tenant_commands::insert_tenant,
            commands::tenant_commands::update_tenant,
            commands::tenant_commands::delete_tenant,
            commands::tenant_commands::switch_tenant,
            // 注册申请
            commands::register_commands::register,
            commands::register_commands::get_registrations,
            commands::register_commands::approve_registration,
            commands::register_commands::reject_registration,
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
            // 流程管理-领用
            commands::process_commands::get_receives,
            commands::process_commands::insert_receive,
            commands::process_commands::update_receive,
            commands::process_commands::delete_receive,
            // 流程管理-归还
            commands::process_commands::get_returns,
            commands::process_commands::insert_return,
            commands::process_commands::update_return,
            commands::process_commands::delete_return,
            // 流程管理-调拨
            commands::process_commands::get_transfers,
            commands::process_commands::insert_transfer,
            commands::process_commands::update_transfer,
            commands::process_commands::delete_transfer,
            // 流程管理-维修
            commands::process_commands::get_repairs,
            commands::process_commands::insert_repair,
            commands::process_commands::update_repair,
            commands::process_commands::delete_repair,
            // 流程管理-报废
            commands::process_commands::get_scraps,
            commands::process_commands::insert_scrap,
            commands::process_commands::update_scrap,
            commands::process_commands::delete_scrap,
            // 流程管理-采购
            commands::process_commands::get_purchases,
            commands::process_commands::insert_purchase,
            commands::process_commands::update_purchase,
            commands::process_commands::delete_purchase,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            tracing::error!("Tauri 应用运行出错: {}", e);
        });
}

#[cfg(test)]
mod tests {
    // 集成测试在 tests/ 目录中
}
