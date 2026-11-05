//! OpenAI 错误体 → [`llm::LlmError`]
//!
//! 错误体形态：`{"error":{"message":"...","type":"...","code":"..."}}`，
//! 部分兼容厂商只返回 `{"message":"..."}` 或纯文本，因此解析尽量宽容。

use std::time::Duration;

use llm::LlmError;

/// 把 HTTP 状态码 + 响应体映射为分类错误
///
/// - `retry_after`：`Retry-After` 头的秒数（429 / 503 常见）
pub fn map_error(status: u16, body: &str, retry_after: Option<Duration>) -> LlmError {
    let message = extract_message(body);

    match status {
        401 | 403 => LlmError::auth(message),
        429 => LlmError::rate_limited(message, retry_after),
        400 | 422 => {
            if is_context_length(&message) {
                LlmError::context_length(message, None)
            } else {
                LlmError::invalid_request(status, message)
            }
        }
        404 => LlmError::invalid_request(status, format!("接口不存在：{message}")),
        _ if status >= 500 => LlmError::server(status, message),
        _ => LlmError::invalid_request(status, message),
    }
}

/// 从响应体中尽量提取可读的错误信息
pub(crate) fn extract_message(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "（响应体为空）".to_string();
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        // {"error":{"message":...}}
        if let Some(message) = value["error"]["message"].as_str() {
            return message.to_string();
        }
        // 部分厂商直接返回 {"message":...}
        if let Some(message) = value["message"].as_str() {
            return message.to_string();
        }
        // {"error":"..."}
        if let Some(message) = value["error"].as_str() {
            return message.to_string();
        }
    }

    // 非 JSON：截断，避免把整页 HTML 写进日志
    let limit = 300.min(trimmed.len());
    let mut snippet: String = trimmed.chars().take(limit).collect();
    if trimmed.chars().count() > limit {
        snippet.push_str("...");
    }
    snippet
}

/// 判断是否为「上下文超长」类错误
fn is_context_length(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("context_length")
        || lower.contains("maximum context")
        || lower.contains("context window")
        || lower.contains("too many tokens")
        || lower.contains("reduce the length")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_status_codes_to_categories() {
        assert!(map_error(401, r#"{"error":{"message":"bad key"}}"#, None).is_auth_error());
        assert!(matches!(
            map_error(429, r#"{"error":{"message":"slow"}}"#, Some(Duration::from_secs(5))),
            LlmError::RateLimited {
                retry_after: Some(_),
                ..
            }
        ));
        assert!(map_error(500, "boom", None).is_retryable());
        assert!(matches!(
            map_error(400, r#"{"error":{"message":"bad model"}}"#, None),
            LlmError::InvalidRequest { status: 400, .. }
        ));
    }

    #[test]
    fn detects_context_length_errors() {
        let error = map_error(
            400,
            r#"{"error":{"message":"This model's maximum context length is 8192 tokens"}}"#,
            None,
        );
        assert!(error.is_context_length());
        assert!(!error.is_retryable());
    }

    #[test]
    fn extracts_message_from_various_shapes() {
        assert_eq!(
            extract_message(r#"{"error":{"message":"nested"}}"#),
            "nested"
        );
        assert_eq!(extract_message(r#"{"message":"flat"}"#), "flat");
        assert_eq!(extract_message(r#"{"error":"plain"}"#), "plain");
        assert_eq!(extract_message("   "), "（响应体为空）");
        assert!(extract_message("<html>nginx</html>").contains("nginx"));
    }
}
