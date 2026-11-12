//! 沙箱错误类型
//!
//! 分十二类，目的是让上层能区分三种完全不同的处置方式（风格对齐
//! `crates/llm/src/error.rs` 与 `crates/vector/src/error.rs`）：
//! 1. 「再试一次可能就好」→ [`SandboxError::is_retryable`]；
//! 2. 「配置写错了，重试也没用」→ `Config` / `Auth` / `NotFound` / `Conflict`；
//! 3. 「能力不支持 / 已取消」→ `Unsupported` / `Cancelled`。

use std::time::Duration;

/// 沙箱调用结果
pub type SandboxResult<T> = Result<T, SandboxError>;

/// 沙箱错误
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SandboxError {
    /// 配置缺失或非法（base_url / api_key / template_id）
    #[error("配置错误：{0}")]
    Config(String),
    /// 鉴权失败（401 / 403）
    #[error("鉴权失败：{0}")]
    Auth(String),
    /// 沙箱 / 模板 / 卷不存在（404）
    #[error("资源不存在：{0}")]
    NotFound(String),
    /// 状态冲突（409：resume 一个已在运行的沙箱、kill 一个已销毁的沙箱）
    #[error("状态冲突：{0}")]
    Conflict(String),
    /// 触发限流（429）
    #[error("触发限流：{message}")]
    RateLimited {
        /// 服务端返回的错误信息
        message: String,
        /// 服务端建议的重试间隔（`Retry-After`）
        retry_after: Option<Duration>,
    },
    /// 请求非法（400 / 422），重试无意义
    #[error("请求非法（HTTP {status}）：{message}")]
    InvalidRequest {
        /// HTTP 状态码
        status: u16,
        /// 服务端返回的错误信息
        message: String,
    },
    /// 服务端错误（5xx）
    #[error("服务端错误（HTTP {status}）：{message}")]
    Server {
        /// HTTP 状态码
        status: u16,
        /// 服务端返回的错误信息
        message: String,
    },
    /// 传输层错误（DNS / TLS / 连接重置）
    #[error("网络错误：{0}")]
    Transport(String),
    /// 响应体解析失败
    #[error("响应解析失败：{0}")]
    Decode(String),
    /// 执行超时
    #[error("执行超时（{0}s）")]
    Timeout(u64),
    /// 当前实现不支持该能力（含 `SANDBOX_ENABLED=false`）
    #[error("能力不支持：{0}")]
    Unsupported(String),
    /// 调用被取消
    #[error("调用已取消")]
    Cancelled,
}

impl SandboxError {
    /// 构造配置错误
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    /// 构造鉴权错误
    pub fn auth(message: impl Into<String>) -> Self {
        Self::Auth(message.into())
    }

    /// 构造「资源不存在」
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    /// 构造「状态冲突」
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }

    /// 构造限流错误
    pub fn rate_limited(message: impl Into<String>, retry_after: Option<Duration>) -> Self {
        Self::RateLimited {
            message: message.into(),
            retry_after,
        }
    }

    /// 构造「请求非法」
    pub fn invalid_request(status: u16, message: impl Into<String>) -> Self {
        Self::InvalidRequest {
            status,
            message: message.into(),
        }
    }

    /// 构造「服务端错误」
    pub fn server(status: u16, message: impl Into<String>) -> Self {
        Self::Server {
            status,
            message: message.into(),
        }
    }

    /// 构造传输层错误
    pub fn transport(message: impl Into<String>) -> Self {
        Self::Transport(message.into())
    }

    /// 构造解析失败错误
    pub fn decode(message: impl Into<String>) -> Self {
        Self::Decode(message.into())
    }

    /// 构造执行超时错误
    pub fn timeout(secs: u64) -> Self {
        Self::Timeout(secs)
    }

    /// 构造「能力不支持」
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::Unsupported(message.into())
    }

    /// 是否值得原样重试（传输抖动 / 限流 / 5xx / 超时）
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Transport(_) | Self::RateLimited { .. } | Self::Server { .. } | Self::Timeout(_)
        )
    }

    /// 服务端建议的重试间隔
    #[must_use]
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// 供前端分类展示的稳定标识
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Config(_) => "config",
            Self::Auth(_) => "auth",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::RateLimited { .. } => "rate_limited",
            Self::InvalidRequest { .. } => "invalid_request",
            Self::Server { .. } => "server",
            Self::Transport(_) => "transport",
            Self::Decode(_) => "decode",
            Self::Timeout(_) => "timeout",
            Self::Unsupported(_) => "unsupported",
            Self::Cancelled => "cancelled",
        }
    }

    /// 面向用户的排查建议（一次一句，可直接展示）
    #[must_use]
    pub fn hint(&self) -> &'static str {
        match self {
            Self::Config(_) => "检查 [sandbox] 段的 base_url / api_key / template_id 是否已正确配置",
            Self::Auth(_) => "CubeAPI 密钥无效或权限不足，请更新 SANDBOX_API_KEY",
            Self::NotFound(_) => "沙箱或模板已不存在，请重新创建会话或更换 SANDBOX_TEMPLATE_ID",
            Self::Conflict(_) => "沙箱状态已变化，请刷新会话状态后重试",
            Self::RateLimited { .. } => "触发 CubeAPI 限流，稍后重试或提升配额",
            Self::InvalidRequest { .. } => "请求参数不被接受，检查代码长度 / 文件大小 / 模板能力",
            Self::Server { .. } => "CubeSandbox 服务端异常，稍后重试或切换节点",
            Self::Transport(_) => "检查到 CubeAPI / CubeProxy 的网络连通性与 SANDBOX_BASE_URL",
            Self::Decode(_) => "响应格式与预期不符，可能 CubeSandbox 版本升级导致协议变化",
            Self::Timeout(_) => "执行超时，缩短任务或调大 SANDBOX_TIMEOUT_SECS",
            Self::Unsupported(_) => "该能力未启用（SANDBOX_ENABLED=false）或当前实现不支持",
            Self::Cancelled => "调用被取消，无需处理",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_variants() -> Vec<SandboxError> {
        vec![
            SandboxError::config("c"),
            SandboxError::auth("a"),
            SandboxError::not_found("n"),
            SandboxError::conflict("conf"),
            SandboxError::rate_limited("r", Some(Duration::from_secs(3))),
            SandboxError::invalid_request(400, "i"),
            SandboxError::server(500, "s"),
            SandboxError::transport("t"),
            SandboxError::decode("d"),
            SandboxError::timeout(60),
            SandboxError::unsupported("u"),
            SandboxError::Cancelled,
        ]
    }

    #[test]
    fn every_variant_carries_kind_and_hint() {
        for error in all_variants() {
            assert!(!error.kind().is_empty(), "{error:?} 缺少 kind");
            assert!(!error.hint().is_empty(), "{error:?} 缺少 hint");
            assert!(!error.to_string().is_empty(), "{error:?} 缺少 Display");
        }
    }

    #[test]
    fn only_transient_errors_are_retryable() {
        let mut retryable = all_variants()
            .into_iter()
            .filter(SandboxError::is_retryable)
            .map(|e| e.kind())
            .collect::<Vec<_>>();
        retryable.sort_unstable();
        assert_eq!(retryable, ["rate_limited", "server", "timeout", "transport"]);
    }

    #[test]
    fn retry_after_is_exposed_for_rate_limit_only() {
        let error = SandboxError::rate_limited("slow", Some(Duration::from_secs(7)));
        assert_eq!(error.retry_after(), Some(Duration::from_secs(7)));
        assert_eq!(SandboxError::transport("x").retry_after(), None);
    }
}
