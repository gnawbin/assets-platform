//! CubeSandbox 沙箱实现（TencentCloud/CubeSandbox）
//!
//! 两个平面：
//! - **控制面**：CubeAPI `/cubeapi/v1`（`control.rs`）——建箱 / 暂停 / 恢复 / 销毁 /
//!   续期 / 列表 / 日志 / 快照；认证走 `X-API-Key`（或 `Authorization: Bearer`）。
//! - **数据面**：CubeProxy → envd（`dataplane.rs`）——执行代码 / 命令、读写文件；
//!   通过虚拟 `Host: {envd_port}-{sandbox_id}.{suffix}` 路由到具体 MicroVM。
//!
//! 本 crate 独持 `reqwest`；契约（trait / 类型 / 错误）在 `crates/sandbox`，
//! 上层（`service` / `api` / `engine`）只依赖契约。
//!
//! # 用法
//! ```rust,ignore
//! use sandbox::SandboxRuntime;
//! use sandbox_cube::{CubeOptions, CubeSandboxRuntime};
//!
//! let runtime = CubeSandboxRuntime::new(CubeOptions::from_env()?)?;
//! let handle = runtime.create(SandboxSpec::default()).await?;
//! let result = runtime.exec(&handle.id, ExecRequest::code("print(1 + 1)")).await?;
//! println!("{}", result.combined_output());
//! ```
//!
//! ```bash
//! cargo test -p sandbox-cube
//! ```
//!
//! # 校准说明
//! 控制面路径来自 CubeSandbox 仓库的 `CubeAPI/src/handlers/*.rs` 与 `src/openapi.rs`；
//! 若部署实例的 `openapi.json` 不同，只需改 `control.rs` 顶部的路径常量。

pub mod client;
mod control;
mod convert;
mod dataplane;
pub mod error;
mod wire;

pub use client::{CubeOptions, CubeSandboxRuntime, DEFAULT_CUBEAPI_PORT};
pub use error::{extract_message, map_error, parse_retry_after};
pub use sandbox;

use sandbox::SandboxResult;

impl CubeSandboxRuntime {
    /// 从环境变量构造（`SANDBOX_*`，由 `.env.toml` 的 `[sandbox]` 段映射而来）
    ///
    /// # 错误
    /// `SANDBOX_BASE_URL` 为空 / 非法时返回 [`sandbox::SandboxError::Config`]。
    pub fn from_env() -> SandboxResult<Self> {
        Self::new(CubeOptions::from_env()?)
    }
}
