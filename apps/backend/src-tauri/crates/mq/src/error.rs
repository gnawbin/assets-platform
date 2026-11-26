//! 消息总线错误模型。
//!
//! 不依赖 `api` / `service`，由使用方在边界处按需转换（与 `sandbox` / `vector` 一致）。

/// 消息总线统一错误
#[derive(Debug, thiserror::Error)]
pub enum MqError {
    /// `PULSAR_ENABLED` 未开启且调用方仍尝试使用总线
    #[error("消息总线未启用（PULSAR_ENABLED 未设置）")]
    Disabled,

    /// 编译期没有启用所需 feature（`memory` / `pulsar`），或驱动尚未接入
    #[error("能力未编译：请启用 crate feature `pulsar`")]
    FeatureDisabled,

    /// 配置非法（环境变量、订阅名、topic 等）
    #[error("配置非法：{0}")]
    Config(String),

    /// 建连失败（broker 不可达、认证失败、TLS 失败等）
    #[error("建连失败：{0}")]
    Connect(String),

    /// 发布失败
    #[error("发布失败（topic={topic}）：{detail}")]
    Publish {
        /// 完整 topic URI
        topic: String,
        /// 驱动返回的错误描述（不叫 `source`，避免被 thiserror 当成 `Error::source`）
        detail: String,
    },

    /// 订阅失败
    #[error("订阅失败（topic={topic}）：{detail}")]
    Subscribe {
        /// 完整 topic URI
        topic: String,
        /// 驱动返回的错误描述
        detail: String,
    },

    /// 信封序列化 / 反序列化失败
    #[error("信封序列化 / 反序列化失败：{0}")]
    Codec(String),

    /// topic 非法（命名、URI、长度等）
    #[error("topic 非法：{0}")]
    InvalidTopic(String),

    /// 同一 `Bus` 内同订阅名重复订阅
    #[error("订阅已存在（同进程重复订阅）：{subscription}")]
    AlreadySubscribed {
        /// 订阅名
        subscription: String,
    },

    /// 同一订阅名但类型 / 位点不同
    #[error("订阅配置冲突（同名但类型 / 位点不同）：{subscription}")]
    SubscriptionConflict {
        /// 订阅名
        subscription: String,
    },

    /// 总线已关闭
    #[error("消息总线已关闭")]
    Closed,
}

/// 便捷结果别名
pub type MqResult<T> = Result<T, MqError>;
