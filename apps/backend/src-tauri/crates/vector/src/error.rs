//! 向量存储错误类型
//!
//! 错误分六类，目的是让上层（界面 / 调度器）能区分「配置写错」「能力不支持」
//! 「网络抖动」三种完全不同的处置方式，而不是笼统的一个 `String`。

/// 向量存储操作结果
pub type VectorResult<T> = Result<T, VectorError>;

/// 向量存储错误
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VectorError {
    /// 配置缺失或非法（端点、凭证、表名、维度等）
    #[error("配置错误：{0}")]
    Config(String),
    /// 当前后端不支持该操作（例：Redis 没装 RediSearch 模块却要求 ANN 索引）
    #[error("能力不支持：{0}")]
    Unsupported(String),
    /// 驱动 / 服务端返回的错误
    #[error("后端错误：{0}")]
    Backend(String),
    /// 表结构不匹配（维度、列名、索引类型）
    #[error("结构不匹配：{0}")]
    Schema(String),
    /// 目标表不存在
    #[error("表不存在：{0}")]
    TableNotFound(String),
    /// 连接超时、DNS 失败、TLS 握手失败等传输层问题
    #[error("连接失败：{0}")]
    Transport(String),
}

impl VectorError {
    /// 构造配置错误
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    /// 构造驱动错误
    pub fn backend(message: impl Into<String>) -> Self {
        Self::Backend(message.into())
    }

    /// 构造传输错误
    pub fn transport(message: impl Into<String>) -> Self {
        Self::Transport(message.into())
    }

    /// 构造能力不支持错误
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::Unsupported(message.into())
    }

    /// 构造结构不匹配错误
    pub fn schema(message: impl Into<String>) -> Self {
        Self::Schema(message.into())
    }

    /// 是否值得原样重试（只有传输层错误是「再试一次可能就好」）
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Transport(_))
    }

    /// 面向运维人员的排查建议（界面直接展示，避免只给一句英文报错）
    pub fn hint(&self) -> &'static str {
        match self {
            Self::Config(_) => "检查端点格式、账号密码、表名与维度是否与目标库一致",
            Self::Unsupported(_) => "确认服务端组件/模块版本，或改用其它向量后端",
            Self::Backend(_) => "查看服务端日志与采集账号权限（需 SELECT / 读命令）",
            Self::Schema(_) => "对齐建表参数：维度、距离度量、向量列名必须与已有表一致",
            Self::TableNotFound(_) => "先调用 ensure_table 建表，或确认表名 / 命名空间拼写",
            Self::Transport(_) => "检查网络连通性、防火墙、TLS 证书与超时设置后重试",
        }
    }

    /// 供界面分类展示的稳定标识
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Config(_) => "config",
            Self::Unsupported(_) => "unsupported",
            Self::Backend(_) => "backend",
            Self::Schema(_) => "schema",
            Self::TableNotFound(_) => "table_not_found",
            Self::Transport(_) => "transport",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_transport_errors_are_retryable() {
        assert!(VectorError::transport("connection reset").is_retryable());
        assert!(!VectorError::config("bad endpoint").is_retryable());
        assert!(!VectorError::backend("boom").is_retryable());
    }

    #[test]
    fn every_variant_carries_kind_and_hint() {
        let cases = [
            VectorError::config("c"),
            VectorError::unsupported("u"),
            VectorError::backend("b"),
            VectorError::schema("s"),
            VectorError::TableNotFound("t".to_string()),
            VectorError::transport("tr"),
        ];
        for error in &cases {
            assert!(!error.kind().is_empty());
            assert!(!error.hint().is_empty());
            // Display 必须能显示，且带中文分类前缀
            assert!(error.to_string().contains('：'), "{error}");
        }
    }
}
