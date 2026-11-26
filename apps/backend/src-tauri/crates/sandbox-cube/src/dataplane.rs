//! 数据面实现（CubeProxy → envd，E2B 协议兼容）
//!
//! 请求统一带虚拟 `Host: {envd_port}-{sandbox_id}.{suffix}`：
//! - DNS 模式：直接请求 `http://<host>`；
//! - IP 直连模式（配置了 `SANDBOX_PROXY_NODE_IP`）：请求 `http://<ip>:<port>` 但保留虚拟 Host。
//!
//! 文件写入先试 `application/octet-stream`，遇 4xx 再退 `multipart/form-data`
//! （CubeSandbox 官方 `sdk/files-api-streaming.md` 记录的兼容差异）。

use std::time::{Duration, Instant};

use reqwest::header::{CONTENT_TYPE, HOST};
use sandbox::{ExecRequest, ExecResult, FileEntry, MAX_TIMEOUT_SECS, SandboxError, SandboxResult};
use serde_json::Value;

use crate::client::{CubeOptions, CubeSandboxRuntime};
use crate::convert::{parse_exec_body, parse_file_entries};
use crate::wire::StartProcess;

/// 数据面：文件读写 / 目录列举
const PATH_FILES: &str = "/files";
/// 数据面：执行（envd `Process/Start`）
const PATH_PROCESS_START: &str = "/process.Process/Start";
/// multipart 回退用的固定 boundary
const MULTIPART_BOUNDARY: &str = "----assets-platform-sandbox-boundary";

/// 数据面地址与虚拟 Host
struct DataEndpoint {
    /// 实际请求的 base（DNS 模式是虚拟主机，IP 直连模式是节点 IP + 端口）
    base: String,
    /// 虚拟 Host 头
    host: String,
}

impl CubeSandboxRuntime {
    /// 计算数据面地址（`envd_port` 与 `data_host_suffix` 来自配置）
    fn data_endpoint(&self, id: &str) -> DataEndpoint {
        let options = self.options();
        let port = options.envd_port;
        let host = format!("{port}-{id}.{}", options.data_host_suffix);
        match options.proxy_node_ip.as_deref() {
            Some(ip) if !ip.trim().is_empty() => DataEndpoint {
                base: format!("http://{ip}:{port}"),
                host,
            },
            _ => DataEndpoint {
                base: format!("http://{host}"),
                host,
            },
        }
    }

    /// 执行代码 / 命令
    pub(crate) async fn data_exec(
        &self,
        id: &str,
        request: ExecRequest,
    ) -> SandboxResult<ExecResult> {
        let started = Instant::now();
        let (cmd, args) = split_request(self.options(), &request)?;
        let timeout_secs = request
            .timeout_secs
            .unwrap_or_else(|| u32::try_from(self.timeout_secs()).unwrap_or(MAX_TIMEOUT_SECS))
            .clamp(1, MAX_TIMEOUT_SECS);

        let endpoint = self.data_endpoint(id);
        let url = format!("{}{PATH_PROCESS_START}", endpoint.base);
        let body = StartProcess {
            cmd: &cmd,
            args: &args,
            cwd: request.cwd.as_deref(),
            envs: &request.env,
            user: request.user.as_deref(),
        };

        let builder = self
            .http()
            .post(url)
            .header(HOST, &endpoint.host)
            .timeout(Duration::from_secs(u64::from(timeout_secs)))
            .json(&body);
        let response = self.send(builder).await?;
        let body = self.handle_text(response).await?;

        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let result = parse_exec_body(&body, duration_ms);
        tracing::debug!(
            sandbox = %id,
            cmd = %cmd,
            exit_code = result.exit_code,
            duration_ms,
            "CubeSandbox 执行完成"
        );
        Ok(result)
    }

    /// 读文件
    pub(crate) async fn data_read_file(
        &self,
        id: &str,
        path: &str,
        user: &str,
    ) -> SandboxResult<Vec<u8>> {
        let endpoint = self.data_endpoint(id);
        let builder = self
            .http()
            .get(format!("{}{PATH_FILES}", endpoint.base))
            .header(HOST, &endpoint.host)
            .query(&[("path", path.to_string()), ("username", user.to_string())]);
        let response = self.send(builder).await?;
        self.handle_bytes(response).await
    }

    /// 列目录
    pub(crate) async fn data_list_files(
        &self,
        id: &str,
        dir: &str,
        user: &str,
    ) -> SandboxResult<Vec<FileEntry>> {
        let endpoint = self.data_endpoint(id);
        let builder = self
            .http()
            .get(format!("{}{PATH_FILES}", endpoint.base))
            .header(HOST, &endpoint.host)
            .query(&[("path", dir.to_string()), ("username", user.to_string())]);
        let response = self.send(builder).await?;
        let value: Value = self.handle_json(response).await?;
        Ok(parse_file_entries(&value))
    }
}

impl CubeSandboxRuntime {
    /// 写文件（octet-stream 优先，4xx 回退 multipart）
    pub(crate) async fn data_write_file(
        &self,
        id: &str,
        path: &str,
        user: &str,
        data: Vec<u8>,
    ) -> SandboxResult<()> {
        if data.len() > sandbox::MAX_TRANSFER_BYTES {
            return Err(SandboxError::invalid_request(
                400,
                format!(
                    "文件超过单次传输上限 {} 字节，请改用对象存储中转",
                    sandbox::MAX_TRANSFER_BYTES
                ),
            ));
        }

        let endpoint = self.data_endpoint(id);
        let url = format!("{}{PATH_FILES}", endpoint.base);
        let query = [("path", path.to_string()), ("username", user.to_string())];

        // 1) application/octet-stream
        let builder = self
            .http()
            .post(&url)
            .header(HOST, &endpoint.host)
            .query(&query)
            .header(CONTENT_TYPE, "application/octet-stream")
            .body(data.clone());
        let response = self.send(builder).await?;
        if response.status().is_success() {
            return Ok(());
        }
        if !response.status().is_client_error() {
            return self.handle_unit(response).await;
        }

        // 2) multipart/form-data 回退
        tracing::debug!(sandbox = %id, path = %path, "octet-stream 被拒绝，回退 multipart 上传");
        let (content_type, body) = multipart_body(path, &data);
        let builder = self
            .http()
            .post(&url)
            .header(HOST, &endpoint.host)
            .query(&query)
            .header(CONTENT_TYPE, content_type)
            .body(body);
        let response = self.send(builder).await?;
        self.handle_unit(response).await
    }
}

/// 把契约请求翻译成 `(cmd, args)`
///
/// - `code` → `{SANDBOX_PYTHON} -c <code>`
/// - `command` + 无 `argv` → `/bin/sh -lc <command>`（保留管道 / 重定向语义）
/// - `command` + `argv` → 直接以 `command` 为可执行文件，避免自行拼 shell 字符串
fn split_request(
    options: &CubeOptions,
    request: &ExecRequest,
) -> SandboxResult<(String, Vec<String>)> {
    if let Some(code) = request.code.as_deref().filter(|code| !code.trim().is_empty()) {
        if code.len() > sandbox::MAX_CODE_BYTES {
            return Err(SandboxError::invalid_request(
                400,
                format!("代码超过上限 {} 字节", sandbox::MAX_CODE_BYTES),
            ));
        }
        return Ok((
            options.python.clone(),
            vec!["-c".to_string(), code.to_string()],
        ));
    }

    let Some(command) = request
        .command
        .as_deref()
        .map(str::trim)
        .filter(|command| !command.is_empty())
    else {
        return Err(SandboxError::invalid_request(
            400,
            "code 与 command 至少需要提供一个",
        ));
    };

    if request.argv.is_empty() {
        Ok((
            "/bin/sh".to_string(),
            vec!["-lc".to_string(), command.to_string()],
        ))
    } else {
        Ok((command.to_string(), request.argv.clone()))
    }
}

/// 组装 multipart 请求体（不引入 reqwest 的 multipart feature）
fn multipart_body(path: &str, data: &[u8]) -> (String, Vec<u8>) {
    let filename = path
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or("file");

    let head = format!(
        "--{MULTIPART_BOUNDARY}\r\n\
         Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n\
         Content-Type: application/octet-stream\r\n\r\n"
    );
    let mut body = Vec::with_capacity(head.len() + data.len() + 64);
    body.extend_from_slice(head.as_bytes());
    body.extend_from_slice(data);
    body.extend_from_slice(format!("\r\n--{MULTIPART_BOUNDARY}--\r\n").as_bytes());

    (
        format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
        body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_request_maps_code_to_python() {
        let options = CubeOptions::default();
        let (cmd, args) = split_request(&options, &ExecRequest::code("print(1)")).expect("应可映射");
        assert_eq!(cmd, "python3");
        assert_eq!(args, vec!["-c".to_string(), "print(1)".to_string()]);
    }

    #[test]
    fn split_request_wraps_command_in_shell() {
        let options = CubeOptions::default();
        let request = ExecRequest::command("echo hi | tr a-z A-Z");
        let (cmd, args) = split_request(&options, &request).expect("应可映射");
        assert_eq!(cmd, "/bin/sh");
        assert_eq!(args[0], "-lc");
        assert_eq!(args[1], "echo hi | tr a-z A-Z");
    }

    #[test]
    fn split_request_keeps_argv_verbatim() {
        let options = CubeOptions::default();
        let mut request = ExecRequest::command("ls");
        request.argv = vec!["-l".to_string(), "/tmp".to_string()];
        let (cmd, args) = split_request(&options, &request).expect("应可映射");
        assert_eq!(cmd, "ls");
        assert_eq!(args, vec!["-l".to_string(), "/tmp".to_string()]);
    }

    #[test]
    fn split_request_rejects_empty_and_oversized_input() {
        let options = CubeOptions::default();
        let error = split_request(&options, &ExecRequest::default()).expect_err("空请求必须报错");
        assert_eq!(error.kind(), "invalid_request");

        let oversized = "x".repeat(sandbox::MAX_CODE_BYTES + 1);
        let error = split_request(&options, &ExecRequest::code(oversized)).expect_err("超限必须报错");
        assert_eq!(error.kind(), "invalid_request");
    }

    #[test]
    fn multipart_body_carries_filename_and_payload() {
        let (content_type, body) = multipart_body("/work/a.py", b"print(1)");
        assert!(content_type.starts_with("multipart/form-data; boundary="));
        let text = String::from_utf8(body).expect("body 应为 UTF-8");
        assert!(text.contains("filename=\"a.py\""), "{text}");
        assert!(text.contains("print(1)"), "{text}");
        assert!(text.ends_with("--\r\n"));
    }

    #[test]
    fn data_endpoint_uses_virtual_host_by_default() {
        let runtime = CubeSandboxRuntime::new(CubeOptions::default()).expect("应可构造");
        let endpoint = runtime.data_endpoint("sb-1");
        assert_eq!(endpoint.host, "49983-sb-1.cube.app");
        assert_eq!(endpoint.base, "http://49983-sb-1.cube.app");
    }

    #[test]
    fn data_endpoint_uses_node_ip_when_configured() {
        let runtime = CubeSandboxRuntime::new(CubeOptions {
            proxy_node_ip: Some("10.0.0.9".to_string()),
            envd_port: 49999,
            ..CubeOptions::default()
        })
        .expect("应可构造");
        let endpoint = runtime.data_endpoint("sb-1");
        assert_eq!(endpoint.base, "http://10.0.0.9:49999");
        assert_eq!(endpoint.host, "49999-sb-1.cube.app");
    }
}
