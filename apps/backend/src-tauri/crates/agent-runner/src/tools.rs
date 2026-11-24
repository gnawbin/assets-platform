//! 沙箱本地工具：`sandbox_exec` / `file_read` / `file_write`
//!
//! 所有文件路径必须位于 [`StepInput::workspace`] 内；命令执行受单步超时与输出上限约束。
//! 真正的隔离（Landlock / seccomp / 出网策略）由 `OpenShell` supervisor 提供，
//! 本模块只做**边界检查**与限额，避免把明显越权的请求交给沙箱。

use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use agent_core::protocol::StepInput;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Value, json};

/// runner 可本地执行的工具
pub const LOCAL_TOOLS: [&str; 3] = ["sandbox_exec", "file_read", "file_write"];

/// 是否属于本地工具
#[must_use]
pub fn is_local_tool(name: &str) -> bool {
    LOCAL_TOOLS.contains(&name)
}

/// 执行本地工具，返回 observation
///
/// # Errors
/// 参数缺失、路径越界、超时、IO 失败时返回可展示错误。
pub async fn execute(step: &StepInput, tool: &str, arguments: &Value) -> Result<Value, String> {
    match tool {
        "sandbox_exec" => exec(step, arguments).await,
        "file_read" => read_file(step, arguments).await,
        "file_write" => write_file(step, arguments).await,
        other => Err(format!("未知本地工具：{other}")),
    }
}

fn workspace(step: &StepInput) -> &str {
    let workspace = step.workspace.trim();
    if workspace.is_empty() { "/workspace" } else { workspace }
}

/// 解析并校验路径（必须在 workspace 内，且不含 `..`）
fn resolve_path(step: &StepInput, raw: &str) -> Result<PathBuf, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("path 不能为空".to_string());
    }
    let base = Path::new(workspace(step));
    let candidate = Path::new(raw);
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base.join(candidate)
    };

    if joined
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(format!("路径不允许包含 `..`：{raw}"));
    }
    if !joined.starts_with(base) {
        return Err(format!(
            "路径越界（必须位于 {} 下）：{raw}",
            base.display()
        ));
    }
    Ok(joined)
}

fn required_path(arguments: &Value) -> Result<String, String> {
    arguments
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| "缺少 path 参数".to_string())
}

/// 按字节上限截断（不切坏 UTF-8）
fn truncate(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut out = String::with_capacity(max_bytes + 16);
    for ch in text.chars() {
        if out.len() + ch.len_utf8() > max_bytes {
            break;
        }
        out.push(ch);
    }
    out.push_str("\n…（输出已截断）");
    out
}

fn split_lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(ToString::to_string)
        .collect()
}

async fn exec(step: &StepInput, arguments: &Value) -> Result<Value, String> {
    let command = arguments
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "sandbox_exec 缺少 command".to_string())?;

    let cwd = arguments
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| resolve_path(step, value))
        .transpose()?;

    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-lc")
        .arg(command)
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match cwd {
        Some(dir) => {
            cmd.current_dir(dir);
        }
        None => {
            cmd.current_dir(workspace(step));
        }
    }

    let timeout_secs = step.limits.exec_timeout_secs.max(1);
    let child = cmd
        .spawn()
        .map_err(|error| format!("命令启动失败：{error}"))?;
    let output = tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output())
        .await
        .map_err(|_| format!("命令超时（{timeout_secs}s）"))?
        .map_err(|error| format!("命令执行失败：{error}"))?;

    Ok(json!({
        "exit_code": output.status.code().unwrap_or(-1),
        "text": truncate(
            &String::from_utf8_lossy(&output.stdout),
            step.limits.max_output_bytes
        ),
        "stderr": split_lines(&output.stderr),
    }))
}

async fn read_file(step: &StepInput, arguments: &Value) -> Result<Value, String> {
    let raw = required_path(arguments)?;
    let path = resolve_path(step, &raw)?;
    let data = tokio::fs::read(&path)
        .await
        .map_err(|error| format!("读取失败 {}：{error}", path.display()))?;
    if data.len() > step.limits.max_file_bytes {
        return Err(format!(
            "文件超过上限 {} 字节：{}",
            step.limits.max_file_bytes,
            data.len()
        ));
    }
    let text = String::from_utf8(data)
        .map_err(|_| "文件不是 UTF-8 文本；二进制请用 sandbox_exec + base64".to_string())?;
    let truncated = text.len() > step.limits.max_output_bytes;
    Ok(json!({
        "path": raw,
        "size": text.len(),
        "truncated": truncated,
        "content": truncate(&text, step.limits.max_output_bytes),
    }))
}

async fn write_file(step: &StepInput, arguments: &Value) -> Result<Value, String> {
    let raw = required_path(arguments)?;
    let path = resolve_path(step, &raw)?;

    let data = if let Some(encoded) = arguments
        .get("content_base64")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        BASE64
            .decode(encoded)
            .map_err(|error| format!("content_base64 解码失败：{error}"))?
    } else if let Some(content) = arguments.get("content").and_then(Value::as_str) {
        content.as_bytes().to_vec()
    } else {
        return Err("file_write 需要 content 或 content_base64".to_string());
    };

    if data.len() > step.limits.max_file_bytes {
        return Err(format!(
            "文件超过上限 {} 字节：{}",
            step.limits.max_file_bytes,
            data.len()
        ));
    }

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("创建目录失败 {}：{error}", parent.display()))?;
    }
    tokio::fs::write(&path, &data)
        .await
        .map_err(|error| format!("写入失败 {}：{error}", path.display()))?;

    Ok(json!({"path": raw, "size": data.len(), "written": true}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn step_in(workspace: &str) -> StepInput {
        serde_json::from_value(json!({
            "thread_id": "t:1",
            "node_id": "n1",
            "agent": {"prompt": "x"},
            "llm": {"base_url": "http://127.0.0.1:1/v1", "default_model": "m"},
            "workspace": workspace
        }))
        .expect("step 应可解析")
    }

    fn temp_workspace() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agent-runner-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("创建临时目录");
        dir
    }

    #[test]
    fn local_tools_are_stable() {
        assert!(is_local_tool("sandbox_exec"));
        assert!(is_local_tool("file_read"));
        assert!(is_local_tool("file_write"));
        assert!(!is_local_tool("knowledge_search"));
        assert!(!is_local_tool("current_time"));
    }

    #[test]
    fn resolve_path_rejects_traversal_and_outside_absolute() {
        let dir = temp_workspace();
        let workspace = dir.to_string_lossy().to_string();
        let step = step_in(&workspace);

        assert!(resolve_path(&step, "../etc/passwd").is_err());
        assert!(resolve_path(&step, "/etc/passwd").is_err());
        assert!(resolve_path(&step, "notes/a.txt").is_ok());
        assert!(resolve_path(&step, "").is_err());
    }

    #[tokio::test]
    async fn file_write_then_read_round_trip() {
        let dir = temp_workspace();
        let step = step_in(&dir.to_string_lossy());

        let written = write_file(&step, &json!({"path": "a/b.txt", "content": "你好"}))
            .await
            .expect("写入应成功");
        assert_eq!(written["written"], true);

        let read = read_file(&step, &json!({"path": "a/b.txt"}))
            .await
            .expect("读取应成功");
        assert_eq!(read["content"], "你好");
        assert_eq!(read["truncated"], false);
    }

    #[tokio::test]
    async fn file_write_supports_base64() {
        let dir = temp_workspace();
        let step = step_in(&dir.to_string_lossy());

        let encoded = BASE64.encode(b"\x00\x01binary");
        write_file(&step, &json!({"path": "bin.dat", "content_base64": encoded}))
            .await
            .expect("写入应成功");

        let data = std::fs::read(dir.join("bin.dat")).expect("文件存在");
        assert_eq!(data, b"\x00\x01binary");
    }

    #[tokio::test]
    async fn file_read_rejects_non_utf8() {
        let dir = temp_workspace();
        std::fs::write(dir.join("bin.dat"), [0xff, 0xfe]).expect("写入测试文件");
        let step = step_in(&dir.to_string_lossy());
        let error = read_file(&step, &json!({"path": "bin.dat"}))
            .await
            .expect_err("非 UTF-8 必须报错");
        assert!(error.contains("UTF-8"), "{error}");
    }

    #[test]
    fn truncate_keeps_utf8_boundaries() {
        let text = "你好世界";
        assert_eq!(truncate(text, 100), text);
        let truncated = truncate(text, 5);
        assert!(truncated.starts_with("你"), "{truncated}");
        assert!(truncated.contains("已截断"), "{truncated}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn exec_runs_shell_command_in_workspace() {
        let dir = temp_workspace();
        std::fs::write(dir.join("hello.txt"), "hi").expect("写入测试文件");
        let step = step_in(&dir.to_string_lossy());

        let output = exec(&step, &json!({"command": "cat hello.txt"}))
            .await
            .expect("命令应执行");
        assert_eq!(output["exit_code"], 0);
        assert_eq!(output["text"], "hi");
    }
}
