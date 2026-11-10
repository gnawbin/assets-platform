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
            async move { runner.agent_node(ctx, config).await }
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
                    .execute_tool(&tool, &arguments)
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
            .execute_tool(&tool, &arguments)
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
        let tools = config.tool_names();
        let system = format!(
            "你是资产平台的工作流 Agent。请严格只输出一个 JSON 对象，不要输出 Markdown。\n\
             可用工具：{}。\n\
             需要调用工具时输出：{{\"action\":\"tool\",\"tool\":\"工具名\",\"arguments\":{{}},\"thought\":\"简短理由\"}}\n\
             可以给出最终答案时输出：{{\"action\":\"final\",\"answer\":\"最终答案\",\"thought\":\"简短理由\"}}\n\
             工具参数：knowledge_search 支持 question、tree_node_id、top_k；current_time 无参数。",
            tools.join(", ")
        );

        let input = state.get("input").cloned().unwrap_or(Value::Null);
        let user = format!(
            "任务说明：\n{}\n\n业务输入：\n{}\n\n已有工具观察：\n{}",
            if config.prompt.trim().is_empty() {
                "根据业务输入完成用户任务。"
            } else {
                config.prompt.as_str()
            },
            serde_json::to_string_pretty(&input).unwrap_or_else(|_| "null".to_string()),
            serde_json::to_string_pretty(&observations).unwrap_or_else(|_| "[]".to_string()),
        );

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
        parse_llm_json(&response.content)
    }

    async fn execute_tool(&self, tool: &str, arguments: &Value) -> Result<Value, String> {
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
            other => Err(format!("不支持的工具: {other}")),
        }
    }
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
fn parse_llm_json(content: &str) -> Result<Value, String> {
    let trimmed = content.trim();
    let without_fence = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);

    if let Ok(value) = serde_json::from_str::<Value>(without_fence) {
        return validate_action(value);
    }

    let start = without_fence.find('{');
    let end = without_fence.rfind('}');
    if let (Some(start), Some(end)) = (start, end) {
        if end > start {
            if let Ok(value) = serde_json::from_str::<Value>(&without_fence[start..=end]) {
                return validate_action(value);
            }
        }
    }

    Err(format!(
        "LLM 未返回合法 JSON：{}",
        without_fence.chars().take(300).collect::<String>()
    ))
}

fn validate_action(value: Value) -> Result<Value, String> {
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


