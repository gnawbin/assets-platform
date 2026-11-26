//! 沙箱内 Agent step 循环（M1b / M2）
//!
//! 平台侧只做「编排 + 状态 + 平台工具」，沙箱侧 `agent-runner` 只做
//! 「LLM 调用 + 本地工具」。一次 exec = 一步：
//! 写 `/workspace/in/step.json` → `agent-runner step` → 读 `/workspace/out/step.json`。
//!
//! 开关：`SANDBOX_AGENT_IN_SANDBOX=true` **且** `mode=openshell` 才启用；
//! 其它组合一律回退 M1a（`AgentGraphRunner`，平台进程内）。

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use agent_core::protocol::{
    AgentStep, LlmStep, PROTOCOL, StepInput, StepKind, StepLimits, StepOutput, StepUsage,
};
use sandbox::ExecResult;
use serde_json::{Value, json};
use service::sandbox_audit::AgentEvent;
use service::sandbox_service::{self, SandboxMode, SessionKey};

use crate::agent::{AgentConfig, AgentInvocation, AgentOutcome, execute_platform_tool};

/// 单步输入/输出固定路径（镜像内 `WORKDIR /workspace`）
const STEP_INPUT_PATH: &str = "/workspace/in/step.json";
const STEP_OUTPUT_PATH: &str = "/workspace/out/step.json";
/// 镜像内的 runner 可执行文件
const RUNNER_BIN: &str = "agent-runner";

/// 沙箱 IO 抽象（便于单测注入假实现）
#[async_trait::async_trait]
pub trait SandboxIo: Send + Sync {
    /// 执行前准备（生产实现用于把 `AGENT_TOKEN` 注入沙箱环境；缺省不做事）
    async fn prepare(&self, _key: &SessionKey) -> Result<(), String> {
        Ok(())
    }
    /// 写文件（沙箱内绝对路径）
    async fn write_file(&self, key: &SessionKey, path: &str, data: Vec<u8>) -> Result<(), String>;
    /// 读文件
    async fn read_file(&self, key: &SessionKey, path: &str) -> Result<Vec<u8>, String>;
    /// 执行命令
    async fn exec(
        &self,
        key: &SessionKey,
        command: String,
        argv: Vec<String>,
        cwd: Option<String>,
        timeout_secs: Option<u32>,
    ) -> Result<ExecResult, String>;
    /// 审计事件（生产实现写 `sys_oper_log`；缺省丢弃，便于单测注入假实现）
    async fn audit(&self, _event: Value) {}
}

/// 默认沙箱 IO：走 `service::sandbox_service`（Cube / OpenShell 通用）
///
/// `token` 非空时在 [`Self::prepare`] 阶段登记为沙箱环境变量（键名 `token_env`），
/// 供沙箱内 `agent-runner` 调用平台内部 LLM 端点。
pub struct ServiceSandboxIo {
    token_env: String,
    token: Option<String>,
}

impl ServiceSandboxIo {
    /// 构造：`token` 为空表示不注入（沙箱内 LLM 调用会缺少 Bearer token）
    #[must_use]
    pub fn new(token_env: String, token: Option<String>) -> Self {
        Self { token_env, token }
    }
}

#[async_trait::async_trait]
impl SandboxIo for ServiceSandboxIo {
    async fn prepare(&self, key: &SessionKey) -> Result<(), String> {
        let Some(token) = self
            .token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(());
        };
        let mut env = BTreeMap::new();
        env.insert(self.token_env.clone(), token.to_string());
        sandbox_service::prepare_session_env(key.clone(), env);
        Ok(())
    }

    async fn write_file(&self, key: &SessionKey, path: &str, data: Vec<u8>) -> Result<(), String> {
        sandbox_service::upload_file(key.clone(), path.to_string(), data).await
    }

    async fn read_file(&self, key: &SessionKey, path: &str) -> Result<Vec<u8>, String> {
        sandbox_service::download_file(key.clone(), path.to_string()).await
    }

    async fn exec(
        &self,
        key: &SessionKey,
        command: String,
        argv: Vec<String>,
        cwd: Option<String>,
        timeout_secs: Option<u32>,
    ) -> Result<ExecResult, String> {
        sandbox_service::run_command_with_options(key.clone(), command, argv, cwd, timeout_secs)
            .await
    }

    /// 写 `sys_oper_log`：后台任务执行，失败只 WARN（不反压 step 循环）
    async fn audit(&self, event: Value) {
        let scope = event
            .get("labels")
            .and_then(|labels| labels.get("session_id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        tokio::spawn(async move {
            let Ok(pool) = database::get_write_pool() else {
                tracing::warn!(%scope, "沙箱 Agent 审计跳过：写库连接不可用");
                return;
            };
            let outcome = service::sandbox_audit::record_agent(&pool, &event).await;
            if outcome.inserted == 0 {
                tracing::warn!(?outcome, %scope, "沙箱 Agent 审计未写入");
            }
        });
    }
}

/// 沙箱 Agent 执行器
pub struct SandboxAgentRunner {
    io: Arc<dyn SandboxIo>,
    step_timeout_secs: u32,
    max_steps: u32,
    llm_base_url: String,
    token_env: String,
    default_model: String,
    exec_timeout_secs: u64,
    /// 是否逐步写 `agent_step` 审计（`SANDBOX_AGENT_AUDIT_STEPS`，缺省 true）
    audit_steps: bool,
}

impl SandboxAgentRunner {
    /// 生产构造：读取 `SANDBOX_AGENT_*` 配置
    #[must_use]
    pub fn new() -> Self {
        let token_env =
            env_string("SANDBOX_AGENT_TOKEN_ENV").unwrap_or_else(|| "AGENT_TOKEN".to_string());
        let token = env_string("SANDBOX_AGENT_TOKEN");
        if token.is_none() {
            static WARN_ONCE: std::sync::Once = std::sync::Once::new();
            WARN_ONCE.call_once(|| {
                tracing::warn!(
                    "SANDBOX_AGENT_TOKEN 未配置：沙箱内 agent-runner 调内部 LLM 端点会缺少 Bearer token（401）。\
                     请在 .env.toml 的 [sandbox] 段设置 agent_token（租户级 API Key，scope 含 sandbox:llm）"
                );
            });
        }

        Self {
            io: Arc::new(ServiceSandboxIo::new(token_env.clone(), token)),
            step_timeout_secs: env_u32("SANDBOX_AGENT_STEP_TIMEOUT_SECS", 120).max(1),
            max_steps: env_u32("SANDBOX_AGENT_MAX_STEPS", 6).max(1),
            llm_base_url: env_string("SANDBOX_AGENT_LLM_BASE_URL").unwrap_or_default(),
            token_env,
            default_model: env_string("SANDBOX_AGENT_MODEL").unwrap_or_default(),
            exec_timeout_secs: u64::from(env_u32("SANDBOX_AGENT_EXEC_TIMEOUT_SECS", 60).max(1)),
            audit_steps: env_bool("SANDBOX_AGENT_AUDIT_STEPS", true),
        }
    }

    /// 测试构造：注入 IO，其余配置取缺省
    #[must_use]
    pub fn with_io(io: Arc<dyn SandboxIo>, step_timeout_secs: u32, max_steps: u32) -> Self {
        Self {
            io,
            step_timeout_secs: step_timeout_secs.max(1),
            max_steps: max_steps.max(1),
            llm_base_url: env_string("SANDBOX_AGENT_LLM_BASE_URL")
                .unwrap_or_else(|| "http://127.0.0.1:1/internal/llm/v1".to_string()),
            token_env: "AGENT_TOKEN".to_string(),
            default_model: String::new(),
            exec_timeout_secs: 60,
            audit_steps: true,
        }
    }
}

impl Default for SandboxAgentRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// 是否启用「Agent 在沙箱内」（`SANDBOX_AGENT_IN_SANDBOX` + `mode=openshell`）
///
/// 组合不成立时只 WARN 一次并回退 M1a（避免「Agent 常驻 Cube 沙箱」这种未实现的组合）。
#[must_use]
pub fn enabled() -> bool {
    let in_sandbox = env_bool("SANDBOX_AGENT_IN_SANDBOX", false);
    let mode = sandbox_service::mode();
    if in_sandbox && !matches!(mode, SandboxMode::OpenShell) {
        static WARN_ONCE: std::sync::Once = std::sync::Once::new();
        WARN_ONCE.call_once(|| {
            tracing::warn!(
                "SANDBOX_AGENT_IN_SANDBOX=true 但沙箱 mode={mode:?}，回退平台进程内 Agent（M1a）"
            );
        });
    }
    enabled_with(in_sandbox, mode)
}

/// 纯函数版本（便于单测）
#[must_use]
pub fn enabled_with(in_sandbox: bool, mode: SandboxMode) -> bool {
    in_sandbox && matches!(mode, SandboxMode::OpenShell)
}

fn env_string(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_u32(key: &str, default: u32) -> u32 {
    env_string(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_bool(key: &str, default: bool) -> bool {
    match env_string(key).map(|value| value.to_ascii_lowercase()) {
        Some(value) => matches!(value.as_str(), "true" | "1" | "yes" | "on"),
        None => default,
    }
}

impl SandboxAgentRunner {
    /// 执行 Agent（与 [`crate::AgentGraphRunner::run`] 同语义）
    ///
    /// 返回：
    /// - `Completed`：最后一步 `kind=final`；或达到步数上限（**优雅停止**，不再触发重试）
    /// - `Interrupted`：`kind=interrupt`（approval 在下一次调用经 `approval` 传回）
    /// - `Failed`：`kind=error` / IO / 协议错误（含非法中断）
    pub async fn run(&self, invocation: AgentInvocation) -> AgentOutcome {
        if self.llm_base_url.trim().is_empty() {
            return AgentOutcome::Failed(
                "沙箱 Agent 未配置 SANDBOX_AGENT_LLM_BASE_URL（平台内部 LLM 端点）".to_string(),
            );
        }

        let key = SessionKey::new(
            database::current_schema_name(),
            invocation.user_id,
            invocation.sandbox_scope.clone(),
        );
        // 有效步数上限：节点配置（clamp 1..=30）与全局预算取小
        let limit = self.step_limit(&invocation.config);
        let (mut observations, resume_step) = resume_state(&invocation.approval);
        // 恢复步可能因配置收紧而越界，clamp 到 `0..=limit`
        let resume_step = resume_step.min(limit);
        let mut approval = invocation.approval.clone();
        let mut tokens = TokenUsage::default();

        // 建箱前登记沙箱环境（`AGENT_TOKEN`）：只影响尚未创建的会话
        if let Err(error) = self.io.prepare(&key).await {
            return AgentOutcome::Failed(format!("准备沙箱环境失败：{error}"));
        }

        if resume_step >= limit {
            return self
                .stop_at_limit(&invocation, limit, resume_step, observations, tokens)
                .await;
        }

        for step in resume_step..limit {
            let started = Instant::now();
            let input = self.build_step(&invocation, step, observations.clone(), approval.take());

            let payload = match serde_json::to_vec_pretty(&input) {
                Ok(payload) => payload,
                Err(error) => {
                    return AgentOutcome::Failed(format!("step.json 序列化失败：{error}"));
                }
            };

            if let Err(error) = self.io.write_file(&key, STEP_INPUT_PATH, payload).await {
                return AgentOutcome::Failed(format!("写入 step 输入失败：{error}"));
            }

            let argv = vec![
                "step".to_string(),
                "--in".to_string(),
                STEP_INPUT_PATH.to_string(),
                "--out".to_string(),
                STEP_OUTPUT_PATH.to_string(),
            ];
            if let Err(error) = self
                .io
                .exec(
                    &key,
                    RUNNER_BIN.to_string(),
                    argv,
                    Some("/workspace".to_string()),
                    Some(self.step_timeout_secs.saturating_add(10)),
                )
                .await
            {
                return AgentOutcome::Failed(format!("沙箱内 agent-runner 执行失败：{error}"));
            }

            let raw = match self.io.read_file(&key, STEP_OUTPUT_PATH).await {
                Ok(raw) => raw,
                Err(error) => {
                    return AgentOutcome::Failed(format!("读取 step 输出失败：{error}"));
                }
            };
            let output: StepOutput = match serde_json::from_slice(&raw) {
                Ok(output) => output,
                Err(error) => {
                    return AgentOutcome::Failed(format!("step 输出非法：{error}"));
                }
            };

            // token 用量累计 + 单步审计（不含 arguments / LLM 原文）
            let usage = output.usage;
            tokens.add(usage);
            if self.audit_steps {
                let mut event = self.event("agent_step", &invocation);
                event.step = Some(step);
                event.status = Some(step_status(output.kind).to_string());
                event.tool = output
                    .action
                    .as_ref()
                    .and_then(|action| action.get("tool"))
                    .or_else(|| {
                        output
                            .interrupt
                            .as_ref()
                            .and_then(|value| value.get("tool"))
                    })
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                event.duration_ms = Some(elapsed_ms(started));
                event.tokens = usage_of(usage);
                self.io.audit(event.build()).await;
            }

            match output.kind {
                StepKind::Final => {
                    let answer = output.final_output.unwrap_or_else(|| json!({"answer": ""}));
                    return AgentOutcome::Completed {
                        output: answer,
                        state: json!({
                            "sandbox": true,
                            "steps": step + 1,
                            "limit": limit,
                            "tokens": tokens.as_json(),
                            "observations": observations,
                        }),
                    };
                }
                StepKind::Tool => {
                    let action = output.action.unwrap_or_else(|| json!({}));
                    let tool = action
                        .get("tool")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if tool.is_empty() {
                        return AgentOutcome::Failed(
                            "沙箱 Agent 返回 tool 但缺少工具名".to_string(),
                        );
                    }
                    let arguments = action
                        .get("arguments")
                        .cloned()
                        .filter(|value| value.is_object())
                        .unwrap_or_else(|| json!({}));

                    let observation = match output.observation {
                        // 本地工具已在沙箱内执行
                        Some(observation) => observation,
                        // 平台工具：由平台执行（RAG 依赖 DB / 向量库）
                        None => match execute_platform_tool(&tool, &arguments).await {
                            Ok(value) => value,
                            Err(error) => json!({"tool": tool, "error": error}),
                        },
                    };
                    observations.push(json!({
                        "tool": tool,
                        "arguments": arguments,
                        "result": observation,
                    }));
                }
                StepKind::Interrupt => {
                    return match interrupt_outcome(
                        &invocation,
                        step,
                        step + 1,
                        observations,
                        output.interrupt.unwrap_or(Value::Null),
                    ) {
                        Ok(outcome) => outcome,
                        Err(error) => AgentOutcome::Failed(error),
                    };
                }
                StepKind::Error => {
                    return AgentOutcome::Failed(
                        output
                            .error
                            .unwrap_or_else(|| "沙箱 Agent 执行失败".to_string()),
                    );
                }
            }
        }

        self.stop_at_limit(&invocation, limit, limit, observations, tokens)
            .await
    }
}

/// 累计 token 用量（`StepUsage` 是每步增量）
#[derive(Debug, Clone, Copy, Default)]
struct TokenUsage {
    input: i64,
    output: i64,
}

impl TokenUsage {
    fn add(&mut self, usage: Option<StepUsage>) {
        if let Some(usage) = usage {
            self.input += i64::from(usage.input_tokens.max(0));
            self.output += i64::from(usage.output_tokens.max(0));
        }
    }

    fn as_json(self) -> Value {
        json!({"input": self.input, "output": self.output})
    }
}

fn usage_of(usage: Option<StepUsage>) -> Option<(i64, i64)> {
    usage.map(|usage| {
        (
            i64::from(usage.input_tokens.max(0)),
            i64::from(usage.output_tokens.max(0)),
        )
    })
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

const fn step_status(kind: StepKind) -> &'static str {
    match kind {
        StepKind::Final => "final",
        StepKind::Tool => "tool",
        StepKind::Interrupt => "interrupt",
        StepKind::Error => "error",
    }
}

/// 按 §11 契约构造中断结果
///
/// 顶层 `kind=agent_approval_required`，细节（`kind` / `tool` / `answer` /
/// `observations` / `step`）全部在 `data` 内；平台 `agent_interrupt` 原样保存该结构。
fn interrupt_outcome(
    invocation: &AgentInvocation,
    step: u32,
    steps_used: u32,
    observations: Vec<Value>,
    interrupt: Value,
) -> Result<AgentOutcome, String> {
    let Value::Object(mut data) = interrupt else {
        return Err(format!(
            "沙箱 Agent 返回非法中断：interrupt 必须是对象，实际 {interrupt}"
        ));
    };
    let kind = data
        .get("kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if kind.is_empty() {
        return Err("沙箱 Agent 返回非法中断：interrupt 缺少 kind".to_string());
    }

    data.insert("observations".to_string(), Value::Array(observations));
    data.insert("step".to_string(), json!(step));
    data.insert("steps_used".to_string(), json!(steps_used));

    let checkpoint_id = format!("sandbox:{}:{}", invocation.thread_id, step);
    let payload = json!({
        "kind": "agent_approval_required",
        "message": "沙箱 Agent 等待人工审批",
        "data": Value::Object(data),
        "thread_id": invocation.thread_id.clone(),
        "checkpoint_id": checkpoint_id.clone(),
        "node_id": invocation.node_id.clone(),
        "step": step,
    });
    Ok(AgentOutcome::Interrupted {
        thread_id: invocation.thread_id.clone(),
        checkpoint_id,
        payload,
    })
}

impl SandboxAgentRunner {
    /// 有效步数上限 = 节点 `max_iterations`（已 clamp 1..=30）与全局预算取小
    fn step_limit(&self, config: &AgentConfig) -> u32 {
        config.max_iterations().min(self.max_steps).max(1)
    }

    /// 审计事件骨架（scope / user / node 对齐平台侧）
    fn event(&self, kind: &str, invocation: &AgentInvocation) -> AgentEvent {
        let mut event = AgentEvent::new(kind, invocation.sandbox_scope.clone());
        event.runner = Some("sandbox".to_string());
        event.user_id = (invocation.user_id > 0).then_some(invocation.user_id);
        event.node = Some(invocation.node_id.clone());
        event
    }

    /// 达到步数上限：**优雅结束**（`Completed`），避免 `Failed` 触发平台重试 / 反复建箱
    async fn stop_at_limit(
        &self,
        invocation: &AgentInvocation,
        limit: u32,
        steps_used: u32,
        observations: Vec<Value>,
        tokens: TokenUsage,
    ) -> AgentOutcome {
        let mut event = self.event("agent_limit", invocation);
        event.step = Some(steps_used);
        event.status = Some("exceeded".to_string());
        event.limit = Some(limit);
        self.io.audit(event.build()).await;

        AgentOutcome::Completed {
            output: json!({
                "answer": format!("Agent 达到最大步数 {limit}，已停止。"),
                "steps": steps_used,
                "limit": limit,
            }),
            state: json!({
                "sandbox": true,
                "steps": steps_used,
                "limit": limit,
                "tokens": tokens.as_json(),
                "observations": observations,
            }),
        }
    }

    fn build_step(
        &self,
        invocation: &AgentInvocation,
        step: u32,
        observations: Vec<Value>,
        approval: Option<Value>,
    ) -> StepInput {
        let config: &AgentConfig = &invocation.config;
        StepInput {
            protocol: PROTOCOL.to_string(),
            thread_id: invocation.thread_id.clone(),
            node_id: invocation.node_id.clone(),
            step,
            agent: AgentStep {
                prompt: config.prompt.clone(),
                model: config.model.clone(),
                temperature: config.temperature,
                max_tokens: config.max_tokens,
                tools: config.tool_names(),
                interrupt_policy: Some(config.interrupt_policy().to_string()),
                max_iterations: Some(config.max_iterations()),
            },
            input: invocation.input.clone(),
            observations,
            approval,
            llm: LlmStep {
                base_url: self.llm_base_url.clone(),
                token_env: self.token_env.clone(),
                default_model: self.default_model.clone(),
                timeout_secs: u64::from(self.step_timeout_secs),
            },
            workspace: "/workspace".to_string(),
            limits: StepLimits {
                exec_timeout_secs: self.exec_timeout_secs,
                ..StepLimits::default()
            },
        }
    }
}

/// 审批恢复：从上一次中断的 `data` 里取回 observations / step
///
/// 平台 `process_service_task` 会把 `agent_interrupt.data` 原样作为 `approval` 传回，
/// 因此这里同时兼容顶层与 `data` 嵌套（与 `approval_field` 一致）。
pub(crate) fn resume_state(approval: &Option<Value>) -> (Vec<Value>, u32) {
    let Some(approval) = approval.as_ref() else {
        return (Vec::new(), 0);
    };
    let source = approval
        .get("data")
        .filter(|value| value.is_object())
        .unwrap_or(approval);
    let observations = source
        .get("observations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let step = source
        .get("step")
        .and_then(Value::as_u64)
        .map(|value| u32::try_from(value).unwrap_or(0))
        .unwrap_or(0);
    (observations, step)
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;

    use agent_core::protocol::{StepOutput, StepUsage};
    use sandbox::ExecResult;

    use super::*;

    /// 假 IO：`exec` 从预置队列弹出一条 `StepOutput` 写入 out/step.json
    struct FakeIo {
        outputs: Mutex<VecDeque<Value>>,
        files: Mutex<HashMap<String, Vec<u8>>>,
        execs: Mutex<Vec<Value>>,
        inputs: Mutex<Vec<Value>>,
        audits: Mutex<Vec<Value>>,
    }

    impl FakeIo {
        fn new(outputs: Vec<StepOutput>) -> Arc<Self> {
            Arc::new(Self {
                outputs: Mutex::new(
                    outputs
                        .into_iter()
                        .map(|output| serde_json::to_value(output).expect("step 输出应可序列化"))
                        .collect(),
                ),
                files: Mutex::new(HashMap::new()),
                execs: Mutex::new(Vec::new()),
                inputs: Mutex::new(Vec::new()),
                audits: Mutex::new(Vec::new()),
            })
        }

        fn exec_count(&self) -> usize {
            self.execs.lock().unwrap().len()
        }

        fn last_exec(&self) -> Value {
            self.execs
                .lock()
                .unwrap()
                .last()
                .cloned()
                .unwrap_or(Value::Null)
        }

        fn written_input(&self) -> Value {
            self.inputs
                .lock()
                .unwrap()
                .last()
                .cloned()
                .unwrap_or(Value::Null)
        }

        fn all_inputs(&self) -> Vec<Value> {
            self.inputs.lock().unwrap().clone()
        }

        fn audits(&self) -> Vec<Value> {
            self.audits.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl SandboxIo for FakeIo {
        async fn write_file(
            &self,
            _key: &SessionKey,
            path: &str,
            data: Vec<u8>,
        ) -> Result<(), String> {
            if path == STEP_INPUT_PATH {
                let value: Value =
                    serde_json::from_slice(&data).map_err(|error| error.to_string())?;
                self.inputs.lock().unwrap().push(value);
            }
            self.files.lock().unwrap().insert(path.to_string(), data);
            Ok(())
        }

        async fn read_file(&self, _key: &SessionKey, path: &str) -> Result<Vec<u8>, String> {
            self.files
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| format!("文件不存在: {path}"))
        }

        async fn exec(
            &self,
            _key: &SessionKey,
            command: String,
            argv: Vec<String>,
            cwd: Option<String>,
            timeout_secs: Option<u32>,
        ) -> Result<ExecResult, String> {
            self.execs.lock().unwrap().push(json!({
                "command": command,
                "argv": argv,
                "cwd": cwd,
                "timeout": timeout_secs,
            }));
            let next = self
                .outputs
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "没有预置的 step 输出".to_string())?;
            let bytes = serde_json::to_vec(&next).map_err(|error| error.to_string())?;
            self.files
                .lock()
                .unwrap()
                .insert(STEP_OUTPUT_PATH.to_string(), bytes);
            Ok(ExecResult {
                exit_code: 0,
                text: None,
                stdout: Vec::new(),
                stderr: Vec::new(),
                error: None,
                duration_ms: 1,
            })
        }

        async fn audit(&self, event: Value) {
            self.audits.lock().unwrap().push(event);
        }
    }

    /// 写入必失败的 IO
    struct WriteFailIo;

    #[async_trait::async_trait]
    impl SandboxIo for WriteFailIo {
        async fn write_file(
            &self,
            _key: &SessionKey,
            _path: &str,
            _data: Vec<u8>,
        ) -> Result<(), String> {
            Err("disk full".to_string())
        }

        async fn read_file(&self, _key: &SessionKey, _path: &str) -> Result<Vec<u8>, String> {
            Err("unused".to_string())
        }

        async fn exec(
            &self,
            _key: &SessionKey,
            _command: String,
            _argv: Vec<String>,
            _cwd: Option<String>,
            _timeout_secs: Option<u32>,
        ) -> Result<ExecResult, String> {
            Err("unused".to_string())
        }
    }

    /// 准备阶段必失败的 IO（验证建箱前护栏）
    struct PrepareFailIo;

    #[async_trait::async_trait]
    impl SandboxIo for PrepareFailIo {
        async fn prepare(&self, _key: &SessionKey) -> Result<(), String> {
            Err("token 注入失败".to_string())
        }

        async fn write_file(
            &self,
            _key: &SessionKey,
            _path: &str,
            _data: Vec<u8>,
        ) -> Result<(), String> {
            Err("unused".to_string())
        }

        async fn read_file(&self, _key: &SessionKey, _path: &str) -> Result<Vec<u8>, String> {
            Err("unused".to_string())
        }

        async fn exec(
            &self,
            _key: &SessionKey,
            _command: String,
            _argv: Vec<String>,
            _cwd: Option<String>,
            _timeout_secs: Option<u32>,
        ) -> Result<ExecResult, String> {
            Err("unused".to_string())
        }
    }

    fn invocation() -> AgentInvocation {
        AgentInvocation {
            thread_id: "exec-1:agent_1".to_string(),
            node_id: "agent_1".to_string(),
            config: AgentConfig {
                prompt: "整理资产".to_string(),
                tools: Some("knowledge_search,current_time".to_string()),
                ..AgentConfig::default()
            },
            input: json!({"asset_id": "1"}),
            approval: None,
            user_id: 7,
            sandbox_scope: "wf:exec-1".to_string(),
        }
    }

    #[test]
    fn enabled_only_for_openshell_with_flag() {
        assert!(enabled_with(true, SandboxMode::OpenShell));
        assert!(!enabled_with(false, SandboxMode::OpenShell));
        assert!(!enabled_with(true, SandboxMode::Cube));
        assert!(!enabled_with(true, SandboxMode::InProcess));
    }

    #[test]
    fn resume_state_reads_flat_and_nested_payloads() {
        let (observations, step) = resume_state(&None);
        assert!(observations.is_empty());
        assert_eq!(step, 0);

        let flat = Some(json!({"observations": [{"tool": "x"}], "step": 3}));
        assert_eq!(resume_state(&flat).1, 3);
        assert_eq!(resume_state(&flat).0.len(), 1);

        let nested = Some(json!({"data": {"observations": [1, 2], "step": 4}}));
        assert_eq!(resume_state(&nested).1, 4);
        assert_eq!(resume_state(&nested).0.len(), 2);
    }

    #[tokio::test]
    async fn final_step_completes_with_output() {
        let io = FakeIo::new(vec![StepOutput::final_answer(json!({"answer": "done"}))]);
        let runner = SandboxAgentRunner::with_io(io.clone(), 30, 3);

        match runner.run(invocation()).await {
            AgentOutcome::Completed { output, state } => {
                assert_eq!(output["answer"], "done");
                assert_eq!(state["sandbox"], true);
                assert_eq!(state["steps"], 1);
            }
            other => panic!("应 Completed，实际 {other:?}"),
        }

        assert_eq!(io.exec_count(), 1);
        let exec = io.last_exec();
        assert_eq!(exec["command"], "agent-runner");
        assert_eq!(exec["cwd"], "/workspace");
        assert_eq!(exec["argv"][1], "--in");

        let input = io.written_input();
        assert_eq!(input["protocol"], PROTOCOL);
        assert_eq!(input["thread_id"], "exec-1:agent_1");
        assert_eq!(input["step"], 0);
        assert_eq!(input["workspace"], "/workspace");
        assert_eq!(input["agent"]["prompt"], "整理资产");
        assert!(
            input["llm"]["base_url"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
    }

    #[tokio::test]
    async fn platform_tool_is_executed_by_platform_and_fed_back() {
        let io = FakeIo::new(vec![
            StepOutput::tool(json!({"tool": "current_time", "arguments": {}}), None),
            StepOutput::final_answer(json!({"answer": "ok"})),
        ]);
        let runner = SandboxAgentRunner::with_io(io.clone(), 30, 3);

        match runner.run(invocation()).await {
            AgentOutcome::Completed { state, .. } => {
                let observations = state["observations"].as_array().expect("observations");
                assert_eq!(observations.len(), 1);
                assert_eq!(observations[0]["tool"], "current_time");
                assert!(observations[0]["result"]["now"].is_string());
            }
            other => panic!("应 Completed，实际 {other:?}"),
        }

        assert_eq!(io.exec_count(), 2);
        let second = io.written_input();
        assert_eq!(second["step"], 1);
        assert_eq!(second["observations"][0]["tool"], "current_time");
    }

    #[tokio::test]
    async fn sandbox_local_tool_observation_passes_through() {
        let io = FakeIo::new(vec![
            StepOutput::tool(
                json!({"tool": "sandbox_exec", "arguments": {"command": "ls"}}),
                Some(json!({"exit_code": 0, "stdout": ["a.txt"]})),
            ),
            StepOutput::final_answer(json!({"answer": "ok"})),
        ]);
        let runner = SandboxAgentRunner::with_io(io.clone(), 30, 3);

        match runner.run(invocation()).await {
            AgentOutcome::Completed { state, .. } => {
                assert_eq!(state["observations"][0]["result"]["exit_code"], 0);
            }
            other => panic!("应 Completed，实际 {other:?}"),
        }
        assert_eq!(io.exec_count(), 2);
    }

    #[tokio::test]
    async fn interrupt_maps_to_agent_outcome_with_resume_state() {
        let io = FakeIo::new(vec![StepOutput::interrupt(json!({
            "kind": "before_tool",
            "tool": "sandbox_exec",
            "arguments": {"command": "rm -rf build"},
        }))]);
        let runner = SandboxAgentRunner::with_io(io, 30, 3);

        match runner.run(invocation()).await {
            AgentOutcome::Interrupted {
                thread_id,
                checkpoint_id,
                payload,
            } => {
                assert_eq!(thread_id, "exec-1:agent_1");
                assert!(checkpoint_id.starts_with("sandbox:exec-1:agent_1:"));
                assert_eq!(payload["kind"], "agent_approval_required");
                assert_eq!(payload["data"]["tool"], "sandbox_exec");
                assert_eq!(payload["data"]["step"], 0);
                assert!(payload["data"]["observations"].is_array());
            }
            other => panic!("应 Interrupted，实际 {other:?}"),
        }
    }

    #[tokio::test]
    async fn resume_from_approval_replays_observations_and_step() {
        let io = FakeIo::new(vec![
            StepOutput::tool(
                json!({"tool": "sandbox_exec", "arguments": {"command": "ls"}}),
                Some(json!({"exit_code": 0})),
            ),
            StepOutput::final_answer(json!({"answer": "ok"})),
        ]);
        let runner = SandboxAgentRunner::with_io(io.clone(), 30, 5);

        let mut inv = invocation();
        inv.approval = Some(json!({
            "tool": "sandbox_exec",
            "arguments": {"command": "ls"},
            "approved": true,
            "comment": "同意",
            "observations": [{"tool": "current_time", "result": {"now": "t"}}],
            "step": 2,
        }));

        match runner.run(inv).await {
            AgentOutcome::Completed { state, .. } => {
                assert_eq!(state["steps"], 4);
                let observations = state["observations"].as_array().expect("observations");
                assert_eq!(observations.len(), 2, "恢复的观察 + 本次工具结果");
            }
            other => panic!("应 Completed，实际 {other:?}"),
        }

        let inputs = io.all_inputs();
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0]["step"], 2);
        assert_eq!(inputs[0]["approval"]["approved"], true);
        assert_eq!(inputs[0]["observations"].as_array().map(Vec::len), Some(1));
        assert!(inputs[1]["approval"].is_null());
    }

    #[tokio::test]
    async fn error_step_fails() {
        let io = FakeIo::new(vec![StepOutput::error("boom")]);
        let runner = SandboxAgentRunner::with_io(io, 30, 3);

        match runner.run(invocation()).await {
            AgentOutcome::Failed(error) => assert!(error.contains("boom")),
            other => panic!("应 Failed，实际 {other:?}"),
        }
    }

    #[tokio::test]
    async fn max_steps_stops_gracefully_instead_of_failing() {
        let io = FakeIo::new(vec![
            StepOutput::tool(
                json!({"tool": "sandbox_exec", "arguments": {}}),
                Some(json!({"exit_code": 0})),
            ),
            StepOutput::tool(
                json!({"tool": "sandbox_exec", "arguments": {}}),
                Some(json!({"exit_code": 0})),
            ),
        ]);
        let runner = SandboxAgentRunner::with_io(io.clone(), 30, 2);

        match runner.run(invocation()).await {
            AgentOutcome::Completed { output, state } => {
                assert_eq!(output["limit"], 2);
                assert_eq!(output["steps"], 2);
                assert!(
                    output["answer"]
                        .as_str()
                        .is_some_and(|value| value.contains("最大步数")),
                    "{output}"
                );
                assert_eq!(state["steps"], 2);
            }
            other => panic!("超步数应优雅 Completed，实际 {other:?}"),
        }
        assert_eq!(io.exec_count(), 2, "到上限后不再执行第 3 步");
    }

    #[tokio::test]
    async fn node_max_iterations_caps_global_budget() {
        let io = FakeIo::new(vec![
            StepOutput::tool(json!({"tool": "sandbox_exec", "arguments": {}}), None),
            StepOutput::tool(json!({"tool": "sandbox_exec", "arguments": {}}), None),
            StepOutput::tool(json!({"tool": "sandbox_exec", "arguments": {}}), None),
        ]);
        let runner = SandboxAgentRunner::with_io(io.clone(), 30, 6);

        let mut inv = invocation();
        inv.config.max_iterations = Some(2);

        match runner.run(inv).await {
            AgentOutcome::Completed { output, .. } => {
                assert_eq!(output["limit"], 2);
                assert_eq!(output["steps"], 2);
            }
            other => panic!("应 Completed，实际 {other:?}"),
        }
        assert_eq!(io.exec_count(), 2);
    }

    #[tokio::test]
    async fn resume_step_beyond_limit_stops_without_exec() {
        let io = FakeIo::new(vec![]);
        let runner = SandboxAgentRunner::with_io(io.clone(), 30, 3);

        let mut inv = invocation();
        inv.approval = Some(json!({"data": {"observations": [], "step": 9}}));

        match runner.run(inv).await {
            AgentOutcome::Completed { output, .. } => {
                assert_eq!(output["limit"], 3);
                assert_eq!(output["steps"], 3, "越界恢复步被 clamp 到上限");
            }
            other => panic!("应 Completed，实际 {other:?}"),
        }
        assert_eq!(io.exec_count(), 0);
    }

    #[tokio::test]
    async fn token_usage_accumulates_across_steps() {
        let io = FakeIo::new(vec![
            StepOutput::tool(json!({"tool": "current_time", "arguments": {}}), None).with_usage(
                StepUsage {
                    input_tokens: 10,
                    output_tokens: 4,
                },
            ),
            StepOutput::final_answer(json!({"answer": "ok"})).with_usage(StepUsage {
                input_tokens: 6,
                output_tokens: 2,
            }),
        ]);
        let runner = SandboxAgentRunner::with_io(io.clone(), 30, 3);

        match runner.run(invocation()).await {
            AgentOutcome::Completed { state, .. } => {
                assert_eq!(state["tokens"]["input"], 16);
                assert_eq!(state["tokens"]["output"], 6);
            }
            other => panic!("应 Completed，实际 {other:?}"),
        }

        let audits = io.audits();
        let steps: Vec<&Value> = audits
            .iter()
            .filter(|event| event["unmapped"]["event"] == "agent_step")
            .collect();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0]["unmapped"]["tokens"]["input"], 10);
        assert_eq!(steps[1]["unmapped"]["tokens"]["output"], 2);
    }

    #[tokio::test]
    async fn audit_records_step_without_arguments() {
        let io = FakeIo::new(vec![
            StepOutput::tool(
                json!({"tool": "sandbox_exec", "arguments": {"command": "rm -rf build"}}),
                Some(json!({"exit_code": 0})),
            ),
            StepOutput::final_answer(json!({"answer": "ok"})),
        ]);
        let runner = SandboxAgentRunner::with_io(io.clone(), 30, 4);

        let outcome = runner.run(invocation()).await;
        assert!(matches!(outcome, AgentOutcome::Completed { .. }));

        let audits = io.audits();
        assert_eq!(audits.len(), 2, "两步各一条 agent_step");

        let step = &audits[0];
        assert_eq!(step["unmapped"]["event"], "agent_step");
        assert_eq!(step["unmapped"]["runner"], "sandbox");
        assert_eq!(step["unmapped"]["node"], "agent_1");
        assert_eq!(step["unmapped"]["step"], 0);
        assert_eq!(step["unmapped"]["tool"], "sandbox_exec");
        assert_eq!(step["labels"]["session_id"], "wf:exec-1");
        assert_eq!(step["labels"]["user_id"], 7);
        assert!(
            !step.to_string().contains("rm -rf build"),
            "审计不得包含 tool arguments"
        );
        assert_eq!(audits[1]["status"], "final");
    }

    #[tokio::test]
    async fn limit_audit_event_is_recorded() {
        let io = FakeIo::new(vec![StepOutput::tool(
            json!({"tool": "sandbox_exec", "arguments": {}}),
            None,
        )]);
        let runner = SandboxAgentRunner::with_io(io.clone(), 30, 1);

        assert!(matches!(
            runner.run(invocation()).await,
            AgentOutcome::Completed { .. }
        ));

        let audits = io.audits();
        let limit = audits
            .iter()
            .find(|event| event["unmapped"]["event"] == "agent_limit")
            .expect("应记录 agent_limit");
        assert_eq!(limit["unmapped"]["limit"], 1);
        assert_eq!(limit["unmapped"]["step"], 1);
        assert_eq!(limit["status"], "exceeded");
    }

    #[tokio::test]
    async fn interrupt_without_kind_fails_instead_of_silently_resuming() {
        let io = FakeIo::new(vec![StepOutput::interrupt(json!({"tool": "sandbox_exec"}))]);
        let runner = SandboxAgentRunner::with_io(io, 30, 3);

        match runner.run(invocation()).await {
            AgentOutcome::Failed(error) => {
                assert!(error.contains("非法中断"), "{error}");
                assert!(error.contains("kind"), "{error}");
            }
            other => panic!("应 Failed，实际 {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_object_interrupt_fails() {
        let io = FakeIo::new(vec![StepOutput::interrupt(json!("before_tool"))]);
        let runner = SandboxAgentRunner::with_io(io, 30, 3);

        match runner.run(invocation()).await {
            AgentOutcome::Failed(error) => assert!(error.contains("必须是对象"), "{error}"),
            other => panic!("应 Failed，实际 {other:?}"),
        }
    }

    #[tokio::test]
    async fn write_failure_surfaces_actionable_error() {
        let runner = SandboxAgentRunner::with_io(Arc::new(WriteFailIo), 30, 3);

        match runner.run(invocation()).await {
            AgentOutcome::Failed(error) => assert!(error.contains("写入 step 输入失败")),
            other => panic!("应 Failed，实际 {other:?}"),
        }
    }

    #[tokio::test]
    async fn prepare_failure_stops_before_any_step() {
        let runner = SandboxAgentRunner::with_io(Arc::new(PrepareFailIo), 30, 3);

        match runner.run(invocation()).await {
            AgentOutcome::Failed(error) => {
                assert!(error.contains("准备沙箱环境失败"), "{error}");
                assert!(error.contains("token 注入失败"), "{error}");
            }
            other => panic!("应 Failed，实际 {other:?}"),
        }
    }

    #[tokio::test]
    async fn service_io_registers_agent_token_for_session() {
        let key = SessionKey::new("__sandbox_agent_test__", 41, "wf:token");
        let io = ServiceSandboxIo::new("AGENT_TOKEN".to_string(), Some("tok-123".to_string()));
        io.prepare(&key).await.expect("prepare 应成功");

        let env = sandbox_service::session_env(&key);
        assert_eq!(env.get("AGENT_TOKEN").map(String::as_str), Some("tok-123"));

        // 空 token 不登记（避免把空串注入沙箱）
        let empty = SessionKey::new("__sandbox_agent_test__", 42, "wf:token");
        ServiceSandboxIo::new("AGENT_TOKEN".to_string(), None)
            .prepare(&empty)
            .await
            .expect("prepare 应成功");
        assert!(sandbox_service::session_env(&empty).is_empty());
    }
}
