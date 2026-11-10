//! 统一模型 ↔ Anthropic Messages 双向转换

use llm::{
    ChatRequest, ContentPart, ImageSource, LlmError, LlmResult, Message, Role, ToolChoice,
};

use crate::wire::{MessagesRequest, WireMessage, WireTool};

/// 构建请求体
///
/// - `default_max_tokens`：Anthropic 要求 `max_tokens` 必填，请求未指定时用它兜底
pub fn build_request(
    request: &ChatRequest,
    default_max_tokens: i32,
) -> LlmResult<MessagesRequest> {
    let (tools, tool_choice) = tools_to_wire(request);

    Ok(MessagesRequest {
        model: request.model.clone(),
        max_tokens: request.max_tokens.unwrap_or(default_max_tokens),
        messages: messages_to_wire(&request.messages)?,
        system: collect_system(&request.messages),
        temperature: request.temperature,
        top_p: request.top_p,
        stop_sequences: request.stop.clone(),
        tools,
        tool_choice,
        stream: request.stream,
    })
}

/// 合并**全部** system 消息为顶层 `system` 字段
///
/// 旧实现只取第一条 system 并丢弃其余，这里改为按顺序拼接（非文本片段忽略，
/// Anthropic 的 system 只支持文本）。
fn collect_system(messages: &[Message]) -> Option<serde_json::Value> {
    let mut merged = String::new();
    for message in messages.iter().filter(|message| message.role == Role::System) {
        for part in &message.content {
            if let ContentPart::Text { text } = part {
                if !merged.is_empty() {
                    merged.push('\n');
                }
                merged.push_str(text);
            }
        }
    }
    if merged.is_empty() {
        None
    } else {
        Some(serde_json::Value::String(merged))
    }
}

/// 消息列表转换：连续同角色自动合并，`tool_result` 归入 user 轮
fn messages_to_wire(messages: &[Message]) -> LlmResult<Vec<WireMessage>> {
    let mut acc: Vec<(&'static str, Vec<serde_json::Value>)> = Vec::new();

    for message in messages.iter().filter(|m| m.role != Role::System) {
        let mut blocks = Vec::new();
        for part in &message.content {
            if let Some(block) = part_to_block(part)? {
                blocks.push(block);
            }
        }
        if blocks.is_empty() {
            continue;
        }

        // 工具结果与普通用户消息都属于 user 轮
        let role = match message.role {
            Role::Assistant => "assistant",
            _ => "user",
        };

        match acc.last_mut() {
            Some((last_role, last_blocks)) if *last_role == role => last_blocks.extend(blocks),
            _ => acc.push((role, blocks)),
        }
    }

    Ok(acc
        .into_iter()
        .map(|(role, content)| WireMessage { role, content })
        .collect())
}

/// 单个内容片段 → Anthropic 内容块
fn part_to_block(part: &ContentPart) -> LlmResult<Option<serde_json::Value>> {
    Ok(Some(match part {
        ContentPart::Text { text } => serde_json::json!({ "type": "text", "text": text }),
        ContentPart::Image { source } => match source {
            ImageSource::Base64 { media_type, data } => serde_json::json!({
                "type": "image",
                "source": { "type": "base64", "media_type": media_type, "data": data }
            }),
            // Messages API 只接受 base64 source，URL 需调用方先下载
            ImageSource::Url { .. } => {
                return Err(LlmError::unsupported(
                    "Anthropic Messages 不接受图片 URL，请先下载并转为 base64",
                ))
            }
        },
        ContentPart::ToolUse { id, name, input } => serde_json::json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": input
        }),
        ContentPart::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => serde_json::json!({
            "type": "tool_result",
            "tool_use_id": tool_use_id,
            "content": [{ "type": "text", "text": parts_text(content) }],
            "is_error": is_error
        }),
    }))
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

/// 工具定义 + 选择策略转换
///
/// `ToolChoice::None` 或没有工具时，`tools` 与 `tool_choice` 都不发送
/// （Anthropic 对空 tools 数组会报 400）。
fn tools_to_wire(request: &ChatRequest) -> (Vec<WireTool>, Option<serde_json::Value>) {
    if request.tools.is_empty() || matches!(request.tool_choice, Some(ToolChoice::None)) {
        return (Vec::new(), None);
    }

    let tools = request
        .tools
        .iter()
        .map(|tool| WireTool {
            name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.parameters.clone(),
        })
        .collect();

    let choice = request.tool_choice.as_ref().map(|choice| match choice {
        ToolChoice::Auto | ToolChoice::None => serde_json::json!({ "type": "auto" }),
        ToolChoice::Any | ToolChoice::Required => serde_json::json!({ "type": "any" }),
        ToolChoice::Named(name) => serde_json::json!({ "type": "tool", "name": name }),
    });

    (tools, choice)
}

use crate::wire::{ContentBlock, MessagesResponse, UsageBody};
use llm::{ChatResponse, StopReason, Usage};

/// 响应转换
pub fn response_to_unified(response: MessagesResponse) -> ChatResponse {
    let mut content = Vec::new();
    for block in response.content {
        match block {
            ContentBlock::Text { text } => {
                if !text.is_empty() {
                    content.push(ContentPart::Text { text });
                }
            }
            ContentBlock::ToolUse { id, name, input } => {
                content.push(ContentPart::ToolUse { id, name, input });
            }
            ContentBlock::Unknown => {}
        }
    }

    ChatResponse {
        id: response.id,
        model: response.model,
        content,
        stop_reason: response
            .stop_reason
            .as_deref()
            .map_or(StopReason::EndTurn, StopReason::from_anthropic),
        usage: response.usage.as_ref().map_or(Usage::default(), to_usage),
    }
}

/// 线上用量 → 统一用量
pub(crate) fn to_usage(body: &UsageBody) -> Usage {
    Usage {
        input_tokens: body.input_tokens,
        output_tokens: body.output_tokens,
        cache_creation_input_tokens: body.cache_creation_input_tokens,
        cache_read_input_tokens: body.cache_read_input_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm::{Message, ToolDef};

    #[test]
    fn system_messages_are_merged_and_excluded_from_messages() {
        let request = ChatRequest::new(
            "claude-sonnet-4-20250514",
            vec![
                Message::system("你是资产管理员"),
                Message::system("回答要简洁"),
                Message::user("你好"),
            ],
        );

        let wire = build_request(&request, 4096).unwrap();
        assert_eq!(
            wire.system,
            Some(serde_json::json!("你是资产管理员\n回答要简洁"))
        );
        assert_eq!(wire.messages.len(), 1);
        assert_eq!(wire.messages[0].role, "user");
    }

    #[test]
    fn consecutive_same_role_messages_are_merged() {
        let request = ChatRequest::new(
            "claude-sonnet-4-20250514",
            vec![
                Message::user("第一句"),
                Message::user("第二句"),
                Message::assistant("收到"),
            ],
        );

        let wire = build_request(&request, 4096).unwrap();
        assert_eq!(wire.messages.len(), 2);
        let json = serde_json::to_value(&wire).unwrap();
        // 两条 user 合并成一个 content 数组
        assert_eq!(json["messages"][0]["content"].as_array().unwrap().len(), 2);
        assert_eq!(json["messages"][1]["role"], "assistant");
    }

    #[test]
    fn max_tokens_defaults_when_absent() {
        let request = ChatRequest::new("c", vec![Message::user("hi")]);
        assert_eq!(build_request(&request, 4096).unwrap().max_tokens, 4096);
        assert_eq!(
            build_request(&request.with_max_tokens(128), 4096)
                .unwrap()
                .max_tokens,
            128
        );
    }

    #[test]
    fn base64_image_becomes_anthropic_source_block() {
        let request = ChatRequest::new(
            "c",
            vec![Message::new(
                Role::User,
                vec![
                    ContentPart::text("看图"),
                    ContentPart::image_base64("image/jpeg", "QUJD"),
                ],
            )],
        );

        let json = serde_json::to_value(build_request(&request, 4096).unwrap()).unwrap();
        let blocks = json["messages"][0]["content"].as_array().unwrap();
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[1]["source"]["type"], "base64");
        assert_eq!(blocks[1]["source"]["media_type"], "image/jpeg");
        assert_eq!(blocks[1]["source"]["data"], "QUJD");
    }

    #[test]
    fn image_url_is_reported_as_unsupported() {
        let request = ChatRequest::new(
            "c",
            vec![Message::new(
                Role::User,
                vec![ContentPart::image_url("https://example.com/a.png")],
            )],
        );
        assert!(matches!(
            build_request(&request, 4096),
            Err(LlmError::Unsupported(_))
        ));
    }

    #[test]
    fn tool_use_and_tool_result_are_mapped_to_content_blocks() {
        let assistant = Message::new(
            Role::Assistant,
            vec![ContentPart::ToolUse {
                id: "toolu_1".to_string(),
                name: "lookup_asset".to_string(),
                input: serde_json::json!({ "id": 42 }),
            }],
        );
        let tool = Message::tool_result("toolu_1", "资产：打印机");

        let request = ChatRequest::new("c", vec![Message::user("查资产"), assistant, tool]);
        let json = serde_json::to_value(build_request(&request, 4096).unwrap()).unwrap();

        // assistant 轮里是 tool_use 块
        assert_eq!(json["messages"][1]["role"], "assistant");
        assert_eq!(json["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(json["messages"][1]["content"][0]["name"], "lookup_asset");
        // tool_result 归入 user 轮
        assert_eq!(json["messages"][2]["role"], "user");
        assert_eq!(json["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(json["messages"][2]["content"][0]["tool_use_id"], "toolu_1");
        assert_eq!(
            json["messages"][2]["content"][0]["content"][0]["text"],
            "资产：打印机"
        );
    }

    #[test]
    fn tools_use_input_schema_and_choice_types() {
        let request = ChatRequest::new("c", vec![Message::user("查")])
            .with_tools(vec![
                ToolDef::new("lookup_asset", serde_json::json!({ "type": "object" }))
                    .with_description("按 ID 查资产"),
            ])
            .with_tool_choice(ToolChoice::Any);

        let json = serde_json::to_value(build_request(&request, 4096).unwrap()).unwrap();
        assert_eq!(json["tools"][0]["name"], "lookup_asset");
        assert_eq!(json["tools"][0]["input_schema"]["type"], "object");
        assert!(json["tools"][0].get("parameters").is_none());
        assert_eq!(json["tool_choice"]["type"], "any");

        // ToolChoice::None 时不发送 tools / tool_choice
        let no_tools = ChatRequest::new("c", vec![Message::user("查")])
            .with_tools(vec![ToolDef::new("x", serde_json::json!({}))])
            .with_tool_choice(ToolChoice::None);
        let json = serde_json::to_value(build_request(&no_tools, 4096).unwrap()).unwrap();
        assert!(json.get("tools").is_none());
        assert!(json.get("tool_choice").is_none());
    }

    #[test]
    fn response_is_normalized_with_blocks_and_cache_tokens() {
        let raw = r#"{
            "id": "msg_1",
            "model": "claude-sonnet-4-20250514",
            "content": [
                { "type": "text", "text": "我来查" },
                { "type": "tool_use", "id": "toolu_1", "name": "lookup_asset", "input": { "id": 42 } },
                { "type": "thinking", "thinking": "内部推理" }
            ],
            "stop_reason": "tool_use",
            "usage": {
                "input_tokens": 20,
                "output_tokens": 15,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 8
            }
        }"#;

        let parsed: MessagesResponse = serde_json::from_str(raw).unwrap();
        let response = response_to_unified(parsed);

        assert_eq!(response.text(), "我来查");
        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.usage.input_tokens, 20);
        assert_eq!(response.usage.output_tokens, 15);
        assert_eq!(response.usage.cache_read_input_tokens, Some(8));
        assert_eq!(response.tool_uses().len(), 1);
        assert_eq!(response.tool_uses()[0].0, "toolu_1");
    }
}



