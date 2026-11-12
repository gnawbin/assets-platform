//! CubeSandbox 错误映射
//!
//! 错误体形态：CubeAPI 返回 `{"code":..,"message":".."}`（`ApiError`），
//! 数据面 envd 可能返回 `{"error":{...}}` 或纯文本，因此解析尽量宽容。
//! 分类风格对齐 `llm-openai::error::map_error`。

use std::time::Duration;

use sandbox::SandboxError;

/// 把 HTTP 状态码 + 响应体映射为分类错误
///
/// - `retry_after`：`Retry-After` 头的秒数（429 / 503 常见）
#[must_use]
pub fn map_error(status: u16, body: &str, retry_after: Option<Duration>) -> SandboxError {
    let message = extract_message(body);

    match status {
        401 | 403 => SandboxError::auth(message),
        404 => SandboxError::not_found(message),
        409 => SandboxError::conflict(message),
        429 => SandboxError::rate_limited(message, retry_after),
        400 | 422 => SandboxError::invalid_request(status, message),
        s if s >= 500 => SandboxError::server(status, message),
        _ => SandboxError::invalid_request(status, message),
    }
}

/// 从响应体中尽量提取可读的错误信息
#[must_use]
pub fn extract_message(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "（响应体为空）".to_string();
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        // {"message":"..."}（CubeAPI 的 ApiError）
        if let Some(message) = value["message"].as_str() {
            return message.to_string();
        }
        // {"error":{"message":"..."}}
        if let Some(message) = value["error"]["message"].as_str() {
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

/// 解析 `Retry-After` 头（只支持秒数形式，HTTP-date 形式忽略）
#[must_use]
pub fn parse_retry_after(raw: Option<&str>) -> Option<Duration> {
    let value = raw?.trim();
    let secs = value.parse::<u64>().ok()?;
    Some(Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_status_codes_to_categories() {
        assert!(matches!(
            map_error(401, r#"{"message":"bad key"}"#, None),
            SandboxError::Auth(_)
        ));
        assert!(matches!(
            map_error(403, "", None),
            SandboxError::Auth(_)
        ));
        assert!(matches!(
            map_error(404, r#"{"message":"missing"}"#, None),
            SandboxError::NotFound(_)
        ));
        assert!(matches!(
            map_error(409, "", None),
            SandboxError::Conflict(_)
        ));
        assert!(matches!(
            map_error(400, "", None),
            SandboxError::InvalidRequest { status: 400, .. }
        ));
        assert!(matches!(
            map_error(422, "", None),
            SandboxError::InvalidRequest { status: 422, .. }
        ));
        assert!(matches!(
            map_error(503, "", None),
            SandboxError::Server { status: 503, .. }
        ));
    }

    #[test]
    fn maps_rate_limit_with_retry_after() {
        let error = map_error(429, r#"{"message":"slow down"}"#, Some(Duration::from_secs(5)));
        match error {
            SandboxError::RateLimited { message, retry_after } => {
                assert_eq!(message, "slow down");
                assert_eq!(retry_after, Some(Duration::from_secs(5)));
                assert_eq!(retry_after, Some(Duration::from_secs(5)));
            }
            other => panic!("应为 RateLimited，实际 {other:?}"),
        }
    }

    #[test]
    fn extracts_message_from_various_shapes() {
        assert_eq!(extract_message(r#"{"message":"flat"}"#), "flat");
        assert_eq!(
            extract_message(r#"{"error":{"message":"nested"}}"#),
            "nested"
        );
        assert_eq!(extract_message(r#"{"error":"plain"}"#), "plain");
        assert_eq!(extract_message("   "), "（响应体为空）");
        assert!(extract_message("<html>nginx</html>").contains("nginx"));
    }

    #[test]
    fn retry_after_parses_seconds_only() {
        assert_eq!(parse_retry_after(Some("7")), Some(Duration::from_secs(7)));
        assert_eq!(parse_retry_after(Some(" 3 ")), Some(Duration::from_secs(3)));
        assert_eq!(parse_retry_after(Some("Wed, 21 Oct 2015 07:28:00 GMT")), None);
        assert_eq!(parse_retry_after(None), None);
    }
}
