//! 启动初始化（桌面 Tauri / 无头 HTTP 服务共用）
//!
//! 背景：改造前 `apps/backend/src-tauri/src/lib.rs` 的 `setup` 回调里内联了
//! 数据库初始化、默认租户 schema、LLM Router、工作流运行时与 HTTP 服务的启动逻辑，
//! 导致「服务端形态」必须依附 Tauri 才能跑起来（需要 WebView 与图形环境）。
//!
//! 本 crate 把这些与 Tauri **无关**的初始化抽成可复用函数：
//! - 桌面入口 [`crate`] 的使用方：`crates/server`（无头 bin）与 `apps/backend/src-tauri`
//! - 依赖方向：`bootstrap → api / apidoc / database / service / workflow / utils`，
//!   不依赖 `tauri`，因此无 GUI 环境可单独编译（`cargo build -p server`）
//!
//! 日志级别与错误处理刻意保持与改造前的桌面行为一致：
//! 初始化失败只记录日志，由调用方决定是否中断启动。

pub mod config;

use std::sync::Arc;

use anyhow::Result;
use service::llm_gateway_service::LLMRouter;

/// 初始化数据库，并绑定默认租户 schema
///
/// 对应改造前 `setup` 回调中的两段逻辑：
/// 1. `database::init_database()`（内含建表 / 迁移 / 初始数据）
/// 2. 从 `public.sys_tenant` 取 `id = 1` 的租户并 `set_global_schema()`
///
/// schema 绑定不可省略：否则 `GLOBAL_SCHEMA` 停留在默认值 `public`，Tauri 模式下
/// `schema_prefix()` 退化为空串，业务表查询落到 `public` 并报
/// `relation "xxx" does not exist`。
///
/// # 错误
/// 数据库初始化失败时返回错误；调用方需自行决定是否继续启动：
/// - 桌面形态：只告警，保证窗口仍能打开（与改造前一致）
/// - 无头服务形态：应直接退出，避免带病提供服务
pub async fn init_database_and_schema() -> Result<()> {
    database::init_database().await?;
    tracing::info!("数据库初始化完成");

    // 只要连接池可用就绑定默认租户 schema（即使部分迁移失败）
    match database::get_pool() {
        Ok(pool) => {
            let schema: Option<String> = sqlx::query_scalar(
                "SELECT schema_name FROM public.sys_tenant WHERE id = 1 AND enable = true",
            )
            .fetch_optional(&pool)
            .await
            .ok()
            .flatten();

            match schema {
                Some(sn) => {
                    database::set_global_schema(sn);
                    tracing::info!("已设置默认租户 schema");
                }
                None => tracing::warn!("未找到默认租户 schema，GLOBAL_SCHEMA 保持 public"),
            }
        }
        Err(e) => tracing::warn!("数据库连接池不可用，跳过默认租户 schema 设置: {}", e),
    }

    Ok(())
}

/// 初始化 LLM Router 与工作流运行时管理器
///
/// 返回的 `LLMRouter` 由调用方持有（桌面侧还需 `app.manage()` 注入给命令层）。
/// 工作流运行时管理器内部使用全局单例，无需调用方保活。
pub async fn init_llm_router() -> Arc<LLMRouter> {
    let llm_router = Arc::new(LLMRouter::new());

    match llm_router.refresh_providers().await {
        Ok(()) => tracing::info!("LLM Router 初始化完成"),
        Err(e) => tracing::warn!("LLM Provider 加载失败（可能需要稍后手动刷新）: {}", e),
    }

    workflow::WorkflowManager::init(llm_router.clone());
    tracing::info!("工作流运行时管理器已初始化");

    llm_router
}

/// 启动 HTTP API 服务（阻塞直到服务退出）
///
/// OpenAPI / Swagger UI 路由由 `API_DOCS_ENABLED` 控制（默认开启），
/// 通过 `api::start_http_server_with` 注入，避免 `api → apidoc` 反向依赖。
///
/// 监听地址由 `API_BIND` + `API_PORT` 决定（见 `crates/api`）。
pub async fn serve_http(pool: sqlx::PgPool, llm_router: Arc<LLMRouter>) -> Result<()> {
    let docs = apidoc::docs_enabled().then(apidoc::docs_router);

    tracing::info!("正在启动 HTTP API 服务...");
    match api::start_http_server_with(pool, Some(llm_router), docs).await {
        Ok(()) => {
            tracing::info!("HTTP API 服务已停止");
            Ok(())
        }
        Err(e) => {
            tracing::error!("HTTP API 服务异常退出: {:?}", e);
            Err(e)
        }
    }
}
