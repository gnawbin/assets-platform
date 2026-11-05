//! OpenAI Chat Completions 协议实现
//!
//! 覆盖两类厂商：
//! - **原生 OpenAI**：`/chat/completions`、`/embeddings`、`/models`；
//! - **OpenAI 兼容**：Qwen（DashScope 兼容模式）、DeepSeek、火山方舟、腾讯混元、
//!   Ollama、GLM、Moonshot、OpenRouter、Groq、xAI、SiliconFlow。
//!
//! 端点由 `base_url` 决定（版本段写在 base_url 里，见 `llm::presets`），
//! 因此火山 `/api/v3`、GLM `/api/paas/v4` 这类非 `/v1` 路径无需特判。
//!
//! ```no_run
//! use llm::LlmClientConfig;
//! use llm_openai::{OpenAiOptions, OpenAiProvider};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let provider = OpenAiProvider::new(OpenAiOptions::new(
//!     "openai",
//!     LlmClientConfig::new("https://api.openai.com/v1", "sk-xxx", "gpt-4o"),
//! ))?;
//! # let _ = provider;
//! # Ok(())
//! # }
//! ```
//!
//! # 归一化职责
//! - 请求侧：把 `llm::Message` 转成 `messages[]`（含 `image_url` 多模态、
//!   `tool_calls` / `role=tool` 工具往返）；
//! - 响应侧：`finish_reason` → `StopReason`、`cached_tokens` → 缓存用量；
//! - 流式：`data: {...}` 分帧 + 工具参数按 `index` 累积，统一成 `StreamEvent`。

mod client;
mod convert;
mod error;
mod sse;
mod wire;

pub use client::{AuthStyle, MaxTokensField, OpenAiOptions, OpenAiProvider, normalize_base_url};
pub use error::map_error;
pub use sse::{ChunkParser, SseDecoder, SseFrame};
