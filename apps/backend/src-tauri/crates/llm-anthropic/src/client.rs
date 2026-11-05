//! Anthropic Messages Provider 实现

use std::time::Duration;

use llm::{LlmClientConfig, LlmError, LlmResult};

/// 默认 `anthropic-version`
pub const DEFAULT_API_VERSION: &str = "2023-06-01";
/// 默认 `max_tokens`（Messages API 必填，旧实现 2048 容易截断长回答）
pub const DEFAULT_MAX_TOKENS: i32 = 4096;

/// Provider 构造参数
pub struct AnthropicOptions {
    /// 厂商编码（与 `llm_provider.provider_code` 一致）
    pub provider_code: String,
    /// 端点 / 密钥 / 默认模型
    pub config: LlmClientConfig,
    /// `anthropic-version` 头
    pub api_version: String,
    /// 可选 `anthropic-beta`（如 prompt caching、interleaved thinking）
    pub betas: Vec<String>,
    /// 请求未指定 `max_tokens` 时的兜底值
    pub default_max_tokens: i32,
}

impl AnthropicOptions {
    pub fn new(provider_code: impl Into<String>, config: LlmClientConfig) -> Self {
        Self {
            provider_code: provider_code.into(),
            config,
            api_version: DEFAULT_API_VERSION.to_string(),
            betas: Vec::new(),
            default_max_tokens: DEFAULT_MAX_TOKENS,
        }
    }

    pub fn with_api_version(mut self, api_version: impl Into<String>) -> Self {
        self.api_version = api_version.into();
        self
    }

    pub fn with_beta(mut self, beta: impl Into<String>) -> Self {
        self.betas.push(beta.into());
        self
    }

    pub fn with_default_max_tokens(mut self, max_tokens: i32) -> Self {
        self.default_max_tokens = max_tokens;
        self
    }
}

/// Anthropic Messages 协议的 Provider
pub struct AnthropicProvider {
    provider_code: String,
    config: LlmClientConfig,
    api_version: String,
    betas: Vec<String>,
    default_max_tokens: i32,
    http: reqwest::Client,
    messages_url: String,
    models_url: String,
}

impl AnthropicProvider {
    /// 构造 Provider（校验 base_url / 默认模型，创建 HTTP 客户端）
    pub fn new(options: AnthropicOptions) -> LlmResult<Self> {
        let config = options.config;
        let base = normalize_base_url(&config.base_url);
        if base.is_empty() {
            return Err(LlmError::config("base_url 为空"));
        }
        if config.default_model.trim().is_empty() {
            return Err(LlmError::config("默认模型为空"));
        }

        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|error| LlmError::config(format!("HTTP 客户端创建失败：{error}")))?;

        Ok(Self {
            messages_url: format!("{base}/v1/messages"),
            models_url: format!("{base}/v1/models"),
            provider_code: options.provider_code,
            api_version: options.api_version,
            betas: options.betas,
            default_max_tokens: options.default_max_tokens,
            config,
            http,
        })
    }

    /// 注入 `x-api-key` / `anthropic-version` / `anthropic-beta` 与自定义头
    fn with_headers(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut builder = builder.header("anthropic-version", &self.api_version);

        let key = self.config.api_key.trim();
        if !key.is_empty() {
            builder = builder.header("x-api-key", key);
        }
        if !self.betas.is_empty() {
            builder = builder.header("anthropic-beta", self.betas.join(","));
        }
        for (name, value) in &self.config.headers {
            builder = builder.header(name, value);
        }
        builder
    }

    /// 请求未指定模型时使用配置中的默认模型
    fn fill_defaults(&self, mut request: llm::ChatRequest) -> llm::ChatRequest {
        if request.model.trim().is_empty() {
            request.model = self.config.default_model.clone();
        }
        request
    }
}

/// 归一化 base_url
///
/// - 去掉尾部 `/`；
/// - 去掉误填的 `/v1`、`/v1/messages`、`/v1/models`（实现侧统一追加 `/v1/messages`）；
/// - 其余路径（如反向代理前缀）原样保留。
pub fn normalize_base_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }

    let mut base = trimmed;
    for suffix in ["/v1/messages", "/v1/models", "/v1"] {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped.trim_end_matches('/');
            break;
        }
    }
    base.to_string()
}

/// 读取 `Retry-After` 头（秒）
fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

use std::collections::VecDeque;

use async_trait::async_trait;
use futures::StreamExt;
use llm::{
    ChatRequest, ChatResponse, ChatStream, LlmProvider, ModelInfo, Protocol, StreamEvent,
};

use crate::convert::{build_request, response_to_unified};
use crate::error::map_error;
use crate::sse::{MessageStreamParser, SseDecoder};
use crate::wire::MessagesResponse;

/// 字节流（reqwest 错误统一成 String，便于放进 unfold 状态）
type ByteStream = futures::stream::BoxStream<'static, Result<Vec<u8>, String>>;
/// 流式解析状态：(字节流, SSE 解码器, 事件解析器, 待发事件队列, 是否结束)
type StreamState = (
    ByteStream,
    SseDecoder,
    MessageStreamParser,
    VecDeque<LlmResult<StreamEvent>>,
    bool,
);

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn provider_code(&self) -> &str {
        &self.provider_code
    }

    fn protocol(&self) -> Protocol {
        Protocol::AnthropicMessages
    }

    fn default_model(&self) -> &str {
        &self.config.default_model
    }

    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatResponse> {
        let request = self.fill_defaults(request);
        let body = build_request(&request, self.default_max_tokens)?;

        let response = self
            .with_headers(self.http.post(&self.messages_url))
            .json(&body)
            .send()
            .await
            .map_err(|error| LlmError::transport(format!("请求失败：{error}")))?;

        let parsed: MessagesResponse = parse_json(response).await?;
        Ok(response_to_unified(parsed))
    }

    async fn chat_stream(&self, request: ChatRequest) -> LlmResult<ChatStream> {
        let request = self.fill_defaults(request).with_stream(true);
        let body = build_request(&request, self.default_max_tokens)?;

        let response = self
            .with_headers(self.http.post(&self.messages_url))
            .json(&body)
            .send()
            .await
            .map_err(|error| LlmError::transport(format!("请求失败：{error}")))?;

        let status = response.status();
        if !status.is_success() {
            let retry = retry_after(&response);
            let text = response.text().await.unwrap_or_default();
            return Err(map_error(status.as_u16(), &text, retry));
        }

        let bytes: ByteStream = response
            .bytes_stream()
            .map(|result| {
                result
                    .map(|chunk| chunk.to_vec())
                    .map_err(|error| error.to_string())
            })
            .boxed();

        let state: StreamState = (
            bytes,
            SseDecoder::new(),
            MessageStreamParser::new(),
            VecDeque::new(),
            false,
        );

        let stream = futures::stream::unfold(state, |mut state| async move {
            loop {
                if let Some(event) = state.3.pop_front() {
                    return Some((event, state));
                }
                if state.4 {
                    return None;
                }
                match state.0.next().await {
                    Some(Ok(bytes)) => {
                        let text = String::from_utf8_lossy(&bytes).to_string();
                        let frames = state.1.push(&text);
                        for frame in frames {
                            let events = state.2.feed(&frame);
                            state.3.extend(events);
                        }
                    }
                    Some(Err(error)) => {
                        state.4 = true;
                        state
                            .3
                            .push_back(Err(LlmError::transport(format!("流中断：{error}"))));
                    }
                    None => {
                        state.4 = true;
                        if let Some(tail) = state.1.finish() {
                            let events = state.2.feed(&tail);
                            state.3.extend(events);
                        }
                    }
                }
            }
        });

        Ok(Box::pin(stream))
    }

    /// 模型列表：`GET /v1/models`，接口不存在时回落到默认模型（老版本代理可能没有该端点）
    async fn list_models(&self) -> LlmResult<Vec<ModelInfo>> {
        let response = self
            .with_headers(self.http.get(&self.models_url))
            .send()
            .await
            .map_err(|error| LlmError::transport(format!("请求失败：{error}")))?;

        match parse_json::<crate::wire::ModelsResponse>(response).await {
            Ok(parsed) => Ok(parsed
                .data
                .into_iter()
                .map(|entry| ModelInfo {
                    id: entry.id,
                    owned_by: entry.display_name,
                    created: None,
                })
                .collect()),
            Err(LlmError::InvalidRequest { status: 404, .. }) => {
                Ok(vec![ModelInfo::new(self.config.default_model.clone())])
            }
            Err(error) => Err(error),
        }
    }

    async fn health_check(&self) -> LlmResult<()> {
        self.list_models().await.map(|_| ())
    }
}

/// 统一的状态码检查 + JSON 解析
async fn parse_json<T>(response: reqwest::Response) -> LlmResult<T>
where
    T: serde::de::DeserializeOwned,
{
    let status = response.status();
    let retry = retry_after(&response);
    let body = response
        .text()
        .await
        .map_err(|error| LlmError::transport(format!("读取响应失败：{error}")))?;

    if !status.is_success() {
        return Err(map_error(status.as_u16(), &body, retry));
    }

    serde_json::from_str(&body)
        .map_err(|error| LlmError::decode(format!("{error}；原文：{}", truncate(&body, 200))))
}

/// 截断过长文本
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

    fn options(base_url: &str, model: &str) -> AnthropicOptions {
        AnthropicOptions::new(
            "claude",
            LlmClientConfig::new(base_url, "sk-ant-test-123456", model),
        )
    }

    #[test]
    fn normalize_base_url_strips_version_and_endpoints() {
        assert_eq!(
            normalize_base_url("https://api.anthropic.com"),
            "https://api.anthropic.com"
        );
        assert_eq!(
            normalize_base_url("https://api.anthropic.com/"),
            "https://api.anthropic.com"
        );
        assert_eq!(
            normalize_base_url("https://api.anthropic.com/v1"),
            "https://api.anthropic.com"
        );
        assert_eq!(
            normalize_base_url("https://api.anthropic.com/v1/messages"),
            "https://api.anthropic.com"
        );
        assert_eq!(
            normalize_base_url("https://api.anthropic.com/v1/models/"),
            "https://api.anthropic.com"
        );
        // 反向代理前缀保留
        assert_eq!(
            normalize_base_url("https://proxy.example.com/anthropic/v1"),
            "https://proxy.example.com/anthropic"
        );
        assert_eq!(normalize_base_url("  "), "");
    }

    #[test]
    fn new_builds_endpoints_from_base_url() {
        let provider = AnthropicProvider::new(options(
            "https://api.anthropic.com",
            "claude-sonnet-4-20250514",
        ))
        .expect("构造 Provider 失败");

        assert_eq!(
            provider.messages_url,
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(provider.models_url, "https://api.anthropic.com/v1/models");
        assert_eq!(provider.provider_code(), "claude");
        assert_eq!(provider.protocol(), Protocol::AnthropicMessages);
        assert_eq!(provider.default_model(), "claude-sonnet-4-20250514");
        assert_eq!(provider.default_max_tokens, DEFAULT_MAX_TOKENS);
        assert_eq!(provider.api_version, DEFAULT_API_VERSION);
    }

    #[test]
    fn new_rejects_empty_base_url_or_model() {
        assert!(matches!(
            AnthropicProvider::new(options("   ", "claude-sonnet-4-20250514")),
            Err(LlmError::Config(_))
        ));
        assert!(matches!(
            AnthropicProvider::new(options("https://api.anthropic.com", " ")),
            Err(LlmError::Config(_))
        ));
    }

    #[test]
    fn options_support_beta_headers_and_custom_max_tokens() {
        let options = options("https://api.anthropic.com", "claude-sonnet-4-20250514")
            .with_beta("prompt-caching-2024-07-31")
            .with_beta("interleaved-thinking-2025-05-14")
            .with_api_version("2024-01-01")
            .with_default_max_tokens(8192);

        assert_eq!(options.betas.len(), 2);
        assert_eq!(options.betas.join(","), "prompt-caching-2024-07-31,interleaved-thinking-2025-05-14");
        assert_eq!(options.api_version, "2024-01-01");
        assert_eq!(options.default_max_tokens, 8192);
    }

    #[test]
    fn fill_defaults_uses_configured_model() {
        let provider = AnthropicProvider::new(options(
            "https://api.anthropic.com",
            "claude-sonnet-4-20250514",
        ))
        .expect("构造 Provider 失败");

        let request = llm::ChatRequest::new("", vec![llm::Message::user("hi")]);
        assert_eq!(
            provider.fill_defaults(request).model,
            "claude-sonnet-4-20250514"
        );
    }
}


