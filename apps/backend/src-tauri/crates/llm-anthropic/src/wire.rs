//! Anthropic Messages 线上结构

use serde::{Deserialize, Serialize};

/// `POST {base}/v1/messages` 请求体
#[derive(Debug, Serialize)]
pub struct MessagesRequest {
    pub model: String,
    /// **必填**：Anthropic 不接受缺省
    pub max_tokens: i32,
    pub messages: Vec<WireMessage>,
    /// system 提示是顶层字段（可为字符串或内容块数组）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stop_sequences: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<WireTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    pub stream: bool,
}

/// 请求中的一条消息（`content` 必为内容块数组）
#[derive(Debug, Serialize)]
pub struct WireMessage {
    pub role: &'static str,
    pub content: Vec<serde_json::Value>,
}

/// 工具定义（参数用 `input_schema` 而非 `parameters`）
#[derive(Debug, Serialize)]
pub struct WireTool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

/// 非流式响应
#[derive(Debug, Deserialize)]
pub struct MessagesResponse {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub usage: Option<UsageBody>,
}

/// 响应内容块（未知类型忽略，避免新特性导致解析失败）
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: serde_json::Value,
    },
    #[serde(other)]
    Unknown,
}

/// 用量（含缓存 token）
#[derive(Debug, Default, Deserialize)]
pub struct UsageBody {
    #[serde(default)]
    pub input_tokens: i32,
    #[serde(default)]
    pub output_tokens: i32,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<i32>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<i32>,
}

/// `GET {base}/v1/models` 响应
#[derive(Debug, Deserialize)]
pub struct ModelsResponse {
    #[serde(default)]
    pub data: Vec<ModelEntry>,
}

/// 模型条目（`created_at` 是 RFC3339 字符串，统一类型里不落时间）
#[derive(Debug, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    /// 仅用于保留接口字段，当前不消费
    #[serde(default)]
    #[allow(dead_code)]
    pub created_at: Option<String>,
}
