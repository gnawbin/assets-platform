//! OpenAI 兼容 Provider 实现（第一部分：构造参数）

use std::time::Duration;

use llm::{LlmClientConfig, LlmError, LlmResult};

/// 鉴权方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStyle {
    /// `Authorization: Bearer {key}`（绝大多数厂商）
    Bearer,
    /// `api-key: {key}`（Azure OpenAI）
    ApiKey,
}

/// `max_tokens` 字段名（新模型改用 `max_completion_tokens`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaxTokensField {
    MaxTokens,
    MaxCompletionTokens,
}

/// Provider 构造参数
pub struct OpenAiOptions {
    /// 厂商编码（与 `llm_provider.provider_code` 一致）
    pub provider_code: String,
    /// 端点 / 密钥 / 默认模型
    pub config: LlmClientConfig,
    /// 鉴权方式
    pub auth: AuthStyle,
    /// `max_tokens` 字段名
    pub max_tokens_field: MaxTokensField,
    /// 是否支持 `stream_options.include_usage`（部分兼容厂商会 400）
    pub supports_stream_options: bool,
}

impl OpenAiOptions {
    /// 默认：Bearer 鉴权 + `max_tokens` + 发送 `stream_options`
    pub fn new(provider_code: impl Into<String>, config: LlmClientConfig) -> Self {
        Self {
            provider_code: provider_code.into(),
            config,
            auth: AuthStyle::Bearer,
            max_tokens_field: MaxTokensField::MaxTokens,
            supports_stream_options: true,
        }
    }

    /// 改为 `api-key` 头鉴权（Azure）
    pub fn with_auth(mut self, auth: AuthStyle) -> Self {
        self.auth = auth;
        self
    }

    /// 覆盖 `max_tokens` 字段名
    pub fn with_max_tokens_field(mut self, field: MaxTokensField) -> Self {
        self.max_tokens_field = field;
        self
    }

    /// 关闭 `stream_options`（兼容不支持该字段的厂商）
    pub fn with_stream_options(mut self, supports: bool) -> Self {
        self.supports_stream_options = supports;
        self
    }
}

/// OpenAI 兼容协议的 Provider
pub struct OpenAiProvider {
    provider_code: String,
    config: LlmClientConfig,
    auth: AuthStyle,
    max_tokens_field: MaxTokensField,
    supports_stream_options: bool,
    http: reqwest::Client,
    chat_url: String,
    embedding_url: String,
    models_url: String,
}

impl OpenAiProvider {
    /// 构造 Provider（校验 base_url / 默认模型，创建 HTTP 客户端）
    pub fn new(options: OpenAiOptions) -> LlmResult<Self> {
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
            chat_url: format!("{base}/chat/completions"),
            embedding_url: format!("{base}/embeddings"),
            models_url: format!("{base}/models"),
            provider_code: options.provider_code,
            auth: options.auth,
            max_tokens_field: options.max_tokens_field,
            supports_stream_options: options.supports_stream_options,
            config,
            http,
        })
    }

    /// POST 请求（带鉴权与自定义头）
    fn request(&self, url: &str) -> reqwest::RequestBuilder {
        self.with_auth_headers(self.http.post(url))
    }

    /// GET 请求（带鉴权与自定义头）
    fn get(&self, url: &str) -> reqwest::RequestBuilder {
        self.with_auth_headers(self.http.get(url))
    }

    /// 统一注入鉴权与自定义请求头（密钥为空时跳过，兼容本地 Ollama）
    fn with_auth_headers(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let key = self.config.api_key.trim();
        let mut builder = if key.is_empty() {
            builder
        } else {
            match self.auth {
                AuthStyle::Bearer => builder.bearer_auth(key),
                AuthStyle::ApiKey => builder.header("api-key", key),
            }
        };
        for (name, value) in &self.config.headers {
            builder = builder.header(name, value);
        }
        builder
    }

    /// 请求体 JSON（按配置决定 `max_tokens` 字段名）
    fn body_value(
        &self,
        request: &llm::ChatRequest,
        stream: bool,
        include_usage: bool,
    ) -> serde_json::Value {
        let mut value =
            serde_json::to_value(crate::convert::build_request(request, stream, include_usage))
                .unwrap_or_else(|_| serde_json::json!({}));

        if self.max_tokens_field == MaxTokensField::MaxCompletionTokens {
            if let Some(map) = value.as_object_mut() {
                if let Some(max_tokens) = map.remove("max_tokens") {
                    map.insert("max_completion_tokens".to_string(), max_tokens);
                }
            }
        }
        value
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
/// - 去掉误填的完整端点（`/chat/completions`、`/embeddings`、`/models`）；
/// - 路径为空时补 `/v1`（`https://api.openai.com` → `.../v1`）；
/// - 已有版本段（`/v1`、`/api/v3`、`/compatible-mode/v1`、`/paas/v4`）时原样保留。
pub fn normalize_base_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }

    let mut base = trimmed;
    for endpoint in ["/chat/completions", "/embeddings", "/models"] {
        if let Some(stripped) = base.strip_suffix(endpoint) {
            base = stripped.trim_end_matches('/');
            break;
        }
    }

    let path = match base.split_once("://") {
        Some((_, rest)) => rest.split_once('/').map(|(_, path)| path).unwrap_or(""),
        None => base.split_once('/').map(|(_, path)| path).unwrap_or(""),
    };

    if path.is_empty() {
        format!("{base}/v1")
    } else {
        base.to_string()
    }
}

/// 读取 `Retry-After` 头（秒）
pub(crate) fn retry_after(response: &reqwest::Response) -> Option<Duration> {
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
    ChatRequest, ChatResponse, ChatStream, EmbeddingRequest, EmbeddingResponse, LlmProvider,
    ModelInfo, Protocol, StreamEvent,
};

use crate::convert::{response_to_unified, to_usage};
use crate::error::map_error;
use crate::sse::{ChunkParser, SseDecoder};
use crate::wire::{ChatCompletionResponse, EmbeddingBody, EmbeddingResponseBody, ModelsResponse};

/// 字节流（已把 reqwest 错误统一成 String，便于放进 unfold 状态）
type ByteStream = futures::stream::BoxStream<'static, Result<Vec<u8>, String>>;
/// 流式解析状态：(字节流, SSE 解码器, 分片解析器, 待发事件队列, 是否结束)
type StreamState = (
    ByteStream,
    SseDecoder,
    ChunkParser,
    VecDeque<LlmResult<StreamEvent>>,
    bool,
);

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn provider_code(&self) -> &str {
        &self.provider_code
    }

    fn protocol(&self) -> Protocol {
        Protocol::OpenAiCompatible
    }

    fn default_model(&self) -> &str {
        &self.config.default_model
    }

    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatResponse> {
        let request = self.fill_defaults(request);
        let body = self.body_value(&request, false, false);
        let response = self
            .request(&self.chat_url)
            .json(&body)
            .send()
            .await
            .map_err(|error| LlmError::transport(format!("请求失败：{error}")))?;

        let parsed: ChatCompletionResponse = parse_json(response).await?;
        Ok(response_to_unified(parsed))
    }

    async fn chat_stream(&self, request: ChatRequest) -> LlmResult<ChatStream> {
        let request = self.fill_defaults(request);
        let body = self.body_value(&request, true, self.supports_stream_options);
        let response = self
            .request(&self.chat_url)
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
            ChunkParser::new(),
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
                            let events = state.2.feed(&frame.data);
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
                            let events = state.2.feed(&tail.data);
                            state.3.extend(events);
                        }
                    }
                }
            }
        });

        Ok(Box::pin(stream))
    }

    async fn embed(&self, request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        let model = if request.model.trim().is_empty() {
            self.config.default_model.clone()
        } else {
            request.model.clone()
        };
        let body = EmbeddingBody {
            model: model.clone(),
            input: request.input,
        };

        let response = self
            .request(&self.embedding_url)
            .json(&body)
            .send()
            .await
            .map_err(|error| LlmError::transport(format!("请求失败：{error}")))?;

        let parsed: EmbeddingResponseBody = parse_json(response).await?;
        let mut entries: Vec<(u32, Vec<f32>)> = parsed
            .data
            .into_iter()
            .map(|entry| (entry.index, entry.embedding))
            .collect();
        entries.sort_by_key(|(index, _)| *index);

        Ok(EmbeddingResponse {
            model: if parsed.model.is_empty() {
                model
            } else {
                parsed.model
            },
            embeddings: entries.into_iter().map(|(_, embedding)| embedding).collect(),
            usage: parsed
                .usage
                .as_ref()
                .map_or_else(llm::Usage::default, to_usage),
        })
    }

    async fn list_models(&self) -> LlmResult<Vec<ModelInfo>> {
        let response = self
            .get(&self.models_url)
            .send()
            .await
            .map_err(|error| LlmError::transport(format!("请求失败：{error}")))?;

        let parsed: ModelsResponse = parse_json(response).await?;
        Ok(parsed
            .data
            .into_iter()
            .map(|entry| ModelInfo {
                id: entry.id,
                owned_by: entry.owned_by,
                created: entry.created,
            })
            .collect())
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

    #[test]
    fn normalize_base_url_keeps_version_segments() {
        assert_eq!(
            normalize_base_url("https://api.deepseek.com/v1"),
            "https://api.deepseek.com/v1"
        );
        assert_eq!(
            normalize_base_url("https://ark.cn-beijing.volces.com/api/v3/"),
            "https://ark.cn-beijing.volces.com/api/v3"
        );
        assert_eq!(
            normalize_base_url("https://dashscope.aliyuncs.com/compatible-mode/v1"),
            "https://dashscope.aliyuncs.com/compatible-mode/v1"
        );
        assert_eq!(
            normalize_base_url("https://open.bigmodel.cn/api/paas/v4"),
            "https://open.bigmodel.cn/api/paas/v4"
        );
    }

    #[test]
    fn normalize_base_url_appends_v1_when_path_missing() {
        assert_eq!(
            normalize_base_url("https://api.openai.com"),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            normalize_base_url("https://api.openai.com/"),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            normalize_base_url("http://localhost:11434"),
            "http://localhost:11434/v1"
        );
        assert_eq!(normalize_base_url("   "), "");
    }

    #[test]
    fn normalize_base_url_strips_pasted_endpoints() {
        assert_eq!(
            normalize_base_url("https://api.openai.com/v1/chat/completions"),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            normalize_base_url("https://api.openai.com/v1/embeddings"),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            normalize_base_url("https://api.openai.com/v1/models"),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn new_rejects_empty_base_url_or_model() {
        let empty_base = OpenAiOptions::new(
            "openai",
            LlmClientConfig::new("  ", "sk-test-123456", "gpt-4o"),
        );
        assert!(matches!(
            OpenAiProvider::new(empty_base),
            Err(LlmError::Config(_))
        ));

        let empty_model =
            OpenAiOptions::new("openai", LlmClientConfig::new("https://api.openai.com", "", " "));
        assert!(matches!(
            OpenAiProvider::new(empty_model),
            Err(LlmError::Config(_))
        ));
    }

    #[test]
    fn provider_exposes_protocol_and_default_model() {
        let provider = OpenAiProvider::new(OpenAiOptions::new(
            "deepseek",
            LlmClientConfig::new("https://api.deepseek.com/v1", "sk-test-123456", "deepseek-chat"),
        ))
        .expect("构造 Provider 失败");

        assert_eq!(provider.provider_code(), "deepseek");
        assert_eq!(provider.protocol(), Protocol::OpenAiCompatible);
        assert_eq!(provider.default_model(), "deepseek-chat");
        assert_eq!(
            provider.chat_url,
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert_eq!(provider.models_url, "https://api.deepseek.com/v1/models");
    }

    #[test]
    fn fill_defaults_uses_configured_model() {
        let provider = OpenAiProvider::new(OpenAiOptions::new(
            "openai",
            LlmClientConfig::new("https://api.openai.com/v1", "sk-test-123456", "gpt-4o"),
        ))
        .expect("构造 Provider 失败");

        let request = llm::ChatRequest::new("", vec![llm::Message::user("hi")]);
        assert_eq!(provider.fill_defaults(request).model, "gpt-4o");
    }

    #[test]
    fn max_tokens_field_can_be_switched_for_new_models() {
        let legacy = OpenAiProvider::new(OpenAiOptions::new(
            "openai",
            LlmClientConfig::new("https://api.openai.com/v1", "sk-test-123456", "gpt-4o"),
        ))
        .expect("构造 Provider 失败");
        let request = llm::ChatRequest::new("gpt-4o", vec![llm::Message::user("hi")])
            .with_max_tokens(64);
        let body = legacy.body_value(&request, false, false);
        assert_eq!(body["max_tokens"], 64);
        assert!(body.get("max_completion_tokens").is_none());

        let modern = OpenAiProvider::new(
            OpenAiOptions::new(
                "openai",
                LlmClientConfig::new("https://api.openai.com/v1", "sk-test-123456", "gpt-4o"),
            )
            .with_max_tokens_field(MaxTokensField::MaxCompletionTokens),
        )
        .expect("构造 Provider 失败");
        let body = modern.body_value(&request, false, false);
        assert_eq!(body["max_completion_tokens"], 64);
        assert!(body.get("max_tokens").is_none());
    }
}




