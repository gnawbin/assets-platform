//! 传输层数据类型与不依赖具体协议的纯逻辑
//!
//! 这里的代码在 `ssh` feature 关闭时也会编译（`probe` / `open` 返回
//! [`SshError::Disabled`]），因此**不能**引用 `russh` 的任何类型，
//! 便于上层（service / api）在无网络环境先行编译自测。

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::mpsc;

use crate::error::SshError;

/// SSH 默认端口
pub const DEFAULT_PORT: u16 = 22;
/// 连接超时下限（秒）：再小会把内网慢机器误判为不可达
pub const MIN_TIMEOUT_SECS: u64 = 3;
/// 连接超时上限（秒）：避免一次误操作把请求线程挂几分钟
pub const MAX_TIMEOUT_SECS: u64 = 60;
/// 申请 PTY 时上报的终端类型（xterm.js 兼容性最好）
pub const PTY_TERM: &str = "xterm-256color";

/// SSH 认证材料
///
/// `Debug` 手写为掩码，保证凭证不会随 `tracing::debug!` / 错误链路泄漏。
#[derive(Clone)]
pub enum SshAuth {
    /// 密码认证
    Password(String),
    /// 私钥认证（OpenSSH / PEM 文本；可选口令）
    PrivateKey {
        /// 私钥文本（PEM / OpenSSH 格式）
        pem: String,
        /// 私钥口令，未加密的私钥为 `None`
        passphrase: Option<String>,
    },
}

impl fmt::Debug for SshAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Password(_) => f.write_str("Password(***)"),
            Self::PrivateKey { pem, passphrase } => f
                .debug_struct("PrivateKey")
                .field("pem", &format_args!("<{} 字节>", pem.len()))
                .field("passphrase", &passphrase.as_ref().map(|_| "***"))
                .finish(),
        }
    }
}

impl SshAuth {
    /// 认证方式稳定编码（与 `ops_ssh_target.auth_type` 一致）
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Password(_) => "password",
            Self::PrivateKey { .. } => "private_key",
        }
    }
}

/// 连接一台主机的全部参数
#[derive(Clone, Debug)]
pub struct SshTargetSpec {
    /// 主机名或 IP
    pub host: String,
    /// 端口
    pub port: u16,
    /// 登录用户名
    pub username: String,
    /// 认证材料
    pub auth: SshAuth,
    /// 连接 / 握手超时（秒，会被夹到 `3..=60`）
    pub timeout_secs: u64,
    /// 已固定（TOFU）的主机指纹；`None` 表示只扫描不校验
    pub expected_fingerprint: Option<String>,
}

impl SshTargetSpec {
    /// 构造一个使用默认端口、默认超时的参数集
    #[must_use]
    pub fn new(host: impl Into<String>, username: impl Into<String>, auth: SshAuth) -> Self {
        Self {
            host: host.into(),
            port: DEFAULT_PORT,
            username: username.into(),
            auth,
            timeout_secs: 10,
            expected_fingerprint: None,
        }
    }

    /// 生效的连接超时（夹取到 `MIN_TIMEOUT_SECS..=MAX_TIMEOUT_SECS`）
    #[must_use]
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS))
    }

    /// 归一化后的期望指纹（`None` 表示不做校验）
    #[must_use]
    pub fn normalized_fingerprint(&self) -> Option<String> {
        let normalized = normalize_fingerprint(self.expected_fingerprint.as_deref()?);
        (!normalized.is_empty()).then_some(normalized)
    }

    /// 参数自检：在建立 TCP 连接前把明显错误挡在本地
    ///
    /// # Errors
    ///
    /// 返回 [`SshError::InvalidSpec`]，消息可直接展示给用户。
    pub fn validate(&self) -> Result<(), SshError> {
        let host = self.host.trim();
        if host.is_empty() {
            return Err(SshError::InvalidSpec("主机地址不能为空".to_string()));
        }
        if host.chars().any(char::is_whitespace) {
            return Err(SshError::InvalidSpec(
                "主机地址不能包含空白字符".to_string(),
            ));
        }
        if host.contains("://") || host.contains('/') {
            return Err(SshError::InvalidSpec(
                "主机地址只填 host 或 IP，不要带 scheme / 路径".to_string(),
            ));
        }
        if self.port == 0 {
            return Err(SshError::InvalidSpec(
                "端口必须在 1-65535 之间".to_string(),
            ));
        }
        if self.username.trim().is_empty() {
            return Err(SshError::InvalidSpec("登录账号不能为空".to_string()));
        }
        match &self.auth {
            SshAuth::Password(password) if password.is_empty() => {
                return Err(SshError::InvalidSpec("密码不能为空".to_string()));
            }
            SshAuth::PrivateKey { pem, .. } if pem.trim().is_empty() => {
                return Err(SshError::InvalidSpec("私钥内容不能为空".to_string()));
            }
            _ => {}
        }
        if let Some(fingerprint) = &self.expected_fingerprint {
            let normalized = normalize_fingerprint(fingerprint);
            if !normalized.is_empty() && !normalized.starts_with("SHA256:") {
                return Err(SshError::InvalidSpec(
                    "主机指纹只支持 SHA256:... 格式（不解析 MD5: 旧格式）".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// 供日志使用的脱敏描述（不含账号密码 / 私钥）
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "{}@{}:{}（{}）",
            self.username.trim(),
            self.host.trim(),
            self.port,
            self.auth.kind()
        )
    }
}

/// 服务端主机密钥信息
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostKeyInfo {
    /// `SHA256:<base64>` 形式的指纹（OpenSSH 同款格式）
    pub fingerprint: String,
    /// 密钥算法，例如 `ssh-ed25519` / `rsa-sha2-512`
    pub key_type: String,
}

/// 从会话读出的输出事件
#[derive(Debug)]
pub enum SshEvent {
    /// PTY 原始字节（可能不是合法 UTF-8，必须按字节转发）
    Data(Vec<u8>),
    /// 远端退出状态
    Exit(Option<u32>),
    /// 会话结束（原因供前端展示）
    Closed(String),
}

/// 发往会话的指令
#[derive(Debug)]
pub enum SessionCmd {
    /// 键盘输入（原始字节，包含控制字符与转义序列）
    Data(Vec<u8>),
    /// 窗口大小变化
    Resize {
        /// 列数
        cols: u32,
        /// 行数
        rows: u32,
    },
    /// 主动关闭会话
    Close,
}

/// 已建立的 SSH 会话
///
/// 读写分离：输入走内部有界队列（[`SessionCmd`]），输出由 [`Self::take_output`]
/// 取出的接收端消费；字节计数用原子变量，供审计在关闭时一次性落库。
pub struct SshSession {
    host_key: HostKeyInfo,
    input: mpsc::Sender<SessionCmd>,
    output: Option<mpsc::Receiver<SshEvent>>,
    bytes_in: Arc<AtomicU64>,
    bytes_out: Arc<AtomicU64>,
}

impl SshSession {
    #[cfg_attr(not(feature = "ssh"), allow(dead_code))]
    pub(crate) fn new(
        host_key: HostKeyInfo,
        input: mpsc::Sender<SessionCmd>,
        output: mpsc::Receiver<SshEvent>,
        bytes_in: Arc<AtomicU64>,
        bytes_out: Arc<AtomicU64>,
    ) -> Self {
        Self {
            host_key,
            input,
            output: Some(output),
            bytes_in,
            bytes_out,
        }
    }

    /// 本次连接实际校验通过的主机密钥
    #[must_use]
    pub fn host_key(&self) -> &HostKeyInfo {
        &self.host_key
    }

    /// `(客户端→主机, 主机→客户端)` 的累计字节数
    #[must_use]
    pub fn bytes(&self) -> (u64, u64) {
        (
            self.bytes_in.load(Ordering::Relaxed),
            self.bytes_out.load(Ordering::Relaxed),
        )
    }

    /// 取出输出端（只能取一次；返回 `None` 说明已被取走）
    pub fn take_output(&mut self) -> Option<mpsc::Receiver<SshEvent>> {
        self.output.take()
    }

    /// 写入键盘输入
    ///
    /// # Errors
    ///
    /// 会话已关闭（写任务退出）时返回 [`SshError::Closed`]。
    pub async fn write(&self, data: Vec<u8>) -> Result<(), SshError> {
        self.send(SessionCmd::Data(data)).await
    }

    /// 上报窗口大小变化（尽力而为：会话已关闭不报错）
    pub async fn resize(&self, cols: u32, rows: u32) {
        let _ = self.input.send(SessionCmd::Resize { cols, rows }).await;
    }

    /// 请求关闭会话（幂等：会话已关闭直接返回 `Ok`）
    pub async fn close(&self) -> Result<(), SshError> {
        // 队列已关闭说明写任务早就退出，同样视为「已关闭」
        let _ = self.input.send(SessionCmd::Close).await;
        Ok(())
    }

    /// 会话是否已经断开
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.input.is_closed()
    }

    async fn send(&self, command: SessionCmd) -> Result<(), SshError> {
        self.input
            .send(command)
            .await
            .map_err(|_| SshError::Closed("会话已断开".to_string()))
    }
}

impl fmt::Debug for SshSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (bytes_in, bytes_out) = self.bytes();
        f.debug_struct("SshSession")
            .field("host_key", &self.host_key)
            .field("bytes_in", &bytes_in)
            .field("bytes_out", &bytes_out)
            .field("closed", &self.is_closed())
            .finish()
    }
}

/// 归一化主机指纹：去首尾空白，仅把 `sha256` 前缀统一成 `SHA256`
///
/// base64 正文大小写敏感，**不能**整体转大写。
#[must_use]
pub fn normalize_fingerprint(raw: &str) -> String {
    let trimmed = raw.trim();
    for prefix in ["SHA256:", "sha256:", "Sha256:"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return format!("SHA256:{}", rest.trim());
        }
    }
    trimmed.to_string()
}

/// 比较期望指纹与实测指纹
///
/// `expected` 为 `None` 或空串时视为「只扫描不校验」（TOFU 首次纳管），
/// 其余情况必须逐字相等。
#[must_use]
pub fn fingerprint_matches(expected: Option<&str>, actual: &str) -> bool {
    match expected.map(normalize_fingerprint) {
        None => true,
        Some(expected) if expected.is_empty() => true,
        Some(expected) => expected == normalize_fingerprint(actual),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_masks_password() {
        let auth = SshAuth::Password("s3cret-password".to_string());
        let text = format!("{auth:?}");
        assert!(!text.contains("s3cret-password"), "{text}");
        assert!(text.contains("***"), "{text}");
    }

    #[test]
    fn debug_masks_private_key() {
        let auth = SshAuth::PrivateKey {
            pem: "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n".to_string(),
            passphrase: Some("hunter2".to_string()),
        };
        let text = format!("{auth:?}");
        assert!(!text.contains("BEGIN OPENSSH"), "{text}");
        assert!(!text.contains("hunter2"), "{text}");
    }

    #[test]
    fn validate_rejects_empty_host() {
        let mut spec = SshTargetSpec::new("  ", "root", SshAuth::Password("p".to_string()));
        let error = spec.validate().unwrap_err();
        assert!(error.to_string().contains("主机地址"), "{error}");
        spec.host = "127.0.0.1".to_string();
        assert!(spec.validate().is_ok());
    }

    #[test]
    fn validate_rejects_scheme_and_space() {
        let mut spec = SshTargetSpec::new("ssh://10.0.0.1", "root", SshAuth::Password("p".into()));
        assert!(spec.validate().is_err());
        spec.host = "10.0.0.1 22".to_string();
        assert!(spec.validate().is_err());
        spec.host = "10.0.0.1".to_string();
        assert!(spec.validate().is_ok());
    }

    #[test]
    fn validate_rejects_zero_port_and_empty_user() {
        let mut spec = SshTargetSpec::new("10.0.0.1", "root", SshAuth::Password("p".into()));
        spec.port = 0;
        assert!(spec.validate().is_err());
        spec.port = 22;
        spec.username = "   ".to_string();
        assert!(spec.validate().is_err());
    }

    #[test]
    fn validate_rejects_missing_secret() {
        let spec = SshTargetSpec::new("10.0.0.1", "root", SshAuth::Password(String::new()));
        assert!(spec.validate().is_err());
        let spec = SshTargetSpec::new(
            "10.0.0.1",
            "root",
            SshAuth::PrivateKey {
                pem: "   ".to_string(),
                passphrase: None,
            },
        );
        assert!(spec.validate().is_err());
    }

    #[test]
    fn validate_rejects_md5_fingerprint() {
        let mut spec = SshTargetSpec::new("10.0.0.1", "root", SshAuth::Password("p".into()));
        spec.expected_fingerprint = Some("MD5:aa:bb:cc".to_string());
        let error = spec.validate().unwrap_err();
        assert!(error.to_string().contains("SHA256"), "{error}");
        spec.expected_fingerprint = Some("SHA256:Nh0Me49Zh9fDw".to_string());
        assert!(spec.validate().is_ok());
    }

    #[test]
    fn timeout_is_clamped() {
        let mut spec = SshTargetSpec::new("10.0.0.1", "root", SshAuth::Password("p".into()));
        spec.timeout_secs = 0;
        assert_eq!(spec.timeout(), Duration::from_secs(MIN_TIMEOUT_SECS));
        spec.timeout_secs = 3600;
        assert_eq!(spec.timeout(), Duration::from_secs(MAX_TIMEOUT_SECS));
        spec.timeout_secs = 15;
        assert_eq!(spec.timeout(), Duration::from_secs(15));
    }

    #[test]
    fn fingerprint_prefix_is_normalized() {
        assert_eq!(normalize_fingerprint("  sha256:AbC+dEf  "), "SHA256:AbC+dEf");
        assert_eq!(normalize_fingerprint("SHA256:AbC"), "SHA256:AbC");
        assert_eq!(normalize_fingerprint("  "), "");
    }

    #[test]
    fn fingerprint_matching_rules() {
        // 未记录指纹：只扫描，放行
        assert!(fingerprint_matches(None, "SHA256:AbC"));
        assert!(fingerprint_matches(Some("   "), "SHA256:AbC"));
        // 前缀大小写差异不影响判定
        assert!(fingerprint_matches(Some("sha256:AbC"), "SHA256:AbC"));
        // 正文逐字比较
        assert!(!fingerprint_matches(Some("SHA256:AbC"), "SHA256:abc"));
        assert!(!fingerprint_matches(Some("SHA256:AbC"), "SHA256:AbD"));
    }

    #[tokio::test]
    async fn session_write_surfaces_closed_channel() {
        let (input, receiver) = mpsc::channel::<SessionCmd>(1);
        drop(receiver);
        let session = SshSession::new(
            HostKeyInfo {
                fingerprint: "SHA256:AbC".to_string(),
                key_type: "ssh-ed25519".to_string(),
            },
            input,
            mpsc::channel(1).1,
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
        );
        assert!(session.write(b"ls\n".to_vec()).await.is_err());
        assert!(session.close().await.is_ok());
    }
}

