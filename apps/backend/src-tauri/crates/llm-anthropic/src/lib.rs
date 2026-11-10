//! Anthropic Messages 协议实现（Claude）
//!
//! 与 OpenAI 协议的关键差异都在本 crate 内消化，上层只看到 `llm` 的统一类型：
//!
//! | 差异 | OpenAI | Anthropic |
//! |------|--------|-----------|
//! | system 提示 | `messages` 里的一条消息 | 顶层 `system` 字段（多条合并） |
//! | `max_tokens` | 可选 | **必填**（默认取 `default_max_tokens`） |
//! | 图片 | `image_url` / data URL | 只能 base64 `source`（URL 返回 `Unsupported`） |
//! | 工具调用 | `tool_calls` + `role=tool` | `tool_use` / `tool_result` 内容块 |
//! | 结束原因 | `finish_reason` | `stop_reason` |
//! | 流式 | `data: {...}` 匿名帧 | 命名事件（`message_start` / `content_block_delta` ...） |
//!
//! ```no_run
//! use llm::LlmClientConfig;
//! use llm_anthropic::{AnthropicOptions, AnthropicProvider};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let provider = AnthropicProvider::new(AnthropicOptions::new(
//!     "claude",
//!     LlmClientConfig::new("https://api.anthropic.com", "sk-ant-xxx", "claude-sonnet-4-20250514"),
//! ))?;
//! # let _ = provider;
//! # Ok(())
//! # }
//! ```

mod client;
mod convert;
mod error;
mod sse;
mod wire;

pub use client::{AnthropicOptions, AnthropicProvider, normalize_base_url};
pub use error::map_error;
pub use sse::{MessageStreamParser, SseDecoder, SseFrame};
