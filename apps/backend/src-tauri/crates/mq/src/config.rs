//! 消息总线配置（全部来自环境变量，缺省即关闭）。
//!
//! 只读一次环境变量：`MqConfig::from_env()` 之后热路径不再 `std::env::var`，
//! 与 `crates/gateway/src/config.rs` 的读取方式一致。
//!
//! 多客户端语义见设计文档 §5.2：
//! - `PULSAR_SUBSCRIPTION_SCOPE=instance`（默认）：订阅名带 `instance_id`，每实例全量；
//! - `shared`：订阅名只含 `{prefix}-{purpose}`，多副本自动分摊。

use std::{fmt, path::PathBuf, time::Duration};

use crate::{MqError, TopicName};

/// 订阅名最大长度（Pulsar 限制内收敛）
pub const MAX_SUBSCRIPTION_NAME_LEN: usize = 200;

/// `purpose` 最大长度
pub const MAX_PURPOSE_LEN: usize = 64;

/// 订阅作用域（来自 `PULSAR_SUBSCRIPTION_SCOPE`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubscriptionScope {
    /// 每实例独立订阅，各自全量（默认）
    #[default]
    Instance,
    /// 跨副本共享订阅，自动分摊
    Shared,
}

impl SubscriptionScope {
    /// 解析环境变量取值
    pub fn parse(raw: &str) -> Result<Self, MqError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "instance" => Ok(Self::Instance),
            "shared" => Ok(Self::Shared),
            other => Err(MqError::Config(format!(
                "PULSAR_SUBSCRIPTION_SCOPE 非法（需 instance / shared）：{other}"
            ))),
        }
    }
}

/// 订阅类型（mq 自持枚举：默认 `memory` feature 下不引用 pulsar 类型）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MqSubType {
    Exclusive,
    Shared,
    Failover,
    /// 多节点分摊 + 按 key 保序（集群复制默认）
    #[default]
    KeyShared,
}

impl MqSubType {
    /// 解析环境变量取值
    pub fn parse(raw: &str) -> Result<Self, MqError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "exclusive" => Ok(Self::Exclusive),
            "shared" => Ok(Self::Shared),
            "failover" => Ok(Self::Failover),
            "key_shared" | "key-shared" | "keyshared" => Ok(Self::KeyShared),
            other => Err(MqError::Config(format!(
                "PULSAR_SUBSCRIPTION_TYPE 非法（需 exclusive / shared / failover / key_shared）：{other}"
            ))),
        }
    }
}

/// 订阅首次创建时的起始位置；已有订阅从 cursor 继续，不受它影响
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MqInitialPosition {
    #[default]
    Latest,
    Earliest,
}

impl MqInitialPosition {
    /// 解析环境变量取值
    pub fn parse(raw: &str) -> Result<Self, MqError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "latest" => Ok(Self::Latest),
            "earliest" => Ok(Self::Earliest),
            other => Err(MqError::Config(format!(
                "PULSAR_INITIAL_POSITION 非法（需 latest / earliest）：{other}"
            ))),
        }
    }
}

/// 认证方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuthMode {
    /// 不认证
    #[default]
    None,
    /// JWT token
    Token,
}

impl AuthMode {
    /// 解析环境变量取值
    pub fn parse(raw: &str) -> Result<Self, MqError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "token" => Ok(Self::Token),
            other => Err(MqError::Config(format!(
                "PULSAR_AUTH 非法（需 none / token）：{other}"
            ))),
        }
    }
}

/// 发布压缩算法
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compression {
    None,
    #[default]
    Lz4,
    Zlib,
    Zstd,
    Snappy,
}

impl Compression {
    /// 解析环境变量取值
    pub fn parse(raw: &str) -> Result<Self, MqError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "lz4" => Ok(Self::Lz4),
            "zlib" => Ok(Self::Zlib),
            "zstd" => Ok(Self::Zstd),
            "snappy" => Ok(Self::Snappy),
            other => Err(MqError::Config(format!(
                "PULSAR_COMPRESSION 非法（需 none / lz4 / zlib / zstd / snappy）：{other}"
            ))),
        }
    }
}

/// 建连失败策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FailMode {
    /// 返回错误（默认）
    #[default]
    FailClosed,
    /// 装配 `DegradedBus` 并仅 WARN；`publish` 仍返回错误，不伪造成功
    FailOpen,
}

impl FailMode {
    /// 解析环境变量取值
    pub fn parse(raw: &str) -> Result<Self, MqError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "fail_closed" | "fail-closed" | "closed" => Ok(Self::FailClosed),
            "fail_open" | "fail-open" | "open" => Ok(Self::FailOpen),
            other => Err(MqError::Config(format!(
                "PULSAR_FAIL_MODE 非法（需 fail_closed / fail_open）：{other}"
            ))),
        }
    }
}

/// 消息总线运行配置
#[derive(Clone)]
pub struct MqConfig {
    /// 是否连接真实 broker（`PULSAR_ENABLED`）；关闭时装配内存实现
    pub enabled: bool,
    /// `pulsar://host:6650` / `pulsar+ssl://`；多 broker 逗号分隔
    pub url: String,
    pub tenant: String,
    pub namespace: String,
    pub auth: AuthMode,
    /// token 认证串（`Debug` 脱敏，不落日志）
    pub token: Option<String>,
    pub tls_ca_file: Option<PathBuf>,
    pub tls_verify_hostname: bool,
    pub connect_timeout: Duration,
    pub op_timeout: Duration,
    pub producer_name: String,
    pub instance_id: String,
    pub batch_size: usize,
    pub batch_delay: Duration,
    pub compression: Compression,
    pub subscription_prefix: String,
    pub subscription_scope: SubscriptionScope,
    pub subscription_type: MqSubType,
    pub initial_position: MqInitialPosition,
    pub read_compacted: bool,
    pub receiver_queue_size: usize,
    pub max_redeliver: u32,
    pub dlq_suffix: String,
    pub publish_retry_max: u32,
    pub fail_mode: FailMode,
}

impl fmt::Debug for MqConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MqConfig")
            .field("enabled", &self.enabled)
            .field("url", &self.url)
            .field("tenant", &self.tenant)
            .field("namespace", &self.namespace)
            .field("auth", &self.auth)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .field("tls_ca_file", &self.tls_ca_file)
            .field("tls_verify_hostname", &self.tls_verify_hostname)
            .field("connect_timeout", &self.connect_timeout)
            .field("op_timeout", &self.op_timeout)
            .field("producer_name", &self.producer_name)
            .field("instance_id", &self.instance_id)
            .field("batch_size", &self.batch_size)
            .field("batch_delay", &self.batch_delay)
            .field("compression", &self.compression)
            .field("subscription_prefix", &self.subscription_prefix)
            .field("subscription_scope", &self.subscription_scope)
            .field("subscription_type", &self.subscription_type)
            .field("initial_position", &self.initial_position)
            .field("read_compacted", &self.read_compacted)
            .field("receiver_queue_size", &self.receiver_queue_size)
            .field("max_redeliver", &self.max_redeliver)
            .field("dlq_suffix", &self.dlq_suffix)
            .field("publish_retry_max", &self.publish_retry_max)
            .field("fail_mode", &self.fail_mode)
            .finish()
    }
}

impl Default for MqConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            tenant: "public".to_string(),
            namespace: "default".to_string(),
            auth: AuthMode::None,
            token: None,
            tls_ca_file: None,
            tls_verify_hostname: true,
            connect_timeout: Duration::from_secs(10),
            op_timeout: Duration::from_secs(30),
            producer_name: "assets".to_string(),
            instance_id: format!("{}-0", default_hostname()),
            batch_size: 100,
            batch_delay: Duration::from_millis(10),
            compression: Compression::Lz4,
            subscription_prefix: "assets".to_string(),
            subscription_scope: SubscriptionScope::Instance,
            subscription_type: MqSubType::KeyShared,
            initial_position: MqInitialPosition::Latest,
            read_compacted: false,
            receiver_queue_size: 1000,
            max_redeliver: 5,
            dlq_suffix: ".dlq".to_string(),
            publish_retry_max: 3,
            fail_mode: FailMode::FailClosed,
        }
    }
}

impl MqConfig {
    /// 从环境变量读取配置；非法值返回 [`MqError::Config`]。
    pub fn from_env() -> Result<Self, MqError> {
        let enabled = env_bool("PULSAR_ENABLED", false);
        let url = env_string("PULSAR_URL", "");
        if enabled && url.is_empty() {
            return Err(MqError::Config(
                "PULSAR_ENABLED=true 时 PULSAR_URL 不能为空".to_string(),
            ));
        }

        let auth = AuthMode::parse(&env_string("PULSAR_AUTH", "none"))?;
        let token = env_opt_string("PULSAR_TOKEN");
        if auth == AuthMode::Token && token.is_none() {
            return Err(MqError::Config(
                "PULSAR_AUTH=token 时 PULSAR_TOKEN 不能为空".to_string(),
            ));
        }

        let worker_id = match env_opt_string("SNOWFLAKE_WORKER_ID") {
            Some(raw) => {
                let value = raw
                    .parse::<i64>()
                    .map_err(|_| MqError::Config(format!("SNOWFLAKE_WORKER_ID 非数字：{raw}")))?;
                if !(0..=31).contains(&value) {
                    return Err(MqError::Config(format!(
                        "SNOWFLAKE_WORKER_ID 必须在 0..=31 之间（当前 {value}）"
                    )));
                }
                value
            }
            None => 0,
        };

        let instance_id = env_opt_string("PULSAR_INSTANCE_ID")
            .unwrap_or_else(|| format!("{}-{}", default_hostname(), worker_id));
        validate_instance_id(&instance_id)?;

        let subscription_prefix = env_string("PULSAR_SUBSCRIPTION_PREFIX", "assets");
        validate_prefix(&subscription_prefix)?;

        let dlq_suffix = env_string("PULSAR_DLQ_SUFFIX", ".dlq");
        if dlq_suffix.is_empty() {
            return Err(MqError::Config("PULSAR_DLQ_SUFFIX 不能为空".to_string()));
        }

        let tenant = env_string("PULSAR_TENANT", "public");
        let namespace = env_string("PULSAR_NAMESPACE", "default");
        // 提前校验 tenant / namespace（逻辑名用占位值，只看前两段）
        TopicName::build(&tenant, &namespace, "validate")?;

        Ok(Self {
            enabled,
            url,
            tenant,
            namespace,
            auth,
            token,
            tls_ca_file: env_opt_string("PULSAR_TLS_CA_FILE").map(PathBuf::from),
            tls_verify_hostname: env_bool("PULSAR_TLS_VERIFY_HOSTNAME", true),
            connect_timeout: Duration::from_secs(env_u64("PULSAR_CONNECT_TIMEOUT_SECS", 10).max(1)),
            op_timeout: Duration::from_secs(env_u64("PULSAR_OP_TIMEOUT_SECS", 30).max(1)),
            producer_name: env_string("PULSAR_PRODUCER_NAME", "assets"),
            instance_id,
            batch_size: env_usize("PULSAR_BATCH_SIZE", 100).max(1),
            batch_delay: Duration::from_millis(env_u64("PULSAR_BATCH_DELAY_MS", 10)),
            compression: Compression::parse(&env_string("PULSAR_COMPRESSION", "lz4"))?,
            subscription_prefix,
            subscription_scope: SubscriptionScope::parse(&env_string(
                "PULSAR_SUBSCRIPTION_SCOPE",
                "instance",
            ))?,
            subscription_type: MqSubType::parse(&env_string(
                "PULSAR_SUBSCRIPTION_TYPE",
                "key_shared",
            ))?,
            initial_position: MqInitialPosition::parse(&env_string(
                "PULSAR_INITIAL_POSITION",
                "latest",
            ))?,
            read_compacted: env_bool("PULSAR_READ_COMPACTED", false),
            receiver_queue_size: env_usize("PULSAR_RECEIVER_QUEUE_SIZE", 1000).max(1),
            max_redeliver: env_u32("PULSAR_MAX_REDELIVER", 5),
            dlq_suffix,
            publish_retry_max: env_u32("PULSAR_PUBLISH_RETRY_MAX", 3),
            fail_mode: FailMode::parse(&env_string("PULSAR_FAIL_MODE", "fail_closed"))?,
        })
    }

    /// 订阅名唯一解析入口：`instance` 带实例标识，`shared` 跨副本共享。
    ///
    /// 调用方**禁止**手拼订阅名，否则多客户端 / 多副本语义会失控。
    pub fn subscription_name(&self, purpose: &str) -> Result<String, MqError> {
        validate_purpose(purpose)?;

        let name = match self.subscription_scope {
            SubscriptionScope::Instance => format!(
                "{}-{}-{}",
                self.subscription_prefix, self.instance_id, purpose
            ),
            SubscriptionScope::Shared => {
                format!("{}-{}", self.subscription_prefix, purpose)
            }
        };

        if name.len() > MAX_SUBSCRIPTION_NAME_LEN {
            return Err(MqError::Config(format!(
                "订阅名超长（{} > {MAX_SUBSCRIPTION_NAME_LEN}）：{name}",
                name.len()
            )));
        }
        Ok(name)
    }

    /// broker 侧 producer 名：`{name}-{instance_id}-{pid}-{bus_seq}-{topic_hash}`。
    ///
    /// 即使两个 `Bus` 配置完全相同，`pid` / `bus_seq` 也能保证不撞名
    /// （Pulsar 的 producer 名在 topic 内必须唯一）。
    #[must_use]
    pub fn producer_name(&self, full_topic: &str, bus_seq: u64) -> String {
        format!(
            "{}-{}-{}-{}-{:08x}",
            self.producer_name,
            self.instance_id,
            std::process::id(),
            bus_seq,
            fnv1a(full_topic)
        )
    }
}

fn validate_purpose(purpose: &str) -> Result<(), MqError> {
    if purpose.is_empty() || purpose.len() > MAX_PURPOSE_LEN {
        return Err(MqError::Config(format!(
            "purpose 长度非法（需 1..={MAX_PURPOSE_LEN}）：{purpose}"
        )));
    }

    let mut chars = purpose.chars();
    let first = chars.next().expect("purpose 非空已校验");
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err(MqError::Config(format!(
            "purpose 必须以小写字母或数字开头：{purpose}"
        )));
    }
    if !chars
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(MqError::Config(format!(
            "purpose 只能包含小写字母、数字、`_`、`-`、`.`：{purpose}"
        )));
    }
    Ok(())
}

fn validate_prefix(prefix: &str) -> Result<(), MqError> {
    if prefix.is_empty() || prefix.len() > 100 {
        return Err(MqError::Config(format!(
            "PULSAR_SUBSCRIPTION_PREFIX 长度非法（需 1..=100）：{prefix}"
        )));
    }
    let mut chars = prefix.chars();
    let first = chars.next().expect("prefix 非空已校验");
    if !first.is_ascii_alphanumeric() {
        return Err(MqError::Config(format!(
            "PULSAR_SUBSCRIPTION_PREFIX 必须以字母或数字开头：{prefix}"
        )));
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')) {
        return Err(MqError::Config(format!(
            "PULSAR_SUBSCRIPTION_PREFIX 只能包含字母、数字、`_`、`-`、`.`：{prefix}"
        )));
    }
    Ok(())
}

fn validate_instance_id(instance_id: &str) -> Result<(), MqError> {
    if instance_id.is_empty() || instance_id.len() > 100 {
        return Err(MqError::Config(format!(
            "PULSAR_INSTANCE_ID 长度非法（需 1..=100）：{instance_id}"
        )));
    }
    let mut chars = instance_id.chars();
    let first = chars.next().expect("instance_id 非空已校验");
    if !first.is_ascii_alphanumeric() {
        return Err(MqError::Config(format!(
            "PULSAR_INSTANCE_ID 必须以字母或数字开头：{instance_id}"
        )));
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')) {
        return Err(MqError::Config(format!(
            "PULSAR_INSTANCE_ID 只能包含字母、数字、`_`、`-`、`.`：{instance_id}"
        )));
    }
    Ok(())
}

fn env_string(key: &str, default: &str) -> String {
    env_opt_string(key).unwrap_or_else(|| default.to_string())
}

fn env_opt_string(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .and_then(|raw| parse_bool(&raw))
        .unwrap_or(default)
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .unwrap_or(default)
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
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

/// 主机名：Linux/macOS 用 `HOSTNAME`，Windows 用 `COMPUTERNAME`
fn default_hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .unwrap_or_else(|| "mq".to_string())
}

/// FNV-1a：只用于 producer 名的 topic 短 hash，不用于安全场景
pub(crate) fn fnv1a(raw: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in raw.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> MqConfig {
        MqConfig {
            instance_id: "host-a-0".to_string(),
            ..MqConfig::default()
        }
    }

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
    fn parses_scope_and_driver_enums() {
        assert_eq!(
            SubscriptionScope::parse("SHARED").unwrap(),
            SubscriptionScope::Shared
        );
        assert_eq!(
            MqSubType::parse("key_shared").unwrap(),
            MqSubType::KeyShared
        );
        assert_eq!(
            MqInitialPosition::parse("earliest").unwrap(),
            MqInitialPosition::Earliest
        );
        assert_eq!(FailMode::parse("fail_open").unwrap(), FailMode::FailOpen);
        assert_eq!(Compression::parse("zstd").unwrap(), Compression::Zstd);
        assert!(SubscriptionScope::parse("cluster").is_err());
    }

    #[test]
    fn subscription_name_instance_contains_instance_id() {
        let cfg = base_config();
        assert_eq!(
            cfg.subscription_name("graph-replicate").unwrap(),
            "assets-host-a-0-graph-replicate"
        );
    }

    #[test]
    fn subscription_name_shared_ignores_instance_id() {
        let cfg = MqConfig {
            subscription_scope: SubscriptionScope::Shared,
            ..base_config()
        };
        assert_eq!(
            cfg.subscription_name("graph-replicate").unwrap(),
            "assets-graph-replicate"
        );
    }

    #[test]
    fn subscription_name_rejects_invalid_purpose() {
        let cfg = base_config();
        for purpose in ["", "Graph", "a/b", "a b", "graph_replicate!"] {
            assert!(
                cfg.subscription_name(purpose).is_err(),
                "purpose 应被拒绝：{purpose}"
            );
        }
        assert!(
            cfg.subscription_name(&"a".repeat(MAX_PURPOSE_LEN + 1))
                .is_err()
        );
    }

    #[test]
    fn producer_name_is_unique_per_topic_and_bus() {
        let cfg = base_config();
        let first = cfg.producer_name("persistent://public/default/tg.a.graph.0", 1);
        let second = cfg.producer_name("persistent://public/default/tg.a.graph.1", 1);
        let third = cfg.producer_name("persistent://public/default/tg.a.graph.0", 2);

        assert!(first.contains("host-a-0"));
        assert_ne!(first, second);
        assert_ne!(first, third);
        assert!(first.ends_with(&format!(
            "{:08x}",
            fnv1a("persistent://public/default/tg.a.graph.0")
        )));
    }

    #[test]
    fn fnv1a_is_deterministic() {
        assert_eq!(fnv1a("same"), fnv1a("same"));
        assert_ne!(fnv1a("same"), fnv1a("other"));
    }

    #[test]
    fn debug_redacts_token() {
        let cfg = MqConfig {
            token: Some("super-secret".to_string()),
            ..base_config()
        };
        let rendered = format!("{cfg:?}");
        assert!(!rendered.contains("super-secret"));
        assert!(rendered.contains("<redacted>"));
    }
}
