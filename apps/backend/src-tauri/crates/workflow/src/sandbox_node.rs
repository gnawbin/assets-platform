//! 沙箱执行节点（BPMN ServiceTask / 手工编排共用）
//!
//! 只依赖 `crates/sandbox` 契约：运行时（CubeSandbox / 未来其它实现）由调用方注入，
//! 因此本模块不需要 `reqwest`，也不会把 Cube API 细节带进工作流引擎。
//!
//! 用法（在 ServiceTask worker 内）：
//! ```rust,ignore
//! let config: SandboxNodeConfig = serde_json::from_value(node.config.clone())?;
//! let output = sandbox_node::execute(runtime.as_ref(), &sandbox_id, &config).await?;
//! ```

use sandbox::{ExecRequest, SandboxRuntime};
use serde::{Deserialize, Serialize};

/// 节点配置：`{"kind":"sandbox","code":"...", "timeout_secs": 60}` 或
/// `{"kind":"sandbox","command":"ls","argv":["-l"]}`
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SandboxNodeConfig {
    /// 代码片段（缺省解释器 python3）
    #[serde(default)]
    pub code: Option<String>,
    /// shell 命令（无 `argv` 时按 `/bin/sh -lc` 执行）
    #[serde(default)]
    pub command: Option<String>,
    /// 命令参数（给出时按可执行文件 + 参数处理）
    #[serde(default)]
    pub argv: Vec<String>,
    /// 工作目录
    #[serde(default)]
    pub cwd: Option<String>,
    /// 单次执行超时（秒）
    #[serde(default)]
    pub timeout_secs: Option<u32>,
}

impl SandboxNodeConfig {
    /// 从节点 `config` JSON 解析（`kind` 等无关字段会被忽略）
    ///
    /// # 错误
    /// JSON 结构不符时返回可展示的错误信息。
    pub fn from_value(value: &serde_json::Value) -> Result<Self, String> {
        serde_json::from_value(value.clone())
            .map_err(|error| format!("沙箱节点配置非法：{error}"))
    }

    /// 是否至少指定了 `code` 或 `command`
    #[must_use]
    pub fn is_executable(&self) -> bool {
        self.code.as_ref().is_some_and(|code| !code.trim().is_empty())
            || self
                .command
                .as_ref()
                .is_some_and(|command| !command.trim().is_empty())
    }

    /// 翻译成契约请求
    ///
    /// # 错误
    /// `code` 与 `command` 都为空时返回错误。
    pub fn to_exec_request(&self) -> Result<ExecRequest, String> {
        let mut request = if let Some(code) = self
            .code
            .as_ref()
            .filter(|code| !code.trim().is_empty())
        {
            ExecRequest::code(code.clone())
        } else if let Some(command) = self
            .command
            .as_ref()
            .filter(|command| !command.trim().is_empty())
        {
            ExecRequest::command(command.clone())
        } else {
            return Err("沙箱节点缺少 code / command".to_string());
        };

        request.argv.clone_from(&self.argv);
        request.cwd.clone_from(&self.cwd);
        request.timeout_secs = self.timeout_secs;
        Ok(request)
    }
}

/// 节点输出（可直接写入流程变量）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxNodeOutput {
    /// 退出码
    pub exit_code: i32,
    /// 主输出文本
    pub text: String,
    /// stderr 行
    pub stderr: Vec<String>,
    /// 耗时（毫秒）
    pub duration_ms: u64,
}

/// 在指定沙箱上执行节点
///
/// # 错误
/// 配置非法、沙箱不可用或执行报错时返回可展示的错误信息。
pub async fn execute(
    runtime: &dyn SandboxRuntime,
    sandbox_id: &str,
    config: &SandboxNodeConfig,
) -> Result<SandboxNodeOutput, String> {
    let request = config.to_exec_request()?;
    let result = runtime
        .exec(sandbox_id, request)
        .await
        .map_err(|error| format!("{}（{}）", error.hint(), error))?;

    Ok(SandboxNodeOutput {
        exit_code: result.exit_code,
        text: result.combined_output(),
        stderr: result.stderr,
        duration_ms: result.duration_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_code_and_command_configs() {
        let code = SandboxNodeConfig::from_value(&serde_json::json!({
            "kind": "sandbox",
            "code": "print(1)",
            "timeout_secs": 30
        }))
        .expect("代码配置应可解析");
        assert_eq!(code.code.as_deref(), Some("print(1)"));
        assert_eq!(code.timeout_secs, Some(30));
        assert!(code.is_executable());

        let command = SandboxNodeConfig::from_value(&serde_json::json!({
            "command": "ls",
            "argv": ["-l", "/tmp"]
        }))
        .expect("命令配置应可解析");
        assert_eq!(command.argv, vec!["-l".to_string(), "/tmp".to_string()]);
    }

    #[test]
    fn rejects_empty_config_and_bad_json() {
        let empty = SandboxNodeConfig::default();
        assert!(!empty.is_executable());
        assert!(empty.to_exec_request().is_err());

        let error = SandboxNodeConfig::from_value(&serde_json::json!({"argv": "not-an-array"}))
            .expect_err("类型不符必须报错");
        assert!(error.contains("沙箱节点配置非法"), "{error}");
    }

    #[test]
    fn to_exec_request_prefers_code_and_carries_limits() {
        let config = SandboxNodeConfig {
            code: Some("print(1)".to_string()),
            command: Some("ls".to_string()),
            cwd: Some("/work".to_string()),
            timeout_secs: Some(15),
            argv: vec!["-l".to_string()],
        };
        let request = config.to_exec_request().expect("应可转换");
        assert!(request.is_code());
        // `code` 优先：`command` 被忽略（不会同时下发两种执行方式）
        assert!(request.command.is_none());
        assert_eq!(request.cwd.as_deref(), Some("/work"));
        assert_eq!(request.timeout_secs, Some(15));
        assert_eq!(request.argv, vec!["-l".to_string()]);
    }
}
