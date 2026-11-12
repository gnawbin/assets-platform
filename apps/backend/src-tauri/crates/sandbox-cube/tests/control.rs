//! 控制面 / 数据面集成测试：手写假 CubeAPI（风格对齐 `crates/ops` 的 `spawn_fake_docker`）
//!
//! 覆盖：请求形状与鉴权头、状态码分类映射、幂等语义、octet-stream → multipart 回退、
//! 虚拟 Host 头、执行事件解析。**不引入 wiremock / httpmock**。

use std::sync::{Arc, Mutex};

use sandbox::{ExecRequest, SandboxRuntime, SandboxSpec};
use sandbox_cube::{CubeOptions, CubeSandboxRuntime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// 假服务捕获到的请求
#[derive(Debug, Clone)]
struct Captured {
    method: String,
    path: String,
    host: Option<String>,
    api_key: Option<String>,
    content_type: Option<String>,
    body: String,
}

/// 假服务的固定响应
#[derive(Debug, Clone)]
struct Reply {
    status: u16,
    body: String,
    headers: Vec<(String, String)>,
}

fn reply(status: u16, body: &str) -> Reply {
    Reply {
        status,
        body: body.to_string(),
        headers: Vec::new(),
    }
}

fn reply_with(status: u16, body: &str, headers: &[(&str, &str)]) -> Reply {
    Reply {
        status,
        body: body.to_string(),
        headers: headers
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect(),
    }
}

struct Fake {
    base: String,
    port: u16,
    captured: Arc<Mutex<Vec<Captured>>>,
}

impl Fake {
    fn requests(&self) -> Vec<Captured> {
        self.captured.lock().expect("锁未中毒").clone()
    }
}

/// 启动假服务：按顺序返回 `replies`（超出后重复最后一条）
async fn spawn_fake(replies: Vec<Reply>) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("绑定端口失败");
    let addr = listener.local_addr().expect("读取本地地址失败");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&captured);
    let last = replies.last().cloned();

    tokio::spawn(async move {
        let mut index = 0usize;
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            let Some(request) = read_request(&mut socket).await else {
                continue;
            };
            sink.lock().expect("锁未中毒").push(request);

            let next = replies.get(index).cloned().or_else(|| last.clone());
            index += 1;
            if let Some(reply) = next {
                let _ = write_reply(&mut socket, &reply).await;
            }
        }
    });

    Fake {
        base: format!("http://{addr}"),
        port: addr.port(),
        captured,
    }
}

/// 读取一个 HTTP/1.1 请求（足够覆盖本 crate 的请求形状）
async fn read_request(socket: &mut TcpStream) -> Option<Captured> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    let header_end = loop {
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(pos) = find(&buffer, b"\r\n\r\n") {
            break pos;
        }
    };

    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut parts = lines.next()?.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();

    let mut host = None;
    let mut api_key = None;
    let mut content_type = None;
    let mut content_length = 0usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().to_string();
        match name.trim().to_ascii_lowercase().as_str() {
            "host" => host = Some(value),
            "x-api-key" => api_key = Some(value),
            "content-type" => content_type = Some(value),
            "content-length" => content_length = value.parse().unwrap_or(0),
            _ => {}
        }
    }

    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < content_length {
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(content_length);

    Some(Captured {
        method,
        path,
        host,
        api_key,
        content_type,
        body: String::from_utf8_lossy(&body).to_string(),
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

async fn write_reply(socket: &mut TcpStream, reply: &Reply) -> std::io::Result<()> {
    let mut response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        reply.status,
        reason(reply.status),
        reply.body.len()
    );
    for (name, value) in &reply.headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("\r\n");
    response.push_str(&reply.body);
    socket.write_all(response.as_bytes()).await?;
    socket.flush().await
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        409 => "Conflict",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        _ => "Error",
    }
}

/// 构造指向假服务的运行时
fn runtime(fake: &Fake) -> CubeSandboxRuntime {
    CubeSandboxRuntime::new(CubeOptions {
        base_url: fake.base.clone(),
        api_key: "test-key".to_string(),
        default_template_id: "tpl-default".to_string(),
        data_host_suffix: "cube.app".to_string(),
        envd_port: fake.port,
        proxy_node_ip: Some("127.0.0.1".to_string()),
        ..CubeOptions::default()
    })
    .expect("运行时构造失败")
}

#[tokio::test]
async fn create_sends_expected_request_and_parses_handle() {
    let fake = spawn_fake(vec![reply(
        201,
        r#"{"sandboxID":"sb-42","state":"running","template":"py"}"#,
    )])
    .await;
    let runtime = runtime(&fake);

    let handle = runtime
        .create(SandboxSpec {
            template_id: "py".to_string(),
            timeout_secs: 120,
            ..SandboxSpec::default()
        })
        .await
        .expect("创建应成功");

    assert_eq!(handle.id, "sb-42");
    assert_eq!(handle.template_id.as_deref(), Some("py"));
    assert_eq!(
        handle.data_host(fake.port),
        format!("{}-sb-42.cube.app", fake.port)
    );

    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/cubeapi/v1/sandboxes");
    assert_eq!(requests[0].api_key.as_deref(), Some("test-key"));
    assert!(
        requests[0].body.contains("\"template\":\"py\""),
        "{}",
        requests[0].body
    );
    assert!(
        requests[0].body.contains("\"timeout\":120"),
        "{}",
        requests[0].body
    );
}

#[tokio::test]
async fn create_falls_back_to_default_template() {
    let fake = spawn_fake(vec![reply(201, r#"{"sandbox_id":"sb-1"}"#)]).await;
    let runtime = runtime(&fake);

    runtime
        .create(SandboxSpec::default())
        .await
        .expect("缺省模板应可用");

    let requests = fake.requests();
    assert!(
        requests[0].body.contains("\"template\":\"tpl-default\""),
        "{}",
        requests[0].body
    );
}

#[tokio::test]
async fn create_without_any_template_is_config_error() {
    let runtime = CubeSandboxRuntime::new(CubeOptions {
        base_url: "http://127.0.0.1:1".to_string(),
        default_template_id: String::new(),
        ..CubeOptions::default()
    })
    .expect("应可构造");

    let error = runtime
        .create(SandboxSpec::default())
        .await
        .expect_err("无模板必须报错");
    assert_eq!(error.kind(), "config");
}

#[tokio::test]
async fn create_maps_auth_and_invalid_request_and_server_errors() {
    for (status, expected) in [(401_u16, "auth"), (400, "invalid_request"), (500, "server")] {
        let fake = spawn_fake(vec![reply(status, r#"{"message":"boom"}"#)]).await;
        let runtime = runtime(&fake);

        let error = runtime
            .create(SandboxSpec {
                template_id: "py".to_string(),
                ..SandboxSpec::default()
            })
            .await
            .expect_err("应返回错误");
        assert_eq!(error.kind(), expected, "status={status}");
    }
}

#[tokio::test]
async fn rate_limit_carries_retry_after() {
    let fake = spawn_fake(vec![reply_with(
        429,
        r#"{"message":"slow down"}"#,
        &[("Retry-After", "3")],
    )])
    .await;
    let runtime = runtime(&fake);

    let error = runtime
        .create(SandboxSpec {
            template_id: "py".to_string(),
            ..SandboxSpec::default()
        })
        .await
        .expect_err("应触发限流");
    assert_eq!(error.kind(), "rate_limited");
    assert_eq!(error.retry_after(), Some(std::time::Duration::from_secs(3)));
    assert!(error.is_retryable());
}

#[tokio::test]
async fn kill_is_idempotent_on_404() {
    let fake = spawn_fake(vec![reply(404, r#"{"message":"gone"}"#)]).await;
    let runtime = runtime(&fake);

    runtime.kill("sb-gone").await.expect("kill 404 应幂等成功");

    let requests = fake.requests();
    assert_eq!(requests[0].method, "DELETE");
    assert_eq!(requests[0].path, "/cubeapi/v1/sandboxes/sb-gone");
}

#[tokio::test]
async fn resume_maps_404_to_not_found() {
    let fake = spawn_fake(vec![reply(404, r#"{"message":"gone"}"#)]).await;
    let runtime = runtime(&fake);

    let error = runtime.resume("sb-gone").await.expect_err("resume 404 应报错");
    assert_eq!(error.kind(), "not_found");
    assert!(!error.is_retryable());
}

#[tokio::test]
async fn write_file_falls_back_to_multipart_after_octet_stream_rejected() {
    let fake = spawn_fake(vec![reply(415, ""), reply(200, "")]).await;
    let runtime = runtime(&fake);

    runtime
        .write_file("sb-1", "/work/a.py", "root", b"print(1)".to_vec())
        .await
        .expect("multipart 回退应成功");

    let requests = fake.requests();
    assert_eq!(requests.len(), 2, "应有两次尝试");
    assert_eq!(
        requests[0].content_type.as_deref(),
        Some("application/octet-stream")
    );
    let second = requests[1].content_type.clone().unwrap_or_default();
    assert!(
        second.starts_with("multipart/form-data; boundary="),
        "{second}"
    );
    let expected_host = format!("{}-sb-1.cube.app", fake.port);
    assert_eq!(requests[1].host.as_deref(), Some(expected_host.as_str()));
}

#[tokio::test]
async fn exec_posts_process_start_and_parses_events() {
    let fake = spawn_fake(vec![reply(200, r#"[{"stdout":"2\n"},{"exit_code":0}]"#)]).await;
    let runtime = runtime(&fake);

    let result = runtime
        .exec("sb-1", ExecRequest::code("print(1 + 1)"))
        .await
        .expect("执行应成功");

    assert_eq!(result.text.as_deref(), Some("2"));
    assert_eq!(result.stdout, vec!["2"]);
    assert_eq!(result.exit_code, 0);
    assert!(result.is_ok());

    let requests = fake.requests();
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/process.Process/Start");
    let expected_host = format!("{}-sb-1.cube.app", fake.port);
    assert_eq!(requests[0].host.as_deref(), Some(expected_host.as_str()));
    assert!(
        requests[0].body.contains("python3"),
        "{}",
        requests[0].body
    );
    assert!(
        requests[0].body.contains("print(1 + 1)"),
        "{}",
        requests[0].body
    );
}

#[tokio::test]
async fn list_sends_metadata_and_state_filters() {
    let fake = spawn_fake(vec![reply(
        200,
        r#"[{"sandboxID":"sb-1","state":"running","metadata":{"tenant":"t1"}}]"#,
    )])
    .await;
    let runtime = runtime(&fake);

    let summaries = runtime
        .list(sandbox::SandboxFilter {
            metadata: [("tenant".to_string(), "t1".to_string())]
                .into_iter()
                .collect(),
            states: vec![sandbox::SandboxState::Running],
            limit: Some(5000),
        })
        .await
        .expect("列表应成功");

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].id, "sb-1");
    assert_eq!(
        summaries[0].metadata.get("tenant").map(String::as_str),
        Some("t1")
    );

    let requests = fake.requests();
    let path = &requests[0].path;
    assert!(path.contains("limit=500"), "{path}");
    assert!(path.contains("metadata=tenant%3Dt1"), "{path}");
    assert!(path.contains("state=running"), "{path}");
}
