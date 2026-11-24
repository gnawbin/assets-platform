//! adk-graph Agent 执行器。
//!
//! 每个 `ai-agent-task` / `ai-agent-resume` 服务任务运行同一张 StateGraph：
//! `agent` 节点调用 LLM，按 JSON 协议选择工具，工具结果写回 state 后循环，
//! 直到得到 final 答案或触发人工审批 interrupt。
//!
//! LLM 路由层没有 function calling，因此使用显式 JSON 协议。

use crate::persistence::PostgresCheckpointer;
use adk_graph::checkpoint::Checkpointer;
use adk_graph::edge::{END, START};
use adk_graph::error::GraphError;
use adk_graph::graph::StateGraph;
use adk_graph::node::{ExecutionConfig, NodeContext, NodeOutput};
use adk_graph::state::State;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use agent_core::{action, prompt};
use database::models::{
    ChatMessage, LLMChatRequest, RetrieveParams,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use service::llm_gateway_service::LLMRouter;
use service::rag_service::RAGRetriever;
use std::sync::Arc;
use std::time::Duration;

/// Agent 节点配置（来自 IR `node.config`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<i32>,
    /// 逗号分隔的工具名。空字符串表示默认工具。
    #[serde(default)]
    pub tools: Option<String>,
    /// never / before_tool / before_final
    #[serde(default)]
    pub interrupt_policy: Option<String>,
    #[serde(default)]
    pub max_iterations: Option<u32>,
    #[serde(default)]
    pub timeout_sec: Option<u64>,
    /// 知识库检索的 tree node id。
    #[serde(default)]
    pub knowledge_ids: Option<String>,
}

impl AgentConfig {
    /// 从节点 JSON 配置解析；缺省值保证空配置也可运行。
    pub fn from_value(value: &Value) -> Self {
        serde_json::from_value(value.clone()).unwrap_or_else(|_| Self::default())
    }

    fn tool_names(&self) -> Vec<String> {
        let names: Vec<String> = self
            .tools
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect();
        if names.is_empty() {
            vec!["knowledge_search".to_string(), "current_time".to_string()]
        } else {
            names
        }
    }

    fn interrupt_policy(&self) -> &str {
        self.interrupt_policy.as_deref().unwrap_or("before_tool")
    }

    fn max_iterations(&self) -> u32 {
        self.max_iterations.unwrap_or(6).clamp(1, 30)
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_sec.unwrap_or(120).clamp(5, 900))
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            model: None,
            temperature: None,
            max_tokens: None,
            tools: None,
            interrupt_policy: None,
            max_iterations: None,
            timeout_sec: None,
            knowledge_ids: None,
        }
    }
}

/// Agent 运行结果。
#[derive(Debug, Clone, Serialize)]
pub enum AgentOutcome {
    /// 正常结束。
    Completed { output: Value, state: Value },
    /// 等待人工审批。
    Interrupted {
        thread_id: String,
        checkpoint_id: String,
        payload: Value,
    },
    /// 执行失败。
    Failed(String),
}

/// Agent 运行入参。
#[derive(Debug, Clone)]
pub struct AgentInvocation {
    pub thread_id: String,
    pub node_id: String,
    pub config: AgentConfig,
    pub input: Value,
    pub approval: Option<Value>,
    /// 调用用户 ID（沙箱会话键 / 审计；未知时为 0）
    pub user_id: i64,
    /// 沙箱会话 scope（例如 `wf:<execution_id>`）
    pub sandbox_scope: String,
}

/// 工具执行上下文（沙箱工具需要租户 + 用户 + 会话）
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// 调用用户 ID（未知为 0）
    pub user_id: i64,
    /// 沙箱会话 scope
    pub scope: String,
}

impl ToolContext {
    /// 构造沙箱会话键（租户取当前 task-local schema）
    fn session_key(&self) -> service::sandbox_service::SessionKey {
        service::sandbox_service::SessionKey::new(
            database::current_schema_name(),
            self.user_id,
            self.scope.clone(),
        )
    }
}

/// adk-graph Agent 执行器。
#[derive(Clone)]
pub struct AgentGraphRunner {
    llm: Arc<LLMRouter>,
    checkpointer: Arc<PostgresCheckpointer>,
}

impl AgentGraphRunner {
    /// 创建 runner。
    pub fn new(llm: Arc<LLMRouter>, checkpointer: Arc<PostgresCheckpointer>) -> Self {
        Self { llm, checkpointer }
    }

    /// 运行 Agent。首次运行与恢复运行使用同一个方法，区别只在 `approval`。
    pub async fn run(&self, invocation: AgentInvocation) -> AgentOutcome {
        let runner = self.clone();
        let config = invocation.config.clone();
        let tool_ctx = ToolContext {
            user_id: invocation.user_id,
            scope: invocation.sandbox_scope.clone(),
        };
        let max_iterations = config.max_iterations();
        let recursion_limit = (max_iterations as usize).saturating_mul(2).saturating_add(8);

        let graph = StateGraph::with_channels(&[
            "input",
            "observations",
            "approval",
            "final",
            "error",
            "iterations",
            "pending_tool",
            "pending_final",
        ])
        .add_node_fn("agent", move |ctx| {
            let runner = runner.clone();
            let config = config.clone();
            let tool_ctx = tool_ctx.clone();
            async move { runner.agent_node(ctx, config, tool_ctx).await }
        })
        .add_edge(START, "agent")
        .add_conditional_edges(
            "agent",
            |state: &State| {
                if state.get("final").is_some() || state.get("error").is_some() {
                    END.to_string()
                } else {
                    "agent".to_string()
                }
            },
            [(END, END), ("agent", "agent")],
        )
        .compile();

        let graph = match graph {
            Ok(graph) => graph
                .with_checkpointer_arc(Arc::clone(&self.checkpointer) as Arc<dyn Checkpointer>)
                .with_recursion_limit(recursion_limit),
            Err(error) => return AgentOutcome::Failed(format!("Agent 图构建失败: {error}")),
        };

        let mut initial: State = State::new();
        initial.insert("input".to_string(), invocation.input.clone());
        initial.insert("observations".to_string(), json!([]));
        initial.insert("iterations".to_string(), json!(0));
        if let Some(approval) = invocation.approval.clone() {
            initial.insert("approval".to_string(), approval);
        }

        let execution = ExecutionConfig::new(&invocation.thread_id)
            .with_recursion_limit(recursion_limit);

        let result = tokio::time::timeout(
            invocation.config.timeout(),
            graph.invoke(initial, execution),
        )
        .await;

        match result {
            Err(_) => AgentOutcome::Failed(format!(
                "Agent 执行超时（{} 秒）",
                invocation.config.timeout().as_secs()
            )),
            Ok(Err(GraphError::Interrupted(interrupted))) => {
                let payload = match &interrupted.interrupt {
                    adk_graph::interrupt::Interrupt::Dynamic { message, data } => json!({
                        "kind": data
                            .as_ref()
                            .and_then(|value| value.get("kind"))
                            .and_then(Value::as_str)
                            .unwrap_or("dynamic"),
                        "message": message,
                        "data": data,
                        "thread_id": interrupted.thread_id,
                        "checkpoint_id": interrupted.checkpoint_id,
                        "node_id": invocation.node_id,
                        "step": interrupted.step,
                    }),
                    other => json!({
                        "kind": "static",
                        "message": other.to_string(),
                        "thread_id": interrupted.thread_id,
                        "checkpoint_id": interrupted.checkpoint_id,
                        "node_id": invocation.node_id,
                        "step": interrupted.step,
                    }),
                };
                AgentOutcome::Interrupted {
                    thread_id: interrupted.thread_id.clone(),
                    checkpoint_id: interrupted.checkpoint_id.clone(),
                    payload,
                }
            }
            Ok(Err(error)) => AgentOutcome::Failed(format!("Agent 执行失败: {error}")),
            Ok(Ok(state)) => {
                let output = state
                    .get("final")
                    .cloned()
                    .or_else(|| state.get("error").cloned())
                    .unwrap_or_else(|| json!({"answer": ""}));
                AgentOutcome::Completed {
                    output,
                    state: serde_json::to_value(&state).unwrap_or(Value::Null),
                }
            }
        }
    }

    /// StateGraph 的 `agent` 节点。
    async fn agent_node(
        &self,
        ctx: NodeContext,
        config: AgentConfig,
        tool_ctx: ToolContext,
    ) -> Result<NodeOutput, GraphError> {
        let mut observations = read_array(&ctx.state, "observations");
        let mut iterations = ctx
            .state
            .get("iterations")
            .and_then(Value::as_u64)
            .unwrap_or(0);

        // 审批恢复：直接执行被挂起的工具或返回被挂起的 final，避免重新询问 LLM。
        if let Some(approval) = ctx.state.get("approval") {
            let approved = approval
                .get("approved")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !approved {
                return Ok(NodeOutput::new()
                    .with_update("final", json!({"answer": "用户未批准本次操作。"}))
                    .with_update("observations", json!(observations))
                    .with_goto([END]));
            }

            if let Some(tool) = approval_field(approval, "tool").and_then(|value| match value {
                Value::String(name) => Some(name),
                Value::Null => None,
                other => Some(other.to_string()),
            }) {
                let arguments = approval
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let observation = self
                    .execute_tool(&tool_ctx, &tool, &arguments)
                    .await
                    .unwrap_or_else(|error| json!({"tool": tool.clone(), "error": error}));
                observations.push(json!({"tool": tool, "result": observation}));
                return Ok(NodeOutput::new()
                    .with_update("observations", json!(observations))
                    .with_update("approval", Value::Null)
                    .with_update("pending_tool", Value::Null)
                    .with_goto(["agent"]));
            }

            if let Some(answer) = approval_field(approval, "answer") {
                return Ok(NodeOutput::new()
                    .with_update("final", json!({"answer": answer}))
                    .with_update("approval", Value::Null)
                    .with_update("pending_final", Value::Null)
                    .with_goto([END]));
            }
        }

        iterations += 1;
        if iterations > u64::from(config.max_iterations()) {
            return Ok(NodeOutput::new()
                .with_update(
                    "final",
                    json!({"answer": "Agent 达到最大迭代次数，已停止。", "iterations": iterations}),
                )
                .with_update("iterations", json!(iterations))
                .with_goto([END]));
        }

        let output = match self.call_llm(&ctx.state, &config, &observations).await {
            Ok(output) => output,
            Err(error) => {
                return Ok(NodeOutput::new()
                    .with_update("final", json!({"answer": "", "error": error}))
                    .with_update("iterations", json!(iterations))
                    .with_goto([END]));
            }
        };

        if let Some(answer) = output.get("answer").and_then(Value::as_str) {
            if config.interrupt_policy() == "before_final" {
                return Ok(NodeOutput::interrupt_with_data(
                    "agent_approval_required",
                    json!({
                        "kind": "before_final",
                        "answer": answer,
                        "thought": output.get("thought").cloned().unwrap_or(Value::Null),
                    }),
                ));
            }
            return Ok(NodeOutput::new()
                .with_update("final", json!({"answer": answer}))
                .with_update("iterations", json!(iterations))
                .with_goto([END]));


        }

        let tool = output
            .get("tool")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if tool.is_empty() {
            observations.push(json!({
                "error": "LLM 返回既没有 answer 也没有 tool",
                "raw": output,
            }));
            return Ok(NodeOutput::new()
                .with_update("observations", json!(observations))
                .with_update("iterations", json!(iterations))
                .with_goto(["agent"]));
        }
        let arguments = output
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        if config.interrupt_policy() == "before_tool" {
            return Ok(NodeOutput::interrupt_with_data(
                "agent_approval_required",
                json!({
                    "kind": "before_tool",
                    "tool": tool,
                    "arguments": arguments,
                    "thought": output.get("thought").cloned().unwrap_or(Value::Null),
                }),
            ));
        }

        let observation = self
            .execute_tool(&tool_ctx, &tool, &arguments)
            .await
            .unwrap_or_else(|error| json!({"tool": tool, "error": error}));
        observations.push(json!({"tool": tool, "arguments": arguments, "result": observation}));
        Ok(NodeOutput::new()
            .with_update("observations", json!(observations))
            .with_update("iterations", json!(iterations))
            .with_goto(["agent"]))
    }

    async fn call_llm(
        &self,
        state: &State,
        config: &AgentConfig,
        observations: &[Value],
    ) -> Result<Value, String> {
        let sandbox_ok = sandbox_tools_available();
        let tools = filter_tools(config.tool_names(), sandbox_ok);
        let system = prompt::build_system(&tools, sandbox_ok);

        let input = state.get("input").cloned().unwrap_or(Value::Null);
        let user = prompt::build_user(&config.prompt, &input, observations);

        let request = LLMChatRequest {
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system,
                    content_parts: None,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user,
                    content_parts: None,
                },
            ],
            model: config.model.clone(),
            temperature: config.temperature,
            max_tokens: config.max_tokens,
            stream: Some(false),
            user_id: None,
            conv_id: None,
        };

        let response = self.llm.chat(request).await?;
        action::parse_llm_json(&response.content)
    }

    async fn execute_tool(&self, ctx: &ToolContext, tool: &str, arguments: &Value) -> Result<Value, String> {
        match tool {
            "knowledge_search" => {
                let question = arguments
                    .get("question")
                    .or_else(|| arguments.get("query"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if question.is_empty() {
                    return Err("knowledge_search 缺少 question 参数".to_string());
                }
                let tree_node_id = arguments.get("tree_node_id").and_then(|value| {
                    value
                        .as_i64()
                        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
                });
                let top_k = arguments
                    .get("top_k")
                    .and_then(Value::as_i64)
                    .unwrap_or(5)
                    .clamp(1, 20) as i32;

                let params = RetrieveParams {
                    question,
                    bind_tree_node_id: tree_node_id,
                    top_k,
                    max_tokens: 2000,
                    okf_type_filter: None,
                    min_similarity: 0.0,
                };
                let chunks = RAGRetriever::retrieve(&params).await?;
                let results: Vec<Value> = chunks
                    .into_iter()
                    .map(|chunk| {
                        json!({
                            "title": chunk.title,
                            "text": chunk.chunk_text,
                            "similarity": chunk.similarity,
                            "asset_id": chunk.asset_id.to_string(),
                        })
                    })
                    .collect();
                Ok(json!({"results": results, "count": results.len()}))
            }
            "current_time" => Ok(json!({"now": chrono::Utc::now().to_rfc3339()})),
            TOOL_SANDBOX_EXEC => {
                ensure_sandbox_enabled()?;
                let (command, argv, cwd, timeout_secs) = parse_exec_arguments(arguments)?;
                let result = service::sandbox_service::run_command_with_options(
                    ctx.session_key(),
                    command,
                    argv,
                    cwd,
                    timeout_secs,
                )
                .await?;
                Ok(json!({
                    "exit_code": result.exit_code,
                    "text": truncate_chars(&result.combined_output(), MAX_TOOL_OUTPUT_CHARS),
                    "stderr": result.stderr,
                    "duration_ms": result.duration_ms,
                }))
            }
            TOOL_FILE_READ => {
                ensure_sandbox_enabled()?;
                let path = required_path(arguments)?;
                let data = service::sandbox_service::download_file(ctx.session_key(), path.clone())
                    .await?;
                let text = String::from_utf8(data).map_err(|_| {
                    "文件不是 UTF-8 文本；二进制请用 sandbox_exec + base64".to_string()
                })?;
                let total = text.chars().count();
                Ok(json!({
                    "path": path,
                    "size": text.len(),
                    "truncated": total > MAX_TOOL_OUTPUT_CHARS,
                    "content": truncate_chars(&text, MAX_TOOL_OUTPUT_CHARS),
                }))
            }
            TOOL_FILE_WRITE => {
                ensure_sandbox_enabled()?;
                let (path, data) = parse_write_payload(arguments)?;
                let size = data.len();
                service::sandbox_service::upload_file(ctx.session_key(), path.clone(), data).await?;
                Ok(json!({"path": path, "size": size, "written": true}))
            }
            other => Err(format!("不支持的工具: {other}")),
        }
    }
}

/// 沙箱工具名（仅当 `mode != in_process` 时对 Agent 可见）
const TOOL_SANDBOX_EXEC: &str = "sandbox_exec";
const TOOL_FILE_READ: &str = "file_read";
const TOOL_FILE_WRITE: &str = "file_write";

/// 工具输出回灌给 LLM 的最大字符数（避免撑爆上下文）
const MAX_TOOL_OUTPUT_CHARS: usize = 8000;

/// 当前是否可用沙箱工具
fn sandbox_tools_available() -> bool {
    service::sandbox_service::mode() != service::sandbox_service::SandboxMode::InProcess
}

/// 沙箱未启用时给出可操作错误（而不是让工具「看起来存在但永远失败」）
fn ensure_sandbox_enabled() -> Result<(), String> {
    if sandbox_tools_available() {
        Ok(())
    } else {
        Err(
            "沙箱未启用（mode=in_process）：请把配置 `[sandbox] mode` 设为 `cube` 或 `openshell`，或改用 knowledge_search / current_time"
                .to_string(),
        )
    }
}

/// 是否属于沙箱工具
fn is_sandbox_tool(name: &str) -> bool {
    matches!(name, TOOL_SANDBOX_EXEC | TOOL_FILE_READ | TOOL_FILE_WRITE)
}

/// 过滤工具清单：沙箱不可用时隐藏沙箱工具
fn filter_tools(tools: Vec<String>, sandbox_ok: bool) -> Vec<String> {
    tools
        .into_iter()
        .filter(|tool| sandbox_ok || !is_sandbox_tool(tool))
        .collect()
}

/// `sandbox_exec` 解析结果：`(command, argv, cwd, timeout_secs)`
type SandboxExecArgs = (String, Vec<String>, Option<String>, Option<u32>);

/// 解析 `sandbox_exec` 参数
fn parse_exec_arguments(arguments: &Value) -> Result<SandboxExecArgs, String> {
    let command = arguments
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    let argv: Vec<String> = arguments
        .get("argv")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();

    // `argv` 模式下必须有 command 作为可执行文件
    let command = command.ok_or_else(|| {
        "sandbox_exec 缺少 command 参数（shell 命令；或与 argv 搭配的可执行文件）".to_string()
    })?;

    let cwd = arguments
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    let timeout_secs = arguments.get("timeout_secs").and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
    });
    let timeout_secs = timeout_secs.map(|secs| u32::try_from(secs).unwrap_or(u32::MAX));

    Ok((command, argv, cwd, timeout_secs))
}

/// 必填的 `path` 参数
fn required_path(arguments: &Value) -> Result<String, String> {
    arguments
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| "缺少 path 参数（沙箱内绝对路径）".to_string())
}

/// 解析 `file_write` 载荷：优先 `content_base64`，否则 `content`
fn parse_write_payload(arguments: &Value) -> Result<(String, Vec<u8>), String> {
    let path = required_path(arguments)?;

    if let Some(encoded) = arguments
        .get("content_base64")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let data = BASE64
            .decode(encoded)
            .map_err(|error| format!("content_base64 解码失败：{error}"))?;
        return Ok((path, data));
    }

    let content = arguments
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| "file_write 需要 content 或 content_base64".to_string())?;
    Ok((path, content.as_bytes().to_vec()))
}

/// 截断到最多 `max_chars` 个字符（按字符而不是字节，避免切坏 UTF-8）
fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(max_chars).collect();
    truncated.push_str("\n…（输出已截断）");
    truncated
}

fn read_array(state: &State, key: &str) -> Vec<Value> {
    state
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn approval_field(approval: &Value, key: &str) -> Option<Value> {
    approval
        .get(key)
        .cloned()
        .or_else(|| approval.get("data").and_then(|data| data.get(key)).cloned())
}

/// 解析 LLM 返回的 JSON；兼容 ```json 代码块和前后说明文字。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_tools_hides_sandbox_tools_when_disabled() {
        let tools = vec![
            "knowledge_search".to_string(),
            "current_time".to_string(),
            "sandbox_exec".to_string(),
            "file_read".to_string(),
            "file_write".to_string(),
        ];

        let hidden = filter_tools(tools.clone(), false);
        assert_eq!(hidden, vec!["knowledge_search", "current_time"]);

        let visible = filter_tools(tools, true);
        assert_eq!(visible.len(), 5);
    }

    #[test]
    fn parse_exec_arguments_accepts_command_and_argv() {
        let (command, argv, cwd, timeout) = parse_exec_arguments(&serde_json::json!({
            "command": "ls",
            "argv": ["-l", "/tmp"],
            "cwd": "/workspace",
            "timeout_secs": 15
        }))
        .expect("参数合法");
        assert_eq!(command, "ls");
        assert_eq!(argv, vec!["-l".to_string(), "/tmp".to_string()]);
        assert_eq!(cwd.as_deref(), Some("/workspace"));
        assert_eq!(timeout, Some(15));
    }

    #[test]
    fn parse_exec_arguments_requires_command() {
        let error = parse_exec_arguments(&serde_json::json!({"argv": ["-l"]}))
            .expect_err("缺少 command 必须报错");
        assert!(error.contains("command"), "{error}");

        let error = parse_exec_arguments(&serde_json::json!({"command": "   "}))
            .expect_err("空白 command 必须报错");
        assert!(error.contains("command"), "{error}");
    }

    #[test]
    fn parse_write_payload_prefers_base64() {
        let (path, data) = parse_write_payload(&serde_json::json!({
            "path": "/workspace/a.txt",
            "content_base64": BASE64.encode(b"hello"),
            "content": "ignored"
        }))
        .expect("base64 载荷合法");
        assert_eq!(path, "/workspace/a.txt");
        assert_eq!(data, b"hello");
    }

    #[test]
    fn parse_write_payload_falls_back_to_content() {
        let (path, data) = parse_write_payload(&serde_json::json!({
            "path": "/workspace/a.txt",
            "content": "你好"
        }))
        .expect("文本载荷合法");
        assert_eq!(path, "/workspace/a.txt");
        assert_eq!(data, "你好".as_bytes());

        let error = parse_write_payload(&serde_json::json!({"path": "/x"}))
            .expect_err("缺少内容必须报错");
        assert!(error.contains("content"), "{error}");
    }

    #[test]
    fn truncate_chars_respects_utf8_boundaries() {
        let text = "你好世界";
        assert_eq!(truncate_chars(text, 10), text);

        let truncated = truncate_chars(text, 2);
        assert!(truncated.starts_with("你好"), "{truncated}");
        assert!(truncated.contains("截断"), "{truncated}");
        assert_eq!(truncated.chars().take(2).collect::<String>(), "你好");
    }

    #[test]
    fn sandbox_tool_names_are_stable() {
        assert!(is_sandbox_tool("sandbox_exec"));
        assert!(is_sandbox_tool("file_read"));
        assert!(is_sandbox_tool("file_write"));
        assert!(!is_sandbox_tool("knowledge_search"));
        assert!(!is_sandbox_tool("current_time"));
    }
}
