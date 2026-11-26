//! step 协议 v1（`assets-agent-step/v1`）
//!
//! 平台与沙箱内 runner 通过 `/workspace/in/step.json` / `out/step.json` 交换，
//! 一次执行 = 一次 LLM 调用 + 最多一次工具执行（"单步无状态"）。
//!
//! 字段全部 `serde(default)` 友好：旧字段缺失时取安全默认值。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 协议版本
pub const PROTOCOL: &str = "assets-agent-step/v1";

fn default_protocol() -> String {
    PROTOCOL.to_string()
}

fn default_workspace() -> String {
    "/workspace".to_string()
}

fn default_token_env() -> String {
    "AGENT_TOKEN".to_string()
}

const fn default_llm_timeout() -> u64 {
    120
}

const fn default_exec_timeout() -> u64 {
    60
}

const fn default_max_output_bytes() -> usize {
    1024 * 1024
}

const fn default_max_file_bytes() -> usize {
    32 * 1024 * 1024
}

/// 单步输入
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[allow(clippy::derive_partial_eq_without_eq)] // 含 f32（temperature），无法实现 Eq
pub struct StepInput {
    /// 协议版本（必须等于 [`PROTOCOL`]）
    #[serde(default = "default_protocol")]
    pub protocol: String,
    /// 工作流线程（`<execution_id>:<node_id>`）
    pub thread_id: String,
    /// 当前节点
    pub node_id: String,
    /// 第几步（从 0 开始）
    #[serde(default)]
    pub step: u32,
    /// Agent 配置
    pub agent: AgentStep,
    /// 业务输入
    #[serde(default)]
    pub input: Value,
    /// 已有工具观察
    #[serde(default)]
    pub observations: Vec<Value>,
    /// 审批结果（恢复时传入）
    #[serde(default)]
    pub approval: Option<Value>,
    /// LLM 网关配置
    #[serde(default)]
    pub llm: LlmStep,
    /// 工作区根目录（文件工具限制在此目录内）
    #[serde(default = "default_workspace")]
    pub workspace: String,
    /// 执行限额
    #[serde(default)]
    pub limits: StepLimits,
}

impl StepInput {
    /// 校验协议版本与必填项
    ///
    /// # Errors
    /// 协议版本不符 / `thread_id` 为空 / `llm.base_url` 为空时返回错误。
    pub fn validate(&self) -> Result<(), String> {
        if self.protocol != PROTOCOL {
            return Err(format!(
                "协议版本不符：期望 {PROTOCOL}，实际 {}",
                self.protocol
            ));
        }
        if self.thread_id.trim().is_empty() {
            return Err("thread_id 不能为空".to_string());
        }
        if self.llm.base_url.trim().is_empty() {
            return Err("llm.base_url 不能为空（平台内部 LLM 端点）".to_string());
        }
        Ok(())
    }
}

/// Agent 子配置
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[allow(clippy::derive_partial_eq_without_eq)] // 含 f32（temperature），无法实现 Eq
pub struct AgentStep {
    /// 任务说明
    #[serde(default)]
    pub prompt: String,
    /// 模型（空 → `llm.default_model`）
    #[serde(default)]
    pub model: Option<String>,
    /// 温度
    #[serde(default)]
    pub temperature: Option<f32>,
    /// 最大输出 token
    #[serde(default)]
    pub max_tokens: Option<i32>,
    /// 可用工具（空 → 平台默认两种）
    #[serde(default)]
    pub tools: Vec<String>,
    /// `never` / `before_tool` / `before_final`
    #[serde(default)]
    pub interrupt_policy: Option<String>,
    /// 最大迭代次数（仅提示用）
    #[serde(default)]
    pub max_iterations: Option<u32>,
}

impl AgentStep {
    /// 工具清单（空时回落平台默认）
    #[must_use]
    pub fn tool_names(&self) -> Vec<String> {
        if self.tools.is_empty() {
            vec!["knowledge_search".to_string(), "current_time".to_string()]
        } else {
            self.tools.clone()
        }
    }

    /// 审批策略（缺省 `before_tool`）
    #[must_use]
    pub fn interrupt_policy(&self) -> &str {
        self.interrupt_policy.as_deref().unwrap_or("before_tool")
    }
}

/// LLM 网关配置（L1：沙箱出网访问平台内部端点）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmStep {
    /// 平台内部 LLM 端点（OpenAI 兼容，例如 `http://assets-server:3001/internal/llm/v1`）
    #[serde(default)]
    pub base_url: String,
    /// 从该环境变量读取 Bearer token
    #[serde(default = "default_token_env")]
    pub token_env: String,
    /// 缺省模型
    #[serde(default)]
    pub default_model: String,
    /// 单次 LLM 超时（秒）
    #[serde(default = "default_llm_timeout")]
    pub timeout_secs: u64,
}

impl Default for LlmStep {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            token_env: default_token_env(),
            default_model: String::new(),
            timeout_secs: default_llm_timeout(),
        }
    }
}

/// 执行限额
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepLimits {
    /// 单次命令超时（秒）
    #[serde(default = "default_exec_timeout")]
    pub exec_timeout_secs: u64,
    /// 回灌给 LLM 的最大字节数
    #[serde(default = "default_max_output_bytes")]
    pub max_output_bytes: usize,
    /// 单文件读写上限（字节）
    #[serde(default = "default_max_file_bytes")]
    pub max_file_bytes: usize,
}

impl Default for StepLimits {
    fn default() -> Self {
        Self {
            exec_timeout_secs: default_exec_timeout(),
            max_output_bytes: default_max_output_bytes(),
            max_file_bytes: default_max_file_bytes(),
        }
    }
}

/// 单步结果类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepKind {
    /// 最终答案
    Final,
    /// 工具意图 / 本地工具观察
    Tool,
    /// 需要人工审批
    Interrupt,
    /// 执行失败
    Error,
}

/// token 用量（由 runner 从 LLM 响应回传，平台计入 `llm_call_record`）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepUsage {
    /// 输入 token
    #[serde(default)]
    pub input_tokens: i32,
    /// 输出 token
    #[serde(default)]
    pub output_tokens: i32,
}

/// 单步输出
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepOutput {
    /// 协议版本
    #[serde(default = "default_protocol")]
    pub protocol: String,
    /// 结果类型
    pub kind: StepKind,
    /// LLM 动作原文（`{"action":"tool",...}`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Value>,
    /// 本地工具执行结果（平台工具时为 `None`，由平台执行）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<Value>,
    /// 最终答案
    #[serde(rename = "final", default, skip_serializing_if = "Option::is_none")]
    pub final_output: Option<Value>,
    /// 中断描述（`kind` / `tool` / `arguments` / `answer`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interrupt: Option<Value>,
    /// 状态增量（平台合并进检查点）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_patch: Option<Value>,
    /// token 用量
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<StepUsage>,
    /// 错误信息
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl StepOutput {
    fn new(kind: StepKind) -> Self {
        Self {
            protocol: default_protocol(),
            kind,
            action: None,
            observation: None,
            final_output: None,
            interrupt: None,
            state_patch: None,
            usage: None,
            error: None,
        }
    }

    /// 最终答案
    #[must_use]
    pub fn final_answer(answer: Value) -> Self {
        Self {
            final_output: Some(answer),
            ..Self::new(StepKind::Final)
        }
    }

    /// 工具意图（`observation` 为本地工具结果；平台工具留 `None`）
    #[must_use]
    pub fn tool(action: Value, observation: Option<Value>) -> Self {
        Self {
            action: Some(action),
            observation,
            ..Self::new(StepKind::Tool)
        }
    }

    /// 审批中断
    #[must_use]
    pub fn interrupt(interrupt: Value) -> Self {
        Self {
            interrupt: Some(interrupt),
            ..Self::new(StepKind::Interrupt)
        }
    }

    /// 错误
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            error: Some(message.into()),
            ..Self::new(StepKind::Error)
        }
    }

    /// 附带 token 用量
    #[must_use]
    pub const fn with_usage(mut self, usage: StepUsage) -> Self {
        self.usage = Some(usage);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> StepInput {
        serde_json::from_value(json!({
            "protocol": PROTOCOL,
            "thread_id": "exec-1:node-1",
            "node_id": "node-1",
            "step": 2,
            "agent": {
                "prompt": "整理资产",
                "model": "gpt-4o-mini",
                "tools": ["knowledge_search", "sandbox_exec"],
                "interrupt_policy": "before_tool"
            },
            "input": {"asset_id": "1"},
            "observations": [],
            "llm": {
                "base_url": "http://assets-server:3001/internal/llm/v1",
                "default_model": "gpt-4o-mini"
            }
        }))
        .expect("示例输入应可解析")
    }

    #[test]
    fn parses_sample_and_applies_defaults() {
        let step = sample();
        assert_eq!(step.protocol, PROTOCOL);
        assert_eq!(step.workspace, "/workspace");
        assert_eq!(step.llm.token_env, "AGENT_TOKEN");
        assert_eq!(step.limits.exec_timeout_secs, 60);
        assert_eq!(step.limits.max_file_bytes, 32 * 1024 * 1024);
        assert_eq!(
            step.agent.tool_names(),
            vec!["knowledge_search", "sandbox_exec"]
        );
        assert!(step.validate().is_ok());
    }

    #[test]
    fn empty_tools_fall_back_to_platform_defaults() {
        let mut step = sample();
        step.agent.tools.clear();
        assert_eq!(
            step.agent.tool_names(),
            vec!["knowledge_search", "current_time"]
        );
        assert_eq!(step.agent.interrupt_policy(), "before_tool");
    }

    #[test]
    fn validate_rejects_bad_protocol_and_missing_fields() {
        let mut step = sample();
        step.protocol = "v0".to_string();
        assert!(step.validate().unwrap_err().contains("协议版本"));

        let mut step = sample();
        step.thread_id = "  ".to_string();
        assert!(step.validate().unwrap_err().contains("thread_id"));

        let mut step = sample();
        step.llm.base_url.clear();
        assert!(step.validate().unwrap_err().contains("base_url"));
    }

    #[test]
    fn output_serializes_kind_and_final_alias() {
        let output = StepOutput::final_answer(json!({"answer": "ok"}));
        let value = serde_json::to_value(&output).expect("应可序列化");
        assert_eq!(value["protocol"], PROTOCOL);
        assert_eq!(value["kind"], "final");
        assert_eq!(value["final"]["answer"], "ok");
        assert!(value.get("observation").is_none());
    }

    #[test]
    fn output_constructors_cover_all_kinds() {
        let tool = StepOutput::tool(json!({"tool": "sandbox_exec"}), Some(json!({"exit_code": 0})));
        assert_eq!(tool.kind, StepKind::Tool);
        assert!(tool.observation.is_some());

        let interrupt = StepOutput::interrupt(json!({"kind": "before_tool"}));
        assert_eq!(interrupt.kind, StepKind::Interrupt);

        let error = StepOutput::error("boom");
        assert_eq!(error.kind, StepKind::Error);
        assert_eq!(error.error.as_deref(), Some("boom"));

        let with_usage = StepOutput::final_answer(json!({"answer": "x"})).with_usage(StepUsage {
            input_tokens: 3,
            output_tokens: 5,
        });
        assert_eq!(with_usage.usage.expect("usage").output_tokens, 5);
    }

    #[test]
    fn output_round_trips() {
        let output = StepOutput::tool(json!({"tool": "file_read"}), None);
        let text = serde_json::to_string(&output).expect("序列化");
        let parsed: StepOutput = serde_json::from_str(&text).expect("反序列化");
        assert_eq!(parsed, output);
    }
}

