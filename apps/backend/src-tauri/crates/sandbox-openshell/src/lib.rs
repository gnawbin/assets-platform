//! `OpenShell` 沙箱实现（vendored `.proto` + tonic 自建客户端）
//!
//! 设计约束（对齐上游文档《`OpenShell` 沙箱整合可行性分析》）：
//! 1. `gateway` **独立部署**，本 crate 只做客户端；`proto` 已 vendor（来源 tag `v0.1.9`），不引入 git 依赖；
//! 2. 禁止反向依赖 `service` / `database` / `api`；
//! 3. `pause` / `resume` 映射到 `SuspendSandbox` / `ResumeSandbox`，**语义降级**：不保证内存快照；
//! 4. 能力缺口（创建期策略 `YAML` / 快照 / 回滚 / 日志）显式返回 `Unsupported`。

mod client;
mod convert;
mod error;

pub mod proto {
    //! tonic/prost 生成代码（package 树：`openshell::v1` / `openshell::datamodel::v1` / `openshell::sandbox::v1`）
    //!
    //! 生成代码不遵循本仓库的 lint 约束，模块级关闭告警，避免污染 `cargo clippy` 输出。
    #![allow(warnings, clippy::all, clippy::pedantic, clippy::nursery)]

    include!(concat!(env!("OUT_DIR"), "/_includes.rs"));
}

pub use client::OpenShellOptions;

use std::time::Duration;

use async_trait::async_trait;
use sandbox::{
    ExecRequest, ExecResult, FileEntry, HealthInfo, SandboxError, SandboxFilter, SandboxHandle,
    SandboxResult, SandboxRuntime, SandboxSpec, SandboxState, SandboxSummary,
};
use tonic::Request;
use tonic::transport::Channel;
use tokio::sync::OnceCell;

use proto::openshell::v1::open_shell_client::OpenShellClient;
use proto::openshell::v1::{
    CreateSandboxRequest, DeleteSandboxRequest, ExecSandboxRequest, GetSandboxRequest,
    HealthRequest, ListSandboxesRequest, ResumeSandboxRequest, Sandbox, SandboxPhase,
    ServiceStatus, SuspendSandboxRequest, exec_sandbox_event,
};

/// OpenShell 后端（连接是懒的：首次 RPC 才建 gRPC 通道）
pub struct OpenShellRuntime {
    options: OpenShellOptions,
    client: OnceCell<OpenShellClient<Channel>>,
}

impl OpenShellRuntime {
    /// 从 `SANDBOX_OPENSHELL_*` 环境变量构造
    pub fn from_env() -> SandboxResult<Self> {
        Ok(Self::with_options(OpenShellOptions::from_env()?))
    }

    /// 用显式选项构造（不建连接，避免在非 Tokio 上下文里触发 reactor）
    #[must_use]
    pub fn with_options(options: OpenShellOptions) -> Self {
        Self {
            options,
            client: OnceCell::new(),
        }
    }

    /// 连接选项（日志 / 诊断）
    #[must_use]
    pub fn options(&self) -> &OpenShellOptions {
        &self.options
    }

    /// 惰性建连（`Endpoint::connect_lazy` 需要运行在 Tokio 上下文）
    async fn client(&self) -> SandboxResult<&OpenShellClient<Channel>> {
        self.client
            .get_or_try_init(|| async {
                let channel = self.options.channel()?;
                Ok::<OpenShellClient<Channel>, SandboxError>(OpenShellClient::new(channel))
            })
            .await
    }

    fn request<T>(&self, message: T) -> SandboxResult<Request<T>> {
        let mut request = Request::new(message);
        if let Some(token) = self
            .options
            .token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let value = format!("Bearer {token}")
                .parse()
                .map_err(|_| SandboxError::config("token 含非法字符，无法放入 Authorization 头"))?;
            request.metadata_mut().insert("authorization", value);
        }
        Ok(request)
    }

    async fn health(&self) -> SandboxResult<HealthInfo> {
        let mut client = self.client().await?.clone();
        let response = client
            .health(self.request(HealthRequest {})?)
            .await
            .map_err(|status| error::status("health", status))?
            .into_inner();

        let ok = ServiceStatus::try_from(response.status)
            .map(|status| status == ServiceStatus::Healthy)
            .unwrap_or(false);
        Ok(HealthInfo {
            ok,
            version: Some(response.version),
            message: Some(format!(
                "OpenShell gateway status={:?}",
                ServiceStatus::try_from(response.status)
            )),
        })
    }
}

#[async_trait]
impl SandboxRuntime for OpenShellRuntime {
    async fn health(&self) -> SandboxResult<HealthInfo> {
        OpenShellRuntime::health(self).await
    }

    async fn create(&self, spec: SandboxSpec) -> SandboxResult<SandboxHandle> {
        let request = CreateSandboxRequest {
            spec: Some(convert::to_proto_spec(&spec, &self.options)?),
            labels: convert::to_labels(&spec),
            workspace_scope: Some(convert::workspace(&self.options.workspace)),
            ..CreateSandboxRequest::default()
        };

        let mut client = self.client().await?.clone();
        let sandbox = client
            .create_sandbox(self.request(request)?)
            .await
            .map_err(|status| error::status("create_sandbox", status))?
            .into_inner()
            .sandbox
            .ok_or_else(|| SandboxError::decode("CreateSandbox 响应缺少 sandbox"))?;

        let name = sandbox
            .metadata
            .as_ref()
            .map(|meta| meta.name.clone())
            .unwrap_or_default();
        if name.is_empty() {
            return Err(SandboxError::decode(
                "CreateSandbox 响应缺少 sandbox.metadata.name",
            ));
        }

        self.wait_ready(&name, client).await
    }

    async fn connect(&self, id: &str) -> SandboxResult<SandboxHandle> {
        let mut client = self.client().await?.clone();
        let sandbox = client
            .get_sandbox(self.request(GetSandboxRequest {
                name: id.to_string(),
                workspace_scope: Some(convert::workspace(&self.options.workspace)),
                ..GetSandboxRequest::default()
            })?)
            .await
            .map_err(|status| error::status("get_sandbox", status))?
            .into_inner()
            .sandbox
            .ok_or_else(|| SandboxError::decode("GetSandbox 响应缺少 sandbox"))?;
        Ok(convert::to_handle(&sandbox, &self.options))
    }

    async fn pause(&self, id: &str) -> SandboxResult<()> {
        let mut client = self.client().await?.clone();
        client
            .suspend_sandbox(self.request(SuspendSandboxRequest {
                name: id.to_string(),
                workspace_scope: Some(convert::workspace(&self.options.workspace)),
                ..SuspendSandboxRequest::default()
            })?)
            .await
            .map_err(|status| error::status("suspend_sandbox", status))?;
        Ok(())
    }

    async fn resume(&self, id: &str) -> SandboxResult<SandboxHandle> {
        let mut client = self.client().await?.clone();
        let sandbox = client
            .resume_sandbox(self.request(ResumeSandboxRequest {
                name: id.to_string(),
                workspace_scope: Some(convert::workspace(&self.options.workspace)),
                restart_if_restore_fails: true,
            })?)
            .await
            .map_err(|status| error::status("resume_sandbox", status))?
            .into_inner()
            .sandbox
            .ok_or_else(|| SandboxError::decode("ResumeSandbox 响应缺少 sandbox"))?;
        Ok(convert::to_handle(&sandbox, &self.options))
    }

    async fn kill(&self, id: &str) -> SandboxResult<()> {
        let mut client = self.client().await?.clone();
        client
            .delete_sandbox(self.request(DeleteSandboxRequest {
                name: id.to_string(),
                workspace_scope: Some(convert::workspace(&self.options.workspace)),
                allow_missing: true,
                request_id: String::new(),
            })?)
            .await
            .map_err(|status| error::status("delete_sandbox", status))?;
        Ok(())
    }

    async fn list(&self, filter: SandboxFilter) -> SandboxResult<Vec<SandboxSummary>> {
        let label_selector = if filter.metadata.is_empty() {
            String::new()
        } else {
            filter
                .metadata
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join(",")
        };

        let mut client = self.client().await?.clone();
        let response = client
            .list_sandboxes(self.request(ListSandboxesRequest {
                workspace_scope: Some(convert::workspace(&self.options.workspace)),
                page_size: 0,
                page_token: String::new(),
                label_selector,
            })?)
            .await
            .map_err(|status| error::status("list_sandboxes", status))?
            .into_inner();

        let limit = filter
            .limit
            .unwrap_or(100)
            .min(sandbox::MAX_LIST_LIMIT) as usize;

        Ok(response
            .sandboxes
            .iter()
            .map(convert::to_summary)
            .filter(|item| filter.states.is_empty() || filter.states.contains(&item.state))
            .take(limit)
            .collect())
    }

    /// OpenShell 没有「续期」RPC：TTL 由 gateway / driver 管理
    async fn set_timeout(&self, _id: &str, _timeout_secs: u32) -> SandboxResult<()> {
        Ok(())
    }

    async fn exec(&self, id: &str, request: ExecRequest) -> SandboxResult<ExecResult> {
        let command = convert::to_command(&request, &self.options)?;
        let timeout = convert::exec_timeout_secs(&request, &self.options);
        let started = std::time::Instant::now();
        let (stdout, stderr, exit_code) = self.exec_stream(id, command, None, timeout).await?;

        Ok(ExecResult {
            exit_code,
            text: Some(String::from_utf8_lossy(&stdout).to_string()),
            stdout: convert::split_lines(&stdout),
            stderr: convert::split_lines(&stderr),
            error: None,
            duration_ms: convert::elapsed_ms(started),
        })
    }

    /// 读文件：proto 无文件 RPC，用 `base64 < path` 兜底（受 `MAX_TRANSFER_BYTES` 限制）
    async fn read_file(&self, id: &str, path: &str, _user: &str) -> SandboxResult<Vec<u8>> {
        let command = vec![
            "sh".to_string(),
            "-lc".to_string(),
            format!("base64 < {}", convert::shell_quote(path)),
        ];
        let timeout = self.options.exec_timeout.as_secs().max(1);
        let (stdout, stderr, exit_code) = self.exec_stream(id, command, None, timeout).await?;
        if exit_code != 0 {
            return Err(SandboxError::not_found(format!(
                "读取失败（exit {exit_code}）：{}",
                String::from_utf8_lossy(&stderr).trim()
            )));
        }
        let data = convert::decode_base64(&stdout)?;
        if data.len() > sandbox::MAX_TRANSFER_BYTES {
            return Err(SandboxError::invalid_request(
                400,
                format!("文件超过上限 {} 字节", sandbox::MAX_TRANSFER_BYTES),
            ));
        }
        Ok(data)
    }

    /// 写文件：`base64 -d` 从 stdin 读取，避免命令拼接二进制
    async fn write_file(
        &self,
        id: &str,
        path: &str,
        _user: &str,
        data: Vec<u8>,
    ) -> SandboxResult<()> {
        if data.len() > sandbox::MAX_TRANSFER_BYTES {
            return Err(SandboxError::invalid_request(
                400,
                format!("文件超过上限 {} 字节", sandbox::MAX_TRANSFER_BYTES),
            ));
        }
        let quoted = convert::shell_quote(path);
        let script = format!("mkdir -p \"$(dirname {quoted})\" && base64 -d > {quoted}");
        let command = vec!["sh".to_string(), "-lc".to_string(), script];
        let timeout = self.options.exec_timeout.as_secs().max(1);
        let (_stdout, stderr, exit_code) = self
            .exec_stream(id, command, Some(data), timeout)
            .await?;
        if exit_code != 0 {
            return Err(SandboxError::server(
                502,
                format!(
                    "写入失败（exit {exit_code}）：{}",
                    String::from_utf8_lossy(&stderr).trim()
                ),
            ));
        }
        Ok(())
    }

    /// 列目录：`ls -1ap` 解析（不返回字节数，`size` 恒为 0；P6 视需要再补 `stat`）
    async fn list_files(&self, id: &str, dir: &str, _user: &str) -> SandboxResult<Vec<FileEntry>> {
        let command = vec![
            "sh".to_string(),
            "-lc".to_string(),
            format!("ls -1ap {}", convert::shell_quote(dir)),
        ];
        let timeout = self.options.exec_timeout.as_secs().max(1);
        let (stdout, stderr, exit_code) = self.exec_stream(id, command, None, timeout).await?;
        if exit_code != 0 {
            return Err(SandboxError::not_found(format!(
                "列目录失败（exit {exit_code}）：{}",
                String::from_utf8_lossy(&stderr).trim()
            )));
        }

        let base = dir.trim_end_matches('/');
        let mut entries = Vec::new();
        for line in convert::split_lines(&stdout) {
            let name = line.trim_end();
            if name.is_empty() || name == "./" || name == "../" {
                continue;
            }
            let is_dir = name.ends_with('/');
            let name = name.trim_end_matches('/').to_string();
            if name.is_empty() || name == "." || name == ".." {
                continue;
            }
            entries.push(FileEntry {
                path: format!("{base}/{name}"),
                name,
                is_dir,
                size: 0,
            });
        }
        Ok(entries)
    }
}

impl OpenShellRuntime {
    /// `ExecSandbox` 是 server-streaming：聚合 stdout/stderr，取 exit 事件
    async fn exec_stream(
        &self,
        name: &str,
        command: Vec<String>,
        stdin: Option<Vec<u8>>,
        timeout_secs: u64,
    ) -> SandboxResult<(Vec<u8>, Vec<u8>, i32)> {
        let request = ExecSandboxRequest {
            workspace_scope: Some(convert::workspace(&self.options.workspace)),
            sandbox: name.to_string(),
            command,
            stdin: stdin.unwrap_or_default(),
            execution_timeout: Some(convert::proto_duration(timeout_secs)),
            ..ExecSandboxRequest::default()
        };

        let mut client = self.client().await?.clone();
        let mut stream = client
            .exec_sandbox(self.request(request)?)
            .await
            .map_err(|status| error::status("exec_sandbox", status))?
            .into_inner();

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code = -1;

        while let Some(event) = stream
            .message()
            .await
            .map_err(|status| error::status("exec_sandbox.stream", status))?
        {
            match event.payload {
                Some(exec_sandbox_event::Payload::Stdout(chunk)) => {
                    stdout.extend_from_slice(&chunk.data);
                }
                Some(exec_sandbox_event::Payload::Stderr(chunk)) => {
                    stderr.extend_from_slice(&chunk.data);
                }
                Some(exec_sandbox_event::Payload::Exit(done)) => exit_code = done.exit_code,
                None => {}
            }
        }

        Ok((stdout, stderr, exit_code))
    }
}

impl OpenShellRuntime {
    /// 轮询 `GetSandbox` 直到 Ready（或超时 / 进入终态）
    async fn wait_ready(
        &self,
        name: &str,
        mut client: OpenShellClient<Channel>,
    ) -> SandboxResult<SandboxHandle> {
        let deadline = tokio::time::Instant::now() + self.options.ready_timeout;

        loop {
            let sandbox: Sandbox = client
                .get_sandbox(self.request(GetSandboxRequest {
                    name: name.to_string(),
                    workspace_scope: Some(convert::workspace(&self.options.workspace)),
                    ..GetSandboxRequest::default()
                })?)
                .await
                .map_err(|status| error::status("get_sandbox", status))?
                .into_inner()
                .sandbox
                .ok_or_else(|| SandboxError::decode("GetSandbox 响应缺少 sandbox"))?;

            let phase = sandbox
                .status
                .as_ref()
                .and_then(|status| SandboxPhase::try_from(status.phase).ok())
                .unwrap_or_default();

            match convert::to_state(phase) {
                SandboxState::Running => return Ok(convert::to_handle(&sandbox, &self.options)),
                SandboxState::Unknown
                    if matches!(phase, SandboxPhase::Error | SandboxPhase::Completed) =>
                {
                    return Err(SandboxError::server(
                        502,
                        format!("沙箱进入终态 {phase:?}（name={name}）"),
                    ));
                }
                _ => {}
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(SandboxError::timeout(
                    self.options.ready_timeout.as_secs().max(1),
                ));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}
