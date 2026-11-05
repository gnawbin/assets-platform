//! LLM 协议契约（OpenAI Chat Completions / Anthropic Messages 双协议统一层）
//!
//! 本 crate 只定义**契约**，不含任何 HTTP 依赖：
//! - [`model`]：协议无关的会话模型（`ChatRequest` / `ChatResponse` /
//!   `ContentPart` / `ToolDef` / `StreamEvent` / `Usage` ...）；
//! - [`error`]：`LlmError`，按「可重试 / 需人工修配置」分十类，供上层熔断与重试决策；
//! - [`provider`]：`LlmProvider` trait 与 `Protocol`，两套线上协议的实现都收敛到它；
//! - [`config`]：`LlmClientConfig`，`Debug` 输出对密钥脱敏；
//! - [`presets`]：厂商预设表（provider_code → 协议 / base_url / 默认模型）。
//!
//! # 实现分布
//! - `llm-openai`：OpenAI Chat Completions + Embeddings（Qwen / DeepSeek / 火山 /
//!   混元 / Ollama / GLM / Moonshot / OpenRouter / Groq / xAI / SiliconFlow 均走此实现）
//! - `llm-anthropic`：Anthropic Messages API（含 SSE 事件归一化）
//!
//! 两个实现 crate 各自持有自己的 `reqwest`，也不回灌驱动到 `crates/service`。
//!
//! ```bash
//! cargo test -p llm
//! ```
//!
//! # 归一化约定
//! 所有厂商差异（`system` 提取、`tool_use`/`tool_result` 语义、`stop_reason` 取值、
//! SSE 事件名、缓存 token 字段）都在实现 crate 内消化，上层只看到 [`model`] 里的类型。

pub mod config;
pub mod error;
pub mod model;
pub mod presets;
pub mod provider;

pub use config::LlmClientConfig;
pub use error::{LlmError, LlmResult};
pub use model::{
    ChatRequest, ChatResponse, ContentPart, EmbeddingRequest, EmbeddingResponse, ImageSource,
    Message, ModelInfo, Role, StopReason, StreamEvent, ToolChoice, ToolDef, Usage,
};
pub use presets::{PRESETS, ProviderPreset, default_base_url, default_model, lookup_preset};
pub use provider::{ChatStream, LlmProvider, Protocol};

/// 装箱后的 LLM 协议实现（便于在工厂 / 注册表中按协议选择实现）
pub type BoxedProvider = Box<dyn LlmProvider>;
