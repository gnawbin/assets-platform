//! AI Agent 沙箱服务（CubeSandbox）
//!
//! 职责边界（设计文档 §6.1）：
//! 1. 配置装载：`SANDBOX_*` 环境变量（由 `.env.toml` 的 `[sandbox]` 段映射）；
//! 2. 运行时单例：`SANDBOX_ENABLED=false`（缺省）时装 [`DisabledRuntime`]，
//!    保证「不配置也能启动」，所有能力返回 `Unsupported`；
//! 3. 会话亲和：`SessionKey(tenant, user, scope) → 沙箱句柄`，避免每次执行都冷启动；
//! 4. TTL 回收：空闲超过 `SANDBOX_SESSION_TTL_SECS` 先 `pause`，超过
//!    `SANDBOX_KILL_AFTER_SECS` 再 `kill`。
//!
//! **不做的事**：不解析 HTTP、不碰权限判定（留在 `crates/api` 的 `authz`）。

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use sandbox::{
    DisabledRuntime, ExecRequest, ExecResult, FileEntry, HealthInfo, SandboxError, SandboxHandle,
    SandboxRuntime, SandboxSpec, SandboxState, SandboxSummary, DEFAULT_TIMEOUT_SECS,
};
use sandbox_cube::CubeSandboxRuntime;
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

/// 沙箱配置（**不含密钥**，密钥留在 [`sandbox_cube::CubeOptions`]，其 `Debug` 已脱敏）
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// 总开关
    pub enabled: bool,
    /// CubeAPI 根地址（仅用于日志与诊断展示）
    pub base_url: String,
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
            enabled: false,
            base_url: String::new(),
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
            enabled: parse_bool("SANDBOX_ENABLED", defaults.enabled),
            base_url: env_trim("SANDBOX_BASE_URL").unwrap_or_default(),
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

    /// 是否真正装配 CubeSandbox（`enabled` 且 `base_url` 非空）
    #[must_use]
    pub fn should_enable(&self) -> bool {
        self.enabled && !self.base_url.trim().is_empty()
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

/// 全局配置（进程内只读）
pub fn config() -> &'static SandboxConfig {
    CONFIG.get_or_init(SandboxConfig::from_env)
}

/// 全局运行时（缺省 [`DisabledRuntime`]）
pub fn runtime() -> Arc<dyn SandboxRuntime> {
    RUNTIME
        .get_or_init(|| {
            let config = config();
            if !config.should_enable() {
                tracing::info!(
                    enabled = config.enabled,
                    base_url = %config.base_url,
                    "沙箱未启用：装配 DisabledRuntime（代码执行能力返回 Unsupported）"
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

/// `true` / `1` / `yes` / `on` 视为开；其余一律按缺省值处理
fn parse_bool(key: &str, default: bool) -> bool {
    match env_trim(key).map(|v| v.to_ascii_lowercase()) {
        Some(value) => matches!(value.as_str(), "true" | "1" | "yes" | "on"),
        None => default,
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
    let handle = ensure_session(key).await?;
    let request = ExecRequest {
        argv,
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

/// 控制面健康检查
pub async fn health() -> Result<HealthInfo, String> {
    runtime()
        .health()
        .await
        .map_err(|error| to_message(&error))
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
    fn should_enable_requires_flag_and_base_url() {
        let enabled_with_url = SandboxConfig {
            enabled: true,
            base_url: "http://cubeapi:3000".to_string(),
            ..SandboxConfig::default()
        };
        assert!(enabled_with_url.should_enable());

        let enabled_without_url = SandboxConfig {
            enabled: true,
            base_url: "  ".to_string(),
            ..SandboxConfig::default()
        };
        assert!(!enabled_without_url.should_enable());

        let disabled_with_url = SandboxConfig {
            enabled: false,
            base_url: "http://cubeapi:3000".to_string(),
            ..SandboxConfig::default()
        };
        assert!(!disabled_with_url.should_enable());
    }

    #[test]
    fn defaults_are_safe_when_env_missing() {
        let defaults = SandboxConfig::default();
        assert!(!defaults.enabled, "缺省必须是关闭");
        assert_eq!(defaults.timeout_secs, DEFAULT_TIMEOUT_SECS);
        assert!(defaults.session_ttl_secs >= 30);
        assert!(parse_bool("__SANDBOX_NO_SUCH_ENV__", true));
        assert!(!parse_bool("__SANDBOX_NO_SUCH_ENV__", false));
        assert_eq!(parse_u64("__SANDBOX_NO_SUCH_ENV__", 7), 7);
        assert_eq!(parse_u32("__SANDBOX_NO_SUCH_ENV__", 9), 9);
    }

    #[test]
    fn error_message_combines_hint_and_detail() {
        let error = SandboxError::unsupported("沙箱未启用");
        let message = to_message(&error);
        assert!(message.contains(error.hint()), "{message}");
        assert!(message.contains("沙箱未启用"), "{message}");
    }
}
