//! 统一模型 ↔ OpenAI 线上结构双向转换

use llm::{ChatRequest, ContentPart, ImageSource, Message, Role, ToolChoice};

use crate::wire::{
    ChatCompletionRequest, StreamOptions, WireFunctionOut, WireMessage, WireTool, WireToolCallOut,
    WireToolDef,
};

/// 构建请求体
///
/// - `stream`：是否流式
/// - `include_usage`：流式时是否索要末尾用量（部分兼容厂商不支持该字段）
pub fn build_request(
    request: &ChatRequest,
    stream: bool,
    include_usage: bool,
) -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: request.model.clone(),
        messages: messages_to_wire(&request.messages),
        temperature: request.temperature,
        top_p: request.top_p,
        max_tokens: request.max_tokens,
        stop: request.stop.clone(),
        tools: request.tools.iter().map(tool_to_wire).collect(),
        tool_choice: request.tool_choice.as_ref().map(tool_choice_to_wire),
        stream,
        stream_options: if stream && include_usage {
            Some(StreamOptions {
                include_usage: true,
            })
        } else {
            None
        },
        user: request.user.clone(),
    }
}

/// 消息列表转换（`tool_result` 会展开为独立的 `role=tool` 消息）
pub fn messages_to_wire(messages: &[Message]) -> Vec<WireMessage> {
    let mut out = Vec::new();

    for message in messages {
        let mut tool_calls = Vec::new();
        let mut blocks = Vec::new();
        let mut tool_results = Vec::new();

        for part in &message.content {
            match part {
                ContentPart::ToolUse { id, name, input } => tool_calls.push(WireToolCallOut {
                    id: id.clone(),
                    kind: "function",
                    function: WireFunctionOut {
                        name: name.clone(),
                        arguments: arguments_json(input),
                    },
                }),
                ContentPart::ToolResult {
                    tool_use_id,
                    content,
                    ..
                } => tool_results.push((tool_use_id.clone(), parts_text(content))),
                other => blocks.push(part_to_block(other)),
            }
        }

        // 工具结果必须走独立消息，不能再携带空 content
        if !tool_results.is_empty() {
            for (tool_call_id, text) in tool_results {
                out.push(WireMessage {
                    role: "tool",
                    content: Some(serde_json::Value::String(text)),
                    tool_calls: None,
                    tool_call_id: Some(tool_call_id),
                });
            }
            continue;
        }

        if message.role == Role::Tool {
            continue;
        }

        out.push(WireMessage {
            role: message.role.as_str(),
            content: content_value(&blocks),
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            tool_call_id: None,
        });
    }

    out
}

/// 内容片段 → `content` 字段
///
/// 单文本片段退化为字符串，兼容只接受 `content: "..."` 的老厂商。
fn content_value(blocks: &[serde_json::Value]) -> Option<serde_json::Value> {
    if blocks.is_empty() {
        return None;
    }
    if blocks.len() == 1 {
        if let Some(text) = blocks[0].get("text").and_then(serde_json::Value::as_str) {
            return Some(serde_json::Value::String(text.to_string()));
        }
    }
    Some(serde_json::Value::Array(blocks.to_vec()))
}

/// 单个内容片段 → OpenAI 内容块
fn part_to_block(part: &ContentPart) -> serde_json::Value {
    match part {
        ContentPart::Text { text } => serde_json::json!({ "type": "text", "text": text }),
        ContentPart::Image { source } => {
            let url = match source {
                ImageSource::Url { url } => url.clone(),
                // OpenAI 兼容协议统一用 data URL 传 base64
                ImageSource::Base64 { media_type, data } => {
                    format!("data:{media_type};base64,{data}")
                }
            };
            serde_json::json!({ "type": "image_url", "image_url": { "url": url } })
        }
        // 工具相关内容由 `messages_to_wire` 单独处理
        ContentPart::ToolUse { .. } | ContentPart::ToolResult { .. } => serde_json::Value::Null,
    }
}

/// 拼接片段中的纯文本（工具结果内容）
fn parts_text(parts: &[ContentPart]) -> String {
    let mut out = String::new();
    for part in parts {
        if let ContentPart::Text { text } = part {
            out.push_str(text);
        }
    }
    out
}

/// 工具入参序列化：字符串原样透传，其它按 JSON 序列化
fn arguments_json(input: &serde_json::Value) -> String {
    match input {
        serde_json::Value::String(raw) => raw.clone(),
        other => other.to_string(),
    }
}

/// 工具定义转换
fn tool_to_wire(tool: &llm::ToolDef) -> WireTool {
    WireTool {
        kind: "function",
        function: WireToolDef {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: tool.parameters.clone(),
        },
    }
}

/// 工具选择策略转换
fn tool_choice_to_wire(choice: &ToolChoice) -> serde_json::Value {
    match choice {
        ToolChoice::Auto => serde_json::json!("auto"),
        ToolChoice::None => serde_json::json!("none"),
        ToolChoice::Any | ToolChoice::Required => serde_json::json!("required"),
        ToolChoice::Named(name) => serde_json::json!({
            "type": "function",
            "function": { "name": name }
        }),
    }
}

use crate::wire::{ChatCompletionResponse, UsageBody};
use llm::{ChatResponse, StopReason, Usage};

/// 响应转换
pub fn response_to_unified(response: ChatCompletionResponse) -> ChatResponse {
    let choice = response.choices.into_iter().next();
    let stop_reason = choice
        .as_ref()
        .and_then(|choice| choice.finish_reason.as_deref())
        .map_or(StopReason::EndTurn, StopReason::from_openai);

    let mut content = Vec::new();
    if let Some(message) = choice.and_then(|choice| choice.message) {
        push_text_parts(&mut content, message.content);
        for call in message.tool_calls {
            let Some(function) = call.function else {
                continue;
            };
            let name = function.name.unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            content.push(ContentPart::ToolUse {
                id: call.id.unwrap_or_else(|| format!("call_{name}")),
                name,
                input: parse_arguments(function.arguments.as_deref()),
            });
        }
    }

    ChatResponse {
        id: response.id,
        model: response.model,
        content,
        stop_reason,
        usage: response.usage.as_ref().map_or(Usage::default(), to_usage),
    }
}

/// `content` 字段（字符串或片段数组）→ 文本片段
fn push_text_parts(content: &mut Vec<ContentPart>, raw: Option<serde_json::Value>) {
    match raw {
        Some(serde_json::Value::String(text)) => {
            if !text.is_empty() {
                content.push(ContentPart::Text { text });
            }
        }
        Some(serde_json::Value::Array(parts)) => {
            for part in parts {
                if let Some(text) = part.get("text").and_then(serde_json::Value::as_str) {
                    if !text.is_empty() {
                        content.push(ContentPart::Text {
                            text: text.to_string(),
                        });
                    }
                }
            }
        }
        _ => {}
    }
}

/// 工具参数 JSON 字符串 → `serde_json::Value`（非法 JSON 时保留原文，便于排查）
pub(crate) fn parse_arguments(raw: Option<&str>) -> serde_json::Value {
    match raw.map(str::trim) {
        None | Some("") => serde_json::json!({}),
        Some(text) => {
            serde_json::from_str(text).unwrap_or_else(|_| serde_json::json!({ "raw": text }))
        }
    }
}

/// 线上用量 → 统一用量
pub(crate) fn to_usage(body: &UsageBody) -> Usage {
    let cached = body
        .prompt_tokens_details
        .as_ref()
        .and_then(|details| details.cached_tokens);
    Usage {
        input_tokens: body.prompt_tokens,
        output_tokens: body.completion_tokens,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: cached,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm::{Message, ToolDef};

    #[test]
    fn text_only_messages_use_string_content() {
        let request = ChatRequest::new(
            "gpt-4o",
            vec![Message::system("你是助手"), Message::user("你好")],
        )
        .with_temperature(0.3)
        .with_max_tokens(256);

        let wire = build_request(&request, false, false);
        assert_eq!(wire.model, "gpt-4o");
        assert_eq!(wire.temperature, Some(0.3));
        assert_eq!(wire.max_tokens, Some(256));
        assert!(!wire.stream);
        assert!(wire.stream_options.is_none());

        let json = serde_json::to_value(&wire).unwrap();
        assert_eq!(json["messages"][0]["role"], "system");
        // 单文本片段退化为字符串，兼容老厂商
        assert_eq!(json["messages"][0]["content"], "你是助手");
        assert_eq!(json["messages"][1]["content"], "你好");
    }

    #[test]
    fn multimodal_message_produces_image_url_block() {
        let message = Message::new(
            Role::User,
            vec![
                ContentPart::text("这张图是什么？"),
                ContentPart::image_base64("image/png", "QUJD"),
            ],
        );
        let wire = messages_to_wire(&[message]);
        let json = serde_json::to_value(&wire).unwrap();

        assert_eq!(json[0]["content"][0]["type"], "text");
        assert_eq!(json[0]["content"][1]["type"], "image_url");
        assert_eq!(
            json[0]["content"][1]["image_url"]["url"],
            "data:image/png;base64,QUJD"
        );
    }

    #[test]
    fn tool_result_expands_to_tool_role_message() {
        let assistant = Message::new(
            Role::Assistant,
            vec![ContentPart::ToolUse {
                id: "call_1".to_string(),
                name: "lookup_asset".to_string(),
                input: serde_json::json!({ "id": 42 }),
            }],
        );
        let tool = Message::tool_result("call_1", "资产名称：打印机");

        let wire = messages_to_wire(&[assistant, tool]);
        let json = serde_json::to_value(&wire).unwrap();

        // assistant 只带 tool_calls，不发送空 content
        assert!(json[0].get("content").is_none());
        assert_eq!(json[0]["tool_calls"][0]["id"], "call_1");
        assert_eq!(
            json[0]["tool_calls"][0]["function"]["arguments"],
            "{\"id\":42}"
        );
        // 工具结果独立成 role=tool 消息
        assert_eq!(json[1]["role"], "tool");
        assert_eq!(json[1]["tool_call_id"], "call_1");
        assert_eq!(json[1]["content"], "资产名称：打印机");
    }

    #[test]
    fn tool_definitions_and_choice_are_mapped() {
        let request = ChatRequest::new("gpt-4o", vec![Message::user("查资产")])
            .with_tools(vec![
                ToolDef::new("lookup_asset", serde_json::json!({ "type": "object" }))
                    .with_description("按 ID 查资产"),
            ])
            .with_tool_choice(ToolChoice::Named("lookup_asset".to_string()));

        let wire = build_request(&request, false, false);
        let json = serde_json::to_value(&wire).unwrap();
        assert_eq!(json["tools"][0]["type"], "function");
        assert_eq!(json["tools"][0]["function"]["name"], "lookup_asset");
        assert_eq!(
            json["tools"][0]["function"]["description"],
            "按 ID 查资产"
        );
        assert_eq!(json["tool_choice"]["function"]["name"], "lookup_asset");
        assert_eq!(tool_choice_to_wire(&ToolChoice::Any), "required");
        assert_eq!(tool_choice_to_wire(&ToolChoice::Auto), "auto");
    }

    #[test]
    fn stream_request_asks_for_usage_only_when_supported() {
        let request = ChatRequest::new("gpt-4o", vec![Message::user("hi")]);
        let with_usage = serde_json::to_value(build_request(&request, true, true)).unwrap();
        assert_eq!(with_usage["stream"], true);
        assert_eq!(with_usage["stream_options"]["include_usage"], true);

        let without_usage = serde_json::to_value(build_request(&request, true, false)).unwrap();
        assert!(without_usage.get("stream_options").is_none());
    }

    #[test]
    fn response_is_normalized_with_tool_calls_and_cached_tokens() {
        let raw = r#"{
            "id": "chatcmpl-1",
            "model": "gpt-4o",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "我来查",
                    "tool_calls": [{
                        "id": "call_9",
                        "type": "function",
                        "function": { "name": "lookup_asset", "arguments": "{\"id\":42}" }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 12,
                "completion_tokens": 8,
                "total_tokens": 20,
                "prompt_tokens_details": { "cached_tokens": 5 }
            }
        }"#;

        let parsed: ChatCompletionResponse = serde_json::from_str(raw).unwrap();
        let response = response_to_unified(parsed);

        assert_eq!(response.id, "chatcmpl-1");
        assert_eq!(response.text(), "我来查");
        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.usage.input_tokens, 12);
        assert_eq!(response.usage.output_tokens, 8);
        assert_eq!(response.usage.cache_read_input_tokens, Some(5));

        let calls = response.tool_uses();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "call_9");
        assert_eq!(calls[0].2["id"], 42);
    }

    #[test]
    fn invalid_tool_arguments_keep_raw_text() {
        assert_eq!(parse_arguments(Some("")), serde_json::json!({}));
        assert_eq!(parse_arguments(None), serde_json::json!({}));
        assert_eq!(parse_arguments(Some("{\"a\":1}")), serde_json::json!({"a":1}));
        assert_eq!(
            parse_arguments(Some("not-json")),
            serde_json::json!({ "raw": "not-json" })
        );
    }
}


