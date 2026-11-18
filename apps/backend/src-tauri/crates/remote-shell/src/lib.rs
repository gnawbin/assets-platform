//! 远程终端传输层（目前只实现 SSH）
//!
//! 只负责「协议 + 字节流」，不做 HTTP、不碰数据库：
//! - [`probe`]：只握手，抓主机密钥指纹（不认证、不起 shell，不改远端状态）；
//! - [`open`]：认证 + 申请 PTY + 申请 shell，返回可按字节读写的 [`SshSession`]。
//!
//! `ssh` feature 关闭时本 crate 仍可编译，`probe` / `open` 返回
//! [`SshError::Disabled`]，便于无网络环境先跑通上层（service / api）。

pub mod error;
mod spec;

#[cfg(feature = "ssh")]
mod imp;

pub use self::error::SshError;
pub use self::spec::{
    DEFAULT_PORT, HostKeyInfo, MAX_TIMEOUT_SECS, MIN_TIMEOUT_SECS, PTY_TERM, SessionCmd, SshAuth,
    SshEvent, SshSession, SshTargetSpec, fingerprint_matches, normalize_fingerprint,
};

/// 编译期是否启用了 SSH 传输
#[must_use]
pub const fn ssh_enabled() -> bool {
    cfg!(feature = "ssh")
}

/// 只握手取主机密钥指纹（不认证）
///
/// # Errors
///
/// - [`SshError::Disabled`]：未启用 `ssh` feature；
/// - [`SshError::InvalidSpec`]：参数非法；
/// - [`SshError::Connect`] / [`SshError::Handshake`] / [`SshError::HostKeyMismatch`]：
///   连接、握手或指纹校验失败。
#[cfg(feature = "ssh")]
pub async fn probe(spec: &SshTargetSpec) -> Result<HostKeyInfo, SshError> {
    imp::probe(spec).await
}

/// 只握手取主机密钥指纹（`ssh` feature 关闭时的占位实现）
///
/// # Errors
///
/// 恒返回 [`SshError::Disabled`]。
#[cfg(not(feature = "ssh"))]
pub async fn probe(_spec: &SshTargetSpec) -> Result<HostKeyInfo, SshError> {
    Err(SshError::Disabled)
}

/// 认证并打开一个交互式会话（PTY + shell）
///
/// # Errors
///
/// 同 [`probe`]，另有 [`SshError::AuthFailed`] / [`SshError::Rejected`] /
/// [`SshError::Credential`]。
#[cfg(feature = "ssh")]
pub async fn open(spec: &SshTargetSpec, cols: u32, rows: u32) -> Result<SshSession, SshError> {
    imp::open(spec, cols, rows).await
}

/// 认证并打开一个交互式会话（`ssh` feature 关闭时的占位实现）
///
/// # Errors
///
/// 恒返回 [`SshError::Disabled`]。
#[cfg(not(feature = "ssh"))]
pub async fn open(_spec: &SshTargetSpec, _cols: u32, _rows: u32) -> Result<SshSession, SshError> {
    Err(SshError::Disabled)
}

