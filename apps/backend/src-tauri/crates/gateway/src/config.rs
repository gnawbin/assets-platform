//! 网关配置（全部来自环境变量，缺省值对齐设计文档附录 D.11）
//!
//! 只读一次环境变量：`GatewayConfig::from_env()` 之后全进程不再读环境，
//! 避免热路径反复 `std::env::var` 加锁。

use std::time::Duration;

/// PostgreSQL 通知通道缺省值（`LISTEN/NOTIFY` 的 channel 名）
///
/// 同一 PostgreSQL 集群上跑多套环境时需要改成不同值，否则会互相收到无关通知。
pub const DEFAULT_PG_CHANNEL: &str = "assets_gw";

/// 网关运行配置
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// 监听地址（默认 `0.0.0.0`）
    pub bind: String,
    /// 监听端口（默认 8080）
    pub port: u16,
    /// 实例标识（租约归属键，重启后保持不变才能立即接管自己的旧租约）
    pub instance_id: String,
    /// 雪花算法 worker id（0..=31，多实例必须互不相同）
    pub worker_id: i16,
    /// worker id 租约时长（秒，默认 60）
    pub worker_lease_secs: i64,
    /// `LISTEN/NOTIFY` 通道名
    pub pg_channel: String,
    /// SSE 事件日志保留时长（秒，默认 600）
    pub event_retention_secs: i64,
    /// SSE 一次性票据有效期（秒，默认 30）
    pub ticket_ttl_secs: i64,
    /// 单次断线回放的最大事件数（默认 200）
    pub replay_max_events: i64,
    /// SSE 心跳间隔（秒，默认 15）
    pub keepalive_secs: u64,
    /// 单用户最大 SSE 连接数（默认 3）
    pub max_sse_per_user: usize,
    /// 是否对非 API Key 主体也做分布式限流（默认关，附录 D.4）
    pub rate_limit_enforce_global: bool,
    /// 限流存储不可用时是否放行（默认 `true`，修 B8 的 fail-closed 风险）
    pub rate_limit_fail_open: bool,
    /// leader 清理间隔（秒，默认 60）
    pub cleanup_interval_secs: u64,
    /// 是否开启响应压缩（默认关，附录 D.12）
    pub gzip: bool,
    /// 是否挂载联调端点（默认关）
    pub dev_endpoints: bool,
}

impl GatewayConfig {
    /// 从环境变量读取配置（缺省即单机可用）
    pub fn from_env() -> anyhow::Result<Self> {
        let port = env_i64("GATEWAY_PORT", 8080);
        let port = u16::try_from(port)
            .map_err(|_| anyhow::anyhow!("GATEWAY_PORT 非法（需 1..=65535）：{}", port))?;

        let worker_id = env_i64("SNOWFLAKE_WORKER_ID", 0);
        if !(0..=31).contains(&worker_id) {
            anyhow::bail!(
                "SNOWFLAKE_WORKER_ID 必须在 0..=31 之间（当前 {}）",
                worker_id
            );
        }
        let worker_id = worker_id as i16;

        let instance_id = std::env::var("GATEWAY_INSTANCE_ID")
            .ok()
            .map(|raw| raw.trim().to_string())
            .filter(|raw| !raw.is_empty())
            .unwrap_or_else(|| default_instance_id(worker_id));

        Ok(Self {
            bind: env_string("GATEWAY_BIND", "0.0.0.0"),
            port,
            instance_id,
            worker_id,
            worker_lease_secs: env_i64("GATEWAY_WORKER_LEASE_SECS", 60),
            pg_channel: env_string("GATEWAY_PG_CHANNEL", DEFAULT_PG_CHANNEL),
            event_retention_secs: env_i64("GATEWAY_SSE_RETENTION_SECS", 600),
            ticket_ttl_secs: env_i64("GATEWAY_SSE_TICKET_TTL_SECS", 30),
            replay_max_events: env_i64("GATEWAY_SSE_REPLAY_MAX", 200),
            keepalive_secs: env_i64("GATEWAY_SSE_KEEPALIVE_SECS", 15).max(1) as u64,
            max_sse_per_user: env_i64("GATEWAY_SSE_MAX_PER_USER", 3).max(1) as usize,
            rate_limit_enforce_global: env_bool("GATEWAY_RATE_LIMIT_ENFORCE_GLOBAL", false),
            rate_limit_fail_open: env_bool("GATEWAY_RATE_LIMIT_FAIL_OPEN", true),
            cleanup_interval_secs: env_i64("GATEWAY_CLEANUP_INTERVAL_SECS", 60).max(5) as u64,
            gzip: env_bool("GATEWAY_GZIP", false),
            dev_endpoints: env_bool("GATEWAY_DEV_ENDPOINTS", false),
        })
    }

    /// 租约续约间隔 = 租约时长 / 3（附录 D.11：允许连续丢两次续约才判死）
    pub fn lease_renew_interval(&self) -> Duration {
        Duration::from_secs((self.worker_lease_secs.max(3) / 3) as u64)
    }

    /// SSE 心跳间隔
    pub fn keepalive(&self) -> Duration {
        Duration::from_secs(self.keepalive_secs.max(1))
    }
}

/// 缺省实例标识：`主机名-worker_id`
///
/// 用主机名而不是随机值，是为了让「同机重启」能立刻接管自己的旧租约，
/// 而「换机同名」的极端情况最多让旧租约在 60s 内自然过期。
fn default_instance_id(worker_id: i16) -> String {
    let host = std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .unwrap_or_else(|| String::from("gw"));
    format!("{}-{}", host, worker_id)
}

fn env_string(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn env_i64(key: &str, default: i64) -> i64 {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .unwrap_or(default)
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .and_then(|raw| parse_bool(&raw))
        .unwrap_or(default)
}

/// 解析布尔值：`1/true/yes/on` 为真，`0/false/no/off` 为假，其余 `None`
fn parse_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bool_accepts_common_forms() {
        assert_eq!(parse_bool("1"), Some(true));
        assert_eq!(parse_bool(" TRUE "), Some(true));
        assert_eq!(parse_bool("on"), Some(true));
        assert_eq!(parse_bool("0"), Some(false));
        assert_eq!(parse_bool("Off"), Some(false));
        assert_eq!(parse_bool("maybe"), None);
    }

    #[test]
    fn lease_renew_interval_is_one_third() {
        let cfg = GatewayConfig {
            bind: String::from("0.0.0.0"),
            port: 8080,
            instance_id: String::from("test-0"),
            worker_id: 0,
            worker_lease_secs: 60,
            pg_channel: DEFAULT_PG_CHANNEL.to_string(),
            event_retention_secs: 600,
            ticket_ttl_secs: 30,
            replay_max_events: 200,
            keepalive_secs: 15,
            max_sse_per_user: 3,
            rate_limit_enforce_global: false,
            rate_limit_fail_open: true,
            cleanup_interval_secs: 60,
            gzip: false,
            dev_endpoints: false,
        };
        assert_eq!(cfg.lease_renew_interval(), Duration::from_secs(20));
        assert_eq!(cfg.keepalive(), Duration::from_secs(15));
    }
}
