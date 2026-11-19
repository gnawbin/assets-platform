//! 容器日志 / 性能的统一视图与纯换算函数（Docker 侧）
//!
//! 本模块**不发请求**：只把 bollard 返回的原始结构换算成前端可直接展示的数值，
//! 网络调用在 [`crate::connector::docker::DockerConnector::container_logs`] /
//! [`crate::connector::docker::DockerConnector::container_stats`] 里。
//!
//! 换算口径与 `docker stats` 对齐，便于运维对照排查：
//! - CPU%：`(cpu_delta / system_delta) * online_cpus * 100`（多核可 > 100）；
//! - 内存：`usage - cache`，cache 依次取 `total_inactive_file`（cgroup v1）→
//!   `inactive_file`（cgroup v2）→ `cache`；
//! - 网络：所有网卡的 `rx_bytes` / `tx_bytes` 求和；
//! - 块设备：`io_service_bytes_recursive` 按 `op` 求和（cgroup v2 下可能整体缺失）。

use std::collections::HashMap;

use bollard::models::ContainerStatsResponse;
use serde::{Deserialize, Serialize};

use crate::connector::docker::date_millis;

/// 默认行数（对齐总方案 §4「200~500 行」）
pub const DEFAULT_LOG_LINES: u32 = 200;
/// 行数硬上限
pub const MAX_LOG_LINES: u32 = 2000;
/// 默认字节上限 512 KiB
pub const DEFAULT_LOG_BYTES: usize = 512 * 1024;

/// 日志拉取参数
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContainerLogsRequest {
    /// 末尾行数：`None` 用 200，最终夹到 `1..=2000`
    pub lines: Option<u32>,
    /// 是否让 daemon 给每行加 RFC3339 时间戳
    pub timestamps: bool,
    /// 响应字节上限：`None` 用 512 KiB
    pub max_bytes: Option<usize>,
}

/// 单行日志的来源
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    /// 标准输出
    Stdout,
    /// 标准错误
    Stderr,
    /// 标准输入（几乎不会出现）
    Stdin,
    /// TTY 容器：stdout / stderr 已合并，无帧头
    Console,
}

impl LogStream {
    /// 稳定字符串编码（落库 / 前端判定都用它）
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::Stdin => "stdin",
            Self::Console => "console",
        }
    }
}

/// 单行日志
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerLogLine {
    /// 来源
    pub stream: LogStream,
    /// 行内容（已做 UTF-8 有损解码，保留原始 ANSI 转义）
    pub message: String,
}

/// 日志拉取结果
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerLogs {
    /// 容器 ID / 名称（原样回显，便于前端关联）
    pub container: String,
    /// 按 daemon 返回顺序排列的行
    pub lines: Vec<ContainerLogLine>,
    /// 实际累计字节数
    pub bytes: usize,
    /// 是否因超过字节上限提前结束
    pub truncated: bool,
}

impl ContainerLogsRequest {
    /// 规范化后的 `tail` 查询值（`"200"` / `"2000"`）
    #[must_use]
    pub fn tail(&self) -> String {
        self.lines
            .unwrap_or(DEFAULT_LOG_LINES)
            .clamp(1, MAX_LOG_LINES)
            .to_string()
    }

    /// 规范化后的字节上限
    #[must_use]
    pub fn byte_limit(&self) -> usize {
        self.max_bytes.unwrap_or(DEFAULT_LOG_BYTES).max(1)
    }
}

/// 容器性能快照（数值型，前端不再二次换算）
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContainerStats {
    /// 容器 ID / 名称
    pub container: String,
    /// 采样时间（epoch 毫秒，取自 `read`）
    pub read_at_ms: i64,
    /// CPU 使用率（%），无基线时为 `None`
    pub cpu_percent: Option<f64>,
    /// 工作集内存（已减 page cache）
    pub mem_used_bytes: u64,
    /// 内存上限（未限制时为宿主机内存量级）
    pub mem_limit_bytes: u64,
    /// 内存使用率（%）
    pub mem_percent: f64,
    /// 所有网卡接收字节数之和
    pub net_rx_bytes: u64,
    /// 所有网卡发送字节数之和
    pub net_tx_bytes: u64,
    /// cgroup 内进程数
    pub pids: Option<u64>,
    /// 块设备累计读字节
    pub blkio_read_bytes: Option<u64>,
    /// 块设备累计写字节
    pub blkio_write_bytes: Option<u64>,
}

impl ContainerStats {
    /// 由一帧（或两帧）原始样本换算成视图
    ///
    /// `previous` 为空时退回样本自带的 `precpu_stats`；`one-shot` 下两者通常都没有有效基线，
    /// 此时 `cpu_percent` 为 `None`，属于预期行为。
    #[must_use]
    pub fn from_samples(
        container: &str,
        current: &ContainerStatsResponse,
        previous: Option<&ContainerStatsResponse>,
    ) -> Self {
        let (mem_used_bytes, mem_limit_bytes, mem_percent) =
            memory_usage(current).unwrap_or((0, 0, 0.0));
        let (net_received, net_sent) = network_totals(current);
        let (blkio_read_bytes, blkio_write_bytes) = blkio_totals(current);
        Self {
            container: container.to_string(),
            read_at_ms: current.read.as_ref().map(date_millis).unwrap_or_default(),
            cpu_percent: cpu_percent(current, previous),
            mem_used_bytes,
            mem_limit_bytes,
            mem_percent,
            net_rx_bytes: net_received,
            net_tx_bytes: net_sent,
            pids: current.pids_stats.as_ref().and_then(|stats| stats.current),
            blkio_read_bytes,
            blkio_write_bytes,
        }
    }
}

/// CPU 使用率（%）；`previous` 为空时用样本自带的 `precpu_stats`
#[must_use]
// 累计纳秒数转 f64 只为算百分比，2^53 纳秒量级内精度足够
#[allow(clippy::cast_precision_loss)]
pub fn cpu_percent(
    current: &ContainerStatsResponse,
    previous: Option<&ContainerStatsResponse>,
) -> Option<f64> {
    let cpu = current.cpu_stats.as_ref()?;
    // 显式传入上一帧时用它的 `cpu_stats`；否则退回本帧自带的 `precpu_stats`
    //（Docker 的 `precpu_stats` 本身就是 cpu_stats 结构，不是完整 stats 结构）
    let prev = match previous {
        Some(previous) => previous.cpu_stats.as_ref()?,
        None => current.precpu_stats.as_ref()?,
    };
    let total = cpu.cpu_usage.as_ref()?.total_usage?;
    let prev_total = prev.cpu_usage.as_ref()?.total_usage?;
    let system = cpu.system_cpu_usage?;
    let prev_system = prev.system_cpu_usage?;

    // 容器重启后 total_usage 会回退，必须判掉，否则算出负数
    let cpu_delta = total.checked_sub(prev_total)? as f64;
    let system_delta = system.checked_sub(prev_system)? as f64;
    if cpu_delta <= 0.0 || system_delta <= 0.0 {
        return None;
    }
    let cpus = f64::from(cpu.online_cpus.unwrap_or(1).max(1));
    Some(cpu_delta / system_delta * cpus * 100.0)
}

/// 工作集内存与使用率：`(used, limit, percent)`
#[must_use]
// 字节数转 f64 只为算百分比
#[allow(clippy::cast_precision_loss)]
pub fn memory_usage(current: &ContainerStatsResponse) -> Option<(u64, u64, f64)> {
    let memory = current.memory_stats.as_ref()?;
    let usage = memory.usage?;
    let cache = memory
        .stats
        .as_ref()
        .map(mem_cache_bytes)
        .unwrap_or_default();
    let used = usage.saturating_sub(cache);
    let limit = memory.limit.unwrap_or_default();
    let percent = if limit == 0 {
        0.0
    } else {
        used as f64 / limit as f64 * 100.0
    };
    Some((used, limit, percent))
}

/// 页缓存字节数：cgroup v1 用 `total_inactive_file`，v2 用 `inactive_file`
fn mem_cache_bytes(stats: &HashMap<String, u64>) -> u64 {
    ["total_inactive_file", "inactive_file", "cache"]
        .iter()
        .find_map(|key| stats.get(*key).copied())
        .unwrap_or_default()
}

/// 网络累计收发：多网卡求和
#[must_use]
pub fn network_totals(current: &ContainerStatsResponse) -> (u64, u64) {
    current.networks.as_ref().map_or((0, 0), |networks| {
        networks.values().fold((0, 0), |(rx, tx), stats| {
            (
                rx + stats.rx_bytes.unwrap_or_default(),
                tx + stats.tx_bytes.unwrap_or_default(),
            )
        })
    })
}

/// 块设备累计读写；`io_service_bytes_recursive` 缺失时返回 `(None, None)`
#[must_use]
pub fn blkio_totals(current: &ContainerStatsResponse) -> (Option<u64>, Option<u64>) {
    let Some(entries) = current
        .blkio_stats
        .as_ref()
        .and_then(|stats| stats.io_service_bytes_recursive.as_ref())
    else {
        return (None, None);
    };
    if entries.is_empty() {
        return (None, None);
    }
    let mut read = 0_u64;
    let mut write = 0_u64;
    for entry in entries {
        let value = entry.value.unwrap_or_default();
        match entry.op.as_deref() {
            Some("Read") => read += value,
            Some("Write") => write += value,
            _ => {}
        }
    }
    (Some(read), Some(write))
}

#[cfg(test)]
mod tests {
    use bollard::models::{
        ContainerBlkioStatEntry, ContainerBlkioStats, ContainerCpuStats, ContainerCpuUsage,
        ContainerMemoryStats, ContainerNetworkStats, ContainerPidsStats,
    };

    use super::*;

    fn cpu_stats(total: u64, system: u64, cpus: u32) -> ContainerCpuStats {
        ContainerCpuStats {
            cpu_usage: Some(ContainerCpuUsage {
                total_usage: Some(total),
                ..ContainerCpuUsage::default()
            }),
            system_cpu_usage: Some(system),
            online_cpus: Some(cpus),
            ..ContainerCpuStats::default()
        }
    }

    fn sample() -> ContainerStatsResponse {
        let mut cache = HashMap::new();
        cache.insert("total_inactive_file".to_string(), 52_428_800_u64);
        let mut networks = HashMap::new();
        networks.insert(
            "eth0".to_string(),
            ContainerNetworkStats {
                rx_bytes: Some(100),
                tx_bytes: Some(200),
                ..ContainerNetworkStats::default()
            },
        );
        networks.insert(
            "eth1".to_string(),
            ContainerNetworkStats {
                rx_bytes: Some(1),
                tx_bytes: Some(2),
                ..ContainerNetworkStats::default()
            },
        );
        ContainerStatsResponse {
            cpu_stats: Some(cpu_stats(3_000_000_000, 10_000_000_000, 2)),
            precpu_stats: Some(cpu_stats(1_000_000_000, 6_000_000_000, 2)),
            memory_stats: Some(ContainerMemoryStats {
                usage: Some(104_857_600),
                limit: Some(536_870_912),
                stats: Some(cache),
                ..ContainerMemoryStats::default()
            }),
            networks: Some(networks),
            pids_stats: Some(ContainerPidsStats {
                current: Some(5),
                ..ContainerPidsStats::default()
            }),
            blkio_stats: Some(ContainerBlkioStats {
                io_service_bytes_recursive: Some(vec![
                    ContainerBlkioStatEntry {
                        op: Some("Read".to_string()),
                        value: Some(1024),
                        ..ContainerBlkioStatEntry::default()
                    },
                    ContainerBlkioStatEntry {
                        op: Some("Write".to_string()),
                        value: Some(2048),
                        ..ContainerBlkioStatEntry::default()
                    },
                ]),
                ..ContainerBlkioStats::default()
            }),
            ..ContainerStatsResponse::default()
        }
    }

    #[test]
    fn cpu_percent_between_two_samples() {
        let current = sample();
        let previous = ContainerStatsResponse {
            cpu_stats: Some(cpu_stats(1_000_000_000, 6_000_000_000, 2)),
            ..ContainerStatsResponse::default()
        };
        // cpu_delta=2e9, system_delta=4e9, 2 核 → 100%
        let percent = cpu_percent(&current, Some(&previous)).expect("应能算出 CPU%");
        assert!((percent - 100.0).abs() < 1e-6, "实际 {percent}");
    }

    #[test]
    fn cpu_percent_falls_back_to_precpu_stats() {
        let percent = cpu_percent(&sample(), None).expect("precpu_stats 可用");
        assert!((percent - 100.0).abs() < 1e-6, "实际 {percent}");
    }

    #[test]
    fn cpu_percent_negative_delta_is_none() {
        let current = sample();
        let previous = ContainerStatsResponse {
            cpu_stats: Some(cpu_stats(9_000_000_000, 20_000_000_000, 2)),
            ..ContainerStatsResponse::default()
        };
        assert_eq!(cpu_percent(&current, Some(&previous)), None);
    }

    #[test]
    fn memory_prefers_total_inactive_file() {
        let (used, limit, percent) = memory_usage(&sample()).expect("内存字段完整");
        assert_eq!(used, 52_428_800);
        assert_eq!(limit, 536_870_912);
        assert!((percent - 9.765_625).abs() < 1e-6, "实际 {percent}");
    }

    #[test]
    fn memory_falls_back_to_inactive_file_for_cgroup_v2() {
        let mut cache = HashMap::new();
        cache.insert("inactive_file".to_string(), 10_u64);
        let sample = ContainerStatsResponse {
            memory_stats: Some(ContainerMemoryStats {
                usage: Some(100),
                limit: Some(1000),
                stats: Some(cache),
                ..ContainerMemoryStats::default()
            }),
            ..ContainerStatsResponse::default()
        };
        let (used, _, percent) = memory_usage(&sample).expect("cgroup v2 字段");
        assert_eq!(used, 90);
        assert!((percent - 9.0).abs() < 1e-6, "实际 {percent}");
    }

    #[test]
    fn network_sums_all_interfaces() {
        assert_eq!(network_totals(&sample()), (101, 202));
    }

    #[test]
    fn blkio_sums_by_op() {
        assert_eq!(blkio_totals(&sample()), (Some(1024), Some(2048)));
    }

    #[test]
    fn blkio_missing_is_none() {
        let sample = ContainerStatsResponse::default();
        assert_eq!(blkio_totals(&sample), (None, None));
    }

    #[test]
    fn log_request_normalizes_tail_and_bytes() {
        let request = ContainerLogsRequest {
            lines: Some(0),
            ..ContainerLogsRequest::default()
        };
        assert_eq!(request.tail(), "1");
        let request = ContainerLogsRequest {
            lines: Some(u32::MAX),
            ..ContainerLogsRequest::default()
        };
        assert_eq!(request.tail(), "2000");
        let request = ContainerLogsRequest {
            max_bytes: Some(0),
            ..ContainerLogsRequest::default()
        };
        assert_eq!(request.byte_limit(), 1);
        assert_eq!(
            ContainerLogsRequest::default().byte_limit(),
            DEFAULT_LOG_BYTES
        );
    }

    #[test]
    fn stats_from_samples_reports_all_fields() {
        let stats = ContainerStats::from_samples("web-1", &sample(), None);
        assert_eq!(stats.container, "web-1");
        assert_eq!(stats.pids, Some(5));
        assert_eq!(stats.net_rx_bytes, 101);
        assert_eq!(stats.net_tx_bytes, 202);
        assert_eq!(stats.mem_used_bytes, 52_428_800);
        assert_eq!(stats.blkio_read_bytes, Some(1024));
        assert!(stats.cpu_percent.is_some());
    }
}
