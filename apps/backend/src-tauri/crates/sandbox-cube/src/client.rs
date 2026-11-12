//! CubeSandbox 客户端与连接配置

use std::fmt;
use std::time::Duration;

use reqwest::header::RETRY_AFTER;
use sandbox::{SandboxError, SandboxResult};
use serde::de::DeserializeOwned;

use crate::error::{map_error, parse_retry_after};

/// CubeAPI 缺省端口（控制面）
pub const DEFAULT_CUBEAPI_PORT: u16 = 3000;

/// 控制面路径前缀
const API_PREFIX: &str = "/cubeapi/v1";

/// CubeSandbox 连接参数
#[derive(Clone)]
pub struct CubeOptions {
    /// CubeAPI 根地址，如 `http://10.0.0.12:3000`（可带 `/cubeapi/v1`，会被容错去掉再拼）
    pub base_url: String,
    /// `X-API-Key`；为空则以无鉴权方式访问（仅建议本地调试）
    pub api_key: String,
    /// 缺省模板（ID 或别名）
    pub default_template_id: String,
    /// 数据面 Host 后缀，如 `cube.app`
    pub data_host_suffix: String,
    /// 数据面 envd 端口
    pub envd_port: u16,
    /// 无 DNS 环境：直连该 IP 并保留虚拟 Host 头（对应官方 `CUBE_PROXY_NODE_IP`）
    pub proxy_node_ip: Option<String>,
    /// 控制面请求超时
    pub timeout: Duration,
    /// 代码执行的解释器（缺省 `python3`，可换成模板内的解释器路径）
    pub python: String,
}

impl Default for CubeOptions {
    fn default() -> Self {
        Self {
            base_url: format!("http://127.0.0.1:{DEFAULT_CUBEAPI_PORT}"),
            api_key: String::new(),
            default_template_id: String::new(),
            data_host_suffix: "cube.app".to_string(),
            envd_port: sandbox::DEFAULT_ENVD_PORT,
            proxy_node_ip: None,
            timeout: Duration::from_secs(u64::from(sandbox::DEFAULT_TIMEOUT_SECS)),
            python: "python3".to_string(),
        }
    }
}

/// 手写 `Debug`：`api_key` 必须脱敏（对齐 `llm::LlmClientConfig`）
impl fmt::Debug for CubeOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CubeOptions")
            .field("base_url", &self.base_url)
            .field("api_key", &mask(&self.api_key))
            .field("default_template_id", &self.default_template_id)
            .field("data_host_suffix", &self.data_host_suffix)
            .field("envd_port", &self.envd_port)
            .field("proxy_node_ip", &self.proxy_node_ip)
            .field("timeout", &self.timeout)
            .field("python", &self.python)
            .finish()
    }
}

/// 密钥脱敏：保留前 3 后 2；长度不足 8 时整体打码
fn mask(key: &str) -> String {
    let len = key.chars().count();
    if len == 0 {
        return String::new();
    }
    if len < 8 {
        return "***".to_string();
    }
    let head: String = key.chars().take(3).collect();
    let tail: String = key.chars().skip(len - 2).collect();
    format!("{head}***{tail}")
}

impl CubeOptions {
    /// 从环境变量构造（`SANDBOX_*`，由 `.env.toml` 的 `[sandbox]` 段映射而来）
    ///
    /// 校验规则：
    /// - `SANDBOX_BASE_URL` 为空 → [`SandboxError::Config`]
    /// - `SANDBOX_API_KEY` 为空**不报错**（CubeAPI 允许关闭鉴权）
    /// - `SANDBOX_TEMPLATE_ID` 为空**不报错**（`create` 时才要求）
    pub fn from_env() -> SandboxResult<Self> {
        let defaults = Self::default();

        let base_url = env_trim("SANDBOX_BASE_URL").unwrap_or(defaults.base_url);
        if base_url.is_empty() {
            return Err(SandboxError::config("SANDBOX_BASE_URL 为空"));
        }

        Ok(Self {
            base_url,
            api_key: env_trim("SANDBOX_API_KEY").unwrap_or_default(),
            default_template_id: env_trim("SANDBOX_TEMPLATE_ID").unwrap_or_default(),
            data_host_suffix: env_trim("SANDBOX_DATA_HOST_SUFFIX")
                .unwrap_or(defaults.data_host_suffix),
            envd_port: env_parse("SANDBOX_ENVD_PORT").unwrap_or(defaults.envd_port),
            proxy_node_ip: env_trim("SANDBOX_PROXY_NODE_IP").filter(|s| !s.is_empty()),
            timeout: Duration::from_secs(
                env_parse("SANDBOX_TIMEOUT_SECS")
                    .unwrap_or_else(|| u64::from(sandbox::DEFAULT_TIMEOUT_SECS))
                    .max(1),
            ),
            python: env_trim("SANDBOX_PYTHON").unwrap_or(defaults.python),
        })
    }

    /// `{base_url}/cubeapi/v1`（去掉尾部 `/`，容忍用户误配 `/cubeapi/v1` 后缀）
    #[must_use]
    pub fn control_base(&self) -> String {
        let mut base = self.base_url.trim().trim_end_matches('/').to_string();
        if let Some(stripped) = base.strip_suffix(API_PREFIX) {
            base = stripped.trim_end_matches('/').to_string();
        }
        format!("{base}{API_PREFIX}")
    }
}

/// 读取环境变量并 trim（空串视为未设置）
fn env_trim(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string())
}

/// 读取并解析环境变量（解析失败视为未设置）
fn env_parse<T: std::str::FromStr>(key: &str) -> Option<T> {
    env_trim(key).and_then(|v| v.parse::<T>().ok())
}

/// CubeSandbox 运行时（控制面 + 数据面）
#[derive(Debug, Clone)]
pub struct CubeSandboxRuntime {
    options: CubeOptions,
    http: reqwest::Client,
    control_base: String,
    timeout_secs: u64,
}

impl CubeSandboxRuntime {
    /// 构造实现（校验 base_url 并创建 HTTP 客户端）
    ///
    /// # 错误
    /// `base_url` 为空或非 `http(s)://` 前缀时返回 [`SandboxError::Config`]。
    pub fn new(options: CubeOptions) -> SandboxResult<Self> {
        let base = options.base_url.trim();
        if base.is_empty() {
            return Err(SandboxError::config("CubeAPI base_url 为空"));
        }
        if !(base.starts_with("http://") || base.starts_with("https://")) {
            return Err(SandboxError::config(format!(
                "CubeAPI base_url 必须以 http:// 或 https:// 开头：{base}"
            )));
        }
        if options.api_key.trim().is_empty() {
            tracing::warn!("SANDBOX_API_KEY 为空：将以无鉴权方式访问 CubeAPI（仅建议本地调试）");
        }

        let http = reqwest::Client::builder()
            .timeout(options.timeout)
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("assets-platform/", env!("CARGO_PKG_NAME")))
            .build()
            .map_err(|e| SandboxError::config(format!("HTTP 客户端创建失败：{e}")))?;

        let control_base = options.control_base();
        let timeout_secs = options.timeout.as_secs().max(1);

        Ok(Self {
            options,
            http,
            control_base,
            timeout_secs,
        })
    }

    /// 当前连接参数（只读）
    #[must_use]
    pub fn options(&self) -> &CubeOptions {
        &self.options
    }

    /// 底层 HTTP 客户端（数据面复用）
    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// 控制面完整 URL
    pub(crate) fn control_url(&self, path: &str) -> String {
        format!("{}{path}", self.control_base)
    }

    /// 注入鉴权头：`X-API-Key`
    pub(crate) fn with_auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let key = self.options.api_key.trim();
        if key.is_empty() {
            builder
        } else {
            builder.header("X-API-Key", key)
        }
    }

    /// 消费响应：成功 → 反序列化；失败 → 分类错误
    pub(crate) async fn handle_json<T: DeserializeOwned>(
        &self,
        response: reqwest::Response,
    ) -> SandboxResult<T> {
        let response = self.ensure_success(response).await?;
        response
            .json::<T>()
            .await
            .map_err(|e| SandboxError::decode(format!("响应体解析失败：{e}")))
    }

    /// 消费响应：成功且响应体为空 → `None`（兼容 200/201/204 无体）
    pub(crate) async fn handle_optional_json<T: DeserializeOwned>(
        &self,
        response: reqwest::Response,
    ) -> SandboxResult<Option<T>> {
        let response = self.ensure_success(response).await?;
        let bytes = response
            .bytes()
            .await
            .map_err(|e| SandboxError::transport(format!("读取响应体失败：{e}")))?;
        if bytes.is_empty() {
            return Ok(None);
        }
        serde_json::from_slice::<T>(&bytes)
            .map(Some)
            .map_err(|e| SandboxError::decode(format!("响应体解析失败：{e}")))
    }

    /// 消费响应：只关心是否成功（2xx 即可）
    pub(crate) async fn handle_unit(&self, response: reqwest::Response) -> SandboxResult<()> {
        self.ensure_success(response).await.map(|_| ())
    }

    /// 消费响应：成功 → 原始文本（数据面自行解析事件流）
    pub(crate) async fn handle_text(&self, response: reqwest::Response) -> SandboxResult<String> {
        let response = self.ensure_success(response).await?;
        response
            .text()
            .await
            .map_err(|e| SandboxError::transport(format!("读取响应体失败：{e}")))
    }

    /// 消费响应：成功 → 原始字节（文件下载）
    pub(crate) async fn handle_bytes(&self, response: reqwest::Response) -> SandboxResult<Vec<u8>> {
        let response = self.ensure_success(response).await?;
        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|e| SandboxError::transport(format!("读取响应体失败：{e}")))
    }

    /// 发送请求并把网络层错误转成契约错误
    pub(crate) async fn send(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> SandboxResult<reqwest::Response> {
        builder.send().await.map_err(|e| self.transport_error(e))
    }

    /// 非 2xx → 分类错误（含 `Retry-After` 解析）
    async fn ensure_success(&self, response: reqwest::Response) -> SandboxResult<reqwest::Response> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }

        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|raw| parse_retry_after(Some(raw)));
        let body = response.text().await.unwrap_or_default();
        Err(map_error(status.as_u16(), &body, retry_after))
    }

    /// 网络层错误 → 契约错误（超时单独分类）
    pub(crate) fn transport_error(&self, error: reqwest::Error) -> SandboxError {
        if error.is_timeout() {
            SandboxError::timeout(self.timeout_secs)
        } else {
            SandboxError::transport(error.to_string())
        }
    }

    /// 配置的超时（秒）
    pub(crate) fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_base_normalizes_urls() {
        let cases = [
            ("http://cubeapi:3000", "http://cubeapi:3000/cubeapi/v1"),
            ("http://cubeapi:3000/", "http://cubeapi:3000/cubeapi/v1"),
            (
                "http://cubeapi:3000/cubeapi/v1",
                "http://cubeapi:3000/cubeapi/v1",
            ),
            (
                "http://cubeapi:3000/cubeapi/v1/",
                "http://cubeapi:3000/cubeapi/v1",
            ),
        ];
        for (input, expected) in cases {
            let options = CubeOptions {
                base_url: input.to_string(),
                ..CubeOptions::default()
            };
            assert_eq!(options.control_base(), expected, "input={input}");
        }
    }

    #[test]
    fn api_key_is_masked_in_debug() {
        let options = CubeOptions {
            api_key: "cube-secret-key-123456".to_string(),
            ..CubeOptions::default()
        };
        let rendered = format!("{options:?}");
        assert!(!rendered.contains("secret-key"), "{rendered}");
        assert!(rendered.contains("cub***56"), "{rendered}");
    }

    #[test]
    fn mask_handles_short_and_empty_keys() {
        assert_eq!(mask(""), "");
        assert_eq!(mask("abc"), "***");
        assert_eq!(mask("abcdefgh"), "abc***gh");
    }

    #[test]
    fn new_rejects_invalid_base_url() {
        let error = CubeSandboxRuntime::new(CubeOptions {
            base_url: "cubeapi:3000".to_string(),
            ..CubeOptions::default()
        })
        .expect_err("缺少 scheme 必须报错");
        assert_eq!(error.kind(), "config");

        let error = CubeSandboxRuntime::new(CubeOptions {
            base_url: "   ".to_string(),
            ..CubeOptions::default()
        })
        .expect_err("空 base_url 必须报错");
        assert_eq!(error.kind(), "config");
    }

    #[test]
    fn new_accepts_valid_base_url() {
        let runtime = CubeSandboxRuntime::new(CubeOptions::default()).expect("缺省配置应可用");
        assert_eq!(runtime.options().envd_port, sandbox::DEFAULT_ENVD_PORT);
        assert!(
            runtime
                .control_url("/health")
                .ends_with("/cubeapi/v1/health")
        );
    }
}
