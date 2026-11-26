//! 任务队列配置
//!
//! 全部来自环境变量，**默认值保证「未开启 = 行为与改造前一致」**：
//! `TASK_QUEUE_MODE` 缺省为 `inline`，此时既不入队、也不启动 worker。
//!
//! 命名与容错策略沿用现有约定（参考 `service::doc_parser::parse_backend_value`）：
//! 全大写 + `_SECS` / `_CONCURRENCY` 后缀，非法值一律回落默认值而不是报错。

/// 总开关取值
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 保持改造前行为：请求内同步解析，不使用队列
    Inline,
    /// 正常入队；队列不可用直接失败（不回退到长连接同步解析）
    Queue,
    /// 入队失败回退 inline，并打 WARN（灰度首周用）
    QueueFailOpen,
}

impl Mode {
    /// 配置值 / 日志用的字符串表示
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::Queue => "queue",
            Self::QueueFailOpen => "queue_fail_open",
        }
    }

    /// 解析配置值（大小写 / 首尾空白容错，非法值回落 [`Mode::Inline`]）
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "queue" => Self::Queue,
            "queue_fail_open" => Self::QueueFailOpen,
            _ => Self::Inline,
        }
    }

    /// 是否入队（`inline` 之外的两种模式都入队）
    #[must_use]
    pub const fn enqueues(self) -> bool {
        !matches!(self, Self::Inline)
    }

    /// 入队失败时是否允许回退到同步解析
    #[must_use]
    pub const fn fail_open(self) -> bool {
        matches!(self, Self::QueueFailOpen)
    }
}

/// 队列运行配置
#[derive(Debug, Clone)]
pub struct TaskQueueConfig {
    /// 总开关
    pub mode: Mode,
    /// `parse` 队列每实例 worker 数
    pub parse_concurrency: usize,
    /// 单次领取任务数（`parse` 队列固定 1，见设计文档 §10.3）
    pub batch_size: usize,
    /// 是否启用视频/音频队列（桌面形态恒为 false）
    pub video_enabled: bool,
    /// 是否挂载 apalis-board 子路由（P2）
    pub board_enabled: bool,
    /// 实例标识：用于 worker 命名与日志定位
    pub instance_id: String,
}

impl Default for TaskQueueConfig {
    fn default() -> Self {
        Self {
            mode: Mode::Inline,
            parse_concurrency: 2,
            batch_size: 1,
            video_enabled: false,
            board_enabled: false,
            instance_id: "local".to_string(),
        }
    }
}

impl TaskQueueConfig {
    /// 从环境变量加载（缺省 / 非法值一律回落默认）
    #[must_use]
    pub fn from_env() -> Self {
        let instance_id = std::env::var("TASK_QUEUE_INSTANCE_ID")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .or_else(|| {
                std::env::var("SNOWFLAKE_WORKER_ID")
                    .ok()
                    .map(|v| format!("w{}", v.trim()))
            })
            .unwrap_or_else(|| "local".to_string());

        Self {
            mode: Mode::parse(&std::env::var("TASK_QUEUE_MODE").unwrap_or_default()),
            parse_concurrency: env_usize("TASK_QUEUE_PARSE_CONCURRENCY", 2, 1, 8),
            batch_size: env_usize("TASK_QUEUE_BATCH_SIZE", 1, 1, 16),
            video_enabled: env_flag("TASK_QUEUE_VIDEO_ENABLED", false),
            board_enabled: env_flag("TASK_QUEUE_BOARD_ENABLED", false),
            instance_id,
        }
    }

    /// 是否启用队列（生产/消费两侧都必须先看它）
    #[must_use]
    pub const fn queue_enabled(&self) -> bool {
        self.mode.enqueues()
    }
}

/// 读取整数型环境变量：缺失 / 非法 → `default`；超出 `[min, max]` → 夹逼
fn env_usize(name: &str, default: usize, min: usize, max: usize) -> usize {
    parse_usize(std::env::var(name).ok().as_deref(), default, min, max)
}

/// 纯函数形式的整数解析（便于单测，不依赖环境变量）
fn parse_usize(raw: Option<&str>, default: usize, min: usize, max: usize) -> usize {
    raw.and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|v| *v >= min)
        .map_or(default, |v| v.min(max))
}

/// 读取布尔型环境变量：`1/true/yes/on` 为真，其余（含缺失）取默认
fn env_flag(name: &str, default: bool) -> bool {
    std::env::var(name).map_or(default, |raw| {
        matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_parse_falls_back_to_inline() {
        assert_eq!(
            Mode::parse(""),
            Mode::Inline,
            "❌ 未配置时必须保持改造前行为"
        );
        assert_eq!(Mode::parse(" QUEUE "), Mode::Queue, "❌ 应容忍大小写与空白");
        assert_eq!(Mode::parse("queue_fail_open"), Mode::QueueFailOpen);
        assert_eq!(Mode::parse("typo"), Mode::Inline, "❌ 非法值应回落 inline");
    }

    #[test]
    fn test_mode_helpers() {
        assert!(!Mode::Inline.enqueues());
        assert!(Mode::Queue.enqueues());
        assert!(Mode::QueueFailOpen.enqueues());
        assert!(!Mode::Queue.fail_open());
        assert!(Mode::QueueFailOpen.fail_open());
        assert_eq!(Mode::Queue.as_str(), "queue");
    }

    #[test]
    fn test_parse_usize_clamps_and_defaults() {
        assert_eq!(parse_usize(None, 2, 1, 8), 2, "❌ 缺失应取默认值");
        assert_eq!(parse_usize(Some("abc"), 2, 1, 8), 2, "❌ 非法应取默认值");
        assert_eq!(parse_usize(Some(" 5 "), 2, 1, 8), 5);
        assert_eq!(parse_usize(Some("0"), 2, 1, 8), 2, "❌ 低于下限应回落默认");
        assert_eq!(parse_usize(Some("99"), 2, 1, 8), 8, "❌ 高于上限应夹逼");
    }

    #[test]
    fn test_default_is_inline_and_single_batch() {
        let cfg = TaskQueueConfig::default();
        assert_eq!(cfg.mode, Mode::Inline);
        assert!(!cfg.queue_enabled());
        assert_eq!(cfg.batch_size, 1, "❌ parse 队列默认批大小为 1（公平性）");
        assert!(!cfg.video_enabled, "❌ 桌面/默认不启用视频队列");
    }
}
