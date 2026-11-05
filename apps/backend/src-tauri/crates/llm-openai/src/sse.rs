//! SSE 分帧与 OpenAI 分片解析
//!
//! 分两层，均与网络无关、可单测：
//! - [`SseDecoder`]：把字节流增量切成 `event` / `data` 帧（处理跨 chunk 边界、`\r\n`、注释行）；
//! - [`ChunkParser`]：把 `data` JSON 归一化成 [`llm::StreamEvent`]，并按 `index` 累积工具参数。

use std::collections::{BTreeMap, BTreeSet};

use llm::{LlmError, LlmResult, StopReason, StreamEvent};

use crate::convert::to_usage;
use crate::wire::ChatCompletionChunk;

/// 一帧 SSE
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseFrame {
    /// `event:` 字段（OpenAI 通常不发送）
    pub event: Option<String>,
    /// `data:` 字段（多行 data 以 `\n` 拼接）
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
        // 其它字段（id: / retry:）忽略
    }

    if data_lines.is_empty() {
        return None;
    }
    Some(SseFrame {
        event,
        data: data_lines.join("\n"),
    })
}

/// OpenAI 分片解析器（有状态：工具调用跨分片）
#[derive(Debug, Default)]
pub struct ChunkParser {
    started: bool,
    finished: bool,
    /// 工具调用 index → 调用 id
    tool_ids: BTreeMap<u32, String>,
    /// 已经发过 `ToolUseStart` 的 index
    started_tools: BTreeSet<u32>,
}

impl ChunkParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// 处理一条 `data:` 内容
    pub fn feed(&mut self, data: &str) -> Vec<LlmResult<StreamEvent>> {
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            return Vec::new();
        }

        let chunk: ChatCompletionChunk = match serde_json::from_str(data) {
            Ok(chunk) => chunk,
            Err(error) => {
                return vec![Err(LlmError::decode(format!(
                    "分片解析失败：{error}；原文：{}",
                    truncate(data, 200)
                )))]
            }
        };

        let mut events = Vec::new();

        if !self.started {
            self.started = true;
            events.push(Ok(StreamEvent::Start {
                id: chunk.id.clone(),
                model: chunk.model.clone(),
            }));
        }

        if let Some(usage) = &chunk.usage {
            events.push(Ok(StreamEvent::Usage(to_usage(usage))));
        }

        for choice in chunk.choices {
            if let Some(content) = choice.delta.content {
                if !content.is_empty() {
                    events.push(Ok(StreamEvent::TextDelta { text: content }));
                }
            }

            for call in choice.delta.tool_calls {
                if let Some(id) = call.id.clone().filter(|id| !id.is_empty()) {
                    self.tool_ids.insert(call.index, id);
                }
                let call_id = self
                    .tool_ids
                    .get(&call.index)
                    .cloned()
                    .unwrap_or_else(|| format!("call_{}", call.index));

                let name = call
                    .function
                    .as_ref()
                    .and_then(|function| function.name.clone())
                    .filter(|name| !name.is_empty());

                // 首个带 name 的分片才发 ToolUseStart，避免重复
                if let Some(name) = name {
                    if self.started_tools.insert(call.index) {
                        events.push(Ok(StreamEvent::ToolUseStart {
                            id: call_id.clone(),
                            name,
                        }));
                    }
                }

                if let Some(arguments) = call.function.and_then(|function| function.arguments) {
                    if !arguments.is_empty() {
                        events.push(Ok(StreamEvent::ToolUseArgsDelta {
                            id: call_id,
                            partial_json: arguments,
                        }));
                    }
                }
            }

            if let Some(reason) = choice.finish_reason {
                for index in std::mem::take(&mut self.started_tools) {
                    if let Some(id) = self.tool_ids.get(&index) {
                        events.push(Ok(StreamEvent::ToolUseEnd { id: id.clone() }));
                    }
                }
                if !self.finished {
                    self.finished = true;
                    events.push(Ok(StreamEvent::Stop(StopReason::from_openai(&reason))));
                }
            }
        }

        events
    }
}

/// 截断过长文本，避免错误信息淹没日志
fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_reassembles_frames_split_across_chunks() {
        let mut decoder = SseDecoder::new();
        assert!(decoder.push("data: {\"a\":").is_empty());
        let frames = decoder.push("1}\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, "{\"a\":1}");
        assert_eq!(frames[0].event, None);
    }

    #[test]
    fn decoder_handles_crlf_comments_and_multiline_data() {
        let mut decoder = SseDecoder::new();
        let frames = decoder.push(": keep-alive\r\n\r\nevent: message\r\ndata: line1\r\ndata: line2\r\n\r\n");
        // 注释帧被跳过，只剩一个真实帧
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event.as_deref(), Some("message"));
        assert_eq!(frames[0].data, "line1\nline2");
    }

    #[test]
    fn decoder_finish_returns_trailing_frame() {
        let mut decoder = SseDecoder::new();
        assert!(decoder.push("data: [DONE]").is_empty());
        let tail = decoder.finish().expect("应返回残留帧");
        assert_eq!(tail.data, "[DONE]");
        assert!(decoder.finish().is_none());
    }

    #[test]
    fn parser_emits_start_text_and_stop() {
        let mut parser = ChunkParser::new();
        let first = parser.feed(
            r#"{"id":"c1","model":"gpt-4o","choices":[{"index":0,"delta":{"content":"你"}}]}"#,
        );
        assert_eq!(
            first[0].as_ref().unwrap(),
            &StreamEvent::Start {
                id: "c1".to_string(),
                model: "gpt-4o".to_string()
            }
        );
        assert_eq!(
            first[1].as_ref().unwrap(),
            &StreamEvent::TextDelta {
                text: "你".to_string()
            }
        );

        let last = parser.feed(
            r#"{"id":"c1","model":"gpt-4o","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7}}"#,
        );
        // usage 先于 stop 上报
        match last[0].as_ref().unwrap() {
            StreamEvent::Usage(usage) => {
                assert_eq!(usage.input_tokens, 3);
                assert_eq!(usage.output_tokens, 4);
            }
            other => panic!("期望 Usage，实际 {other:?}"),
        }
        assert_eq!(
            last[1].as_ref().unwrap(),
            &StreamEvent::Stop(StopReason::EndTurn)
        );
    }

    #[test]
    fn parser_accumulates_tool_arguments_once_per_index() {
        let mut parser = ChunkParser::new();
        let head = parser.feed(
            r#"{"id":"c2","model":"m","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"name":"lookup","arguments":""}}]}}]}"#,
        );
        assert!(matches!(
            head[1].as_ref().unwrap(),
            StreamEvent::ToolUseStart { id, name } if id == "call_a" && name == "lookup"
        ));

        let body = parser.feed(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"id\":42}"}}]}}]}"#,
        );
        // 只有参数增量，没有重复的 ToolUseStart
        assert_eq!(body.len(), 1);
        assert_eq!(
            body[0].as_ref().unwrap(),
            &StreamEvent::ToolUseArgsDelta {
                id: "call_a".to_string(),
                partial_json: "{\"id\":42}".to_string()
            }
        );

        let tail = parser.feed(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#);
        assert_eq!(
            tail[0].as_ref().unwrap(),
            &StreamEvent::ToolUseEnd {
                id: "call_a".to_string()
            }
        );
        assert_eq!(
            tail[1].as_ref().unwrap(),
            &StreamEvent::Stop(StopReason::ToolUse)
        );
    }

    #[test]
    fn parser_ignores_done_marker_and_reports_bad_json() {
        let mut parser = ChunkParser::new();
        assert!(parser.feed("[DONE]").is_empty());
        assert!(parser.feed("   ").is_empty());

        let events = parser.feed("<html>oops</html>");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], Err(LlmError::Decode(_))));
    }
}

