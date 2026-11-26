//! 内部 LLM 端点（OpenAI 兼容）——沙箱内 `agent-runner` 的唯一模型出口
//!
//! - 只挂 `auth_middleware`（API Key），要求 `PrincipalKind::Service` + `sandbox:llm` scope；
//! - **不挂** `authz` / `protected_routes`（服务身份没有 `UserContext`）；
//! - 内部映射到 `LLMRouter::chat`，成功与失败都写 `llm_call_record`；
//! - 非流式（`stream=true` 直接 400）：runner 按单步协议调用。

use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::{Extension, Json};
use database::models::{ChatMessage, LLMChatRequest, LLMChatResponse};
use serde::{Deserialize, Serialize};
use service::llm_call_record::{self, CallRecord};
use service::llm_gateway_service::LLMRouter;

use super::auth::{AuthPrincipal, PrincipalKind};
use super::response::{ApiError, ApiResponse};

/// 内部 LLM 端点状态（模型网关 + 计量连接池）
pub struct InternalLlmState {
    /// 平台 LLM Router（故障转移 / 计量 id 由它给出）
    pub llm: Arc<LLMRouter>,
    /// 写 `llm_call_record` 使用的连接池
    pub pool: sqlx::PgPool,
}

/// OpenAI 兼容的 chat 请求（仅子集）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct OpenAiChatRequest {
    /// 模型名（空 → Router 默认）
    #[serde(default)]
    pub model: Option<String>,
    /// 消息列表
    pub messages: Vec<OpenAiMessage>,
    /// 温度
    #[serde(default)]
    pub temperature: Option<f32>,
    /// 最大输出 token
    #[serde(default)]
    pub max_tokens: Option<i32>,
    /// 是否流式（本端点只支持 false）
    #[serde(default)]
    pub stream: Option<bool>,
}

/// OpenAI 兼容的消息
#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct OpenAiMessage {
    /// system / user / assistant / tool
    pub role: String,
    /// 文本内容
    #[serde(default)]
    pub content: String,
}

/// OpenAI 兼容的 chat 响应
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct OpenAiChatResponse {
    /// 响应 ID
    pub id: String,
    /// 固定 `chat.completion`
    pub object: String,
    /// Unix 秒
    pub created: i64,
    /// 实际模型
    pub model: String,
    /// 候选列表
    pub choices: Vec<OpenAiChoice>,
    /// 用量
    pub usage: OpenAiUsage,
}

/// 候选
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct OpenAiChoice {
    /// 序号
    pub index: u32,
    /// 消息
    pub message: OpenAiMessageOut,
    /// 结束原因
    pub finish_reason: String,
}

/// 输出消息
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct OpenAiMessageOut {
    /// 固定 `assistant`
    pub role: String,
    /// 文本内容
    pub content: String,
}

/// 用量
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct OpenAiUsage {
    /// 输入 token
    pub prompt_tokens: i32,
    /// 输出 token
    pub completion_tokens: i32,
    /// 合计
    pub total_tokens: i32,
}

/// OpenAI 请求 → 平台内部请求
pub(crate) fn to_llm_request(body: OpenAiChatRequest) -> Result<LLMChatRequest, String> {
    if body.messages.is_empty() {
        return Err("messages 不能为空".to_string());
    }
    if body.stream == Some(true) {
        return Err("本端点暂不支持 stream=true（runner 使用非流式单步）".to_string());
    }
    Ok(LLMChatRequest {
        messages: body
            .messages
            .into_iter()
            .map(|message| ChatMessage {
                role: message.role,
                content: message.content,
                content_parts: None,
            })
            .collect(),
        model: body.model,
        temperature: body.temperature,
        max_tokens: body.max_tokens,
        stream: Some(false),
        user_id: None,
        conv_id: None,
    })
}

/// 平台响应 → OpenAI 兼容响应
pub(crate) fn to_wire(response: &LLMChatResponse) -> OpenAiChatResponse {
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(0))
        .unwrap_or(0);

    OpenAiChatResponse {
        id: response.request_id.clone(),
        object: "chat.completion".to_string(),
        created,
        model: response.model.clone(),
        choices: vec![OpenAiChoice {
            index: 0,
            message: OpenAiMessageOut {
                role: "assistant".to_string(),
                content: response.content.clone(),
            },
            finish_reason: "stop".to_string(),
        }],
        usage: OpenAiUsage {
            prompt_tokens: response.usage.input_tokens,
            completion_tokens: response.usage.output_tokens,
            total_tokens: response.usage.total_tokens,
        },
    }
}

/// 仅服务身份 + `sandbox:llm` scope
fn ensure_service_llm(principal: &AuthPrincipal) -> Result<(), ApiError> {
    if principal.kind != PrincipalKind::Service {
        return Err(ApiError::forbidden(
            "仅服务身份可调用内部 LLM 端点（沙箱 token）",
        ));
    }
    let required = vec!["sandbox:llm".to_string()];
    if !principal.allows(&required) {
        return Err(ApiError::forbidden_with_scope(&required));
    }
    Ok(())
}

/// OpenAI 兼容的 chat completions（非流式）
#[utoipa::path(
    post,
    path = "/internal/llm/v1/chat/completions",
    tag = "沙箱",
    request_body = OpenAiChatRequest,
    responses(
        (status = 200, description = "OpenAI 兼容响应", body = ApiResponse<OpenAiChatResponse>),
        (status = 400, description = "请求非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 403, description = "非服务身份或缺少 sandbox:llm scope", body = ApiError),
    )
)]
pub async fn chat_completions(
    State(state): State<Arc<InternalLlmState>>,
    Extension(principal): Extension<AuthPrincipal>,
    Json(body): Json<OpenAiChatRequest>,
) -> Result<Json<ApiResponse<OpenAiChatResponse>>, ApiError> {
    ensure_service_llm(&principal)?;
    let request = to_llm_request(body).map_err(ApiError::bad_request)?;
    let started = Instant::now();

    match state.llm.chat(request).await {
        Ok(response) => {
            let record = CallRecord {
                user_id: None,
                conv_id: None,
                provider_id: response.provider_id,
                model_id: response.model_id,
                call_type: "sandbox_chat",
                input_tokens: response.usage.input_tokens,
                output_tokens: response.usage.output_tokens,
                duration_ms: millis(started),
                status: "success",
                error_msg: None,
                request_id: Some(response.request_id.as_str()),
            };
            if let Err(error) = llm_call_record::record(&state.pool, &record).await {
                tracing::warn!(%error, "llm_call_record 写入失败（不阻塞调用方）");
            }
            Ok(Json(ApiResponse::success(to_wire(&response))))
        }
        Err(error) => {
            let record = CallRecord {
                user_id: None,
                conv_id: None,
                provider_id: 0,
                model_id: 0,
                call_type: "sandbox_chat",
                input_tokens: 0,
                output_tokens: 0,
                duration_ms: millis(started),
                status: "error",
                error_msg: Some(error.as_str()),
                request_id: None,
            };
            if let Err(write_error) = llm_call_record::record(&state.pool, &record).await {
                tracing::warn!(%write_error, "llm_call_record 写入失败（不阻塞调用方）");
            }
            Err(ApiError::internal_error(format!("LLM 调用失败：{error}")))
        }
    }
}

fn millis(started: Instant) -> i32 {
    i32::try_from(started.elapsed().as_millis()).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::models::TokenUsage;

    fn principal(kind: PrincipalKind, scopes: &[&str]) -> AuthPrincipal {
        AuthPrincipal {
            kind,
            subject: "k:1".to_string(),
            user_id: None,
            api_key_id: Some(1),
            tenant_id: Some(1),
            schema_name: "tenant_a".to_string(),
            scopes: scopes.iter().map(|scope| scope.to_string()).collect(),
            is_super_admin: false,
            rate_limit_per_min: Some(600),
        }
    }

    fn body() -> OpenAiChatRequest {
        OpenAiChatRequest {
            model: Some("gpt-4o-mini".to_string()),
            messages: vec![
                OpenAiMessage {
                    role: "system".to_string(),
                    content: "sys".to_string(),
                },
                OpenAiMessage {
                    role: "user".to_string(),
                    content: "hi".to_string(),
                },
            ],
            temperature: Some(0.2),
            max_tokens: Some(256),
            stream: Some(false),
        }
    }

    #[test]
    fn maps_request_fields() {
        let request = to_llm_request(body()).expect("应可转换");
        assert_eq!(request.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[0].role, "system");
        assert_eq!(request.messages[1].content, "hi");
        assert_eq!(request.temperature, Some(0.2));
        assert_eq!(request.max_tokens, Some(256));
        assert_eq!(request.stream, Some(false));
    }

    #[test]
    fn rejects_empty_messages_and_streaming() {
        let mut empty = body();
        empty.messages.clear();
        assert!(to_llm_request(empty).unwrap_err().contains("messages"));

        let mut streaming = body();
        streaming.stream = Some(true);
        assert!(to_llm_request(streaming).unwrap_err().contains("stream"));
    }

    #[test]
    fn maps_response_to_openai_shape() {
        let response = LLMChatResponse {
            content: "hello".to_string(),
            model: "gpt-4o-mini".to_string(),
            usage: TokenUsage {
                input_tokens: 3,
                output_tokens: 5,
                total_tokens: 8,
                cost: 0.0,
            },
            provider_id: 11,
            model_id: 22,
            request_id: "req-1".to_string(),
        };
        let wire = to_wire(&response);
        assert_eq!(wire.id, "req-1");
        assert_eq!(wire.object, "chat.completion");
        assert_eq!(wire.model, "gpt-4o-mini");
        assert_eq!(wire.choices[0].message.content, "hello");
        assert_eq!(wire.choices[0].finish_reason, "stop");
        assert_eq!(wire.usage.prompt_tokens, 3);
        assert_eq!(wire.usage.completion_tokens, 5);
        assert_eq!(wire.usage.total_tokens, 8);
    }

    #[test]
    fn rejects_user_identity() {
        let error = ensure_service_llm(&principal(PrincipalKind::User, &["sandbox:llm"]))
            .expect_err("用户身份必须拒绝");
        assert_eq!(error.code, 403);
    }

    #[test]
    fn rejects_missing_scope() {
        let error = ensure_service_llm(&principal(PrincipalKind::Service, &["sandbox:audit"]))
            .expect_err("缺少 scope 必须拒绝");
        assert_eq!(error.code, 403);
        assert!(
            error
                .scope
                .as_deref()
                .is_some_and(|scopes| scopes == ["sandbox:llm".to_string()])
        );
    }

    #[test]
    fn accepts_service_with_scope() {
        assert!(ensure_service_llm(&principal(PrincipalKind::Service, &["sandbox:llm"])).is_ok());
    }
}
