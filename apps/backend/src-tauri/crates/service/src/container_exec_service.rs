//! 容器终端 Service（Docker + K8s，写操作）
//!
//! 复用 SSH 终端的成熟模式：`opening → active → closed/failed` 审计状态机、
//! 进程内 `DashMap` 会话 hub、单用户并发上限、空闲回收、一次性 WS 票据（票据由 api 层签发）。
//!
//! 三重开关（默认全关，实施方案 §5.2）：
//! 1. 环境开关 `OPS_CONTAINER_EXEC_ENABLED=false`；
//! 2. 连接开关 `ops_connection.exec_enabled = 1`；
//! 3. 权限点 `ops:terminal:exec`（由 HTTP 层 authz 保证）。
//!
//! 审计纪律：只记元数据（连接 / 容器 / shell / 字节数 / 退出码），
//! **不得**把终端输出写入日志或数据库。

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use ops::connector::docker::DockerConnector;
use ops::connector::k8s::K8sConnector;
use ops::model::{Provider, ResourceKey, ResourceKind};
use ops_exec::{DockerExecSession, K8sExecSession, TerminalBackend};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};
use utils::snowflake::next_id;

use crate::ops_service;

/// 单用户默认并发会话上限（与 SSH 同口径）
const DEFAULT_MAX_SESSIONS_PER_USER: usize = 3;
/// 默认空闲回收阈值（秒）
const DEFAULT_IDLE_SECS: i64 = 1800;
/// 空闲回收扫描间隔（秒）
const REAPER_INTERVAL_SECS: u64 = 60;
/// 会话最长存活时间（秒）
const MAX_SESSION_LIFETIME_SECS: i64 = 86_400;
/// 退出码未采集到时的哨兵值
const EXIT_UNKNOWN: i64 = i64::MIN;
/// 输出通道容量（背压，避免 WS 慢时无限缓存）
const OUTPUT_CHANNEL_CAPACITY: usize = 256;

/// 当前时间（epoch 毫秒）
fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
}

/// 运行时配置（首次读取后缓存，改了环境变量需重启）
struct ExecRuntimeConfig {
    enabled: bool,
    idle_secs: i64,
    max_sessions_per_user: usize,
    require_linked: bool,
}

fn config() -> &'static ExecRuntimeConfig {
    static CONFIG: OnceLock<ExecRuntimeConfig> = OnceLock::new();
    CONFIG.get_or_init(|| ExecRuntimeConfig {
        enabled: env_bool("OPS_CONTAINER_EXEC_ENABLED", false),
        idle_secs: std::env::var("OPS_CONTAINER_EXEC_IDLE_SECS")
            .ok()
            .and_then(|value| value.trim().parse::<i64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_IDLE_SECS),
        max_sessions_per_user: std::env::var("OPS_CONTAINER_EXEC_MAX_SESSIONS")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_MAX_SESSIONS_PER_USER),
        require_linked: env_bool("OPS_EXEC_REQUIRE_LINKED", false),
    })
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default)
}

/// 环境开关是否启用容器终端（供 API / 前端提示）
#[must_use]
pub fn exec_enabled() -> bool {
    config().enabled
}

/// 终端目标（Docker 与 K8s 共用同一套会话 / WS / 前端）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TerminalTarget {
    /// Docker 容器
    Docker {
        /// 容器 ID 或名称
        container: String,
    },
    /// K8s Pod
    K8s {
        /// 命名空间
        namespace: String,
        /// Pod 名
        pod: String,
        /// 容器名（多容器 Pod 必填）
        #[serde(default)]
        container: Option<String>,
    },
}

/// 开会话响应（票据用于随后的 WS 升级，结构与 SSH 完全一致）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct OpsTerminalSessionTicket {
    /// 会话 ID（字符串，避免 JS 精度丢失）
    pub session_id: String,
    /// 一次性 WS 票据
    pub ticket: String,
    /// 票据有效期（秒）
    pub expires_in: u64,
}

/// 会话输出事件（与 `ssh_service::SshEvent` 同构）
#[derive(Debug, Clone)]
pub enum ExecEvent {
    /// 容器输出字节
    Data(Vec<u8>),
    /// 远端退出码
    Exit(Option<i32>),
    /// 会话关闭
    Closed(String),
}

/// 进程内会话条目
struct SessionEntry {
    user_id: i64,
    connection_id: i64,
    provider: String,
    /// 开会话时的租户 schema 快照（WS 路由没有 auth 中间件）
    schema: String,
    opened_at: Instant,
    last_active: AtomicI64,
    exit_code: AtomicI64,
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
    /// 读泵与写入并发访问（backend 内部按通道加锁）
    backend: Arc<dyn TerminalBackend>,
    /// 输出端：WS 升级时被 `attach_output` 取走，只能取一次
    output: Mutex<Option<mpsc::Receiver<ExecEvent>>>,
    /// 输出泵任务句柄（关闭时先 abort，避免卡在阻塞读）
    pump: Mutex<Option<JoinHandle<()>>>,
}

fn hub() -> &'static DashMap<i64, Arc<SessionEntry>> {
    static HUB: OnceLock<DashMap<i64, Arc<SessionEntry>>> = OnceLock::new();
    HUB.get_or_init(DashMap::new)
}

/// 单用户当前并发会话数
fn session_count_for_user(user_id: i64) -> usize {
    hub()
        .iter()
        .filter(|entry| entry.value().user_id == user_id)
        .count()
}

/// 取写连接池
fn write_pool() -> Result<sqlx::PgPool, String> {
    database::get_write_pool().map_err(|err| format!("数据库连接失败: {err}"))
}

/// 读取连接行（软删视为不存在）
async fn load_connection(connection_id: i64) -> Result<database::models::OpsConnection, String> {
    ops_service::load_row(connection_id).await
}

/// 插入审计行，返回会话 id
#[allow(clippy::too_many_arguments)]
async fn insert_session_row(
    connection_id: i64,
    user_id: i64,
    provider: &str,
    target_kind: &str,
    container_uid: Option<&str>,
    namespace: Option<&str>,
    pod_name: Option<&str>,
    container_name: Option<&str>,
    shell: Option<&str>,
    client_ip: Option<&str>,
) -> Result<i64, String> {
    let pool = write_pool()?;
    let p = database::schema_prefix();
    let sql = format!(
        "INSERT INTO {p}ops_terminal_session \
         (id, provider, connection_id, target_kind, container_uid, namespace, pod_name, \
          container_name, shell, user_id, state, client_ip, started_at, bytes_in, bytes_out) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'opening', $11, NOW(), 0, 0) \
         RETURNING id"
    );
    sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(sql))
        .bind(next_id() as i64)
        .bind(provider)
        .bind(connection_id)
        .bind(target_kind)
        .bind(container_uid)
        .bind(namespace)
        .bind(pod_name)
        .bind(container_name)
        .bind(shell)
        .bind(user_id)
        .bind(client_ip)
        .fetch_one(&pool)
        .await
        .map_err(|err| format!("创建容器终端会话审计失败: {err}"))
}

/// 更新审计行（调用方负责租户上下文）
async fn finalize_session_row(
    session_id: i64,
    state: &str,
    bytes_in: i64,
    bytes_out: i64,
    exit_code: Option<i32>,
    error: Option<&str>,
) -> Result<(), String> {
    let pool = write_pool()?;
    let p = database::schema_prefix();
    let sql = format!(
        "UPDATE {p}ops_terminal_session SET state = $2, bytes_in = $3, bytes_out = $4, \
         exit_code = $5, error = $6, ended_at = COALESCE(ended_at, NOW()), updated_at = NOW() \
         WHERE id = $1"
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(session_id)
        .bind(state)
        .bind(bytes_in)
        .bind(bytes_out)
        .bind(exit_code)
        .bind(error)
        .execute(&pool)
        .await
        .map_err(|err| format!("更新容器终端会话审计失败: {err}"))?;
    Ok(())
}

/// 更新会话为 active
async fn finalize_active_row(session_id: i64) -> Result<(), String> {
    let pool = write_pool()?;
    let p = database::schema_prefix();
    let sql = format!(
        "UPDATE {p}ops_terminal_session SET state = 'active', updated_at = NOW() WHERE id = $1"
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(session_id)
        .execute(&pool)
        .await
        .map_err(|err| format!("更新容器终端会话状态失败: {err}"))?;
    Ok(())
}

/// 严格模式下要求资源已在 `asset_ops_link` 中登记
async fn ensure_linked(
    connection_id: i64,
    config: &ops::ConnectionConfig,
    container_uid: Option<&str>,
    namespace: Option<&str>,
    pod_name: Option<&str>,
) -> Result<(), String> {
    let uid = match (container_uid, pod_name) {
        (Some(container_id), _) => {
            // Swarm 与单机 Docker 的 resource_uid 前缀不同，必须跟随连接来源
            let provider = if config.provider == Provider::Swarm {
                Provider::Swarm
            } else {
                Provider::Docker
            };
            ResourceKey::new(
                provider,
                config.endpoint_key(),
                ResourceKind::Container,
                container_id.to_string(),
            )
            .uid()
        }
        (None, Some(pod)) => {
            let namespace = namespace
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "缺少命名空间，无法校验资源登记".to_string())?;
            ResourceKey::new(
                Provider::Kubernetes,
                config.endpoint_key(),
                ResourceKind::Pod,
                pod.to_string(),
            )
            .with_namespace(namespace.to_string())
            .uid()
        }
        _ => return Ok(()),
    };
    let pool = database::get_read_pool().map_err(|err| format!("数据库连接失败: {err}"))?;
    let p = database::schema_prefix();
    let sql = format!(
        "SELECT 1 FROM {p}asset_ops_link \
         WHERE connection_id = $1 AND resource_uid = $2 AND deleted = 0 LIMIT 1"
    );
    let hit = sqlx::query_scalar::<_, i32>(sqlx::AssertSqlSafe(sql))
        .bind(connection_id)
        .bind(&uid)
        .fetch_optional(&pool)
        .await
        .map_err(|err| format!("校验资源登记失败: {err}"))?;
    if hit.is_none() {
        return Err("该资源尚未纳入运维采集（严格模式要求先采集成功）".to_string());
    }
    Ok(())
}


/// 打开成功的 backend 及其审计元数据
struct OpenedSession {
    backend: Box<dyn TerminalBackend>,
    shell: String,
    container_uid: Option<String>,
    container_name: Option<String>,
    namespace: Option<String>,
    pod_name: Option<String>,
}

/// 按目标类型打开 backend（Docker / K8s）
async fn open_backend(
    config: ops::ConnectionConfig,
    target: &TerminalTarget,
    shell: Option<&str>,
    cols: u32,
    rows: u32,
) -> Result<OpenedSession, String> {
    match target {
        TerminalTarget::Docker { container } => {
            if !matches!(config.provider, Provider::Docker | Provider::Swarm) {
                return Err("该连接不是 Docker / Swarm 来源，无法打开容器终端".to_string());
            }
            let connector = DockerConnector::connect(config)
                .map_err(|err| format!("初始化 Docker 连接器失败：{err}"))?;
            let session = DockerExecSession::open(connector, container, shell, cols, rows)
                .await
                .map_err(|err| err.to_string())?;
            Ok(OpenedSession {
                shell: session.shell().to_string(),
                container_uid: Some(session.container_id().to_string()),
                container_name: Some(container.clone()),
                namespace: None,
                pod_name: None,
                backend: Box::new(session),
            })
        }
        TerminalTarget::K8s {
            namespace,
            pod,
            container,
        } => {
            if config.provider != Provider::Kubernetes {
                return Err("该连接不是 Kubernetes 来源，无法打开 Pod 终端".to_string());
            }
            let connector = K8sConnector::connect(config)
                .await
                .map_err(|err| format!("初始化 Kubernetes 连接器失败：{err}"))?;
            let session = K8sExecSession::open(
                connector,
                namespace,
                pod,
                container.as_deref(),
                shell,
                cols,
                rows,
            )
            .await
            .map_err(|err| err.to_string())?;
            Ok(OpenedSession {
                shell: session.shell().to_string(),
                container_uid: None,
                container_name: container.clone(),
                namespace: Some(namespace.clone()),
                pod_name: Some(pod.clone()),
                backend: Box::new(session),
            })
        }
    }
}


/// 打开终端会话：三重开关 → 限额 → 建 backend → 审计 → 注册 hub
///
/// 返回会话 id，由 api 层再签发一次性 WS 票据。
///
/// # Errors
///
/// 未启用、连接 / 容器 / Pod 不存在、镜像无 shell、并发超限、连接失败。
pub async fn open_session(
    user_id: i64,
    connection_id: i64,
    target: &TerminalTarget,
    shell: Option<&str>,
    cols: u32,
    rows: u32,
    client_ip: Option<String>,
) -> Result<i64, String> {
    if !config().enabled {
        return Err("未启用容器终端（OPS_CONTAINER_EXEC_ENABLED=false）".to_string());
    }
    let limit = config().max_sessions_per_user;
    let active = session_count_for_user(user_id);
    if active >= limit {
        return Err(format!(
            "当前已有 {active} 个容器终端会话，已达上限 {limit}，请先关闭不用的会话"
        ));
    }

    let row = load_connection(connection_id).await?;
    if row.enabled == 0 {
        return Err(format!("连接「{}」已停用", row.connection_name));
    }
    if row.exec_enabled == 0 {
        return Err(format!(
            "连接「{}」未允许容器终端，请先在连接配置中打开「允许容器终端」",
            row.connection_name
        ));
    }
    if row.auth_type == "plain_tcp" {
        return Err(
            "明文 TCP（2375）端点禁止打开终端，请改用本机 socket 或 mTLS（2376）".to_string(),
        );
    }
    let conn_config = ops_service::config_from_row(&row)?;
    let provider = match target {
        TerminalTarget::Docker { .. } => match conn_config.provider {
            Provider::Docker => "docker",
            Provider::Swarm => "swarm",
            _ => return Err("目标类型与连接来源不匹配".to_string()),
        },
        TerminalTarget::K8s { .. } => "k8s",
    };

    let opened = open_backend(conn_config.clone(), target, shell, cols, rows).await?;
    if config().require_linked
        && let Err(err) = ensure_linked(
            connection_id,
            &conn_config,
            opened.container_uid.as_deref(),
            opened.namespace.as_deref(),
            opened.pod_name.as_deref(),
        )
        .await
    {
        opened.backend.close().await;
        return Err(err);
    }

    let (target_kind, container_uid, namespace, pod_name, container_name) = match target {
        TerminalTarget::Docker { .. } => (
            "docker_container",
            opened.container_uid.as_deref(),
            None,
            None,
            opened.container_name.as_deref(),
        ),
        TerminalTarget::K8s {
            namespace,
            pod,
            container,
        } => (
            "k8s_pod",
            None,
            Some(namespace.as_str()),
            Some(pod.as_str()),
            container.as_deref(),
        ),
    };
    let session_id = insert_session_row(
        connection_id,
        user_id,
        provider,
        target_kind,
        container_uid,
        namespace,
        pod_name,
        container_name,
        Some(opened.shell.as_str()),
        client_ip.as_deref(),
    )
    .await?;


    // 输出泵：把 backend 的输出转成 ExecEvent；send 失败（WS 关闭）即退出
    let (tx, rx) = mpsc::channel::<ExecEvent>(OUTPUT_CHANNEL_CAPACITY);
    let entry = Arc::new(SessionEntry {
        user_id,
        connection_id,
        provider: provider.to_string(),
        schema: database::current_schema_name(),
        opened_at: Instant::now(),
        last_active: AtomicI64::new(now_millis()),
        exit_code: AtomicI64::new(EXIT_UNKNOWN),
        bytes_in: AtomicU64::new(0),
        bytes_out: AtomicU64::new(0),
        backend: Arc::from(opened.backend),
        output: Mutex::new(Some(rx)),
        pump: Mutex::new(None),
    });
    let pump_entry = Arc::clone(&entry);
    let pump = tokio::spawn(async move {
        let backend = Arc::clone(&pump_entry.backend);
        loop {
            let next = backend.next_chunk().await;
            match next {
                Some(Ok(data)) => {
                    let len = u64::try_from(data.len()).unwrap_or(u64::MAX);
                    pump_entry.bytes_out.fetch_add(len, Ordering::Relaxed);
                    pump_entry.last_active.store(now_millis(), Ordering::Relaxed);
                    if tx.send(ExecEvent::Data(data)).await.is_err() {
                        break;
                    }
                }
                Some(Err(err)) => {
                    let _ = tx.send(ExecEvent::Closed(err.to_string())).await;
                    break;
                }
                None => {
                    let code = backend.exit_code().await;
                    if let Some(code) = code {
                        pump_entry
                            .exit_code
                            .store(i64::from(code), Ordering::Relaxed);
                    }
                    let _ = tx.send(ExecEvent::Exit(code)).await;
                    let _ = tx.send(ExecEvent::Closed("会话已结束".to_string())).await;
                    break;
                }
            }
        }
    });
    if let Ok(mut slot) = entry.pump.lock() {
        *slot = Some(pump);
    }
    hub().insert(session_id, entry);
    if let Err(err) = finalize_active_row(session_id).await {
        warn!(session_id, error = %err, "更新容器终端会话为 active 失败");
    }
    info!(
        session_id,
        user_id,
        connection_id,
        provider,
        target_kind,
        shell = opened.shell,
        "容器终端会话已打开"
    );
    Ok(session_id)
}


/// 取会话条目（校验归属；非归属者一律按「不存在」处理，避免枚举他人会话）
fn owned_entry(session_id: i64, user_id: i64) -> Result<Arc<SessionEntry>, String> {
    let entry = hub()
        .get(&session_id)
        .map(|entry| Arc::clone(entry.value()))
        .ok_or_else(|| "会话不存在".to_string())?;
    if entry.user_id != user_id {
        return Err("会话不存在".to_string());
    }
    Ok(entry)
}

/// 写入键盘输入（刷新活跃时间 + 累计上行字节）
///
/// # Errors
///
/// 会话不存在 / 不属于当前用户 / 已断开。
pub async fn write_input(session_id: i64, user_id: i64, data: Vec<u8>) -> Result<(), String> {
    let entry = owned_entry(session_id, user_id)?;
    entry.last_active.store(now_millis(), Ordering::Relaxed);
    let len = u64::try_from(data.len()).unwrap_or(u64::MAX);
    entry.bytes_in.fetch_add(len, Ordering::Relaxed);
    entry
        .backend
        .write(&data)
        .await
        .map_err(|err| err.to_string())
}

/// 上报窗口大小变化（夹到 1..=1000，与 SSH 口径一致）
///
/// # Errors
///
/// 会话不存在或不属于当前用户。
pub async fn resize_session(
    session_id: i64,
    user_id: i64,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    let entry = owned_entry(session_id, user_id)?;
    entry.last_active.store(now_millis(), Ordering::Relaxed);
    let cols = cols.clamp(1, 1000);
    let rows = rows.clamp(1, 1000);
    entry
        .backend
        .resize(cols, rows)
        .await
        .map_err(|err| err.to_string())
}

/// 取会话输出端（WS 升级后调用；只能取一次）
///
/// # Errors
///
/// 会话不存在 / 不属于当前用户 / 输出已被接管。
pub fn attach_output(
    session_id: i64,
    user_id: i64,
) -> Result<mpsc::Receiver<ExecEvent>, String> {
    let entry = owned_entry(session_id, user_id)?;
    let taken = entry
        .output
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    taken.ok_or_else(|| "会话输出已被接管".to_string())
}

/// 记录退出码（WS 泵收到 `Exit` 事件时调用）
pub fn note_exit(session_id: i64, code: Option<i32>) {
    if let (Some(entry), Some(code)) = (hub().get(&session_id), code) {
        entry.exit_code.store(i64::from(code), Ordering::Relaxed);
    }
}

/// 关闭会话（幂等：会话已不存在视为成功）
///
/// 审计写入使用开会话时快照的 schema，避免 WS 路由（无 auth 中间件）写错租户。
///
/// # Errors
///
/// 会话属于其他用户时返回「会话不存在」。
pub async fn close_session(session_id: i64, user_id: i64, reason: &str) -> Result<(), String> {
    let Some(entry) = hub().remove(&session_id).map(|(_, entry)| entry) else {
        // 双触发（WS 断线 + 前端 DELETE）时保持幂等
        return Ok(());
    };
    if entry.user_id != user_id {
        hub().insert(session_id, entry);
        return Err("会话不存在".to_string());
    }
    // 先 abort 输出泵（它可能正阻塞在 read 上），再关闭 backend
    let pump = entry
        .pump
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if let Some(handle) = pump {
        handle.abort();
    }
    entry.backend.close().await;

    let raw_exit = entry.exit_code.load(Ordering::Relaxed);
    let exit_code = (raw_exit != EXIT_UNKNOWN)
        .then(|| i32::try_from(raw_exit).ok())
        .flatten();
    let bytes_in = i64::try_from(entry.bytes_in.load(Ordering::Relaxed)).unwrap_or(i64::MAX);
    let bytes_out = i64::try_from(entry.bytes_out.load(Ordering::Relaxed)).unwrap_or(i64::MAX);
    let schema = entry.schema.clone();
    let audit = database::with_schema(schema, async move {
        finalize_session_row(session_id, "closed", bytes_in, bytes_out, exit_code, None).await
    })
    .await;
    if let Err(err) = audit {
        warn!(session_id, error = %err, "写容器终端会话审计失败");
    }
    debug!(
        session_id,
        user_id,
        connection_id = entry.connection_id,
        provider = %entry.provider,
        reason,
        bytes_in,
        bytes_out,
        exit_code,
        "容器终端会话已关闭"
    );
    Ok(())
}


/// 是否应回收（空闲超阈值或存活超上限）
fn should_reap(now: i64, last_active: i64, opened_for: u64, idle_secs: i64) -> bool {
    let idle = now.saturating_sub(last_active) / 1000;
    idle >= idle_secs || opened_for >= u64::try_from(MAX_SESSION_LIFETIME_SECS).unwrap_or(u64::MAX)
}

/// 启动空闲会话回收任务（进程内只启动一次）
pub fn spawn_idle_reaper() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if !config().enabled {
        debug!("容器终端未启用：跳过会话空闲回收任务");
        return;
    }
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        let interval = Duration::from_secs(REAPER_INTERVAL_SECS);
        loop {
            tokio::time::sleep(interval).await;
            reap_once().await;
        }
    });
    info!(
        interval_secs = REAPER_INTERVAL_SECS,
        "容器终端会话空闲回收任务已启动"
    );
}

/// 扫描一轮：空闲超阈值或存活超上限的会话直接关闭（含审计落库）
async fn reap_once() {
    let now = now_millis();
    let idle_secs = config().idle_secs;
    let candidates: Vec<(i64, i64)> = hub()
        .iter()
        .filter(|entry| {
            let last_active = entry.last_active.load(Ordering::Relaxed);
            let opened_for = entry.opened_at.elapsed().as_secs();
            should_reap(now, last_active, opened_for, idle_secs)
        })
        .map(|entry| (*entry.key(), entry.user_id))
        .collect();

    for (session_id, user_id) in candidates {
        match close_session(session_id, user_id, "空闲超时回收").await {
            Ok(()) => info!(session_id, "容器终端会话已空闲回收"),
            Err(err) => warn!(session_id, error = %err, "容器终端会话空闲回收失败"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_threshold_reaps_only_expired_sessions() {
        assert!(!should_reap(10_000, 9_000, 1, 30));
        assert!(should_reap(40_000, 10_000, 30, 30));
        assert!(!should_reap(10_000, 10_000, 100, 30));
        assert!(should_reap(
            10_000,
            10_000,
            u64::try_from(MAX_SESSION_LIFETIME_SECS).unwrap_or(u64::MAX),
            30
        ));
    }

    #[test]
    fn env_bool_accepts_common_truthy_values() {
        // 只测纯解析分支，不污染进程环境变量
        for value in ["1", "true", "YES", "on"] {
            assert!(matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            ));
        }
        assert!(!matches!(
            "false".trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ));
    }

    #[test]
    fn terminal_target_serializes_with_kind_tag() {
        let target = TerminalTarget::K8s {
            namespace: "prod".to_string(),
            pod: "web-0".to_string(),
            container: Some("web".to_string()),
        };
        let json = serde_json::to_value(&target).expect("序列化");
        assert_eq!(json["kind"], "k8s");
        assert_eq!(json["namespace"], "prod");
    }
}

