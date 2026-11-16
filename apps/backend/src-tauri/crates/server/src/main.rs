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
/// 编译期绝对路径在换机器 / 容器部署后必然失效。改为按「运行期工作目录」的相对位置查找，
/// 因此必须覆盖**两种真实启动方式**的工作目录：
///
/// - 部署形态：二进制与 `.env.toml` 同目录（`./.env.toml`），或 `ASSETS_CONFIG_DIR` 指定
/// - 仓库开发形态：`pnpm dev:server` = `cd apps/backend/src-tauri && cargo run -p server`，
///   而唯一的配置在 **上一级** `apps/backend/.env.toml`（桌面 `tauri dev` 用的也是这份）
///
/// 历史坑（2026-11-16 修复）：候选表里曾漏掉 `../.env.toml`，导致 `pnpm dev:server`
/// 找不到配置 → 全部回落各模块默认值 → PostgreSQL 密码认证失败
/// （`password authentication failed for user "postgres"`）→ 无头形态直接退出，
/// 而报错信息离「配置没加载」很远，很难定位。故同时补上 `tried` 诊断日志。
fn config_candidates_from(config_dir: Option<&str>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    // ① 显式指定（systemd / Docker 推荐：ASSETS_CONFIG_DIR=/etc/assets-platform）
    if let Some(dir) = config_dir.filter(|d| !d.trim().is_empty()) {
        candidates.push(PathBuf::from(dir).join(".env.toml"));
    }

    // ② 当前工作目录中的常见位置（部署目录、或被 `cd src-tauri` 之后）
    candidates.push(PathBuf::from(".env.toml"));
    candidates.push(PathBuf::from("src-tauri/.env.toml"));

    // ③ 仓库开发布局：cwd = apps/backend/src-tauri → ../.env.toml = apps/backend/.env.toml
    candidates.push(PathBuf::from("../.env.toml"));

    // ④ 仓库根启动（cwd = 仓库根，例如某些 IDE 运行配置）
    candidates.push(PathBuf::from("apps/backend/.env.toml"));

    // ⑤ 系统配置目录（systemd 常见约定）
    candidates.push(PathBuf::from("/etc/assets-platform/.env.toml"));

    candidates
}

/// 读取 `ASSETS_CONFIG_DIR` 后的候选表（启动入口使用）
fn config_candidates() -> Vec<PathBuf> {
    config_candidates_from(std::env::var("ASSETS_CONFIG_DIR").ok().as_deref())
}

/// 把候选表渲染成单行文本，便于日志复制排查
fn candidates_for_log(candidates: &[PathBuf]) -> String {
    candidates
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(" , ")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. 日志系统（OTel 在配置加载后再初始化，需要读取 OTEL_* 环境变量）
    if let Err(e) = utils::logging::init_tracing() {
        eprintln!("日志系统初始化失败: {}", e);
    }

    // 2. 配置文件 → 环境变量
    //    未加载到配置时**必须显式告警候选路径**：后续失败点（数据库认证、S3 签名）
    //    离此很远，不给路径几乎无法定位（详见 `config_candidates_from` 的历史坑注释）。
    let candidates = config_candidates();
    if bootstrap::config::load_env_toml(&candidates).is_none() {
        tracing::warn!(
            tried = %candidates_for_log(&candidates),
            "未加载到任何 .env.toml，将使用环境变量与各模块默认值；若随后数据库/对象存储认证失败，\
             请用 ASSETS_CONFIG_DIR=<配置目录> 指定，或把 .env.toml 放到当前工作目录"
        );
    }

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
        tracing::error!(
            "排查顺序：① 启动日志里是否有「已加载 .env TOML 配置文件 config=…」（没有就设置 \
             ASSETS_CONFIG_DIR，或把 .env.toml 放到当前工作目录 {cwd}）② PostgreSQL 主机/端口/库名/密码 \
             ③ 库是否已创建（见部署指南 §3.0）④ 服务端是否可达",
            cwd = std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "<未知>".to_string())
        );
        return Err(e);
    }

    // 6. LLM Router + 工作流运行时管理器
    let llm_router = bootstrap::init_llm_router().await;

    // 7. HTTP API：阻塞运行，收到 Ctrl+C / SIGTERM 时优雅关闭
    let pool = database::get_pool()?;
    bootstrap::serve_http(pool, llm_router).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ASSETS_CONFIG_DIR` 必须是**最高优先级**（生产 systemd / Docker 用它固定配置位置）
    #[test]
    fn explicit_config_dir_wins() {
        let candidates = config_candidates_from(Some("/etc/assets-platform"));
        assert_eq!(
            candidates
                .first()
                .map(|p| p.display().to_string())
                .as_deref(),
            Some("/etc/assets-platform/.env.toml")
        );
    }

    /// 空串 / 纯空白的 `ASSETS_CONFIG_DIR` 视为未设置（否则会拼出 `"   /.env.toml"` 这类无效路径，
    /// 既浪费一次 IO，也会让诊断日志里出现看不懂的候选）
    #[test]
    fn blank_config_dir_is_ignored() {
        let baseline = config_candidates_from(None);
        for blank in ["", "   ", "\t"] {
            let candidates = config_candidates_from(Some(blank));
            assert_eq!(
                candidates, baseline,
                "空白 ASSETS_CONFIG_DIR({blank:?}) 应等价于未设置"
            );
        }
    }

    /// 开发形态回归：`pnpm dev:server`（cwd = `apps/backend/src-tauri`）必须能找到
    /// `apps/backend/.env.toml`，即候选表里要有 `../.env.toml`。
    ///
    /// 缺这条时服务会静默回落默认值并在数据库认证处退出，正是「后端 http server 无法启动」的根因。
    #[test]
    fn repo_dev_layout_prefers_parent_env_toml() {
        let candidates = config_candidates_from(None);
        let rendered: Vec<String> = candidates.iter().map(|p| p.display().to_string()).collect();

        let parent = rendered
            .iter()
            .position(|p| p == "../.env.toml")
            .expect("缺少 ../.env.toml 候选：cwd=src-tauri 时将读不到 apps/backend/.env.toml");
        let system = rendered
            .iter()
            .position(|p| p == "/etc/assets-platform/.env.toml")
            .expect("缺少系统配置目录候选");

        // 仓库内配置要排在系统配置之前，避免开发机上误读 /etc 里的旧配置
        assert!(
            parent < system,
            "仓库开发布局候选应优先于 /etc：{rendered:?}"
        );
    }

    /// 部署形态回归：部署目录下的 `./.env.toml` 必须在使用 parent 候选之前命中
    #[test]
    fn cwd_candidate_precedes_parent() {
        let rendered: Vec<String> = config_candidates_from(None)
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        let cwd_index = rendered
            .iter()
            .position(|p| p == ".env.toml")
            .expect("缺少 ./.env.toml");
        let parent_index = rendered
            .iter()
            .position(|p| p == "../.env.toml")
            .expect("缺少 ../.env.toml");
        assert!(cwd_index < parent_index, "{rendered:?}");
    }

    /// 诊断日志里的候选路径要一行可读（排查时直接复制给运维）
    #[test]
    fn candidates_are_rendered_as_single_line() {
        let text = candidates_for_log(&config_candidates_from(None));
        assert!(text.contains(".env.toml"));
        assert!(!text.contains('\n'));
        assert!(text.contains(" , "), "多个候选之间应有分隔: {text}");
    }
}
