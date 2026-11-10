//! LLM 网关核心服务
//!
//! 协议实现已经拆出到独立 crate，本模块只保留网关职责：
//! - **装载**：`refresh_providers()` 从 `llm_provider` / `llm_model` 表构造协议实现；
//! - **路由**：按 `model_type`（chat / vision / embedding）挑选候选 Provider；
//! - **容错**：权重负载均衡计数 + 熔断器 + 多 Provider 故障转移；
//! - **桥接**：`database::models` DTO ↔ `llm` 统一类型（见 [`crate::llm_factory`]）。
//!
//! | 依赖 | 说明 |
//! |------|------|
//! | `llm` | 统一类型与 trait（零 HTTP） |
//! | `llm-openai` | OpenAI Chat Completions（含全部兼容厂商） |
//! | `llm-anthropic` | Anthropic Messages |
//!
//! 错误对上层仍以 `String` 暴露，但统一为 `"{kind}: {message}"`，前端可据此分类提示。

use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use database::models::{
    LLMChatRequest, LLMChatResponse, LLMEmbeddingRequest, LLMEmbeddingResponse, LlmProvider,
};
use llm::{ChatStream, LlmError, LlmProvider as LlmProviderTrait};
use tracing::{error, info};

pub use crate::llm_factory::{
    create_provider, get_default_base_url, get_default_model, to_db_response, to_llm_request,
};
use crate::llm_factory::{to_db_embedding, to_llm_embedding};

/// 每个模型类型（chat / vision / embedding）对应一个条目
#[derive(Clone)]
pub struct WeightedProvider {
    pub provider_id: i64,
    pub provider_code: String,
    /// 协议实现（OpenAI 兼容或 Anthropic）
    pub provider: Arc<dyn LlmProviderTrait>,
    /// 负载均衡权重
    pub weight: i32,
    /// `chat` / `vision` / `embedding`
    pub model_type: String,
    /// 该条目使用的模型（数据库配置优先，否则用预设默认值）
    pub model: String,
}

// ======================== 负载均衡 ========================

/// 负载均衡器：权重随机选择 + 失败计数（同一 Provider 连续失败 3 次后跳过）
pub struct LoadBalancer {
    pub providers: Vec<WeightedProvider>,
    failure_counts: HashMap<i64, i32>,
}

impl LoadBalancer {
    pub fn new(providers: Vec<WeightedProvider>) -> Self {
        Self {
            providers,
            failure_counts: HashMap::new(),
        }
    }

    /// 按模型类型与权重选择 Provider（显式指定 `provider_id` 时优先）
    pub fn select(
        &mut self,
        model_type: &str,
        provider_id: Option<i64>,
    ) -> Result<&WeightedProvider, String> {
        if let Some(provider_id) = provider_id {
            return self
                .providers
                .iter()
                .find(|entry| entry.provider_id == provider_id && entry.model_type == model_type)
                .ok_or_else(|| "指定的 Provider 不可用".to_string());
        }

        let candidates: Vec<&WeightedProvider> = self
            .providers
            .iter()
            .filter(|entry| entry.model_type == model_type)
            .filter(|entry| {
                let failures = self.failure_counts.get(&entry.provider_id).unwrap_or(&0);
                *failures < 3
            })
            .collect();

        if candidates.is_empty() {
            return Err("没有可用的 Provider".to_string());
        }

        let total_weight: i32 = candidates.iter().map(|entry| entry.weight).sum();
        let mut threshold = if total_weight > 0 {
            rand::random::<i32>().abs() % total_weight + 1
        } else {
            1
        };

        for candidate in &candidates {
            threshold -= candidate.weight.max(0);
            if threshold <= 0 {
                return Ok(candidate);
            }
        }

        Ok(candidates.last().unwrap())
    }

    /// 指定模型类型的全部条目（克隆出来，避免跨 `await` 持锁）
    pub fn entries(&self, model_type: &str) -> Vec<WeightedProvider> {
        self.providers
            .iter()
            .filter(|entry| entry.model_type == model_type)
            .cloned()
            .collect()
    }

    /// 查找指定 Provider 的条目
    pub fn find(&self, provider_id: i64, model_type: &str) -> Option<WeightedProvider> {
        self.providers
            .iter()
            .find(|entry| entry.provider_id == provider_id && entry.model_type == model_type)
            .cloned()
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

/// 熔断器：连续可重试失败达到阈值后短暂跳闸，超时后自动半开
pub struct CircuitBreaker {
    state: CircuitState,
    failure_threshold: i32,
    recovery_timeout: Duration,
    failure_count: AtomicI32,
    last_failure_time: Mutex<Option<Instant>>,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: i32, recovery_timeout_secs: u64) -> Self {
        Self {
            state: CircuitState::Closed,
            failure_threshold,
            recovery_timeout: Duration::from_secs(recovery_timeout_secs),
            failure_count: AtomicI32::new(0),
            last_failure_time: Mutex::new(None),
        }
    }

    /// 检查是否允许调用（Open 且超过恢复时间则转入半开）
    pub fn is_allowed(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                let elapsed = self
                    .last_failure_time
                    .lock()
                    .unwrap()
                    .map(|time| time.elapsed())
                    .unwrap_or_default();
                if elapsed >= self.recovery_timeout {
                    // 半开：放行一次探测，成功即回到 Closed
                    self.state = CircuitState::HalfOpen;
                    true
                } else {
                    false
                }
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

/// LLM 路由网关（进程内单例，注册在 Tauri 状态里）
pub struct LLMRouter {
    load_balancer: Mutex<LoadBalancer>,
    circuit_breaker: Mutex<CircuitBreaker>,
}

impl Default for LLMRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl LLMRouter {
    pub fn new() -> Self {
        Self {
            load_balancer: Mutex::new(LoadBalancer::new(Vec::new())),
            circuit_breaker: Mutex::new(CircuitBreaker::new(5, 60)),
        }
    }

    /// 熔断器是否放行
    fn circuit_allows(&self) -> bool {
        self.circuit_breaker.lock().unwrap().is_allowed()
    }

    /// 记录一次调用结果
    ///
    /// - 负载均衡器：任何失败都计数（用于跳过坏 Provider）；
    /// - 熔断器：只统计**可重试**错误（鉴权 / 参数错误重试无意义，不该拖垮全局）。
    fn record(&self, provider_id: i64, result: &Result<(), LlmError>) {
        {
            let mut balancer = self.load_balancer.lock().unwrap();
            match result {
                Ok(()) => balancer.record_success(provider_id),
                Err(_) => balancer.record_failure(provider_id),
            }
        }

        let mut breaker = self.circuit_breaker.lock().unwrap();
        match result {
            Ok(()) => breaker.record_success(),
            Err(error) if error.is_retryable() => breaker.record_failure(),
            Err(_) => {}
        }
    }

    /// 指定模型类型的候选 Provider（按数据库权重排序）
    fn provider_ids(&self, model_type: &str) -> Vec<i64> {
        self.load_balancer
            .lock()
            .unwrap()
            .entries(model_type)
            .iter()
            .map(|entry| entry.provider_id)
            .collect()
    }

    /// 查找指定 Provider 的条目
    fn entry(&self, provider_id: i64, model_type: &str) -> Option<WeightedProvider> {
        self.load_balancer
            .lock()
            .unwrap()
            .find(provider_id, model_type)
    }
}

/// 错误转字符串（带分类前缀，便于前端判别）
fn error_string(error: &LlmError) -> String {
    format!("{}: {}", error.kind(), error)
}

/// 从数据库读取某厂商某类型的首个启用模型
async fn load_model_code(
    pool: &sqlx::PgPool,
    provider_id: i64,
    model_type: &str,
) -> Result<Option<String>, String> {
    let prefix = database::schema_prefix();
    let sql = format!(
        "SELECT model_code FROM {}llm_model WHERE provider_id = $1 AND model_type = $2 \
         AND enable = true AND deleted = 0 ORDER BY id LIMIT 1",
        prefix
    );
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(provider_id)
        .bind(model_type)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("查询 {} 模型失败: {}", model_type, e))
}

/// 需要装载的模型类型
const MODEL_TYPES: [&str; 3] = ["chat", "embedding", "vision"];

impl LLMRouter {
    /// 从数据库重新加载厂商与模型
    ///
    /// 每次对话前调用，保证「LLM 厂商配置」页面的改动即时生效，
    /// 与旧实现（每次刷新重建适配器）行为一致。
    pub async fn refresh_providers(&self) -> Result<(), String> {
        let pool = database::get_read_pool().map_err(|e| format!("数据库连接失败: {}", e))?;
        let prefix = database::schema_prefix();

        let providers = sqlx::query_as::<_, LlmProvider>(sqlx::AssertSqlSafe(format!(
            "SELECT id, provider_code, provider_name, base_url, api_key, secret_key, \
             extra_config, weight, is_local, enable, created_by, created_at, updated_at, deleted \
             FROM {}llm_provider WHERE enable = true AND deleted = 0 ORDER BY weight DESC",
            prefix
        )))
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("查询厂商失败: {}", e))?;

        let mut weighted = Vec::new();

        for provider in providers {
            let weight = provider.weight.unwrap_or(10);

            for model_type in MODEL_TYPES {
                let model_code = match load_model_code(&pool, provider.id, model_type).await {
                    Ok(code) => code,
                    Err(e) => {
                        error!("{}", e);
                        None
                    }
                };

                match create_provider(&provider, model_code.as_deref()) {
                    Ok(implementation) => {
                        let model = model_code
                            .unwrap_or_else(|| get_default_model(&provider.provider_code));
                        weighted.push(WeightedProvider {
                            provider_id: provider.id,
                            provider_code: provider.provider_code.clone(),
                            provider: implementation,
                            weight,
                            model_type: model_type.to_string(),
                            model,
                        });
                    }
                    Err(e) => error!(
                        "跳过 Provider {} {}（{}）: {}",
                        provider.id, model_type, provider.provider_code, e
                    ),
                }
            }
        }

        let chat_count = weighted
            .iter()
            .filter(|entry| entry.model_type == "chat")
            .count();
        let vision_count = weighted
            .iter()
            .filter(|entry| entry.model_type == "vision")
            .count();
        let embedding_count = weighted
            .iter()
            .filter(|entry| entry.model_type == "embedding")
            .count();

        {
            let mut balancer = self.load_balancer.lock().unwrap();
            *balancer = LoadBalancer::new(weighted);
        }

        info!(
            chat = chat_count,
            vision = vision_count,
            embedding = embedding_count,
            "LLM Provider 装载完成"
        );
        Ok(())
    }
}

impl LLMRouter {
    /// 依次尝试候选 Provider，任一成功即返回（任一失败继续下一家）
    async fn chat_candidates(
        &self,
        request: &LLMChatRequest,
        candidates: Vec<i64>,
        model_type: &str,
    ) -> Result<LLMChatResponse, LlmError> {
        if candidates.is_empty() {
            return Err(LlmError::config(format!(
                "没有可用的 {} 模型 Provider",
                model_type
            )));
        }

        let mut last_error: Option<LlmError> = None;

        for provider_id in candidates {
            let Some(entry) = self.entry(provider_id, model_type) else {
                continue;
            };

            let chat_request = to_llm_request(request, &entry.model);
            info!(
                provider_id,
                provider = %entry.provider_code,
                model = %entry.model,
                "调用 LLM Provider"
            );

            match entry.provider.chat(chat_request).await {
                Ok(response) => {
                    self.record(provider_id, &Ok(()));
                    return Ok(to_db_response(provider_id, 0, response));
                }
                Err(error) => {
                    error!(
                        provider_id,
                        kind = error.kind(),
                        "Provider 调用失败: {}",
                        error
                    );
                    self.record(provider_id, &Err(error.clone()));
                    last_error = Some(error);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            LlmError::config(format!("没有可用的 {} 模型 Provider", model_type))
        }))
    }

    /// 调用 Chat（多 Provider 自动故障转移）
    pub async fn chat(&self, request: LLMChatRequest) -> Result<LLMChatResponse, String> {
        if !self.circuit_allows() {
            return Err("LLM 服务暂时不可用（熔断器开启）".to_string());
        }
        let candidates = self.provider_ids("chat");
        self.chat_candidates(&request, candidates, "chat")
            .await
            .map_err(|error| error_string(&error))
    }

    /// 使用指定 Provider 调用 Chat（`None` 时自动故障转移）
    pub async fn chat_with_provider_id(
        &self,
        request: LLMChatRequest,
        provider_id: Option<i64>,
    ) -> Result<LLMChatResponse, String> {
        let Some(provider_id) = provider_id else {
            return self.chat(request).await;
        };

        if !self.circuit_allows() {
            return Err("LLM 服务暂时不可用（熔断器开启）".to_string());
        }
        self.chat_candidates(&request, vec![provider_id], "chat")
            .await
            .map_err(|error| error_string(&error))
    }

    /// 使用 vision（视觉 / 多模态）模型调用 Chat
    ///
    /// - `provider_id` 指定时：必须该厂商注册过 vision 模型；
    /// - 未指定时：在所有 vision 条目间故障转移；
    /// - 一个都没有时：给出可操作的错误提示（引导去配置页添加视觉模型）。
    pub async fn chat_with_vision(
        &self,
        request: LLMChatRequest,
        provider_id: Option<i64>,
    ) -> Result<LLMChatResponse, String> {
        if !self.circuit_allows() {
            return Err("LLM 服务暂时不可用（熔断器开启）".to_string());
        }

        let vision_ids = self.provider_ids("vision");
        if vision_ids.is_empty() {
            return Err(
                "未配置视觉模型，无法处理图片/视频。请在 LLM 厂商配置中添加 vision 类型模型（如 qwen-vl / gpt-4o / glm-4v / llava）"
                    .to_string(),
            );
        }

        let candidates = match provider_id {
            Some(provider_id) if vision_ids.contains(&provider_id) => vec![provider_id],
            Some(provider_id) => {
                return Err(format!(
                    "所选厂商 {} 未配置视觉模型，请在 LLM 厂商配置中添加 vision 类型模型",
                    provider_id
                ))
            }
            None => vision_ids,
        };

        self.chat_candidates(&request, candidates, "vision")
            .await
            .map_err(|error| format!("视觉模型调用失败：{}", error_string(&error)))
    }
}

impl LLMRouter {
    /// 调用 Embedding（多 Provider 故障转移；Anthropic 不支持会自动跳过）
    pub async fn embedding(
        &self,
        request: LLMEmbeddingRequest,
    ) -> Result<LLMEmbeddingResponse, String> {
        if !self.circuit_allows() {
            return Err("LLM 服务暂时不可用（熔断器开启）".to_string());
        }

        let candidates = self.provider_ids("embedding");
        if candidates.is_empty() {
            return Err("没有可用的 embedding Provider".to_string());
        }

        let mut last_error: Option<LlmError> = None;

        for provider_id in candidates {
            let Some(entry) = self.entry(provider_id, "embedding") else {
                continue;
            };
            let embedding_request = to_llm_embedding(&request, &entry.model);

            match entry.provider.embed(embedding_request).await {
                Ok(response) => {
                    self.record(provider_id, &Ok(()));
                    return Ok(to_db_embedding(provider_id, 0, response));
                }
                Err(error) => {
                    error!(
                        provider_id,
                        kind = error.kind(),
                        "Embedding 调用失败: {}",
                        error
                    );
                    self.record(provider_id, &Err(error.clone()));
                    last_error = Some(error);
                }
            }
        }

        Err(last_error.map_or_else(
            || "没有可用的 embedding Provider".to_string(),
            |error| error_string(&error),
        ))
    }

    /// 流式对话
    ///
    /// 只对「建立流之前」的失败做换 Provider；流建立成功后的中途错误
    /// 以 `Err(LlmError)` 事件形式出现在返回的流里，由调用方感知并降级。
    ///
    /// - `model_type`：`chat` 或 `vision`（后者用于图片/视频问答）
    pub async fn chat_stream(
        &self,
        request: &LLMChatRequest,
        provider_id: Option<i64>,
        model_type: &str,
    ) -> Result<ChatStream, String> {
        if !self.circuit_allows() {
            return Err("LLM 服务暂时不可用（熔断器开启）".to_string());
        }

        let candidates = match provider_id {
            Some(provider_id) => vec![provider_id],
            None => self.provider_ids(model_type),
        };
        if candidates.is_empty() {
            return Err(format!("没有可用的 {} 模型 Provider", model_type));
        }

        let mut last_error = String::new();

        for provider_id in candidates {
            let Some(entry) = self.entry(provider_id, model_type) else {
                continue;
            };
            let chat_request = to_llm_request(request, &entry.model).with_stream(true);

            match entry.provider.chat_stream(chat_request).await {
                Ok(stream) => {
                    self.record(provider_id, &Ok(()));
                    return Ok(stream);
                }
                Err(error) => {
                    error!(
                        provider_id,
                        kind = error.kind(),
                        "建立流式对话失败: {}",
                        error
                    );
                    self.record(provider_id, &Err(error.clone()));
                    last_error = error_string(&error);
                }
            }
        }

        Err(last_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm::{ChatRequest, ChatResponse, LlmResult, Protocol, StopReason, Usage};

    /// 仅用于负载均衡 / 熔断测试的假 Provider
    struct DummyProvider {
        code: String,
    }

    #[async_trait::async_trait]
    impl LlmProviderTrait for DummyProvider {
        fn provider_code(&self) -> &str {
            &self.code
        }

        fn protocol(&self) -> Protocol {
            Protocol::OpenAiCompatible
        }

        fn default_model(&self) -> &str {
            "dummy-model"
        }

        async fn chat(&self, _request: ChatRequest) -> LlmResult<ChatResponse> {
            Ok(ChatResponse {
                id: "dummy".to_string(),
                model: "dummy-model".to_string(),
                content: Vec::new(),
                stop_reason: StopReason::EndTurn,
                usage: Usage::default(),
            })
        }

        async fn chat_stream(&self, _request: ChatRequest) -> LlmResult<ChatStream> {
            Ok(Box::pin(futures::stream::empty()))
        }
    }

    fn entry(provider_id: i64, model_type: &str, weight: i32) -> WeightedProvider {
        WeightedProvider {
            provider_id,
            provider_code: "openai".to_string(),
            provider: Arc::new(DummyProvider {
                code: "openai".to_string(),
            }),
            weight,
            model_type: model_type.to_string(),
            model: "gpt-4o".to_string(),
        }
    }

    #[test]
    fn load_balancer_prefers_pinned_provider() {
        let mut balancer = LoadBalancer::new(vec![
            entry(1, "chat", 90),
            entry(2, "chat", 10),
            entry(3, "vision", 50),
        ]);

        let picked = balancer.select("chat", Some(2)).expect("应选中指定 Provider");
        assert_eq!(picked.provider_id, 2);

        // 指定了未注册该模型类型的 Provider → 明确报错
        assert!(balancer.select("chat", Some(3)).is_err());
    }

    #[test]
    fn load_balancer_skips_provider_after_three_failures() {
        let mut balancer = LoadBalancer::new(vec![entry(1, "chat", 10)]);

        assert!(balancer.select("chat", None).is_ok());
        for _ in 0..3 {
            balancer.record_failure(1);
        }
        assert!(balancer.select("chat", None).is_err());

        balancer.record_success(1);
        assert!(balancer.select("chat", None).is_ok());
    }

    #[test]
    fn load_balancer_filters_entries_by_model_type() {
        let balancer = LoadBalancer::new(vec![
            entry(1, "chat", 10),
            entry(1, "vision", 10),
            entry(2, "embedding", 10),
        ]);

        assert_eq!(balancer.entries("chat").len(), 1);
        assert_eq!(balancer.entries("vision").len(), 1);
        assert!(balancer.find(2, "embedding").is_some());
        assert!(balancer.find(2, "chat").is_none());
    }

    #[test]
    fn circuit_breaker_opens_after_threshold_and_resets_on_success() {
        let mut breaker = CircuitBreaker::new(2, 60);
        assert!(breaker.is_allowed());

        breaker.record_failure();
        assert!(breaker.is_allowed());

        breaker.record_failure();
        assert!(!breaker.is_allowed(), "达到阈值后应跳闸");

        breaker.record_success();
        assert!(breaker.is_allowed(), "成功后应恢复");
    }

    #[test]
    fn error_string_carries_kind_prefix() {
        assert!(error_string(&LlmError::auth("bad key")).starts_with("auth: "));
        assert!(
            error_string(&LlmError::rate_limited("slow", None)).starts_with("rate_limited: ")
        );
        assert!(error_string(&LlmError::Cancelled).starts_with("cancelled: "));
    }

    #[tokio::test]
    async fn only_retryable_errors_trip_the_circuit_breaker() {
        let router = LLMRouter::new();
        {
            let mut balancer = router.load_balancer.lock().unwrap();
            *balancer = LoadBalancer::new(vec![entry(1, "chat", 10)]);
        }

        // 鉴权错误重试无意义，不应拖垮全局
        for _ in 0..10 {
            router.record(1, &Err(LlmError::auth("bad key")));
        }
        assert!(router.circuit_allows());

        // 可重试错误累计到阈值才跳闸
        for _ in 0..5 {
            router.record(1, &Err(LlmError::transport("connection reset")));
        }
        assert!(!router.circuit_allows());
    }

    #[tokio::test]
    async fn chat_fails_over_to_next_provider_when_none_registered() {
        // 没有任何 Provider 时给出明确的配置错误，而不是 panic
        let router = LLMRouter::new();
        let request = LLMChatRequest {
            messages: Vec::new(),
            model: None,
            temperature: None,
            max_tokens: None,
            stream: Some(false),
            user_id: None,
            conv_id: None,
        };
        let error = router.chat(request).await.expect_err("应返回错误");
        assert!(error.starts_with("config: "), "{error}");
    }
}





