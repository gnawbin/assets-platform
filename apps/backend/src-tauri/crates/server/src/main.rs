//! 无头 HTTP 服务入口（Web 部署形态）
//!
//! 与桌面入口 `apps/backend/src-tauri/src/main.rs` 共用同一套 `crates/*` 业务实现，
//! 差别仅在于：**不创建 Tauri WebView 窗口**，只启动 axum HTTP API。
//!
//! 用途：
//! - `apps/web` 静态产物（Next.js `output: "export"`）由 nginx 托管，前端以
//!   `NEXT_PUBLIC_API_ADAPTER=http`（或浏览器自动探测）访问本服务的 `/api/*`
//! - 桌面形态保持不变（`tauri dev` / `tauri build`），两者可同时部署
//!
//! 启动顺序（与桌面 `setup` 回调的语义一一对应）：
//! 1. `init_tracing`（先于配置加载，保证配置文件相关的日志可见）
//! 2. `.env.toml` → 环境变量
//! 3. `init_otel`
//! 4. doc-parser 侧车（受 `DOC_PARSER_ENABLED` / `DOC_PARSER_BACKEND` 控制）
//! 5. 数据库 + 默认租户 schema（失败即退出：无头形态不带病服务）
//! 6. LLM Router + 工作流运行时
//! 7. HTTP API（阻塞，收到 Ctrl+C / SIGTERM 优雅退出）
//!
//! 环境变量：
//! - `API_BIND` / `API_PORT`：监听地址与端口（默认 `0.0.0.0:3001`）
//! - `API_DOCS_ENABLED`：是否暴露 `/swagger-ui` 与 `/api-docs/openapi.json`
//! - `PG_*` / `S3_*` / `JWT_SECRET` / `OTEL_*`：见 `apps/backend/.env.example.toml`
//! - `ASSETS_CONFIG_DIR`：额外指定的配置目录（其下的 `.env.toml` 优先加载）

use std::path::PathBuf;

/// `.env.toml` 候选路径（按优先级从高到低）
///
/// 与桌面入口不同，这里刻意**不使用** `CARGO_MANIFEST_DIR`：
/// 编译期绝对路径在换机器 / 容器部署后必然失效。
fn config_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    // 显式指定（systemd / Docker 推荐：ASSETS_CONFIG_DIR=/etc/assets-platform）
    if let Ok(dir) = std::env::var("ASSETS_CONFIG_DIR") {
        candidates.push(PathBuf::from(dir).join(".env.toml"));
    }

    // 当前工作目录（在 src-tauri 或部署目录下启动）
    candidates.push(PathBuf::from(".env.toml"));
    candidates.push(PathBuf::from("src-tauri/.env.toml"));
    // 系统配置目录（systemd 常见约定）
    candidates.push(PathBuf::from("/etc/assets-platform/.env.toml"));

    candidates
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. 日志系统（OTel 在配置加载后再初始化，需要读取 OTEL_* 环境变量）
    if let Err(e) = utils::logging::init_tracing() {
        eprintln!("日志系统初始化失败: {}", e);
    }

    // 2. 配置文件 → 环境变量
    bootstrap::config::load_env_toml(&config_candidates());

    // 3. OpenTelemetry（已在 Tokio 运行时上下文内，无需 block_on）
    if let Err(e) = utils::logging::init_otel() {
        tracing::warn!("OpenTelemetry 初始化失败: {}", e);
    }

    // 4. doc-parser 侧车（Python FastAPI）。
    //    独立部署（systemd / Docker）时用 DOC_PARSER_ENABLED=false 关闭，
    //    由后端作为客户端调用外部服务；句柄需保活到进程结束。
    let _doc_parser_child = service::doc_parser::start_doc_parser();

    // 5. 数据库 + 默认租户 schema：无头形态下初始化失败等同于不可用，直接退出
    if let Err(e) = bootstrap::init_database_and_schema().await {
        tracing::error!("数据库初始化失败，服务终止: {:?}", e);
        return Err(e);
    }

    // 6. LLM Router + 工作流运行时管理器
    let llm_router = bootstrap::init_llm_router().await;

    // 7. HTTP API：阻塞运行，收到 Ctrl+C / SIGTERM 时优雅关闭
    let pool = database::get_pool()?;
    bootstrap::serve_http(pool, llm_router).await
}
