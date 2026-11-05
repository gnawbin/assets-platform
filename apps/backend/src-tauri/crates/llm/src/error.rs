//! LLM 错误类型
//!
//! 错误分十类，目的是让上层（Router / 界面）能区分三种完全不同的处置方式：
//! 1. 「再试一次可能就好」→ [`LlmError::is_retryable`]；
//! 2. 「配置写错了，重试也没用」→ [`LlmError::is_auth_error`] / [`LlmError::is_context_length`]；
//! 3. 「能力不支持」→ [`LlmError::Unsupported`]。
//!
//! 风格对齐 `crates/vector/src/error.rs`（`kind()` / `hint()` / `is_retryable()`）。

use std::time::Duration;

/// LLM 调用结果
pub type LlmResult<T> = Result<T, LlmError>;

/// LLM 错误
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LlmError {
    /// 配置缺失或非法（端点、密钥、默认模型）
    #[error("配置错误：{0}")]
    Config(String),
    /// 鉴权失败（401 / 403）
    #[error("鉴权失败：{0}")]
    Auth(String),
    /// 触发限流（429 / Anthropic `overloaded_error`）
    #[error("触发限流：{message}")]
    RateLimited {
        message: String,
        /// 服务端建议的重试间隔（`retry-after`）
        retry_after: Option<Duration>,
    },
    /// 上下文超长
    #[error("上下文超长：{message}")]
    ContextLength {
        /// 模型窗口上限（能解析到时填充）
        limit: Option<i32>,
        message: String,
    },
    /// 请求非法（400 / 422），重试无意义
    #[error("请求非法（HTTP {status}）：{message}")]
    InvalidRequest { status: u16, message: String },
    /// 服务端错误（5xx）
    #[error("服务端错误（HTTP {status}）：{message}")]
    Server { status: u16, message: String },
    /// 传输层错误（DNS / TLS / 超时 / 连接重置）
    #[error("网络错误：{0}")]
    Transport(String),
    /// 响应体解析失败
    #[error("响应解析失败：{0}")]
    Decode(String),
    /// 当前厂商 / 模型不支持该能力
    #[error("能力不支持：{0}")]
    Unsupported(String),
    /// 调用被上游取消 / 流被中断
    #[error("调用已取消")]
    Cancelled,
}

impl LlmError {
    /// 构造配置错误
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    /// 构造鉴权错误
    pub fn auth(message: impl Into<String>) -> Self {
        Self::Auth(message.into())
    }

    /// 构造限流错误
    pub fn rate_limited(message: impl Into<String>, retry_after: Option<Duration>) -> Self {
        Self::RateLimited {
            message: message.into(),
            retry_after,
        }
    }

    /// 构造上下文超长错误
    pub fn context_length(message: impl Into<String>, limit: Option<i32>) -> Self {
        Self::ContextLength {
            limit,
            message: message.into(),
        }
    }

    /// 构造请求非法错误
    pub fn invalid_request(status: u16, message: impl Into<String>) -> Self {
        Self::InvalidRequest {
            status,
            message: message.into(),
        }
    }

    /// 构造服务端错误
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

    /// 构造能力不支持错误
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::Unsupported(message.into())
    }
}

impl LlmError {
    /// 是否值得原样重试（传输抖动 / 限流 / 服务端 5xx）
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Transport(_) | Self::RateLimited { .. } | Self::Server { .. }
        )
    }

    /// 是否鉴权类错误（配置写错，重试无用）
    pub fn is_auth_error(&self) -> bool {
        matches!(self, Self::Auth(_))
    }

    /// 是否上下文超长
    pub fn is_context_length(&self) -> bool {
        matches!(self, Self::ContextLength { .. })
    }

    /// 服务端建议的重试间隔
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// 供界面分类展示的稳定标识
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Config(_) => "config",
            Self::Auth(_) => "auth",
            Self::RateLimited { .. } => "rate_limited",
            Self::ContextLength { .. } => "context_length",
            Self::InvalidRequest { .. } => "invalid_request",
            Self::Server { .. } => "server",
            Self::Transport(_) => "transport",
            Self::Decode(_) => "decode",
            Self::Unsupported(_) => "unsupported",
            Self::Cancelled => "cancelled",
        }
    }

    /// 面向用户的排查建议
    pub fn hint(&self) -> &'static str {
        match self {
            Self::Config(_) => "检查厂商 base_url、默认模型与 API Key 是否已正确配置",
            Self::Auth(_) => "API Key 无效或权限不足，请在 LLM 厂商配置中重新填写",
            Self::RateLimited { .. } => "触发厂商限流，稍后重试或提高配额 / 增加备用厂商",
            Self::ContextLength { .. } => "上下文超出模型窗口，请缩短输入或改用长窗口模型",
            Self::InvalidRequest { .. } => "请求参数不被接受，检查模型名、max_tokens 与工具定义",
            Self::Server { .. } => "厂商服务端异常，可稍后重试或切换到其它厂商",
            Self::Transport(_) => "检查网络连通性、代理设置与超时时间后重试",
            Self::Decode(_) => "响应格式与预期不符，可能是厂商接口版本变化或返回了错误页",
            Self::Unsupported(_) => "该厂商 / 模型不支持此能力，请改用支持它的模型",
            Self::Cancelled => "调用被取消，无需处理",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_transient_errors_are_retryable() {
        assert!(LlmError::transport("connection reset").is_retryable());
        assert!(LlmError::rate_limited("slow down", Some(Duration::from_secs(3))).is_retryable());
        assert!(LlmError::server(503, "overloaded").is_retryable());

        assert!(!LlmError::auth("bad key").is_retryable());
        assert!(!LlmError::invalid_request(400, "bad model").is_retryable());
        assert!(!LlmError::context_length("too long", Some(8192)).is_retryable());
        assert!(!LlmError::unsupported("no embedding").is_retryable());
        assert!(!LlmError::Cancelled.is_retryable());
    }

    #[test]
    fn retry_after_is_exposed_for_rate_limit() {
        let error = LlmError::rate_limited("slow down", Some(Duration::from_secs(7)));
        assert_eq!(error.retry_after(), Some(Duration::from_secs(7)));
        assert_eq!(LlmError::transport("x").retry_after(), None);
    }

    #[test]
    fn every_variant_carries_kind_and_hint() {
        let cases = [
            LlmError::config("c"),
            LlmError::auth("a"),
            LlmError::rate_limited("r", None),
            LlmError::context_length("ctx", Some(4096)),
            LlmError::invalid_request(400, "i"),
            LlmError::server(500, "s"),
            LlmError::transport("t"),
            LlmError::decode("d"),
            LlmError::unsupported("u"),
            LlmError::Cancelled,
        ];
        for error in &cases {
            assert!(!error.kind().is_empty());
            assert!(!error.hint().is_empty());
            assert!(!error.to_string().is_empty());
        }
        assert!(LlmError::auth("x").is_auth_error());
        assert!(LlmError::context_length("x", None).is_context_length());
    }
}


