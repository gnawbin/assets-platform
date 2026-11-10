//! LLM 厂商工厂 + DTO 桥接
//!
//! 只做三件事：
//! 1. `provider_code` → 预设（协议 / base_url / 默认模型），预设表在 `crates/llm`；
//! 2. API Key 解密（收敛原先散落在 gateway 与 provider_service 的两处启发式）；
//! 3. 依据协议选择 `llm-openai` / `llm-anthropic` 实现。
//!
//! 另提供 `database::models` DTO ↔ `llm` 统一类型的桥接，保证 Tauri 命令与
//! 前端契约零改动。

use std::sync::Arc;

use database::models::LlmProvider;
use llm::{LlmClientConfig, LlmError, LlmProvider as LlmProviderTrait, Protocol};
use llm_anthropic::{AnthropicOptions, AnthropicProvider};
use llm_openai::{OpenAiOptions, OpenAiProvider};

/// 厂商默认 base_url（未知厂商回落本地 Ollama，与旧行为一致）
pub fn get_default_base_url(provider_code: &str) -> String {
    llm::default_base_url(provider_code)
        .unwrap_or("http://localhost:11434/v1")
        .to_string()
}

/// 厂商默认模型（数据库中未配置模型时使用）
pub fn get_default_model(provider_code: &str) -> String {
    llm::default_model(provider_code)
        .unwrap_or("gpt-4o")
        .to_string()
}

/// 解密 API Key
///
/// 与旧实现保持一致：长度可疑（> 20）时才尝试解密，失败则按明文处理，
/// 兼容历史数据与手工写入的明文密钥。
pub fn resolve_api_key(raw: Option<&String>) -> String {
    match raw {
        Some(key) if !key.is_empty() && key.len() > 20 => {
            utils::crypto::decrypt_api_key(key).unwrap_or_else(|_| key.clone())
        }
        Some(key) => key.clone(),
        None => String::new(),
    }
}

/// 依据厂商配置构造协议实现
///
/// - `default_model`：该条目在 `llm_model` 表中配置的模型编码（可空，空则用预设值）
pub fn create_provider(
    provider: &LlmProvider,
    default_model: Option<&str>,
) -> Result<Arc<dyn LlmProviderTrait>, LlmError> {
    let preset = llm::lookup_preset(&provider.provider_code).ok_or_else(|| {
        LlmError::config(format!("不支持的厂商类型：{}", provider.provider_code))
    })?;

    let base_url = provider
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| preset.base_url.to_string());

    let model = default_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| preset.default_model.to_string());

    let api_key = resolve_api_key(provider.api_key.as_ref());
    let config = LlmClientConfig::new(base_url, api_key, model);

    match preset.protocol {
        Protocol::OpenAiCompatible => {
            let options = OpenAiOptions::new(provider.provider_code.clone(), config);
            Ok(Arc::new(OpenAiProvider::new(options)?))
        }
        Protocol::AnthropicMessages => {
            let options = AnthropicOptions::new(provider.provider_code.clone(), config);
            Ok(Arc::new(AnthropicProvider::new(options)?))
        }
    }
}

use database::models::{
    ChatMessage, ContentPart as DbContentPart, LLMChatRequest, LLMChatResponse, LLMEmbeddingRequest,
    LLMEmbeddingResponse, TokenUsage,
};
use llm::{ChatRequest, ChatResponse, ContentPart, Message, Role};

/// `database::models::LLMChatRequest` → `llm::ChatRequest`
///
/// `default_model` 由调用方（Router 中选中的 Provider 条目）提供。
pub fn to_llm_request(request: &LLMChatRequest, default_model: &str) -> ChatRequest {
    let model = request
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_model.to_string());

    let mut chat = ChatRequest::new(model, to_llm_messages(&request.messages));
    chat.temperature = request.temperature;
    chat.max_tokens = request.max_tokens;
    chat.stream = request.stream.unwrap_or(false);
    chat.user = request.user_id.map(|id| id.to_string());
    chat
}

/// 消息列表桥接：`content_parts` 优先于纯文本（与旧实现一致）
fn to_llm_messages(messages: &[ChatMessage]) -> Vec<Message> {
    messages
        .iter()
        .map(|message| {
            let role = Role::from_wire(&message.role);
            let content = match &message.content_parts {
                Some(parts) if !parts.is_empty() => parts.iter().map(to_llm_part).collect(),
                _ => vec![ContentPart::text(message.content.clone())],
            };
            Message::new(role, content)
        })
        .collect()
}

/// 单片段桥接
fn to_llm_part(part: &DbContentPart) -> ContentPart {
    match part {
        DbContentPart::Text { text, .. } => ContentPart::text(text.clone()),
        DbContentPart::Image { image_url, .. } => image_part_from_url(&image_url.url),
    }
}

/// 图片 URL → 统一片段
///
/// - `data:image/png;base64,xxxx` → [`ContentPart::image_base64`]（Anthropic 必需）
/// - `http(s)://...` → [`ContentPart::image_url`]（OpenAI 兼容厂商可直接吃）
pub fn image_part_from_url(url: &str) -> ContentPart {
    if let Some(rest) = url.strip_prefix("data:") {
        if let Some((meta, data)) = rest.split_once(',') {
            let media_type = meta.trim_end_matches(";base64").trim();
            if !media_type.is_empty() && !data.is_empty() {
                return ContentPart::image_base64(media_type, data);
            }
        }
    }
    ContentPart::image_url(url)
}

/// `llm::ChatResponse` → `database::models::LLMChatResponse`
///
/// 文本与工具调用统一收成 `content` 字符串（DTO 不携带工具语义），
/// 用量换算成 `TokenUsage`（`cost` 由价格表另行计算）。
pub fn to_db_response(provider_id: i64, model_id: i64, response: ChatResponse) -> LLMChatResponse {
    let usage = response.usage;
    LLMChatResponse {
        content: response.text(),
        model: response.model,
        usage: TokenUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total(),
            cost: 0.0,
        },
        provider_id,
        model_id,
        request_id: response.id,
    }
}

/// `database::models::LLMEmbeddingRequest` → `llm::EmbeddingRequest`
pub fn to_llm_embedding(
    request: &LLMEmbeddingRequest,
    default_model: &str,
) -> llm::EmbeddingRequest {
    let model = request
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_model.to_string());
    llm::EmbeddingRequest::new(model, request.input.clone())
}

/// `llm::EmbeddingResponse` → `database::models::LLMEmbeddingResponse`
pub fn to_db_embedding(
    provider_id: i64,
    model_id: i64,
    response: llm::EmbeddingResponse,
) -> LLMEmbeddingResponse {
    let usage = response.usage;
    LLMEmbeddingResponse {
        embeddings: response.embeddings,
        model: response.model,
        usage: TokenUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total(),
            cost: 0.0,
        },
        provider_id,
        model_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::models::ImageUrl;

    #[test]
    fn preset_lookups_cover_known_and_unknown_vendors() {
        assert_eq!(
            get_default_base_url("deepseek"),
            "https://api.deepseek.com/v1"
        );
        assert_eq!(get_default_model("claude"), "claude-sonnet-4-20250514");
        // 未知厂商回落本地 Ollama / gpt-4o，保持旧行为
        assert_eq!(
            get_default_base_url("no-such-vendor"),
            "http://localhost:11434/v1"
        );
        assert_eq!(get_default_model("no-such-vendor"), "gpt-4o");
    }

    #[test]
    fn data_url_becomes_base64_part() {
        match image_part_from_url("data:image/png;base64,QUJD") {
            ContentPart::Image {
                source: llm::ImageSource::Base64 { media_type, data },
            } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "QUJD");
            }
            other => panic!("期望 base64 片段，实际 {other:?}"),
        }
    }

    #[test]
    fn http_url_stays_a_url_part() {
        match image_part_from_url("https://example.com/a.jpg") {
            ContentPart::Image {
                source: llm::ImageSource::Url { url },
            } => assert_eq!(url, "https://example.com/a.jpg"),
            other => panic!("期望 URL 片段，实际 {other:?}"),
        }
    }

    #[test]
    fn content_parts_take_precedence_over_plain_text() {
        let request = LLMChatRequest {
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "被忽略的纯文本".to_string(),
                content_parts: Some(vec![
                    DbContentPart::Text {
                        r#type: "text".to_string(),
                        text: "看图".to_string(),
                    },
                    DbContentPart::Image {
                        r#type: "image_url".to_string(),
                        image_url: ImageUrl {
                            url: "data:image/jpeg;base64,QUJD".to_string(),
                        },
                    },
                ]),
            }],
            model: None,
            temperature: Some(0.5),
            max_tokens: Some(256),
            stream: Some(false),
            user_id: Some(7),
            conv_id: None,
        };

        let chat = to_llm_request(&request, "gpt-4o");
        assert_eq!(chat.model, "gpt-4o");
        assert_eq!(chat.temperature, Some(0.5));
        assert_eq!(chat.max_tokens, Some(256));
        assert_eq!(chat.user.as_deref(), Some("7"));
        assert_eq!(chat.messages.len(), 1);
        assert_eq!(chat.messages[0].role, Role::User);
        assert_eq!(chat.messages[0].content.len(), 2);
        assert_eq!(chat.messages[0].text(), "看图");
    }

    #[test]
    fn explicit_model_wins_over_default() {
        let request = LLMChatRequest {
            messages: vec![ChatMessage {
                role: "assistant".to_string(),
                content: "hi".to_string(),
                content_parts: None,
            }],
            model: Some(" deepseek-chat ".to_string()),
            temperature: None,
            max_tokens: None,
            stream: None,
            user_id: None,
            conv_id: None,
        };
        let chat = to_llm_request(&request, "gpt-4o");
        assert_eq!(chat.model, "deepseek-chat");
        assert_eq!(chat.messages[0].role, Role::Assistant);
        assert!(!chat.stream);
    }

    #[test]
    fn db_response_aggregates_text_and_usage() {
        let response = ChatResponse {
            id: "chatcmpl-1".to_string(),
            model: "gpt-4o".to_string(),
            content: vec![
                ContentPart::text("你好"),
                ContentPart::text("，世界"),
                ContentPart::ToolUse {
                    id: "call_1".to_string(),
                    name: "lookup".to_string(),
                    input: serde_json::json!({}),
                },
            ],
            stop_reason: llm::StopReason::ToolUse,
            usage: llm::Usage {
                input_tokens: 11,
                output_tokens: 7,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: Some(3),
            },
        };

        let dto = to_db_response(2, 5, response);
        assert_eq!(dto.content, "你好，世界");
        assert_eq!(dto.model, "gpt-4o");
        assert_eq!(dto.usage.input_tokens, 11);
        assert_eq!(dto.usage.output_tokens, 7);
        assert_eq!(dto.usage.total_tokens, 18);
        assert_eq!(dto.provider_id, 2);
        assert_eq!(dto.model_id, 5);
        assert_eq!(dto.request_id, "chatcmpl-1");
    }
}


