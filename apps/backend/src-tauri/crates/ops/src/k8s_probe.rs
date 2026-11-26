//! K8s 侧日志 / 指标的视图与纯换算函数
//!
//! 与 [`crate::probe`] 同一定位：**不发请求**，只做参数组装与 JSON → 数值的换算；
//! 网络调用在 [`crate::connector::k8s::K8sConnector::pod_logs`] /
//! [`crate::connector::k8s::K8sConnector::pod_metrics`] 里。
//!
//! 指标来源限定为 **metrics-server**（`metrics.k8s.io/v1beta1`）：
//! 只有 CPU 与内存，默认 15s 聚合窗口，**没有**网络 / 块设备 / pids。
//! 未安装 metrics-server 时由 connector 映射成可读错误（见 `map_metrics_error`）。

use kube::api::LogParams;
use serde::{Deserialize, Serialize};

/// 默认行数（与 Docker 侧保持一致）
pub const DEFAULT_LOG_LINES: u32 = 200;
/// 行数硬上限
pub const MAX_LOG_LINES: u32 = 2000;
/// 默认字节上限 512 KiB
pub const DEFAULT_LOG_BYTES: usize = 512 * 1024;

/// Pod 日志拉取参数
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodLogsRequest {
    /// 命名空间
    pub namespace: String,
    /// Pod 名
    pub pod: String,
    /// 容器名；`None` 表示单容器 Pod 由服务端自行选择
    pub container: Option<String>,
    /// 是否读取**上一次运行**的日志（CrashLoopBackOff 场景）
    pub previous: bool,
    /// 末尾行数：夹到 `1..=2000`
    pub lines: u32,
    /// 是否让 kubelet 给每行加 RFC3339 时间戳
    pub timestamps: bool,
    /// 响应字节上限：`None` 用 512 KiB
    pub max_bytes: Option<usize>,
}

impl PodLogsRequest {
    /// 构造请求（容器 / 行数等按默认值）
    #[must_use]
    pub fn new(namespace: impl Into<String>, pod: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            pod: pod.into(),
            container: None,
            previous: false,
            lines: DEFAULT_LOG_LINES,
            timestamps: false,
            max_bytes: None,
        }
    }

    /// 规范化后的行数（`1..=2000`）
    #[must_use]
    pub fn line_limit(&self) -> i64 {
        i64::from(self.lines.clamp(1, MAX_LOG_LINES))
    }

    /// 规范化后的字节上限
    #[must_use]
    pub fn byte_limit(&self) -> usize {
        self.max_bytes.unwrap_or(DEFAULT_LOG_BYTES).max(1)
    }
}

/// Pod 日志结果（kubelet 已合并 stdout/stderr，无法区分来源）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PodLogs {
    /// 命名空间
    pub namespace: String,
    /// Pod 名
    pub pod: String,
    /// 容器名（请求显式指定时回显）
    pub container: Option<String>,
    /// 是否读取的上一次运行日志
    pub previous: bool,
    /// 按返回顺序的日志行（不含行尾换行）
    pub lines: Vec<String>,
    /// 实际累计字节数
    pub bytes: usize,
    /// 是否因超过字节上限提前结束
    pub truncated: bool,
}

/// 单个容器的指标
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerMetrics {
    /// 容器名
    pub name: String,
    /// CPU 使用量（nanocores）
    pub cpu_nanocores: u64,
    /// 内存使用量（bytes，工作集）
    pub memory_bytes: u64,
}

/// Pod 指标快照（metrics-server）
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PodMetrics {
    /// 命名空间
    pub namespace: String,
    /// Pod 名
    pub pod: String,
    /// 采样窗口（如 `15s`）
    pub window: Option<String>,
    /// 采样时间（RFC3339 原文）
    pub timestamp: Option<String>,
    /// 容器级明细
    pub containers: Vec<ContainerMetrics>,
    /// 所有容器 CPU 之和（nanocores）
    pub cpu_nanocores: u64,
    /// 所有容器内存之和（bytes）
    pub memory_bytes: u64,
}

impl PodMetrics {
    /// 把 `PodMetrics` 原始 JSON（`DynamicObject.data`）换算成视图
    ///
    /// 单个容器缺少 `usage` 时按 0 计（刚创建的容器可能还没有样本），
    /// 但不会整体失败。
    #[must_use]
    pub fn from_json(namespace: &str, pod: &str, data: &serde_json::Value) -> Self {
        let window = data
            .get("window")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let timestamp = data
            .get("timestamp")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let mut containers = Vec::new();
        if let Some(items) = data.get("containers").and_then(|value| value.as_array()) {
            for item in items {
                let name = item
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string();
                let cpu_nanocores = item
                    .pointer("/usage/cpu")
                    .and_then(|value| value.as_str())
                    .and_then(parse_cpu_nanocores)
                    .unwrap_or_default();
                let memory_bytes = item
                    .pointer("/usage/memory")
                    .and_then(|value| value.as_str())
                    .and_then(parse_memory_bytes)
                    .unwrap_or_default();
                containers.push(ContainerMetrics {
                    name,
                    cpu_nanocores,
                    memory_bytes,
                });
            }
        }
        let cpu_nanocores = containers
            .iter()
            .map(|container| container.cpu_nanocores)
            .sum();
        let memory_bytes = containers.iter().map(|container| container.memory_bytes).sum();
        Self {
            namespace: namespace.to_string(),
            pod: pod.to_string(),
            window,
            timestamp,
            containers,
            cpu_nanocores,
            memory_bytes,
        }
    }
}

/// 组装 `pods/log` 的查询参数
#[must_use]
pub fn pod_log_params(request: &PodLogsRequest) -> LogParams {
    LogParams {
        container: request.container.clone(),
        previous: request.previous,
        tail_lines: Some(request.line_limit()),
        timestamps: request.timestamps,
        limit_bytes: request
            .max_bytes
            .and_then(|bytes| i64::try_from(bytes).ok()),
        ..LogParams::default()
    }
}

/// CPU 使用量 → nanocores
///
/// 支持 `n`（nano）、`u`（micro）、`m`（milli）与无后缀（core）；
/// 其它后缀返回 `None`，避免把 `1Gi` 之类的内存单位误当 CPU。
#[must_use]
pub fn parse_cpu_nanocores(raw: &str) -> Option<u64> {
    let (value, suffix) = split_quantity(raw)?;
    let factor = match suffix {
        "" => 1_000_000_000.0,
        "n" => 1.0,
        "u" => 1_000.0,
        "m" => 1_000_000.0,
        _ => return None,
    };
    to_u64(value * factor)
}

/// 内存使用量 → bytes
///
/// 支持十进制 `k` / `M` / `G` / `T` 与二进制 `Ki` / `Mi` / `Gi` / `Ti`，无后缀按字节。
#[must_use]
pub fn parse_memory_bytes(raw: &str) -> Option<u64> {
    let (value, suffix) = split_quantity(raw)?;
    let factor = match suffix {
        "" => 1.0,
        "k" | "K" => 1_000.0,
        "M" => 1_000_000.0,
        "G" => 1_000_000_000.0,
        "T" => 1_000_000_000_000.0,
        "Ki" => 1024.0,
        "Mi" => 1024.0 * 1024.0,
        "Gi" => 1024.0 * 1024.0 * 1024.0,
        "Ti" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    to_u64(value * factor)
}

/// 拆分 quantity 的「数字 + 后缀」；负数 / 空串 / 非数字开头一律 `None`
fn split_quantity(raw: &str) -> Option<(f64, &str)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let split = trimmed
        .find(|ch: char| !ch.is_ascii_digit() && ch != '.')
        .unwrap_or(trimmed.len());
    let (number, suffix) = trimmed.split_at(split);
    if number.is_empty() {
        return None;
    }
    let value = number.parse::<f64>().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    Some((value, suffix))
}

/// 浮点 → `u64`（越界返回 `None`，不静默饱和）
// 已先校验有限 / 非负 / 上界，此处的 f64 → u64 转换是安全的
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn to_u64(value: f64) -> Option<u64> {
    if !value.is_finite() || value < 0.0 || value > u64::MAX as f64 {
        return None;
    }
    Some(value.round() as u64)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn cpu_quantities_are_parsed() {
        assert_eq!(parse_cpu_nanocores("12345678n"), Some(12_345_678));
        assert_eq!(parse_cpu_nanocores("250m"), Some(250_000_000));
        assert_eq!(parse_cpu_nanocores("1"), Some(1_000_000_000));
        assert_eq!(parse_cpu_nanocores("1.5"), Some(1_500_000_000));
        assert_eq!(parse_cpu_nanocores("abc"), None);
        assert_eq!(parse_cpu_nanocores("-1"), None);
        assert_eq!(parse_cpu_nanocores("1Gi"), None);
    }

    #[test]
    fn memory_quantities_are_parsed() {
        assert_eq!(parse_memory_bytes("12345Ki"), Some(12_641_280));
        assert_eq!(parse_memory_bytes("1Mi"), Some(1_048_576));
        assert_eq!(parse_memory_bytes("3Gi"), Some(3_221_225_472));
        assert_eq!(parse_memory_bytes("1024"), Some(1024));
        assert_eq!(parse_memory_bytes("1.5Gi"), Some(1_610_612_736));
        assert_eq!(parse_memory_bytes("1X"), None);
        assert_eq!(parse_memory_bytes(""), None);
    }

    #[test]
    fn pod_metrics_from_json_sums_containers() {
        let data = json!({
            "window": "15s",
            "timestamp": "2026-11-19T10:00:00Z",
            "containers": [
                { "name": "app", "usage": { "cpu": "250m", "memory": "128Mi" } },
                { "name": "sidecar", "usage": { "cpu": "1000000n", "memory": "1024Ki" } }
            ]
        });
        let metrics = PodMetrics::from_json("prod", "web-0", &data);
        assert_eq!(metrics.window.as_deref(), Some("15s"));
        assert_eq!(metrics.containers.len(), 2);
        assert_eq!(metrics.cpu_nanocores, 250_000_000 + 1_000_000);
        assert_eq!(metrics.memory_bytes, 128 * 1024 * 1024 + 1024 * 1024);
    }

    #[test]
    fn pod_metrics_tolerates_missing_usage() {
        let data = json!({ "containers": [{ "name": "app" }] });
        let metrics = PodMetrics::from_json("prod", "web-0", &data);
        assert_eq!(metrics.containers.len(), 1);
        assert_eq!(metrics.cpu_nanocores, 0);
        assert_eq!(metrics.memory_bytes, 0);
        assert!(metrics.window.is_none());
    }

    #[test]
    fn pod_log_params_maps_request() {
        let request = PodLogsRequest {
            container: Some("app".to_string()),
            previous: true,
            lines: 5000,
            timestamps: true,
            max_bytes: Some(4096),
            ..PodLogsRequest::new("prod", "web-0")
        };
        let params = pod_log_params(&request);
        assert_eq!(params.container.as_deref(), Some("app"));
        assert!(params.previous);
        assert!(params.timestamps);
        assert_eq!(params.tail_lines, Some(2000));
        assert_eq!(params.limit_bytes, Some(4096));
    }

    #[test]
    fn pod_log_request_clamps_lines_and_bytes() {
        let mut request = PodLogsRequest::new("ns", "pod");
        request.lines = 0;
        assert_eq!(request.line_limit(), 1);
        let mut request = PodLogsRequest::new("ns", "pod");
        request.max_bytes = Some(0);
        assert_eq!(request.byte_limit(), 1);
        assert_eq!(
            PodLogsRequest::new("ns", "pod").byte_limit(),
            DEFAULT_LOG_BYTES
        );
        assert_eq!(PodLogsRequest::new("ns", "pod").lines, DEFAULT_LOG_LINES);
    }
}
