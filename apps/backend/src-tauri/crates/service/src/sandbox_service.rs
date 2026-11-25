//! AI Agent 沙箱服务（CubeSandbox）
//!
//! 职责边界（设计文档 §6.1）：
//! 1. 配置装载：`SANDBOX_*` 环境变量（由 `.env.toml` 的 `[sandbox]` 段映射）；
//! 2. 运行时单例：`mode = "in_process"`（缺省，不启用沙盒）时装 [`DisabledRuntime`]，
//!    保证「不配置也能启动」，所有能力返回 `Unsupported`；
//! 3. 会话亲和：`SessionKey(tenant, user, scope) → 沙箱句柄`，避免每次执行都冷启动；
//! 4. TTL 回收：空闲超过 `SANDBOX_SESSION_TTL_SECS` 先 `pause`，超过
//!    `SANDBOX_KILL_AFTER_SECS` 再 `kill`。
//!
//! **不做的事**：不解析 HTTP、不碰权限判定（留在 `crates/api` 的 `authz`）。

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use sandbox::{
    DisabledRuntime, ExecRequest, ExecResult, FileEntry, HealthInfo, SandboxError, SandboxHandle,
    SandboxRuntime, SandboxSpec, SandboxState, SandboxSummary, DEFAULT_TIMEOUT_SECS,
};
use sandbox_cube::CubeSandboxRuntime;
use sandbox_openshell::OpenShellRuntime;
use tokio::sync::Mutex;

/// 会话键：租户 + 用户 + 业务会话（对话 ID / 工作流运行 ID / Skill ID）
///
/// `tenant` 用 schema 名（`UserContext::schema_name` / `database::current_schema_name()`），
/// 因为 HTTP 层与桌面层都只能拿到 schema，而不是数值租户 ID。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionKey {
    /// 租户 schema 名
    pub tenant: String,
    /// 用户 ID
    pub user_id: i64,
    /// 业务会话标识（例：`conv:123` / `wf:456` / `skill:excel-helper`）
    pub scope: String,
}

impl SessionKey {
    /// 构造会话键
    #[must_use]
    pub fn new(tenant: impl Into<String>, user_id: i64, scope: impl Into<String>) -> Self {
        Self {
            tenant: tenant.into(),
            user_id,
            scope: scope.into(),
        }
    }
}

/// 沙箱运行模式
///
/// - [`SandboxMode::InProcess`]（缺省）：**同进程，不启用沙盒**，不连接任何外部服务；
/// - [`SandboxMode::Cube`]：CubeSandbox（需 `SANDBOX_BASE_URL`）；
/// - [`SandboxMode::OpenShell`]：OpenShell gateway（`crates/sandbox-openshell`；缺 gateway 时降级为 Disabled）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SandboxMode {
    /// 同进程（缺省）
    #[default]
    InProcess,
    /// CubeSandbox
    Cube,
    /// OpenShell（`crates/sandbox-openshell` 客户端）
    OpenShell,
}

impl SandboxMode {
    /// 稳定字符串（日志 / 健康信息 / 业务元数据）
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InProcess => "in_process",
            Self::Cube => "cube",
            Self::OpenShell => "openshell",
        }
    }

    /// 解析配置值（大小写、`-` 与 `_` 容错；`none` / `off` / `disabled` 视同 `in_process`）
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let normalized = raw.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        match normalized.as_str() {
            "in_process" | "inprocess" | "none" | "off" | "disabled" => Some(Self::InProcess),
            "cube" | "cube_sandbox" | "cubesandbox" => Some(Self::Cube),
            "openshell" | "open_shell" => Some(Self::OpenShell),
            _ => None,
        }
    }
}

/// 沙箱配置（**不含密钥**，密钥留在 [`sandbox_cube::CubeOptions`]，其 `Debug` 已脱敏）
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// 运行模式（缺省 `InProcess`：同进程，不启用沙盒）
    pub mode: SandboxMode,
    /// CubeAPI 根地址（仅用于日志与诊断展示）
    pub base_url: String,
    /// OpenShell gateway 地址（仅用于日志与 `should_enable` 判定；连接由 `sandbox-openshell` 懒建）
    pub openshell_gateway_url: String,
    /// 缺省模板
    pub template_id: String,
    /// 沙箱存活 / 续期时长（秒）
    pub timeout_secs: u32,
    /// 空闲多久后 `pause`（秒）
    pub session_ttl_secs: u64,
    /// `pause` 多久后 `kill`（秒）
    pub kill_after_secs: u64,
    /// 回收任务扫描间隔（秒，最小 30）
    pub reaper_interval_secs: u64,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            mode: SandboxMode::InProcess,
            base_url: String::new(),
            openshell_gateway_url: String::new(),
            template_id: String::new(),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            session_ttl_secs: 900,
            kill_after_secs: 3600,
            reaper_interval_secs: 60,
        }
    }
}

impl SandboxConfig {
    /// 从环境变量读取（全部有缺省值，读不到不报错）
    #[must_use]
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            mode: resolve_mode(
                env_trim("SANDBOX_MODE").as_deref(),
                env_trim("SANDBOX_ENABLED").as_deref(),
                env_trim("SANDBOX_BACKEND").as_deref(),
            ),
            base_url: env_trim("SANDBOX_BASE_URL").unwrap_or_default(),
            openshell_gateway_url: env_trim("SANDBOX_OPENSHELL_GATEWAY_URL").unwrap_or_default(),
            template_id: env_trim("SANDBOX_TEMPLATE_ID").unwrap_or_default(),
            timeout_secs: parse_u32("SANDBOX_TIMEOUT_SECS", defaults.timeout_secs).max(1),
            session_ttl_secs: parse_u64("SANDBOX_SESSION_TTL_SECS", defaults.session_ttl_secs)
                .max(30),
            kill_after_secs: parse_u64("SANDBOX_KILL_AFTER_SECS", defaults.kill_after_secs)
                .max(defaults.session_ttl_secs),
            reaper_interval_secs: parse_u64(
                "SANDBOX_REAPER_INTERVAL_SECS",
                defaults.reaper_interval_secs,
            )
            .max(30),
        }
    }

    /// 当前模式是否具备装配条件
    ///
    /// - `InProcess`：永远 false（同进程，不装配沙盒）；
    /// - `Cube`：需 `SANDBOX_BASE_URL` 非空；
    /// - `OpenShell`：需 `SANDBOX_OPENSHELL_GATEWAY_URL` 非空（gateway 独立部署）。
    #[must_use]
    pub fn should_enable(&self) -> bool {
        match self.mode {
            SandboxMode::InProcess => false,
            SandboxMode::Cube => !self.base_url.trim().is_empty(),
            SandboxMode::OpenShell => !self.openshell_gateway_url.trim().is_empty(),
        }
    }
}

/// 会话条目
#[derive(Debug, Clone)]
struct SessionEntry {
    handle: SandboxHandle,
    last_used_at: Instant,
}

static CONFIG: OnceLock<SandboxConfig> = OnceLock::new();
static RUNTIME: OnceLock<Arc<dyn SandboxRuntime>> = OnceLock::new();
static SESSIONS: OnceLock<DashMap<SessionKey, SessionEntry>> = OnceLock::new();
static LOCKS: OnceLock<DashMap<SessionKey, Arc<Mutex<()>>>> = OnceLock::new();
/// 会话级注入环境变量（建箱时写进 `SandboxSpec.env`）
static SESSION_ENV: OnceLock<DashMap<SessionKey, BTreeMap<String, String>>> = OnceLock::new();

/// 全局配置（进程内只读）
pub fn config() -> &'static SandboxConfig {
    CONFIG.get_or_init(SandboxConfig::from_env)
}

/// 当前运行模式（缺省 [`SandboxMode::InProcess`]）
#[must_use]
pub fn mode() -> SandboxMode {
    config().mode
}

/// 全局运行时（缺省 [`DisabledRuntime`]）
pub fn runtime() -> Arc<dyn SandboxRuntime> {
    RUNTIME
        .get_or_init(|| {
            let config = config();
            match config.mode {
                SandboxMode::InProcess => {
                    tracing::info!(
                        mode = config.mode.as_str(),
                        "沙箱未启用（同进程模式）：装配 DisabledRuntime"
                    );
                    Arc::new(DisabledRuntime)
                }
                SandboxMode::OpenShell => {
                    if !config.should_enable() {
                        tracing::info!(
                            mode = config.mode.as_str(),
                            "OpenShell 配置不完整（SANDBOX_OPENSHELL_GATEWAY_URL 为空）：装配 DisabledRuntime"
                        );
                        return Arc::new(DisabledRuntime);
                    }

                    match OpenShellRuntime::from_env() {
                        Ok(runtime) => {
                            tracing::info!(
                                gateway = %config.openshell_gateway_url,
                                workspace = %std::env::var("SANDBOX_OPENSHELL_WORKSPACE")
                                    .unwrap_or_else(|_| "default".to_string()),
                                "沙箱运行时已就绪（OpenShell）"
                            );
                            Arc::new(runtime)
                        }
                        Err(error) => {
                            tracing::warn!(
                                "OpenShell 运行时初始化失败，降级为 DisabledRuntime：{}",
                                to_message(&error)
                            );
                            Arc::new(DisabledRuntime)
                        }
                    }
                }
                SandboxMode::Cube => {
                    if !config.should_enable() {
                        tracing::info!(
                            mode = config.mode.as_str(),
                            base_url = %config.base_url,
                            "Cube 配置不完整（SANDBOX_BASE_URL 为空）：装配 DisabledRuntime"
                        );
                        return Arc::new(DisabledRuntime);
                    }

                    match CubeSandboxRuntime::from_env() {
                        Ok(runtime) => {
                            tracing::info!(
                                base_url = %config.base_url,
                                template = %config.template_id,
                                "沙箱运行时已就绪（CubeSandbox）"
                            );
                            Arc::new(runtime)
                        }
                        Err(error) => {
                            tracing::warn!(
                                "沙箱运行时初始化失败，降级为 DisabledRuntime：{}",
                                to_message(&error)
                            );
                            Arc::new(DisabledRuntime)
                        }
                    }
                }
            }
        })
        .clone()
}

fn sessions() -> &'static DashMap<SessionKey, SessionEntry> {
    SESSIONS.get_or_init(DashMap::new)
}

/// 每个会话键一把锁，避免并发请求重复建沙箱
fn lock_for(key: &SessionKey) -> Arc<Mutex<()>> {
    LOCKS
        .get_or_init(DashMap::new)
        .entry(key.clone())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// 契约错误 → 面向用户的一句话（`hint` + 原始信息）
#[must_use]
pub fn to_message(error: &SandboxError) -> String {
    format!("{}（{}）", error.hint(), error)
}

fn remember(key: &SessionKey, handle: SandboxHandle) {
    sessions().insert(
        key.clone(),
        SessionEntry {
            handle,
            last_used_at: Instant::now(),
        },
    );
}

fn touch(key: &SessionKey) {
    if let Some(mut entry) = sessions().get_mut(key) {
        entry.last_used_at = Instant::now();
    }
}

fn session_env_map() -> &'static DashMap<SessionKey, BTreeMap<String, String>> {
    SESSION_ENV.get_or_init(DashMap::new)
}

/// 登记会话要注入沙箱的环境变量（幂等合并；**仅在下一次建箱时生效**）
///
/// 用途：`agent_in_sandbox` 需要把短时 `AGENT_TOKEN` 注入沙箱（设计文档 §10.1 / §14），
/// 沙箱内 `agent-runner` 再从 `step.llm.token_env` 指到同名变量读取。
///
/// 已存在的沙箱不会被重建：换 token 后需等 TTL 回收，或显式 `release_session` 后重建。
pub fn prepare_session_env(key: SessionKey, env: BTreeMap<String, String>) {
    if env.is_empty() {
        return;
    }
    let mut entry = session_env_map().entry(key).or_default();
    for (name, value) in env {
        entry.insert(name, value);
    }
}

/// 读取会话已登记的环境变量（诊断 / 单测用）
#[must_use]
pub fn session_env(key: &SessionKey) -> BTreeMap<String, String> {
    session_env_map()
        .get(key)
        .map(|entry| entry.clone())
        .unwrap_or_default()
}

/// 读取环境变量并 trim（空串视为未设置）
fn env_trim(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string())
}

fn parse_u64(key: &str, default: u64) -> u64 {
    env_trim(key)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

fn parse_u32(key: &str, default: u32) -> u32 {
    env_trim(key)
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(default)
}

/// `true` / `1` / `yes` / `on` 视为开
fn parse_bool_value(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes" | "on"
    )
}

/// 解析运行模式：`SANDBOX_MODE` 优先，未设置时兼容旧键 `SANDBOX_ENABLED` + `SANDBOX_BACKEND`
///
/// 规则（安全优先：任何异常都回退到 `InProcess`，即不启用沙盒）：
/// 1. `SANDBOX_MODE` 合法 → 直接采用；非法 → `InProcess` + WARN；
/// 2. 未设置 `SANDBOX_MODE` 且 `SANDBOX_ENABLED` 为假（缺省）→ `InProcess`；
/// 3. 旧键开启时按 `SANDBOX_BACKEND`（缺省 `cube`）解析；未知值 → `InProcess` + WARN。
fn resolve_mode(
    mode: Option<&str>,
    legacy_enabled: Option<&str>,
    legacy_backend: Option<&str>,
) -> SandboxMode {
    if let Some(raw) = mode.map(str::trim).filter(|value| !value.is_empty()) {
        return match SandboxMode::parse(raw) {
            Some(parsed) => parsed,
            None => {
                tracing::warn!(
                    value = %raw,
                    "SANDBOX_MODE 非法，回退为 in_process（不启用沙盒）"
                );
                SandboxMode::InProcess
            }
        };
    }

    if !legacy_enabled.map(parse_bool_value).unwrap_or(false) {
        return SandboxMode::InProcess;
    }

    let backend = legacy_backend
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("cube");

    match SandboxMode::parse(backend) {
        Some(parsed @ (SandboxMode::Cube | SandboxMode::OpenShell)) => {
            tracing::info!(
                mode = parsed.as_str(),
                "已按旧键 SANDBOX_ENABLED + SANDBOX_BACKEND 推导运行模式"
            );
            parsed
        }
        _ => {
            tracing::warn!(
                value = %backend,
                "SANDBOX_BACKEND 未知，回退为 in_process（不启用沙盒）"
            );
            SandboxMode::InProcess
        }
    }
}

/// 取得（或创建）会话对应的沙箱句柄
///
/// 逻辑（设计文档 §7.2）：
/// 1. `Running` → 续期后直接复用；
/// 2. `Paused` / `Pausing` → `resume`；404 表示已被回收，删除条目后重建；
/// 3. `Unknown` → 先 `kill`（忽略错误）再重建；
/// 4. 未命中 → `create`，`metadata` 写入 `tenant` / `user_id` / `session_id`。
///
/// # 错误
/// 返回「`hint()`（原始错误）」形式的一句话，可直接展示给用户。
pub async fn ensure_session(key: SessionKey) -> Result<SandboxHandle, String> {
    let runtime = runtime();
    let config = config();
    let lock = lock_for(&key);
    let _guard = lock.lock().await;

    if let Some(entry) = sessions().get(&key).map(|entry| entry.handle.clone()) {
        match entry.state {
            SandboxState::Running => {
                if let Err(error) = runtime.set_timeout(&entry.id, config.timeout_secs).await {
                    tracing::debug!(sandbox = %entry.id, %error, "沙箱续期失败（本次仍复用）");
                }
                touch(&key);
                return Ok(entry);
            }
            SandboxState::Paused | SandboxState::Pausing => match runtime.resume(&entry.id).await {
                Ok(handle) => {
                    tracing::info!(sandbox = %handle.id, "沙箱已恢复");
                    remember(&key, handle.clone());
                    return Ok(handle);
                }
                Err(SandboxError::NotFound(_)) => {
                    tracing::info!(sandbox = %entry.id, "沙箱已被服务端回收，重建会话");
                    sessions().remove(&key);
                }
                Err(other) => return Err(to_message(&other)),
            },
            SandboxState::Unknown => {
                let _ = runtime.kill(&entry.id).await;
                sessions().remove(&key);
            }
        }
    }

    let mut metadata = std::collections::BTreeMap::new();
    metadata.insert("tenant".to_string(), key.tenant.clone());
    metadata.insert("user_id".to_string(), key.user_id.to_string());
    metadata.insert("session_id".to_string(), key.scope.clone());

    let spec = SandboxSpec {
        template_id: config.template_id.clone(),
        timeout_secs: config.timeout_secs,
        metadata,
        // 会话级注入（如 `AGENT_TOKEN`）；未登记时为空，行为与之前一致
        env: session_env(&key),
        ..SandboxSpec::default()
    };
    let handle = runtime
        .create(spec)
        .await
        .map_err(|error| to_message(&error))?;
    remember(&key, handle.clone());
    Ok(handle)
}

/// 执行代码（会话级复用）
///
/// # 错误
/// 代码超限 / 沙箱不可用 / 执行失败，均返回可展示的一句话。
pub async fn exec_code(
    key: SessionKey,
    code: String,
    timeout_secs: Option<u32>,
) -> Result<ExecResult, String> {
    if code.len() > sandbox::MAX_CODE_BYTES {
        return Err(to_message(&SandboxError::invalid_request(
            400,
            format!("代码超过上限 {} 字节", sandbox::MAX_CODE_BYTES),
        )));
    }

    let handle = ensure_session(key).await?;
    let request = ExecRequest {
        timeout_secs,
        ..ExecRequest::code(code)
    };
    exec_with_guard(&handle.id, request).await
}

/// 执行命令（会话级复用）
pub async fn run_command(
    key: SessionKey,
    command: String,
    argv: Vec<String>,
) -> Result<ExecResult, String> {
    run_command_with_options(key, command, argv, None, None).await
}

/// 执行命令（可指定工作目录 / 单次超时；供 Agent 沙箱工具使用）
pub async fn run_command_with_options(
    key: SessionKey,
    command: String,
    argv: Vec<String>,
    cwd: Option<String>,
    timeout_secs: Option<u32>,
) -> Result<ExecResult, String> {
    let handle = ensure_session(key).await?;
    let request = ExecRequest {
        argv,
        cwd,
        timeout_secs,
        ..ExecRequest::command(command)
    };
    exec_with_guard(&handle.id, request).await
}

/// 带兜底超时的执行（实现层超时之外再包一层，避免连接挂死）
async fn exec_with_guard(id: &str, request: ExecRequest) -> Result<ExecResult, String> {
    let timeout_secs = request
        .timeout_secs
        .unwrap_or_else(|| config().timeout_secs)
        .clamp(1, sandbox::MAX_TIMEOUT_SECS);
    let guard = Duration::from_secs(u64::from(timeout_secs) + 5);
    let runtime = runtime();

    match tokio::time::timeout(guard, runtime.exec(id, request)).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(error)) => Err(to_message(&error)),
        Err(_) => Err(to_message(&SandboxError::timeout(u64::from(timeout_secs)))),
    }
}

/// 上传文件到沙箱
pub async fn upload_file(key: SessionKey, path: String, data: Vec<u8>) -> Result<(), String> {
    let handle = ensure_session(key).await?;
    runtime()
        .write_file(&handle.id, &path, "root", data)
        .await
        .map_err(|error| to_message(&error))
}

/// 从沙箱下载文件
pub async fn download_file(key: SessionKey, path: String) -> Result<Vec<u8>, String> {
    let handle = ensure_session(key).await?;
    runtime()
        .read_file(&handle.id, &path, "root")
        .await
        .map_err(|error| to_message(&error))
}

/// 列出沙箱目录
pub async fn list_files(key: SessionKey, dir: String) -> Result<Vec<FileEntry>, String> {
    let handle = ensure_session(key).await?;
    runtime()
        .list_files(&handle.id, &dir, "root")
        .await
        .map_err(|error| to_message(&error))
}

/// 暂停会话对应的沙箱（保留现场）
pub async fn pause_session(key: SessionKey) -> Result<(), String> {
    let Some(entry) = sessions().get(&key).map(|entry| entry.handle.clone()) else {
        return Ok(());
    };
    runtime()
        .pause(&entry.id)
        .await
        .map_err(|error| to_message(&error))?;
    if let Some(mut current) = sessions().get_mut(&key) {
        current.handle.state = SandboxState::Paused;
        current.last_used_at = Instant::now();
    }
    Ok(())
}

/// 恢复会话对应的沙箱（不存在时按需创建）
pub async fn resume_session(key: SessionKey) -> Result<SandboxHandle, String> {
    let Some(entry) = sessions().get(&key).map(|entry| entry.handle.clone()) else {
        return ensure_session(key).await;
    };

    match runtime().resume(&entry.id).await {
        Ok(handle) => {
            remember(&key, handle.clone());
            Ok(handle)
        }
        Err(SandboxError::NotFound(_)) => {
            sessions().remove(&key);
            ensure_session(key).await
        }
        Err(other) => Err(to_message(&other)),
    }
}

/// 显式释放（`DELETE` 会话时调用）
pub async fn release_session(key: SessionKey) -> Result<(), String> {
    let Some(entry) = sessions().remove(&key).map(|(_, entry)| entry.handle) else {
        return Ok(());
    };
    match runtime().kill(&entry.id).await {
        Ok(()) | Err(SandboxError::NotFound(_)) => Ok(()),
        Err(other) => Err(to_message(&other)),
    }
}

/// 控制面健康检查（消息里显式带出运行模式）
pub async fn health() -> Result<HealthInfo, String> {
    let info = runtime()
        .health()
        .await
        .map_err(|error| to_message(&error))?;
    let base = info.message.unwrap_or_default();
    let message = if base.is_empty() {
        format!("mode={}", config().mode.as_str())
    } else {
        format!("{base}（mode={}）", config().mode.as_str())
    };
    Ok(HealthInfo {
        ok: info.ok,
        version: info.version,
        message: Some(message),
    })
}

/// 按租户列出沙箱（`metadata.tenant` 过滤）
pub async fn list_by_tenant(tenant: &str) -> Result<Vec<SandboxSummary>, String> {
    let mut metadata = std::collections::BTreeMap::new();
    metadata.insert("tenant".to_string(), tenant.to_string());
    runtime()
        .list(sandbox::SandboxFilter {
            metadata,
            states: Vec::new(),
            limit: None,
        })
        .await
        .map_err(|error| to_message(&error))
}

/// 启动空闲回收任务（`api::start_http_server_with` 里 spawn 一次即可）
///
/// 未启用沙箱时直接返回，不占用运行时资源。
pub fn spawn_idle_reaper() {
    let config = config().clone();
    if !config.should_enable() {
        tracing::debug!("沙箱未启用：跳过空闲回收任务");
        return;
    }

    let interval_secs = config.reaper_interval_secs;
    tokio::spawn(async move {
        let interval = Duration::from_secs(interval_secs);
        loop {
            tokio::time::sleep(interval).await;
            reap_idle(&config).await;
        }
    });
    tracing::info!(interval_secs, "沙箱空闲回收任务已启动");
}

/// 扫描一次空闲会话：超过 TTL 先 pause，超过 kill 阈值再销毁
async fn reap_idle(config: &SandboxConfig) {
    let runtime = runtime();
    let candidates = sessions()
        .iter()
        .map(|entry| {
            (
                entry.key().clone(),
                entry.handle.clone(),
                entry.last_used_at.elapsed().as_secs(),
            )
        })
        .collect::<Vec<_>>();

    for (key, handle, idle_secs) in candidates {
        if idle_secs < config.session_ttl_secs {
            continue;
        }

        if idle_secs >= config.kill_after_secs {
            match runtime.kill(&handle.id).await {
                Ok(()) | Err(SandboxError::NotFound(_)) => {
                    sessions().remove(&key);
                    tracing::info!(sandbox = %handle.id, idle_secs, "空闲沙箱已销毁");
                }
                Err(error) => tracing::warn!(sandbox = %handle.id, %error, "空闲沙箱销毁失败"),
            }
            continue;
        }

        match runtime.pause(&handle.id).await {
            Ok(()) => {
                if let Some(mut entry) = sessions().get_mut(&key) {
                    entry.handle.state = SandboxState::Paused;
                }
                tracing::info!(sandbox = %handle.id, idle_secs, "空闲沙箱已暂停（保留现场）");
            }
            Err(error) => tracing::warn!(sandbox = %handle.id, %error, "空闲沙箱暂停失败"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_key_is_hashable_and_distinguishes_scope() {
        let mut set = std::collections::HashSet::new();
        set.insert(SessionKey::new("tenant_a", 1, "conv:1"));
        set.insert(SessionKey::new("tenant_a", 1, "conv:2"));
        set.insert(SessionKey::new("tenant_b", 1, "conv:1"));
        set.insert(SessionKey::new("tenant_a", 2, "conv:1"));
        assert_eq!(set.len(), 4);
    }

    #[test]
    fn should_enable_requires_mode_and_base_url() {
        let cube_with_url = SandboxConfig {
            mode: SandboxMode::Cube,
            base_url: "http://cubeapi:3000".to_string(),
            ..SandboxConfig::default()
        };
        assert!(cube_with_url.should_enable());

        let cube_without_url = SandboxConfig {
            mode: SandboxMode::Cube,
            base_url: "  ".to_string(),
            ..SandboxConfig::default()
        };
        assert!(!cube_without_url.should_enable());

        let in_process_with_url = SandboxConfig {
            base_url: "http://cubeapi:3000".to_string(),
            ..SandboxConfig::default()
        };
        assert!(!in_process_with_url.should_enable(), "默认同进程不得装配沙盒");

        let openshell_without_gateway = SandboxConfig {
            mode: SandboxMode::OpenShell,
            base_url: "http://cubeapi:3000".to_string(),
            ..SandboxConfig::default()
        };
        assert!(
            !openshell_without_gateway.should_enable(),
            "OpenShell 未配置 gateway 时不得装配"
        );

        let openshell_with_gateway = SandboxConfig {
            mode: SandboxMode::OpenShell,
            openshell_gateway_url: "http://127.0.0.1:8080".to_string(),
            ..SandboxConfig::default()
        };
        assert!(openshell_with_gateway.should_enable());
    }

    #[test]
    fn defaults_are_safe_when_env_missing() {
        let defaults = SandboxConfig::default();
        assert_eq!(
            defaults.mode,
            SandboxMode::InProcess,
            "缺省必须是同进程（不启用沙盒）"
        );
        assert_eq!(defaults.timeout_secs, DEFAULT_TIMEOUT_SECS);
        assert!(defaults.session_ttl_secs >= 30);
        assert!(parse_bool_value("true"));
        assert!(parse_bool_value(" ON "));
        assert!(!parse_bool_value("false"));
        assert!(!parse_bool_value("0"));
        assert_eq!(parse_u64("__SANDBOX_NO_SUCH_ENV__", 7), 7);
        assert_eq!(parse_u32("__SANDBOX_NO_SUCH_ENV__", 9), 9);
    }

    #[test]
    fn resolve_mode_prefers_explicit_value() {
        assert_eq!(
            resolve_mode(Some("cube"), Some("false"), None),
            SandboxMode::Cube
        );
        assert_eq!(
            resolve_mode(Some(" OpenShell "), None, None),
            SandboxMode::OpenShell
        );
        assert_eq!(
            resolve_mode(Some("IN-PROCESS"), Some("true"), Some("cube")),
            SandboxMode::InProcess
        );
        assert_eq!(
            resolve_mode(Some("none"), Some("true"), Some("openshell")),
            SandboxMode::InProcess
        );
    }

    #[test]
    fn resolve_mode_falls_back_on_invalid_value() {
        assert_eq!(
            resolve_mode(Some("bogus"), Some("true"), Some("cube")),
            SandboxMode::InProcess
        );
    }

    #[test]
    fn resolve_mode_maps_legacy_keys() {
        assert_eq!(resolve_mode(None, None, None), SandboxMode::InProcess);
        assert_eq!(
            resolve_mode(None, Some("false"), Some("openshell")),
            SandboxMode::InProcess
        );
        assert_eq!(resolve_mode(None, Some("true"), None), SandboxMode::Cube);
        assert_eq!(
            resolve_mode(None, Some("1"), Some("openshell")),
            SandboxMode::OpenShell
        );
        assert_eq!(
            resolve_mode(None, Some("true"), Some("bogus")),
            SandboxMode::InProcess
        );
    }

    #[test]
    fn mode_as_str_round_trips_with_parse() {
        for mode in [
            SandboxMode::InProcess,
            SandboxMode::Cube,
            SandboxMode::OpenShell,
        ] {
            assert_eq!(SandboxMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(SandboxMode::parse("cube-sandbox"), Some(SandboxMode::Cube));
        assert_eq!(SandboxMode::parse("nope"), None);
    }

    #[test]
    fn error_message_combines_hint_and_detail() {
        let error = SandboxError::unsupported("沙箱未启用");
        let message = to_message(&error);
        assert!(message.contains(error.hint()), "{message}");
        assert!(message.contains("沙箱未启用"), "{message}");
    }

    #[test]
    fn session_env_registration_merges_and_stays_isolated() {
        use std::collections::BTreeMap;

        let key = SessionKey::new("__sandbox_env_test__", 1, "wf:env");
        let other = SessionKey::new("__sandbox_env_test__", 2, "wf:env");
        assert!(session_env(&key).is_empty(), "未登记时为空");

        let mut first = BTreeMap::new();
        first.insert("AGENT_TOKEN".to_string(), "t1".to_string());
        prepare_session_env(key.clone(), first);

        let mut second = BTreeMap::new();
        second.insert("AGENT_TOKEN".to_string(), "t2".to_string());
        second.insert("EXTRA".to_string(), "v".to_string());
        prepare_session_env(key.clone(), second);

        let env = session_env(&key);
        assert_eq!(
            env.get("AGENT_TOKEN").map(String::as_str),
            Some("t2"),
            "后登记的值覆盖先前的"
        );
        assert_eq!(env.get("EXTRA").map(String::as_str), Some("v"));
        assert!(session_env(&other).is_empty(), "不同会话之间必须隔离");

        prepare_session_env(key.clone(), BTreeMap::new());
        assert_eq!(session_env(&key), env, "空集合不清空已登记值");
    }
}
