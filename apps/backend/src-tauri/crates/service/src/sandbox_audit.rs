//! OCSF 安全事件 → `public.sys_oper_log`（上游文档 §6.8 通道 2）
//!
//! 设计要点：
//! 1. **幂等**：按 OCSF `event_uid` 在应用层去重（进程内集合，容量上限后清空）；
//! 2. **脱敏**：家目录 → `~`、绝对路径 → `<path>`（保留域名与进程名，审计价值高）；
//! 3. **截断**：`oper_content` 最多 8 KiB（按字符边界切，不切坏 UTF-8）；
//! 4. **背压**：单批最多 [`MAX_BATCH`] 条，超出计入 `dropped`，不阻塞调用方；
//! 5. **保留期**：`SANDBOX_AUDIT_RETENTION_DAYS`（缺省 180），由 `api` 每日清理。
//!
//! 表结构与上游文档有一处差异：`sys_oper_log.target_id` 是 `BIGINT`，
//! 无法承载 `wf:<id>` 这类业务 scope，因此 scope 写入 `client_info`
//! （`scope=...`），`target_id` 保持 `NULL`。
//!
//! 除 gateway 推送的 OCSF 事件外，本模块还负责**平台 Agent 侧事件**
//! （[`AGENT_EVENTS`]）：`agent_step` / `agent_approval` / `agent_limit`。
//! 这些事件由 [`AgentEvent`] 构造，**不包含** tool arguments、提示词与 LLM 原文。

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, TimeZone, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;

/// `oper_module` 固定值
pub const OPER_MODULE: &str = "sandbox";
/// `oper_content` 上限（字节）
pub const MAX_CONTENT_BYTES: usize = 8 * 1024;
/// 单批最多处理条数
pub const MAX_BATCH: usize = 200;
/// 平台 Agent 侧事件类型（`unmapped.event`），`classify` 直接沿用、不做语义猜测
pub const AGENT_EVENTS: [&str; 3] = ["agent_step", "agent_approval", "agent_limit"];
/// 审批意见最多保留字符数（超出截断，避免写爆 `oper_content`）
pub const MAX_COMMENT_CHARS: usize = 200;
/// 应用层去重集合容量上限
const DEDUPE_CAPACITY: usize = 8192;

/// 映射后的一行审计记录
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    /// 业务用户（OCSF 缺失时为 `None`，不伪造）
    pub user_id: Option<i64>,
    /// 归一化事件类型
    pub oper_type: String,
    /// 脱敏 + 截断后的完整 OCSF JSON
    pub oper_content: String,
    /// 来源 IP（仅网络类事件）
    pub ip: Option<String>,
    /// `agent / sandbox / generation / scope`
    pub client_info: Option<String>,
    /// 事件时间（OCSF `time`，毫秒时间戳）
    pub created_at: DateTime<Utc>,
    /// OCSF 事件唯一标识（去重键）
    pub event_uid: Option<String>,
}

/// 批量写入结果
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecordOutcome {
    /// 实际写入条数
    pub inserted: usize,
    /// 被去重跳过条数
    pub deduped: usize,
    /// 写库失败条数（只记 WARN，不阻塞调用方）
    pub failed: usize,
    /// 超出 [`MAX_BATCH`] 被丢弃条数
    pub dropped: usize,
}

/// 拆批：兼容单事件对象与 `{ "events": [...] }` 两种推送形态
#[must_use]
pub fn parse_batch(payload: &Value) -> Vec<Value> {
    if let Some(events) = payload.get("events").and_then(Value::as_array) {
        return events.clone();
    }
    if payload.is_array() {
        return payload.as_array().cloned().unwrap_or_default();
    }
    vec![payload.clone()]
}

/// 单事件 → 审计行（非对象直接丢弃）
#[must_use]
pub fn to_row(event: &Value) -> Option<AuditRow> {
    if !event.is_object() {
        return None;
    }
    Some(AuditRow {
        user_id: extract_user_id(event),
        oper_type: classify(event),
        oper_content: truncate_content(&sanitize(&event.to_string())),
        ip: extract_ip(event),
        client_info: Some(client_info(event)),
        created_at: extract_time(event),
        event_uid: extract_uid(event),
    })
}

/// 归一化事件类型：`policy_denied` / `config_change` / `process_start` /
/// `session_open` / `session_stop` / `agent_step` / `agent_approval` /
/// `agent_limit` / `security_event`
#[must_use]
pub fn classify(event: &Value) -> String {
    // 平台 Agent 侧事件自带类型，直接沿用（否则会被下面的关键字规则误判）
    if let Some(name) = lookup_str(event, &["unmapped", "event"])
        && AGENT_EVENTS.contains(&name)
    {
        return name.to_string();
    }

    let text = [
        lookup_str(event, &["class_name"]),
        lookup_str(event, &["activity_name"]),
        lookup_str(event, &["unmapped", "event"]),
        lookup_str(event, &["status"]),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase();

    if text.contains("denied") || text.contains("deny") || text.contains("blocked") {
        "policy_denied".to_string()
    } else if text.contains("config") || text.contains("policy_updated") {
        "config_change".to_string()
    } else if text.contains("process") && (text.contains("start") || text.contains("create")) {
        "process_start".to_string()
    } else if text.contains("create")
        || text.contains("open")
        || text.contains("start")
        || text.contains("ready")
    {
        "session_open".to_string()
    } else if text.contains("delet")
        || text.contains("stop")
        || text.contains("terminat")
        || text.contains("destroy")
    {
        "session_stop".to_string()
    } else {
        "security_event".to_string()
    }
}

fn lookup<'a>(event: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = event;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

fn lookup_str<'a>(event: &'a Value, path: &[&str]) -> Option<&'a str> {
    lookup(event, path).and_then(Value::as_str).map(str::trim)
}

/// 兼容字符串 / 数字 / 布尔（OCSF 的 `generation` 常是数字）
fn lookup_text(event: &Value, path: &[&str]) -> Option<String> {
    lookup(event, path)
        .and_then(|value| match value {
            Value::String(text) => Some(text.trim().to_string()),
            Value::Number(number) => Some(number.to_string()),
            Value::Bool(flag) => Some(flag.to_string()),
            _ => None,
        })
        .filter(|text| !text.is_empty())
}

fn extract_uid(event: &Value) -> Option<String> {
    lookup_str(event, &["event_uid"])
        .or_else(|| lookup_str(event, &["metadata", "uid"]))
        .or_else(|| lookup_str(event, &["unmapped", "event_uid"]))
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn extract_user_id(event: &Value) -> Option<i64> {
    let candidates = [
        &["actor", "user", "uid"][..],
        &["unmapped", "user_id"][..],
        &["labels", "user_id"][..],
        &["metadata", "labels", "user_id"][..],
        &["user_id"][..],
    ];
    for path in candidates {
        if let Some(value) = lookup(event, path) {
            if let Some(id) = value.as_i64() {
                return Some(id);
            }
            if let Some(id) = value.as_str().and_then(|text| text.trim().parse().ok()) {
                return Some(id);
            }
        }
    }
    None
}

fn extract_ip(event: &Value) -> Option<String> {
    lookup_str(event, &["src_endpoint", "ip"])
        .or_else(|| lookup_str(event, &["src_endpoint", "ipv4"]))
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn extract_sandbox(event: &Value) -> Option<String> {
    lookup_str(event, &["unmapped", "sandbox"])
        .or_else(|| lookup_str(event, &["labels", "sandbox"]))
        .or_else(|| lookup_str(event, &["metadata", "labels", "sandbox"]))
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn extract_scope(event: &Value) -> Option<String> {
    lookup_str(event, &["labels", "session_id"])
        .or_else(|| lookup_str(event, &["metadata", "labels", "session_id"]))
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn client_info(event: &Value) -> String {
    // gateway 事件缺省标 `openshell`；平台侧事件（`agent_step` 等）显式写 `runner`
    let runner = lookup_str(event, &["unmapped", "runner"]).unwrap_or("openshell");
    let mut parts = vec![format!("agent={runner}")];
    if let Some(sandbox) = extract_sandbox(event) {
        parts.push(format!("sandbox={sandbox}"));
    }
    if let Some(node) = lookup_str(event, &["unmapped", "node"]) {
        parts.push(format!("node={node}"));
    }
    if let Some(step) = lookup_text(event, &["unmapped", "step"])
        .or_else(|| lookup_text(event, &["labels", "step"]))
    {
        parts.push(format!("step={step}"));
    }
    if let Some(generation) = lookup_text(event, &["unmapped", "generation"])
        .or_else(|| lookup_text(event, &["labels", "generation"]))
    {
        parts.push(format!("generation={generation}"));
    }
    if let Some(scope) = extract_scope(event) {
        parts.push(format!("scope={scope}"));
    }
    parts.join(", ")
}

/// 平台 Agent 侧审计事件（`agent_step` / `agent_approval` / `agent_limit`）
///
/// 只为「可审计且不含敏感内容」的字段建模：**没有** tool arguments、提示词、
/// LLM 输出原文的入口，从类型上避免误写。
#[derive(Debug, Clone, Default)]
pub struct AgentEvent {
    /// 事件类型（取 [`AGENT_EVENTS`] 之一）
    pub kind: String,
    /// 业务 scope（`wf:<execution_id>`，与 gateway 事件的 `session_id` 对齐）
    pub scope: String,
    /// 显式事件 uid（缺省按 kind+scope+step+status+毫秒生成）
    pub uid: Option<String>,
    /// 业务用户（未知时留 `None`，不伪造）
    pub user_id: Option<i64>,
    /// 执行位置：`sandbox` / `platform`
    pub runner: Option<String>,
    /// 工作流节点 id
    pub node: Option<String>,
    /// 第几步（从 0 开始）
    pub step: Option<u32>,
    /// 工具名（仅 `before_tool` 审批与 tool 步）
    pub tool: Option<String>,
    /// 状态：`final` / `tool` / `interrupt` / `error` / `requested` /
    /// `approved` / `rejected` / `pending` / `exceeded`
    pub status: Option<String>,
    /// 中断类型（`before_tool` / `before_final`）
    pub detail: Option<String>,
    /// 单步耗时（毫秒）
    pub duration_ms: Option<u64>,
    /// 单步 token 用量 `(input, output)`
    pub tokens: Option<(i64, i64)>,
    /// 审批结论
    pub approved: Option<bool>,
    /// 审批意见（截断到 [`MAX_COMMENT_CHARS`]）
    pub comment: Option<String>,
    /// 步数上限（仅 `agent_limit`）
    pub limit: Option<u32>,
}

impl AgentEvent {
    /// 构造最小事件（`kind` + `scope`）
    #[must_use]
    pub fn new(kind: impl Into<String>, scope: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            scope: scope.into(),
            ..Self::default()
        }
    }

    /// 转成可交给 [`record`] 的 OCSF 形态
    #[must_use]
    pub fn build(&self) -> Value {
        let now = Utc::now().timestamp_millis();
        let uid = self.uid.clone().unwrap_or_else(|| {
            let mut seed = format!("{}:{}", self.kind, self.scope);
            if let Some(step) = self.step {
                seed.push_str(&format!(":{step}"));
            }
            if let Some(status) = &self.status {
                seed.push_str(&format!(":{status}"));
            }
            format!("{seed}:{now}")
        });

        let mut unmapped = serde_json::Map::new();
        unmapped.insert("event".to_string(), json!(self.kind));
        if let Some(runner) = &self.runner {
            unmapped.insert("runner".to_string(), json!(runner));
        }
        if let Some(node) = &self.node {
            unmapped.insert("node".to_string(), json!(node));
        }
        if let Some(step) = self.step {
            unmapped.insert("step".to_string(), json!(step));
        }
        if let Some(tool) = &self.tool {
            unmapped.insert("tool".to_string(), json!(tool));
        }
        if let Some(detail) = &self.detail {
            unmapped.insert("detail".to_string(), json!(detail));
        }
        if let Some(duration_ms) = self.duration_ms {
            unmapped.insert("duration_ms".to_string(), json!(duration_ms));
        }
        if let Some((input, output)) = self.tokens {
            unmapped.insert(
                "tokens".to_string(),
                json!({"input": input, "output": output}),
            );
        }
        if let Some(approved) = self.approved {
            unmapped.insert("approved".to_string(), json!(approved));
        }
        if let Some(comment) = &self.comment {
            unmapped.insert(
                "comment".to_string(),
                json!(truncate_chars(&sanitize(comment), MAX_COMMENT_CHARS)),
            );
        }
        if let Some(limit) = self.limit {
            unmapped.insert("limit".to_string(), json!(limit));
        }

        let mut labels = serde_json::Map::new();
        labels.insert("session_id".to_string(), json!(self.scope));
        if let Some(user_id) = self.user_id {
            labels.insert("user_id".to_string(), json!(user_id));
        }

        let mut event = serde_json::Map::new();
        event.insert("event_uid".to_string(), json!(uid));
        event.insert("time".to_string(), json!(now));
        event.insert("class_name".to_string(), json!("Agent Activity"));
        event.insert("activity_name".to_string(), json!(self.kind));
        if let Some(status) = &self.status {
            event.insert("status".to_string(), json!(status));
        }
        event.insert("labels".to_string(), Value::Object(labels));
        event.insert("unmapped".to_string(), Value::Object(unmapped));
        Value::Object(event)
    }
}

/// 写入一条平台 Agent 侧事件（失败只 WARN，不影响主链路）
pub async fn record_agent(pool: &PgPool, event: &Value) -> RecordOutcome {
    record(pool, std::slice::from_ref(event)).await
}

/// 按字符截断（不切坏 UTF-8）
fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

fn extract_time(event: &Value) -> DateTime<Utc> {
    if let Some(millis) = lookup(event, &["time"]).and_then(Value::as_i64) {
        // OCSF `time` 为 epoch 毫秒
        if let Some(time) = Utc.timestamp_millis_opt(millis).single() {
            return time;
        }
    }
    if let Some(text) = lookup_str(event, &["time"]).or_else(|| lookup_str(event, &["time_dt"]))
        && let Ok(time) = DateTime::parse_from_rfc3339(text)
    {
        return time.with_timezone(&Utc);
    }
    Utc::now()
}

fn seen() -> &'static Mutex<HashSet<String>> {
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(HashSet::new()))
}

/// 首次见到返回 `true`（应写入）；重复返回 `false`
fn mark_seen(uid: &str) -> bool {
    let mut set = match seen().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if set.len() >= DEDUPE_CAPACITY {
        set.clear();
    }
    set.insert(uid.to_string())
}

/// 脱敏：家目录 → `~`，绝对路径 → `<path>`（保留域名与进程名，审计价值高）
#[must_use]
pub fn sanitize(text: &str) -> String {
    let replaced = match home_dir() {
        Some(home) => text.replace(&home, "~"),
        None => text.to_string(),
    };

    let chars: Vec<char> = replaced.chars().collect();
    let mut out = String::with_capacity(replaced.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '/' {
            let start = index;
            index += 1;
            while index < chars.len() && is_path_char(chars[index]) {
                index += 1;
            }
            let segment: String = chars[start..index].iter().collect();
            // 至少两级（`/a/b`）才视为绝对路径；`//host` 是 URL 协议部分，保留
            if segment.matches('/').count() >= 2 && !segment.starts_with("//") {
                out.push_str("<path>");
            } else {
                out.push_str(&segment);
            }
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn home_dir() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .map(|value| value.trim_end_matches(['/', '\\']).to_string())
        .filter(|value| !value.is_empty())
}

fn is_path_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_' | '~' | '+' | '@' | ':')
}

/// 截断到 [`MAX_CONTENT_BYTES`]（按字符边界）
#[must_use]
pub fn truncate_content(text: &str) -> String {
    if text.len() <= MAX_CONTENT_BYTES {
        return text.to_string();
    }
    let mut out = String::with_capacity(MAX_CONTENT_BYTES + 16);
    for ch in text.chars() {
        if out.len() + ch.len_utf8() > MAX_CONTENT_BYTES {
            break;
        }
        out.push(ch);
    }
    out.push_str("…(truncated)");
    out
}

/// 批量写入 `public.sys_oper_log`
///
/// 单条失败只记 WARN（返回 `failed` 计数），**不向调用方报错**，
/// 避免审计链路反压沙箱主链路（上游文档 §6.8）。
pub async fn record(pool: &PgPool, events: &[Value]) -> RecordOutcome {
    let mut outcome = RecordOutcome::default();
    if events.len() > MAX_BATCH {
        outcome.dropped = events.len() - MAX_BATCH;
    }

    for event in events.iter().take(MAX_BATCH) {
        let Some(row) = to_row(event) else {
            continue;
        };
        if let Some(uid) = row.event_uid.as_deref()
            && !mark_seen(uid)
        {
            outcome.deduped += 1;
            continue;
        }
        match insert(pool, &row).await {
            Ok(()) => outcome.inserted += 1,
            Err(error) => {
                outcome.failed += 1;
                tracing::warn!(%error, "沙箱审计写入失败（不阻塞主链路）");
            }
        }
    }
    outcome
}

async fn insert(pool: &PgPool, row: &AuditRow) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO public.sys_oper_log \
         (user_id, oper_module, oper_type, oper_content, ip, client_info, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(row.user_id)
    .bind(OPER_MODULE)
    .bind(&row.oper_type)
    .bind(&row.oper_content)
    .bind(row.ip.as_deref())
    .bind(row.client_info.as_deref())
    .bind(row.created_at)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|error| error.to_string())
}

/// 清理超过保留期的沙箱审计记录，返回删除条数
pub async fn cleanup(pool: &PgPool, retention_days: u64) -> Result<u64, String> {
    let days = i64::try_from(retention_days.max(1)).unwrap_or(i64::MAX);
    sqlx::query(
        "DELETE FROM public.sys_oper_log \
         WHERE oper_module = $1 AND created_at < NOW() - ($2 * INTERVAL '1 day')",
    )
    .bind(OPER_MODULE)
    .bind(days)
    .execute(pool)
    .await
    .map(|result| result.rows_affected())
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_batch_accepts_single_array_and_wrapper() {
        assert_eq!(parse_batch(&json!({"a": 1})).len(), 1);
        assert_eq!(parse_batch(&json!([{"a": 1}, {"b": 2}])).len(), 2);
        assert_eq!(parse_batch(&json!({"events": [{"a": 1}, {"b": 2}]})).len(), 2);
    }

    #[test]
    fn classify_maps_common_activities() {
        assert_eq!(
            classify(&json!({"activity_name": "Network OPEN DENIED"})),
            "policy_denied"
        );
        assert_eq!(
            classify(&json!({"class_name": "Config Change"})),
            "config_change"
        );
        assert_eq!(
            classify(&json!({"unmapped": {"event": "process_start"}})),
            "process_start"
        );
        assert_eq!(
            classify(&json!({"activity_name": "Sandbox Created"})),
            "session_open"
        );
        assert_eq!(
            classify(&json!({"activity_name": "Sandbox Deleted"})),
            "session_stop"
        );
        assert_eq!(
            classify(&json!({"activity_name": "something else"})),
            "security_event"
        );
    }

    #[test]
    fn to_row_maps_identity_ip_and_client_info() {
        let event = json!({
            "event_uid": "evt-1",
            "time": 1_700_000_000_000_i64,
            "activity_name": "Network OPEN DENIED",
            "actor": {"user": {"uid": "42"}},
            "src_endpoint": {"ip": "10.0.0.8"},
            "unmapped": {"sandbox": "sb-1", "generation": 2},
            "labels": {"session_id": "wf:123"},
            "path": "/workspace/app/main.py"
        });
        let row = to_row(&event).expect("应映射成功");
        assert_eq!(row.user_id, Some(42));
        assert_eq!(row.oper_type, "policy_denied");
        assert_eq!(row.ip.as_deref(), Some("10.0.0.8"));
        assert_eq!(row.event_uid.as_deref(), Some("evt-1"));
        assert_eq!(row.created_at.timestamp_millis(), 1_700_000_000_000);

        let info = row.client_info.expect("client_info");
        assert!(info.contains("sandbox=sb-1"), "{info}");
        assert!(info.contains("generation=2"), "{info}");
        assert!(info.contains("scope=wf:123"), "{info}");

        assert!(row.oper_content.contains("<path>"), "{}", row.oper_content);
        assert!(
            !row.oper_content.contains("/workspace/app/main.py"),
            "{}",
            row.oper_content
        );
    }

    #[test]
    fn non_object_events_are_dropped() {
        assert!(to_row(&json!("nope")).is_none());
        assert!(to_row(&json!([1, 2])).is_none());
    }

    #[test]
    fn sanitize_rewrites_absolute_paths_but_keeps_urls() {
        let sanitized = sanitize("open /workspace/app/main.py and http://gateway:8080/x");
        assert!(sanitized.contains("<path>"), "{sanitized}");
        assert!(!sanitized.contains("/workspace/app/main.py"), "{sanitized}");
        assert!(sanitized.contains("http://gateway:8080/x"), "{sanitized}");
    }

    #[test]
    fn truncate_content_caps_at_limit_without_breaking_utf8() {
        let text = "租".repeat(MAX_CONTENT_BYTES);
        let truncated = truncate_content(&text);
        assert!(
            truncated.len() <= MAX_CONTENT_BYTES + "…(truncated)".len(),
            "len={}",
            truncated.len()
        );
        assert!(truncated.ends_with("…(truncated)"));
        assert!(
            truncated
                .trim_end_matches("…(truncated)")
                .chars()
                .all(|ch| ch == '租')
        );
    }

    #[test]
    fn dedupe_marks_first_seen_only() {
        assert!(mark_seen("unit-test-uid-1"));
        assert!(!mark_seen("unit-test-uid-1"));
        assert!(mark_seen("unit-test-uid-2"));
    }

    #[test]
    fn missing_fields_are_not_fabricated() {
        let row = to_row(&json!({"activity_name": "unknown"})).expect("应映射");
        assert!(row.user_id.is_none());
        assert!(row.ip.is_none());
        assert!(row.event_uid.is_none());
        assert_eq!(row.oper_type, "security_event");
    }

    #[test]
    fn classify_preserves_agent_event_kinds() {
        for kind in AGENT_EVENTS {
            assert_eq!(classify(&json!({"unmapped": {"event": kind}})), kind);
        }
        // 非白名单事件仍走语义规则
        assert_eq!(
            classify(&json!({"unmapped": {"event": "network_denied"}})),
            "policy_denied"
        );
    }

    #[test]
    fn agent_event_maps_scope_user_and_metrics() {
        let mut event = AgentEvent::new("agent_step", "wf:9");
        event.runner = Some("sandbox".to_string());
        event.user_id = Some(7);
        event.node = Some("agent_1".to_string());
        event.step = Some(2);
        event.tool = Some("sandbox_exec".to_string());
        event.status = Some("tool".to_string());
        event.duration_ms = Some(1200);
        event.tokens = Some((11, 5));

        let value = event.build();
        let row = to_row(&value).expect("应映射成功");
        assert_eq!(row.oper_type, "agent_step");
        assert_eq!(row.user_id, Some(7));
        assert!(row.event_uid.is_some());

        let info = row.client_info.expect("client_info");
        assert!(info.contains("agent=sandbox"), "{info}");
        assert!(info.contains("node=agent_1"), "{info}");
        assert!(info.contains("step=2"), "{info}");
        assert!(info.contains("scope=wf:9"), "{info}");

        assert!(
            row.oper_content.contains("\"tokens\""),
            "{}",
            row.oper_content
        );
        // 审计内容不含 tool arguments（类型上就没有该字段）
        assert!(
            !row.oper_content.contains("arguments"),
            "{}",
            row.oper_content
        );
    }

    #[test]
    fn agent_approval_comment_is_sanitized_and_truncated() {
        let mut event = AgentEvent::new("agent_approval", "wf:9");
        event.status = Some("approved".to_string());
        event.approved = Some(true);
        event.detail = Some("before_tool".to_string());
        event.tool = Some("sandbox_exec".to_string());
        event.comment = Some(format!("/workspace/app/secret.txt {}", "同".repeat(300)));

        let value = event.build();
        assert_eq!(classify(&value), "agent_approval");
        let comment = value["unmapped"]["comment"]
            .as_str()
            .expect("comment 应保留");
        assert!(comment.starts_with("<path>"), "{comment}");
        assert!(!comment.contains("/workspace/app/secret.txt"), "{comment}");
        assert_eq!(comment.chars().count(), MAX_COMMENT_CHARS + 1);
        assert!(comment.ends_with('…'), "{comment}");
    }

    #[test]
    fn agent_event_uid_is_unique_per_step_and_status() {
        let mut first = AgentEvent::new("agent_step", "wf:9");
        first.step = Some(1);
        first.status = Some("tool".to_string());
        let mut second = first.clone();
        second.step = Some(2);

        assert_ne!(first.build()["event_uid"], second.build()["event_uid"]);
        assert_eq!(
            first.build()["labels"]["session_id"],
            json!("wf:9"),
            "scope 用于与 gateway 事件串联"
        );
    }
}


