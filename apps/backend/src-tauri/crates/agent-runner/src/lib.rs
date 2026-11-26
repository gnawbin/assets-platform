//! `agent-runner`：`OpenShell` 沙箱内的单步 agent 执行器
//!
//! - [`llm`]：调用平台内部 LLM 端点（L1）；
//! - [`tools`]：本地工具（`sandbox_exec` / `file_read` / `file_write`），路径限制在 workspace 内；
//! - [`runner`]：单步状态机（LLM → 动作 → 本地工具 / 中断）。
//!
//! 依赖纪律：**禁止**依赖 `service` / `database` / `sqlx` / `axum` / `workflow`，
//! 避免平台密钥与 DB 依赖被带进沙箱镜像。

pub mod llm;
pub mod runner;
pub mod tools;
