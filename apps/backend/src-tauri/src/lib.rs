mod api;
mod commands;
mod database;
mod engine;
mod service;
mod storage;
mod utils;
mod workflow;

use std::sync::Arc;
use tauri::Manager;

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

    // 自动启动 doc-parser 侧车（Python FastAPI，用于多模态文件/视频解析）
    // 端口已在运行时跳过；返回的 Child 句柄保持到应用退出
    let _doc_parser_child = service::doc_parser::start_doc_parser();

    // 初始化 tracing 日志系统
    if let Err(e) = utils::logging::init_tracing() {
        eprintln!("日志系统初始化失败: {}", e);
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(commands::skill_commands::SkillRegistryState(
            std::sync::Arc::new(tokio::sync::Mutex::new(
                engine::skill_registry::SkillRegistry::new(),
            )),
        ))
        .setup(|app| {
            // 初始化 OpenTelemetry（需要 Tokio 运行时上下文）
            tauri::async_runtime::block_on(async {
                if let Err(e) = utils::logging::init_otel() {
                    tracing::warn!("OpenTelemetry 初始化失败: {}", e);
                }
            });

            // 应用启动时自动初始化数据库
            tauri::async_runtime::block_on(async {
                match database::init_database().await {
                    Ok(()) => {
                        tracing::info!("数据库初始化完成");
                        // 初始化默认租户 schema（Tauri 模式下 GLOBAL_SCHEMA 的默认值）
                        if let Ok(pool) = database::get_pool() {
                            let schema: Option<String> = sqlx::query_scalar(
                                "SELECT schema_name FROM public.sys_tenant WHERE id = 1 AND enable = true"
                            )
                            .fetch_optional(&pool)
                            .await
                            .ok()
                            .flatten();
                            if let Some(sn) = schema {
                                database::set_global_schema(sn);
                                tracing::info!("已设置默认租户 schema");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("数据库初始化失败: {:?}", e);
                        // 不 panic，让 HTTP API 服务尝试启动（如果连接池可用）
                    }
                }
            });

            // 初始化 LLM Router
            let llm_router = Arc::new(service::llm_gateway_service::LLMRouter::new());
            tauri::async_runtime::block_on(async {
                if let Err(e) = llm_router.refresh_providers().await {
                    tracing::warn!("LLM Provider 加载失败（可能需要稍后手动刷新）: {}", e);
                } else {
                    tracing::info!("LLM Router 初始化完成");
                }
            });
            app.manage(llm_router.clone());

            // 在后台启动 HTTP API 服务（与 Tauri 共用 Tokio 运行时）
            let llm_router_clone = llm_router.clone();
            match database::get_pool() {
                Ok(pool) => {
                    tracing::info!("准备启动 HTTP API 服务...");
                    tauri::async_runtime::spawn(async move {
                        tracing::info!("正在启动 HTTP API 服务...");
                        if let Err(e) = api::start_http_server(pool, Some(llm_router_clone)).await {
                            tracing::error!("HTTP API 服务异常退出: {:?}", e);
                        } else {
                            tracing::info!("HTTP API 服务已停止");
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("获取数据库连接池失败，无法启动 HTTP API 服务: {}", e);
                }
            }

            tracing::info!("Tauri setup 回调完成");
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
            commands::tenant_commands::assign_user_tenants,
            commands::tenant_commands::get_user_tenants,
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
            // 知识资产（OKF 新表）
            commands::knowledge_asset_commands::get_knowledge_asset_by_tree_node,
            commands::knowledge_asset_commands::get_knowledge_asset,
            commands::knowledge_asset_commands::list_knowledge_assets,
            commands::knowledge_asset_commands::create_knowledge_asset,
            commands::knowledge_asset_commands::update_knowledge_asset,
            commands::knowledge_asset_commands::delete_knowledge_asset,
            commands::knowledge_asset_commands::attach_file_to_knowledge,
            // 知识库
            commands::knowledge_commands::get_knowledge_tree,
            commands::knowledge_commands::insert_knowledge_node,
            commands::knowledge_commands::update_knowledge_node,
            commands::knowledge_commands::delete_knowledge_node,
            commands::knowledge_commands::move_knowledge_node,
            commands::knowledge_commands::get_knowledge_list,
            commands::knowledge_commands::get_knowledge_by_id,
            commands::knowledge_commands::insert_knowledge,
            commands::knowledge_commands::update_knowledge,
            commands::knowledge_commands::delete_knowledge,
            // 知识库模块 - RAG
            commands::rag_commands::chunk_and_vectorize,
            commands::rag_commands::test_rag_retrieval,
            // 知识库模块 - 对话
            commands::conversation_commands::create_conversation,
            commands::conversation_commands::send_message,
            commands::conversation_commands::get_conversations,
            commands::conversation_commands::get_conversation_messages,
            commands::conversation_commands::update_conversation_title,
            commands::conversation_commands::delete_conversation,
            // 知识库模块 - LLM厂商
            commands::llm_provider_commands::get_llm_providers,
            commands::llm_provider_commands::get_llm_provider,
            commands::llm_provider_commands::create_llm_provider,
            commands::llm_provider_commands::update_llm_provider,
            commands::llm_provider_commands::delete_llm_provider,
            commands::llm_provider_commands::get_llm_models,
            commands::llm_provider_commands::create_llm_model,
            commands::llm_provider_commands::update_llm_model,
            commands::llm_provider_commands::delete_llm_model,
            commands::llm_provider_commands::fetch_llm_models,
            commands::llm_provider_commands::get_user_llm_setting,
            commands::llm_provider_commands::save_user_llm_setting,
            // Zen Engine - Skill 管理
            commands::skill_commands::list_skills,
            commands::skill_commands::get_skill,
            commands::skill_commands::execute_skill,
            commands::skill_commands::register_custom_skill,
            commands::skill_commands::unregister_skill,
            commands::skill_commands::get_skill_count,
            // 编号规则
            commands::numbering_commands::get_numbering_rules,
            commands::numbering_commands::get_numbering_rule,
            commands::numbering_commands::save_numbering_rule,
            commands::numbering_commands::reset_numbering_sequence,
            // 大文件上传（两步提交）
            commands::upload_commands::upload_init,
            commands::upload_commands::upload_start,
            commands::upload_commands::upload_report_chunk,
            commands::upload_commands::upload_complete,
            commands::upload_commands::upload_abort,
            commands::upload_commands::upload_get_progress,
            commands::upload_commands::upload_commit,
            commands::upload_commands::upload_get_version_history,
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
