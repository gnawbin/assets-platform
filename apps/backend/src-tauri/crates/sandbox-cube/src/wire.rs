//! 私有 wire DTO（与 CubeAPI / envd 的线上格式一一对应）
//!
//! 只在本 crate 内使用：向上只暴露 `sandbox` 契约类型。
//! 所有结构都加 `#[serde(default)]`，容忍 CubeSandbox 版本升级新增字段。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `skip_serializing_if` 谓词：字段类型是 `&BTreeMap`，serde 会传 `&&BTreeMap`
fn is_empty_map(map: &&BTreeMap<String, String>) -> bool {
    map.is_empty()
}

/// `skip_serializing_if` 谓词：字段类型是 `&[String]`
fn is_empty_slice(items: &&[String]) -> bool {
    items.is_empty()
}

/// 控制面：创建沙箱请求体（`NewSandbox`）
#[derive(Debug, Serialize)]
pub(crate) struct NewSandbox<'a> {
    /// 模板 ID 或别名
    pub template: &'a str,
    /// 存活时长（秒）
    pub timeout: u32,
    #[serde(skip_serializing_if = "is_empty_map")]
    pub metadata: &'a BTreeMap<String, String>,
    #[serde(skip_serializing_if = "is_empty_map")]
    pub envs: &'a BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<SandboxNetworkConfig<'a>>,
}

/// 控制面：出网策略（`SandboxNetworkConfig`）
#[derive(Debug, Serialize)]
pub(crate) struct SandboxNetworkConfig<'a> {
    /// 允许访问的域名（由 CubeEgress 执行 L7 过滤）
    pub allow_domains: &'a [String],
}

/// 控制面：续期 / 连接请求体（`SetTimeoutRequest`、`ConnectSandbox` 共用）
#[derive(Debug, Serialize)]
pub(crate) struct TimeoutBody {
    pub timeout: u32,
}

/// 控制面：快照请求体（`CreateSnapshotRequest`）
#[derive(Debug, Serialize)]
pub(crate) struct SnapshotBody<'a> {
    pub name: &'a str,
}

/// 控制面：回滚请求体（`RollbackRequest`）
#[derive(Debug, Serialize)]
pub(crate) struct RollbackBody<'a> {
    pub snapshot_name: &'a str,
}

/// 控制面：沙箱对象（`Sandbox` / `ListedSandbox` / `ResumedSandbox` 的超集）
///
/// 字段名做了多重别名：Cube 原生用 snake_case，E2B `/v2/*` 用它自己的写法。
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct SandboxDto {
    #[serde(default, alias = "sandboxID", alias = "sandboxId", alias = "sandbox_id")]
    pub id: String,
    #[serde(default, alias = "status")]
    pub state: String,
    #[serde(
        default,
        alias = "templateID",
        alias = "templateId",
        alias = "template_id",
        alias = "template"
    )]
    pub template: Option<String>,
    #[serde(
        default,
        alias = "startedAt",
        alias = "started_at",
        alias = "createdAt",
        alias = "created_at"
    )]
    pub started_at: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

/// 控制面：健康检查（`HealthResponse`）
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct HealthDto {
    #[serde(default)]
    pub ok: Option<bool>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

/// 控制面：快照对象（`SnapshotInfo`）
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct SnapshotDto {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, alias = "createdAt", alias = "created_at")]
    pub created_at: Option<String>,
}

/// 数据面：执行请求体（envd `process.Process/Start`）
#[derive(Debug, Serialize)]
pub(crate) struct StartProcess<'a> {
    pub cmd: &'a str,
    #[serde(skip_serializing_if = "is_empty_slice")]
    pub args: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<&'a str>,
    #[serde(skip_serializing_if = "is_empty_map")]
    pub envs: &'a BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<&'a str>,
}
