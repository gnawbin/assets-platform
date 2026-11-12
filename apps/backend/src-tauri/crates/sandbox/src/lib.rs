//! AI Agent 沙箱运行时契约（与实现无关）
//!
//! 本 crate 只定义**契约**，不含任何 HTTP / IO 驱动依赖：
//! - [`model`]：协议无关的数据类型（`SandboxSpec` / `SandboxHandle` /
//!   `ExecRequest` / `ExecResult` / `FileEntry` ...）；
//! - [`error`]：`SandboxError`，按「可重试 / 需人工修配置 / 能力不支持」分类，
//!   供上层重试与前端提示（`kind()` / `hint()`）使用；
//! - [`runtime`]：`SandboxRuntime` trait 与未启用时的 [`runtime::DisabledRuntime`]。
//!
//! # 实现分布
//! - `sandbox-cube`：TencentCloud/CubeSandbox（控制面 CubeAPI `/cubeapi/v1` +
//!   数据面 CubeProxy / envd）。Cube 未启用时由 [`runtime::DisabledRuntime`] 兜底。
//! - 未来的 `sandbox-e2b` / `sandbox-docker`：只依赖本 crate，互不牵连。
//!
//! 依赖方向（硬约束）：
//! `api → service → sandbox`（契约）、`service → sandbox-cube`（实现）、
//! `engine → sandbox`（只依赖契约，不依赖实现）。
//!
//! ```bash
//! cargo test -p sandbox
//! ```
//!
//! # 设计约束
//! 1. 契约层不解析时间：时间字段以 RFC3339 字符串透传，格式归一化由实现层负责；
//! 2. 所有错误都可通过 `match` 区分，契约层不使用 `String` 传参；
//! 3. 可选能力（快照 / 回滚 / 日志）在 trait 里给默认实现，返回 `Unsupported`。

pub mod error;
pub mod model;
pub mod runtime;

pub use error::{SandboxError, SandboxResult};
pub use model::{
    ExecFailure, ExecRequest, ExecResult, FileEntry, HealthInfo, NetworkPolicy, SandboxFilter,
    SandboxHandle, SandboxSpec, SandboxState, SandboxSummary, SnapshotInfo, VolumeMount,
};
pub use runtime::{BoxedSandboxRuntime, DisabledRuntime, SandboxRuntime};

/// 单次执行的缺省超时（秒）
pub const DEFAULT_TIMEOUT_SECS: u32 = 60;

/// 单次执行 / 沙箱存活时长上限（秒），超过返回 [`SandboxError::InvalidRequest`]
pub const MAX_TIMEOUT_SECS: u32 = 3600;

/// 单次 `code` 载荷上限（256 KiB）
pub const MAX_CODE_BYTES: usize = 256 * 1024;

/// 单次 `read_file` / `write_file` 载荷上限（32 MiB），更大的文件走对象存储中转
pub const MAX_TRANSFER_BYTES: usize = 32 * 1024 * 1024;

/// `SandboxFilter::limit` 上限，超出截断
pub const MAX_LIST_LIMIT: u32 = 500;

/// 数据面 envd 端口缺省值
pub const DEFAULT_ENVD_PORT: u16 = 49983;
