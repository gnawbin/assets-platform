//! 单步执行：一次 LLM 调用 + 最多一次本地工具执行
//!
//! 语义与平台侧 `AgentGraphRunner::agent_node` 对齐：
//! - `before_tool` / `before_final` 返回中断，由平台发起审批；
//! - 平台工具（`knowledge_search` / `current_time`）只回传意图，由平台执行；
//! - 本地工具（`sandbox_exec` / `file_read` / `file_write`）在沙箱内执行并回传 observation。

use agent_core::protocol::{StepInput, StepOutput};
use agent_core::{action, prompt};
use serde_json::{Value, json};

use crate::{llm, tools};

/// 失败类型（决定进程退出码）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepError {
    /// 进程退出码：2=输入非法 / 3=LLM 失败 / 4=本地工具失败
    pub exit_code: i32,
    /// 可展示错误
    pub message: String,
}

impl StepError {
    /// 输入非法（退出码 2）
    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            exit_code: 2,
            message: message.into(),
        }
    }

    /// LLM 调用/解析失败（退出码 3）
    #[must_use]
    pub fn llm(message: impl Into<String>) -> Self {
        Self {
            exit_code: 3,
            message: message.into(),
        }
    }

    /// 本地工具失败（退出码 4）
    #[must_use]
    pub fn tool(message: impl Into<String>) -> Self {
        Self {
            exit_code: 4,
            message: message.into(),
        }
    }
}

/// 执行一步
///
/// # Errors
/// 输入非法、LLM 失败或本地工具失败时返回 [`StepError`]。
pub async fn run_step(step: StepInput) -> Result<StepOutput, StepError> {
    step.validate().map_err(StepError::invalid)?;

    if let Some(approval) = step.approval.clone() {
        return resume_with_approval(&step, &approval).await;
    }

    let tool_names = step.agent.tool_names();
    let sandbox_ok = tool_names.iter().any(|tool| tools::is_local_tool(tool));
    let system = prompt::build_system(&tool_names, sandbox_ok);
    let user = prompt::build_user(&step.agent.prompt, &step.input, &step.observations);

    let (content, usage) = llm::chat(&step, system, user).await.map_err(StepError::llm)?;
    let action = action::parse_llm_json(&content).map_err(StepError::llm)?;

    let mut output = dispatch(&step, action).await?;
    output.usage = Some(usage);
    Ok(output)
}

async fn dispatch(step: &StepInput, action: Value) -> Result<StepOutput, StepError> {
    let policy = step.agent.interrupt_policy();

    if let Some(answer) = action.get("answer").and_then(Value::as_str) {
        if policy == "before_final" {
            return Ok(StepOutput::interrupt(json!({
                "kind": "before_final",
                "answer": answer,
                "thought": action.get("thought").cloned().unwrap_or(Value::Null),
            })));
        }
        return Ok(StepOutput::final_answer(json!({"answer": answer})));
    }

    let tool = action
        .get("tool")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if tool.is_empty() {
        return Ok(StepOutput::error("LLM 返回既没有 answer 也没有 tool"));
    }
    let arguments = action
        .get("arguments")
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));

    if policy == "before_tool" {
        return Ok(StepOutput::interrupt(json!({
            "kind": "before_tool",
            "tool": tool,
            "arguments": arguments,
            "thought": action.get("thought").cloned().unwrap_or(Value::Null),
        })));
    }

    let observation = execute_tool(step, &tool, &arguments).await?;
    Ok(StepOutput::tool(
        json!({"tool": tool, "arguments": arguments}),
        observation,
    ))
}

async fn execute_tool(
    step: &StepInput,
    tool: &str,
    arguments: &Value,
) -> Result<Option<Value>, StepError> {
    if !tools::is_local_tool(tool) {
        // 平台工具：只回传意图，由平台执行后进入下一步
        return Ok(None);
    }
    tools::execute(step, tool, arguments)
        .await
        .map(Some)
        .map_err(StepError::tool)
}

async fn resume_with_approval(step: &StepInput, approval: &Value) -> Result<StepOutput, StepError> {
    let approved = approval_field(approval, "approved")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !approved {
        return Ok(StepOutput::final_answer(
            json!({"answer": "用户未批准本次操作。"}),
        ));
    }

    if let Some(tool) = approval_field(approval, "tool").and_then(Value::as_str) {
        let tool = tool.to_string();
        let arguments = approval_field(approval, "arguments")
            .cloned()
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        let observation = execute_tool(step, &tool, &arguments).await?;
        return Ok(StepOutput::tool(
            json!({"tool": tool, "arguments": arguments}),
            observation,
        ));
    }

    if let Some(answer) = approval_field(approval, "answer") {
        return Ok(StepOutput::final_answer(json!({"answer": answer})));
    }

    Err(StepError::invalid("approval 缺少 tool / answer"))
}

/// 与平台 `approval_field` 一致：兼容顶层与 `data` 嵌套
fn approval_field<'a>(approval: &'a Value, key: &str) -> Option<&'a Value> {
    approval
        .get(key)
        .or_else(|| approval.get("data").and_then(|data| data.get(key)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::protocol::{PROTOCOL, StepKind};
    use std::path::{Path, PathBuf};

    fn temp_workspace() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agent-runner-run-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("创建临时目录");
        dir
    }

    fn step_input(
        base_url: &str,
        policy: &str,
        tool_names: &[&str],
        workspace: &Path,
    ) -> StepInput {
        serde_json::from_value(json!({
            "protocol": PROTOCOL,
            "thread_id": "t:1",
            "node_id": "n1",
            "agent": {
                "prompt": "x",
                "model": "m",
                "tools": tool_names,
                "interrupt_policy": policy
            },
            "llm": {"base_url": base_url, "default_model": "m"},
            "workspace": workspace.to_string_lossy()
        }))
        .expect("step 应可解析")
    }

    /// 假 LLM：`POST {base}/chat/completions` 返回固定 content
    async fn spawn_llm(content: &str) -> String {
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            axum::routing::post({
                let content = content.to_string();
                move |axum::Json(body): axum::Json<serde_json::Value>| {
                    let content = content.clone();
                    async move {
                        axum::Json(json!({
                            "id": "resp-1",
                            "object": "chat.completion",
                            "created": 0,
                            "model": body.get("model").cloned().unwrap_or_else(|| json!("m")),
                            "choices": [{
                                "index": 0,
                                "message": {"role": "assistant", "content": content},
                                "finish_reason": "stop"
                            }],
                            "usage": {"prompt_tokens": 3, "completion_tokens": 5, "total_tokens": 8}
                        }))
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("绑定临时端口");
        let addr = listener.local_addr().expect("本地地址");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("假 LLM 服务");
        });
        format!("http://{addr}/v1")
    }

    #[tokio::test]
    async fn final_answer_via_fake_llm() {
        let base_url = spawn_llm(r#"{"action":"final","answer":"ok"}"#).await;
        let step = step_input(&base_url, "never", &["knowledge_search"], &temp_workspace());

        let output = run_step(step).await.expect("应成功");
        assert_eq!(output.kind, StepKind::Final);
        assert_eq!(output.final_output.expect("final")["answer"], "ok");
        assert_eq!(output.usage.expect("usage").output_tokens, 5);
    }

    #[tokio::test]
    async fn platform_tool_returns_intent_without_observation() {
        let base_url = spawn_llm(
            r#"{"action":"tool","tool":"knowledge_search","arguments":{"question":"x"}}"#,
        )
        .await;
        let step = step_input(
            &base_url,
            "never",
            &["knowledge_search"],
            &temp_workspace(),
        );

        let output = run_step(step).await.expect("应成功");
        assert_eq!(output.kind, StepKind::Tool);
        assert!(output.observation.is_none(), "平台工具不本地执行");
        assert_eq!(output.action.expect("action")["tool"], "knowledge_search");
    }

    #[tokio::test]
    async fn before_tool_policy_returns_interrupt() {
        let base_url = spawn_llm(
            r#"{"action":"tool","tool":"sandbox_exec","arguments":{"command":"ls"}}"#,
        )
        .await;
        let step = step_input(&base_url, "before_tool", &["sandbox_exec"], &temp_workspace());

        let output = run_step(step).await.expect("应成功");
        assert_eq!(output.kind, StepKind::Interrupt);
        let interrupt = output.interrupt.expect("interrupt");
        assert_eq!(interrupt["kind"], "before_tool");
        assert_eq!(interrupt["tool"], "sandbox_exec");
    }

    #[tokio::test]
    async fn local_tool_executes_in_workspace() {
        let workspace = temp_workspace();
        let base_url = spawn_llm(
            r#"{"action":"tool","tool":"file_write","arguments":{"path":"out/a.txt","content":"hi"}}"#,
        )
        .await;
        let step = step_input(&base_url, "never", &["file_write"], &workspace);

        let output = run_step(step).await.expect("应成功");
        assert_eq!(output.kind, StepKind::Tool);
        let observation = output.observation.expect("本地工具应执行");
        assert_eq!(observation["written"], true);
        assert_eq!(
            std::fs::read_to_string(workspace.join("out/a.txt")).expect("文件存在"),
            "hi"
        );
    }

    #[tokio::test]
    async fn rejected_approval_returns_answer_without_llm() {
        let mut step = step_input(
            "http://127.0.0.1:1/v1",
            "never",
            &["file_write"],
            &temp_workspace(),
        );
        step.approval = Some(json!({"approved": false}));

        let output = run_step(step).await.expect("应成功");
        assert_eq!(output.kind, StepKind::Final);
        assert!(
            output.final_output.expect("final")["answer"]
                .as_str()
                .is_some()
        );
    }

    #[tokio::test]
    async fn approved_approval_executes_local_tool() {
        let workspace = temp_workspace();
        let mut step = step_input(
            "http://127.0.0.1:1/v1",
            "never",
            &["file_write"],
            &workspace,
        );
        // 同时覆盖 `data` 嵌套（与平台 `approval_field` 一致）
        step.approval = Some(json!({
            "approved": true,
            "data": {
                "tool": "file_write",
                "arguments": {"path": "ok.txt", "content": "yes"}
            }
        }));

        let output = run_step(step).await.expect("应成功");
        assert_eq!(output.kind, StepKind::Tool);
        assert!(output.observation.is_some());
        assert_eq!(
            std::fs::read_to_string(workspace.join("ok.txt")).expect("文件存在"),
            "yes"
        );
    }

    #[tokio::test]
    async fn invalid_protocol_has_exit_code_2() {
        let mut step = step_input("http://127.0.0.1:1/v1", "never", &[], &temp_workspace());
        step.protocol = "v0".to_string();
        let error = run_step(step).await.expect_err("必须报错");
        assert_eq!(error.exit_code, 2);
    }
}

