//! wire DTO ↔ 契约类型转换
//!
//! 所有容错规则集中在这里（设计文档 §5.5）：
//! 1. 状态字符串走 [`SandboxState::parse`]，空值按上下文缺省；
//! 2. 时间字段统一 trim，空串 → `None`（不解析、不校验格式）；
//! 3. 执行事件流与文件列表用 `serde_json::Value` 宽容解析，未知字段直接忽略。

use sandbox::{ExecFailure, ExecResult, FileEntry, SandboxHandle, SandboxState, SandboxSummary};
use serde_json::Value;

use crate::client::CubeOptions;
use crate::wire::SandboxDto;

/// `SandboxDto` → `SandboxHandle`
///
/// 创建响应常常不带状态字段，此时按「创建成功即运行中」处理。
pub(crate) fn to_handle(dto: &SandboxDto, options: &CubeOptions) -> SandboxHandle {
    let state = if dto.state.trim().is_empty() {
        SandboxState::Running
    } else {
        SandboxState::parse(&dto.state)
    };

    SandboxHandle {
        id: dto.id.clone(),
        state,
        template_id: dto.template.clone().filter(|t| !t.trim().is_empty()),
        created_at: normalize_time(dto.started_at.clone()),
        data_host_suffix: options.data_host_suffix.clone(),
        envd_port: options.envd_port,
    }
}

/// `SandboxDto` → `SandboxSummary`
pub(crate) fn to_summary(dto: &SandboxDto) -> SandboxSummary {
    SandboxSummary {
        id: dto.id.clone(),
        state: SandboxState::parse(&dto.state),
        template_id: dto.template.clone().filter(|t| !t.trim().is_empty()),
        metadata: dto.metadata.clone(),
        started_at: normalize_time(dto.started_at.clone()),
    }
}

/// 时间字段归一化：trim 后非空才保留
pub(crate) fn normalize_time(raw: Option<String>) -> Option<String> {
    raw.map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 解析执行结果（envd 事件流 / 纯文本 / 单对象都兼容）
pub(crate) fn parse_exec_body(body: &str, duration_ms: u64) -> ExecResult {
    let mut result = ExecResult {
        exit_code: 0,
        text: None,
        stdout: Vec::new(),
        stderr: Vec::new(),
        error: None,
        duration_ms,
    };

    let trimmed = body.trim();
    if trimmed.is_empty() {
        return result;
    }

    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Array(items)) => {
            for item in &items {
                absorb_event(item, &mut result);
            }
        }
        Ok(value) => {
            absorb_event(&value, &mut result);
            if let Some(text) = value.as_str() {
                push_lines(text, &mut result.stdout);
            }
        }
        Err(_) => push_lines(trimmed, &mut result.stdout),
    }

    result.text = result
        .stdout
        .iter()
        .rev()
        .find(|line| !line.trim().is_empty())
        .cloned();
    result
}

/// 吸收单个事件（兼容 `{"stdout":..}` 与 `{"event":{"stdout":..}}` 两种包装）
fn absorb_event(value: &Value, result: &mut ExecResult) {
    let target = value.get("event").unwrap_or(value);

    if let Some(text) = target.get("stdout").and_then(Value::as_str) {
        push_lines(text, &mut result.stdout);
    }
    if let Some(text) = target.get("stderr").and_then(Value::as_str) {
        push_lines(text, &mut result.stderr);
    }
    if let Some(code) = target
        .get("exit_code")
        .or_else(|| target.get("exitCode"))
        .and_then(Value::as_i64)
    {
        result.exit_code = i32::try_from(code).unwrap_or(1);
    }
    if let Some(error) = target.get("error").filter(|v| !v.is_null()) {
        result.error = Some(parse_failure(error));
    }
}

/// 异常对象 → `ExecFailure`
fn parse_failure(value: &Value) -> ExecFailure {
    if let Some(text) = value.as_str() {
        return ExecFailure {
            name: "Error".to_string(),
            value: text.to_string(),
            traceback: Vec::new(),
        };
    }

    ExecFailure {
        name: str_field(value, &["name", "type"]).unwrap_or_else(|| "Error".to_string()),
        value: str_field(value, &["value", "message"]).unwrap_or_default(),
        traceback: value
            .get("traceback")
            .and_then(Value::as_array)
            .map(|lines| {
                lines
                    .iter()
                    .filter_map(|line| line.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// 文件列表解析（数组 / `{entries|files|data: [...]}` 都兼容）
pub(crate) fn parse_file_entries(value: &Value) -> Vec<FileEntry> {
    let items: &[Value] = match value {
        Value::Array(items) => items,
        Value::Object(map) => ["entries", "files", "data"]
            .iter()
            .find_map(|key| map.get(*key).and_then(Value::as_array))
            .map_or(&[], Vec::as_slice),
        _ => &[],
    };

    items.iter().map(to_file_entry).collect()
}

/// 单个文件条目
fn to_file_entry(value: &Value) -> FileEntry {
    let path = str_field(value, &["path"]).unwrap_or_default();
    let name = str_field(value, &["name"])
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| basename(&path));
    let kind = str_field(value, &["type"]).unwrap_or_default();
    let is_dir = kind.eq_ignore_ascii_case("dir")
        || kind.eq_ignore_ascii_case("directory")
        || bool_field(value, &["isDir", "is_dir"]);

    FileEntry {
        path,
        name,
        is_dir,
        size: u64_field(value, &["size"]).unwrap_or(0),
    }
}

/// 日志解析（字符串数组 / `{logs: [...]}` / 纯文本都兼容）
pub(crate) fn parse_logs(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items.iter().filter_map(log_line).collect(),
        Value::Object(map) => ["logs", "entries"]
            .iter()
            .find_map(|key| map.get(*key))
            .map_or_else(Vec::new, parse_logs),
        Value::String(text) => text.lines().map(str::to_string).collect(),
        _ => Vec::new(),
    }
}

/// 单行日志
fn log_line(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    str_field(value, &["line", "message", "text", "log"])
}

/// 按换行切分并去掉行尾 `\r` / 末尾空行
fn push_lines(text: &str, target: &mut Vec<String>) {
    for line in text.lines() {
        target.push(line.trim_end_matches('\r').to_string());
    }
    while target.last().is_some_and(String::is_empty) {
        target.pop();
    }
}

/// 取字符串字段（按别名顺序）
fn str_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::to_string)
}

/// 取布尔字段
fn bool_field(value: &Value, keys: &[&str]) -> bool {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_bool))
        .unwrap_or(false)
}

/// 取整数字段
fn u64_field(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_u64))
}

/// 路径最后一段
fn basename(path: &str) -> String {
    path.rsplit('/')
        .find(|part| !part.is_empty())
        .map_or_else(|| path.to_string(), str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from_json(raw: &str) -> SandboxDto {
        serde_json::from_str(raw).expect("示例响应应可解析")
    }

    #[test]
    fn handle_defaults_to_running_when_state_missing() {
        let options = CubeOptions::default();
        let handle = to_handle(
            &from_json(r#"{"sandboxID":"sb-9","template":"python"}"#),
            &options,
        );
        assert_eq!(handle.id, "sb-9");
        assert_eq!(handle.state, SandboxState::Running);
        assert_eq!(handle.template_id.as_deref(), Some("python"));
        assert_eq!(handle.created_at, None);
        assert_eq!(handle.envd_port, options.envd_port);
    }

    #[test]
    fn handle_reads_snake_case_and_paused_state() {
        let dto = from_json(
            r#"{"sandbox_id":"sb-1","state":"paused","created_at":" 2026-01-01T00:00:00Z ","template_id":"tpl"}"#,
        );
        let handle = to_handle(&dto, &CubeOptions::default());
        assert_eq!(handle.id, "sb-1");
        assert_eq!(handle.state, SandboxState::Paused);
        assert_eq!(
            handle.created_at.as_deref(),
            Some("2026-01-01T00:00:00Z"),
            "时间字段应被 trim"
        );
    }

    #[test]
    fn summary_keeps_metadata_and_unknown_state() {
        let dto = from_json(r#"{"sandboxID":"sb-2","status":"error","metadata":{"tenant":"t1"}}"#);
        let summary = to_summary(&dto);
        assert_eq!(summary.state, SandboxState::Unknown);
        assert_eq!(summary.metadata.get("tenant").map(String::as_str), Some("t1"));
    }

    #[test]
    fn normalize_time_drops_blank_values() {
        assert_eq!(normalize_time(None), None);
        assert_eq!(normalize_time(Some("  ".to_string())), None);
        assert_eq!(
            normalize_time(Some(" 2026-01-01 ".to_string())),
            Some("2026-01-01".to_string())
        );
    }

    #[test]
    fn exec_body_parses_event_array() {
        let body = r#"[
            {"event":{"stdout":"hello\n"}},
            {"stdout":"world\n","exit_code":0}
        ]"#;
        let result = parse_exec_body(body, 12);
        assert_eq!(result.stdout, vec!["hello", "world"]);
        assert_eq!(result.text.as_deref(), Some("world"));
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.duration_ms, 12);
    }

    #[test]
    fn exec_body_parses_plain_text_and_errors() {
        let result = parse_exec_body("line1\nline2\n", 5);
        assert_eq!(result.stdout, vec!["line1", "line2"]);
        assert_eq!(result.text.as_deref(), Some("line2"));

        let with_error = parse_exec_body(
            r#"{"stderr":"boom\n","error":{"name":"ZeroDivisionError","value":"division by zero","traceback":["a","b"]}}"#,
            7,
        );
        assert_eq!(with_error.stderr, vec!["boom"]);
        let failure = with_error.error.expect("应解析出异常");
        assert_eq!(failure.name, "ZeroDivisionError");
        assert_eq!(failure.value, "division by zero");
        assert_eq!(failure.traceback.len(), 2);
    }

    #[test]
    fn file_entries_parse_array_and_wrapped_object() {
        let flat = serde_json::json!([
            {"path":"/work/a.py","size":12},
            {"name":"dir","type":"dir","isDir":true}
        ]);
        let entries = parse_file_entries(&flat);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "a.py");
        assert_eq!(entries[0].size, 12);
        assert!(entries[1].is_dir);

        let wrapped = serde_json::json!({"entries":[{"path":"/work/b.txt"}]});
        let entries = parse_file_entries(&wrapped);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "b.txt");
        assert!(!entries[0].is_dir);
    }

    #[test]
    fn logs_parse_strings_objects_and_text() {
        assert_eq!(
            parse_logs(&serde_json::json!(["a", {"line":"b"}, {"message":"c"}])),
            vec!["a", "b", "c"]
        );
        assert_eq!(parse_logs(&serde_json::json!({"logs":["x"]})), vec!["x"]);
        assert_eq!(parse_logs(&serde_json::json!("l1\nl2")), vec!["l1", "l2"]);
    }
}
