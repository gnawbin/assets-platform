//! 文档解析客户端：`local`（进程内 Rust）/ `remote`（Python 侧车）双后端
//!
//! 由环境变量 `DOC_PARSER_BACKEND` 单一开关控制（`local` | `remote`，缺省 `remote`）：
//! - `local`：调用 `crates/parser`，解析在 Tauri 进程内完成，**不拉起 Python**；
//! - `remote`：HTTP 调用 doc-parser 侧车（`DOC_PARSER_URL`，令牌 `DOC_PARSER_TOKEN`）。
//!
//! 两条路径返回同一个 [`ParseResult`]（定义在 `crates/parser`），调用方无需感知差异。
//! 远端模式由 [`start_doc_parser`] 自动拉起侧车进程；本地模式该函数直接早退。
//!
//! 相关文档：
//! - docs/Rust原生多模态解析后端设计方案.md
//! - docs/知识库模块/智能问答多模态（文件与视频上传）设计方案.md

use tracing::{error, info};

/// 解析结果（与 doc-parser 的 ParseResult 对齐，定义在 `crates/parser`）
pub use parser::ParseResult;

/// 解析后端
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// 进程内 Rust 解析（`crates/parser`）
    Local,
    /// Python 侧车 HTTP（`apps/doc-parser`）
    Remote,
}

impl Backend {
    /// 配置值 / 日志用的字符串表示
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
        }
    }
}

/// 当前解析后端
///
/// 读 `DOC_PARSER_BACKEND`（由 `.env.toml` 的 `[doc_parser] backend` 映射而来）。
/// **缺省 / 非法值一律回落 `remote`**，保证未配置时行为与改造前完全一致。
pub fn backend() -> Backend {
    parse_backend_value(&std::env::var("DOC_PARSER_BACKEND").unwrap_or_default())
}

/// 解析配置值 → 后端（大小写 / 首尾空白容错，非法值回落 `remote`）
fn parse_backend_value(raw: &str) -> Backend {
    match raw.trim().to_ascii_lowercase().as_str() {
        "local" => Backend::Local,
        _ => Backend::Remote,
    }
}

/// Python 侧车 HTTP 客户端
pub struct DocParserClient {
    base_url: String,
    token: String,
    http_client: reqwest::Client,
}

impl Default for DocParserClient {
    fn default() -> Self {
        Self::new()
    }
}

impl DocParserClient {
    pub fn new() -> Self {
        let base_url = std::env::var("DOC_PARSER_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8321".to_string());
        let token = std::env::var("DOC_PARSER_TOKEN").unwrap_or_default();

        let http_client = reqwest::Client::builder()
            // 视频/音频解析耗时长，放宽超时（10 分钟）
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .unwrap_or_else(|e| {
                error!("构建 doc-parser HTTP 客户端失败: {}", e);
                reqwest::Client::new()
            });

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            http_client,
        }
    }

    /// 解析本地文件 → 纯文本 + 图片路径
    ///
    /// 按 [`backend`] 分发：
    /// - `local` → `crates/parser`（进程内）；
    /// - `remote` → doc-parser 侧车 HTTP。
    ///
    /// `options` 常见字段：
    /// - `skip_index`: 视频解析后是否跳过向量化入库（聊天场景必须 `true`，仅解析）
    /// - `frame_interval`: 视频抽帧间隔（秒）
    pub async fn parse_file(
        &self,
        file_path: &str,
        options: &serde_json::Value,
    ) -> Result<ParseResult, String> {
        match backend() {
            Backend::Local => self.parse_file_local(file_path, options).await,
            Backend::Remote => self.parse_file_remote(file_path, options).await,
        }
    }

    /// 本地后端：进程内 Rust 解析，不经过 HTTP
    ///
    /// Phase 1 覆盖 `pdf` / `doc` / `docx` / `xls` / `xlsx`；图片 / 音频 / 视频尚未实现，
    /// 此时返回可操作的错误提示（而不是静默走远端，避免 `local` 模式下误连不存在的侧车）。
    async fn parse_file_local(
        &self,
        file_path: &str,
        options: &serde_json::Value,
    ) -> Result<ParseResult, String> {
        let path = std::path::Path::new(file_path);
        let ext = parser::text::ext_of(path);
        if !parser::is_locally_supported_ext(&ext) {
            return Err(format!(
                "本地解析后端尚未支持 .{}（{}），请将 DOC_PARSER_BACKEND 设为 remote",
                ext,
                parser::detect_file_type(&ext).unwrap_or("unknown")
            ));
        }

        let parse_options = parser::ParseOptions::from_json(options);
        let result = parser::parse_file(path, parse_options).await?;
        info!(
            "[local] 解析成功: file={}, type={}, text_len={}, images={}",
            result.file_name,
            result.file_type,
            result.raw_text.len(),
            result.images.len()
        );
        Ok(result)
    }

    /// 远端后端：HTTP 调 doc-parser 侧车
    async fn parse_file_remote(
        &self,
        file_path: &str,
        options: &serde_json::Value,
    ) -> Result<ParseResult, String> {
        let url = format!("{}/parse", self.base_url);
        let body = serde_json::json!({
            "file_path": file_path,
            "options": options,
        });

        let mut req = self.http_client.post(&url).json(&body);
        if !self.token.is_empty() {
            req = req.header("X-API-Token", &self.token);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| format!("doc-parser 请求失败: {}", e))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| format!("doc-parser 读取响应失败: {}", e))?;

        if !status.is_success() {
            return Err(format!("doc-parser 解析失败 [{}]: {}", status, text));
        }

        match serde_json::from_str::<ParseResult>(&text) {
            Ok(result) => {
                info!(
                    "doc-parser 解析成功: file={}, type={}, text_len={}, images={}",
                    result.file_name,
                    result.file_type,
                    result.raw_text.len(),
                    result.images.len()
                );
                Ok(result)
            }
            Err(e) => Err(format!("doc-parser 响应解析失败: {}", e)),
        }
    }
}

// ======================== 侧车进程管理 ========================

/// 启动 doc-parser 侧车服务（Python FastAPI，`uvicorn`）
///
/// 由 Tauri 应用启动时自动拉起，监听 `127.0.0.1:8321`。
///
/// 配置（`.env.toml` 的 `[doc_parser]` 段，经 `load_env()` 转为环境变量）：
/// - `DOC_PARSER_ENABLED`: 是否启用（默认 true）
/// - `DOC_PARSER_PYTHON`: Python 解释器路径（默认 `aiagent` conda 环境，等价 `conda activate aiagent`）
/// - `DOC_PARSER_HOST` / `DOC_PARSER_PORT`: 监听地址（默认 `127.0.0.1:8321`）
/// - `DOC_PARSER_DIR`: doc-parser 代码目录（可选；默认沿本 crate 祖先路径查找
///   第一个包含 `doc-parser/main.py` 的目录）
///
/// 行为：
/// - 目标端口已被监听（服务已在运行）→ 跳过启动；
/// - 生成随机 `DOC_PARSER_TOKEN`（UUID v4）注入自身进程与子进程，
///   保证 `DocParserClient` 与 doc-parser 使用同一认证令牌；
/// - 将 Python 所在目录注入子进程 `PATH`，确保 `ffmpeg` / `ffprobe` 可用；
/// - 日志输出到 `/tmp/doc-parser.log`；
/// - **仅 `DOC_PARSER_BACKEND=remote` 时启动**；`local` 直接返回 `None`（不拉起 Python）。
pub fn start_doc_parser() -> Option<std::process::Child> {
    // 0. 本地后端无需 Python 侧车
    if backend() != Backend::Remote {
        tracing::info!(
            "[doc-parser] 当前 DOC_PARSER_BACKEND={}，跳过 Python 侧车启动",
            backend().as_str()
        );
        return None;
    }

    // 1. 是否启用
    if std::env::var("DOC_PARSER_ENABLED")
        .map(|v| v == "false" || v == "0")
        .unwrap_or(false)
    {
        tracing::info!("[doc-parser] 已通过 DOC_PARSER_ENABLED 配置禁用，跳过启动");
        return None;
    }

    let host = std::env::var("DOC_PARSER_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port: u16 = std::env::var("DOC_PARSER_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8321);

    // 2. 端口已被占用 → 服务已在运行，跳过（避免端口冲突 / 重复拉起）
    if std::net::TcpStream::connect((host.as_str(), port)).is_ok() {
        tracing::info!("[doc-parser] 检测到 {}:{} 已在运行，跳过启动", host, port);
        return None;
    }

    // 3. 定位 doc-parser 目录
    //    优先使用 DOC_PARSER_DIR 环境变量；否则从 CARGO_MANIFEST_DIR 沿祖先路径
    //    向上查找第一个包含 doc-parser/main.py 的目录。
    //    注意：本函数位于 workspace 成员 crate（crates/service）内，
    //    CARGO_MANIFEST_DIR 为 .../src-tauri/crates/service，
    //    不能按固定层级推导仓库根目录，故采用逐级向上查找。
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let Some(parser_dir) = std::env::var("DOC_PARSER_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            manifest_dir
                .ancestors()
                .map(|p| p.join("doc-parser"))
                .find(|p| p.join("main.py").exists())
        })
    else {
        tracing::warn!(
            "[doc-parser] 未定位到 doc-parser 目录（CARGO_MANIFEST_DIR={}），跳过启动",
            manifest_dir.display()
        );
        return None;
    };
    if !parser_dir.join("main.py").exists() {
        tracing::warn!(
            "[doc-parser] {} 下未找到 main.py，跳过启动",
            parser_dir.display()
        );
        return None;
    }

    // 4. Python 解释器（优先配置，默认 aiagent conda 环境）
    let python = std::env::var("DOC_PARSER_PYTHON")
        .unwrap_or_else(|_| "/home/ubuntu/conda/envs/aiagent/bin/python".to_string());

    // 5. 生成动态认证 token，注入自身进程（DocParserClient 读取）与子进程
    let token = uuid::Uuid::new_v4().to_string();
    std::env::set_var("DOC_PARSER_TOKEN", &token);

    // 6. PATH 注入 Python 所在目录（保证 ffmpeg/ffprobe 与 uvicorn 依赖可用）
    let python_path = std::path::Path::new(&python);
    let env_bin = python_path.parent().map(|p| p.to_path_buf());
    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = match &env_bin {
        Some(bin) => format!("{}:{}", bin.display(), current_path),
        None => current_path,
    };

    // 7. 日志重定向到 /tmp/doc-parser.log
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/doc-parser.log")
        .ok();

    // 8. spawn uvicorn
    let mut cmd = std::process::Command::new(&python);
    cmd.arg("-m")
        .arg("uvicorn")
        .arg("main:app")
        .arg("--host")
        .arg(&host)
        .arg("--port")
        .arg(port.to_string())
        .current_dir(&parser_dir)
        .env("DOC_PARSER_TOKEN", &token)
        .env("PARSER_HOST", &host)
        .env("PARSER_PORT", port.to_string())
        .env("PATH", &new_path);

    if let Some(f) = &log_file {
        if let Ok(dup) = f.try_clone() {
            cmd.stdout(std::process::Stdio::from(dup));
        }
        if let Ok(dup) = f.try_clone() {
            cmd.stderr(std::process::Stdio::from(dup));
        }
    } else {
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
    }

    match cmd.spawn() {
        Ok(child) => {
            tracing::info!(
                "[doc-parser] 已启动 (PID: {}): {} -m uvicorn main:app --host {} --port {}",
                child.id(),
                python,
                host,
                port
            );
            Some(child)
        }
        Err(e) => {
            tracing::error!("[doc-parser] 启动失败: {}", e);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_backend_value_defaults_to_remote() {
        assert_eq!(parse_backend_value(""), Backend::Remote, "❌ 未配置时必须保持改造前行为");
        assert_eq!(parse_backend_value(" LOCAL "), Backend::Local, "❌ 应容忍大小写与空白");
        assert_eq!(parse_backend_value("remote"), Backend::Remote);
        assert_eq!(parse_backend_value("typo"), Backend::Remote, "❌ 非法值应回落 remote");
    }

    #[test]
    fn test_backend_as_str() {
        assert_eq!(Backend::Local.as_str(), "local");
        assert_eq!(Backend::Remote.as_str(), "remote");
    }
}

