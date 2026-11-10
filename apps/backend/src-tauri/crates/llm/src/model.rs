//! 协议无关的会话模型
//!
//! 设计要点：
//! - 消息内容统一为 `Vec<ContentPart>`，纯文本即单片段，便于多模态与工具调用共存；
//! - `Usage` 携带 Anthropic 的缓存 token 字段，OpenAI 侧用 `cached_tokens` 映射；
//! - `StopReason` 把两套协议的结束原因收敛为同一枚举，业务层无需感知厂商差异。

use serde::{Deserialize, Serialize};

// ======================== 角色 ========================

/// 消息角色
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    /// 线上的角色名（OpenAI / Anthropic 均为小写）
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }

    /// 由字符串解析，未知值按 `User` 处理（保持旧实现「未知 → user」的宽容行为）
    pub fn from_wire(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "system" => Self::System,
            "assistant" => Self::Assistant,
            "tool" => Self::Tool,
            _ => Self::User,
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ======================== 内容片段 ========================

/// 图片来源：远程 URL 或 base64
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageSource {
    /// 远程可访问的图片地址（OpenAI 兼容厂商可直接吃）
    Url { url: String },
    /// base64 数据 + MIME 类型（Anthropic 只接受这种）
    Base64 { media_type: String, data: String },
}

/// 内容片段：文本 / 图片 / 工具调用 / 工具结果
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ContentPart {
    Text {
        text: String,
    },
    Image {
        source: ImageSource,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: Vec<ContentPart>,
        is_error: bool,
    },
}

impl ContentPart {
    /// 文本片段
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// 远程图片片段
    pub fn image_url(url: impl Into<String>) -> Self {
        Self::Image {
            source: ImageSource::Url { url: url.into() },
        }
    }

    /// base64 图片片段
    pub fn image_base64(media_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self::Image {
            source: ImageSource::Base64 {
                media_type: media_type.into(),
                data: data.into(),
            },
        }
    }

    /// 若为文本片段，返回其内容
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }
}

// ======================== 消息 ========================

/// 一条消息
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentPart>,
}

impl Message {
    pub fn new(role: Role, content: Vec<ContentPart>) -> Self {
        Self { role, content }
    }

    /// system 消息
    pub fn system(text: impl Into<String>) -> Self {
        Self::new(Role::System, vec![ContentPart::text(text)])
    }

    /// user 纯文本消息
    pub fn user(text: impl Into<String>) -> Self {
        Self::new(Role::User, vec![ContentPart::text(text)])
    }

    /// assistant 纯文本消息
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::new(Role::Assistant, vec![ContentPart::text(text)])
    }

    /// 工具结果消息（OpenAI 侧 role=tool，Anthropic 侧 user 轮的 tool_result 块）
    pub fn tool_result(tool_use_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new(
            Role::Tool,
            vec![ContentPart::ToolResult {
                tool_use_id: tool_use_id.into(),
                content: vec![ContentPart::text(text)],
                is_error: false,
            }],
        )
    }

    /// 拼接全部文本片段
    pub fn text(&self) -> String {
        let mut out = String::new();
        for part in &self.content {
            if let ContentPart::Text { text } = part {
                out.push_str(text);
            }
        }
        out
    }

    /// 是否只含文本片段
    pub fn is_text_only(&self) -> bool {
        self.content
            .iter()
            .all(|part| matches!(part, ContentPart::Text { .. }))
    }
}

// ======================== 用量 ========================

/// Token 用量（含 Anthropic 缓存 token）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: i32,
    pub output_tokens: i32,
    /// 写入缓存的输入 token（Anthropic 专有）
    pub cache_creation_input_tokens: Option<i32>,
    /// 命中缓存的输入 token（Anthropic / OpenAI cached_tokens）
    pub cache_read_input_tokens: Option<i32>,
}

impl Usage {
    /// 输入 + 输出总量
    pub fn total(&self) -> i32 {
        self.input_tokens + self.output_tokens
    }

    /// 合并流式过程中分片上报的用量（后到的非零字段覆盖先到的）
    pub fn merge(&mut self, other: &Self) {
        if other.input_tokens > 0 {
            self.input_tokens = other.input_tokens;
        }
        if other.output_tokens > 0 {
            self.output_tokens = other.output_tokens;
        }
        if other.cache_creation_input_tokens.is_some() {
            self.cache_creation_input_tokens = other.cache_creation_input_tokens;
        }
        if other.cache_read_input_tokens.is_some() {
            self.cache_read_input_tokens = other.cache_read_input_tokens;
        }
    }
}

// ======================== 结束原因 ========================

/// 结束原因（两套协议归一化）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    /// 正常结束
    EndTurn,
    /// 达到 max_tokens
    MaxTokens,
    /// 模型要求调用工具
    ToolUse,
    /// 命中 stop 序列
    StopSequence,
    /// 内容被安全策略拦截
    ContentFilter,
    /// 其它厂商专有值，原样保留
    Other(String),
}

impl StopReason {
    /// 稳定字符串，便于落库与前端判别
    pub fn as_str(&self) -> &str {
        match self {
            Self::EndTurn => "end_turn",
            Self::MaxTokens => "max_tokens",
            Self::ToolUse => "tool_use",
            Self::StopSequence => "stop_sequence",
            Self::ContentFilter => "content_filter",
            Self::Other(value) => value,
        }
    }

    /// OpenAI `finish_reason` → 统一结束原因
    pub fn from_openai(value: &str) -> Self {
        match value {
            "stop" => Self::EndTurn,
            "length" => Self::MaxTokens,
            "tool_calls" | "function_call" => Self::ToolUse,
            "stop_sequence" => Self::StopSequence,
            "content_filter" => Self::ContentFilter,
            other => Self::Other(other.to_string()),
        }
    }

    /// Anthropic `stop_reason` → 统一结束原因
    pub fn from_anthropic(value: &str) -> Self {
        match value {
            "end_turn" => Self::EndTurn,
            "max_tokens" => Self::MaxTokens,
            "tool_use" => Self::ToolUse,
            "stop_sequence" => Self::StopSequence,
            "refusal" => Self::ContentFilter,
            other => Self::Other(other.to_string()),
        }
    }

    /// 是否因为要调工具而结束
    pub fn is_tool_use(&self) -> bool {
        matches!(self, Self::ToolUse)
    }
}

impl std::fmt::Display for StopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ======================== 工具调用 ========================

/// 工具定义（JSON Schema 描述参数）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: Option<String>,
    /// JSON Schema（OpenAI `parameters` / Anthropic `input_schema`）
    pub parameters: serde_json::Value,
}

impl ToolDef {
    pub fn new(name: impl Into<String>, parameters: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            description: None,
            parameters,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// 工具选择策略
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolChoice {
    /// 模型自行决定
    Auto,
    /// 禁止调用工具
    None,
    /// 强制调用任意一个工具
    Any,
    /// 必须调用工具（OpenAI 语义，等价于 `Any`）
    Required,
    /// 强制调用指定工具
    Named(String),
}

// ======================== 请求 / 响应 ========================

/// 统一的对话请求
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_tokens: Option<i32>,
    pub stop: Vec<String>,
    pub tools: Vec<ToolDef>,
    pub tool_choice: Option<ToolChoice>,
    pub stream: bool,
    /// 终端用户标识（OpenAI `user` 字段，供应商可据此做滥用检测）
    pub user: Option<String>,
}

impl ChatRequest {
    /// 新建请求（默认非流式）
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            temperature: None,
            top_p: None,
            max_tokens: None,
            stop: Vec::new(),
            tools: Vec::new(),
            tool_choice: None,
            stream: false,
            user: None,
        }
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: i32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn with_stream(mut self, stream: bool) -> Self {
        self.stream = stream;
        self
    }

    pub fn with_tools(mut self, tools: Vec<ToolDef>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_tool_choice(mut self, tool_choice: ToolChoice) -> Self {
        self.tool_choice = Some(tool_choice);
        self
    }

    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }
}

/// 统一的对话响应
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    /// 文本与工具调用混合输出
    pub content: Vec<ContentPart>,
    pub stop_reason: StopReason,
    pub usage: Usage,
}

impl ChatResponse {
    /// 拼接全部文本片段
    pub fn text(&self) -> String {
        let mut out = String::new();
        for part in &self.content {
            if let ContentPart::Text { text } = part {
                out.push_str(text);
            }
        }
        out
    }

    /// 提取全部工具调用
    pub fn tool_uses(&self) -> Vec<(&str, &str, &serde_json::Value)> {
        self.content
            .iter()
            .filter_map(|part| match part {
                ContentPart::ToolUse { id, name, input } => {
                    Some((id.as_str(), name.as_str(), input))
                }
                _ => None,
            })
            .collect()
    }
}

/// Embedding 请求
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingRequest {
    pub model: String,
    pub input: Vec<String>,
}

impl EmbeddingRequest {
    pub fn new(model: impl Into<String>, input: Vec<String>) -> Self {
        Self {
            model: model.into(),
            input,
        }
    }
}

/// Embedding 响应
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    pub model: String,
    pub embeddings: Vec<Vec<f32>>,
    pub usage: Usage,
}

/// 模型条目
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub owned_by: Option<String>,
    pub created: Option<i64>,
}

impl ModelInfo {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            owned_by: None,
            created: None,
        }
    }
}

// ======================== 流式事件 ========================

/// 流式事件（两套协议归一化后的事件）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StreamEvent {
    /// 流开始（携带响应 id 与模型名）
    Start { id: String, model: String },
    /// 文本增量
    TextDelta { text: String },
    /// 工具调用开始（同一 `id` 后续会有若干 [`StreamEvent::ToolUseArgsDelta`]）
    ToolUseStart { id: String, name: String },
    /// 工具参数 JSON 增量（需按 `id` 累积后再解析）
    ToolUseArgsDelta { id: String, partial_json: String },
    /// 工具调用结束
    ToolUseEnd { id: String },
    /// 用量上报（通常在流末尾）
    Usage(Usage),
    /// 流结束
    Stop(StopReason),
}

impl StreamEvent {
    /// 提取文本增量
    pub fn text_delta(&self) -> Option<&str> {
        match self {
            Self::TextDelta { text } => Some(text),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_parses_known_and_falls_back_to_user() {
        assert_eq!(Role::from_wire("system"), Role::System);
        assert_eq!(Role::from_wire("ASSISTANT"), Role::Assistant);
        assert_eq!(Role::from_wire("tool"), Role::Tool);
        // 未知角色按 user 处理，保持旧实现的宽容行为
        assert_eq!(Role::from_wire("developer"), Role::User);
        assert_eq!(Role::Assistant.to_string(), "assistant");
    }

    #[test]
    fn message_text_aggregates_only_text_parts() {
        let message = Message::new(
            Role::User,
            vec![
                ContentPart::text("请描述这张图："),
                ContentPart::image_base64("image/png", "AAA"),
                ContentPart::text("（附图）"),
            ],
        );
        assert_eq!(message.text(), "请描述这张图：（附图）");
        assert!(!message.is_text_only());
        assert!(Message::user("纯文本").is_text_only());
    }

    #[test]
    fn usage_total_and_merge_keep_latest_non_zero() {
        let mut usage = Usage {
            input_tokens: 10,
            output_tokens: 0,
            cache_creation_input_tokens: Some(3),
            cache_read_input_tokens: None,
        };
        assert_eq!(usage.total(), 10);

        usage.merge(&Usage {
            input_tokens: 0,
            output_tokens: 25,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: Some(7),
        });
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 25);
        assert_eq!(usage.cache_creation_input_tokens, Some(3));
        assert_eq!(usage.cache_read_input_tokens, Some(7));
        assert_eq!(usage.total(), 35);
    }

    #[test]
    fn stop_reason_maps_both_protocols() {
        assert_eq!(StopReason::from_openai("stop"), StopReason::EndTurn);
        assert_eq!(StopReason::from_openai("length"), StopReason::MaxTokens);
        assert_eq!(StopReason::from_openai("tool_calls"), StopReason::ToolUse);
        assert!(StopReason::from_openai("tool_calls").is_tool_use());
        assert_eq!(
            StopReason::from_openai("eos"),
            StopReason::Other("eos".to_string())
        );

        assert_eq!(StopReason::from_anthropic("end_turn"), StopReason::EndTurn);
        assert_eq!(
            StopReason::from_anthropic("max_tokens"),
            StopReason::MaxTokens
        );
        assert_eq!(StopReason::from_anthropic("tool_use"), StopReason::ToolUse);
        assert_eq!(
            StopReason::from_anthropic("refusal"),
            StopReason::ContentFilter
        );
        assert_eq!(StopReason::EndTurn.as_str(), "end_turn");
    }

    #[test]
    fn chat_response_splits_text_and_tool_uses() {
        let response = ChatResponse {
            id: "resp_1".to_string(),
            model: "gpt-4o".to_string(),
            content: vec![
                ContentPart::text("我来查一下"),
                ContentPart::ToolUse {
                    id: "call_1".to_string(),
                    name: "lookup_asset".to_string(),
                    input: serde_json::json!({ "id": 42 }),
                },
            ],
            stop_reason: StopReason::ToolUse,
            usage: Usage::default(),
        };

        assert_eq!(response.text(), "我来查一下");
        let calls = response.tool_uses();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "call_1");
        assert_eq!(calls[0].1, "lookup_asset");
        assert_eq!(calls[0].2["id"], 42);
    }

    #[test]
    fn chat_request_builder_sets_options() {
        let request = ChatRequest::new("gpt-4o", vec![Message::user("hi")])
            .with_temperature(0.2)
            .with_max_tokens(512)
            .with_stream(true)
            .with_user("u-1");
        assert_eq!(request.model, "gpt-4o");
        assert_eq!(request.temperature, Some(0.2));
        assert_eq!(request.max_tokens, Some(512));
        assert!(request.stream);
        assert_eq!(request.user.as_deref(), Some("u-1"));
    }
}



