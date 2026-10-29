//! 运维采集统一错误类型与故障分类
//!
//! 连接失败必须能被运维人员「看懂并直接行动」，因此错误分为两层：
//! 1. [`OpsError`]：面向代码的精确错误（携带原始消息）；
//! 2. [`FailureClass`]：面向界面的故障归类（网络不可达 / TLS 握手 / 401 / 403 / 版本不匹配 ...），
//!    由 [`OpsError::classify`] 推导，连通性探测（`examples/ops_probe.rs`）直接输出它。

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// 运维采集统一结果类型
pub type OpsResult<T> = Result<T, OpsError>;

/// 运维采集错误
#[derive(Debug, Clone, thiserror::Error)]
pub enum OpsError {
    /// 当前构建未启用对应连接器 feature（`docker` / `k8s`）
    #[error("当前构建未启用所需的连接器特性：{0}")]
    Unsupported(&'static str),

    /// 连接配置非法（缺端点、缺凭证、协议不匹配 ...）
    #[error("连接配置非法：{0}")]
    Config(String),

    /// 连接建立失败（DNS、TCP 拒绝、socket 不存在 ...）
    #[error("连接失败：{0}")]
    Connect(String),

    /// 请求超时
    #[error("请求超时（{} ms）", .0.as_millis())]
    Timeout(Duration),

    /// TLS 握手 / 证书校验失败
    #[error("TLS 握手或证书校验失败：{0}")]
    Tls(String),

    /// 认证失败（HTTP 401 / Docker unauthorized）
    #[error("认证失败（401）：{0}")]
    Auth(String),

    /// 权限不足（HTTP 403 / Docker forbidden）
    #[error("权限不足（403）：{0}")]
    Forbidden(String),

    /// API 调用失败（含 API 版本不匹配）
    #[error("API 调用失败：{0}")]
    Api(String),

    /// 响应解析失败
    #[error("响应解析失败：{0}")]
    Parse(String),

    /// 内部错误（未预期的分支）
    #[error("内部错误：{0}")]
    Internal(String),
}

impl OpsError {
    /// 故障归类，供界面与探针直接展示
    pub fn classify(&self) -> FailureClass {
        match self {
            Self::Unsupported(_) | Self::Config(_) | Self::Internal(_) => FailureClass::Config,
            Self::Timeout(_) => FailureClass::Timeout,
            Self::Tls(_) => FailureClass::Tls,
            Self::Auth(_) => FailureClass::Auth,
            Self::Forbidden(_) => FailureClass::Forbidden,
            Self::Parse(_) => FailureClass::Api,
            Self::Api(message) => {
                if looks_like_version_mismatch(message) {
                    FailureClass::VersionMismatch
                } else {
                    FailureClass::Api
                }
            }
            Self::Connect(message) => classify_connect(message),
        }
    }

    /// 是否值得重试（网络抖动、超时可重试；认证/配置错误重试无意义）
    pub fn is_retryable(&self) -> bool {
        matches!(
            self.classify(),
            FailureClass::Network | FailureClass::Timeout | FailureClass::Api
        )
    }
}

/// 连通性/权限故障归类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    /// 探测成功，无故障
    None,
    /// 域名解析失败（DNS / 内网域名未配置）
    Dns,
    /// 主机或端口不可达（拒绝连接、网络不可达、socket 不存在）
    Network,
    /// 请求超时（防火墙丢包、链路慢）
    Timeout,
    /// TLS 握手 / 证书链 / 主机名校验失败
    Tls,
    /// 认证失败（401，凭证错误或过期）
    Auth,
    /// 权限不足（403，凭证有效但 RBAC/角色不足）
    Forbidden,
    /// API 版本不匹配（Docker API 版本过老、K8s 版本超出 kube-rs 支持范围）
    VersionMismatch,
    /// 其它 API/协议错误（含 404、响应解析失败）
    Api,
    /// 配置或内部错误（端点/凭证未填全）
    Config,
}

impl FailureClass {
    /// 稳定字符串编码（落库、接口返回、前端判定都用它）
    pub const fn code(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Dns => "dns",
            Self::Network => "network",
            Self::Timeout => "timeout",
            Self::Tls => "tls",
            Self::Auth => "auth",
            Self::Forbidden => "forbidden",
            Self::VersionMismatch => "version_mismatch",
            Self::Api => "api",
            Self::Config => "config",
        }
    }

    /// 中文标签
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "正常",
            Self::Dns => "域名解析失败",
            Self::Network => "网络不可达",
            Self::Timeout => "请求超时",
            Self::Tls => "TLS 握手失败",
            Self::Auth => "认证失败（401）",
            Self::Forbidden => "权限不足（403）",
            Self::VersionMismatch => "API 版本不匹配",
            Self::Api => "接口调用失败",
            Self::Config => "配置错误",
        }
    }

    /// 排查建议（直接写给运维看的下一步动作）
    pub const fn hint(self) -> &'static str {
        match self {
            Self::None => "无需处理",
            Self::Dns => "检查端点域名拼写、DNS 服务器与内网解析策略",
            Self::Network => {
                "检查 host:port 是否放通（安全组/防火墙），服务是否监听且未只绑 127.0.0.1"
            }
            Self::Timeout => "检查链路质量与防火墙丢包，必要时调大 OPS_TIMEOUT_SECS",
            Self::Tls => "确认 CA 证书、客户端证书/私钥是否配对，服务端证书是否包含正确 SAN",
            Self::Auth => "核对 Token / 客户端证书是否过期，Docker 是否开启 TLS 校验",
            Self::Forbidden => "为该凭证补齐只读权限（Docker 只读角色 / K8s RBAC get-list 权限）",
            Self::VersionMismatch => {
                "确认服务端版本在支持范围内（Docker API ≥ 1.20、K8s 约 1.32+）"
            }
            Self::Api => "查看 error 原文，必要时开启 RUST_LOG=ops=debug 复现",
            Self::Config => "补全连接配置（端点、认证方式、证书路径）",
        }
    }
}

impl std::fmt::Display for FailureClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// 从连接错误原文里识别 DNS / 网络不可达
fn classify_connect(message: &str) -> FailureClass {
    let lowered = message.to_ascii_lowercase();
    const DNS_MARKERS: [&str; 8] = [
        "dns error",
        "failed to lookup address",
        "name or service not known",
        "nodename nor servname",
        "no such host",
        "getaddrinfo",
        "temporary failure in name resolution",
        "os error 11001",
    ];
    if DNS_MARKERS.iter().any(|marker| lowered.contains(marker)) {
        return FailureClass::Dns;
    }
    if lowered.contains("timed out") || lowered.contains("timeout") {
        return FailureClass::Timeout;
    }
    FailureClass::Network
}

/// 从 API 错误原文里识别版本不匹配
fn looks_like_version_mismatch(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    const MARKERS: [&str; 6] = [
        "client version",
        "too old",
        "api version",
        "unsupported version",
        "versions of the api",
        "version negotiation",
    ];
    MARKERS.iter().any(|marker| lowered.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_error_is_classified() {
        let err = OpsError::Connect(
            "error trying to connect: dns error: failed to lookup address information".to_string(),
        );
        assert_eq!(err.classify(), FailureClass::Dns);
        assert_eq!(err.classify().code(), "dns");
    }

    #[test]
    fn refused_connection_is_network() {
        let err = OpsError::Connect(
            "error trying to connect: tcp connect error: 由于目标计算机积极拒绝，无法连接。 (os error 10061)"
                .to_string(),
        );
        assert_eq!(err.classify(), FailureClass::Network);
        assert!(err.is_retryable());
    }

    #[test]
    fn error_variants_map_to_stable_classes() {
        assert_eq!(OpsError::Auth("401".into()).classify(), FailureClass::Auth);
        assert_eq!(
            OpsError::Forbidden("403".into()).classify(),
            FailureClass::Forbidden
        );
        assert_eq!(
            OpsError::Tls("certificate verify failed".into()).classify(),
            FailureClass::Tls
        );
        assert_eq!(
            OpsError::Timeout(Duration::from_secs(5)).classify(),
            FailureClass::Timeout
        );
        assert_eq!(
            OpsError::Config("缺少端点".into()).classify(),
            FailureClass::Config
        );
        assert_eq!(
            OpsError::Unsupported("k8s").classify(),
            FailureClass::Config
        );
        assert!(!OpsError::Auth("401".into()).is_retryable());
    }

    #[test]
    fn api_version_mismatch_is_detected() {
        let err = OpsError::Api(
            "client version 1.20 is too old. Minimum supported API version is 1.24".to_string(),
        );
        assert_eq!(err.classify(), FailureClass::VersionMismatch);
        assert_eq!(err.classify().code(), "version_mismatch");
    }

    #[test]
    fn failure_class_labels_and_hints_are_complete() {
        for class in [
            FailureClass::None,
            FailureClass::Dns,
            FailureClass::Network,
            FailureClass::Timeout,
            FailureClass::Tls,
            FailureClass::Auth,
            FailureClass::Forbidden,
            FailureClass::VersionMismatch,
            FailureClass::Api,
            FailureClass::Config,
        ] {
            assert!(!class.label().is_empty(), "{} 缺少中文标签", class.code());
            assert!(!class.hint().is_empty(), "{} 缺少排查建议", class.code());
        }
    }
}
