//! `OpenShell` `gateway` 连接选项（`Debug` 已脱敏 `token`）
//!
//! 环境变量（`bootstrap::config` 由 `.env.toml` 的 `[sandbox]` 段映射）：
//! - `SANDBOX_OPENSHELL_GATEWAY_URL`（必填，gRPC `:8080`）
//! - `SANDBOX_OPENSHELL_TOKEN`（Bearer，留空 = 无鉴权）
//! - `SANDBOX_OPENSHELL_CA_CERT`（私有 CA 的 PEM 路径）
//! - `SANDBOX_OPENSHELL_WORKSPACE`（缺省 `default`）
//! - `SANDBOX_OPENSHELL_READY_TIMEOUT_SECS`（缺省 120）
//! - `SANDBOX_OPENSHELL_IMAGE` / `SANDBOX_OPENSHELL_COMMAND`（逗号串）
//! - `SANDBOX_OPENSHELL_PYTHON`（`code` 分支解释器，缺省 `python3`）
//! - `SANDBOX_TIMEOUT_SECS`（单次执行超时）

use std::fmt;
use std::time::Duration;

use sandbox::{SandboxError, SandboxResult};
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint};

/// `OpenShell` `gateway` 连接与默认规格
#[derive(Clone)]
pub struct OpenShellOptions {
    /// `gateway` 地址（`http://host:8080` / `https://...`）
    pub gateway: String,
    /// `Bearer` `token`（明文只存在于内存）
    pub token: Option<String>,
    /// 私有 CA 的 PEM 路径
    pub ca_cert_path: Option<String>,
    /// 目标 workspace
    pub workspace: String,
    /// 建箱后等待 Ready 的超时
    pub ready_timeout: Duration,
    /// 建连超时
    pub connect_timeout: Duration,
    /// 默认镜像
    pub default_image: Option<String>,
    /// 默认主进程命令
    pub default_command: Vec<String>,
    /// `code` 分支解释器
    pub python: String,
    /// 单次执行超时
    pub exec_timeout: Duration,
}

impl OpenShellOptions {
    /// 从环境变量读取（`gateway` 未配置时直接报配置错误）
    pub fn from_env() -> SandboxResult<Self> {
        let gateway = env_trim("SANDBOX_OPENSHELL_GATEWAY_URL").ok_or_else(|| {
            SandboxError::config(
                "mode=openshell 需要 SANDBOX_OPENSHELL_GATEWAY_URL（gateway 独立部署的 gRPC 地址）",
            )
        })?;

        Ok(Self {
            gateway,
            token: env_trim("SANDBOX_OPENSHELL_TOKEN"),
            ca_cert_path: env_trim("SANDBOX_OPENSHELL_CA_CERT"),
            workspace: env_trim("SANDBOX_OPENSHELL_WORKSPACE")
                .unwrap_or_else(|| "default".to_string()),
            ready_timeout: Duration::from_secs(
                parse_u64("SANDBOX_OPENSHELL_READY_TIMEOUT_SECS", 120).max(1),
            ),
            connect_timeout: Duration::from_secs(
                parse_u64("SANDBOX_OPENSHELL_CONNECT_TIMEOUT_SECS", 10).max(1),
            ),
            default_image: env_trim("SANDBOX_OPENSHELL_IMAGE"),
            default_command: env_trim("SANDBOX_OPENSHELL_COMMAND")
                .map(|raw| {
                    raw.split(',')
                        .map(str::trim)
                        .filter(|part| !part.is_empty())
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            python: env_trim("SANDBOX_OPENSHELL_PYTHON").unwrap_or_else(|| "python3".to_string()),
            exec_timeout: Duration::from_secs(parse_u64("SANDBOX_TIMEOUT_SECS", 60).max(1)),
        })
    }

    /// 建 lazily 连接的 Channel（`connect_lazy` 不发起拨号，首次 RPC 才连）
    pub fn channel(&self) -> SandboxResult<Channel> {
        let mut endpoint = Endpoint::from_shared(self.gateway.clone())
            .map_err(|error| SandboxError::config(format!("gateway 地址非法：{error}")))?
            .connect_timeout(self.connect_timeout)
            .timeout(self.exec_timeout);

        if self.gateway.starts_with("https://") {
            let mut tls = ClientTlsConfig::new();
            if let Some(path) = self
                .ca_cert_path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                let pem = std::fs::read(path).map_err(|error| {
                    SandboxError::config(format!("CA 证书读取失败 {path}：{error}"))
                })?;
                tls = tls.ca_certificate(Certificate::from_pem(pem));
            }
            endpoint = endpoint
                .tls_config(tls)
                .map_err(|error| SandboxError::config(format!("TLS 配置失败：{error}")))?;
        }

        Ok(endpoint.connect_lazy())
    }
}

fn env_trim(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_u64(key: &str, default: u64) -> u64 {
    env_trim(key)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

impl fmt::Debug for OpenShellOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenShellOptions")
            .field("gateway", &self.gateway)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .field("ca_cert_path", &self.ca_cert_path)
            .field("workspace", &self.workspace)
            .field("ready_timeout", &self.ready_timeout)
            .field("connect_timeout", &self.connect_timeout)
            .field("default_image", &self.default_image)
            .field("default_command", &self.default_command)
            .field("python", &self.python)
            .field("exec_timeout", &self.exec_timeout)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> OpenShellOptions {
        OpenShellOptions {
            gateway: "http://127.0.0.1:8080".to_string(),
            token: Some("secret-token".to_string()),
            ca_cert_path: None,
            workspace: "default".to_string(),
            ready_timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(5),
            default_image: None,
            default_command: Vec::new(),
            python: "python3".to_string(),
            exec_timeout: Duration::from_secs(60),
        }
    }

    #[test]
    fn debug_redacts_token() {
        let text = format!("{:?}", options());
        assert!(text.contains("<redacted>"), "{text}");
        assert!(!text.contains("secret-token"), "{text}");
    }

    #[tokio::test]
    async fn plaintext_gateway_builds_channel() {
        // `connect_lazy` 需要 Tokio reactor：必须在运行时上下文里建通道
        assert!(options().channel().is_ok());
    }

    #[test]
    fn invalid_gateway_is_config_error() {
        let mut broken = options();
        broken.gateway = "not a url".to_string();
        let error = broken.channel().expect_err("非法地址必须报错");
        assert_eq!(error.kind(), "config");
    }
}
