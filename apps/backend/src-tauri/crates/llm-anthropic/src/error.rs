//! Anthropic 错误体 → [`llm::LlmError`]
//!
//! 错误体形态：`{"type":"error","error":{"type":"...","message":"..."}}`。
//! `error.type` 比 HTTP 状态码更精确（例如 `overloaded_error` 对应 529）。

use std::time::Duration;

use llm::LlmError;

/// 从错误体中提取 `(error.type, error.message)`
fn extract(body: &str) -> (Option<String>, String) {
    let trimmed = body.trim();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        let kind = value["error"]["type"].as_str().map(str::to_string);
        if let Some(message) = value["error"]["message"].as_str() {
            return (kind, message.to_string());
        }
        if let Some(message) = value["message"].as_str() {
            return (kind, message.to_string());
        }
    }
    if trimmed.is_empty() {
        return (None, "（响应体为空）".to_string());
    }
    let limit = 300.min(trimmed.chars().count());
    let mut snippet: String = trimmed.chars().take(limit).collect();
    if trimmed.chars().count() > limit {
        snippet.push_str("...");
    }
    (None, snippet)
}

/// 把 HTTP 状态码 + 响应体映射为分类错误
pub fn map_error(status: u16, body: &str, retry_after: Option<Duration>) -> LlmError {
    let (kind, message) = extract(body);
    let lower = message.to_ascii_lowercase();

    match kind.as_deref() {
        Some("authentication_error" | "permission_error") => LlmError::auth(message),
        Some("rate_limit_error") => LlmError::rate_limited(message, retry_after),
        Some("overloaded_error") => LlmError::server(529, message),
        Some("api_error") => LlmError::server(status.max(500), message),
        Some("timeout_error") => LlmError::transport(message),
        Some("not_found_error") => {
            LlmError::invalid_request(status, format!("接口或模型不存在：{message}"))
        }
        Some("invalid_request_error") | Some("request_too_large") => {
            if lower.contains("prompt is too long")
                || lower.contains("too many tokens")
                || lower.contains("context")
            {
                LlmError::context_length(message, None)
            } else {
                LlmError::invalid_request(status, message)
            }
        }
        _ => match status {
            401 | 403 => LlmError::auth(message),
            429 => LlmError::rate_limited(message, retry_after),
            404 => LlmError::invalid_request(status, format!("接口不存在：{message}")),
            400 | 413 | 422 => LlmError::invalid_request(status, message),
            _ if status >= 500 => LlmError::server(status, message),
            _ => LlmError::invalid_request(status, message),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_error_types() {
        let auth = map_error(
            401,
            r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#,
            None,
        );
        assert!(auth.is_auth_error());

        let overloaded = map_error(
            529,
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            None,
        );
        assert!(matches!(overloaded, LlmError::Server { status: 529, .. }));
        assert!(overloaded.is_retryable());

        let limited = map_error(
            429,
            r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow"}}"#,
            Some(Duration::from_secs(10)),
        );
        assert_eq!(limited.retry_after(), Some(Duration::from_secs(10)));
    }

    #[test]
    fn detects_context_length() {
        let error = map_error(
            400,
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 210000 tokens"}}"#,
            None,
        );
        assert!(error.is_context_length());
        assert!(!error.is_retryable());
    }

    #[test]
    fn falls_back_to_status_code() {
        assert!(map_error(503, "gateway down", None).is_retryable());
        assert!(map_error(401, "", None).is_auth_error());
        assert!(matches!(
            map_error(400, r#"{"error":{"message":"bad"}}"#, None),
            LlmError::InvalidRequest { .. }
        ));
    }
}
