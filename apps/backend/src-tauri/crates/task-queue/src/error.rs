//! 任务错误与 apalis 错误语义的映射
//!
//! apalis 1.2 的错误约定（`apalis_core::error`）：
//! - 返回普通错误 → 视为**可重试**（按任务的 `max_attempts` 重试）；
//! - 返回 [`AbortError`] → **立即终止**，不再重试；
//! - `RetryAfterError` 用于指定退避时长（本 crate 暂不使用，交给 worker 层的重试策略）。
//!
//! 因此本模块只做一件事：把业务语义（参数非法 / 依赖抖动 / 确定性失败）翻译成上面两种行为。

use apalis::prelude::{AbortError, BoxDynError};

/// 任务执行错误
#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    /// 参数非法（租户为空、引用格式错误、扩展名不支持）：重试没有意义
    #[error("任务参数非法: {0}")]
    Invalid(String),
    /// 依赖暂不可用（S3 抖动、侧车超时、模型 OOM）：可重试
    #[error("依赖暂不可用: {0}")]
    Transient(String),
    /// 确定性失败（解析实现明确返回不支持/损坏）：不重试
    #[error("解析失败: {0}")]
    Fatal(String),
}

impl TaskError {
    /// 是否可重试（仅 [`TaskError::Transient`]）
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        matches!(self, Self::Transient(_))
    }

    /// 转成 worker 的返回值：可重试 → 普通错误；其余 → [`AbortError`]
    #[must_use]
    pub fn into_box(self) -> BoxDynError {
        match self {
            Self::Transient(message) => Box::new(Self::Transient(message)),
            Self::Invalid(message) => Box::new(AbortError::new(Self::Invalid(message))),
            Self::Fatal(message) => Box::new(AbortError::new(Self::Fatal(message))),
        }
    }

    /// 把 doc-parser / parser 返回的错误文本归类
    ///
    /// 这两个后端把「格式不支持」和「临时失败」都表达成 `String`，只能按关键字区分：
    /// 命中已知的确定性错误 → [`TaskError::Invalid`]（不重试）；其余一律按可重试处理，
    /// 宁可多重试几次（`max_attempts` 有上限），也不要把可恢复故障直接判死。
    pub fn from_parse_error(message: impl Into<String>) -> Self {
        /// 已知的确定性错误片段（与 `crates/parser` / Python 侧车的文案对应）
        const PERMANENT: [&str; 6] = [
            "不支持的文件类型",
            "本地解析尚未支持",
            "文件不存在",
            "not supported",
            "invalid",
            "unsupported",
        ];

        let message = message.into();
        let lower = message.to_ascii_lowercase();
        if PERMANENT
            .iter()
            .any(|needle| lower.contains(&needle.to_ascii_lowercase()))
        {
            Self::Invalid(message)
        } else {
            Self::Transient(message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_only_transient_is_retryable() {
        assert!(TaskError::Transient("S3 超时".into()).is_retryable());
        assert!(!TaskError::Invalid("tenant 为空".into()).is_retryable());
        assert!(!TaskError::Fatal("解析器返回损坏".into()).is_retryable());
    }

    #[test]
    fn test_transient_stays_retryable_as_plain_error() {
        let err = TaskError::Transient("S3 超时".into()).into_box();
        assert!(
            err.downcast_ref::<TaskError>().is_some(),
            "❌ 可重试错误不应被包成 AbortError，实际：{err}"
        );
        assert!(
            err.downcast_ref::<AbortError>().is_none(),
            "❌ 可重试错误被误判为不可重试"
        );
    }

    #[test]
    fn test_invalid_and_fatal_abort() {
        for err in [
            TaskError::Invalid("tenant 为空".into()),
            TaskError::Fatal("文件损坏".into()),
        ] {
            let label = err.to_string();
            let boxed = err.into_box();
            assert!(
                boxed.downcast_ref::<AbortError>().is_some(),
                "❌ 不可恢复错误必须包成 AbortError（{label}）"
            );
        }
    }

    #[test]
    fn test_from_parse_error_classifies_permanent_errors() {
        assert!(matches!(
            TaskError::from_parse_error("不支持的文件类型: .exe"),
            TaskError::Invalid(_)
        ));
        assert!(matches!(
            TaskError::from_parse_error("本地解析尚未支持 .mp4（video），请切换 remote 后端"),
            TaskError::Invalid(_)
        ));
        assert!(
            matches!(
                TaskError::from_parse_error("doc-parser 请求超时"),
                TaskError::Transient(_)
            ),
            "❌ 未知错误应按可重试处理"
        );
    }
}
