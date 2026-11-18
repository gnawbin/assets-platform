# SSH 终端（Web Terminal）实施方案

> **关联文档**：`../运维采集模块/运维采集模块实施方案.md`（下称“ops 方案”，本方案沿用其分层、加密与权限口径）、`../桌面与Web全HTTP改造清单.md`、`../API-Swagger规范.md`
> **落地形态**：在现有 `assets-platform` 上增量扩展；**不改** `ops_connection` / `assets` / `asset_category` 任何列
> **状态**：方案已定稿（待实施） · **更新日期**：2026-11-17

---

## 0. 文档定位与本期边界

### 0.1 本期做什么

1. **新增交互式运维通道**：把 `/ops` 前端与后端能力打通，网页里直接开 SSH 会话（PTY + 交互输入 + 窗口 resize），输出与输入走全双工 WebSocket。
2. **主机配置中心**：新增 `ops_ssh_target`（主机 / 端口 / 账号 / 认证方式 / 主机指纹 / 可选资产绑定），密码与私钥只以 AES-256-GCM 密文落库。
3. **会话审计**：新增 `ops_ssh_session`，记录谁、何时、连了哪台、传输字节数、退出码、失败原因。
4. **权限与菜单**：新增 `ssh:*` 权限点、`sys_menu` 菜单 62（`/ops/terminal`）与 6 个按钮行；权限映射写入 `authz::ROUTE_PERMS`。
5. **前端页面**：`/ops/terminal`（左侧主机列表 + 右侧 xterm.js 终端），支持 `?target=<id>` 直接进终端。

### 0.2 本期不做什么

| 不做 | 归属 |
|---|---|
| ssh-agent / 跳板机 ProxyJump / 证书签名登录 | P2（本期只做 `password` 与 `private_key`） |
| SFTP / 文件上传下载 | P2 |
| 命令级录制与回放（asciinema 式） | P2（本期只有字节数与会话元数据审计） |
| 会话共享 / 协同 / 只读旁观 | 不做规划 |
| 网关多实例下的跨实例会话迁移 | 不做（见 §10.1，明确为单实例 / sticky 约束） |
| 在资产台账里新增“管理地址”列并一键登录 | 后续迭代（本期只做 `ops_ssh_target.asset_id` 软引用） |
| 桌面端（Tauri）原生终端窗口 | 不做（前端为纯 HTTP 单通道，见 §1.3） |

### 0.3 验收口径（DoD）

- 后端：**10 条 REST + 1 条 WebSocket** 路由全部落地，命名与 `authz::ROUTE_PERMS`、`sys_menu`、`httpAdapter` 三处逐字对齐。
- 权限：权限点为 3 段式 `ssh:target:*` / `ssh:session:*`（符合「模块:资源[:动作]」约定）；存量库里历史 4 段 `ops:ssh:*` 由 `public_initial_data.sql` 末尾的幂等 `UPDATE` 就地改写。
- 数据：`ops_ssh_target` / `ops_ssh_session` 随 `init_tenant_tables`（新租户）与 `init_postgres_database`（存量租户）自动创建。
- 安全：凭证明文 / 密文**永不出现在任何响应、日志、`Debug` 输出**；主机指纹不匹配时**连接直接失败**（不允许静默接受）。
- 前端：`/ops/terminal` 可用，支持新建 / 编辑 / 删除 / 扫描指纹 / 连接 / 关闭会话 / 窗口 resize；`tsc --noEmit` 0 错误。
- 验证：`cargo test -p remote-shell`、`cargo test -p service ssh`、`cargo test -p api ssh`、`cargo test -p api authz`、`cargo test -p apidoc`、`pnpm --filter web exec tsc --noEmit` 全绿。

---

## 1. 现状核查结论（实施前必须知道的硬事实）

### 1.1 已有能力：ops 模块给了现成的“模板”，但**没有任何 SSH 能力**

| 能力 | 现状 | 证据 |
|---|---|---|
| 连接配置 CRUD + 凭证加密 | 已具备 | `service/src/ops_service.rs`（`encrypt_auth()` = `utils::crypto::encrypt_api_key(serde_json)`） |
| REST + 权限 + 菜单 | 已具备 | `api/src/ops_routes.rs`、`authz::ROUTE_PERMS`、`public_initial_data.sql` §11（菜单 7/60/61、按钮 160–165） |
| 流式通道 | 已有 SSE（非 WS） | `gateway/src/sse.rs`、`api/src/chat_routes.rs`、`apps/web/src/hooks/useChatStream.ts` |
| SSH / 堡垒机 | **完全没有** | `crates/ops` 只有 Docker / Swarm / K8s 只读采集；全仓无 `russh` / `ssh2` / `openssh` 依赖 |
| WebSocket | **未启用** | 根 `Cargo.toml`：`axum = { version = "0.8.9", features = ["macros"] }` |
| 终端组件 | **缺失** | `apps/web/package.json` 无 `@xterm/xterm` |

### 1.2 依赖可行性（本次实机核查，非推测）

| 依赖 | 结论 | 依据 |
|---|---|---|
| `russh` | **可用**，最新 `0.61.2` / `0.62.0` | crates.io sparse index `https://index.crates.io/ru/ss/russh` 成功拉取（约 85s，慢但可达） |
| `russh` 后端 | 选 `ring` | 索引中 `ring` 与 `aws-lc-rs` 均为**可选依赖**；`ring 0.17.14` / `aws-lc-rs 1.17.0` / `aws-lc-sys 0.41.0` / `cmake 0.1.58` 均已在 `~/.cargo/registry` 命中；选 `ring` 可避开 `aws-lc-sys` 的 C / cmake 构建 |
| `axum` 的 `ws` feature | **可用** | 其依赖 `tokio-tungstenite 0.28.0`、`tungstenite 0.28.0` 已在本地 registry 缓存 |
| `@xterm/xterm` | 需新增 | `package.json` 无；`registry.npmmirror.com` 可用，首次安装需网络 |
| Rust 工具链 | 满足 | `rustc 1.99.1` / `cargo 1.99.1`（edition 2024） |

> 结论：**纯 Rust 方案（russh `ring` 后端 + axum `ws`）具备一次编译通过的条件**；首次 `cargo fetch` 可能耗时数分钟，属正常。

### 1.3 三条硬约束（决定了本方案的形态）

1. **前端是纯 HTTP 单通道**（`apps/web/src/utils/adapters/index.ts` 恒为 `'http'`，Tauri IPC 已下线）。→ 终端必须走 HTTP 服务体系，**不新增 Tauri 命令**，`invoke_handler` 不动。
2. **浏览器 WebSocket 不能带 `Authorization` 头**。→ 必须“先 Bearer 换票据，再带票据升级 WS”，票据一次性、短 TTL、绑定会话。
3. **凭证加密密钥全仓唯一**：`LLM_KEY_ENCRYPT_KEY` 前 32 字节（AES-256-GCM，`base64(nonce + ciphertext)`）。→ SSH 密码 / 私钥复用同一函数（`utils::crypto::encrypt_api_key` / `decrypt_api_key`），不引入第二套密钥体系；该变量缺失时**建主机直接报错**，不允许明文兜底。

---

## 2. 关键设计决策（含权衡）

### D1 数据模型：新建 `ops_ssh_target` / `ops_ssh_session`，**不复用 `ops_connection`**

`ops_connection` 的语义是“只读采集连接”：`ops::Provider` 枚举、`service::provider_options()`（前端“采集来源”下拉的唯一真源）、`allowed_auth_types()`、`AuthKind`（`mutual_tls` / `kubeconfig_*` / `direct`）、采集调度全部围绕采集。若把 `ssh` 塞进 `Provider`：

- 前端“采集来源”下拉会凭空多一项，`allowed_auth_types()` 还要为一堆与 SSH 无关的认证方式兜底；
- `Provider::is_data_store()` 等既有分支与单测需要连带修改（回归面扩大）；
- 生命周期不同：采集是“配置 → 定期拉取”，SSH 是“配置 → 实时会话（PTY / 输入 / 审计）”

| 方案 | 改动面 | 风险 | 结论 |
|---|---|---|---|
| 复用 `ops_connection`（provider=ssh） | 需改 `Provider`、`provider_options`、`AuthKind`、菜单与表单联动 | 采集链路回归，用户可在采集下拉选到 ssh | ✗ |
| 新建两表 | 新增迁移 + 3 层代码，采集链路零改动 | 多一套 CRUD 与页面 | ✓ 采用 |

### D2 传输：**WebSocket（axum `ws`）+ 一次性票据**，SSE+POST 作为降级路径

| 维度 | WebSocket（采用） | SSE + POST 输入（备选） |
|---|---|---|
| 方向 | 全双工，一连接 | 半双工：输出 SSE、输入逐键 POST |
| xterm 契合度 | 原生（二进制帧 = PTY 原始字节） | 需文本编码（base64 或 UTF-8），非 UTF-8 输出会损 |
| 鉴权 | 浏览器 WS 不能带头 → 需一次性票据 | 直接复用 Bearer |
| 延迟 | 局域网近乎即时 | 每键一次 RTT（内网可接受） |
| 改动 | 根 `Cargo.toml` 给 axum 加 `ws`（依赖已缓存） | 零新特性 |
| 结论 | **采用** | 保留为“禁止改 axum feature”时的降级方案 |

票据规则：`POST /api/ops/ssh/sessions`（Bearer）→ `ticket`（`uuid v4`，122 bit 随机）；**TTL 60s、单次消费、绑定 `session_id`**；`GET .../ws?ticket=...` 校验通过即从 `DashMap` 中移除。不在 URL 放 JWT（避免日志 / 历史泄露）。

### D3 SSH 实现：新 crate `crates/remote-shell`，feature `ssh` = `russh`（`ring` 后端）

| 方案 | 优点 | 缺点 | 结论 |
|---|---|---|---|
| `russh`（纯 Rust） | 进程内、无外部二进制、PTY / resize / exit status 全可控、Windows 桌面也能用 | 依赖树较大（首次编译慢） | **采用** |
| `ssh2`（libssh2） | 成熟 | 需要 vendored C 编译 | 备选 |
| 调用系统 `ssh` | 零新 crate | Unix 要 `nix` 做 PTY（Windows 不可用）；密码需 prompt 嗅探，脆弱 | 兜底 |

与 `crates/ops` 一致：**feature 门控 + 默认关闭**，在 `service/Cargo.toml` 里一次性打开（`remote-shell = { workspace = true, features = ["ssh"] }`），api / tauri 层共享。

### D4 凭据与主机指纹：TOFU + 显式确认，**绝不允许静默接受**

- `auth_type ∈ {password, private_key}`（`agent` 留 P2）；密文列 `auth_config_enc` 存 `{"password"}` 或 `{"private_key_pem", "passphrase"}` 的 AES-GCM 密文。
- `credential_hint` 只存“密码（已配置）”“私钥 SHA256:ab12…（已配置）”这类提示，**绝不含明文**。
- 指纹：用户在表单点“扫描主机指纹”→ `POST /targets/test` 只握手不认证，返回 `SHA256:...` → 用户确认后随保存写入 `host_key_fingerprint`。
- 连接时 `strict_host_key_check=1` 且已存指纹 → 必须逐字匹配，否则连接失败并在 `last_error` 记录；`strict_host_key_check=0` 仅允许“指纹为空 + 首次连接记录”，用于内网一次性纳管；两种路径都不做“记住新指纹”的静默覆盖。

### D5 会话状态：进程内 `DashMap` + DB 审计行（单实例约束显式化）

- 真正的 TCP 连接只存在于持有它的进程内 → 会话表 `ops_ssh_session` 是**审计事实**，不是“可被任意实例接管”的状态。
- 会话条目内保存 `schema`（开会话时 `database::current_schema_name()` 的快照），WS 断线 / 空闲回收 / 强制关闭时用 `database::with_schema(schema, ...)` 还原租户上下文后再写审计，避免 WS 路由（无 auth 中间件）写错 schema。

---

## 3. 数据模型

新增文件 `apps/backend/src-tauri/crates/database/src/sql/terminal_module_migration.sql`，风格与 `ops_module_migration.sql` 一致：`{schema}` 占位、`IF NOT EXISTS`、`COMMENT ON`、`deleted SMALLINT`、主键由应用层 Snowflake 生成（`utils::snowflake::next_id`，不使用 `BIGSERIAL`）。

```sql
CREATE TABLE IF NOT EXISTS {schema}.ops_ssh_target (
    id BIGINT PRIMARY KEY,
    target_name VARCHAR(100) NOT NULL,
    host VARCHAR(255) NOT NULL,
    port INTEGER NOT NULL DEFAULT 22,
    username VARCHAR(128) NOT NULL,
    auth_type VARCHAR(32) NOT NULL,              -- password / private_key
    auth_config_enc TEXT,                        -- base64(nonce + ciphertext)，只进不出
    credential_hint VARCHAR(128),
    host_key_fingerprint VARCHAR(128),           -- "SHA256:xxxx"（TOFU 固定）
    strict_host_key_check SMALLINT NOT NULL DEFAULT 1,
    asset_id BIGINT,                             -- 软引用资产台账，未绑定为 NULL
    description TEXT,
    connect_timeout_secs INTEGER NOT NULL DEFAULT 10,
    enabled SMALLINT NOT NULL DEFAULT 1,
    last_connected_at TIMESTAMPTZ,
    last_error TEXT,
    created_by BIGINT, created_at TIMESTAMPTZ, updated_by BIGINT, updated_at TIMESTAMPTZ,
    deleted SMALLINT NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX IF NOT EXISTS uk_ops_ssh_target_name
    ON {schema}.ops_ssh_target (target_name) WHERE deleted = 0;
CREATE INDEX IF NOT EXISTS idx_ops_ssh_target_asset
    ON {schema}.ops_ssh_target (asset_id) WHERE deleted = 0;

CREATE TABLE IF NOT EXISTS {schema}.ops_ssh_session (
    id BIGINT PRIMARY KEY,
    target_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    state VARCHAR(16) NOT NULL,                  -- opening / active / closed / failed
    client_ip VARCHAR(64),
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ended_at TIMESTAMPTZ,
    bytes_in BIGINT NOT NULL DEFAULT 0,          -- 客户端 → 目标主机
    bytes_out BIGINT NOT NULL DEFAULT 0,         -- 目标主机 → 客户端
    exit_code INTEGER,
    error TEXT
);
CREATE INDEX IF NOT EXISTS idx_ops_ssh_session_user
    ON {schema}.ops_ssh_session (user_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_ops_ssh_session_target
    ON {schema}.ops_ssh_session (target_id, started_at DESC);
```

> **不加物理外键**（与 `asset_ops_link.asset_id` 一致）：`asset_id` / `target_id` 的合法性由 service 层显式校验，避免租户 schema 重建与软删带来的外键阻塞。

`models.rs` 同步新增 `OpsSshTarget`、`OpsSshSession`：`auth_config_enc` 加 `#[serde(skip)]`；所有 `BIGINT` 字段按仓库约定使用 `#[serde(serialize_with = "i64_to_string", deserialize_with = "i64_from_string")]`（前端大整数精度）。

---

## 4. 后端实现

### 4.1 分层与文件清单

| 层 | 文件 | 变更 |
|---|---|---|
| 依赖 | `apps/backend/src-tauri/Cargo.toml` | `axum` 加 `ws`；新增 `russh = { version = "0.61", default-features = false, features = ["ring"] }` 与 `remote-shell = { path = "crates/remote-shell" }` |
| 传输 crate | `crates/remote-shell/{Cargo.toml, src/lib.rs}` | 新建（`default = []`，`ssh = ["dep:russh"]`） |
| 数据 | `crates/database/src/sql/terminal_module_migration.sql` | 新建（§3） |
| 数据 | `crates/database/src/models.rs` | `OpsSshTarget` / `OpsSshSession` |
| 数据 | `crates/database/src/postgres.rs` | `migrate_tenant_terminal_tables` / `migrate_all_tenant_terminal_tables`，挂到 `init_tenant_tables`（第 781 行后）与 `init_postgres_database` 的 failures 汇总（第 1141 行后） |
| 服务 | `crates/service/src/ssh_service.rs` | 新建（CRUD + 探测 + 会话 hub + 回收） |
| 服务 | `crates/service/src/lib.rs`、`crates/service/Cargo.toml` | `pub mod ssh_service;`、`remote-shell = { workspace = true, features = ["ssh"] }` |
| API | `crates/api/src/ssh_ticket.rs` | 新建（一次性票据） |
| API | `crates/api/src/ssh_routes.rs` | 新建（10 REST + 1 WS） |
| API | `crates/api/src/lib.rs` | 挂 REST 路由 + `merge` WS 子 router + `spawn_idle_reaper()` |
| API | `crates/api/src/authz.rs` | `ROUTE_PERMS` 11 条 + `SERVICE_FORBIDDEN_ROUTES` 6 条 |
| API | `crates/api/Cargo.toml` | `uuid.workspace = true`（票据） |
| 文档 | `crates/apidoc/src/openapi.rs` | 登记 10 条 REST + DTO schema（WS 不登记） |

### 4.2 `crates/remote-shell` 对外契约

```rust
pub enum SshAuth { Password(String), PrivateKey { pem: String, passphrase: Option<String> } } // 手写 Debug 掩码

pub struct SshTargetSpec {
    pub host: String, pub port: u16, pub username: String, pub auth: SshAuth,
    pub timeout_secs: u64,
    pub expected_fingerprint: Option<String>,   // None = 仅扫描指纹
}

pub struct HostKeyInfo { pub fingerprint: String, pub key_type: String }

pub enum SshEvent { Data(Vec<u8>), Exit(Option<u32>), Closed(String) }

/// 只握手 + 取指纹（不认证、不起 shell）
pub async fn probe(spec: &SshTargetSpec) -> Result<HostKeyInfo, SshError>;

/// 认证 + 请求 PTY + shell
pub async fn open(spec: &SshTargetSpec, cols: u32, rows: u32) -> Result<SshSession, SshError>;

impl SshSession {
    pub fn host_key(&self) -> &HostKeyInfo;
    pub async fn write(&self, data: &[u8]) -> Result<(), SshError>;
    pub async fn resize(&self, cols: u32, rows: u32) -> Result<(), SshError>;
    pub async fn close(self);
    pub fn take_output(&mut self) -> mpsc::Receiver<SshEvent>;
}
```

实现要点：

- `russh::client::connect(config, (host.as_str(), port), Handler)`；`config.keepalive_interval = Some(30s)`。
- `Handler::check_server_key`：算 `SHA256:<base64>` 指纹 → `expected_fingerprint == None` 记录并通过；`Some(exp)` 严格比对，不等返回 `Ok(false)`（握手失败，**不静默接受**）。
- 会话建立：`channel_open_session()` → `request_pty("xterm-256color", cols, rows, 0, 0, &[])` → `request_shell(true)`；读循环把 `ChannelMsg::Data / ExtendedData` 转 `SshEvent::Data`，`ExitStatus` → `Exit`，`Close / Eof` → `Closed`。
- 单测（无网络）：`SshAuth` 的 `Debug` 掩码、`SshTargetSpec::validate()`（空 host / `port == 0` / 超时下限夹取）、指纹串归一化（`sha256:` → `SHA256:`）。

### 4.3 `service::ssh_service`（会话 hub）

```rust
struct HubEntry {
    user_id: i64, target_id: i64, target_name: String,
    schema: String,                        // 开会话时的 database::current_schema_name()
    opened_at: Instant, last_active: AtomicI64,
    bytes_in: AtomicU64, bytes_out: AtomicU64,
    input_tx: mpsc::Sender<SessionCmd>,    // Data(Vec<u8>) | Resize(u32,u32) | Close
    host_key: String,
}
static HUB: OnceLock<DashMap<i64, HubEntry>> = OnceLock::new();
```

对外入口（与页面按钮一一对应）：

| 函数 | 说明 |
|---|---|
| `list_targets` / `get_target` | 视图转换，回 `auth_type` + `has_credential` + `credential_hint`，**不含密文/明文** |
| `create_target` / `update_target` / `delete_target` | 名称同租户唯一（`deleted = 0`）；`update` 时认证字段全缺省 = 保留原凭证；`delete` 为软删，**存在活动会话时拒绝**（400） |
| `probe_target(id)` / `probe_draft(input)` | 只握手取指纹；`probe_draft` 供表单未保存时使用（凭证可来自表单） |
| `open_session(user_id, target_id, cols, rows)` | 校验 `enabled`、`SSH_MAX_SESSIONS_PER_USER`（默认 3）→ 解密凭证 → `remote_shell::open`（外层 `tokio::time::timeout`）→ INSERT `ops_ssh_session(state='active')` → 注册 HUB → 更新 `last_connected_at` |
| `write_input` / `resize_session` | 仅允许会话归属者；刷新 `last_active` 与字节计数 |
| `close_session(session_id, user_id, reason)` | 幂等：已不存在视为成功（WS 断线 + 前端 `DELETE` 双触发不报错） |
| `attach_output(session_id, user_id)` | WS 升级后取 `mpsc::Receiver<SshEvent>` |
| `spawn_idle_reaper()` | 60s 一轮：`last_active` 超 `SSH_SESSION_IDLE_SECS`（默认 1800）或存活超 24h 强制关闭 |

要点：

- 审计写入统一走 `database::with_schema(entry.schema.clone(), async { ... })`（WS 路由无 auth 中间件，**不能依赖 task-local 的 CURRENT_SCHEMA**）。
- 会话生命周期：`active` → 远端退出 / 客户端断开 / 空闲回收 → `closed`（含 `exit_code`）或 `failed`（含 `error`）。
- `spawn_idle_reaper()` 在 `start_http_server_with` 内调用一次（与 `service::sandbox_service::spawn_idle_reaper()` 并列）。
- 单测（无需真 SSH）：`credential_hint_for`、`normalize_host_input`（trim / 默认 22 / 非法值拒绝）、`has_credential`（永不回密文）、`should_reap(now, last_active, idle)` 纯函数。

### 4.4 API 层

**票据**（`ssh_ticket.rs`）：

```rust
pub struct TicketStore { inner: DashMap<String, Ticket> }
struct Ticket { user_id: i64, session_id: i64, expires_at: Instant }
impl TicketStore {
    pub fn issue(&self, user_id: i64, session_id: i64) -> String;              // uuid v4，TTL 60s，顺带清理过期项
    pub fn consume(&self, ticket: &str, session_id: i64) -> Option<i64>;        // 单次消费；session 不匹配 → None
}
```

**WS handler 骨架**：

```rust
let user_id = st.tickets.consume(&q.ticket, sid).ok_or_else(|| ApiError::unauthorized("会话票据无效或已过期"))?;
let rx = ssh_service::attach_output(sid, user_id).await.map_err(ApiError::not_found)?;
Ok(ws.on_upgrade(move |socket| pump(socket, sid, user_id, rx)))

// pump：select! 双向转发
//   SshEvent::Data(b)      → Message::Binary(b.into())
//   SshEvent::Exit(code)   → Message::Text({"type":"exit","code":code})
//   Message::Binary(b)     → ssh_service::write_input(...)
//   Message::Text(json)    → {"type":"resize","cols":n,"rows":m}
// 结束：close_session(sid, uid, "client disconnected")（幂等）
```

**装配**（`api/src/lib.rs`）：REST 并入 `create_router(pool)` 的 ops 路由块；WS 单独 `Router::new().route(...).with_state(ssh_state)` 后 `app.merge(...)`（与 upload / chat 的“独立 State、独立挂载”做法一致，WS 路由**不套** `auth_middleware`，靠票据）。

**`authz::ROUTE_PERMS`**（11 条）：

| method | path | perm |
|---|---|---|
| GET | `/api/ops/ssh/targets` | `ssh:target:list` |
| GET | `/api/ops/ssh/targets/{id}` | `ssh:target:list` |
| POST | `/api/ops/ssh/targets` | `ssh:target:create` |
| POST | `/api/ops/ssh/targets/test` | `ssh:target:test` |
| PUT | `/api/ops/ssh/targets/{id}` | `ssh:target:update` |
| DELETE | `/api/ops/ssh/targets/{id}` | `ssh:target:delete` |
| POST | `/api/ops/ssh/targets/{id}/probe` | `ssh:target:test` |
| GET | `/api/ops/ssh/sessions` | `ssh:session:list` |
| POST | `/api/ops/ssh/sessions` | `ssh:session:open` |
| DELETE | `/api/ops/ssh/sessions/{sessionId}` | `ssh:session:close` |
| GET | `/api/ops/ssh/sessions/{sessionId}/ws` | `ssh:session:attach` |

> 权限点命名遵守既有约定「模块:资源[:动作]」（`authz::tests::route_perms_entries_are_well_formed` 强校验 2–3 段、仅小写字母/数字/冒号），
> 因此 SSH 作为独立模块 `ssh`，而不是 `ops:ssh:*`（4 段会被该用例判为非法）。

**`SERVICE_FORBIDDEN_ROUTES`**（服务身份一律 403，OAuth / API Key 没有 `UserContext`）：
`POST /api/ops/ssh/targets`、`PUT /api/ops/ssh/targets/{id}`、`DELETE /api/ops/ssh/targets/{id}`、`POST /api/ops/ssh/sessions`、`DELETE /api/ops/ssh/sessions/{sessionId}`、`GET /api/ops/ssh/sessions/{sessionId}/ws`。

> 对应 handler 一律使用 `Extension<UserContext>`（**强制真人**）；`created_by / updated_by / user_id` 只取令牌 `sub`，body 传入的用户字段不采信。

---

## 5. API 契约

### 5.1 路由一览

| 方法 | 路径 | 权限点 | 说明 |
|---|---|---|---|
| GET | `/api/ops/ssh/targets` | `ssh:target:list` | 主机列表（无密文） |
| POST | `/api/ops/ssh/targets` | `ssh:target:create` | 新建主机 |
| POST | `/api/ops/ssh/targets/test` | `ssh:target:test` | 表单未保存时扫描指纹 |
| GET | `/api/ops/ssh/targets/{id}` | `ssh:target:list` | 主机详情 |
| PUT | `/api/ops/ssh/targets/{id}` | `ssh:target:update` | 编辑（认证字段缺省 = 保留原凭证） |
| DELETE | `/api/ops/ssh/targets/{id}` | `ssh:target:delete` | 软删（有活动会话 → 400） |
| POST | `/api/ops/ssh/targets/{id}/probe` | `ssh:target:test` | 用已保存凭证扫描指纹 + 认证预检 |
| GET | `/api/ops/ssh/sessions` | `ssh:session:list` | 活动会话（管理员/审计用） |
| POST | `/api/ops/ssh/sessions` | `ssh:session:open` | 开会话，返回 `sessionId + ticket` |
| DELETE | `/api/ops/ssh/sessions/{sessionId}` | `ssh:session:close` | 主动关闭（幂等） |
| GET(WS) | `/api/ops/ssh/sessions/{sessionId}/ws?ticket=...` | `ssh:session:attach` | 全双工终端通道 |

`POST /api/ops/ssh/sessions` 请求 / 响应：

```json
// 请求
{ "targetId": "1843000000000000123", "cols": 120, "rows": 32 }
// 响应 data
{ "sessionId": "1843000000000000456", "ticket": "9f1c...-...", "expiresIn": 60 }
```

所有 ID 出参一律**字符串**（Snowflake 精度）；入参 `targetId` 允许字符串或数字（`i64_from_string`）。

### 5.2 WebSocket 帧协议

| 方向 | 帧类型 | 载荷 | 语义 |
|---|---|---|---|
| C → S | Binary | 原始字节 | 键盘输入，直接写入 PTY |
| C → S | Text | `{"type":"resize","cols":120,"rows":32}` | 窗口变化（`window_change`） |
| C → S | Text | `{"type":"ping"}` | 保活（服务端回 `{"type":"pong"}`） |
| S → C | Binary | 原始字节 | PTY 输出（含 ANSI 转义序列，xterm 直接 `write`） |
| S → C | Text | `{"type":"exit","code":0}` | 远端 shell 退出 |
| S → C | Text | `{"type":"closed","reason":"..."}` | 服务端主动关闭（随后关闭 socket） |

### 5.3 错误口径

| 场景 | HTTP | 说明 |
|---|---|---|
| 票据无效 / 过期 / 已消费 | 401 | `ApiError::unauthorized` |
| 会话不存在或不属于当前用户 | **404** | 与“不存在”同文案，避免枚举他人会话 id |
| 目标主机不可达 / 认证失败 / 指纹不匹配 | 400 | 原文进 `last_error`，响应里**不含**凭证 |
| 缺少 `LLM_KEY_ENCRYPT_KEY` | 400 | 明确提示“未设置加密密钥，无法保存凭证” |
| 超过 `SSH_MAX_SESSIONS_PER_USER` | 400 | 提示当前上限，不自动踢旧会话 |
| 服务身份（API Key / OAuth）调用 | 403 | 见 §4.4 |

---

## 6. 前端实现

### 6.1 服务层与命令映射

`apps/web/src/services/sshService.ts`：

```ts
export interface SshTargetView {
    id: string; name: string; host: string; port: number; username: string;
    auth_type: 'password' | 'private_key'; has_credential: boolean;
    credential_hint: string | null; host_key_fingerprint: string | null;
    strict_host_key_check: boolean; asset_id: string | null; description: string | null;
    connect_timeout_secs: number; last_connected_at: string | null; last_error: string | null;
    enabled: boolean; created_at: string | null; updated_at: string | null;
}
export const getSshTargets = () => api.get<SshTargetView[]>('get_ssh_targets');
export const openSshSession = (targetId: string, cols: number, rows: number) =>
    api.post<{ session_id: string; ticket: string; expires_in: number }>(
        'open_ssh_session', { target_id: targetId, cols, rows });
```

> 字段名与后端 serde 逐字一致（snake_case），**不做 camelCase 转换**：`httpAdapter` 直接把命令参数当 body 发，
> 前端擅自改名会直接 400（`sshService.test.ts` 有断言守着）。

`apps/web/src/utils/adapters/httpAdapter.ts` 的 `COMMAND_ROUTE_MAP` 追加 10 条（`get_ssh_targets` / `get_ssh_target` / `create_ssh_target` / `update_ssh_target` / `delete_ssh_target` / `probe_ssh_target` / `probe_draft_ssh_target` / `get_ssh_sessions` / `open_ssh_session` / `close_ssh_session`）；`tests/utils/adapters/httpAdapter.test.ts` 的手工命令清单**只增不改**地补上同名 10 条。

契约护栏：`apps/web/src/tests/services/sshService.test.ts`（13 用例）断言「10 个命令 → §5.1 的 method/path」、请求体键名保持 snake_case、`buildSshWsUrl` 的 ws/wss 与票据编码（JWT 绝不进 URL）。

### 6.2 `useSshTerminal` 状态机

依赖：`@xterm/xterm`、`@xterm/addon-fit`、`@xterm/xterm/css/xterm.css`。

```ts
// 1) openSshSession → { sessionId, ticket }
// 2) wsUrl = (getApiOrigin() ?? window.location.origin).replace(/^http/, 'ws')
//            + `/api/ops/ssh/sessions/${sessionId}/ws?ticket=${encodeURIComponent(ticket)}`
// 3) onopen：fit() → 发送首个 resize；ResizeObserver（debounce 120ms）→ {type:'resize',cols,rows}
// 4) term.onData(d => ws.send(new TextEncoder().encode(d)))  // 输入走 Binary 帧，避开 Text/JSON 控制帧
// 5) ws.onmessage：binary → term.write(new Uint8Array(e.data))；text JSON → exit/closed 分支
// 6) ws.onclose → status='disconnected'，不自动重连（会话已销毁），提供「重新连接」= 重新 openSshSession
// 7) 卸载：ws.close() + closeSshSession(sessionId)（忽略 404/401）
```

> 服务端容错：收到 `Text` 帧先按 JSON 解析，形如 `{"type":...}` 走控制分支，解析失败则按 UTF-8 原样写入 PTY（便于个别终端库发字符串输入）。

状态：`idle → connecting → active → disconnected | failed`；`failed` 时在终端上方显示 `lastError` 文案与「重试」。

### 6.3 页面与组件

| 文件 | 职责 |
|---|---|
| `components/Ops/SshTargetFormFields.tsx` | 仿 `ConnectionFormFields`：host / port / username / authType / 密码或私钥 PEM + passphrase / `AssetSelect` 绑定资产 / 严格指纹开关 / 超时；含「扫描主机指纹」按钮，回填 `SHA256:...` 并要求确认 |
| `components/Ops/SshTerminalPanel.tsx` | xterm 容器 + 工具栏（重新连接 / 关闭会话 / 复制 / 全屏）+ 状态徽标（connecting / active / closed） |
| `app/ops/terminal/page.tsx` | `'use client'` + `Layout`：左列主机卡片（增删改查 / 测试 / 连接），右列终端；支持 `?target=<id>` 自动连接；错误用 `Alert`、操作反馈用 `notifications.show`（与 `/ops/connections` 一致） |

---

## 7. 菜单与权限种子

写入 `crates/database/src/sql/public_initial_data.sql` §11（**必须在 §12「默认角色菜单关联」之前**，且沿用 `INSERT ... SELECT ... WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = N)` 幂等写法）：

| id | 名称 | parent | path | component | icon | perms | menu_type | 绑定路由 |
|---|---|---|---|---|---|---|---|---|
| 62 | SSH 终端 | 7 | `/ops/terminal` | `/ops/terminal/page` | `IconTerminal2` | `ssh:target:list` | 2 菜单 | `GET /api/ops/ssh/targets` |
| 166 | 新增主机 | 62 | — | — | — | `ssh:target:create` | 3 按钮 | `POST /api/ops/ssh/targets` |
| 167 | 编辑主机 | 62 | — | — | — | `ssh:target:update` | 3 按钮 | `PUT /api/ops/ssh/targets/{id}` |
| 168 | 删除主机 | 62 | — | — | — | `ssh:target:delete` | 3 按钮 | `DELETE /api/ops/ssh/targets/{id}` |
| 169 | 测试主机 | 62 | — | — | — | `ssh:target:test` | 3 按钮 | `POST /api/ops/ssh/targets/{id}/probe` |
| 170 | 关闭会话 | 62 | — | — | — | `ssh:session:close` | 3 按钮 | `DELETE /api/ops/ssh/sessions/{sessionId}` |
| 171 | 会话接入(WS) | 62 | — | — | — | `ssh:session:attach` | 3 按钮 | `GET /api/ops/ssh/sessions/{sessionId}/ws` |
| 172 | 查看会话 | 62 | — | — | — | `ssh:session:list` | 3 按钮 | `GET /api/ops/ssh/sessions` |
| 173 | 打开会话 | 62 | — | — | — | `ssh:session:open` | 3 按钮 | `POST /api/ops/ssh/sessions` |

> **每个 `ROUTE_PERMS` 权限点都必须有对应的 `sys_menu` 行**：用户权限 = 其角色被授予菜单的 `perms` 去重，
> 没有菜单行承载的权限点对非超管角色**永远无法授予**（`ssh:target:list` / `ssh:session:list` 就是为此补的 62 与 172）。
> 菜单 62 的 `perms` 取「列表」口径（与菜单 60/61 的 `ops:connection:list` / `ops:resource:list` 一致），
> 会话相关权限由 170–173 承载。

按钮行的 `command_name` / `http_method` / `http_path` 必须与 §5.1 逐字一致（`get_ssh_targets` / `POST /api/ops/ssh/sessions` 等），否则 `authz` 启动自检会把它们算成“code-only 路由”告警。

---

## 8. 任务拆分与验收

| # | 任务 | 主要文件 | DoD / 验证 |
|---|---|---|---|
| T1 | 依赖与脚手架 | 根 `Cargo.toml`、`crates/api/Cargo.toml`、`crates/service/Cargo.toml` | `cargo check -p api` 通过（`ws` / `russh` 可解析） |
| T2 | `remote-shell` 契约 + `probe` | `crates/remote-shell/{Cargo.toml,src/lib.rs}` | `cargo test -p remote-shell`；`cargo check -p remote-shell --no-default-features` |
| T3 | `remote-shell` 会话（PTY / resize / 输出流） | 同上 | 同上；`Handler::check_server_key` 指纹比对单测 |
| T4 | 迁移 + 模型 + 挂载 | `sql/terminal_module_migration.sql`、`models.rs`、`postgres.rs` | 新租户建库 + 存量库重启均建表成功（`\d` 抽查） |
| T5 | 目标 CRUD + 凭证加密 | `service/src/ssh_service.rs` | `cargo test -p service ssh`（CRUD/加密回环/hint/唯一名） |
| T6 | 会话 hub | 同上 | 单测（归属校验、幂等关闭、计数）；手工开/关一次会话 |
| T7 | 空闲回收 + 环境变量 | 同上、`api/src/lib.rs` | 回收阈值纯函数单测；日志可见回收记录 |
| T8 | 一次性票据 | `api/src/ssh_ticket.rs` | 单测：TTL、单次消费、session 不匹配 |
| T9 | REST handlers（10 条） | `api/src/ssh_routes.rs` | `cargo test -p api ssh`；400/401/404 口径 |
| T10 | WS handler + 装配 | `ssh_routes.rs`、`api/src/lib.rs` | `cargo check -p api`；WS 升级本地冒烟 |
| T11 | 权限表 + OpenAPI | `authz.rs`、`apidoc/src/openapi.rs` | `cargo test -p api authz`、`cargo test -p apidoc` |
| T12 | 前端服务与映射 | `sshService.ts`、`httpAdapter.ts`（+ 测试清单） | `tsc --noEmit`、`pnpm --filter web test -- httpAdapter` |
| T13 | 终端 hook / 组件 / 页面 | `useSshTerminal.ts`、`SshTargetFormFields.tsx`、`SshTerminalPanel.tsx`、`app/ops/terminal/page.tsx` | `tsc --noEmit`；页面冒烟（连接 / resize / 断开提示） |
| T14 | 菜单种子 + 文档收尾 | `public_initial_data.sql`、本文档 §11 | 重启后侧边栏出现「SSH 终端」；本文档变更记录追加一行 |

---

## 9. 验证矩阵

### 9.1 自动化

| 层 | 命令 | 期望 | 2026-11-18 实测 |
|---|---|---|---|
| 传输 crate | `cargo test -p remote-shell` | 全绿（含 `--no-default-features` 可编译） | 11 passed（exit 0） |
| 服务 | `cargo test -p service ssh` | 全绿 | 9 passed（全量 `-p service` 76 passed） |
| API | `cargo test -p api ssh` | 全绿（票据 / 归属 / 错误码） | 10 passed（全量 `-p api` 116 passed） |
| 权限 | `cargo test -p api authz` | 全绿（`ROUTE_PERMS × SERVICE_FORBIDDEN_ROUTES` 自洽老用例不破） | 16 passed（含 `route_perms_entries_are_well_formed`） |
| 文档 | `cargo test -p apidoc` | 全绿（Swagger 路径数增加） | 8 passed |
| 编译 | `cargo check -p api -p server -p bootstrap` | 无错误 | exit 0 |
| 前端 | `pnpm --filter web exec tsc --noEmit` | 0 错误 | exit 0 |
| 前端 | `pnpm --filter web test -- sshService httpAdapter` | 全绿（`sshService.test.ts` 13 + `httpAdapter.test.ts` 21） | 2 suites / 34 passed |

> 未执行项：Docker WS 终端端到端冒烟（本机无 docker，见 §9.2 手工步骤）。种子 SQL 的正确性依赖启动
> 时执行 `public_initial_data.sql`（`init_postgres_database` 第 5 步，`run_init_step` 容错）——重启后
> 看日志有无「public 默认数据」失败项即可确认；本次改动未在实库上跑过，属已知缺口。

### 9.2 手工冒烟（实施的最后一关）

1. 起一个本地 sshd：`docker run --rm -p 2222:22 -e PASSWORD_ACCESS=true -e USER_PASSWORD=test lscr.io/linuxserver/openssh-server`（无 docker 时用本机 `localhost:22`）。
2. 登录 Web → 「运维采集 → SSH 终端」→ 新增主机（`127.0.0.1:2222` + 密码）→ 点「扫描主机指纹」→ 确认 `SHA256:...` → 保存。
3. 连接 → 终端可输入 `top`（验证全屏刷新）、`vim`（验证备用屏）、拖拽窗口（验证 `resize`）。
4. 关闭页面 → `ops_ssh_session` 中该行 `state='closed'`、`ended_at` 非空、`bytes_in/out > 0`。
5. 反例：把 `host_key_fingerprint` 改错一位 → 连接失败且错误文案指明指纹不匹配；用另一个用户访问该 `sessionId` → 404。
6. 重启服务端 → 迁移幂等，无重复建表报错；侧边栏菜单仍存在。

---

## 10. 风险与限制

### 10.1 单实例 / sticky 约束（必须写进部署文档）

会话 TCP 与票据都在**进程内**：网关多实例 + 轮询转发时，`POST /sessions` 与随后的 WS 升级可能落到不同实例 → 升级失败。可接受的做法：单实例部署，或对 `/api/ops/ssh/*` 做 sticky。跨实例接管（DB 票据 + 会话迁移）**不做**。

### 10.2 终端 = 任意命令执行（合规风险）

- 兜底手段只有：`ssh:session:open` 权限点 + `ops_ssh_session` 审计（谁 / 何时 / 哪台 / 字节数 / 退出码）。
- 本期**不做**命令级录制；若合规要求“可回放”，按 P2 立项（asciinema 格式落对象存储 + 会话详情页回放）。
- 建议运营侧只给最小必要账号（非 root / 只读账号），并在目标主机侧配 `AllowUsers` / `ForceCommand`。

### 10.3 依赖拉取与降级路径

| 现象 | 处理 |
|---|---|
| `cargo fetch` 慢 / 超时 | 重试；本地 registry 已有 `ring` / `tokio-tungstenite` 缓存，不影响编译 |
| `russh` 拉取失败 | 降级顺序：`ring` → `aws-lc-rs`（需 cmake） → `ssh2`（vendored C） → 调系统 `ssh`（仅 Linux，功能受限） |
| `@xterm/xterm` 装不上 | 页面降级为 `<pre>` 文本流并在顶部明示“降级模式”，**不静默 mock** |

### 10.4 环境变量

| 变量 | 默认 | 作用 |
|---|---|---|
| `LLM_KEY_ENCRYPT_KEY` | 无（必填） | 凭证 AES-256-GCM 主密钥（前 32 字节），缺失时保存凭证直接报错 |
| `SSH_SESSION_IDLE_SECS` | `1800` | 空闲多少秒回收会话 |
| `SSH_MAX_SESSIONS_PER_USER` | `3` | 单用户并发会话上限 |
| `SSH_TICKET_TTL_SECS` | `60` | WS 一次性票据有效期 |

### 10.5 已知未覆盖

- 主机指纹算法只支持 `SHA256:`（不解析 `MD5:` 旧格式，避免误判为“匹配”）。
- `private_key` 仅支持 OpenSSH / PEM 文本，不支持 PKCS#12 与硬件令牌。
- 会话列表默认只显示“活动会话”，历史会话查询留 P2（表结构已支持）。

---

## 11. 变更记录

| 日期 | 阶段 | 内容 | 验证 |
|---|---|---|---|
| 2026-11-17 | 方案 | 定稿：传输（WS + 一次性票据）、数据模型（两表）、SSH 实现（`russh` + `ring`）、权限与菜单、任务 T1–T14 | 本文件评审通过；依赖与现状核查见 §1.2 |
| 2026-11-18 | 实施 | 后端：`crates/remote-shell`（`probe` / `open` / `SshSession`，`ring` 后端）、`terminal_module_migration.sql`（`ops_ssh_target` / `ops_ssh_session`，已挂 `init_tenant_tables`）、`ssh_service` / `ssh_ticket` / `ssh_routes`（10 REST + 1 WS）、`ROUTE_PERMS` 与菜单种子 | `cargo check -p service -p api` 仅告警无错误 |
| 2026-11-18 | 收尾 | 权限点按「模块:资源[:动作]」收正为 3 段 `ssh:target:*` / `ssh:session:*`（替换 4 段 `ops:ssh:*`），同步菜单/按钮种子与本文档；新增 `apps/web/src/tests/services/sshService.test.ts`（13 用例：命令→路由契约、snake_case 请求体、`buildSshWsUrl` 的 ws/wss + 票据编码）；修正 `ssh_service` 空闲回收单测的边界断言 | `cargo test -p api authz`（16）、`-p service ssh`（9）、`-p api ssh`（10）、`-p remote-shell`（11）、`-p apidoc`（8）全绿；`cargo check -p api -p server -p bootstrap` 0 错误；`tsc --noEmit` 0 错误；`jest sshService`（13）+ `jest httpAdapter`（21）全绿 |
| 2026-11-18 | 收尾 | 菜单种子补齐权限点承载：菜单 62 由 `ssh:session:open` 改为 `ssh:target:list`（`get_ssh_targets` / `GET /api/ops/ssh/targets`，与菜单 60/61 的 list 口径一致），新增按钮 172「查看会话」`ssh:session:list`、按钮 173「打开会话」`ssh:session:open`；`public_initial_data.sql` 末尾追加两条幂等 `UPDATE`（4 段→3 段改名；菜单 62 仅命中旧值行才改写）| §9.1 全量实测同批全绿；`git grep 'ops:ssh'` 仅剩该迁移语句与本文档说明；种子在启动时随 `init_public_default_data` 执行，实库效果待重启确认 |
