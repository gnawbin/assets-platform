//! 提示词拼装（与 `workflow/src/agent.rs` 原实现逐字一致）
//!
//! 拆成独立函数后，平台侧 Agent 与沙箱侧 runner 生成**完全相同**的提示词，
//! 避免同一份协议出现两种措辞。

use serde_json::Value;

/// 系统提示词
///
/// `tools` 必须是已按可用性过滤后的工具名；`sandbox_ok` 决定是否追加沙箱工具说明。
#[must_use]
pub fn build_system(tools: &[String], sandbox_ok: bool) -> String {
    let tool_hint = if sandbox_ok {
        "\n沙箱工具：sandbox_exec 支持 command（或 command + argv 数组）+ cwd + timeout_secs；file_read 支持 path；file_write 支持 path + content 或 content_base64。"
    } else {
        ""
    };
    format!(
        "你是资产平台的工作流 Agent。请严格只输出一个 JSON 对象，不要输出 Markdown。\n\
         可用工具：{}。\n\
         需要调用工具时输出：{{\"action\":\"tool\",\"tool\":\"工具名\",\"arguments\":{{}},\"thought\":\"简短理由\"}}\n\
         可以给出最终答案时输出：{{\"action\":\"final\",\"answer\":\"最终答案\",\"thought\":\"简短理由\"}}\n\
         工具参数：knowledge_search 支持 question、tree_node_id、top_k；current_time 无参数。{tool_hint}",
        tools.join(", ")
    )
}

/// 用户消息（任务说明 + 业务输入 + 已有工具观察）
#[must_use]
pub fn build_user(prompt: &str, input: &Value, observations: &[Value]) -> String {
    format!(
        "任务说明：\n{}\n\n业务输入：\n{}\n\n已有工具观察：\n{}",
        if prompt.trim().is_empty() {
            "根据业务输入完成用户任务。"
        } else {
            prompt
        },
        serde_json::to_string_pretty(input).unwrap_or_else(|_| "null".to_string()),
        serde_json::to_string_pretty(&observations).unwrap_or_else(|_| "[]".to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn system_prompt_lists_tools_and_contract() {
        let tools = vec!["knowledge_search".to_string(), "current_time".to_string()];
        let prompt = build_system(&tools, false);
        assert!(prompt.contains("knowledge_search, current_time"));
        assert!(prompt.contains("严格只输出一个 JSON 对象"));
        assert!(prompt.contains("\"action\":\"tool\""));
        assert!(!prompt.contains("sandbox_exec"), "{prompt}");
    }

    #[test]
    fn system_prompt_adds_sandbox_hint_only_when_available() {
        let tools = vec!["sandbox_exec".to_string()];
        let prompt = build_system(&tools, true);
        assert!(prompt.contains("sandbox_exec 支持 command"), "{prompt}");
        assert!(prompt.contains("file_write 支持 path"), "{prompt}");
    }

    #[test]
    fn user_prompt_contains_task_input_and_observations() {
        let input = json!({"asset_id": "1"});
        let observations = vec![json!({"tool": "knowledge_search", "result": {}})];
        let prompt = build_user("整理资产", &input, &observations);

        assert!(prompt.contains("任务说明：\n整理资产"), "{prompt}");
        assert!(prompt.contains("\"asset_id\": \"1\""), "{prompt}");
        assert!(prompt.contains("knowledge_search"), "{prompt}");
    }

    #[test]
    fn empty_prompt_falls_back_to_default_instruction() {
        let prompt = build_user("   ", &Value::Null, &[]);
        assert!(prompt.contains("根据业务输入完成用户任务。"), "{prompt}");
        assert!(prompt.contains("null"), "{prompt}");
    }
}