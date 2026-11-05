//! SSE 分帧与 Anthropic 命名事件解析
//!
//! Anthropic 的流式协议与 OpenAI 不同：每帧都带 `event:` 名字，事件语义分散在
//! `message_start` / `content_block_*` / `message_delta` / `message_stop` 中。
//! 本模块把它们归一化成 [`llm::StreamEvent`]。

use std::collections::BTreeMap;

use llm::{LlmError, LlmResult, StopReason, StreamEvent};

use crate::convert::to_usage;
use crate::wire::UsageBody;

/// 一帧 SSE
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseFrame {
    /// 事件名（Anthropic 必填）
    pub event: Option<String>,
    /// `data:` 内容
    pub data: String,
}

/// 增量 SSE 解码器
#[derive(Debug, Default)]
pub struct SseDecoder {
    buffer: String,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 喂入一段增量文本，返回其中已经完整的帧
    pub fn push(&mut self, chunk: &str) -> Vec<SseFrame> {
        self.buffer.push_str(chunk);
        // 统一换行（CRLF → LF）：分片可能把 `\r\n` 切在中间，末尾的 `\r` 会留到下一片
        if self.buffer.contains('\r') {
            self.buffer = normalize_newlines(&self.buffer);
        }

        let mut frames = Vec::new();
        while let Some(pos) = self.buffer.find("\n\n") {
            let raw: String = self.buffer[..pos].to_string();
            self.buffer.drain(..pos + 2);
            if let Some(frame) = parse_frame(&raw) {
                frames.push(frame);
            }
        }

        frames
    }

    /// 流结束时取出残留帧
    pub fn finish(&mut self) -> Option<SseFrame> {
        let raw = std::mem::take(&mut self.buffer).replace('\r', "\n");
        parse_frame(&raw)
    }
}

/// CRLF → LF（孤立 `\r` 也视为换行；末尾 `\r` 原样保留等待下一分片）
fn normalize_newlines(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(current) = chars.next() {
        if current == '\r' {
            match chars.peek() {
                Some('\n') => {
                    chars.next();
                    out.push('\n');
                }
                Some(_) => out.push('\n'),
                None => out.push('\r'),
            }
        } else {
            out.push(current);
        }
    }
    out
}

/// 解析单帧文本
fn parse_frame(raw: &str) -> Option<SseFrame> {
    let mut event = None;
    let mut data_lines: Vec<&str> = Vec::new();

    for line in raw.split('\n') {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            event = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }

    if data_lines.is_empty() {
        return None;
    }
    Some(SseFrame {
        event,
        data: data_lines.join("\n"),
    })
}

/// Anthropic 事件解析器（有状态：工具块按 `index` 关联）
#[derive(Debug, Default)]
pub struct MessageStreamParser {
    started: bool,
    finished: bool,
    /// 内容块 index → tool_use id
    tool_ids: BTreeMap<u32, String>,
}

impl MessageStreamParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// 处理一帧
    pub fn feed(&mut self, frame: &SseFrame) -> Vec<LlmResult<StreamEvent>> {
        let event = frame.event.as_deref().unwrap_or("");
        if event.is_empty() || frame.data.trim().is_empty() {
            return Vec::new();
        }

        let value: serde_json::Value = match serde_json::from_str(&frame.data) {
            Ok(value) => value,
            Err(error) => {
                return vec![Err(LlmError::decode(format!(
                    "事件解析失败（{event}）：{error}"
                )))]
            }
        };

        match event {
            "message_start" => self.on_message_start(&value),
            "content_block_start" => self.on_block_start(&value),
            "content_block_delta" => self.on_block_delta(&value),
            "content_block_stop" => self.on_block_stop(&value),
            "message_delta" => self.on_message_delta(&value),
            "message_stop" => self.on_message_stop(),
            "ping" => Vec::new(),
            "error" => vec![Err(crate::error::map_error(529, &frame.data, None))],
            // 未知事件（如未来的 server_tool_use）直接忽略，保持向前兼容
            _ => Vec::new(),
        }
    }

    fn on_message_start(&mut self, value: &serde_json::Value) -> Vec<LlmResult<StreamEvent>> {
        let mut events = Vec::new();
        if !self.started {
            self.started = true;
            events.push(Ok(StreamEvent::Start {
                id: value["message"]["id"].as_str().unwrap_or_default().to_string(),
                model: value["message"]["model"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            }));
        }
        if let Ok(usage) = serde_json::from_value::<UsageBody>(value["message"]["usage"].clone()) {
            events.push(Ok(StreamEvent::Usage(to_usage(&usage))));
        }
        events
    }

    fn on_block_start(&mut self, value: &serde_json::Value) -> Vec<LlmResult<StreamEvent>> {
        let index = value["index"].as_u64().unwrap_or(0) as u32;
        let block = &value["content_block"];
        if block["type"].as_str() != Some("tool_use") {
            return Vec::new();
        }
        let id = block["id"].as_str().unwrap_or_default().to_string();
        let name = block["name"].as_str().unwrap_or_default().to_string();
        if id.is_empty() || name.is_empty() {
            return Vec::new();
        }
        self.tool_ids.insert(index, id.clone());
        vec![Ok(StreamEvent::ToolUseStart { id, name })]
    }

    fn on_block_delta(&mut self, value: &serde_json::Value) -> Vec<LlmResult<StreamEvent>> {
        let index = value["index"].as_u64().unwrap_or(0) as u32;
        let delta = &value["delta"];

        match delta["type"].as_str() {
            Some("text_delta") => match delta["text"].as_str() {
                Some(text) if !text.is_empty() => {
                    vec![Ok(StreamEvent::TextDelta {
                        text: text.to_string(),
                    })]
                }
                _ => Vec::new(),
            },
            Some("input_json_delta") => {
                let partial = delta["partial_json"].as_str().unwrap_or_default();
                if partial.is_empty() {
                    return Vec::new();
                }
                let id = self
                    .tool_ids
                    .get(&index)
                    .cloned()
                    .unwrap_or_else(|| format!("tool_{index}"));
                vec![Ok(StreamEvent::ToolUseArgsDelta {
                    id,
                    partial_json: partial.to_string(),
                })]
            }
            // thinking_delta / signature_delta 等不影响回答正文
            _ => Vec::new(),
        }
    }

    fn on_block_stop(&mut self, value: &serde_json::Value) -> Vec<LlmResult<StreamEvent>> {
        let index = value["index"].as_u64().unwrap_or(0) as u32;
        match self.tool_ids.remove(&index) {
            Some(id) => vec![Ok(StreamEvent::ToolUseEnd { id })],
            None => Vec::new(),
        }
    }

    fn on_message_delta(&mut self, value: &serde_json::Value) -> Vec<LlmResult<StreamEvent>> {
        let mut events = Vec::new();

        if let Ok(usage) = serde_json::from_value::<UsageBody>(value["usage"].clone()) {
            events.push(Ok(StreamEvent::Usage(to_usage(&usage))));
        }

        if let Some(reason) = value["delta"]["stop_reason"].as_str() {
            if !self.finished {
                self.finished = true;
                events.push(Ok(StreamEvent::Stop(StopReason::from_anthropic(reason))));
            }
        }
        events
    }

    fn on_message_stop(&mut self) -> Vec<LlmResult<StreamEvent>> {
        // 个别厂商不发 `message_delta.stop_reason`，此处补一个兜底结束事件
        if self.finished {
            return Vec::new();
        }
        self.finished = true;
        vec![Ok(StreamEvent::Stop(StopReason::EndTurn))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(event: &str, data: &str) -> SseFrame {
        SseFrame {
            event: Some(event.to_string()),
            data: data.to_string(),
        }
    }

    #[test]
    fn decoder_reassembles_named_events_across_chunks() {
        let mut decoder = SseDecoder::new();
        assert!(decoder.push("event: message_start\ndata: {\"a\"").is_empty());
        let frames = decoder.push(":1}\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event.as_deref(), Some("message_start"));
        assert_eq!(frames[0].data, "{\"a\":1}");
    }

    #[test]
    fn parser_emits_start_usage_text_and_stop() {
        let mut parser = MessageStreamParser::new();

        let start = parser.feed(&frame(
            "message_start",
            r#"{"type":"message_start","message":{"id":"msg_1","model":"claude-sonnet-4-20250514","usage":{"input_tokens":9,"cache_read_input_tokens":4}}}"#,
        ));
        assert_eq!(
            start[0].as_ref().unwrap(),
            &StreamEvent::Start {
                id: "msg_1".to_string(),
                model: "claude-sonnet-4-20250514".to_string()
            }
        );
        match start[1].as_ref().unwrap() {
            StreamEvent::Usage(usage) => {
                assert_eq!(usage.input_tokens, 9);
                assert_eq!(usage.cache_read_input_tokens, Some(4));
            }
            other => panic!("期望 Usage，实际 {other:?}"),
        }

        let text = parser.feed(&frame(
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"你好"}}"#,
        ));
        assert_eq!(
            text[0].as_ref().unwrap(),
            &StreamEvent::TextDelta {
                text: "你好".to_string()
            }
        );

        let delta = parser.feed(&frame(
            "message_delta",
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":12}}"#,
        ));
        assert_eq!(
            delta[1].as_ref().unwrap(),
            &StreamEvent::Stop(StopReason::EndTurn)
        );
    }

    #[test]
    fn parser_handles_tool_use_blocks() {
        let mut parser = MessageStreamParser::new();
        let _ = parser.feed(&frame(
            "message_start",
            r#"{"message":{"id":"m","model":"c","usage":{"input_tokens":1}}}"#,
        ));

        let start = parser.feed(&frame(
            "content_block_start",
            r#"{"index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"lookup"}}"#,
        ));
        assert_eq!(
            start[0].as_ref().unwrap(),
            &StreamEvent::ToolUseStart {
                id: "toolu_1".to_string(),
                name: "lookup".to_string()
            }
        );

        let args = parser.feed(&frame(
            "content_block_delta",
            r#"{"index":1,"delta":{"type":"input_json_delta","partial_json":"{\"id\":7}"}}"#,
        ));
        assert_eq!(
            args[0].as_ref().unwrap(),
            &StreamEvent::ToolUseArgsDelta {
                id: "toolu_1".to_string(),
                partial_json: "{\"id\":7}".to_string()
            }
        );

        let stop = parser.feed(&frame("content_block_stop", r#"{"index":1}"#));
        assert_eq!(
            stop[0].as_ref().unwrap(),
            &StreamEvent::ToolUseEnd {
                id: "toolu_1".to_string()
            }
        );
    }

    #[test]
    fn parser_ignores_ping_and_reports_error_events() {
        let mut parser = MessageStreamParser::new();
        assert!(parser.feed(&frame("ping", "{}")).is_empty());

        let events = parser.feed(&frame(
            "error",
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
        ));
        assert!(matches!(
            events[0],
            Err(LlmError::Server { status: 529, .. })
        ));
    }

    #[test]
    fn message_stop_is_a_fallback_for_missing_stop_reason() {
        let mut parser = MessageStreamParser::new();
        let stop = parser.feed(&frame("message_stop", "{}"));
        assert_eq!(
            stop[0].as_ref().unwrap(),
            &StreamEvent::Stop(StopReason::EndTurn)
        );
        // 重复的 message_stop 不再产生事件
        assert!(parser.feed(&frame("message_stop", "{}")).is_empty());
    }

    #[test]
    fn thinking_delta_is_ignored() {
        let mut parser = MessageStreamParser::new();
        let events = parser.feed(&frame(
            "content_block_delta",
            r#"{"index":0,"delta":{"type":"thinking_delta","thinking":"内部推理"}}"#,
        ));
        assert!(events.is_empty());
    }
}



