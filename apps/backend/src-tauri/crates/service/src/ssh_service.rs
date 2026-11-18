//! SSH 终端 Service
//!
//! 分层与 `ops_service` 一致：本层是唯一业务入口，不认识 HTTP / Tauri：
//! - 主机配置 CRUD（`ops_ssh_target`，凭证只以 AES-256-GCM 密文落库）
//! - 指纹探测（`probe`：只握手不认证）与会话打开
//! - 会话 hub（进程内 `DashMap` + `mpsc`）+ 审计行（`ops_ssh_session`）
//!
//! 三条硬约束：
//! 1. **凭证只进不出**：视图只有 `has_credential` / `credential_hint`，密文列 `#[serde(skip)]`；
//! 2. **指纹不静默接受**：已固定指纹必须逐字匹配，失配直接失败；
//! 3. **审计不依赖 task-local schema**：WS 路由没有 auth 中间件，关闭会话时用
//!    `database::with_schema(entry.schema, ...)` 显式还原租户上下文。

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use database::models::{OpsSshSession, OpsSshTarget};
use remote_shell::{
    HostKeyInfo, SshAuth, SshEvent, SshSession, SshTargetSpec, fingerprint_matches,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use utils::crypto::{decrypt_api_key, encrypt_api_key};
use utils::snowflake::next_id;

/// `ops_ssh_target` 全列（`RETURNING` / `SELECT` 共用，保证 `FromRow` 字段对齐）
const TARGET_COLUMNS: &str = "id, target_name, host, port, username, auth_type, auth_config_enc, \
     credential_hint, host_key_fingerprint, strict_host_key_check, asset_id, description, \
     connect_timeout_secs, enabled, last_connected_at, last_error, created_by, created_at, \
     updated_by, updated_at, deleted";

/// `ops_ssh_session` 全列
const SESSION_COLUMNS: &str = "id, target_id, user_id, state, client_ip, started_at, ended_at, \
     bytes_in, bytes_out, exit_code, error";

/// 单用户默认并发会话上限
const DEFAULT_MAX_SESSIONS_PER_USER: usize = 3;
/// 默认空闲回收阈值（秒）
const DEFAULT_IDLE_SECS: i64 = 1800;
/// 空闲回收扫描间隔（秒）
const REAPER_INTERVAL_SECS: u64 = 60;
/// 会话最长存活时间（秒）：防止忘记关闭的会话永久占用连接
const MAX_SESSION_LIFETIME_SECS: i64 = 86_400;
/// 打开会话时在传输层超时之上追加的冗余（秒）
const OPEN_TIMEOUT_MARGIN_SECS: u64 = 15;
/// 退出码未采集到时的哨兵值（远端正常退出才写入真实值）
const EXIT_UNKNOWN: i64 = i64::MIN;

/// 当前时间（epoch 毫秒），用于 `last_active` 比较
fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
}

/// 运行时配置（首次读取后缓存；改了环境变量需重启，与 sandbox 一致）
struct SshConfig {
    idle_secs: i64,
    max_sessions_per_user: usize,
}

fn config() -> &'static SshConfig {
    static CONFIG: OnceLock<SshConfig> = OnceLock::new();
    CONFIG.get_or_init(|| SshConfig {
        idle_secs: std::env::var("SSH_SESSION_IDLE_SECS")
            .ok()
            .and_then(|value| value.trim().parse::<i64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_IDLE_SECS),
        max_sessions_per_user: std::env::var("SSH_MAX_SESSIONS_PER_USER")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_MAX_SESSIONS_PER_USER),
    })
}

// ======================== 入参 DTO ========================

/// 新建 / 编辑主机的入参
///
/// 编辑时 `password` / `private_key_pem` 全缺省 = **保留原凭证**；
/// `auth_type` 留空时按提供的凭证自动判定。
#[derive(Debug, Clone, Default, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SshTargetInput {
    /// 主机别名（同租户内唯一）
    #[serde(alias = "targetName")]
    pub name: String,
    /// 主机名或 IP（不含 scheme / 端口）
    pub host: String,
    /// 端口，缺省 22
    #[serde(default)]
    pub port: Option<u16>,
    /// 登录账号
    pub username: String,
    /// 认证方式：password / private_key
    #[serde(default, alias = "authType")]
    pub auth_type: Option<String>,
    /// 密码（仅 `auth_type=password` 时使用）
    #[serde(default)]
    pub password: Option<String>,
    /// 私钥 PEM（仅 `auth_type=private_key` 时使用）
    #[serde(default, alias = "privateKeyPem")]
    pub private_key_pem: Option<String>,
    /// 私钥口令（可选）
    #[serde(default)]
    pub passphrase: Option<String>,
    /// 是否严格校验主机指纹（缺省 true）
    #[serde(default, alias = "strictHostKeyCheck")]
    pub strict_host_key_check: Option<bool>,
    /// 已知主机指纹（`SHA256:...`；留空表示未固定）
    #[serde(default, alias = "hostKeyFingerprint")]
    pub host_key_fingerprint: Option<String>,
    /// 绑定的资产台账 id（软引用）
    #[serde(
        default,
        alias = "assetId",
        deserialize_with = "database::models::opt_i64_from_string"
    )]
    pub asset_id: Option<i64>,
    /// 备注
    #[serde(default)]
    pub description: Option<String>,
    /// 连接超时（秒，3-60）
    #[serde(default, alias = "connectTimeoutSecs")]
    pub connect_timeout_secs: Option<u32>,
    /// 启用标记
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// 表单未保存时的指纹探测入参（凭证只用于本次握手，不落库）
#[derive(Debug, Clone, Default, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SshProbeInput {
    /// 主机名或 IP
    pub host: String,
    /// 端口，缺省 22
    #[serde(default)]
    pub port: Option<u16>,
    /// 登录账号（探测不认证，仅用于参数校验）
    pub username: String,
    /// 认证方式：password / private_key
    #[serde(default, alias = "authType")]
    pub auth_type: Option<String>,
    /// 密码
    #[serde(default)]
    pub password: Option<String>,
    /// 私钥 PEM
    #[serde(default, alias = "privateKeyPem")]
    pub private_key_pem: Option<String>,
    /// 私钥口令
    #[serde(default)]
    pub passphrase: Option<String>,
    /// 连接超时（秒，3-60）
    #[serde(default, alias = "connectTimeoutSecs")]
    pub connect_timeout_secs: Option<u32>,
}

// ======================== 出参 DTO ========================

/// 主机视图（**不含任何凭证 / 密文**）
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SshTargetView {
    /// 主键（字符串，避免 JS 精度丢失）
    pub id: String,
    /// 主机别名
    pub name: String,
    /// 主机名或 IP
    pub host: String,
    /// 端口
    pub port: u16,
    /// 登录账号
    pub username: String,
    /// 认证方式：password / private_key
    pub auth_type: String,
    /// 是否已配置凭证
    pub has_credential: bool,
    /// 凭证提示（绝不含明文）
    pub credential_hint: Option<String>,
    /// 已固定的主机指纹
    pub host_key_fingerprint: Option<String>,
    /// 是否严格校验指纹
    pub strict_host_key_check: bool,
    /// 绑定的资产 id
    pub asset_id: Option<String>,
    /// 备注
    pub description: Option<String>,
    /// 连接超时（秒）
    pub connect_timeout_secs: u32,
    /// 是否启用
    pub enabled: bool,
    /// 最近一次成功连接时间（RFC3339）
    pub last_connected_at: Option<String>,
    /// 最近一次失败原因
    pub last_error: Option<String>,
    /// 创建时间（RFC3339）
    pub created_at: Option<String>,
    /// 更新时间（RFC3339）
    pub updated_at: Option<String>,
}

/// 指纹探测结果
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SshProbeView {
    /// `SHA256:<base64>` 指纹
    pub fingerprint: String,
    /// 密钥算法（`ssh-ed25519` / `rsa-sha2-512` ...）
    pub key_type: String,
    /// 与已记录指纹是否一致；`None` 表示此前没有记录
    pub matched: Option<bool>,
    /// 探测耗时（毫秒）
    pub latency_ms: u64,
}

/// 会话视图（审计口径）
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SshSessionView {
    /// 会话 id
    pub id: String,
    /// 目标主机 id
    pub target_id: String,
    /// 目标主机别名
    pub target_name: String,
    /// 发起用户 id
    pub user_id: String,
    /// 状态：opening / active / closed / failed
    pub state: String,
    /// 客户端 IP
    pub client_ip: Option<String>,
    /// 开始时间（RFC3339）
    pub started_at: Option<String>,
    /// 结束时间（RFC3339）
    pub ended_at: Option<String>,
    /// 客户端 → 主机 字节数
    pub bytes_in: i64,
    /// 主机 → 客户端 字节数
    pub bytes_out: i64,
    /// 远端退出码
    pub exit_code: Option<i32>,
    /// 失败原因
    pub error: Option<String>,
}

/// 开会话的响应（api 层再补一次性票据）
#[derive(Debug, Clone, Default, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SshSessionTicket {
    /// 会话 id（字符串）
    pub session_id: String,
    /// 一次性 WS 票据
    pub ticket: String,
    /// 票据有效期（秒）
    pub expires_in: u64,
}

/// 主机行 → 视图（凭证不出本层）
#[must_use]
pub fn target_to_view(row: &OpsSshTarget) -> SshTargetView {
    SshTargetView {
        id: row.id.to_string(),
        name: row.target_name.clone(),
        host: row.host.clone(),
        port: u16::try_from(row.port).unwrap_or(22),
        username: row.username.clone(),
        auth_type: row.auth_type.clone(),
        has_credential: row
            .auth_config_enc
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()),
        credential_hint: row.credential_hint.clone(),
        host_key_fingerprint: row.host_key_fingerprint.clone(),
        strict_host_key_check: row.strict_host_key_check != 0,
        asset_id: row.asset_id.map(|id| id.to_string()),
        description: row.description.clone(),
        connect_timeout_secs: u32::try_from(row.connect_timeout_secs.max(1)).unwrap_or(10),
        enabled: row.enabled != 0,
        last_connected_at: row.last_connected_at.map(|time| time.to_rfc3339()),
        last_error: row.last_error.clone(),
        created_at: row.created_at.map(|time| time.to_rfc3339()),
        updated_at: row.updated_at.map(|time| time.to_rfc3339()),
    }
}

/// 凭证提示文案（永不包含明文）
#[must_use]
pub fn credential_hint_for(auth_type: &str) -> String {
    match auth_type {
        "private_key" => "私钥（已配置）".to_string(),
        _ => "密码（已配置）".to_string(),
    }
}

/// 明文凭证载荷（序列化后整体加密进 `auth_config_enc`）
#[derive(Debug, Default, Serialize, Deserialize)]
struct SshSecret {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    private_key_pem: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    passphrase: Option<String>,
}

/// 归一化主机输入：去空白，端口缺省 22
fn normalize_target_input(input: &mut SshTargetInput) {
    input.host = input.host.trim().to_string();
    input.username = input.username.trim().to_string();
    input.name = input.name.trim().to_string();
    input.port = Some(input.port.unwrap_or(remote_shell::DEFAULT_PORT));
    input.passphrase = input
        .passphrase
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    input.host_key_fingerprint = input
        .host_key_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
}

/// 解析本次写入要落库的凭证三元组 `(auth_type, 密文, 提示)`
///
/// - 提供了密码 / 私钥 → 加密本次输入（认证方式由凭证种类决定）；
/// - 两者都缺省 → 沿用 `existing`（编辑时不改凭证）；
/// - 全新记录且没有任何凭证 → 报错。
fn resolve_secret(
    input: &SshTargetInput,
    existing: Option<&OpsSshTarget>,
) -> Result<(String, String, String), String> {
    let password = input.password.as_deref().filter(|value| !value.is_empty());
    let pem = input
        .private_key_pem
        .as_deref()
        .filter(|value| !value.trim().is_empty());

    // 显式指定了认证方式却提供了另一种凭证时直接拒绝，避免「存了密码却按私钥连」
    if let Some(declared) = input.auth_type.as_deref().map(str::trim) {
        match declared {
            "" => {}
            "password" if password.is_none() && pem.is_some() => {
                return Err("认证方式为 password，但只提供了私钥".to_string());
            }
            "private_key" if pem.is_none() && password.is_some() => {
                return Err("认证方式为 private_key，但只提供了密码".to_string());
            }
            "password" | "private_key" => {}
            other => return Err(format!("不支持的认证方式：{other}")),
        }
    }

    if let Some(password) = password {
        let secret = SshSecret {
            password: Some(password.to_string()),
            private_key_pem: None,
            passphrase: None,
        };
        return Ok((
            "password".to_string(),
            encrypt_secret(&secret)?,
            credential_hint_for("password"),
        ));
    }
    if let Some(pem) = pem {
        let secret = SshSecret {
            password: None,
            private_key_pem: Some(pem.to_string()),
            passphrase: input.passphrase.clone(),
        };
        return Ok((
            "private_key".to_string(),
            encrypt_secret(&secret)?,
            credential_hint_for("private_key"),
        ));
    }

    match existing {
        Some(row) => {
            let encrypted = row
                .auth_config_enc
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "该主机缺少凭证配置，请重新填写密码或私钥".to_string())?;
            Ok((
                row.auth_type.clone(),
                encrypted.to_string(),
                row.credential_hint
                    .clone()
                    .unwrap_or_else(|| credential_hint_for(&row.auth_type)),
            ))
        }
        None => Err("请填写密码或私钥（凭证不能为空）".to_string()),
    }
}

/// 序列化并加密凭证
fn encrypt_secret(secret: &SshSecret) -> Result<String, String> {
    let json = serde_json::to_string(secret).map_err(|err| format!("序列化凭证失败：{err}"))?;
    encrypt_api_key(&json).map_err(|err| format!("未设置加密密钥，无法保存凭证：{err}"))
}

/// 解密凭证载荷
fn decrypt_secret(row: &OpsSshTarget) -> Result<SshSecret, String> {
    let encrypted = row
        .auth_config_enc
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("主机「{}」缺少凭证配置，请重新编辑保存", row.target_name))?;
    let json = decrypt_api_key(encrypted).map_err(|err| format!("解密主机凭证失败：{err}"))?;
    serde_json::from_str(&json).map_err(|err| format!("解析主机凭证失败：{err}"))
}

/// 行 → 传输层认证材料
fn auth_from_row(row: &OpsSshTarget) -> Result<SshAuth, String> {
    let secret = decrypt_secret(row)?;
    match row.auth_type.as_str() {
        "password" => secret
            .password
            .filter(|value| !value.is_empty())
            .map(SshAuth::Password)
            .ok_or_else(|| format!("主机「{}」的密码为空，请重新编辑保存", row.target_name)),
        "private_key" => secret
            .private_key_pem
            .filter(|value| !value.trim().is_empty())
            .map(|pem| SshAuth::PrivateKey {
                pem,
                passphrase: secret.passphrase,
            })
            .ok_or_else(|| format!("主机「{}」的私钥为空，请重新编辑保存", row.target_name)),
        other => Err(format!("不支持的认证方式：{other}")),
    }
}

/// 行 → 传输层连接参数
///
/// `verify_host_key` 为 `true` 时把已固定指纹交给传输层严格比对；
/// 为 `false` 时只观察指纹（扫描 / 首次纳管）。
fn spec_from_row(
    row: &OpsSshTarget,
    auth: SshAuth,
    verify_host_key: bool,
) -> Result<SshTargetSpec, String> {
    let port = u16::try_from(row.port)
        .ok()
        .filter(|port| *port > 0)
        .ok_or_else(|| format!("主机「{}」的端口非法：{}", row.target_name, row.port))?;
    let expected_fingerprint = if verify_host_key {
        row.host_key_fingerprint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    } else {
        None
    };
    Ok(SshTargetSpec {
        host: row.host.clone(),
        port,
        username: row.username.clone(),
        auth,
        timeout_secs: u64::try_from(row.connect_timeout_secs.max(1)).unwrap_or(10),
        expected_fingerprint,
    })
}

// ======================== 会话 hub ========================

/// 进程内会话条目（真正的 TCP 连接在这个进程里，DB 只是审计事实）
struct HubEntry {
    user_id: i64,
    target_id: i64,
    target_name: String,
    /// 开会话时的租户 schema 快照（WS 路由没有 auth 中间件）
    schema: String,
    opened_at: Instant,
    /// 最近活跃时间（epoch 毫秒）
    last_active: AtomicI64,
    /// 远端退出码（`EXIT_UNKNOWN` 表示未采集到）
    exit_code: AtomicI64,
    /// 持有的会话句柄（写输入 / resize / 字节计数）
    session: Arc<SshSession>,
    /// 输出端：WS 升级时被 `take_output` 取走，只能取一次
    output: Mutex<Option<mpsc::Receiver<SshEvent>>>,
    /// 本次连接校验通过的主机指纹
    host_key: String,
}

fn hub() -> &'static DashMap<i64, Arc<HubEntry>> {
    static HUB: OnceLock<DashMap<i64, Arc<HubEntry>>> = OnceLock::new();
    HUB.get_or_init(DashMap::new)
}

/// 单用户当前并发会话数
fn session_count_for_user(user_id: i64) -> usize {
    hub()
        .iter()
        .filter(|entry| entry.value().user_id == user_id)
        .count()
}

/// 某个主机上是否存在活动会话
fn has_active_session_for_target(target_id: i64) -> bool {
    hub()
        .iter()
        .any(|entry| entry.value().target_id == target_id)
}

/// 取写连接池
fn write_pool() -> Result<sqlx::PgPool, String> {
    database::get_write_pool().map_err(|err| format!("数据库连接失败: {err}"))
}

/// 读取单台主机（软删视为不存在）
async fn load_target_row(id: i64) -> Result<OpsSshTarget, String> {
    let pool = write_pool()?;
    let p = database::schema_prefix();
    let sql = format!("SELECT {TARGET_COLUMNS} FROM {p}ops_ssh_target WHERE id = $1 AND deleted = 0");
    sqlx::query_as::<_, OpsSshTarget>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(&pool)
        .await
        .map_err(|err| format!("读取 SSH 主机失败: {err}"))?
        .ok_or_else(|| "SSH 主机不存在或已删除".to_string())
}

/// 名称同租户唯一（`deleted = 0`）
async fn ensure_name_unique(
    pool: &sqlx::PgPool,
    name: &str,
    exclude_id: Option<i64>,
) -> Result<(), String> {
    let p = database::schema_prefix();
    let sql = format!(
        "SELECT 1 FROM {p}ops_ssh_target WHERE target_name = $1 AND deleted = 0 \
         AND ($2::BIGINT IS NULL OR id <> $2) LIMIT 1"
    );
    let exists = sqlx::query_scalar::<_, i32>(sqlx::AssertSqlSafe(sql))
        .bind(name)
        .bind(exclude_id)
        .fetch_optional(pool)
        .await
        .map_err(|err| format!("校验主机名称失败: {err}"))?;
    if exists.is_some() {
        return Err(format!("主机名称「{name}」已存在"));
    }
    Ok(())
}

/// 入参自检（不触网、不碰库）
fn validate_target_input(input: &SshTargetInput) -> Result<(), String> {
    if input.name.is_empty() {
        return Err("主机名称不能为空".to_string());
    }
    if input.name.chars().count() > 100 {
        return Err("主机名称不能超过 100 个字符".to_string());
    }
    if input.host.is_empty() {
        return Err("主机地址不能为空".to_string());
    }
    if input.host.chars().any(char::is_whitespace) {
        return Err("主机地址不能包含空白字符".to_string());
    }
    if input.host.contains("://") || input.host.contains('/') {
        return Err("主机地址只填 host 或 IP，不要带 scheme / 路径".to_string());
    }
    if input.username.is_empty() {
        return Err("登录账号不能为空".to_string());
    }
    if input.port.unwrap_or(remote_shell::DEFAULT_PORT) == 0 {
        return Err("端口必须在 1-65535 之间".to_string());
    }
    if let Some(fingerprint) = input.host_key_fingerprint.as_deref() {
        if !fingerprint.starts_with("SHA256:") {
            return Err("主机指纹只支持 SHA256:... 格式（不解析 MD5: 旧格式）".to_string());
        }
    }
    Ok(())
}

/// 连接超时夹取到传输层允许的区间
fn clamp_timeout_secs(value: Option<u32>) -> i32 {
    let raw = i64::from(value.unwrap_or(10));
    let min = i64::try_from(remote_shell::MIN_TIMEOUT_SECS).unwrap_or(3);
    let max = i64::try_from(remote_shell::MAX_TIMEOUT_SECS).unwrap_or(60);
    i32::try_from(raw.clamp(min, max)).unwrap_or(10)
}

/// 是否需要回收（纯函数，便于单测）
#[must_use]
fn should_reap(now_ms: i64, last_active_ms: i64, opened_for_secs: u64, idle_secs: i64) -> bool {
    let idle_ms = idle_secs.saturating_mul(1000);
    let idle_expired = now_ms.saturating_sub(last_active_ms) > idle_ms;
    let lifetime_expired = opened_for_secs > MAX_SESSION_LIFETIME_SECS.unsigned_abs();
    idle_expired || lifetime_expired
}

// ======================== 指纹探测 ========================

/// 用已保存的凭证扫描主机指纹（不认证、不落库）
///
/// 扫描时**不**拿已存指纹做握手校验（否则换过主机密钥就永远扫不出来），
/// 而是返回 `matched` 让用户自己确认是否更新。
///
/// # Errors
///
/// 主机不存在、凭证不可用、目标不可达或握手失败。
pub async fn probe_target(id: i64) -> Result<SshProbeView, String> {
    let row = load_target_row(id).await?;
    let auth = auth_from_row(&row)?;
    let spec = spec_from_row(&row, auth, false)?;
    let started = Instant::now();
    let info = remote_shell::probe(&spec).await.map_err(|err| {
        let message = err.to_string();
        // 探测失败同样进 last_error，方便排查"连不上到底是什么原因"
        record_last_error(id, &message);
        message
    })?;
    let matched = row
        .host_key_fingerprint
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|expected| fingerprint_matches(Some(expected), &info.fingerprint));
    Ok(probe_view(info, matched, started))
}

/// 表单未保存时的指纹探测（凭证只在本次握手中使用，不落库）
///
/// # Errors
///
/// 参数非法、凭证缺失、目标不可达或握手失败。
pub async fn probe_draft(input: &SshProbeInput) -> Result<SshProbeView, String> {
    let host = input.host.trim();
    let username = input.username.trim();
    if host.is_empty() {
        return Err("主机地址不能为空".to_string());
    }
    if username.is_empty() {
        return Err("登录账号不能为空".to_string());
    }
    let port = input.port.unwrap_or(remote_shell::DEFAULT_PORT);
    let auth = draft_auth(input)?;
    let spec = SshTargetSpec {
        host: host.to_string(),
        port,
        username: username.to_string(),
        auth,
        timeout_secs: u64::from(input.connect_timeout_secs.unwrap_or(10)),
        expected_fingerprint: None,
    };
    let started = Instant::now();
    let info = remote_shell::probe(&spec)
        .await
        .map_err(|err| err.to_string())?;
    Ok(probe_view(info, None, started))
}

/// 探测入参 → 认证材料
fn draft_auth(input: &SshProbeInput) -> Result<SshAuth, String> {
    if let Some(password) = input.password.as_deref().filter(|value| !value.is_empty()) {
        return Ok(SshAuth::Password(password.to_string()));
    }
    if let Some(pem) = input
        .private_key_pem
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(SshAuth::PrivateKey {
            pem: pem.to_string(),
            passphrase: input
                .passphrase
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
        });
    }
    Err("请填写密码或私钥后再扫描指纹".to_string())
}

/// 探测结果 → 视图
fn probe_view(info: HostKeyInfo, matched: Option<bool>, started: Instant) -> SshProbeView {
    SshProbeView {
        fingerprint: info.fingerprint,
        key_type: info.key_type,
        matched,
        latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    }
}

/// 记录最近一次失败原因（尽力而为，不覆盖原始错误）
fn record_last_error(target_id: i64, message: &str) {
    let Ok(pool) = database::get_write_pool() else {
        return;
    };
    let p = database::schema_prefix();
    let sql = format!(
        "UPDATE {p}ops_ssh_target SET last_error = $2, updated_at = NOW() \
         WHERE id = $1 AND deleted = 0"
    );
    let message = message.to_string();
    tokio::spawn(async move {
        if let Err(err) = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(target_id)
            .bind(message)
            .execute(&pool)
            .await
        {
            warn!(target_id, error = %err, "写入 SSH 主机 last_error 失败");
        }
    });
}

// ======================== 会话生命周期 ========================

/// 插入审计行，返回会话 id
async fn insert_session_row(
    target_id: i64,
    user_id: i64,
    state: &str,
    client_ip: Option<&str>,
) -> Result<i64, String> {
    let pool = write_pool()?;
    let p = database::schema_prefix();
    let sql = format!(
        "INSERT INTO {p}ops_ssh_session \
         (id, target_id, user_id, state, client_ip, started_at, bytes_in, bytes_out) \
         VALUES ($1, $2, $3, $4, $5, NOW(), 0, 0) RETURNING id"
    );
    sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(sql))
        .bind(next_id() as i64)
        .bind(target_id)
        .bind(user_id)
        .bind(state)
        .bind(client_ip)
        .fetch_one(&pool)
        .await
        .map_err(|err| format!("创建 SSH 会话审计失败: {err}"))
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
        "UPDATE {p}ops_ssh_session SET state = $2, ended_at = NOW(), bytes_in = $3, \
         bytes_out = $4, exit_code = $5, error = $6 WHERE id = $1"
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
        .map_err(|err| format!("更新 SSH 会话审计失败: {err}"))?;
    Ok(())
}

/// 标记会话失败（开会话阶段使用，此时还没有 hub 条目）
async fn mark_session_failed(session_id: i64, message: &str) {
    if let Err(err) = finalize_session_row(session_id, "failed", 0, 0, None, Some(message)).await {
        warn!(session_id, error = %err, "标记 SSH 会话失败状态时出错");
    }
}

/// 审计行 opening → active（`ended_at` 仍为空）
async fn finalize_active_row(session_id: i64) -> Result<(), String> {
    let pool = write_pool()?;
    let p = database::schema_prefix();
    let sql = format!("UPDATE {p}ops_ssh_session SET state = 'active' WHERE id = $1");
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(session_id)
        .execute(&pool)
        .await
        .map_err(|err| format!("更新 SSH 会话状态失败: {err}"))?;
    Ok(())
}

/// 首次纳管记录主机指纹（只填空值，绝不覆盖已固定指纹）
async fn persist_fingerprint(target_id: i64, fingerprint: &str) {
    let Ok(pool) = database::get_write_pool() else {
        return;
    };
    let p = database::schema_prefix();
    let sql = format!(
        "UPDATE {p}ops_ssh_target SET host_key_fingerprint = $2, updated_at = NOW() \
         WHERE id = $1 AND deleted = 0 \
         AND (host_key_fingerprint IS NULL OR host_key_fingerprint = '')"
    );
    if let Err(err) = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(target_id)
        .bind(fingerprint)
        .execute(&pool)
        .await
    {
        warn!(target_id, error = %err, "记录主机指纹失败");
    }
}

/// 刷新最近连接时间并清空上次错误
async fn mark_target_connected(target_id: i64) {
    let Ok(pool) = database::get_write_pool() else {
        return;
    };
    let p = database::schema_prefix();
    let sql = format!(
        "UPDATE {p}ops_ssh_target SET last_connected_at = NOW(), last_error = NULL, \
         updated_at = NOW() WHERE id = $1 AND deleted = 0"
    );
    if let Err(err) = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(target_id)
        .execute(&pool)
        .await
    {
        warn!(target_id, error = %err, "更新主机最近连接时间失败");
    }
}

/// 打开会话：限额校验 → 解密凭证 → 握手认证 → 落审计 → 注册 hub
///
/// 返回会话 id，由 api 层再签发一次性 WS 票据。
///
/// # Errors
///
/// 主机不存在 / 已停用、超出并发上限、凭证不可用、连接或认证失败。
pub async fn open_session(
    user_id: i64,
    target_id: i64,
    cols: u32,
    rows: u32,
    client_ip: Option<String>,
) -> Result<i64, String> {
    let limit = config().max_sessions_per_user;
    let active = session_count_for_user(user_id);
    if active >= limit {
        return Err(format!(
            "当前已有 {active} 个 SSH 会话，已达上限 {limit}，请先关闭不用的会话"
        ));
    }

    let row = load_target_row(target_id).await?;
    if row.enabled == 0 {
        return Err(format!("主机「{}」已停用", row.target_name));
    }
    let strict = row.strict_host_key_check != 0;
    let known_fingerprint = row
        .host_key_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if strict && known_fingerprint.is_none() {
        return Err(format!(
            "主机「{}」未记录主机指纹，请先扫描指纹并确认后再连接",
            row.target_name
        ));
    }

    let auth = auth_from_row(&row)?;
    let spec = spec_from_row(&row, auth, strict && known_fingerprint.is_some())?;
    let session_id = insert_session_row(target_id, user_id, "opening", client_ip.as_deref()).await?;

    let timeout = spec.timeout() + Duration::from_secs(OPEN_TIMEOUT_MARGIN_SECS);
    let opened = tokio::time::timeout(timeout, remote_shell::open(&spec, cols, rows)).await;
    let mut shell = match opened {
        Err(_) => {
            let message = format!("打开 SSH 会话超时（{} 秒）", timeout.as_secs());
            mark_session_failed(session_id, &message).await;
            record_last_error(target_id, &message);
            return Err(message);
        }
        Ok(Err(err)) => {
            let message = err.to_string();
            mark_session_failed(session_id, &message).await;
            record_last_error(target_id, &message);
            return Err(message);
        }
        Ok(Ok(shell)) => shell,
    };

    let Some(output) = shell.take_output() else {
        let message = "会话输出通道初始化失败".to_string();
        let _ = shell.close().await;
        mark_session_failed(session_id, &message).await;
        return Err(message);
    };

    let host_key = shell.host_key().fingerprint.clone();
    // 首次纳管（strict=0 且未记录指纹）：记录本次观察到的指纹，绝不覆盖已固定的指纹
    if !strict && known_fingerprint.is_none() {
        persist_fingerprint(target_id, &host_key).await;
    }

    let entry = HubEntry {
        user_id,
        target_id,
        target_name: row.target_name.clone(),
        schema: database::current_schema_name(),
        opened_at: Instant::now(),
        last_active: AtomicI64::new(now_millis()),
        exit_code: AtomicI64::new(EXIT_UNKNOWN),
        session: Arc::new(shell),
        output: Mutex::new(Some(output)),
        host_key: host_key.clone(),
    };
    hub().insert(session_id, Arc::new(entry));

    if let Err(err) = finalize_active_row(session_id).await {
        warn!(session_id, error = %err, "更新 SSH 会话为 active 失败");
    }
    mark_target_connected(target_id).await;
    info!(
        session_id,
        user_id,
        target_id,
        host_key = %host_key,
        "SSH 会话已打开"
    );
    Ok(session_id)
}

/// 取会话条目（校验归属；非归属者一律按「不存在」处理，避免枚举他人会话）
fn owned_entry(session_id: i64, user_id: i64) -> Result<Arc<HubEntry>, String> {
    let entry = hub()
        .get(&session_id)
        .map(|entry| Arc::clone(entry.value()))
        .ok_or_else(|| "会话不存在".to_string())?;
    if entry.user_id != user_id {
        return Err("会话不存在".to_string());
    }
    Ok(entry)
}

/// 写入键盘输入（刷新活跃时间）
///
/// # Errors
///
/// 会话不存在 / 不属于当前用户 / 已断开。
pub async fn write_input(session_id: i64, user_id: i64, data: Vec<u8>) -> Result<(), String> {
    let entry = owned_entry(session_id, user_id)?;
    entry.last_active.store(now_millis(), Ordering::Relaxed);
    entry
        .session
        .write(data)
        .await
        .map_err(|err| err.to_string())
}

/// 上报窗口大小变化（尽力而为，会话已断开不报错）
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
    entry.session.resize(cols, rows).await;
    Ok(())
}

/// 记录远端退出码（WS 泵收到 `Exit` 事件时调用，供关闭时落审计）
pub fn note_exit(session_id: i64, code: Option<u32>) {
    if let (Some(entry), Some(code)) = (hub().get(&session_id), code) {
        entry.exit_code.store(i64::from(code), Ordering::Relaxed);
    }
}

/// 取会话输出端（WS 升级后调用；只能取一次）
///
/// # Errors
///
/// 会话不存在 / 不属于当前用户 / 输出已被接管。
pub fn attach_output(session_id: i64, user_id: i64) -> Result<mpsc::Receiver<SshEvent>, String> {
    let entry = owned_entry(session_id, user_id)?;
    let taken = entry
        .output
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    taken.ok_or_else(|| "会话输出已被接管".to_string())
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
        // 非归属者：放回 hub 并统一按「不存在」处理
        hub().insert(session_id, entry);
        return Err("会话不存在".to_string());
    }

    let _ = entry.session.close().await;
    let (bytes_in, bytes_out) = entry.session.bytes();
    let raw_exit = entry.exit_code.load(Ordering::Relaxed);
    let exit_code = (raw_exit != EXIT_UNKNOWN)
        .then(|| i32::try_from(raw_exit).ok())
        .flatten();
    let schema = entry.schema.clone();
    let target_id = entry.target_id;
    let target_name = entry.target_name.clone();
    let host_key = entry.host_key.clone();
    let bytes_in = i64::try_from(bytes_in).unwrap_or(i64::MAX);
    let bytes_out = i64::try_from(bytes_out).unwrap_or(i64::MAX);
    let audit = database::with_schema(schema, async move {
        finalize_session_row(
            session_id,
            "closed",
            bytes_in,
            bytes_out,
            exit_code,
            None,
        )
        .await
    })
    .await;
    if let Err(err) = audit {
        warn!(session_id, error = %err, "写 SSH 会话审计失败");
    }
    debug!(
        session_id,
        user_id,
        target_id,
        target_name = %target_name,
        host_key = %host_key,
        reason,
        bytes_in,
        bytes_out,
        "SSH 会话已关闭"
    );
    Ok(())
}

/// 活跃会话列表（管理员 / 审计用）
///
/// # Errors
///
/// 数据库不可用。
pub async fn list_sessions(user_id: i64, include_all: bool) -> Result<Vec<SshSessionView>, String> {
    let pool = write_pool()?;
    let p = database::schema_prefix();
    let sql = format!(
        "SELECT {SESSION_COLUMNS} FROM {p}ops_ssh_session \
         WHERE state IN ('opening', 'active') AND ($1::BIGINT IS NULL OR user_id = $1) \
         ORDER BY started_at DESC"
    );
    let filter = if include_all { None } else { Some(user_id) };
    let rows = sqlx::query_as::<_, OpsSshSession>(sqlx::AssertSqlSafe(sql))
        .bind(filter)
        .fetch_all(&pool)
        .await
        .map_err(|err| format!("读取 SSH 会话列表失败: {err}"))?;

    let names = sqlx::query_as::<_, (i64, String)>(
        "SELECT id, target_name FROM ops_ssh_target WHERE deleted = 0",
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_default();

    Ok(rows
        .iter()
        .map(|row| SshSessionView {
            id: row.id.to_string(),
            target_id: row.target_id.to_string(),
            target_name: names
                .iter()
                .find(|(id, _)| *id == row.target_id)
                .map_or_else(String::new, |(_, name)| name.clone()),
            user_id: row.user_id.to_string(),
            state: row.state.clone(),
            client_ip: row.client_ip.clone(),
            started_at: row.started_at.map(|time| time.to_rfc3339()),
            ended_at: row.ended_at.map(|time| time.to_rfc3339()),
            bytes_in: row.bytes_in,
            bytes_out: row.bytes_out,
            exit_code: row.exit_code,
            error: row.error.clone(),
        })
        .collect())
}

// ======================== 空闲回收 ========================

/// 启动空闲会话回收任务（进程内只启动一次，与 sandbox 的装配方式一致）
pub fn spawn_idle_reaper() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if !remote_shell::ssh_enabled() {
        debug!("SSH 传输未编译进来：跳过会话空闲回收任务");
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
        "SSH 会话空闲回收任务已启动"
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
            Ok(()) => info!(session_id, "SSH 会话已空闲回收"),
            Err(err) => warn!(session_id, error = %err, "SSH 会话空闲回收失败"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_row() -> OpsSshTarget {
        OpsSshTarget {
            id: 42,
            target_name: "web-01".to_string(),
            host: "10.0.0.1".to_string(),
            port: 22,
            username: "root".to_string(),
            auth_type: "password".to_string(),
            auth_config_enc: Some("cipher-text".to_string()),
            credential_hint: Some(credential_hint_for("password")),
            host_key_fingerprint: Some("SHA256:AbC".to_string()),
            strict_host_key_check: 1,
            asset_id: None,
            description: None,
            connect_timeout_secs: 10,
            enabled: 1,
            last_connected_at: None,
            last_error: None,
            created_by: None,
            created_at: None,
            updated_by: None,
            updated_at: None,
            deleted: Some(0),
        }
    }

    #[test]
    fn credential_hint_never_contains_secret() {
        assert_eq!(credential_hint_for("password"), "密码（已配置）");
        assert_eq!(credential_hint_for("private_key"), "私钥（已配置）");
    }

    #[test]
    fn normalize_defaults_port_and_trims() {
        let mut input = SshTargetInput {
            name: "  web-01 ".to_string(),
            host: " 10.0.0.1 ".to_string(),
            username: " root ".to_string(),
            port: None,
            ..SshTargetInput::default()
        };
        normalize_target_input(&mut input);
        assert_eq!(input.name, "web-01");
        assert_eq!(input.host, "10.0.0.1");
        assert_eq!(input.username, "root");
        assert_eq!(input.port, Some(22));
    }

    #[test]
    fn clamp_timeout_bounds() {
        assert_eq!(clamp_timeout_secs(None), 10);
        assert_eq!(clamp_timeout_secs(Some(0)), 3);
        assert_eq!(clamp_timeout_secs(Some(3600)), 60);
        assert_eq!(clamp_timeout_secs(Some(15)), 15);
    }

    #[test]
    fn validate_rejects_bad_input() {
        let mut input = SshTargetInput {
            name: String::new(),
            host: "10.0.0.1".to_string(),
            username: "root".to_string(),
            ..SshTargetInput::default()
        };
        assert!(validate_target_input(&input).is_err());
        input.name = "web-01".to_string();
        assert!(validate_target_input(&input).is_ok());
        input.host = "ssh://10.0.0.1".to_string();
        assert!(validate_target_input(&input).is_err());
        input.host = "10.0.0.1".to_string();
        input.host_key_fingerprint = Some("MD5:aa:bb".to_string());
        assert!(validate_target_input(&input).is_err());
        input.host_key_fingerprint = Some("SHA256:AbC".to_string());
        assert!(validate_target_input(&input).is_ok());
    }

    #[test]
    fn resolve_secret_keeps_existing_when_omitted() {
        let row = sample_row();
        let input = SshTargetInput {
            name: "web-01".to_string(),
            host: "10.0.0.1".to_string(),
            username: "root".to_string(),
            ..SshTargetInput::default()
        };
        let (auth_type, enc, hint) = resolve_secret(&input, Some(&row)).expect("沿用旧凭证");
        assert_eq!(auth_type, "password");
        assert_eq!(enc, "cipher-text");
        assert_eq!(hint, "密码（已配置）");
    }

    #[test]
    fn resolve_secret_rejects_new_target_without_secret() {
        let input = SshTargetInput {
            name: "web-01".to_string(),
            host: "10.0.0.1".to_string(),
            username: "root".to_string(),
            ..SshTargetInput::default()
        };
        assert!(resolve_secret(&input, None).is_err());
    }

    #[test]
    fn resolve_secret_rejects_mismatched_auth_type() {
        let input = SshTargetInput {
            name: "web-01".to_string(),
            host: "10.0.0.1".to_string(),
            username: "root".to_string(),
            auth_type: Some("private_key".to_string()),
            password: Some("p@ss".to_string()),
            ..SshTargetInput::default()
        };
        let error = resolve_secret(&input, None).unwrap_err();
        assert!(error.contains("private_key"), "{error}");
    }

    #[test]
    fn should_reap_rules() {
        // 空闲阈值 60 秒：59 秒内不回收，61 秒回收
        assert!(!should_reap(1_059_000, 1_000_000, 10, 60));
        assert!(should_reap(1_061_000, 1_000_000, 10, 60));
        // 存活超过 24 小时强制回收
        let lifetime = u64::try_from(MAX_SESSION_LIFETIME_SECS).unwrap_or(86_400);
        assert!(should_reap(1_000_000, 1_000_000, lifetime + 1, 86_400));
    }

    #[test]
    fn target_view_hides_cipher() {
        let view = target_to_view(&sample_row());
        assert!(view.has_credential);
        let json = serde_json::to_value(&view).expect("序列化视图");
        let object = json.as_object().expect("视图是对象");
        assert!(!object.contains_key("auth_config_enc"));
        assert!(!object.contains_key("password"));
        assert!(!object.contains_key("private_key_pem"));
    }
}




// ======================== 主机 CRUD ========================

/// 主机列表（按名称排序，不含凭证）
///
/// # Errors
///
/// 数据库不可用时返回错误文本。
pub async fn list_targets() -> Result<Vec<SshTargetView>, String> {
    let pool = write_pool()?;
    let p = database::schema_prefix();
    let sql = format!(
        "SELECT {TARGET_COLUMNS} FROM {p}ops_ssh_target WHERE deleted = 0 ORDER BY target_name"
    );
    let rows = sqlx::query_as::<_, OpsSshTarget>(sqlx::AssertSqlSafe(sql))
        .fetch_all(&pool)
        .await
        .map_err(|err| format!("读取 SSH 主机列表失败: {err}"))?;
    Ok(rows.iter().map(target_to_view).collect())
}

/// 主机详情（不含凭证）
///
/// # Errors
///
/// 主机不存在 / 已删除，或数据库不可用。
pub async fn get_target(id: i64) -> Result<SshTargetView, String> {
    let row = load_target_row(id).await?;
    Ok(target_to_view(&row))
}

/// 新建主机
///
/// # Errors
///
/// 参数非法、名称重复、缺少加密密钥或数据库不可用。
pub async fn create_target(
    input: &SshTargetInput,
    user_id: Option<i64>,
) -> Result<SshTargetView, String> {
    let mut input = input.clone();
    normalize_target_input(&mut input);
    validate_target_input(&input)?;
    let (auth_type, auth_config_enc, credential_hint) = resolve_secret(&input, None)?;

    let pool = write_pool()?;
    ensure_name_unique(&pool, &input.name, None).await?;

    let p = database::schema_prefix();
    let sql = format!(
        "INSERT INTO {p}ops_ssh_target \
         (id, target_name, host, port, username, auth_type, auth_config_enc, credential_hint, \
          host_key_fingerprint, strict_host_key_check, asset_id, description, \
          connect_timeout_secs, enabled, created_by, created_at, updated_at, deleted) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, NOW(), NOW(), 0) \
         RETURNING {TARGET_COLUMNS}"
    );
    let row = sqlx::query_as::<_, OpsSshTarget>(sqlx::AssertSqlSafe(sql))
        .bind(next_id() as i64)
        .bind(&input.name)
        .bind(&input.host)
        .bind(i32::from(input.port.unwrap_or(remote_shell::DEFAULT_PORT)))
        .bind(&input.username)
        .bind(&auth_type)
        .bind(&auth_config_enc)
        .bind(&credential_hint)
        .bind(input.host_key_fingerprint.as_deref())
        .bind(if input.strict_host_key_check.unwrap_or(true) {
            1_i16
        } else {
            0_i16
        })
        .bind(input.asset_id)
        .bind(input.description.as_deref().map(str::trim))
        .bind(clamp_timeout_secs(input.connect_timeout_secs))
        .bind(if input.enabled.unwrap_or(true) {
            1_i16
        } else {
            0_i16
        })
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .map_err(|err| format!("创建 SSH 主机失败: {err}"))?;
    info!(id = row.id, name = %row.target_name, "创建 SSH 主机成功");
    Ok(target_to_view(&row))
}

/// 编辑主机（凭证字段缺省 = 保留原凭证）
///
/// # Errors
///
/// 主机不存在、名称重复、参数非法或数据库不可用。
pub async fn update_target(
    id: i64,
    input: &SshTargetInput,
    user_id: Option<i64>,
) -> Result<SshTargetView, String> {
    let existing = load_target_row(id).await?;
    let mut input = input.clone();
    normalize_target_input(&mut input);
    validate_target_input(&input)?;
    let (auth_type, auth_config_enc, credential_hint) = resolve_secret(&input, Some(&existing))?;

    let pool = write_pool()?;
    ensure_name_unique(&pool, &input.name, Some(id)).await?;

    let p = database::schema_prefix();
    let sql = format!(
        "UPDATE {p}ops_ssh_target SET target_name = $2, host = $3, port = $4, username = $5, \
         auth_type = $6, auth_config_enc = $7, credential_hint = $8, host_key_fingerprint = $9, \
         strict_host_key_check = $10, asset_id = $11, description = $12, \
         connect_timeout_secs = $13, enabled = $14, updated_by = $15, updated_at = NOW() \
         WHERE id = $1 AND deleted = 0 RETURNING {TARGET_COLUMNS}"
    );
    let row = sqlx::query_as::<_, OpsSshTarget>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(&input.name)
        .bind(&input.host)
        .bind(i32::from(input.port.unwrap_or(remote_shell::DEFAULT_PORT)))
        .bind(&input.username)
        .bind(&auth_type)
        .bind(&auth_config_enc)
        .bind(&credential_hint)
        .bind(input.host_key_fingerprint.as_deref())
        .bind(if input.strict_host_key_check.unwrap_or(true) {
            1_i16
        } else {
            0_i16
        })
        .bind(input.asset_id)
        .bind(input.description.as_deref().map(str::trim))
        .bind(clamp_timeout_secs(input.connect_timeout_secs))
        .bind(if input.enabled.unwrap_or(true) {
            1_i16
        } else {
            0_i16
        })
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .map_err(|err| format!("更新 SSH 主机失败: {err}"))?;
    info!(id = row.id, name = %row.target_name, "更新 SSH 主机成功");
    Ok(target_to_view(&row))
}

/// 删除主机（软删；存在活动会话时拒绝）
///
/// # Errors
///
/// 主机不存在、存在活动会话或数据库不可用。
pub async fn delete_target(id: i64) -> Result<(), String> {
    if has_active_session_for_target(id) {
        return Err("该主机存在活动会话，请先关闭会话再删除".to_string());
    }
    let pool = write_pool()?;
    let p = database::schema_prefix();
    let sql =
        format!("UPDATE {p}ops_ssh_target SET deleted = 1, updated_at = NOW() WHERE id = $1 AND deleted = 0");
    let affected = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|err| format!("删除 SSH 主机失败: {err}"))?
        .rows_affected();
    if affected == 0 {
        return Err("SSH 主机不存在或已删除".to_string());
    }
    info!(id, "删除 SSH 主机成功");
    Ok(())
}



