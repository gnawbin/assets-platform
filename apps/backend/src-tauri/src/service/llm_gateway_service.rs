//! LLM 网关核心服务
//!
//! 提供统一的 LLM 调用接口：LLMProviderAdapter trait、路由引擎、负载均衡、熔断器。

use crate::database;
use crate::database::models::{
    ChatMessage, LLMChatRequest, LLMChatResponse, LLMEmbeddingRequest, LLMEmbeddingResponse,
    LlmModel, LlmProvider, TokenUsage,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};
use tracing::{error, info};

// ======================== Provider Adapter Trait ========================

/// 厂商适配器接口
#[async_trait]
pub trait LLMProviderAdapter: Send + Sync {
    /// 对话
    async fn chat(&self, request: LLMChatRequest) -> Result<LLMChatResponse, String>;

    /// Embedding
    async fn embedding(&self, request: LLMEmbeddingRequest)
        -> Result<LLMEmbeddingResponse, String>;

    /// 健康检查
    async fn health_check(&self) -> Result<bool, String>;

    /// 获取支持的模型列表
    async fn list_models(&self) -> Result<Vec<String>, String>;

    /// 获取厂商编码
    fn provider_code(&self) -> &str;
}

// ======================== 负载均衡 ========================

/// 带权重的 Provider 信息
#[derive(Clone)]
pub struct WeightedProvider {
    pub provider_id: i64,
    pub provider_code: String,
    pub adapter: std::sync::Arc<Box<dyn LLMProviderAdapter>>,
    pub weight: i32,
    pub model_type: String,
}

/// 负载均衡器
pub struct LoadBalancer {
    providers: Vec<WeightedProvider>,
    failure_counts: HashMap<i64, i32>,
}

impl LoadBalancer {
    pub fn new(providers: Vec<WeightedProvider>) -> Self {
        Self {
            providers,
            failure_counts: HashMap::new(),
        }
    }

    /// 按模型类型和权重选择 Provider
    pub fn select(
        &mut self,
        model_type: &str,
        provider_id: Option<i64>,
    ) -> Result<&WeightedProvider, String> {
        // 如果指定了 provider_id，优先使用
        if let Some(pid) = provider_id {
            return self
                .providers
                .iter()
                .find(|p| p.provider_id == pid && p.model_type == model_type)
                .ok_or_else(|| "指定的 Provider 不可用".to_string());
        }

        let candidates: Vec<&WeightedProvider> = self
            .providers
            .iter()
            .filter(|p| p.model_type == model_type)
            .filter(|p| {
                let fails = self.failure_counts.get(&p.provider_id).unwrap_or(&0);
                *fails < 3
            })
            .collect();

        if candidates.is_empty() {
            return Err("没有可用的 Provider".to_string());
        }

        // 权重随机选择
        let total_weight: i32 = candidates.iter().map(|p| p.weight).sum();
        let mut threshold = rand::random::<i32>().abs() % total_weight + 1;

        for candidate in &candidates {
            threshold -= candidate.weight;
            if threshold <= 0 {
                return Ok(candidate);
            }
        }

        Ok(candidates.last().unwrap())
    }

    pub fn record_success(&mut self, provider_id: i64) {
        self.failure_counts.insert(provider_id, 0);
    }

    pub fn record_failure(&mut self, provider_id: i64) {
        let count = self.failure_counts.entry(provider_id).or_insert(0);
        *count += 1;
    }
}

// ======================== 熔断器 ========================

enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

pub struct CircuitBreaker {
    state: CircuitState,
    failure_threshold: i32,
    recovery_timeout: Duration,
    failure_count: AtomicI32,
    last_failure_time: std::sync::Mutex<Option<Instant>>,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: i32, recovery_timeout_secs: u64) -> Self {
        Self {
            state: CircuitState::Closed,
            failure_threshold,
            recovery_timeout: Duration::from_secs(recovery_timeout_secs),
            failure_count: AtomicI32::new(0),
            last_failure_time: std::sync::Mutex::new(None),
        }
    }

    /// 检查是否允许调用
    pub fn is_allowed(&self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                let last = self.last_failure_time.lock().unwrap();
                if let Some(time) = *last {
                    if time.elapsed() >= self.recovery_timeout {
                        return true; // Half-open 自动转换
                    }
                }
                false
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// 记录成功，重置熔断器
    pub fn record_success(&mut self) {
        self.failure_count.store(0, Ordering::SeqCst);
        self.state = CircuitState::Closed;
    }

    /// 记录失败
    pub fn record_failure(&mut self) {
        let count = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
        *self.last_failure_time.lock().unwrap() = Some(Instant::now());

        if count >= self.failure_threshold {
            self.state = CircuitState::Open;
            error!("熔断器开启：连续失败 {} 次", count);
        }
    }
}

// ======================== LLM Router ========================

/// LLM 路由网关
pub struct LLMRouter {
    load_balancer: std::sync::Mutex<LoadBalancer>,
    circuit_breaker: std::sync::Mutex<CircuitBreaker>,
}

impl LLMRouter {
    pub fn new() -> Self {
        Self {
            load_balancer: std::sync::Mutex::new(LoadBalancer::new(Vec::new())),
            circuit_breaker: std::sync::Mutex::new(CircuitBreaker::new(5, 60)),
        }
    }

    /// 刷新 Provider 列表（从数据库加载）
    pub async fn refresh_providers(&self) -> Result<(), String> {
        let pool = database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
        let prefix = database::schema_prefix();
        let providers = sqlx::query_as::<_, LlmProvider>(sqlx::AssertSqlSafe(
            format!(
                "SELECT id, provider_code, provider_name, base_url, api_key, secret_key, \
                 extra_config, weight, is_local, enable, created_by, created_at, updated_at, deleted \
                 FROM {}llm_provider WHERE enable = true AND deleted = 0",
                prefix
            )
        ))
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("查询厂商失败: {}", e))?;

        let mut weighted = Vec::new();
        for p in providers {
            // 查询该 Provider 的第一个已启用的 chat 模型作为默认模型
            let default_model: Option<String> = sqlx::query_scalar(
                sqlx::AssertSqlSafe(format!(
                    "SELECT model_code FROM {}llm_model WHERE provider_id = $1 AND model_type = 'chat' AND enable = true AND deleted = 0 ORDER BY id LIMIT 1",
                    prefix
                ))
            )
            .bind(p.id)
            .fetch_optional(&pool)
            .await
            .map_err(|e| format!("查询默认模型失败: {}", e))?;

            match create_adapter_with_model(&p, default_model.as_deref()) {
                Ok(adapter) => {
                    let weight = p.weight.unwrap_or(10);
                    weighted.push(WeightedProvider {
                        provider_id: p.id,
                        provider_code: p.provider_code.clone(),
                        adapter: std::sync::Arc::new(adapter),
                        weight,
                        model_type: "chat".to_string(),
                    });
                }
                Err(e) => {
                    error!("跳过 Provider {} ({}): {}", p.id, p.provider_code, e);
                }
            }
        }

        let mut lb = self.load_balancer.lock().unwrap();
        *lb = LoadBalancer::new(weighted);
        Ok(())
    }

    /// 获取所有可用的 chat provider ID 列表
    fn get_chat_provider_ids(&self) -> Vec<i64> {
        let lb = self.load_balancer.lock().unwrap();
        lb.providers
            .iter()
            .filter(|p| p.model_type == "chat")
            .map(|p| p.provider_id)
            .collect()
    }

    /// 使用指定 provider 调用 Chat（内部方法，不锁熔断器）
    async fn chat_with_provider(
        &self,
        request: LLMChatRequest,
        provider_id: i64,
    ) -> Result<LLMChatResponse, String> {
        let adapter = {
            let lb = self.load_balancer.lock().unwrap();
            lb.providers
                .iter()
                .find(|p| p.provider_id == provider_id)
                .map(|p| p.adapter.clone())
        };

        let adapter = match adapter {
            Some(a) => a,
            None => return Err("指定的 Provider 不可用".to_string()),
        };

        let result = adapter.chat(request).await;

        let mut lb = self.load_balancer.lock().unwrap();
        match &result {
            Ok(_) => lb.record_success(provider_id),
            Err(_) => lb.record_failure(provider_id),
        }

        result
    }

    /// 使用指定 Provider 调用 Chat（如果 provider_id 为 None，则自动故障转移）
    pub async fn chat_with_provider_id(
        &self,
        request: LLMChatRequest,
        provider_id: Option<i64>,
    ) -> Result<LLMChatResponse, String> {
        if let Some(pid) = provider_id {
            // 使用用户指定的 provider
            self.chat_with_provider(request, pid).await
        } else {
            // 未指定则走自动故障转移
            self.chat(request).await
        }
    }

    /// 调用 Chat（支持多 Provider 自动故障转移）
    pub async fn chat(&self, request: LLMChatRequest) -> Result<LLMChatResponse, String> {
        // 检查熔断器
        {
            let cb = self.circuit_breaker.lock().unwrap();
            if !cb.is_allowed() {
                return Err("LLM 服务暂时不可用（熔断器开启）".to_string());
            }
        }

        // 获取所有可用的 chat provider
        let provider_ids = self.get_chat_provider_ids();
        if provider_ids.is_empty() {
            return Err("没有可用的 Provider".to_string());
        }

        let mut last_error = String::new();

        // 逐个尝试每个 provider，直到成功
        for &pid in &provider_ids {
            info!("尝试调用 Provider {}", pid);
            match self.chat_with_provider(request.clone(), pid).await {
                Ok(resp) => {
                    info!("Provider {} 调用成功", pid);
                    // 成功时重置熔断器
                    let mut cb = self.circuit_breaker.lock().unwrap();
                    cb.record_success();
                    return Ok(resp);
                }
                Err(e) => {
                    error!("Provider {} 调用失败: {}", pid, e);
                    last_error = e;
                    // 记录失败到熔断器
                    let mut cb = self.circuit_breaker.lock().unwrap();
                    cb.record_failure();
                }
            }
        }

        Err(format!("所有 LLM Provider 都不可用: {}", last_error))
    }

    /// 调用 Embedding
    pub async fn embedding(
        &self,
        request: LLMEmbeddingRequest,
    ) -> Result<LLMEmbeddingResponse, String> {
        let provider_id = None;

        let selected = {
            let mut lb = self.load_balancer.lock().unwrap();
            lb.select("embedding", provider_id)?.clone()
        };

        let result = selected.adapter.embedding(request).await;

        let mut lb = self.load_balancer.lock().unwrap();
        match &result {
            Ok(_) => lb.record_success(selected.provider_id),
            Err(_) => lb.record_failure(selected.provider_id),
        }

        result
    }
}

// ======================== 适配器工厂 ========================

/// 根据厂商配置创建适配器实例（使用数据库中的默认模型）
pub fn create_adapter_with_model(
    provider: &LlmProvider,
    default_model: Option<&str>,
) -> Result<Box<dyn LLMProviderAdapter>, String> {
    let base_url = provider
        .base_url
        .clone()
        .unwrap_or_else(|| get_default_base_url(&provider.provider_code));

    // 解密 API Key
    let api_key = match &provider.api_key {
        Some(key) if !key.is_empty() && key.len() > 20 => {
            // 尝试解密，如果失败则使用原始值（可能是未加密的）
            match crate::utils::crypto::decrypt_api_key(key) {
                Ok(k) => k,
                Err(_) => key.clone(),
            }
        }
        Some(key) => key.clone(),
        None => String::new(),
    };

    // 根据 provider_code 选择默认模型
    let model = default_model
        .unwrap_or_else(|| get_default_model(&provider.provider_code))
        .to_string();

    match provider.provider_code.as_str() {
        "openai" => Ok(Box::new(adapters::OpenAIAdapter::new(
            &api_key,
            &base_url,
            &provider.provider_code,
            &model,
        ))),
        "qwen" | "ollama" | "volcengine" | "deepseek" | "tencent" => {
            // OpenAI 兼容格式
            Ok(Box::new(adapters::OpenAIAdapter::new(
                &api_key,
                &base_url,
                &provider.provider_code,
                &model,
            )))
        }
        "claude" => Ok(Box::new(adapters::ClaudeAdapter::new(&api_key, &base_url))),
        _ => Err(format!("不支持的厂商类型: {}", provider.provider_code)),
    }
}

/// 根据厂商代码获取默认模型名（当数据库中未配置时使用）
fn get_default_model(provider_code: &str) -> &'static str {
    match provider_code {
        "openai" => "gpt-4o",
        "deepseek" => "deepseek-v4-flash",
        "qwen" => "qwen-turbo",
        "volcengine" => "doubao-lite-32k",
        "tencent" => "hunyuan-lite",
        "ollama" => "llama3.2",
        "claude" => "claude-3-5-sonnet-20241022",
        _ => "gpt-4o",
    }
}

/// 根据厂商配置创建对应的适配器实例（向后兼容，不使用默认模型时使用 hardcoded 值）
pub fn create_adapter(provider: &LlmProvider) -> Result<Box<dyn LLMProviderAdapter>, String> {
    create_adapter_with_model(provider, None)
}

fn get_default_base_url(provider_code: &str) -> String {
    match provider_code {
        "openai" => "https://api.openai.com".to_string(),
        "claude" => "https://api.anthropic.com".to_string(),
        "qwen" => "https://dashscope.aliyuncs.com".to_string(),
        "volcengine" => "https://ark.cn-beijing.volces.com".to_string(),
        "tencent" => "https://api.hunyuan.cloud.tencent.com".to_string(),
        "deepseek" => "https://api.deepseek.com".to_string(),
        "ollama" => "http://localhost:11434".to_string(),
        _ => "http://localhost:11434".to_string(),
    }
}

/// 适配器实现模块
pub mod adapters {
    use super::*;

    // ======================== OpenAI 适配器（也用于 Qwen / Ollama） ========================

    pub struct OpenAIAdapter {
        api_key: String,
        base_url: String,
        provider_code: String,
        default_model: String,
        http_client: reqwest::Client,
    }

    impl OpenAIAdapter {
        pub fn new(
            api_key: &str,
            base_url: &str,
            provider_code: &str,
            default_model: &str,
        ) -> Self {
            Self {
                api_key: api_key.to_string(),
                base_url: base_url.trim_end_matches('/').to_string(),
                provider_code: provider_code.to_string(),
                default_model: default_model.to_string(),
                http_client: reqwest::Client::new(),
            }
        }
    }

    #[async_trait]
    impl LLMProviderAdapter for OpenAIAdapter {
        fn provider_code(&self) -> &str {
            &self.provider_code
        }

        async fn chat(&self, request: LLMChatRequest) -> Result<LLMChatResponse, String> {
            let url = format!("{}/v1/chat/completions", self.base_url);

            let messages: Vec<serde_json::Value> = request
                .messages
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "role": m.role,
                        "content": m.content
                    })
                })
                .collect();

            let model_name = request.model.unwrap_or_else(|| self.default_model.clone());
            let body = serde_json::json!({
                "model": model_name,
                "messages": messages,
                "temperature": request.temperature.unwrap_or(0.7),
                "max_tokens": request.max_tokens.unwrap_or(2048),
                "stream": false,
            });

            let resp = self
                .http_client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("请求失败: {}", e))?;

            let status = resp.status();
            let text = resp
                .text()
                .await
                .map_err(|e| format!("读取响应失败: {}", e))?;

            if !status.is_success() {
                return Err(format!("API 错误 [{}]: {}", status, text));
            }

            let json: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| format!("解析响应失败: {}", e))?;

            let content = json["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string();
            let model = json["model"].as_str().unwrap_or("").to_string();
            let input_tokens = json["usage"]["prompt_tokens"].as_i64().unwrap_or(0) as i32;
            let output_tokens = json["usage"]["completion_tokens"].as_i64().unwrap_or(0) as i32;

            Ok(LLMChatResponse {
                content,
                model,
                usage: TokenUsage {
                    input_tokens,
                    output_tokens,
                    total_tokens: input_tokens + output_tokens,
                    cost: 0.0,
                },
                provider_id: 0,
                model_id: 0,
                request_id: String::new(),
            })
        }

        async fn embedding(
            &self,
            request: LLMEmbeddingRequest,
        ) -> Result<LLMEmbeddingResponse, String> {
            let url = format!("{}/v1/embeddings", self.base_url);

            let body = serde_json::json!({
                "model": request.model.unwrap_or_else(|| "text-embedding-3-small".to_string()),
                "input": request.input,
            });

            let resp = self
                .http_client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("请求失败: {}", e))?;

            let text = resp
                .text()
                .await
                .map_err(|e| format!("读取响应失败: {}", e))?;
            let json: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| format!("解析响应失败: {}", e))?;

            let embeddings: Vec<Vec<f32>> = json["data"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|item| {
                    item["embedding"]
                        .as_array()
                        .unwrap_or(&vec![])
                        .iter()
                        .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                        .collect()
                })
                .collect();

            let model = json["model"].as_str().unwrap_or("").to_string();
            let input_tokens = json["usage"]["prompt_tokens"].as_i64().unwrap_or(0) as i32;

            Ok(LLMEmbeddingResponse {
                embeddings,
                model,
                usage: TokenUsage {
                    input_tokens,
                    output_tokens: 0,
                    total_tokens: input_tokens,
                    cost: 0.0,
                },
                provider_id: 0,
                model_id: 0,
            })
        }

        async fn health_check(&self) -> Result<bool, String> {
            let url = format!("{}/v1/models", self.base_url);
            let resp = self.http_client.get(&url).send().await.map_err(|e| {
                error!("健康检查失败: {}", e);
                format!("健康检查失败: {}", e)
            })?;
            Ok(resp.status().is_success())
        }

        async fn list_models(&self) -> Result<Vec<String>, String> {
            let url = format!("{}/v1/models", self.base_url);
            let resp = self.http_client.get(&url).send().await.map_err(|e| {
                error!("获取模型列表失败: {}", e);
                format!("获取模型列表失败: {}", e)
            })?;

            let text = resp
                .text()
                .await
                .map_err(|e| format!("读取响应失败: {}", e))?;
            let json: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| format!("解析响应失败: {}", e))?;

            let models: Vec<String> = json["data"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
                .collect();
            Ok(models)
        }
    }

    // ======================== Claude 适配器 ========================

    pub struct ClaudeAdapter {
        api_key: String,
        base_url: String,
        http_client: reqwest::Client,
    }

    impl ClaudeAdapter {
        pub fn new(api_key: &str, base_url: &str) -> Self {
            Self {
                api_key: api_key.to_string(),
                base_url: base_url.trim_end_matches('/').to_string(),
                http_client: reqwest::Client::new(),
            }
        }

        fn extract_system(messages: &[ChatMessage]) -> (Option<String>, Vec<ChatMessage>) {
            let mut system = None;
            let mut rest = Vec::new();
            for msg in messages {
                if msg.role == "system" && system.is_none() {
                    system = Some(msg.content.clone());
                } else {
                    rest.push(msg.clone());
                }
            }
            (system, rest)
        }
    }

    #[async_trait]
    impl LLMProviderAdapter for ClaudeAdapter {
        fn provider_code(&self) -> &str {
            "claude"
        }

        async fn chat(&self, request: LLMChatRequest) -> Result<LLMChatResponse, String> {
            let (system, messages) = Self::extract_system(&request.messages);

            let url = format!("{}/v1/messages", self.base_url);

            let claude_messages: Vec<serde_json::Value> = messages
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "role": m.role,
                        "content": m.content
                    })
                })
                .collect();

            let mut body = serde_json::json!({
                "model": request.model.unwrap_or_else(|| "claude-3-5-sonnet-20241022".to_string()),
                "messages": claude_messages,
                "max_tokens": request.max_tokens.unwrap_or(2048),
            });

            if let Some(s) = system {
                body["system"] = serde_json::json!(s);
            }
            if let Some(temp) = request.temperature {
                body["temperature"] = serde_json::json!(temp);
            }

            let resp = self
                .http_client
                .post(&url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("请求失败: {}", e))?;

            let status = resp.status();
            let text = resp
                .text()
                .await
                .map_err(|e| format!("读取响应失败: {}", e))?;

            if !status.is_success() {
                return Err(format!("Claude API 错误 [{}]: {}", status, text));
            }

            let json: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| format!("解析响应失败: {}", e))?;

            // 提取 content（Claude 返回格式不同）
            let content = json["content"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|c| c["text"].as_str())
                .unwrap_or("")
                .to_string();

            let model = json["model"].as_str().unwrap_or("").to_string();
            let input_tokens = json["usage"]["input_tokens"].as_i64().unwrap_or(0) as i32;
            let output_tokens = json["usage"]["output_tokens"].as_i64().unwrap_or(0) as i32;

            Ok(LLMChatResponse {
                content,
                model,
                usage: TokenUsage {
                    input_tokens,
                    output_tokens,
                    total_tokens: input_tokens + output_tokens,
                    cost: 0.0,
                },
                provider_id: 0,
                model_id: 0,
                request_id: String::new(),
            })
        }

        async fn embedding(
            &self,
            _request: LLMEmbeddingRequest,
        ) -> Result<LLMEmbeddingResponse, String> {
            Err("Claude 不支持 embedding 功能".to_string())
        }

        async fn health_check(&self) -> Result<bool, String> {
            Ok(true)
        }

        async fn list_models(&self) -> Result<Vec<String>, String> {
            Ok(vec![
                "claude-3-5-sonnet-20241022".to_string(),
                "claude-3-5-haiku-20241022".to_string(),
                "claude-3-opus-20240229".to_string(),
            ])
        }
    }
}
