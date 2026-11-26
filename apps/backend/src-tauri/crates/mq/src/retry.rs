//! 指数退避纯函数（无随机、无 IO，便于单测与复用）。
//!
//! 第 `attempt` 次失败后的等待时长：`min(base * 2^attempt, max)`。
//! 调用方负责决定最大重试次数（`PULSAR_PUBLISH_RETRY_MAX`）。

use std::time::Duration;

/// 缺省基础等待时长
pub const DEFAULT_BASE_DELAY: Duration = Duration::from_millis(50);

/// 缺省封顶等待时长
pub const DEFAULT_MAX_DELAY: Duration = Duration::from_secs(2);

/// 计算第 `attempt` 次失败后的退避时长。
///
/// `attempt` 从 0 开始；超过 31 后按 31 处理，避免指数溢出。
#[must_use]
pub fn backoff_delay(attempt: u32, base: Duration, max: Duration) -> Duration {
    let factor = 2_u32.saturating_pow(attempt.min(31));
    base.saturating_mul(factor).min(max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_exponentially_and_caps() {
        let base = Duration::from_millis(50);
        let max = Duration::from_millis(500);
        assert_eq!(backoff_delay(0, base, max), Duration::from_millis(50));
        assert_eq!(backoff_delay(1, base, max), Duration::from_millis(100));
        assert_eq!(backoff_delay(2, base, max), Duration::from_millis(200));
        assert_eq!(backoff_delay(3, base, max), Duration::from_millis(400));
        assert_eq!(backoff_delay(4, base, max), Duration::from_millis(500));
        assert_eq!(backoff_delay(31, base, max), Duration::from_millis(500));
        assert_eq!(
            backoff_delay(u32::MAX, base, max),
            Duration::from_millis(500)
        );
    }
}
