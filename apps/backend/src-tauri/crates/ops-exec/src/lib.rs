//! 容器终端（exec）传输层：Docker + K8s 两个 backend
//!
//! 本 crate 是唯一允许「在容器内执行命令」的地方，与只读的 [`ops`] 严格分离：
//! - Docker：`POST /containers/{id}/exec` → `/exec/{id}/start`（HTTP Upgrade 单流）→
//!   `/exec/{id}/resize` / `/exec/{id}/json`；
//! - K8s：`POST .../pods/{pod}/exec`（WebSocket 多信道），需要 kube 的 `ws` feature。
//!
//! 安全约束（与实施方案 §3.3 一致）：
//! 1. 只接受 [`SHELL_CANDIDATES`] 白名单命中项，**不接受**前端传入任意命令数组；
//! 2. `tty=true` 时 Docker 输出只有 `LogOutput::Console`，stdout/stderr 已合并，不能再按
//!    `StdOut` / `StdErr` 解复用；
//! 3. 镜像不含 shell 时统一映射为 [`ExecError::NoShell`]，由界面展示中文提示而不是裸 500；
//! 4. 会话句柄持有连接器本体（mTLS 临时证书目录的生命周期守卫），不得只 clone 客户端。
//!
//! ```bash
//! cargo test -p ops-exec
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use bollard::container::LogOutput;
use bollard::errors::Error as BollardError;
use bollard::exec::{StartExecOptions, StartExecResults};
use bollard::models::ExecConfig;
use bollard::query_parameters::{InspectContainerOptions, ResizeExecOptions};
use bollard::Docker;
use futures::channel::mpsc as futures_mpsc;
use futures::{SinkExt, Stream, StreamExt};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Status;
use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, AttachParams, AttachedProcess, TerminalSize};
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, SimplexStream, WriteHalf};
use tokio::sync::Mutex as AsyncMutex;

use ops::connector::docker::DockerConnector;
use ops::connector::k8s::K8sConnector;

/// shell 探测顺序（白名单；只允许命中项，绝不接受任意命令）
pub const SHELL_CANDIDATES: [&str; 4] = ["/bin/sh", "/bin/bash", "/bin/ash", "/busybox/sh"];

/// 默认 shell（探测失败时用于报错文案）
pub const DEFAULT_SHELL: &str = "/bin/sh";

/// 终端输出块大小（Docker `output_capacity` / K8s read buffer）
pub const OUTPUT_CAPACITY: usize = 32 * 1024;

/// 终端错误
#[derive(Debug, Clone, thiserror::Error)]
pub enum ExecError {
    /// 镜像里没有 shell（scratch / distroless）
    #[error("该镜像不含 shell（scratch/distroless），请改用日志/性能或 debug sidecar")]
    NoShell,
    /// 容器 / Pod / 容器名不存在或未运行
    #[error("{0}")]
    NotFound(String),
    /// 连接或上游协议错误
    #[error("终端连接失败：{0}")]
    Upstream(String),
    /// 会话已关闭
    #[error("终端会话已关闭")]
    Closed,
}

impl ExecError {
    /// 错误消息是否表示「镜像里没有 shell」
    #[must_use]
    pub fn looks_like_missing_shell(message: &str) -> bool {
        let lowered = message.to_ascii_lowercase();
        lowered.contains("executable file not found")
            || lowered.contains("no such file or directory")
            || lowered.contains("oci runtime exec failed")
    }
}

/// 终端 backend 抽象：Docker 与 K8s 共用服务层会话管理
///
/// 所有方法都取 `&self`：读泵与键盘写入必须能并发执行（单把 `&mut` 锁会在
/// 阻塞读期间把输入通道一起锁死，终端表现为「能连上但打不了字」）。
/// 可变部分由具体实现内部各自的锁保护。
#[async_trait]
pub trait TerminalBackend: Send + Sync {
    /// 写入键盘输入
    async fn write(&self, data: &[u8]) -> Result<(), ExecError>;
    /// 上报窗口大小
    async fn resize(&self, cols: u32, rows: u32) -> Result<(), ExecError>;
    /// 读取下一块输出；`None` 表示已 EOF
    async fn next_chunk(&self) -> Option<Result<Vec<u8>, ExecError>>;
    /// 读取退出码（尽力而为）
    async fn exit_code(&self) -> Option<i32>;
    /// 主动关闭（幂等）
    async fn close(&self);
}

/// 选取候选 shell：显式指定时必须在白名单内
fn shell_candidates(shell: Option<&str>) -> Result<Vec<&'static str>, ExecError> {
    match shell.map(str::trim).filter(|value| !value.is_empty()) {
        Some(requested) => SHELL_CANDIDATES
            .iter()
            .copied()
            .find(|candidate| *candidate == requested)
            .map(|candidate| vec![candidate])
            .ok_or_else(|| {
                ExecError::Upstream(format!(
                    "不支持的 shell：{requested}（仅允许 {}）",
                    SHELL_CANDIDATES.join(" / ")
                ))
            }),
        None => Ok(SHELL_CANDIDATES.to_vec()),
    }
}

type DockerOutput = Pin<Box<dyn Stream<Item = Result<LogOutput, BollardError>> + Send>>;
type DockerInput = Pin<Box<dyn AsyncWrite + Send>>;

// ======================== Docker backend ========================

/// Docker 容器 exec 会话
pub struct DockerExecSession {
    /// 必须持有连接器本体：内联 mTLS 的临时证书目录随它释放
    #[allow(dead_code)]
    connector: DockerConnector,
    docker: Docker,
    exec_id: String,
    container_id: String,
    shell: String,
    /// 输入与输出分别加锁：读泵阻塞时不能挡住键盘写入
    input: AsyncMutex<Option<DockerInput>>,
    output: AsyncMutex<Option<DockerOutput>>,
}

impl DockerExecSession {
    /// 打开容器终端（先 `inspect_container` 校验存在，再按白名单探测 shell）
    ///
    /// # Errors
    ///
    /// 容器不存在 / 未运行、镜像无 shell、Docker API 错误。
    pub async fn open(
        connector: DockerConnector,
        container: &str,
        shell: Option<&str>,
        cols: u32,
        rows: u32,
    ) -> Result<Self, ExecError> {
        let container = container.trim();
        if container.is_empty() {
            return Err(ExecError::NotFound("容器 ID / 名称不能为空".to_string()));
        }
        let docker = connector.client().clone();
        let inspected = docker
            .inspect_container(container, Option::<InspectContainerOptions>::None)
            .await
            .map_err(|err| match err {
                BollardError::DockerResponseServerError {
                    status_code: 404, ..
                } => ExecError::NotFound(format!("容器不存在：{container}")),
                other => ExecError::Upstream(other.to_string()),
            })?;
        // 审计键优先用容器 Id（与 asset_ops_link.resource_uid 同口径）
        let container_id = inspected.id.unwrap_or_else(|| container.to_string());

        let mut last_error: Option<ExecError> = None;
        for candidate in shell_candidates(shell)? {
            match Self::attach(&docker, container, candidate, cols, rows).await {
                Ok((exec_id, input, output)) => {
                    return Ok(Self {
                        connector,
                        docker,
                        exec_id,
                        container_id,
                        shell: candidate.to_string(),
                        input: AsyncMutex::new(Some(input)),
                        output: AsyncMutex::new(Some(output)),
                    });
                }
                Err(ExecError::NoShell) => continue,
                Err(err) => last_error = Some(err),
            }
        }
        Err(last_error.unwrap_or(ExecError::NoShell))
    }

    /// 实际容器 Id（审计键）
    #[must_use]
    pub fn container_id(&self) -> &str {
        &self.container_id
    }

    /// 实际使用的 shell
    #[must_use]
    pub fn shell(&self) -> &str {
        &self.shell
    }

    async fn attach(
        docker: &Docker,
        container: &str,
        shell: &str,
        cols: u32,
        rows: u32,
    ) -> Result<(String, DockerInput, DockerOutput), ExecError> {
        let config = ExecConfig {
            attach_stdin: Some(true),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            tty: Some(true),
            console_size: Some(vec![rows as usize, cols as usize]),
            cmd: Some(vec![shell.to_string()]),
            ..ExecConfig::default()
        };
        let created = docker
            .create_exec(container, config)
            .await
            .map_err(|err| ExecError::Upstream(err.to_string()))?;
        let started = docker
            .start_exec(
                &created.id,
                Some(StartExecOptions {
                    detach: false,
                    tty: true,
                    output_capacity: Some(OUTPUT_CAPACITY),
                }),
            )
            .await;
        match started {
            Ok(StartExecResults::Attached { output, input }) => {
                // 初始尺寸已在 create_exec 的 ConsoleSize 带上，这里再补一次 resize 兜底
                let _ = docker
                    .resize_exec(
                        &created.id,
                        ResizeExecOptions {
                            h: i32::try_from(rows).unwrap_or(32),
                            w: i32::try_from(cols).unwrap_or(120),
                        },
                    )
                    .await;
                Ok((created.id, input, output))
            }
            Ok(StartExecResults::Detached) => Err(ExecError::Upstream(
                "Docker 返回 detached exec（预期 attached）".to_string(),
            )),
            Err(err) => {
                let message = err.to_string();
                if ExecError::looks_like_missing_shell(&message) {
                    Err(ExecError::NoShell)
                } else {
                    Err(ExecError::Upstream(message))
                }
            }
        }
    }
}


#[async_trait]
impl TerminalBackend for DockerExecSession {
    async fn write(&self, data: &[u8]) -> Result<(), ExecError> {
        let mut guard = self.input.lock().await;
        let Some(input) = guard.as_mut() else {
            return Err(ExecError::Closed);
        };
        input
            .write_all(data)
            .await
            .map_err(|err| ExecError::Upstream(err.to_string()))?;
        input
            .flush()
            .await
            .map_err(|err| ExecError::Upstream(err.to_string()))
    }

    async fn resize(&self, cols: u32, rows: u32) -> Result<(), ExecError> {
        self.docker
            .resize_exec(
                &self.exec_id,
                ResizeExecOptions {
                    h: i32::try_from(rows).unwrap_or(32),
                    w: i32::try_from(cols).unwrap_or(120),
                },
            )
            .await
            .map_err(|err| ExecError::Upstream(err.to_string()))
    }

    async fn next_chunk(&self) -> Option<Result<Vec<u8>, ExecError>> {
        // 只锁输出：写入走 input 锁，两者互不阻塞
        let mut guard = self.output.lock().await;
        let output = guard.as_mut()?;
        loop {
            let item = output.next().await?;
            match item {
                Ok(LogOutput::StdOut { message }
                | LogOutput::StdErr { message }
                | LogOutput::StdIn { message }
                | LogOutput::Console { message }) => {
                    if message.is_empty() {
                        continue;
                    }
                    // tty=true 时只会是 Console；统一成字节块，不再按来源 demux
                    return Some(Ok(message.to_vec()));
                }
                Err(err) => return Some(Err(ExecError::Upstream(err.to_string()))),
            }
        }
    }

    async fn exit_code(&self) -> Option<i32> {
        let inspect = self.docker.inspect_exec(&self.exec_id).await.ok()?;
        inspect.exit_code.and_then(|code| i32::try_from(code).ok())
    }

    async fn close(&self) {
        // 用 try_lock：读泵可能正阻塞在 output 上；拿不到锁时后端随 Arc 释放，
        // 底层连接会由 Drop 关闭，绝不能让关闭请求在这里无限等待。
        if let Ok(mut output) = self.output.try_lock() {
            *output = None;
        }
        if let Ok(mut input) = self.input.try_lock() {
            *input = None;
        }
    }
}


// ======================== K8s backend ========================

type StatusFuture = Pin<Box<dyn Future<Output = Option<Status>> + Send>>;

/// K8s Pod exec 会话
pub struct K8sExecSession {
    /// 各通道独立加锁：读泵阻塞时不能挡住键盘写入 / resize
    process: AsyncMutex<Option<AttachedProcess>>,
    stdin: AsyncMutex<Option<WriteHalf<SimplexStream>>>,
    stdout: AsyncMutex<Option<ReadHalf<SimplexStream>>>,
    resize: AsyncMutex<Option<futures_mpsc::Sender<TerminalSize>>>,
    status: AsyncMutex<Option<StatusFuture>>,
    /// 退出码（`i32::MIN` 表示尚未采集到）
    exit_code: AtomicI32,
    shell: String,
}

impl K8sExecSession {
    /// 打开 Pod 终端（校验 Pod / 容器存在且 Running 后按白名单探测 shell）
    ///
    /// # Errors
    ///
    /// Pod / 容器不存在或未运行、镜像无 shell、APIServer 拒绝。
    pub async fn open(
        connector: K8sConnector,
        namespace: &str,
        pod: &str,
        container: Option<&str>,
        shell: Option<&str>,
        cols: u32,
        rows: u32,
    ) -> Result<Self, ExecError> {
        let namespace = namespace.trim();
        let pod = pod.trim();
        if namespace.is_empty() || pod.is_empty() {
            return Err(ExecError::NotFound(
                "命名空间与 Pod 名不能为空".to_string(),
            ));
        }
        connector
            .ensure_container_running(namespace, pod, container)
            .await
            .map_err(|err| {
                let message = err.to_string();
                if ExecError::looks_like_missing_shell(&message) {
                    ExecError::NoShell
                } else {
                    ExecError::NotFound(message)
                }
            })?;

        let client = connector.client().clone();
        let api: Api<Pod> = Api::namespaced(client, namespace);
        let mut last_error: Option<ExecError> = None;
        for candidate in shell_candidates(shell)? {
            let mut attach = AttachParams::interactive_tty();
            if let Some(container) = container.map(str::trim).filter(|value| !value.is_empty()) {
                attach = attach.container(container);
            }
            match api.exec(pod, vec![candidate.to_string()], &attach).await {
                Ok(mut process) => {
                    let stdin = process.stdin();
                    let stdout = process.stdout();
                    let mut resize = process.terminal_size();
                    let status = process.take_status().map(|future| {
                        let future: StatusFuture = Box::pin(future);
                        future
                    });
                    if let Some(sender) = resize.as_mut() {
                        let _ = sender
                            .send(TerminalSize {
                                width: u16::try_from(cols).unwrap_or(120),
                                height: u16::try_from(rows).unwrap_or(32),
                            })
                            .await;
                    }
                    return Ok(Self {
                        process: AsyncMutex::new(Some(process)),
                        stdin: AsyncMutex::new(stdin),
                        stdout: AsyncMutex::new(stdout),
                        resize: AsyncMutex::new(resize),
                        status: AsyncMutex::new(status),
                        exit_code: AtomicI32::new(i32::MIN),
                        shell: candidate.to_string(),
                    });
                }
                Err(err) => {
                    let message = err.to_string();
                    if ExecError::looks_like_missing_shell(&message) {
                        continue;
                    }
                    last_error = Some(ExecError::Upstream(message));
                }
            }
        }
        Err(last_error.unwrap_or(ExecError::NoShell))
    }

    /// 实际使用的 shell
    #[must_use]
    pub fn shell(&self) -> &str {
        &self.shell
    }

    /// stdout EOF 后先等待 status 再 `join()`（顺序反了会死锁）
    async fn finish(&self) {
        let status = self.status.lock().await.take();
        if let Some(status) = status
            && let Ok(Some(status)) =
                tokio::time::timeout(Duration::from_secs(5), status).await
            && let Some(code) = status_exit_code(&status)
        {
            self.exit_code.store(code, Ordering::Relaxed);
        }
        let process = self.process.lock().await.take();
        if let Some(process) = process {
            // 读干 stdout 之后再 join：kube 内部 simplex 缓冲写满会让 join 永不返回。
            // 加超时兜底，超时后 Drop 会 abort 后台任务，避免关闭会话时挂住。
            let _ = tokio::time::timeout(Duration::from_secs(5), process.join()).await;
        }
    }
}


#[async_trait]
impl TerminalBackend for K8sExecSession {
    async fn write(&self, data: &[u8]) -> Result<(), ExecError> {
        let mut guard = self.stdin.lock().await;
        let Some(stdin) = guard.as_mut() else {
            return Err(ExecError::Closed);
        };
        stdin
            .write_all(data)
            .await
            .map_err(|err| ExecError::Upstream(err.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|err| ExecError::Upstream(err.to_string()))
    }

    async fn resize(&self, cols: u32, rows: u32) -> Result<(), ExecError> {
        let mut guard = self.resize.lock().await;
        let Some(resize) = guard.as_mut() else {
            return Ok(());
        };
        resize
            .send(TerminalSize {
                width: u16::try_from(cols).unwrap_or(120),
                height: u16::try_from(rows).unwrap_or(32),
            })
            .await
            .map_err(|err| ExecError::Upstream(err.to_string()))
    }

    async fn next_chunk(&self) -> Option<Result<Vec<u8>, ExecError>> {
        // 只锁 stdout：键盘写入走 stdin 锁，两者互不阻塞
        let mut guard = self.stdout.lock().await;
        let mut buffer = vec![0_u8; OUTPUT_CAPACITY];
        let read_result = {
            let stdout = guard.as_mut()?;
            stdout.read(&mut buffer).await
        };
        match read_result {
            Ok(0) => {
                *guard = None;
                drop(guard);
                self.finish().await;
                None
            }
            Ok(read) => {
                buffer.truncate(read);
                Some(Ok(buffer))
            }
            Err(err) => Some(Err(ExecError::Upstream(err.to_string()))),
        }
    }

    async fn exit_code(&self) -> Option<i32> {
        let raw = self.exit_code.load(Ordering::Relaxed);
        (raw != i32::MIN).then_some(raw)
    }

    async fn close(&self) {
        // 先 abort 后台任务，stdout 的阻塞读会随之结束；不要再锁 stdout（可能正被读泵持有）。
        // 其余通道用 try_lock，避免关闭请求被正在写入的键盘输入阻塞。
        if let Ok(mut process) = self.process.try_lock()
            && let Some(process) = process.take()
        {
            process.abort();
        }
        if let Ok(mut stdin) = self.stdin.try_lock() {
            *stdin = None;
        }
        if let Ok(mut resize) = self.resize.try_lock() {
            *resize = None;
        }
        if let Ok(mut status) = self.status.try_lock() {
            *status = None;
        }
    }
}

/// 从 `Status` 的 `details.causes[reason=ExitCode]` 取退出码，取不到按时 Success 兜底
fn status_exit_code(status: &Status) -> Option<i32> {
    let value = serde_json::to_value(status).ok()?;
    if let Some(causes) = value
        .pointer("/details/causes")
        .and_then(|causes| causes.as_array())
    {
        for cause in causes {
            if cause.get("reason").and_then(|item| item.as_str()) == Some("ExitCode")
                && let Some(code) = cause
                    .get("message")
                    .and_then(|item| item.as_str())
                    .and_then(|message| message.parse::<i32>().ok())
            {
                return Some(code);
            }
        }
    }
    match value.get("status").and_then(|item| item.as_str()) {
        Some("Success") => Some(0),
        Some(_) => Some(1),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_whitelist_accepts_only_known_shells() {
        assert_eq!(
            shell_candidates(Some("/bin/sh")).expect("白名单项"),
            vec!["/bin/sh"]
        );
        assert!(shell_candidates(Some("/bin/zsh")).is_err());
        assert!(shell_candidates(Some("sh")).is_err());
        assert_eq!(shell_candidates(None).expect("默认候选").len(), 4);
    }

    #[test]
    fn missing_shell_markers_are_recognized() {
        assert!(ExecError::looks_like_missing_shell(
            "exec: \"/bin/sh\": executable file not found in $PATH"
        ));
        assert!(ExecError::looks_like_missing_shell(
            "no such file or directory"
        ));
        assert!(!ExecError::looks_like_missing_shell("permission denied"));
    }
}

