//! HTTP API 模块
//!
//! 提供 RESTful API 服务，与 Tauri 桌面应用同时运行。
//! 使用 axum 框架，支持 OpenAPI/Swagger 文档。

pub mod asset_routes;
pub mod auth;
pub mod category_routes;
pub mod department_routes;
pub mod knowledge_routes;
pub mod openapi;
pub mod process_routes;
pub mod register_routes;
pub mod response;
pub mod role_routes;
pub mod skill_routes;
pub mod tenant_routes;
pub mod upload_routes;
pub mod user_routes;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::http::Method;
use axum::middleware;
use axum::routing::{delete, get, post, put};
use axum::Router;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use self::auth::auth_middleware;
use self::openapi::ApiDoc;
use crate::engine::skill_registry::SkillRegistry;

/// 启动 HTTP API 服务
///
/// 在 Tauri 的 setup 回调中调用此函数，通过 spawn 在后台运行。
/// 返回 Result，方便调用方记录错误日志。
pub async fn start_http_server(pool: sqlx::PgPool) -> anyhow::Result<()> {
    let port: u16 = std::env::var("API_PORT")
        .unwrap_or_else(|_| "3001".to_string())
        .parse()
        .unwrap_or(3001);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    // CORS 配置（允许前端跨域访问）
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(Any);

    // 初始化大文件上传路由（S3 客户端初始化需要 async 上下文）
    let upload_router = {
        let s3_config = crate::storage::s3::S3Config::from_env().unwrap_or_default();
        let s3_client = crate::storage::s3::S3Client::new(s3_config.clone())
            .await
            .map_err(|e| anyhow::anyhow!("S3 客户端初始化失败: {:?}", e))?;
        let upload_mgr =
            crate::storage::upload::UploadManager::new(pool.clone(), s3_client, s3_config);
        let state = std::sync::Arc::new(upload_routes::UploadRouterState { upload_mgr });
        Router::new()
            .route("/api/upload/init", post(upload_routes::init_upload))
            .route("/api/upload/{id}/start", post(upload_routes::start_upload))
            .route(
                "/api/upload/{id}/commit",
                post(upload_routes::commit_upload),
            )
            .route("/api/upload/{id}/chunk", post(upload_routes::report_chunk))
            .route(
                "/api/upload/{id}/progress",
                get(upload_routes::get_progress),
            )
            .route(
                "/api/upload/{id}/complete",
                post(upload_routes::complete_upload),
            )
            .route("/api/upload/{id}", delete(upload_routes::abort_upload))
            .with_state(state)
    };

    // 构建路由
    let app = create_router(pool)
        .merge(upload_router)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    info!("HTTP API 服务启动于 http://{}", addr);
    info!("Swagger UI: http://localhost:{}/api/swagger-ui/", port);

    // 启动 HTTP 服务
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("绑定 TCP 监听地址 {} 失败: {}", addr, e))?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| anyhow::anyhow!("HTTP 服务运行出错: {}", e))?;

    Ok(())
}

/// 创建路由
fn create_router(pool: sqlx::PgPool) -> Router {
    let state = Arc::new(pool);

    // 公开路由（无需认证）
    let public_routes = Router::new()
        .route("/api/auth/login", post(user_routes::login))
        .route("/api/auth/register", post(register_routes::register));

    // 需要认证的路由
    let protected_routes = Router::new()
        // 资产分类
        .route("/api/categories", get(category_routes::get_categories))
        .route(
            "/api/categories/parents",
            get(category_routes::get_categories_parents),
        )
        .route("/api/categories", post(category_routes::insert_category))
        .route(
            "/api/categories/{id}",
            put(category_routes::update_category),
        )
        .route(
            "/api/categories/{id}",
            delete(category_routes::delete_category),
        )
        // 固定资产
        .route(
            "/api/assets/hardware",
            get(asset_routes::get_hardware_assets),
        )
        .route(
            "/api/assets/hardware",
            post(asset_routes::insert_hardware_asset),
        )
        .route(
            "/api/assets/hardware/{id}",
            put(asset_routes::update_hardware_asset),
        )
        .route(
            "/api/assets/hardware/{id}",
            delete(asset_routes::delete_hardware_asset),
        )
        // 无形资产
        .route(
            "/api/assets/intangible",
            get(asset_routes::get_intangible_assets),
        )
        .route(
            "/api/assets/intangible",
            post(asset_routes::insert_intangible_asset),
        )
        .route(
            "/api/assets/intangible/{id}",
            put(asset_routes::update_intangible_asset),
        )
        .route(
            "/api/assets/intangible/{id}",
            delete(asset_routes::delete_intangible_asset),
        )
        // 部门
        .route("/api/departments", get(department_routes::get_departments))
        .route(
            "/api/departments",
            post(department_routes::insert_department),
        )
        .route(
            "/api/departments/{id}",
            put(department_routes::update_department),
        )
        .route(
            "/api/departments/{id}",
            delete(department_routes::delete_department),
        )
        // 用户
        .route("/api/users/me", get(user_routes::get_current_user))
        .route("/api/users", get(user_routes::get_users))
        .route("/api/users", post(user_routes::insert_user))
        .route("/api/users/{id}", put(user_routes::update_user))
        .route("/api/users/{id}", delete(user_routes::delete_user))
        .route(
            "/api/users/{id}/reset-password",
            post(user_routes::reset_password),
        )
        .route("/api/users/{id}/roles", get(role_routes::get_user_role_ids))
        .route(
            "/api/users/{id}/roles",
            post(role_routes::assign_user_roles),
        )
        // 角色
        .route("/api/roles", get(role_routes::get_roles))
        .route("/api/roles", post(role_routes::insert_role))
        .route("/api/roles/{id}", delete(role_routes::delete_role))
        .route("/api/roles/{id}/menus", get(role_routes::get_role_menu_ids))
        .route(
            "/api/roles/{id}/menus",
            post(role_routes::assign_role_menus),
        )
        // 租户
        .route("/api/tenants", get(tenant_routes::get_tenants))
        .route("/api/tenants", post(tenant_routes::insert_tenant))
        .route("/api/tenants/{id}", put(tenant_routes::update_tenant))
        .route("/api/tenants/{id}", delete(tenant_routes::delete_tenant))
        .route("/api/tenants/switch", post(tenant_routes::switch_tenant))
        .route("/api/tenants/assign", post(tenant_routes::assign_tenants))
        .route(
            "/api/users/{id}/tenants",
            get(tenant_routes::get_user_tenants),
        )
        // 注册审核
        .route(
            "/api/auth/registrations",
            get(register_routes::get_registrations),
        )
        .route(
            "/api/auth/registrations/{id}/approve",
            post(register_routes::approve_registration),
        )
        .route(
            "/api/auth/registrations/{id}/reject",
            post(register_routes::reject_registration),
        )
        // 知识库
        .route("/api/knowledge/tree", get(knowledge_routes::get_tree))
        .route("/api/knowledge/node", post(knowledge_routes::insert_node))
        .route(
            "/api/knowledge/node/{id}",
            put(knowledge_routes::update_node),
        )
        .route(
            "/api/knowledge/node/{id}",
            delete(knowledge_routes::delete_node),
        )
        .route(
            "/api/knowledge/node/{id}/move",
            put(knowledge_routes::move_node),
        )
        .route("/api/knowledge/list", get(knowledge_routes::get_list))
        .route("/api/knowledge", post(knowledge_routes::insert_knowledge))
        .route("/api/knowledge/{id}", get(knowledge_routes::get_by_id))
        .route(
            "/api/knowledge/{id}",
            put(knowledge_routes::update_knowledge),
        )
        .route(
            "/api/knowledge/{id}",
            delete(knowledge_routes::delete_knowledge),
        )
        // 菜单
        .route("/api/menus/tree", get(role_routes::get_all_menus_tree))
        .route("/api/menus/user", get(role_routes::get_user_menus))
        // 流程管理-领用
        .route("/api/process/receive", get(process_routes::get_receives))
        .route("/api/process/receive", post(process_routes::insert_receive))
        .route(
            "/api/process/receive/{id}",
            put(process_routes::update_receive),
        )
        .route(
            "/api/process/receive/{id}",
            delete(process_routes::delete_receive),
        )
        // 流程管理-归还
        .route("/api/process/return", get(process_routes::get_returns))
        .route("/api/process/return", post(process_routes::insert_return))
        .route(
            "/api/process/return/{id}",
            put(process_routes::update_return),
        )
        .route(
            "/api/process/return/{id}",
            delete(process_routes::delete_return),
        )
        // 流程管理-调拨
        .route("/api/process/transfer", get(process_routes::get_transfers))
        .route(
            "/api/process/transfer",
            post(process_routes::insert_transfer),
        )
        .route(
            "/api/process/transfer/{id}",
            put(process_routes::update_transfer),
        )
        .route(
            "/api/process/transfer/{id}",
            delete(process_routes::delete_transfer),
        )
        // 流程管理-维修
        .route("/api/process/repair", get(process_routes::get_repairs))
        .route("/api/process/repair", post(process_routes::insert_repair))
        .route(
            "/api/process/repair/{id}",
            put(process_routes::update_repair),
        )
        .route(
            "/api/process/repair/{id}",
            delete(process_routes::delete_repair),
        )
        // 流程管理-报废
        .route("/api/process/scrap", get(process_routes::get_scraps))
        .route("/api/process/scrap", post(process_routes::insert_scrap))
        .route("/api/process/scrap/{id}", put(process_routes::update_scrap))
        .route(
            "/api/process/scrap/{id}",
            delete(process_routes::delete_scrap),
        )
        // 流程管理-采购
        .route("/api/process/purchase", get(process_routes::get_purchases))
        .route(
            "/api/process/purchase",
            post(process_routes::insert_purchase),
        )
        .route(
            "/api/process/purchase/{id}",
            put(process_routes::update_purchase),
        )
        .route(
            "/api/process/purchase/{id}",
            delete(process_routes::delete_purchase),
        )
        // 应用认证中间件
        .layer(middleware::from_fn(auth_middleware));

    // Skill 路由（使用独立的 SkillRouterState）
    let skill_registry = Arc::new(Mutex::new(SkillRegistry::new()));
    let skill_router_state = Arc::new(skill_routes::SkillRouterState {
        registry: skill_registry,
    });
    let skill_routes = skill_routes::skill_routes().with_state(skill_router_state);

    // Swagger UI
    let swagger = SwaggerUi::new("/api/swagger-ui").url("/api/openapi.json", ApiDoc::openapi());

    // 合并所有路由（大文件上传路由在 start_http_server 中通过 .merge() 添加）
    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .merge(skill_routes)
        .merge(swagger)
        .with_state(state)
}

/// 优雅关闭信号处理
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("无法注册 Ctrl+C 信号处理器");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("无法注册 SIGTERM 信号处理器")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("收到关闭信号，HTTP 服务正在关闭...");
}
