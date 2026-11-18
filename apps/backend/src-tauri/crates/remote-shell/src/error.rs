//! SSH 传输错误
//!
//! 统一在传输层收口，上层（service / api）只做「错误 → HTTP 状态码」映射，
//! 不再判断底层是 DNS 失败还是认证失败。

use thiserror::Error;

/// 远程终端传输错误
#[derive(Debug, Error)]
pub enum SshError {
    /// 编译时未启用 `ssh` feature
    #[error("SSH 传输未编译进来（请为 remote-shell 开启 ssh feature）")]
    Disabled,

    /// 输入参数非法（空主机 / 空用户名 / 端口为 0 / 私钥与密码同时缺省）
    #[error("参数非法：{0}")]
    InvalidSpec(String),

    /// TCP 连接失败（DNS / 拒绝 / 超时）
    #[error("连接失败：{0}")]
    Connect(String),

    /// SSH 握手失败（算法协商、版本、密钥交换）
    #[error("SSH 握手失败：{0}")]
    Handshake(String),

    /// 主机密钥校验未通过（指纹与记录不一致，或用户未确认新指纹）
    #[error("主机密钥校验失败：{0}")]
    HostKeyMismatch(String),

    /// 认证失败（密码错误 / 私钥不被接受 / 需要交互式键盘）
    #[error("认证失败：{0}")]
    AuthFailed(String),

    /// 平台拒绝开通道 / 申请 PTY / 申请 shell
    #[error("远端拒绝：{0}")]
    Rejected(String),

    /// 会话已关闭或写入失败
    #[error("会话已断开：{0}")]
    Closed(String),

    /// 凭证无法解析（PEM 损坏、口令错误、格式不支持）
    #[error("凭证不可用：{0}")]
    Credential(String),

    /// 其余底层错误
    #[error("SSH 错误：{0}")]
    Other(String),
}

impl SshError {
    /// 是否属于「配置 / 凭证」类错误（前端提示需要改配置，而不是重试）
    pub fn is_configuration(&self) -> bool {
        matches!(
            self,
            Self::InvalidSpec(_) | Self::Credential(_) | Self::HostKeyMismatch(_)
        )
    }
}

#[cfg(feature = "ssh")]
impl From<russh::Error> for SshError {
    fn from(error: russh::Error) -> Self {
        Self::Other(error.to_string())
    }
}
