//! 契约类型 ↔ OpenShell proto 类型
//!
//! 约定：
//! 1. `SandboxSpec.policy_yaml` 在 proto 的 `SandboxSpec.policy` 里是结构化字段（YAML→proto 不做一次性映射）
//!    → 显式 `Unsupported`，策略放 gateway 侧配置；
//! 2. `image` 映射到 `SandboxTemplate.image`（proto 的 `SandboxSpec` 没有 image 字段）；
//! 3. `labels` 与 `metadata` 都写进 `CreateSandboxRequest.labels`（便于 `ListSandboxes` 统一过滤）。

use std::collections::{BTreeMap, HashMap};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use prost_types::Duration as ProtoDuration;
use sandbox::{
    ExecRequest, SandboxError, SandboxHandle, SandboxResult, SandboxSpec, SandboxState,
    SandboxSummary,
};

use crate::client::OpenShellOptions;
use crate::proto::openshell::datamodel::v1::{WorkspaceSelector, workspace_selector};
use crate::proto::openshell::v1::{
    Sandbox, SandboxPhase, SandboxSpec as ProtoSandboxSpec, SandboxTemplate,
};

/// `workspace_scope` 构造
pub(crate) fn workspace(workspace: &str) -> WorkspaceSelector {
    WorkspaceSelector {
        selection: Some(workspace_selector::Selection::Workspace(
            workspace.to_string(),
        )),
    }
}

/// 契约规格 → proto 规格（labels 由调用方放进 CreateSandboxRequest）
pub(crate) fn to_proto_spec(
    spec: &SandboxSpec,
    options: &OpenShellOptions,
) -> SandboxResult<ProtoSandboxSpec> {
    if spec
        .policy_yaml
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
    {
        return Err(SandboxError::unsupported(
            "OpenShell 创建期策略：proto 的 policy 是结构化字段，YAML 不在此处映射；请把策略放到 gateway 侧配置",
        ));
    }

    let image = spec
        .image
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| options.default_image.clone());

    let command = if spec.command.is_empty() {
        options.default_command.clone()
    } else {
        spec.command.clone()
    };

    Ok(ProtoSandboxSpec {
        environment: spec.env.clone().into_iter().collect(),
        template: image.map(|image| SandboxTemplate {
            image,
            ..SandboxTemplate::default()
        }),
        command,
        ..ProtoSandboxSpec::default()
    })
}

/// proto 阶段 → 契约状态
pub(crate) fn to_state(phase: SandboxPhase) -> SandboxState {
    match phase {
        SandboxPhase::Ready => SandboxState::Running,
        SandboxPhase::Suspended | SandboxPhase::Stopped => SandboxState::Paused,
        SandboxPhase::Suspending | SandboxPhase::Stopping => SandboxState::Pausing,
        _ => SandboxState::Unknown,
    }
}

/// 从 `Sandbox.status.phase` 取契约状态
pub(crate) fn state_of(sandbox: &Sandbox) -> SandboxState {
    sandbox
        .status
        .as_ref()
        .and_then(|status| SandboxPhase::try_from(status.phase).ok())
        .map(to_state)
        .unwrap_or_default()
}

/// proto 沙箱 → 契约句柄（`id` 用 name，后续 RPC 均按 name 寻址）
pub(crate) fn to_handle(sandbox: &Sandbox, options: &OpenShellOptions) -> SandboxHandle {
    let metadata = sandbox.metadata.as_ref();
    let name = metadata.map(|meta| meta.name.clone()).unwrap_or_default();
    SandboxHandle {
        id: if name.is_empty() {
            metadata.map(|meta| meta.id.clone()).unwrap_or_default()
        } else {
            name
        },
        state: state_of(sandbox),
        template_id: None,
        created_at: None,
        data_host_suffix: String::new(),
        envd_port: 0,
        endpoint: Some(options.gateway.clone()),
    }
}

/// proto 沙箱 → 列表项（labels 作为 metadata）
pub(crate) fn to_summary(sandbox: &Sandbox) -> SandboxSummary {
    let metadata = sandbox.metadata.as_ref();
    let name = metadata.map(|meta| meta.name.clone()).unwrap_or_default();
    let labels: BTreeMap<String, String> = metadata
        .map(|meta| {
            meta.labels
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default();
    SandboxSummary {
        id: if name.is_empty() {
            metadata.map(|meta| meta.id.clone()).unwrap_or_default()
        } else {
            name
        },
        state: state_of(sandbox),
        template_id: None,
        metadata: labels,
        started_at: None,
    }
}

/// 契约执行请求 → proto 命令行（`user` 由 gateway 策略决定，不在 proto 里）
pub(crate) fn to_command(
    request: &ExecRequest,
    options: &OpenShellOptions,
) -> SandboxResult<Vec<String>> {
    if let Some(code) = request
        .code
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(vec![
            options.python.clone(),
            "-c".to_string(),
            code.to_string(),
        ]);
    }
    if let Some(command) = request
        .command
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        if request.argv.is_empty() {
            return Ok(vec!["sh".to_string(), "-lc".to_string(), command.to_string()]);
        }
        let mut argv = vec![command.to_string()];
        argv.extend(request.argv.clone());
        return Ok(argv);
    }
    Err(SandboxError::invalid_request(
        400,
        "执行请求缺少 code / command",
    ))
}

/// 秒数 → protobuf Duration
pub(crate) fn proto_duration(secs: u64) -> ProtoDuration {
    ProtoDuration {
        seconds: i64::try_from(secs).unwrap_or(i64::MAX),
        nanos: 0,
    }
}

/// 执行超时（秒）
pub(crate) fn exec_timeout_secs(request: &ExecRequest, options: &OpenShellOptions) -> u64 {
    request
        .timeout_secs
        .map(u64::from)
        .unwrap_or_else(|| options.exec_timeout.as_secs())
        .max(1)
}

/// 执行耗时（毫秒）
pub(crate) fn elapsed_ms(started: std::time::Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// POSIX shell 单引号转义
pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// 解码 `base64` 输出（容忍换行）
pub(crate) fn decode_base64(raw: &[u8]) -> SandboxResult<Vec<u8>> {
    let compact: String = String::from_utf8_lossy(raw)
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect();
    BASE64
        .decode(compact)
        .map_err(|error| SandboxError::decode(format!("base64 解码失败：{error}")))
}

/// 字节输出按行切分
pub(crate) fn split_lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(ToString::to_string)
        .collect()
}

/// `CreateSandboxRequest.labels`：labels 与 metadata 合并
pub(crate) fn to_labels(spec: &SandboxSpec) -> HashMap<String, String> {
    let mut labels: HashMap<String, String> = HashMap::new();
    labels.extend(spec.labels.clone());
    labels.extend(spec.metadata.clone());
    labels
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::OpenShellOptions;
    use sandbox::NetworkPolicy;
    use std::time::Duration;

    fn options() -> OpenShellOptions {
        OpenShellOptions {
            gateway: "http://127.0.0.1:8080".to_string(),
            token: None,
            ca_cert_path: None,
            workspace: "default".to_string(),
            ready_timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(5),
            default_image: Some("registry/agent:1".to_string()),
            default_command: vec!["agent-runner".to_string(), "serve".to_string()],
            python: "python3".to_string(),
            exec_timeout: Duration::from_secs(60),
        }
    }

    #[test]
    fn spec_maps_image_template_command_and_env() {
        let mut spec = SandboxSpec {
            template_id: "ignored".to_string(),
            command: vec!["sh".to_string(), "-lc".to_string(), "ls".to_string()],
            image: Some("registry/custom:2".to_string()),
            ..SandboxSpec::default()
        };
        spec.env.insert("FOO".to_string(), "bar".to_string());

        let proto = to_proto_spec(&spec, &options()).expect("应可转换");
        assert_eq!(
            proto.template.as_ref().map(|template| template.image.as_str()),
            Some("registry/custom:2")
        );
        assert_eq!(proto.command, vec!["sh", "-lc", "ls"]);
        assert_eq!(proto.environment.get("FOO").map(String::as_str), Some("bar"));
    }

    #[test]
    fn empty_image_and_command_fall_back_to_defaults() {
        let proto = to_proto_spec(&SandboxSpec::default(), &options()).expect("应可转换");
        assert_eq!(
            proto.template.as_ref().map(|template| template.image.as_str()),
            Some("registry/agent:1")
        );
        assert_eq!(proto.command, vec!["agent-runner", "serve"]);
    }

    #[test]
    fn policy_yaml_is_rejected_explicitly() {
        let spec = SandboxSpec {
            policy_yaml: Some("network:\n  default: deny\n".to_string()),
            ..SandboxSpec::default()
        };
        let error = to_proto_spec(&spec, &options()).expect_err("策略必须显式拒绝");
        assert_eq!(error.kind(), "unsupported");
    }

    #[test]
    fn labels_merge_spec_labels_and_metadata() {
        let mut spec = SandboxSpec::default();
        spec.labels.insert("team".to_string(), "core".to_string());
        spec.metadata
            .insert("tenant".to_string(), "tenant_a".to_string());

        let labels = to_labels(&spec);
        assert_eq!(labels.get("team").map(String::as_str), Some("core"));
        assert_eq!(labels.get("tenant").map(String::as_str), Some("tenant_a"));
    }

    #[test]
    fn phases_map_to_four_contract_states() {
        assert_eq!(to_state(SandboxPhase::Ready), SandboxState::Running);
        assert_eq!(to_state(SandboxPhase::Suspended), SandboxState::Paused);
        assert_eq!(to_state(SandboxPhase::Suspending), SandboxState::Pausing);
        assert_eq!(to_state(SandboxPhase::Error), SandboxState::Unknown);
        assert_eq!(to_state(SandboxPhase::Provisioning), SandboxState::Unknown);
    }

    #[test]
    fn commands_prefer_code_then_argv_then_shell() {
        let options = options();

        let code = to_command(&ExecRequest::code("print(1)"), &options).expect("code");
        assert_eq!(code[0], "python3");
        assert_eq!(code[2], "print(1)");

        let argv = ExecRequest {
            command: Some("ls".to_string()),
            argv: vec!["-l".to_string()],
            ..ExecRequest::default()
        };
        assert_eq!(
            to_command(&argv, &options).expect("argv"),
            vec!["ls".to_string(), "-l".to_string()]
        );

        assert_eq!(
            to_command(&ExecRequest::command("echo hi"), &options).expect("shell"),
            vec!["sh".to_string(), "-lc".to_string(), "echo hi".to_string()]
        );

        let error = to_command(&ExecRequest::default(), &options).expect_err("必须报错");
        assert_eq!(error.kind(), "invalid_request");
    }

    #[test]
    fn shell_quote_and_base64_helpers() {
        assert_eq!(shell_quote("/tmp/a b"), "'/tmp/a b'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");

        let encoded = BASE64.encode(b"hello world");
        let wrapped = encoded
            .as_bytes()
            .chunks(4)
            .map(|chunk| String::from_utf8_lossy(chunk).to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            decode_base64(wrapped.as_bytes()).expect("解码"),
            b"hello world"
        );
    }

    #[test]
    fn exec_timeout_prefers_request_value() {
        let options = options();
        let request = ExecRequest {
            timeout_secs: Some(15),
            ..ExecRequest::code("1")
        };
        assert_eq!(exec_timeout_secs(&request, &options), 15);
        assert_eq!(exec_timeout_secs(&ExecRequest::code("1"), &options), 60);
    }

    #[test]
    fn network_policy_does_not_fail_when_policy_yaml_absent() {
        let spec = SandboxSpec {
            network: NetworkPolicy::AllowDomains(vec!["example.com".to_string()]),
            ..SandboxSpec::default()
        };
        assert!(to_proto_spec(&spec, &options()).is_ok());
    }
}
