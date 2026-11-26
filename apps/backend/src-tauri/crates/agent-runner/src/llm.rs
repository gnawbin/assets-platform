//! 平台 LLM 网关客户端（L1：沙箱出网访问平台内部端点）
//!
//! 复用仓库的 `llm` 契约 + `llm-openai` 实现：runner 只负责把
//! `StepInput.llm.base_url` 指向平台内部端点，token 从环境变量读取。

use std::time::Duration;

use agent_core::protocol::{StepInput, StepUsage};
use llm::{ChatRequest, LlmClientConfig, LlmProvider, Message};
use llm_openai::{OpenAiOptions, OpenAiProvider};

/// 单次对话，返回（文本, token 用量）
///
/// # Errors
/// 模型为空、客户端构造失败、网关返回错误时返回可展示错误。
pub async fn chat(
    step: &StepInput,
    system: String,
    user: String,
) -> Result<(String, StepUsage), String> {
    let model = step
        .agent
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(
            || step.llm.default_model.trim().to_string(),
            ToString::to_string,
        );
    if model.is_empty() {
        return Err("模型为空：agent.model / llm.default_model 均未设置".to_string());
    }

    let token = std::env::var(step.llm.token_env.trim()).unwrap_or_default();
    let mut config = LlmClientConfig::new(step.llm.base_url.clone(), token, model.clone());
    config.timeout = Duration::from_secs(step.llm.timeout_secs.max(1));

    let provider = OpenAiProvider::new(OpenAiOptions::new("platform", config))
        .map_err(|error| format!("LLM 客户端构造失败：{error}"))?;

    let mut request = ChatRequest::new(model, vec![Message::system(system), Message::user(user)]);
    request.temperature = step.agent.temperature;
    request.max_tokens = step.agent.max_tokens;
    request.stream = false;

    let response = provider
        .chat(request)
        .await
        .map_err(|error| format!("LLM 调用失败：{error}"))?;

    let usage = StepUsage {
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
    };
    Ok((response.text(), usage))
}