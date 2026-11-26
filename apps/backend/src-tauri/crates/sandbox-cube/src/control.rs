//! 控制面实现（CubeAPI `/cubeapi/v1`）
//!
//! 路径全部收敛为本文件的常量：CubeSandbox 迭代较快，Phase 0 校准只改这里。
//! 幂等约定见设计文档 §5.3：`pause` 已暂停 / `kill` 已销毁都按成功处理。

use async_trait::async_trait;
use sandbox::{
    ExecRequest, ExecResult, FileEntry, HealthInfo, MAX_LIST_LIMIT, MAX_TIMEOUT_SECS,
    NetworkPolicy, SandboxError, SandboxFilter, SandboxHandle, SandboxRuntime, SandboxSpec,
    SandboxState, SandboxSummary, SnapshotInfo,
};
use serde_json::Value;

use crate::client::CubeSandboxRuntime;
use crate::convert::{normalize_time, parse_logs, to_handle, to_summary};
use crate::wire::{
    HealthDto, NewSandbox, RollbackBody, SandboxDto, SandboxNetworkConfig, SnapshotBody, SnapshotDto,
    TimeoutBody,
};

/// 控制面路径常量（唯一改 URL 的地方）
const PATH_HEALTH: &str = "/health";
const PATH_SANDBOXES: &str = "/sandboxes";
const PATH_SANDBOX_ONE: &str = "/sandboxes/{id}";
const PATH_SANDBOX_CONNECT: &str = "/sandboxes/{id}/connect";
const PATH_SANDBOX_PAUSE: &str = "/sandboxes/{id}/pause";
const PATH_SANDBOX_RESUME: &str = "/sandboxes/{id}/resume";
const PATH_SANDBOX_TIMEOUT: &str = "/sandboxes/{id}/timeout";
const PATH_SANDBOX_LOGS: &str = "/sandboxes/{id}/logs";
const PATH_SANDBOX_SNAPSHOTS: &str = "/sandboxes/{id}/snapshots";
const PATH_SANDBOX_ROLLBACK: &str = "/sandboxes/{id}/rollback";

/// 填充路径模板里的 `{id}`
fn path_with_id(template: &str, id: &str) -> String {
    template.replace("{id}", id)
}

/// 校验并按需回落超时（0 → 配置缺省；超上限 → 400）
fn clamp_timeout(requested: u32, fallback: u32) -> Result<u32, SandboxError> {
    let value = if requested == 0 {
        fallback.max(1)
    } else {
        requested
    };
    if value > MAX_TIMEOUT_SECS {
        return Err(SandboxError::invalid_request(
            400,
            format!("timeout_secs={value} 超过上限 {MAX_TIMEOUT_SECS}"),
        ));
    }
    Ok(value)
}

/// 契约状态 → 服务端过滤参数
fn state_name(state: SandboxState) -> &'static str {
    match state {
        SandboxState::Running => "running",
        SandboxState::Paused => "paused",
        SandboxState::Pausing => "pausing",
        SandboxState::Unknown => "unknown",
    }
}

impl CubeSandboxRuntime {
    /// 读取单个沙箱（`connect` 409 / 无响应体时的兜底）
    async fn get(&self, id: &str) -> Result<SandboxHandle, SandboxError> {
        let url = self.control_url(&path_with_id(PATH_SANDBOX_ONE, id));
        let response = self.send(self.with_auth(self.http().get(url))).await?;
        let dto: SandboxDto = self.handle_json(response).await?;
        Ok(to_handle(&dto, self.options()))
    }
}

#[async_trait]
impl SandboxRuntime for CubeSandboxRuntime {
    async fn health(&self) -> Result<HealthInfo, SandboxError> {
        let url = self.control_url(PATH_HEALTH);
        let result: Result<HealthDto, SandboxError> = async {
            let response = self.send(self.with_auth(self.http().get(url))).await?;
            self.handle_json(response).await
        }
        .await;

        match result {
            Ok(dto) => {
                let ok = dto.ok.unwrap_or_else(|| {
                    dto.status.is_empty()
                        || dto.status.eq_ignore_ascii_case("ok")
                        || dto.status.eq_ignore_ascii_case("healthy")
                        || dto.status.eq_ignore_ascii_case("up")
                });
                Ok(HealthInfo {
                    ok,
                    version: dto.version,
                    message: dto.message,
                })
            }
            // 配置错误必须冒泡（否则「配错了」会被误读成「服务不可用」）
            Err(SandboxError::Config(message)) => Err(SandboxError::config(message)),
            // 其余错误软失败：健康检查本身不应把调用方打断
            Err(other) => Ok(HealthInfo {
                ok: false,
                version: None,
                message: Some(format!("{}（{}）", other.hint(), other)),
            }),
        }
    }

    async fn create(&self, spec: SandboxSpec) -> Result<SandboxHandle, SandboxError> {
        let template = if spec.template_id.trim().is_empty() {
            self.options().default_template_id.trim().to_string()
        } else {
            spec.template_id.trim().to_string()
        };
        if template.is_empty() {
            return Err(SandboxError::config(
                "模板为空：请设置 SANDBOX_TEMPLATE_ID 或在请求中指定 template",
            ));
        }

        let timeout = clamp_timeout(spec.timeout_secs, self.timeout_secs() as u32)?;
        let network = match &spec.network {
            NetworkPolicy::DenyAll => None,
            NetworkPolicy::AllowDomains(domains) => Some(SandboxNetworkConfig {
                allow_domains: domains.as_slice(),
            }),
        };
        let body = NewSandbox {
            template: &template,
            timeout,
            metadata: &spec.metadata,
            envs: &spec.env,
            network,
        };

        let url = self.control_url(PATH_SANDBOXES);
        let response = self
            .send(self.with_auth(self.http().post(url).json(&body)))
            .await?;
        let dto: SandboxDto = self.handle_json(response).await?;
        if dto.id.trim().is_empty() {
            return Err(SandboxError::decode("创建响应缺少 sandbox id"));
        }

        tracing::info!(sandbox = %dto.id, template = %template, timeout, "CubeSandbox 沙箱已创建");
        Ok(to_handle(&dto, self.options()))
    }

    async fn connect(&self, id: &str) -> Result<SandboxHandle, SandboxError> {
        let body = TimeoutBody {
            timeout: self.timeout_secs() as u32,
        };
        let url = self.control_url(&path_with_id(PATH_SANDBOX_CONNECT, id));
        let response = self
            .send(self.with_auth(self.http().post(url).json(&body)))
            .await?;

        match self.handle_optional_json::<SandboxDto>(response).await {
            Ok(Some(dto)) if !dto.id.trim().is_empty() => Ok(to_handle(&dto, self.options())),
            // 无响应体：回读一次状态
            Ok(_) => self.get(id).await,
            // 409（已在运行）：回读状态即可
            Err(SandboxError::Conflict(_)) => self.get(id).await,
            Err(other) => Err(other),
        }
    }

    async fn pause(&self, id: &str) -> Result<(), SandboxError> {
        let url = self.control_url(&path_with_id(PATH_SANDBOX_PAUSE, id));
        let response = self.send(self.with_auth(self.http().post(url))).await?;
        match self.handle_unit(response).await {
            Ok(()) => Ok(()),
            // 幂等：已经不存在 / 已暂停都视为成功
            Err(SandboxError::NotFound(_) | SandboxError::Conflict(_)) => Ok(()),
            Err(other) => Err(other),
        }
    }

    async fn resume(&self, id: &str) -> Result<SandboxHandle, SandboxError> {
        let body = TimeoutBody {
            timeout: self.timeout_secs() as u32,
        };
        let url = self.control_url(&path_with_id(PATH_SANDBOX_RESUME, id));
        let response = self
            .send(self.with_auth(self.http().post(url).json(&body)))
            .await?;

        match self.handle_optional_json::<SandboxDto>(response).await {
            Ok(Some(dto)) if !dto.id.trim().is_empty() => Ok(to_handle(&dto, self.options())),
            Ok(_) => self.get(id).await,
            Err(other) => Err(other),
        }
    }

    async fn kill(&self, id: &str) -> Result<(), SandboxError> {
        let url = self.control_url(&path_with_id(PATH_SANDBOX_ONE, id));
        let response = self.send(self.with_auth(self.http().delete(url))).await?;
        match self.handle_unit(response).await {
            Ok(()) => Ok(()),
            // 幂等：已销毁的沙箱删除返回 404 也算成功
            Err(SandboxError::NotFound(_)) => Ok(()),
            Err(other) => Err(other),
        }
    }

    async fn list(&self, filter: SandboxFilter) -> Result<Vec<SandboxSummary>, SandboxError> {
        let limit = filter.limit.unwrap_or(100).min(MAX_LIST_LIMIT);
        let mut request = self
            .http()
            .get(self.control_url(PATH_SANDBOXES))
            .query(&[("limit", limit.to_string())]);

        if !filter.metadata.is_empty() {
            let metadata = filter
                .metadata
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join("&");
            request = request.query(&[("metadata", metadata)]);
        }
        if !filter.states.is_empty() {
            let states = filter
                .states
                .iter()
                .map(|state| state_name(*state))
                .collect::<Vec<_>>()
                .join(",");
            request = request.query(&[("state", states)]);
        }

        let response = self.send(self.with_auth(request)).await?;
        let value: Value = self.handle_json(response).await?;
        let items = value
            .as_array()
            .cloned()
            .or_else(|| value.get("sandboxes").and_then(Value::as_array).cloned())
            .unwrap_or_default();

        Ok(items
            .iter()
            .filter_map(|item| serde_json::from_value::<SandboxDto>(item.clone()).ok())
            .map(|dto| to_summary(&dto))
            .collect())
    }

    async fn set_timeout(&self, id: &str, timeout_secs: u32) -> Result<(), SandboxError> {
        if timeout_secs == 0 || timeout_secs > MAX_TIMEOUT_SECS {
            return Err(SandboxError::invalid_request(
                400,
                format!("timeout_secs={timeout_secs} 越界（1..={MAX_TIMEOUT_SECS}）"),
            ));
        }

        let url = self.control_url(&path_with_id(PATH_SANDBOX_TIMEOUT, id));
        let body = TimeoutBody {
            timeout: timeout_secs,
        };
        let response = self
            .send(self.with_auth(self.http().post(url).json(&body)))
            .await?;
        self.handle_unit(response).await
    }

    async fn logs(&self, id: &str) -> Result<Vec<String>, SandboxError> {
        let url = self.control_url(&path_with_id(PATH_SANDBOX_LOGS, id));
        let response = self.send(self.with_auth(self.http().get(url))).await?;
        let value: Value = self.handle_json(response).await?;
        Ok(parse_logs(&value))
    }

    async fn create_snapshot(&self, id: &str, name: &str) -> Result<SnapshotInfo, SandboxError> {
        if name.trim().is_empty() {
            return Err(SandboxError::invalid_request(400, "快照名不能为空"));
        }

        let url = self.control_url(&path_with_id(PATH_SANDBOX_SNAPSHOTS, id));
        let response = self
            .send(self.with_auth(
                self.http()
                    .post(url)
                    .json(&SnapshotBody { name: name.trim() }),
            ))
            .await?;

        let dto = self.handle_optional_json::<SnapshotDto>(response).await?;
        let (resolved_name, created_at) = match dto {
            Some(dto) => (dto.name.filter(|n| !n.trim().is_empty()), dto.created_at),
            None => (None, None),
        };

        Ok(SnapshotInfo {
            name: resolved_name.unwrap_or_else(|| name.trim().to_string()),
            sandbox_id: id.to_string(),
            created_at: normalize_time(created_at),
        })
    }

    async fn rollback(&self, id: &str, name: &str) -> Result<(), SandboxError> {
        if name.trim().is_empty() {
            return Err(SandboxError::invalid_request(400, "快照名不能为空"));
        }

        let url = self.control_url(&path_with_id(PATH_SANDBOX_ROLLBACK, id));
        let response = self
            .send(self.with_auth(self.http().post(url).json(&RollbackBody {
                snapshot_name: name.trim(),
            })))
            .await?;
        self.handle_unit(response).await
    }

    // ── 数据面：实现在 dataplane.rs ─────────────────────────────────────

    async fn exec(&self, id: &str, request: ExecRequest) -> Result<ExecResult, SandboxError> {
        self.data_exec(id, request).await
    }

    async fn read_file(&self, id: &str, path: &str, user: &str) -> Result<Vec<u8>, SandboxError> {
        self.data_read_file(id, path, user).await
    }

    async fn write_file(
        &self,
        id: &str,
        path: &str,
        user: &str,
        data: Vec<u8>,
    ) -> Result<(), SandboxError> {
        self.data_write_file(id, path, user, data).await
    }

    async fn list_files(
        &self,
        id: &str,
        dir: &str,
        user: &str,
    ) -> Result<Vec<FileEntry>, SandboxError> {
        self.data_list_files(id, dir, user).await
    }
}
