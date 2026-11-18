//! russh 0.61 客户端实现
//!
//! 设计要点：
//! - 握手阶段由 [`CapturingHandler`] 记录主机密钥并按「已固定指纹」判定放行，
//!   指纹不匹配时**握手直接失败**，不做「记住新指纹」的静默覆盖；
//! - 会话建立后读写各跑一个 task：读 task 把 `ChannelMsg` 翻译成 [`SshEvent`]，
//!   写 task 消费 [`SessionCmd`] 并负责最终 `disconnect`（`Handle` 归它持有）。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use russh::client::{self, Handle};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKey, decode_secret_key};
use russh::{ChannelMsg, Disconnect};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::error::SshError;
use crate::spec::{
    HostKeyInfo, PTY_TERM, SessionCmd, SshAuth, SshEvent, SshSession, SshTargetSpec,
    fingerprint_matches,
};

/// keepalive 间隔（秒）：终端长时间空闲时保活，避免中间 NAT 静默断链
const KEEPALIVE_SECS: u64 = 30;
/// 输入队列长度：一屏粘贴（几十 KB）也不会阻塞 WS 读循环
const INPUT_BUFFER: usize = 256;
/// 输出队列长度：足够吸收 `top` 这类高频刷新的突发
const OUTPUT_BUFFER: usize = 512;

/// 握手期间记录主机密钥的槽位（`connect()` 返回时一定已写入）
#[derive(Default)]
struct HandlerState {
    host_key: Mutex<Option<HostKeyInfo>>,
}

impl HandlerState {
    fn store(&self, info: HostKeyInfo) {
        *self
            .host_key
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(info);
    }

    fn take(&self) -> Option<HostKeyInfo> {
        self.host_key
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    fn peek(&self) -> Option<HostKeyInfo> {
        self.host_key
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// 只负责校验 / 记录主机密钥的客户端 Handler
struct CapturingHandler {
    /// 已固定的指纹（归一化后）；`None` 表示只扫描
    expected: Option<String>,
    state: Arc<HandlerState>,
}

impl client::Handler for CapturingHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        let info = HostKeyInfo {
            fingerprint: server_public_key.fingerprint(HashAlg::Sha256).to_string(),
            key_type: server_public_key.algorithm().as_str().to_string(),
        };
        let accepted = fingerprint_matches(self.expected.as_deref(), &info.fingerprint);
        if accepted {
            debug!(fingerprint = %info.fingerprint, key_type = %info.key_type, "已接受服务端主机密钥");
        } else {
            warn!(fingerprint = %info.fingerprint, "主机密钥指纹与记录不一致，拒绝连接");
        }
        self.state.store(info);
        Ok(accepted)
    }
}

/// 一次握手 + 连接的结果
struct Connection {
    handle: Handle<CapturingHandler>,
    host_key: HostKeyInfo,
}

/// 建立 TCP + SSH 握手（不认证）
async fn connect(spec: &SshTargetSpec) -> Result<Connection, SshError> {
    spec.validate()?;

    let config = Arc::new(client::Config {
        keepalive_interval: Some(Duration::from_secs(KEEPALIVE_SECS)),
        keepalive_max: 3,
        nodelay: true,
        ..client::Config::default()
    });
    let state = Arc::new(HandlerState::default());
    let handler = CapturingHandler {
        expected: spec.normalized_fingerprint(),
        state: Arc::clone(&state),
    };

    let host = spec.host.trim().to_string();
    let timeout = spec.timeout();
    let outcome = tokio::time::timeout(
        timeout,
        client::connect(config, (host.clone(), spec.port), handler),
    )
    .await;

    let handle = match outcome {
        Err(_) => {
            return Err(SshError::Connect(format!(
                "连接 {host}:{} 超时（{} 秒）",
                spec.port,
                timeout.as_secs()
            )));
        }
        Ok(Err(error)) => return Err(map_handshake_error(&state, spec, &error)),
        Ok(Ok(handle)) => handle,
    };

    let host_key = state
        .take()
        .ok_or_else(|| SshError::Handshake("服务端未提供主机密钥".to_string()))?;
    Ok(Connection { handle, host_key })
}

/// 把握手阶段的底层错误翻译成可读错误，指纹不匹配优先
fn map_handshake_error(state: &HandlerState, spec: &SshTargetSpec, error: &russh::Error) -> SshError {
    if let Some(info) = state.peek() {
        if !fingerprint_matches(spec.expected_fingerprint.as_deref(), &info.fingerprint) {
            return SshError::HostKeyMismatch(format!(
                "服务端主机指纹为 {}，与记录的主机指纹不一致",
                info.fingerprint
            ));
        }
    }
    SshError::Handshake(error.to_string())
}

/// 按认证方式执行认证，失败统一映射为 [`SshError::AuthFailed`]
async fn authenticate(
    handle: &mut Handle<CapturingHandler>,
    spec: &SshTargetSpec,
) -> Result<(), SshError> {
    let user = spec.username.trim();
    let result = match &spec.auth {
        SshAuth::Password(password) => handle.authenticate_password(user, password.clone()).await?,
        SshAuth::PrivateKey { pem, passphrase } => {
            let key = decode_secret_key(pem, passphrase.as_deref())
                .map_err(|error| SshError::Credential(format!("私钥解析失败：{error}")))?;
            let hash_alg = handle.best_supported_rsa_hash().await?.flatten();
            let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg);
            handle.authenticate_publickey(user, key).await?
        }
    };
    if result.success() {
        Ok(())
    } else {
        Err(SshError::AuthFailed(format!(
            "账号 {user} 认证未通过（请检查密码 / 私钥与目标机 sshd 配置）"
        )))
    }
}

/// 只握手：拿指纹后立即断开
pub(crate) async fn probe(spec: &SshTargetSpec) -> Result<HostKeyInfo, SshError> {
    let Connection { handle, host_key } = connect(spec).await?;
    let _ = handle
        .disconnect(Disconnect::ByApplication, "fingerprint probe", "en")
        .await;
    Ok(host_key)
}

/// 认证 + PTY + shell，返回可读写的会话
pub(crate) async fn open(
    spec: &SshTargetSpec,
    cols: u32,
    rows: u32,
) -> Result<SshSession, SshError> {
    let Connection {
        mut handle,
        host_key,
    } = connect(spec).await?;
    authenticate(&mut handle, spec).await?;

    let channel = handle
        .channel_open_session()
        .await
        .map_err(|error| SshError::Rejected(format!("打开会话通道失败：{error}")))?;
    // 0×0 会让部分 sshd 直接拒绝 PTY，这里做下限保护
    let cols = cols.max(1);
    let rows = rows.max(1);
    channel
        .request_pty(true, PTY_TERM, cols, rows, 0, 0, &[])
        .await
        .map_err(|error| SshError::Rejected(format!("申请 PTY 失败：{error}")))?;
    channel
        .request_shell(true)
        .await
        .map_err(|error| SshError::Rejected(format!("申请 shell 失败：{error}")))?;

    let (mut read_half, write_half) = channel.split();
    let (input_tx, mut input_rx) = mpsc::channel::<SessionCmd>(INPUT_BUFFER);
    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(OUTPUT_BUFFER);
    let bytes_in = Arc::new(AtomicU64::new(0));
    let bytes_out = Arc::new(AtomicU64::new(0));

    // 读 task：PTY 输出 → SshEvent
    let reader_input = input_tx.clone();
    let reader_bytes = Arc::clone(&bytes_out);
    tokio::spawn(async move {
        loop {
            match read_half.wait().await {
                Some(ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. }) => {
                    reader_bytes.fetch_add(
                        u64::try_from(data.len()).unwrap_or(u64::MAX),
                        Ordering::Relaxed,
                    );
                    if event_tx.send(SshEvent::Data(data.to_vec())).await.is_err() {
                        break;
                    }
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    let _ = event_tx.send(SshEvent::Exit(Some(exit_status))).await;
                }
                Some(ChannelMsg::Close) => {
                    let _ = event_tx
                        .send(SshEvent::Closed("远端关闭了会话".to_string()))
                        .await;
                    break;
                }
                // Eof / Success / WindowAdjusted 等与终端体验无关，忽略
                Some(_) => {}
                None => {
                    let _ = event_tx
                        .send(SshEvent::Closed("连接已断开".to_string()))
                        .await;
                    break;
                }
            }
        }
        // 读端结束后通知写端收尾，避免 Handle / 通道悬空
        let _ = reader_input.try_send(SessionCmd::Close);
        debug!("ssh reader task finished");
    });

    // 写 task：SessionCmd → 通道；结束时负责 disconnect
    let writer_bytes = Arc::clone(&bytes_in);
    tokio::spawn(async move {
        while let Some(command) = input_rx.recv().await {
            match command {
                SessionCmd::Data(data) => {
                    let length = u64::try_from(data.len()).unwrap_or(u64::MAX);
                    if write_half.data_bytes(data).await.is_err() {
                        break;
                    }
                    writer_bytes.fetch_add(length, Ordering::Relaxed);
                }
                SessionCmd::Resize { cols, rows } => {
                    let _ = write_half.window_change(cols, rows, 0, 0).await;
                }
                SessionCmd::Close => {
                    let _ = write_half.eof().await;
                    let _ = write_half.close().await;
                    break;
                }
            }
        }
        let _ = handle
            .disconnect(Disconnect::ByApplication, "session closed", "en")
            .await;
        debug!("ssh writer task finished");
    });

    Ok(SshSession::new(
        host_key, input_tx, event_rx, bytes_in, bytes_out,
    ))
}

