//! Provider 契约：`Protocol` + `LlmProvider`
//!
//! 两个协议 crate 都实现同一个 trait，上层（`service::llm_gateway_service`）只依赖它，
//! 因此新增厂商 = 在 `presets` 里加一行，不需要动路由代码。

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

use crate::error::{LlmError, LlmResult};
use crate::model::{
    ChatRequest, ChatResponse, EmbeddingRequest, EmbeddingResponse, ModelInfo, StreamEvent,
};

/// 归一化后的流式事件流
pub type ChatStream = Pin<Box<dyn Stream<Item = LlmResult<StreamEvent>> + Send>>;

/// 线上协议族
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    /// OpenAI Chat Completions（含所有 OpenAI 兼容厂商）
    OpenAiCompatible,
    /// Anthropic Messages
    AnthropicMessages,
}

impl Protocol {
    /// 稳定字符串（用于日志与配置展示）
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openai",
            Self::AnthropicMessages => "anthropic",
        }
    }
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// LLM 厂商适配器
///
/// 能力分三档：
/// - **必须实现**：`chat` / `chat_stream`（对话与流式对话）；
/// - **可选实现**：`embed`（Anthropic 无此能力，默认返回 `Unsupported`）；
/// - **默认实现**：`list_models` / `health_check`。
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// 厂商编码（与 `llm_provider.provider_code` 对应）
    fn provider_code(&self) -> &str;

    /// 线上协议族
    fn protocol(&self) -> Protocol;

    /// 未在请求中指定模型时使用的默认模型
    fn default_model(&self) -> &str;

    /// 非流式对话
    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatResponse>;

    /// 流式对话：返回归一化事件流
    async fn chat_stream(&self, request: ChatRequest) -> LlmResult<ChatStream>;

    /// 文本向量化（不支持时返回 [`LlmError::Unsupported`]）
    async fn embed(&self, request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        let _ = request;
        Err(LlmError::unsupported(format!(
            "{} 不支持 embedding",
            self.provider_code()
        )))
    }

    /// 模型列表（不支持时返回 [`LlmError::Unsupported`]）
    async fn list_models(&self) -> LlmResult<Vec<ModelInfo>> {
        Err(LlmError::unsupported(format!(
            "{} 不支持模型列表接口",
            self.provider_code()
        )))
    }

    /// 健康检查（默认视为健康，由实现覆盖为真实探测）
    async fn health_check(&self) -> LlmResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_exposes_stable_names() {
        assert_eq!(Protocol::OpenAiCompatible.as_str(), "openai");
        assert_eq!(Protocol::AnthropicMessages.as_str(), "anthropic");
        assert_eq!(Protocol::AnthropicMessages.to_string(), "anthropic");
    }
}
