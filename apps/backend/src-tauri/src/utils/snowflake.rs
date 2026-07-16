use std::sync::OnceLock;
use twitter_snowflake::Snowflake;

/// Snowflake ID 生成器
///
/// 基于 snowflake 算法生成全局唯一的 64 位整数 ID。
/// 使用单例模式，全局只需初始化一次。
///
/// # 示例
///
/// ```rust
/// use assetsplatform_lib::utils::snowflake;
///
/// let id = snowflake::next_id();
/// println!("生成的 Snowflake ID: {}", id);
/// ```
pub struct SnowflakeGenerator {
    generator: Snowflake,
}

impl SnowflakeGenerator {
    /// 创建一个新的 Snowflake ID 生成器
    ///
    /// # 参数
    /// * `worker_id` - 机器 ID (0-31)
    /// * `datacenter_id` - 数据中心 ID (0-31)
    pub fn new(worker_id: u64) -> Self {
        Self {
            generator: Snowflake::new(worker_id).unwrap(),
        }
    }

    /// 生成下一个唯一 ID
    pub fn next_id(&mut self) -> u64 {
        self.generator.generate().unwrap()
    }
}

/// 全局 Snowflake 生成器实例
static SNOWFLAKE_INSTANCE: OnceLock<std::sync::Mutex<SnowflakeGenerator>> = OnceLock::new();

/// 获取全局 Snowflake 生成器（懒加载初始化）
fn get_generator() -> &'static std::sync::Mutex<SnowflakeGenerator> {
    SNOWFLAKE_INSTANCE.get_or_init(|| {
        // 从环境变量读取 worker_id 和 datacenter_id，默认均为 0
        let worker_id = std::env::var("SNOWFLAKE_WORKER_ID")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        //  let datacenter_id = std::env::var("SNOWFLAKE_DATACENTER_ID")
        //    .ok()
        //  .and_then(|v| v.parse::<u64>().ok())
        //.unwrap_or(0);

        std::sync::Mutex::new(SnowflakeGenerator::new(worker_id))
    })
}

/// 生成下一个全局唯一的 Snowflake ID
///
/// # 示例
///
/// ```rust
/// let id = assetsplatform_lib::utils::snowflake::next_id();
/// assert!(id > 0);
/// ```
pub fn next_id() -> u64 {
    let generator = get_generator();
    let mut guard = generator.lock().expect("获取 Snowflake 生成器锁失败");
    guard.next_id()
}

/// 使用指定的 worker_id 和 datacenter_id 重新初始化 Snowflake 生成器
///
/// 注意：此函数会覆盖全局实例，仅在应用启动初期调用一次。
pub fn init_with(worker_id: u64) {
    let generator = SnowflakeGenerator::new(worker_id);
    let instance =
        SNOWFLAKE_INSTANCE.get_or_init(|| std::sync::Mutex::new(SnowflakeGenerator::new(0)));
    let mut guard = instance.lock().expect("获取 Snowflake 生成器锁失败");
    *guard = generator;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next_id_generates_unique_ids() {
        let mut ids = std::collections::HashSet::new();
        for _ in 0..1000 {
            let id = next_id();
            assert!(id > 0, "生成的 ID 必须大于 0");
            assert!(ids.insert(id), "ID {} 重复生成", id);
        }
    }

    #[test]
    fn test_next_id_is_monotonic() {
        let mut prev = next_id();
        for _ in 0..100 {
            let current = next_id();
            assert!(
                current > prev,
                "Snowflake ID 应单调递增: {} >= {}",
                current,
                prev
            );
            prev = current;
        }
    }

    #[test]
    fn test_init_with_custom_params() {
        init_with(1);
        let id = next_id();
        assert!(id > 0);
    }

    #[test]
    fn test_generator_new() {
        let mut gen = SnowflakeGenerator::new(1);
        let id = gen.next_id();
        assert!(id > 0);
    }
}
