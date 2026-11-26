//! 信封 / topic / 回执等跨驱动数据类型。
//!
//! 信封是**跨版本唯一契约**：payload 用 JSON 而非 prost 生成类型，
//! 避免「升级 pulsar / prost 就要重新生成消息结构」的耦合。

use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::MqError;

/// 信封版本缺省值（旧消息未带该字段时按 v1 解析）
const fn default_schema_version() -> u16 {
    1
}

/// 消息信封
///
/// `serde_json::Value` 含浮点数，无法实现 `Eq`；`PartialEq` 已足够测试与比较。
#[allow(
    clippy::derive_partial_eq_without_eq,
    reason = "payload 是 serde_json::Value，浮点语义下不能实现 Eq"
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    /// 幂等键：雪花 ID（与 `utils::snowflake::next_id` 同源）
    pub id: String,
    /// 逻辑 topic 名（不含 tenant / namespace，便于多环境复用）
    pub topic: String,
    /// 分区键 / message key：同一 key 的事件在 `KeyShared` 订阅下保序。
    /// 由发布方写入（未来集群复制传 `node_id`）；订阅侧不持有该字段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partition_key: Option<String>,
    /// 业务事件类型，如 `"graph.node.upsert"`（未来集群复制用；本轮不定义取值）
    pub kind: String,
    /// 多租户隔离（消费端据此选 schema）
    pub tenant_id: Option<i64>,
    /// 分片内单调序号（消费端幂等水位；客户端只透传，不做解释）
    pub seq: Option<i64>,
    /// 毫秒时间戳
    pub created_at: i64,
    /// 链路追踪 ID（与 tracing / `OTel` 对齐）
    pub trace_id: Option<String>,
    /// 信封版本；消费端按版本分支，实现平滑演进
    #[serde(default = "default_schema_version")]
    pub schema_version: u16,
    pub payload: serde_json::Value,
}

impl Envelope {
    /// 构造一个最小信封。
    ///
    /// `id` 由调用方在发布前生成一次；重试 / 多客户端必须复用同一个 `id`。
    pub fn new(
        id: impl Into<String>,
        topic: impl Into<String>,
        kind: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: id.into(),
            topic: topic.into(),
            partition_key: None,
            kind: kind.into(),
            tenant_id: None,
            seq: None,
            created_at: now_millis(),
            trace_id: None,
            schema_version: default_schema_version(),
            payload,
        }
    }

    /// 冗余写入 Pulsar `properties` 的字段（便于 broker 侧排查）。
    ///
    /// `partition_key` **不在这里**：它走 message key，而不是 property。
    #[must_use]
    pub fn attributes(&self) -> BTreeMap<String, String> {
        let mut attributes = BTreeMap::new();
        attributes.insert(
            "schema_version".to_string(),
            self.schema_version.to_string(),
        );
        if let Some(trace_id) = &self.trace_id {
            attributes.insert("trace_id".to_string(), trace_id.clone());
        }
        if let Some(tenant_id) = self.tenant_id {
            attributes.insert("tenant_id".to_string(), tenant_id.to_string());
        }
        attributes
    }
}

/// `persistent://{tenant}/{namespace}/{name}` 的构造与校验（纯函数，可单测）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicName {
    pub tenant: String,
    pub namespace: String,
    pub name: String,
}

impl TopicName {
    /// 逻辑名必须是 `[a-z0-9][a-z0-9._-]*`，长度 ≤ 200（Pulsar 限制内收敛）
    pub fn build(tenant: &str, namespace: &str, logical: &str) -> Result<String, MqError> {
        validate_component("tenant", tenant)?;
        validate_component("namespace", namespace)?;
        validate_logical(logical)?;
        Ok(format!("persistent://{tenant}/{namespace}/{logical}"))
    }

    /// 解析完整 topic URI（仅支持 `persistent://`）
    pub fn parse(uri: &str) -> Result<Self, MqError> {
        let rest = uri
            .strip_prefix("persistent://")
            .ok_or_else(|| MqError::InvalidTopic(format!("仅支持 persistent:// 前缀：{uri}")))?;

        let mut parts = rest.split('/');
        let tenant = parts.next().unwrap_or_default();
        let namespace = parts.next().unwrap_or_default();
        let name = parts.next().unwrap_or_default();
        if parts.next().is_some() {
            return Err(MqError::InvalidTopic(format!(
                "topic 段数过多（需 tenant/namespace/name）：{uri}"
            )));
        }

        validate_component("tenant", tenant)?;
        validate_component("namespace", namespace)?;
        validate_logical(name)?;

        Ok(Self {
            tenant: tenant.to_string(),
            namespace: namespace.to_string(),
            name: name.to_string(),
        })
    }

    /// 还原完整 topic URI
    #[must_use]
    pub fn to_uri(&self) -> String {
        format!(
            "persistent://{}/{}/{}",
            self.tenant, self.namespace, self.name
        )
    }
}

fn validate_component(label: &str, raw: &str) -> Result<(), MqError> {
    if raw.is_empty() || raw.len() > 100 {
        return Err(MqError::InvalidTopic(format!(
            "{label} 长度非法（需 1..=100）：{raw}"
        )));
    }
    if !raw
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(MqError::InvalidTopic(format!(
            "{label} 只能包含字母、数字、`_`、`-`、`.`：{raw}"
        )));
    }
    Ok(())
}

fn validate_logical(logical: &str) -> Result<(), MqError> {
    if logical.is_empty() || logical.len() > 200 {
        return Err(MqError::InvalidTopic(format!(
            "logical 长度非法（需 1..=200）：{logical}"
        )));
    }
    let mut chars = logical.chars();
    let first = chars.next().expect("logical 非空已校验");
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err(MqError::InvalidTopic(format!(
            "logical 必须以小写字母或数字开头：{logical}"
        )));
    }
    if !chars
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(MqError::InvalidTopic(format!(
            "logical 只能包含小写字母、数字、`.`、`_`、`-`：{logical}"
        )));
    }
    Ok(())
}

/// 投递回执：broker 侧消息标识
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub topic: String,
    pub message_id: String,
}

/// 消费入参：信封 + broker 侧属性只读快照
#[derive(Debug, Clone)]
pub struct Inbound {
    pub envelope: Envelope,
    pub attributes: BTreeMap<String, String>,
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}
