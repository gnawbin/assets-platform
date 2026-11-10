//! OpenAI Chat Completions 线上结构（请求侧）
//!
//! 只描述「我们发出的 JSON」，用 `skip_serializing_if` 保证不发送空字段：
//! 部分兼容厂商对多余字段（尤其 `stream_options`）会直接返回 400。

use serde::Serialize;

/// `POST {base}/chat/completions` 请求体
#[derive(Debug, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<WireMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stop: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<WireTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

/// 请求中的一条消息
#[derive(Debug, Serialize)]
pub struct WireMessage {
    pub role: &'static str,
    /// 字符串（纯文本）或内容片段数组（多模态）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,
    /// assistant 消息中的工具调用
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<WireToolCallOut>>,
    /// `role=tool` 消息对应的调用 id
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// 发出的工具调用（assistant 历史消息回填）
#[derive(Debug, Serialize)]
pub struct WireToolCallOut {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: WireFunctionOut,
}

/// 工具调用的函数体
#[derive(Debug, Serialize)]
pub struct WireFunctionOut {
    pub name: String,
    /// JSON 字符串（不是对象）
    pub arguments: String,
}

/// 工具定义
#[derive(Debug, Serialize)]
pub struct WireTool {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: WireToolDef,
}

/// 工具定义的函数体
#[derive(Debug, Serialize)]
pub struct WireToolDef {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

/// `stream_options`（仅支持该字段的厂商才发送）
#[derive(Debug, Serialize)]
pub struct StreamOptions {
    pub include_usage: bool,
}

/// `POST {base}/embeddings` 请求体
#[derive(Debug, Serialize)]
pub struct EmbeddingBody {
    pub model: String,
    pub input: Vec<String>,
}

// ======================== 响应侧 ========================

/// 非流式对话响应
#[derive(Debug, serde::Deserialize)]
pub struct ChatCompletionResponse {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub choices: Vec<ResponseChoice>,
    #[serde(default)]
    pub usage: Option<UsageBody>,
}

/// 响应中的候选
#[derive(Debug, serde::Deserialize)]
pub struct ResponseChoice {
    #[serde(default)]
    pub message: Option<ResponseMessage>,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// 响应消息
#[derive(Debug, Default, serde::Deserialize)]
pub struct ResponseMessage {
    /// 字符串或内容片段数组（部分厂商的推理模型会返回数组）
    #[serde(default)]
    pub content: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallBody>,
}

/// 响应中的工具调用
#[derive(Debug, Default, serde::Deserialize)]
pub struct ToolCallBody {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: Option<FunctionBody>,
}

/// 响应中的函数体
#[derive(Debug, Default, serde::Deserialize)]
pub struct FunctionBody {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

/// 用量
#[derive(Debug, Default, serde::Deserialize)]
pub struct UsageBody {
    #[serde(default)]
    pub prompt_tokens: i32,
    #[serde(default)]
    pub completion_tokens: i32,
    /// 仅保留接口字段（总量可由 prompt + completion 推导）
    #[serde(default)]
    #[allow(dead_code)]
    pub total_tokens: i32,
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
}

/// 提示词用量明细（命中缓存部分）
#[derive(Debug, Default, serde::Deserialize)]
pub struct PromptTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<i32>,
}

/// 流式分片
#[derive(Debug, serde::Deserialize)]
pub struct ChatCompletionChunk {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub choices: Vec<ChunkChoice>,
    #[serde(default)]
    pub usage: Option<UsageBody>,
}

/// 流式分片中的候选
#[derive(Debug, Default, serde::Deserialize)]
pub struct ChunkChoice {
    /// 仅保留接口字段（多候选场景当前只取第一路内容）
    #[serde(default)]
    #[allow(dead_code)]
    pub index: u32,
    #[serde(default)]
    pub delta: Delta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// 增量内容
#[derive(Debug, Default, serde::Deserialize)]
pub struct Delta {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<DeltaToolCall>,
}

/// 增量中的工具调用（首个分片带 id/name，后续只带 arguments 片段）
#[derive(Debug, Default, serde::Deserialize)]
pub struct DeltaToolCall {
    #[serde(default)]
    pub index: u32,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: Option<DeltaFunction>,
}

/// 增量中的函数体
#[derive(Debug, Default, serde::Deserialize)]
pub struct DeltaFunction {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

/// `GET {base}/models` 响应
#[derive(Debug, serde::Deserialize)]
pub struct ModelsResponse {
    #[serde(default)]
    pub data: Vec<ModelEntry>,
}

/// 模型条目
#[derive(Debug, serde::Deserialize)]
pub struct ModelEntry {
    pub id: String,
    #[serde(default)]
    pub owned_by: Option<String>,
    #[serde(default)]
    pub created: Option<i64>,
}

/// Embedding 响应
#[derive(Debug, serde::Deserialize)]
pub struct EmbeddingResponseBody {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub data: Vec<EmbeddingEntry>,
    #[serde(default)]
    pub usage: Option<UsageBody>,
}

/// Embedding 条目
#[derive(Debug, Default, serde::Deserialize)]
pub struct EmbeddingEntry {
    #[serde(default)]
    pub embedding: Vec<f32>,
    #[serde(default)]
    pub index: u32,
}

