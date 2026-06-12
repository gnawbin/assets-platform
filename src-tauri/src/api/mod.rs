//! HTTP API 模块
//!
//! 提供 RESTful API 服务，与 Tauri 桌面应用同时运行。
//! 使用 axum 框架，支持 OpenAPI/Swagger 文档。

pub mod asset_routes;
pub mod auth;
pub mod category_routes;
pub mod department_routes;
pub mod openapi;
pub mod response;
pub mod role_routes;
pub mod user_routes;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::http::Method;
use axum::middleware;
use axum::routing::{delete, get, post, put};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use self::auth::auth_middleware;
use self::openapi::ApiDoc;

/// 启动 HTTP API 服务
///
/// 在 Tauri 的 setup 回调中调用此函数，通过 spawn 在后台运行。
pub async fn start_http_server(pool: sqlx::PgPool) {
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

    // 构建路由
    let app = create_router(pool)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    info!("HTTP API 服务启动于 http://{}", addr);
    info!("Swagger UI: http://localhost:{}/api/swagger-ui/", port);

    // 启动 HTTP 服务
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

/// 创建路由
fn create_router(pool: sqlx::PgPool) -> Router {
    let state = Arc::new(pool);

    // 公开路由（无需认证）
    let public_routes = Router::new()
        .route("/api/auth/login", post(user_routes::login))
        .route("/api/auth/register", post(register_placeholder));

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
        .route("/api/users", get(user_routes::get_users))
        .route("/api/users", post(user_routes::insert_user))
        .route("/api/users/{id}", put(user_routes::update_user))
        .route("/api/users/{id}", delete(user_routes::delete_user))
        .route("/api/users/{id}/roles", get(role_routes::get_user_role_ids))
        .route("/api/users/{id}/roles", post(role_routes::assign_user_roles))
        // 角色
        .route("/api/roles", get(role_routes::get_roles))
        .route("/api/roles", post(role_routes::insert_role))
        .route("/api/roles/{id}", delete(role_routes::delete_role))
        // 菜单
        .route("/api/menus/tree", get(role_routes::get_all_menus_tree))
        .route("/api/menus/user", get(role_routes::get_user_menus))
        // 应用认证中间件
        .layer(middleware::from_fn(auth_middleware));

    // Swagger UI
    let swagger = SwaggerUi::new("/api/swagger-ui").url("/api/openapi.json", ApiDoc::openapi());

    // 合并所有路由
    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .merge(swagger)
        .with_state(state)
}

/// 注册占位（后续实现）
async fn register_placeholder() -> &'static str {
    "注册功能待实现"
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
