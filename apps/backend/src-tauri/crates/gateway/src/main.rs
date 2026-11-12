//! 业务网关进程入口（多实例部署）
//!
//! 与 `crates/server` 的区别：这里只跑 HTTP + SSE，不加载桌面端专属能力；
//! 启动顺序与 `server` 保持一致（日志 → env.toml → OTel → 数据库 → LLM 路由）。

use std::path::PathBuf;

/// 候选 `env.toml` 路径（与 `crates/server` 一致）
fn config_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("env.toml"),
        PathBuf::from("../../env.toml"),
        PathBuf::from("../../../env.toml"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("env.toml"));
            candidates.push(dir.join("../env.toml"));
        }
    }
    candidates
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Err(e) = utils::logging::init_tracing() {
        eprintln!("日志系统初始化失败: {}", e);
    }
    bootstrap::config::load_env_toml(&config_candidates());
    if let Err(e) = utils::logging::init_otel() {
        tracing::warn!("OpenTelemetry 初始化失败（不影响服务）: {}", e);
    }

    // 文档解析子进程（与 server 一致：预览生成依赖它）
    let _doc_parser = service::doc_parser::start_doc_parser();

    if let Err(e) = bootstrap::init_database_and_schema().await {
        tracing::error!("数据库初始化失败，网关终止: {:?}", e);
        return Err(e);
    }

    let llm_router = bootstrap::init_llm_router().await;
    let cfg = gateway::GatewayConfig::from_env()?;
    let pool = database::get_pool()?;
    gateway::serve(cfg, pool, llm_router).await
}
