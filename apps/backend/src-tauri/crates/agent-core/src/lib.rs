//! Agent 纯逻辑（零 IO）
//!
//! - [`prompt`]：系统提示词与用户消息拼装（从 `workflow/src/agent.rs` 逐字抽取）；
//! - [`action`]：LLM 返回的 JSON 动作解析与校验。
//!
//! 本 crate **不允许**依赖 `tokio` / `reqwest` / `sqlx` / `database` / `adk-graph`，
//! 以便沙箱侧 `agent-runner` 复用同一份协议实现。

pub mod action;
pub mod prompt;
pub mod protocol;
