//! 沙箱运行时契约与「未启用」实现

use async_trait::async_trait;

use crate::error::{SandboxError, SandboxResult};
use crate::model::{
    ExecRequest, ExecResult, FileEntry, HealthInfo, SandboxFilter, SandboxHandle, SandboxSpec,
    SandboxSummary, SnapshotInfo,
};

/// 沙箱运行时契约：所有实现（CubeSandbox / E2B / 本地 Docker）都收敛到它
///
/// 约定：
/// 1. 所有方法都必须**可取消**（上层用 `tokio::time::timeout` 包裹，
///    实现层不负责兜住取消）；
/// 2. `read_file` / `write_file` 单次载荷上限见 [`crate::MAX_TRANSFER_BYTES`]，
///    超限返回 [`SandboxError::InvalidRequest`]，大文件走对象存储中转；
/// 3. 可选能力（快照 / 回滚 / 日志）默认返回 [`SandboxError::Unsupported`]，
///    实现层按需覆盖，避免契约膨胀。
#[async_trait]
pub trait SandboxRuntime: Send + Sync {
    /// 控制面健康检查
    async fn health(&self) -> SandboxResult<HealthInfo>;

    /// 创建沙箱
    async fn create(&self, spec: SandboxSpec) -> SandboxResult<SandboxHandle>;

    /// 连接既有沙箱（断线重连 / 跨端接管）
    async fn connect(&self, id: &str) -> SandboxResult<SandboxHandle>;

    /// 暂停（保留内存快照，可 `resume` 恢复现场）
    async fn pause(&self, id: &str) -> SandboxResult<()>;

    /// 恢复
    async fn resume(&self, id: &str) -> SandboxResult<SandboxHandle>;

    /// 销毁（不可恢复）
    async fn kill(&self, id: &str) -> SandboxResult<()>;

    /// 按 metadata / 状态列出沙箱
    async fn list(&self, filter: SandboxFilter) -> SandboxResult<Vec<SandboxSummary>>;

    /// 续期（避免长会话被服务端回收）
    async fn set_timeout(&self, id: &str, timeout_secs: u32) -> SandboxResult<()>;

    /// 执行代码 / 命令
    async fn exec(&self, id: &str, request: ExecRequest) -> SandboxResult<ExecResult>;

    /// 读文件
    async fn read_file(&self, id: &str, path: &str, user: &str) -> SandboxResult<Vec<u8>>;

    /// 写文件
    async fn write_file(
        &self,
        id: &str,
        path: &str,
        user: &str,
        data: Vec<u8>,
    ) -> SandboxResult<()>;

    /// 列目录
    async fn list_files(&self, id: &str, dir: &str, user: &str) -> SandboxResult<Vec<FileEntry>>;

    // ── 可选能力：默认 Unsupported，实现层按需覆盖 ──────────────────────

    /// 创建快照
    async fn create_snapshot(&self, _id: &str, _name: &str) -> SandboxResult<SnapshotInfo> {
        Err(SandboxError::unsupported("create_snapshot"))
    }

    /// 回滚到快照
    async fn rollback(&self, _id: &str, _name: &str) -> SandboxResult<()> {
        Err(SandboxError::unsupported("rollback"))
    }

    /// 拉取沙箱日志
    async fn logs(&self, _id: &str) -> SandboxResult<Vec<String>> {
        Err(SandboxError::unsupported("logs"))
    }
}

/// 装箱后的沙箱运行时（便于在 `service` 中按配置选择实现）
pub type BoxedSandboxRuntime = Box<dyn SandboxRuntime>;

/// 未启用沙箱时使用的空实现
///
/// `SANDBOX_ENABLED=false`（缺省）时装它：所有能力返回
/// [`SandboxError::Unsupported`]，`health()` 返回 `ok=false`，
/// **不 panic、不阻塞启动**，保证「不配置也能用」。
#[derive(Debug, Default, Clone, Copy)]
pub struct DisabledRuntime;

#[async_trait]
impl SandboxRuntime for DisabledRuntime {
    async fn health(&self) -> SandboxResult<HealthInfo> {
        Ok(HealthInfo {
            ok: false,
            version: None,
            message: Some("沙箱未启用（SANDBOX_ENABLED=false）".to_string()),
        })
    }

    async fn create(&self, _spec: SandboxSpec) -> SandboxResult<SandboxHandle> {
        Err(SandboxError::unsupported(disabled_reason()))
    }

    async fn connect(&self, _id: &str) -> SandboxResult<SandboxHandle> {
        Err(SandboxError::unsupported(disabled_reason()))
    }

    async fn pause(&self, _id: &str) -> SandboxResult<()> {
        Err(SandboxError::unsupported(disabled_reason()))
    }

    async fn resume(&self, _id: &str) -> SandboxResult<SandboxHandle> {
        Err(SandboxError::unsupported(disabled_reason()))
    }

    async fn kill(&self, _id: &str) -> SandboxResult<()> {
        Err(SandboxError::unsupported(disabled_reason()))
    }

    async fn list(&self, _filter: SandboxFilter) -> SandboxResult<Vec<SandboxSummary>> {
        Err(SandboxError::unsupported(disabled_reason()))
    }

    async fn set_timeout(&self, _id: &str, _timeout_secs: u32) -> SandboxResult<()> {
        Err(SandboxError::unsupported(disabled_reason()))
    }

    async fn exec(&self, _id: &str, _request: ExecRequest) -> SandboxResult<ExecResult> {
        Err(SandboxError::unsupported(disabled_reason()))
    }

    async fn read_file(&self, _id: &str, _path: &str, _user: &str) -> SandboxResult<Vec<u8>> {
        Err(SandboxError::unsupported(disabled_reason()))
    }

    async fn write_file(
        &self,
        _id: &str,
        _path: &str,
        _user: &str,
        _data: Vec<u8>,
    ) -> SandboxResult<()> {
        Err(SandboxError::unsupported(disabled_reason()))
    }

    async fn list_files(
        &self,
        _id: &str,
        _dir: &str,
        _user: &str,
    ) -> SandboxResult<Vec<FileEntry>> {
        Err(SandboxError::unsupported(disabled_reason()))
    }
}

/// 未启用时的统一原因文案（`hint()` 已给出配置指引，这里只说明现状）
fn disabled_reason() -> &'static str {
    "sandbox disabled: SANDBOX_ENABLED=false"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabled_runtime_reports_not_ok_and_rejects_everything() {
        let runtime = DisabledRuntime;

        let health = runtime.health().await.expect("health 不应报错");
        assert!(!health.ok);
        assert!(health.message.is_some());

        let error = runtime
            .create(SandboxSpec::default())
            .await
            .expect_err("create 必须返回 Unsupported");
        assert_eq!(error.kind(), "unsupported");
        assert!(!error.is_retryable());

        assert!(runtime.exec("sb-1", ExecRequest::code("1+1")).await.is_err());
        assert!(runtime.pause("sb-1").await.is_err());
        assert!(runtime.kill("sb-1").await.is_err());
        assert!(runtime.list(SandboxFilter::default()).await.is_err());
        assert!(runtime.read_file("sb-1", "/tmp/a", "root").await.is_err());
        assert!(
            runtime
                .write_file("sb-1", "/tmp/a", "root", vec![1])
                .await
                .is_err()
        );
        assert!(runtime.list_files("sb-1", "/tmp", "root").await.is_err());
    }

    #[tokio::test]
    async fn optional_capabilities_default_to_unsupported() {
        let runtime = DisabledRuntime;
        assert!(runtime.create_snapshot("sb-1", "snap").await.is_err());
        assert!(runtime.rollback("sb-1", "snap").await.is_err());
        assert!(runtime.logs("sb-1").await.is_err());
    }
}
