//! LLM 动作 JSON 解析与校验（从 `workflow/src/agent.rs` 逐字迁移）
//!
//! 协议：LLM 必须输出一个 JSON 对象
//! - 调工具：`{"action":"tool","tool":"...","arguments":{},"thought":"..."}`
//! - 最终答案：`{"action":"final","answer":"...","thought":"..."}`

use serde_json::Value;

/// 解析 LLM 返回的 JSON；兼容 ```` ```json ```` 代码块与前后说明文字
///
/// # Errors
/// 找不到合法 JSON 对象或动作字段不合法时返回可展示的错误。
pub fn parse_llm_json(content: &str) -> Result<Value, String> {
    let trimmed = content.trim();
    let without_fence = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map_or(trimmed, str::trim);

    if let Ok(value) = serde_json::from_str::<Value>(without_fence) {
        return validate_action(value);
    }

    let start = without_fence.find('{');
    let end = without_fence.rfind('}');
    if let (Some(start), Some(end)) = (start, end)
        && end > start
        && let Ok(value) = serde_json::from_str::<Value>(&without_fence[start..=end])
    {
        return validate_action(value);
    }

    Err(format!(
        "LLM 未返回合法 JSON：{}",
        without_fence.chars().take(300).collect::<String>()
    ))
}

/// 校验动作结构（`final` 必须有 `answer`，`tool` 必须有 `tool`）
///
/// # Errors
/// 非对象 / 未知 `action` / 缺少必填字段时返回可展示的错误。
pub fn validate_action(value: Value) -> Result<Value, String> {
    if !value.is_object() {
        return Err("LLM JSON 必须是对象".to_string());
    }
    let action = value.get("action").and_then(Value::as_str).unwrap_or("");
    if action == "final" || value.get("answer").is_some() {
        if value.get("answer").and_then(Value::as_str).is_none() {
            return Err("final 动作缺少 answer".to_string());
        }
        return Ok(value);
    }
    if action == "tool" {
        if value.get("tool").and_then(Value::as_str).is_none() {
            return Err("tool 动作缺少 tool".to_string());
        }
        return Ok(value);
    }
    Err(format!("未知 action: {action}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_plain_final_action() {
        let value = parse_llm_json(r#"{"action":"final","answer":"ok"}"#).expect("应解析成功");
        assert_eq!(value.get("answer").and_then(Value::as_str), Some("ok"));
    }

    #[test]
    fn parses_tool_action_inside_json_fence() {
        let raw = "```json\n{\"action\":\"tool\",\"tool\":\"knowledge_search\",\"arguments\":{\"question\":\"x\"}}\n```";
        let value = parse_llm_json(raw).expect("应解析成功");
        assert_eq!(value.get("tool").and_then(Value::as_str), Some("knowledge_search"));
    }

    #[test]
    fn parses_action_with_surrounding_prose() {
        let raw = "好的，我来查一下：\n{\"action\":\"tool\",\"tool\":\"current_time\",\"arguments\":{}}\n以上。";
        let value = parse_llm_json(raw).expect("应解析成功");
        assert_eq!(value.get("tool").and_then(Value::as_str), Some("current_time"));
    }

    #[test]
    fn rejects_non_object_and_unknown_action() {
        assert!(parse_llm_json("[1,2]").is_err());
        let error = parse_llm_json(r#"{"action":"dance"}"#).expect_err("未知动作必须报错");
        assert!(error.contains("未知 action"), "{error}");
    }

    #[test]
    fn rejects_missing_required_fields() {
        let error = parse_llm_json(r#"{"action":"final"}"#).expect_err("缺少 answer 必须报错");
        assert!(error.contains("answer"), "{error}");

        let error = parse_llm_json(r#"{"action":"tool"}"#).expect_err("缺少 tool 必须报错");
        assert!(error.contains("tool"), "{error}");
    }

    #[test]
    fn answer_without_action_is_treated_as_final() {
        let value = parse_llm_json(r#"{"answer":"done"}"#).expect("应解析成功");
        assert_eq!(value.get("answer").and_then(Value::as_str), Some("done"));
    }

    #[test]
    fn validate_action_keeps_extra_fields() {
        let value = validate_action(json!({
            "action": "final",
            "answer": "ok",
            "thought": "because"
        }))
        .expect("应校验通过");
        assert_eq!(value.get("thought").and_then(Value::as_str), Some("because"));
    }

    #[test]
    fn error_message_is_truncated_to_300_chars() {
        let raw = "x".repeat(1000);
        let error = parse_llm_json(&raw).expect_err("必须报错");
        assert!(error.chars().count() < 400, "len={}", error.chars().count());
    }
}