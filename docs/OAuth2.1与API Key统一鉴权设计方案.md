# OAuth 2.1 与 API Key 统一认证 / 鉴权设计方案

| 项 | 值 |
|---|---|
| 文档版本 | v0.2（设计稿） |
| 日期 | 2026-11-10 |
| 适用范围 | `apps/backend/src-tauri`（Tauri v2 + axum 0.8.9 + sqlx 0.9 + utoipa 6 + utoipa-axum + apidoc） |
| 状态 | 待评审，评审通过后进入 Phase 1 实施 |
| 关联文档 | `TODO.md`（#11 依赖清理、#16 i64 精度）、`README.md`（部署）、`docs/多租户数据权限与租户切换设计方案.md`、`docs/前端大整数精度丢失问题(Snowflake ID).md` |

> 本文档同时覆盖 **认证（Authentication：你是谁）** 与 **鉴权（Authorization：你能做什么）**。
> 结论先行：本项目已有**完整鉴权数据模型**（`sys_menu.perms` + `http_method` + `http_path`），但**服务端没有执行点**；API Key 与 OAuth 令牌都要接到这条鉴权管线上，而不是另起一套权限词汇表。

---

## 0. 一页速读

1. **OAuth 2.1 是 OAuth 2.0 的严格子集**（只删不加）。因此「2.1 兼容 2.0」只有单向含义：实现 2.1 的强制子集后，凡是走 `authorization_code + PKCE` 的 2.0 客户端零改动可用；不能兼容的只有 implicit、password grant、`plain` PKCE、URI query 传 token 等已废弃用法。
2. **不实现 `/oauth/token` 不影响对外发凭证**：API Key 本质是「预签发、不轮换的 opaque bearer token」，等价于把 `client_credentials` 的结果固化成静态凭证。对外部系统（ERP/OA/运维平台）而言，这比跑 OAuth 流程更简单也更好撤销。
3. **HTTP 明文下无法声称合规**：2.1 要求 authorize / token 端点必须 TLS ≥ 1.2；规范内唯一例外是 **native app 的 loopback 重定向 URI**（RFC 8252 §7.3），**不是 AS 端点本身**。落地只能是「反代终止 TLS / 进程内 rustls / 显式声明内网例外 + 补偿控制」。
4. **鉴权必须先于开放**：当前 `/api/upload/*`、`/api/chat/stream` 根本没有挂认证（`lib.rs:121` 在 `auth_middleware` 之外 merge），且 105 条受保护路由里只有 38 条在 `sys_menu` 里登记了 `http_path`。**先补齐鉴权覆盖面，再对外发 API Key**，否则等于把未授权接口暴露给外部系统。
5. **一条管线服务三类主体**：JWT 用户、API Key 服务身份、OAuth 令牌，统一解析为 `AuthPrincipal`，再统一走 `(method, MatchedPath) → perms` 的授权判定。API Key 的 `scopes` **直接使用 `sys_menu.perms` 的取值**（如 `asset:hardware:list`），不另造词汇。
6. **硬约束：Tauri 模式零影响**。Tauri 与 axum 同进程（`src/lib.rs` 里 `tauri::Builder` 与后台 `start_http_server_with` 并存），但 Tauri `invoke()` **不经过 axum**。因此所有鉴权代码只允许落在 `api` crate 的 HTTP 层；**禁止进程级 fail-fast、禁止改动 `GLOBAL_SCHEMA` / `schema_prefix` 语义、禁止给 `service` 既有函数改签名**。逐条不变量见 §16。

---

## 1. 规范基线

### 1.1 版本现状（截至 2026-10）

| 规范 | 状态 |
|---|---|
| `draft-ietf-oauth-v2-1-17` | 2026-10-31 发布，**I-D Exists**（Expires 2027-05-04）；OAuth WG 里程碑计划 2026-12 提交 IESG。**尚未成为 RFC** |
| RFC 9700（BCP 240，2025-01） | 已生效的 OAuth 2.0 安全最佳实践，更新 RFC 6749 / 6750 / 6819 |
| RFC 6749 / 6750 | OAuth 2.0 框架 / Bearer Token 用法 |
| RFC 8252 §7.3 | native app 允许使用 loopback（`http://127.0.0.1:端口`）作为**重定向地址** |
| RFC 8414 / 7009 / 7662 / 9207 | AS 元数据 / 撤销 / 内省 / `iss` 参数 |

**工程结论**：本项目所说的「OAuth 2.1」= RFC 6749/6750 + RFC 9700 + draft 2.1 强制项集合，不追求具体 RFC 编号。

### 1.2 2.1 相对 2.0 的变化矩阵

| 特性 | OAuth 2.0 | OAuth 2.1 | 本项目决策 |
|---|---|---|---|
| Authorization Code | 必须 | 必须 | ✅ Phase 2 实现 |
| Implicit（`response_type=token`） | 可选 | **删除** | ❌ 不实现，返回 `unsupported_response_type` |
| Resource Owner Password | 可选 | **删除** | ❌ 不实现。现有 `POST /api/auth/login` 是**自有登录端点**，不属于 OAuth 授权类型，文档中明确标注 |
| Client Credentials | 可选 | 保留 | ⚠️ Phase 2 **不实现**（与 API Key 功能重叠，避免两套长期凭证；见 §14 Q1） |
| PKCE | 可选（public 推荐） | **所有 code 流强制** | ✅ `code_challenge` 必填；默认只接受 `S256` |
| Redirect URI 校验 | 允许前缀/通配 | **精确串匹配** | ✅ 精确匹配，逐条比对，不做前缀 |
| Bearer 传递方式 | header / form / **query** | **仅 header** | ✅ 拒绝 `?access_token=` / `?api_key=`，返回 400 并告警 |
| Refresh Token | 可选轮换 | **必须轮换 + 重放检测** | ✅ 直接复用现有 `family_id` 轮换机制 |
| 传输层 | 推荐 TLS | **强制 TLS ≥ 1.2** | ⚠️ 见 §8 |
| `state` | 推荐 | 推荐（CSRF 防护） | ✅ 必填并回传校验 |
| `iss` 参数 | 无 | 推荐（RFC 9207） | ✅ 授权响应回传 |
| 授权码有效期 | 建议 ≤ 10min | 建议短 | ✅ 60s + 一次性 + 只存哈希 |

### 1.3 HTTP 明文下的合规边界（回答「启用 http 时怎么办」）

- 2.1 §Transport Security：授权端点、令牌端点以及其他**接收或返回凭证的端点**必须使用 TLS ≥ 1.2。
- 唯一规范内例外：**native app 的 loopback 重定向 URI**（RFC 8252 §7.3），如 `http://127.0.0.1:14565/callback`。这是「回调地址」的例外，**不适用于 AS 端点（`/oauth/authorize`、`/oauth/token`）本身**。
- 推论：
  1. Tauri 桌面端作为 OAuth **客户端** 使用 loopback 回调 → 合规；
  2. 本服务作为 **AS** 跑在 `http://` → **不合规**，只能通过 §8 的方案兜底，并在文档中显式声明；
  3. 无论选哪种方案，都必须有「防误上线」护栏（`JWT_SECRET` 缺失时 **HTTP 服务拒绝启动**、默认绑定回环地址、非 TLS 且非回环时拒绝签发令牌）。**这些护栏一律放在 HTTP 启动路径，不做进程级 panic**，否则会连带影响 Tauri 桌面模式（§16 T-2）。

---

## 2. 现状盘点和差距

### 2.1 认证与令牌

| 能力 | 现状 | 位置 | 差距 |
|---|---|---|---|
| 用户认证 | 仅 `Authorization: Bearer <JWT>`，硬编码 `strip_prefix("Bearer ")` | `crates/api/src/auth.rs:129-143` | 不支持 API Key / OAuth 令牌 |
| 签名密钥 | `JWT_SECRET` **env 缺失时回落到默认串** | `auth.rs:43-44`、`service/src/user_service.rs:76-78` | 明文部署 + 默认密钥 = 任何人都能伪造令牌，**必须校验**；校验放在 HTTP 启动入口，不改 Tauri 侧的默认值回落（§16 T-2 / §16.3） |
| 令牌有效期 | access 7 天 / refresh 30 天 | `user_service.rs:59-62` | 对外令牌必须短 TTL（建议 15 分钟） |
| 刷新轮换 | **已实现**：`jti` + `family_id` + `revoked_at` + `replaced_by` + 重放即整族撤销 | `user_service.rs:167-360`、`public_tables.sql:280-288` | 可直接复用于 OAuth `refresh_token` |
| 敏感信息工具 | AES-GCM **可逆**加密（用于 LLM provider key）+ `mask_api_key` | `crates/utils/src/crypto.rs` | 外部 API Key **必须单向哈希**，可逆加密无法等值查找且泄露即全部还原 |
| 错误模型 | 自定义 `{code,message}` | `crates/api/src/response.rs` | OAuth 端点必须用 `{error,error_description}`，两套不能混用 |

### 2.2 鉴权（授权）

| 能力 | 现状 | 位置 | 差距 |
|---|---|---|---|
| 权限数据模型 | `sys_menu.perms`（三段式）+ `menu_type`(1目录/2菜单/3按钮) + `http_method` + `http_path`；链路 `sys_user_role → sys_role_menu → sys_menu`；超管标记 `sys_user.is_super_admin` / `sys_role.is_super_admin` | `public_tables.sql:89-179`、`public_initial_data.sql` | 模型可用 |
| 权限执行点 | **无**。服务端不校验 `perms`，仅个别业务做 `is_super_admin` 特判 | `api/src/user_routes.rs:352-357`、`service/src/role_service.rs` | ❗任意持有 JWT 的用户可调用任意接口（越权） |
| 路由覆盖率 | `api` crate 共 **105** 条 `.route()`（其中 `lib.rs` 99 条）；`sys_menu` 中带 `http_path` 的仅 **38** 条 | `api/src/lib.rs`、`public_initial_data.sql` | ❗**61% 路由无权限点**；fail-closed 上线会大面积 403 |
| 权限点数量 | 已定义 **67** 个 distinct `perms`（62 个三段式 + 5 个二段式） | `public_initial_data.sql` | 命名不统一（`add/edit` vs `create/update`；`knowledge:chat`、`workflow:*` 缺动作段），见 §6.2 |
| 路由保护完整性 | `/api/upload/*`（7 条）与 `/api/chat/stream`（SSE）在 `auth_middleware` **之外** merge | `api/src/lib.rs:89-121` | ❗**当前完全无认证** |
| 数据级鉴权 | 租户隔离靠 PostgreSQL schema（`with_schema` task_local）；同租户内无部门/本人级过滤 | `database/src/lib.rs:48-73`、`auth.rs:75-121` | 同租户任意用户可改任意资产（水平越权） |
| 匿名白名单 | `auth.rs:169` 定义了 `PUBLIC_ROUTES` 常量，但实际路由取舍手写在本文件 `lib.rs` | `api/src/auth.rs:169`、`lib.rs:149-380` | 双份事实来源，易漏保护 |

### 2.3 其他相关事实

- `sys_tenant.schema_name` 唯一，末级租户才有 schema（`public_tables.sql:6-19`）。
- `/api/upload/*` 与 `/api/chat/stream` 目前靠「不在 auth 层内」实现匿名访问，**补鉴权属于行为变更**（见 §14 R1）。
- CORS 配置为 `allow_origin(Any)` + `allow_headers(Any)`（`lib.rs:69-78`），对「浏览器里直接带 API Key」的场景需要收紧（见 §14 R4）。
---

## 3. 设计目标与非目标

### 3.1 目标

| ID | 目标 | 验收信号 |
|---|---|---|
| G1 | 外部系统可凭 **API Key** 调用开放接口 | 外部系统用 `X-API-Key` 调用 `/api/assets/hardware` 成功；吊销后 1s 内失效 |
| G2 | API Key 可过期、可撤销、最小权限、可审计 | `scopes` 必填且 ⊆ 创建者权限；`last_used_at` 可查 |
| G3 | **补齐服务端鉴权执行点**，覆盖全部受保护路由 | 未授权调用返回 403 `insufficient_scope`；启动自检报告 0 条未保护路由 |
| G4 | 三类主体（用户 JWT / API Key / OAuth 令牌）走同一条认证 + 鉴权管线 | handler 无需区分主体，`AuthPrincipal` 统一 |
| G5 | Phase 2 提供最小可用 OAuth 2.1 AS（code+PKCE+refresh+introspect） | 第三方可代用户授权；`/oauth/introspect` 可校验令牌 |
| G6 | HTTP 明文部署有明确边界与护栏 | 非 TLS 且非回环时拒绝签发令牌；`JWT_SECRET` 缺失时 **HTTP 服务拒绝启动**（仅 HTTP，Tauri 模式不受影响） |

### 3.2 非目标

- 不做 OIDC（`id_token` / `userinfo` / 完整 discovery），除非后续要接第三方登录。
- 不做动态客户端注册（RFC 7591）、mTLS（RFC 8705）、DPoP（发送方约束令牌）。
- 不改前端权限配置 UI：继续复用现有「角色 → 菜单/按钮」勾选页，只是让它在**服务端生效**。
- 不做字段级脱敏（列为后续增强）。

---

## 4. 总体架构

### 4.1 请求处理管线

```
请求
 |
 +--[0] 传输层护栏：TLS 或回环 或 显式 OAUTH_ALLOW_INSECURE_HTTP
 |
 +--[1] 凭证提取 Credential Extraction
 |      Authorization: Bearer <JWT>        -> 用户
 |      Authorization: Bearer ak_...       -> 服务身份（API Key）
 |      X-API-Key: ak_...                  -> 服务身份（兼容网关/SDK）
 |      Authorization: Bearer at_...       -> OAuth 令牌（Phase 2，opaque + introspect）
 |      ?access_token= / ?api_key=         -> 400 invalid_request + 安全告警
 |
 +--[2] 认证 Authentication -> AuthPrincipal
 |      验签 / 哈希比对 / 有效期 / 撤销状态 / 租户可用性 / 用户状态
 |      schema_name 解析 -> database::with_schema(task_local)
 |
 +--[3] 鉴权 Authorization
 |      目标 =(MatchedPath, method) -> 所需 perms 集合（来自 sys_menu）
 |      主体 = 用户：角色 perms 并集（超管 "*"）| API Key：key.scopes | OAuth：授权时快照的 scopes
 |      判定 = 目标 ⊆ 主体 ? 放行 : 403 insufficient_scope
 |
 +--[4] 数据级鉴权（租户 schema 已在 [2] 生效；部门/本人级见 6.5）
 |
 +--[5] 速率限制 + 审计（principal / route / perms / result / latency）
 |
 +-- handler（现有代码零改动）
```

### 4.2 凭证通道能力矩阵

| 通道 | 主体 | 默认有效期 | 撤销粒度 | 可代用户 | 典型用途 |
|---|---|---|---|---|---|
| JWT（现有） | 用户 | 7d（建议对内 8h）+ refresh 30d | 用户级 / 令牌族 | ✅ 仅本人 | Web / Tauri 前端 |
| **API Key** | 服务 | 90d（可配，最长 365d） | 单个 key | ❌ | ERP/OA/运维平台集成、CI 脚本 |
| OAuth access token（Phase 2） | 用户或服务 | 15min | 单令牌 + 令牌族 | ✅ 授权用户 | 第三方应用代用户操作 |
| OAuth refresh token（Phase 2） | 同上 | 30d，每次使用轮换 | 令牌族 | ✅ | 免重复授权 |

### 4.3 模块归属（沿用现有 crate 分层，不新增 crate）

| 功能 | crate | 文件 |
|---|---|---|
| Key 生成/哈希/解析 | `utils` | `src/api_key.rs`（新） |
| Key 数据模型 | `database` | `src/models.rs`、`src/postgres.rs`（无需改，`include_str!` 已覆盖） |
| Key 业务逻辑 | `service` | `src/api_key_service.rs`（新） |
| 凭证提取 / 认证中间件 | `api` | `src/auth.rs`（改造） |
| 鉴权中间件 / 权限缓存 | `api` | `src/authz.rs`（新） |
| Key 管理接口 | `api` | `src/api_key_routes.rs`（新） |
| OAuth 端点（Phase 2） | `api` | `src/oauth_routes.rs`、`src/oauth_error.rs`（新） |
| OAuth 业务 | `service` | `src/oauth_service.rs`（新） |
| OpenAPI 汇总 | `apidoc` | `src/lib.rs`（注册 tag/path） |
| DDL | `database` | `src/sql/public_migration.sql` |

> 依赖方向保持不变：`api -> service -> database -> utils`，`apidoc -> api`（单向）。
>
> **注意共享层**：`service` / `database` / `utils` 同时被 Tauri 命令与 HTTP 使用，只允许「新增文件 + 新增函数」，不允许改变既有行为（见 §16 的 T-1 / T-8）。

---

## 5. 认证设计

### 5.1 凭证提取与分流

```rust
// crates/api/src/auth.rs（扩展现有 auth_middleware）
enum Credential { Jwt(String), ApiKey(String), OAuth(String) }

fn extract(req: &Request) -> Result<Credential, ApiError> {
    // 1) 2.1 禁止 URI query 传令牌：命中即拒绝并告警（用于发现老客户端）
    if req.uri().query().map_or(false, |q| q.contains("access_token=") || q.contains("api_key=")) {
        tracing::warn!(path = %req.uri().path(), "拒绝 URI query 传令牌（OAuth 2.1 禁止）");
        return Err(ApiError::bad_request("令牌不得通过 URL 参数传递"));
    }
    if let Some(v) = req.headers().get("x-api-key").and_then(|v| v.to_str().ok()) {
        return Ok(Credential::ApiKey(v.trim().to_string()));
    }
    let raw = req.headers().get("authorization").and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::unauthorized("缺少 Authorization 请求头"))?;
    let token = raw.strip_prefix("Bearer ").ok_or_else(|| ApiError::unauthorized("Authorization 格式错误"))?;
    Ok(match token {
        t if t.starts_with("ak_") => Credential::ApiKey(t.to_string()),
        t if t.starts_with("at_") => Credential::OAuth(t.to_string()),
        t => Credential::Jwt(t.to_string()),
    })
}
```

要点：
- `ak_` / `at_` 前缀与 JWT 的 `eyJ` 不冲突，**无需解码即可分流**。
- 沿用现有 `verify_token()` 校验 JWT（含 `typ` 检查，拒绝 refresh 令牌当访问令牌用）。
- 失败一律 `401 invalid_token`，**不区分「不存在 / 已撤销 / 已过期」**，避免凭证枚举。

### 5.2 API Key 规格

**格式**：`ak_<env>_<prefix16>_<secret>`

| 段 | 例 | 规则 |
|---|---|---|
| 前缀 | `ak` | 固定，用于分流 |
| 环境 | `live` / `test` | 仅分类与展示，不参与校验 |
| 定位段 | `7f3c9a1b2d4e6f80` | 12 字节随机 → base32，**明文入库**，用于列表展示、日志定位、用户自查 |
| 密钥段 | 43 字符 base64url | 32 随机字节（256bit 熵），**只在创建响应出现一次** |

**存储**：`key_hash = SHA-256(完整 key 字符串)` 的 hex（64 字符）。

> ⚠️ 与 `utils::crypto::encrypt_api_key`（AES-GCM 可逆）的本质区别：可逆加密**无法做等值查找**（每次请求都要全表解密），且 DB 泄漏即可还原所有密钥。外部 API Key 必须**单向哈希**。

DDL 见 §9.1。

**校验 SQL**：

```sql
SELECT id, tenant_id, scopes, rate_limit_per_min, expires_at
FROM public.sys_api_key
WHERE key_hash = $1
  AND enable = TRUE
  AND revoked_at IS NULL
  AND (expires_at IS NULL OR expires_at > now())
  AND (deleted IS NULL OR deleted = 0);
```

- 命中即通过，不需要逐字符比较，**无时序侧信道**（不需要 `subtle` 常数时间比较）。
- `last_used_at` 更新**节流 ≥ 60s**（首次命中立即写，之后同 key 60s 内不再写），避免每请求一条 UPDATE。
- 校验结果进 `DashMap` 短缓存（TTL 30s）以降低 QPS 压力；**吊销时主动清除缓存**，保证 1s 内生效。

**生命周期**：创建（返回一次性明文 + 提示妥善保存）→ 使用 → 轮换（新 key 生效，旧 key `expires_at` 提前 24h）→ 吊销（写 `revoked_at`，行保留用于审计）→ 软删除。

**管理接口**（必须走 JWT + 管理员 perms；**禁止用 API Key 管理 API Key**，避免自举提权）：

| 方法 | 路径 | 所需 perms |
|---|---|---|
| POST | `/api/api-keys` | `system:apikey:add` |
| GET | `/api/api-keys` | `system:apikey:list` |
| POST | `/api/api-keys/{id}/rotate` | `system:apikey:edit` |
| DELETE | `/api/api-keys/{id}` | `system:apikey:delete` |

### 5.3 JWT 加固（对外开放前必须完成）

| 项 | 现状 | 目标 |
|---|---|---|
| 签名密钥 | env 缺失时用默认串 | **仅在 HTTP 启动路径校验**（`start_http_server_with()` 内返回 `Err` → 只有 HTTP 服务不启动，Tauri 桌面模式仍用默认值，见 §16 T-2 / §16.3）；长度 ≥ 32 字节；支持 `kid` 以便轮换 |
| access TTL | 7 天 | 内部 8h / 对外 15min（`JWT_ACCESS_TTL_SECS` 可配） |
| claims | `sub:i64, username, exp, iat, typ, jti` | 增 `iss`、`aud`、`scope`；`sub` 改字符串 `"u:<id>"`（同时规避 TODO#16 的 i64→JS 精度问题） |
| 算法 | HS256 | Phase 1 保持 HS256（单体）；若未来外部资源服务需离线验签 → EdDSA/RS256 + JWKS |
| 撤销 | 仅 refresh 有白名单 | access 短 TTL + 可选 `jti` 黑名单（登出/封禁立即生效） |

### 5.4 统一主体

```rust
pub enum PrincipalKind { User, Service, OAuth }

pub struct AuthPrincipal {
    pub kind: PrincipalKind,
    pub subject: String,        // "u:123" | "k:<api_key_id>" | "u:123@client:app1"
    pub user_id: Option<i64>,
    pub api_key_id: Option<i64>,
    pub tenant_id: Option<i64>,
    pub schema_name: String,
    pub scopes: Vec<String>,    // 见 6.4 判定算法
    pub is_super_admin: bool,
    pub rate_limit_per_min: Option<i32>,
}
```

### 5.5 兼容性约束（不能破坏现有代码）

1. 中间件**继续注入** `UserContext` 与 `Claims` —— 现有 handler 全部依赖 `Extension<UserContext>`，一个都不能改。
2. `AuthPrincipal` 为**新增** extension，新代码使用；老代码不感知。
3. 服务身份（API Key）没有 `user_id`：任何依赖 `claims.sub` 的老 handler 若被 API Key 调用会缺失 → 必须由 §6.6 的「scope 白名单」限制：**API Key 只能调用开放接口，管理类接口对其 403**。
---

## 6. 鉴权（授权）设计

**本章是本文档的核心。** 结论：权限数据模型已存在，缺的是服务端执行点与路由映射覆盖率。不做这一步就发 API Key，等于把「任何登录用户都能调用的接口」按 key 发给外部系统。

### 6.1 鉴权分层模型

| 层 | 回答的问题 | 技术手段 | 本方案落点 |
|---|---|---|---|
| L1 身份认证 | 你是谁 | JWT / API Key / OAuth 令牌 | `auth.rs` 中间件 |
| L2 功能权限（接口级 RBAC） | 你能调用哪个接口 | `sys_menu.perms` ↔ `(method, http_path)` | `authz.rs` 中间件（6.2–6.4） |
| L3 数据范围权限 | 你能看到哪些行 | 租户 schema + 部门/本人过滤 | `database::with_schema`（已有）+ `data_scope`（新增，6.5） |
| L4 字段级权限 | 你能看到哪些列 | 列裁剪 / 脱敏 | 后续增强（不在本期） |

**常见误区**：只做 L1（现状）会认为「有 token 就能用一切」；只做 L2 会忽略「同租户内 A 能改 B 的资产」。两者都要有。

### 6.2 权限点目录（来自现有种子数据，共 **67** 个 = 62 个三段式 + 5 个二段式）

| 模块 | 权限点 |
|---|---|
| 资产分类 | `asset:category:list` / `add` / `edit` / `delete` |
| 硬件资产 | `asset:hardware:list` / `add` / `edit` / `delete` / `export` |
| 软件资产 | `asset:software:list` / `add` / `edit` / `delete` / `export` |
| 流程-领用 | `process:receive:add` / `approve` / `delete` |
| 流程-调拨 | `process:transfer:list` / `add` / `approve` / `delete` |
| 流程-维修 | `process:repair:add` / `complete` / `delete` |
| 流程-报废 | `process:scrap:list` / `add` / `approve` / `delete` |
| 流程-采购 | `process:purchase:add` / `approve` / `delete` |
| 流程-归还 | `process:return:list` / `add` / `confirm` / `delete` |
| 流程-总览 | `process:all:list`、`process:approval:list`、`process:maintenance:list` |
| 运维采集 | `ops:connection:list` / `create` / `update` / `delete` / `test` / `collect`、`ops:link:bind`、`ops:resource:list` |
| 统计报表 | `statistics:assets:list` / `department` / `license` / `maintenance` / `status` / `export` |
| 系统管理 | `system:user:list`、`system:tenant:list/add/edit/delete`、`system:department:list`、`system:permission:list`、`system:log:list`、`system:numbering:list` |
| 知识库 | `knowledge:list`、`knowledge:chat`、`knowledge:llm:list` |
| 工作流 | `workflow:list`、`workflow:create`、`workflow:history` |

**现存命名不一致（记录，不在本期强改）**：

| 现象 | 例 | 处理 |
|---|---|---|
| 模块名单复数混用 | `asset:*`（单数）vs `ops:*` | 保持历史值不动，新增一律单数 |
| 动词集不统一 | `add/edit`（asset、tenant）vs `create/update`（ops） | 保持历史值不动，新增统一 `add/edit/delete` |
| 只读视角混入 | `statistics:export:list` | 保持，语义为「导出页面可见」 |
| 二段式（缺动作段） | `knowledge:list`、`knowledge:chat`、`workflow:list` / `create` / `history` | 保持历史值；**新增权限点一律强制 `模块:资源:动作` 三段式**，避免再扩散 |

归一化会改动已有角色配置，属**破坏性变更**，因此本期只做「新增权限点统一命名」，历史值靠本文档 + 映射表对齐。

**本期需要新增的权限点**（当前完全缺失）：

| perms | method | http_path | 说明 |
|---|---|---|---|
| `system:apikey:list` | GET | `/api/api-keys` | Key 列表 |
| `system:apikey:add` | POST | `/api/api-keys` | 创建（返回一次性明文） |
| `system:apikey:edit` | POST | `/api/api-keys/{id}/rotate` | 轮换 |
| `system:apikey:delete` | DELETE | `/api/api-keys/{id}` | 吊销 |
| `upload:file:add` | POST | `/api/upload/init` | 上传初始化 |
| `upload:file:add` | POST | `/api/upload/{id}/start` | 开始上传 |
| `upload:file:add` | POST | `/api/upload/{id}/chunk` | 分片上报 |
| `upload:file:add` | POST | `/api/upload/{id}/complete` | 完成合并 |
| `upload:file:add` | POST | `/api/upload/{id}/commit` | 提交关联业务 |
| `upload:file:list` | GET | `/api/upload/{id}/progress` | 查询进度 |
| `upload:file:delete` | DELETE | `/api/upload/{id}` | 中止上传 |
| `chat:completion:add` | POST | `/api/chat/stream` | 智能问答（SSE） |
| `system:oauthclient:list/add/edit/delete` | - | `/api/oauth-clients*` | Phase 2 |

### 6.3 路由 → 权限点映射

**机制**：axum 路由匹配后会把 `MatchedPath`（路由模板，如 `/api/assets/hardware/{id}`）放进请求扩展，其格式与 `sys_menu.http_path` **完全一致**，因此可做「模板 ↔ 模板」等值比对，无需自写路径归一化。

```rust
// crates/api/src/authz.rs（骨架）
pub async fn authz_middleware(req: Request, next: Next) → Result<Response, ApiError> {
    // 匿名路由不经过本中间件（在 lib.rs 中分层挂载）
    let matched = req.extensions().get::<MatchedPath>()
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| ApiError::internal_error("无法解析路由模板"))?;
    let method = req.method().clone();

    let required = perms_for(&method, &matched)            // DashMap 缓存，TTL 60s
        .ok_or_else(|| {
            tracing::error!(%method, %matched, "路由未登记权限点（fail-closed）");
            ApiError::forbidden("该接口未配置访问权限")
        })?;

    let principal = req.extensions().get::<AuthPrincipal>()
        .ok_or_else(|| ApiError::unauthorized("未认证"))?.clone();

    if !principal.allows(&required) {
        tracing::warn!(subject = %principal.subject, %method, %matched, "鉴权失败");
        return Err(ApiError::forbidden_with_scope(&required));
    }
    Ok(next.run(req).await)
}
```

**覆盖率缺口（必须补齐）**：`api` crate 共 105 条 `.route()`（`lib.rs` 99 条），`sys_menu` 中仅 38 条带 `http_path`，即 **61% 的路由没有权限点**。缺失集中在这几组：

| 路由组 | 现状 |
|---|---|
| `/api/users*`、`/api/roles*`、`/api/tenants*`、`/api/departments*` | 只有零星 `list` 权限点，写操作无映射 |
| `/api/process/*` 的多数写操作 | 有 perms 但部分缺 `http_path` |
| `/api/upload/*`（7 条） | 完全没有 |
| `/api/chat/stream` | 完全没有 |
| `/api/auth/registrations*`、`/api/ops/providers`、`/api/ops/connections/{id}` | 没有 |

**三种补录手段（推荐 1 + 3 组合）**：

1. **SQL 种子补录（首选）**：在 `public_initial_data.sql` / `public_migration.sql` 中沿用现有写法（`INSERT ... SELECT ... WHERE NOT EXISTS`）补齐 `http_path` / `http_method` / `perms`。好处：前端权限配置页天然可用，运维可在 UI 调整。
2. **代码内 `ROUTE_PERMS` 兜底表**：`const ROUTE_PERMS: &[(&str, &str, &str)]`，DB 映射缺失时使用，避免上线即 403。
3. **启动自检**：把 `ROUTE_PERMS`、实际挂载的路由、DB 映射三者取并集做 diff，**在 `start_http_server_with()` 内**打印「未受权限保护的路由 N 条」（`AUTHZ_MODE=warn` 时为 WARN），CI 中断言为 0。自检只读代码表与 DB，不 panic，也不影响 Tauri 启动（§16 T-2）。

**豁免机制**（避免为通过自检而乱配）：

```rust
// 显式声明，代码可审计；禁止「缺映射就默认放行」
const AUTH_ONLY_ROUTES: &[(&str, &str)] = &[("GET", "/api/users/me")];        // 仅需登录
const PUBLIC_ROUTES_DECL: &[(&str, &str)] = &[("POST", "/api/auth/login"), ("POST", "/api/auth/refresh")];
```

### 6.4 判定算法与决策矩阵

```rust
impl AuthPrincipal {
    pub fn allows(&self, required: &[String]) → bool {
        if self.is_super_admin { return true; }              // 超管全通过
        required.iter().all(|need| self.scopes.iter().any(|have| matches(have, need)))
    }
}

// 通配规则：* 与尾部 :* ；精确匹配优先；通配仅 API Key 与超管可持有
fn matches(have: &str, need: &str) → bool {
    have == "*" || have == need
        || (have.ends_with(":*") && need.starts_with(&have[..have.len() - 1]))
}
```

**决策矩阵**：

| 主体 | 开放接口（需 `asset:hardware:list`） | 管理接口（需 `system:user:add`） | 未映射路由 |
|---|---|---|---|
| 超管 JWT | 通过 | 通过 | 403（fail-closed，先补映射） |
| 普通用户 JWT | 角色含该 perms 才通过 | 角色含该 perms 才通过 | 403 |
| API Key（scopes=`asset:hardware:list`） | 通过 | 403 | 403 |
| API Key（scopes 为空） | 403（创建时禁止空 scopes） | 403 | 403 |
| 匿名 | 401 | 401 | 401 |

**为什么 fail-closed**：漏配权限的后果是「功能不可用」（可监控、可快速修复），fail-open 的后果是「数据泄露」（不可逆）。
### 6.5 数据级鉴权（L3）

#### 6.5.1 租户隔离（已有，需明确边界）

- 机制：认证阶段解析 `schema_name`，通过 `database::with_schema` 写入 task_local，后续所有查询自动落到租户 schema。这是**最强的隔离**（物理表分离），但只在「所有 SQL 都经过同一连接入口」时成立。
- 边界与要求：
  1. **禁止**在业务代码里手写 `SET search_path` 或跨 schema 的 `public.` 前缀查询租户表；
  2. `public.sys_*` 是共享表：任何对共享表的写操作都必须显式带 `tenant_id` 条件（API Key 表即按此设计）；
  3. 超管跨租户查看（若需要）必须是**独立权限点** `system:tenant:switch` + 显式请求头 `X-Tenant-Id` + 审计日志，而不是「超管自动跨租户」。
- 现状风险：`auth_middleware` 每个请求都调用 `database::set_global_schema()`（`auth.rs:149`），与 Tauri 命令模式的 `GLOBAL_SCHEMA` 回退路径并存 → 并发请求互相覆盖（详见 `docs/多租户数据权限与租户切换设计方案.md`「一、核心问题」）。HTTP 路径应**只走 task_local**（`with_schema`），`GLOBAL_SCHEMA` 仅供 Tauri 命令模式使用，两者不得混写。

#### 6.5.2 部门 / 本人级（新增 L3 能力）

资产表已有归属字段（如 `department_id`、`user_id`），但没有按归属过滤。建议给角色增加数据范围：

```sql
ALTER TABLE public.sys_role ADD COLUMN IF NOT EXISTS data_scope SMALLINT NOT NULL DEFAULT 1;
COMMENT ON COLUMN public.sys_role.data_scope IS '1=全部数据 2=本部门 3=本部门及下级 4=仅本人 5=自定义部门';

CREATE TABLE IF NOT EXISTS public.sys_role_dept (
  role_id BIGINT NOT NULL,
  dept_id BIGINT NOT NULL,
  PRIMARY KEY (role_id, dept_id)
);
```

**过滤规则**（在 service 层统一构造，禁止每个 handler 各写一套）：

```rust
// service 层统一入口：返回可直接拼进 WHERE 的片段与绑定参数
pub struct DataScope { pub sql: String, pub binds: Vec<i64> }

pub fn build_scope(user_id: i64, dept_id: Option<i64>, roles: &[RoleScope]) → DataScope {
    // 多角色取并集：范围最大的角色生效
    // 1 全部  → sql: "1=1"
    // 2 本部门 → sql: "department_id = $n"
    // 3 及下级 → sql: "department_id IN (WITH RECURSIVE d AS (...) SELECT id FROM d)"
    // 4 仅本人 → sql: "user_id = $n"
    // 5 自定义 → sql: "department_id = ANY($n)"
}
```

**典型用法**（列表接口，注意必须是 SQL 层过滤，不能查全量再筛）：

```sql
SELECT id, asset_code, department_id FROM asset_hardware
WHERE deleted = 0 AND ({data_scope_sql})
ORDER BY id DESC LIMIT $1 OFFSET $2;
```

#### 6.5.3 IDOR / 水平越权防护（必做）

现状：`PUT /api/assets/hardware/{id}`、`DELETE .../{id}` 等直接按主键操作，同租户内任意用户可改任意记录。补齐方式：

| 手段 | 说明 |
|---|---|
| 写入/删除前二次校验 | `UPDATE ... WHERE id = $1 AND deleted = 0 AND ({scope_sql})`，`rows_affected == 0` 时返回 404（**不返回 403**，避免暴露记录存在性） |
| 下载/导出签名 | 一次性签名 URL，TTL 5 分钟，绑定 `subject`，不把对象存储路径暴露给前端 |
| 列表接口 | 只能返回 scope 内数据；**禁止**「前端按部门下拉筛选」当作权限 |
| 关联查询 | 通过 `XX_id` 关联其他业务表时要同样加 scope 条件，避免「A 能通过 B 的 id 读到 C」 |

### 6.6 API Key 的授权约束（防提权）

| 约束 | 规则 | 违反时 |
|---|---|---|
| scopes 非空 | 至少 1 个，最多 50 个，去重 | 400 |
| **子集约束** | `scopes ⊆ 创建者当前 perms 并集`，且 ⊆ `租户可用权限点` | 400 `scope 超出创建者权限` |
| 禁止自举 | 不得包含 `system:apikey:*`、`system:user:add`、`system:tenant:*` 等高危管理点 | 400 |
| 禁止用户态接口 | 需要 `claims.sub` 的接口（个人中心、我的待办、审批人本人操作）对 API Key 一律 403 | 403 |
| 租户绑定 | key 属于创建时租户，**不可跨租户**；`tenant_id` 为空即 `public` | 403 |
| 速率限制 | 每个 key 默认 600 次/分钟（可配），超限 429 + `Retry-After` | 429 |
| 有效期 | 默认 90 天，最长 365 天；到期前 7 天在管理页告警 | - |

**scope 模板（降低配置错误）**：

| 模板 | scopes | 场景 |
|---|---|---|
| 只读集成 | `asset:hardware:list`、`asset:software:list`、`asset:category:list`、`statistics:assets:list` | ERP 拉取资产台账 |
| 工单写入 | 只读 + `process:receive:add`、`process:return:add`、`process:repair:add` | OA 提交单据 |
| 采集上报 | `ops:connection:collect`、`ops:link:bind`、`ops:resource:list` | 运维平台回写资源 |

**为什么禁止 API Key 管理 API Key**：否则一个被泄露的 key 可以给自己（或新建 key）续命与扩权，撤销就失效了。

### 6.7 执行点与失败语义

| 场景 | 状态码 | `WWW-Authenticate` | 响应体 |
|---|---|---|---|
| 缺凭证 | 401 | `Bearer realm="assets-platform"` | `{code:401,message:"未登录"}` |
| 凭证无效/过期/已撤销 | 401 | `Bearer realm="...", error="invalid_token"` | `{code:401,message:"凭证无效或已过期"}` |
| 权限不足 | 403 | `Bearer realm="...", error="insufficient_scope", scope="asset:hardware:list"` | `{code:403,message:"权限不足",scope:[...]}` |
| 未映射路由 | 403 | 同上（scope 为空） | `{code:403,message:"该接口未配置访问权限"}` |
| URI query 传令牌 | 400 | - | `{code:400,message:"令牌不得通过 URL 参数传递"}` |
| 超限 | 429 | - | `{code:429,message:"请求过于频繁"}` |

细节：
1. **保留现有 `{code,message}` 结构**（前端已按此解析），403 仅**新增** `scope` 字段；OAuth 端点（`/oauth/*`）例外，使用规范要求的 `{error,error_description}`（见 §7.2）。
2. **SSE（`/api/chat/stream`）**：鉴权失败必须在**建立流之前**返回 JSON + 403，不能先返回 `text/event-stream` 再说错误，否则前端无法区分。
3. 401 不区分失败原因，403 明确给出所需 scope（便于集成方自查，且不泄露数据信息）。

### 6.8 权限缓存与失效

| 缓存 | Key | TTL | 失效触发 |
|---|---|---|---|
| 用户 perms 并集 | `user_id` | 60s | 改角色 / 改角色菜单 / 禁用用户 / 登出 |
| 路由 → perms 映射 | `(method, path)` | 60s | 启动加载 + 管理端保存权限后主动清空 |
| API Key 校验结果 | `key_hash` | 30s | 吊销 / 轮换时**立即**删除该条 |

- 全部用 `DashMap`（项目已依赖），无需引入 Redis（单实例部署）。
- 明确「撤销延迟上限」：用户权限变更 ≤ 60s，API Key 吊销 ≤ 1s。该承诺写进运维文档。
- 缓存未命中回源 DB；回源失败时**不得**降级为放行。

### 6.9 鉴权审计与告警

- 记录字段：`principal.subject`、`kind`、`route`、`method`、`perms`、`result`、`ip`、`ua`、`latency_ms`、`trace_id`。
- 落库策略：成功请求抽样（1%），**失败请求 100% 落库**（写入现有 `sys_log`）。
- 告警：401 激增（可能被扫）、403 激增（权限配置错误或探测）、query 传令牌命中（老客户端/攻击）、超管跨租户操作、key 即将过期。
- **禁止**在任何日志中输出完整 key / token（仅 `mask_api_key` 后的定位段）。

### 6.10 反模式清单（评审时逐条对照）

| 反模式 | 后果 | 本方案对策 |
|---|---|---|
| 前端隐藏按钮当作授权 | 直接调 API 即可越权 | 服务端 `authz.rs` 强制 |
| 分散的 `is_super_admin` 特判 | 逻辑不一致、遗漏 | 统一到 `AuthPrincipal.is_super_admin`，仅此一处 |
| 通配 scope 滥用（`*`） | 违反最小权限 | 仅超管可持 `*`；key 禁止 |
| 可逆加密存外部 key | DB 泄露即全量还原 | SHA-256 单向哈希 |
| key / token 放 URL | 落日志、Referer 泄露 | 2.1 禁止，直接 400 |
| `allow_origin(Any)` + 浏览器带 key | 任意站点可盗用 | 收紧 CORS 白名单（§14 R4） |
| 缺映射默认放行 | 静默越权 | fail-closed + 启动自检 |
---

## 7. OAuth 2.1 授权服务器（Phase 2，最小可用）

### 7.1 端点清单

| 端点 | 方法 | 认证 | 说明 |
|---|---|---|---|
| `/oauth/authorize` | GET | 需交互登录态 | 授权码签发（PKCE 强制） |
| `/oauth/token` | POST | 客户端（`client_secret_basic` 或 `none`+PKCE） | code 交换 / refresh 轮换 |
| `/oauth/introspect` | POST | 客户端 | 令牌内省（RFC 7662），供资源服务校验 |
| `/oauth/revoke` | POST | 客户端 | 撤销（RFC 7009） |
| `/.well-known/oauth-authorization-server` | GET | 无 | AS 元数据（RFC 8414） |
| `/api/oauth-clients*` | - | 管理端 JWT + perms | 客户端注册管理（不走动态注册） |

### 7.2 错误响应规范（与现有 ApiResponse 隔离）

```json
{ "error": "invalid_request", "error_description": "code_verifier 与 code_challenge 不匹配" }
```

错误码取值：`invalid_request`、`invalid_client`（401）、`invalid_grant`、`unauthorized_client`、`unsupported_grant_type`、`unsupported_response_type`、`invalid_scope`、`access_denied`、`server_error`、`invalid_token`。

**实现要求**：`/oauth/*` 路由单独挂一个 `oauth_error` 响应类型，**不得**复用 `crates/api/src/response.rs` 的 `{code,message}`，否则第三方 OAuth 客户端库无法解析。

### 7.3 `/oauth/authorize` 与「交互登录」难题

标准规定该端点需要**已登录的用户会话**（HTTP Cookie），而本项目目前**只有 Bearer JWT，没有会话 Cookie**。两条路：

| 方案 | 做法 | 合规性 | 工作量 |
|---|---|---|---|
| A. Bearer + 同意页 | 前端携带 JWT 调 `/oauth/authorize`，服务端校验 JWT 后渲染/返回同意页，用户确认后签发 code | 非标准（依赖客户端已登录），适合**内部应用** | 小 |
| B. 会话 Cookie | 新建 `sys_user_session`（httpOnly + SameSite=Lax + Secure），登录时写 Cookie，authorize 走浏览器重定向 | 标准 | 大（要改登录流程、CSRF 防护、会话管理） |

**建议**：Phase 2.1 用方案 A（先让内部/子公司系统接入），Phase 2.2 需接外部第三方时再上方案 B。文档需明确标注：方案 A 下 `/oauth/authorize` **不是标准浏览器重定向流程**。

**参数校验（顺序敏感，先校验再做任何重定向）**：

| 参数 | 规则 |
|---|---|
| `client_id` | 必须存在且 `enable` |
| `redirect_uri` | **精确匹配** `sys_oauth_client.redirect_uris` 中某一条；不匹配则**不得重定向**，直接返回错误页 |
| `response_type` | 必须 `code`；`token`/`id_token` 返回 `unsupported_response_type` |
| `code_challenge` | 必填；`code_challenge_method` 必须 `S256`（缺省视为 `S256`，拒绝 `plain`） |
| `state` | 必填（CSRF），长度 ≤ 512 |
| `scope` | 必填，且 `scope ⊆ client.allowed_scopes`；空 scope 拒绝 |
| `nonce` | 本项目无 OIDC，忽略 |

**流程**：校验参数 → 校验登录态与用户状态 → 生成同意页（展示 client 名称与 scope 中文说明）→ 用户同意 → 生成 `code`（32 随机字节，**只存 SHA-256**，TTL 60s，一次性）→ 302 到 `redirect_uri?code=...&state=...&iss=<issuer>`。

### 7.4 `/oauth/token`

请求（`application/x-www-form-urlencoded`）：

| 字段 | code 交换 | refresh 轮换 |
|---|---|---|
| `grant_type` | `authorization_code` | `refresh_token` |
| `code` | 必填 | - |
| `redirect_uri` | 必填且与授权时**完全一致** | - |
| `code_verifier` | 必填（长度 43–128，`[A-Za-z0-9-._~]`） | - |
| `refresh_token` | - | 必填 |
| `client_id` / `client_secret` | confidential 客户端必填 | 同左 |
| `scope` | 可选，只能**收窄** | 可选，只能收窄 |

校验要点：
1. `SHA-256(code_verifier)` 与 `code_challenge` 比较（**常数时间比较**）；失败即 `invalid_grant`，并**作废该 code**（防爆破）。
2. `code` 一次性：取出后立即标记 `used_at`，重放返回 `invalid_grant` 并撤销其签发的令牌族。
3. `refresh_token` 直接复用现有 `family_id` 轮换 + 重放整族撤销逻辑（`user_service.rs:167-360`）。
4. 响应：`{access_token, token_type:"Bearer", expires_in:900, refresh_token, scope}`，响应头 `Cache-Control: no-store`、`Pragma: no-cache`。

### 7.5 令牌格式决策

| 方案 | 优点 | 缺点 | 结论 |
|---|---|---|---|
| Opaque（`at_` + 内省缓存） | 可即时撤销、无密钥分发 | 资源服务需调用内省（本项目资源服务就是自己，无成本） | **采用** |
| JWT access token | 离线验签 | 撤销困难（需短 TTL + 黑名单） | 备选 |

同时保留现有用户 JWT 不变（前端仍用它），OAuth 令牌仅供第三方应用调用 API 使用。

### 7.6 客户端与密钥

- public client（桌面/SPA）：无 `client_secret`，强制 PKCE。
- confidential client（服务端）：`client_secret_hash = SHA-256(secret)`，基本认证方式 `client_secret_basic`。
- `redirect_uris` 存**数组**（JSONB 或 `text[]`），逐条精确匹配。
- **loopback 特例**：Tauri 桌面端回调端口随机，按 RFC 8252 §7.3，登记 `http://127.0.0.1/callback` 时忽略端口比较；实现中单独一个函数 `matches_redirect()` 处理并写注释说明理由，**不要**污染主流程的严格比较。

### 7.7 Phase 2 落地说明（实现与本文档的差异）

1. **方案 A 的交互形态**：`GET /oauth/authorize` 返回 JSON 同意信息（`client_name` / `scopes` / `state` / `issuer`），`POST /oauth/authorize` 提交 `approve` 后返回 `{ redirect_to }`，由调用方完成 302。**不返回 HTML 同意页、也不做浏览器重定向**：Bearer JWT 无法随浏览器跳转携带，这是方案 A 的固有限制（接外部第三方前需补方案 B）。
2. **端点开关**：`/oauth/authorize`、`/oauth/token`、`/oauth/introspect`、`/oauth/revoke`、`/.well-known/oauth-authorization-server` 仅在 `OAUTH_ENABLED=true`（或 `OAUTH_ALLOW_INSECURE_HTTP=true`）时挂载，缺省 404（§16 T-7）。`/api/oauth-clients*` 管理接口不受该开关影响（权限点仍为 `system:oauthclient:*`）。
3. **issuer**：`OAUTH_ISSUER` 缺省 `http://localhost:<API_PORT>`；启用且为明文 HTTP 时启动日志打印 WARN 横幅（§8.2）。
4. **`family_id` 读写**：DDL 采用 `UUID`，但 sqlx 未启用 `uuid` feature，读写分别用 `$n::uuid` 与 `family_id::text`，无需新增依赖。
5. **OAuth 主体**：`PrincipalKind::OAuth`，`subject = "u:<user_id>@client:<client_id>"`，只注入 `AuthPrincipal`（不注入 `UserContext` / `Claims`）；`authz` 对非用户主体始终 enforce，`system:*` / `upload:*` 与用户态接口一律 403。
6. **重放处置**：刷新令牌重放 → 撤销整个 `family_id`；授权码重放 → 撤销该 `client_id + user_id` 的全部未撤销令牌；PKCE 校验失败 → 立即把该 code 置 `used_at`。
7. **清理任务**：`oauth_service::cleanup_expired_grants()`（授权码保留 24h、令牌过期后保留 30d）供定时任务调用，不挂在请求路径上。

---

### 7.8 Phase 2.2 落地：方案 B（浏览器会话 Cookie）+ 传输护栏

**新增数据表**（`public_migration.sql`，幂等）：

| 表 | 关键列 | 说明 |
|---|---|---|
| `sys_user_session` | `session_hash`、`user_id`、`tenant_id`、`csrf_hash`、`ip inet`、`user_agent`、`expires_at`、`revoked_at` | 只存 `SHA-256`，明文只在 `Set-Cookie` 出现一次；`uk_sys_user_session_hash` + 用户 / 过期索引 |

**Cookie 规格**：

| Cookie | 前缀 | 属性 | 用途 |
|---|---|---|---|
| `ap_session` | `ss_<43>` | `HttpOnly; SameSite=Lax; Path=/; Max-Age=SESSION_TTL_SECS`（TLS 下加 `Secure`） | 浏览器授权流程的登录态 |
| `ap_csrf` | `cs_<43>` | `SameSite=Lax; Path=/`（**不加 HttpOnly**） | 双提交 CSRF 校验 |

**`/oauth/authorize` 双流程分流**（同一 URL，handler 内判定）：

| 请求特征 | 分支 | 响应 |
|---|---|---|
| `Authorization: Bearer <用户 JWT>` | 方案 A | JSON 同意信息 / `{redirect_to}`（与 2.1 行为一致） |
| 无 Bearer，带 `ap_session` | 方案 B | `GET` 渲染 HTML 同意页；`POST` 校验 CSRF 后 **302** 回调 |
| 无 Bearer，无会话 | 方案 B | `401` HTML「请先登录」 |

- 授权端点因此从「受保护路由」移到**公开路由**：方案 A 在 handler 内自行校验 Bearer（API Key / `at_` 令牌一律 `access_denied`），方案 B 靠 Cookie；
- 方案 B 的 `POST` 顺序：CSRF 双提交校验（常数时间）→ 会话校验 → `ensure_user_active` → `prepare_authorization`（含 `redirect_uri` 精确匹配）→ 签发 code 并 302；拒绝授权时跳 `?error=access_denied&state=...`，**不泄漏**任何额外信息；
- 同意页为纯静态 HTML（内联样式、无脚本），所有插值经 HTML 转义，并附 `CSP: default-src 'none'; style-src 'unsafe-inline'; form-action 'self'` 与 `X-Frame-Options: DENY`、`Cache-Control: no-store`。

**会话生命周期**：

| 时机 | 行为 |
|---|---|
| `POST /api/auth/login` | 追加 `Set-Cookie`（会话 + CSRF）；签发失败只记 WARN，不影响 JWT 登录 |
| `POST /api/auth/logout` | 追加清空 Cookie；撤销会话（幂等） |
| `GET /api/auth/session` | 用 Cookie 返回 `{user_id, tenant_id, expires_at}`（无会话 401） |
| `POST /api/auth/session/logout` | 撤销会话 + 清 Cookie |
| 改密 / 删除用户 | `revoke_all_user_sessions(user_id)` 撤销该用户全部会话 |
| 定时任务 | `cleanup_expired_sessions()` 每小时删除「过期超过 7 天」的行（保留期便于审计） |

**传输与运维护栏**：

| 项 | 行为 |
|---|---|
| `OAUTH_TRUSTED_CIDRS` | 非空时 `/oauth/*`、`/api/auth/session*` 只允许清单内来源；**配置了清单但取不到 IP 一律拒绝**（fail-closed），清单为空 = 不限制 |
| `TRUST_PROXY_HEADERS` | 只有为 `true` 才采信 `X-Forwarded-For` / `X-Real-IP` / `X-Forwarded-Proto`；否则用 TCP 对端地址（`into_make_service_with_connect_info`） |
| 安全响应头 | 全局 `X-Content-Type-Options: nosniff`、`X-Frame-Options: DENY`、`Referrer-Policy: no-referrer`；HTTPS 下追加 `Strict-Transport-Security: max-age=31536000; includeSubDomains`（`HSTS_ENABLED=false` 可关闭） |
| `GET /healthz` | 公开存活探测：`SELECT 1`（2s 超时）；正常 `200`，数据库不可达 `503`；附带 `insecure_transport`（§8.2 补偿控制 5） |
| 后台清理 | 启动后 60s 首次执行，之后每 `OAUTH_CLEANUP_INTERVAL_SECS`（默认 3600s）清理授权码 / 令牌 / 会话；失败只记 WARN |

**新增环境变量**：`SESSION_TTL_SECS`、`OAUTH_TRUSTED_CIDRS`、`TRUST_PROXY_HEADERS`、`HSTS_ENABLED`、`OAUTH_CLEANUP_INTERVAL_SECS`。全部有安全缺省值，不配置也能启动。

### 7.9 JWT 加固：`iss` / `aud` 校验与迁移

- 签发的 access / refresh 令牌新增 `iss`（`JWT_ISSUER`，默认 `assets-platform`）与 `aud`（`JWT_AUDIENCE`，默认 `assets-platform-api`）；
- 校验规则：**存在即校验**（与期望值不符 → 401「令牌签发方/受众不匹配」）；缺失时由 `JWT_STRICT_ISS_AUD` 决定 —— `false`（默认）放过历史令牌，`true` 强制要求；
- 迁移步骤：上线本版 → 等待现有令牌自然过期（access 7 天 / refresh 30 天，或提前缩短 `JWT_ACCESS_TTL_SECS`）→ 设 `JWT_STRICT_ISS_AUD=true` 完成严格化；
- 校验点：`api::auth::verify_token()`（资源请求）与 `service::user_service::refresh_tokens()`（续期）两处，未覆盖的路径不会签出新令牌。

---

## 8. HTTP / TLS 落地方案

### 8.1 三方案对比

| 方案 | 做法 | 合规 | 代价 |
|---|---|---|---|
| ① 反向代理终止 TLS（推荐） | Nginx/Caddy 持有证书，后端只监听 `127.0.0.1:xxxx`，代理注入 `X-Forwarded-Proto` | ✅ 满足 2.1 | 部署多一层；需正确配置可信代理头 |
| ② 进程内 rustls | `axum-server` + rustls，证书由部署方提供 | ✅ | 新增依赖与证书管理；Tauri 内嵌场景需处理证书路径 |
| ③ 内网 HTTP 例外 | 明文，仅内网可达 | ❌ **不合规**，必须显式声明 + 补偿控制 | 见 8.2 |

### 8.2 方案 ③ 的补偿控制（若短期必须用 HTTP）

1. 绑定 `127.0.0.1` 或内网网段，**默认不监听 `0.0.0.0`**；
2. 环境变量 `OAUTH_ALLOW_INSECURE_HTTP=true` 才能启动 AS；启动日志打印 **WARN 级横幅**；
3. `introspect` / `token` 端点加 IP 白名单（`OAUTH_TRUSTED_CIDRS`）；
4. API Key 不允许在明文环境创建（管理接口返回 400 并提示原因）；
5. 文档与运维手册中标注「非生产可用」，并在 `/healthz` 响应里暴露 `insecure_transport: true` 以便巡检发现。

### 8.3 响应头与 CORS

| 头 | 值 |
|---|---|
| `Strict-Transport-Security` | `max-age=31536000; includeSubDomains`（仅 TLS 下） |
| `Cache-Control` | 令牌端点 `no-store` |
| `X-Content-Type-Options` | `nosniff` |
| 现有 CORS | 从 `allow_origin(Any)` 收紧为配置化白名单，且 `allow_credentials(false)` |

---

## 9. DDL 汇总（`public_migration.sql` 追加，幂等）

### 9.1 API Key

```sql
CREATE TABLE IF NOT EXISTS public.sys_api_key (
  id BIGINT PRIMARY KEY,                    -- Snowflake（与现有表一致，应用层 utils::snowflake::next_id 生成）
  tenant_id BIGINT,
  name VARCHAR(100) NOT NULL,
  description VARCHAR(500),
  key_prefix VARCHAR(32) NOT NULL,          -- 定位段，明文，用于展示与日志
  key_hash CHAR(64) NOT NULL,               -- SHA-256(完整 key) hex
  scopes TEXT[] NOT NULL DEFAULT '{}',
  rate_limit_per_min INT NOT NULL DEFAULT 600,
  expires_at TIMESTAMPTZ,
  last_used_at TIMESTAMPTZ,
  revoked_at TIMESTAMPTZ,
  created_by BIGINT,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_by BIGINT,
  updated_at TIMESTAMPTZ DEFAULT now(),
  enable BOOLEAN NOT NULL DEFAULT TRUE,
  deleted SMALLINT NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX IF NOT EXISTS uk_sys_api_key_hash ON public.sys_api_key (key_hash);
CREATE INDEX IF NOT EXISTS idx_sys_api_key_tenant ON public.sys_api_key (tenant_id, enable, deleted);
CREATE INDEX IF NOT EXISTS idx_sys_api_key_prefix ON public.sys_api_key (key_prefix);
```

> 命名与现有表一致：`enable BOOLEAN`、`deleted SMALLINT`（0/1）、`created_by` / `created_at` / `updated_by` / `updated_at`；主键 `id BIGINT` 由应用层 Snowflake 生成（`utils::snowflake::next_id`），**不使用 `BIGSERIAL`**，与 `sys_user` / `sys_menu` 保持一致。

### 9.2 OAuth

```sql
CREATE TABLE IF NOT EXISTS public.sys_oauth_client (
  id BIGINT PRIMARY KEY,                    -- Snowflake
  client_id VARCHAR(64) NOT NULL,
  client_name VARCHAR(100) NOT NULL,
  client_secret_hash CHAR(64),              -- public client 为 NULL
  client_type SMALLINT NOT NULL DEFAULT 1,  -- 1=confidential 2=public
  redirect_uris TEXT[] NOT NULL DEFAULT '{}',
  allowed_scopes TEXT[] NOT NULL DEFAULT '{}',
  grant_types TEXT[] NOT NULL DEFAULT '{authorization_code,refresh_token}',
  access_token_ttl INT NOT NULL DEFAULT 900,
  refresh_token_ttl INT NOT NULL DEFAULT 2592000,
  tenant_id BIGINT,
  enable BOOLEAN NOT NULL DEFAULT TRUE,
  deleted SMALLINT NOT NULL DEFAULT 0,
  created_by BIGINT,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_by BIGINT,
  updated_at TIMESTAMPTZ DEFAULT now()
);
CREATE UNIQUE INDEX IF NOT EXISTS uk_sys_oauth_client_id ON public.sys_oauth_client (client_id);

CREATE TABLE IF NOT EXISTS public.sys_oauth_code (
  id BIGINT PRIMARY KEY,                    -- Snowflake
  code_hash CHAR(64) NOT NULL,
  client_id VARCHAR(64) NOT NULL,
  user_id BIGINT NOT NULL,
  tenant_id BIGINT,
  redirect_uri TEXT NOT NULL,
  scopes TEXT[] NOT NULL DEFAULT '{}',
  code_challenge VARCHAR(128) NOT NULL,
  code_challenge_method VARCHAR(10) NOT NULL DEFAULT 'S256',
  expires_at TIMESTAMPTZ NOT NULL,
  used_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ DEFAULT now()
);
CREATE UNIQUE INDEX IF NOT EXISTS uk_sys_oauth_code_hash ON public.sys_oauth_code (code_hash);

CREATE TABLE IF NOT EXISTS public.sys_oauth_token (
  id BIGINT PRIMARY KEY,                    -- Snowflake
  token_hash CHAR(64) NOT NULL,             -- access / refresh 都只存哈希
  token_type SMALLINT NOT NULL,             -- 1=access 2=refresh
  client_id VARCHAR(64) NOT NULL,
  user_id BIGINT,
  tenant_id BIGINT,
  scopes TEXT[] NOT NULL DEFAULT '{}',
  family_id UUID NOT NULL,
  expires_at TIMESTAMPTZ NOT NULL,
  revoked_at TIMESTAMPTZ,
  replaced_by VARCHAR(64),
  created_at TIMESTAMPTZ DEFAULT now()
);
CREATE UNIQUE INDEX IF NOT EXISTS uk_sys_oauth_token_hash ON public.sys_oauth_token (token_hash);
CREATE INDEX IF NOT EXISTS idx_sys_oauth_token_family ON public.sys_oauth_token (family_id);
```

清理任务：`sys_oauth_code` 保留 24h、`sys_oauth_token` 过期后保留 30d 供审计。

---

## 10. 接口清单变更

### 10.1 新增（Phase 1）

| 方法 | 路径 | 认证 | perms |
|---|---|---|---|
| POST | `/api/api-keys` | JWT | `system:apikey:add` |
| GET | `/api/api-keys` | JWT | `system:apikey:list` |
| POST | `/api/api-keys/{id}/rotate` | JWT | `system:apikey:edit` |
| DELETE | `/api/api-keys/{id}` | JWT | `system:apikey:delete` |

请求/响应示例：

```json
POST /api/api-keys
{ "name": "ERP 集成", "description": "财务系统只读对接", "scopes": ["asset:hardware:list","asset:software:list"], "expires_in_days": 90 }

201
{ "code":0, "data": { "id": 12, "api_key": "ak_live_7f3c9a1b2d4e6f80_Xq9...43chars", "key_prefix": "7f3c9a1b2d4e6f80", "scopes": ["asset:hardware:list","asset:software:list"], "expires_at": "2027-02-08T00:00:00Z" } }
```

### 10.2 行为变更（风险点，需同步前端）

| 路由 | 变更 |
|---|---|
| `/api/upload/*`（7 条） | 从「匿名可访问」改为「必须登录且含 `upload:file:add`」 |
| `/api/chat/stream` | 从「匿名」改为「必须登录且含 `chat:completion:add`」 |
| 全部受保护路由 | 未授权统一 403 `{code:403,message:"权限不足",scope:[...]}` |

---

## 11. 安全需求对照（RFC 9700 / draft 2.1）

| 要求 | 出处 | 本方案 |
|---|---|---|
| 所有 code 流强制 PKCE | 2.1 §PKCE | ✅ 必填 + S256 |
| 禁止 implicit / password | 2.1 | ✅ 不实现 |
| 重定向 URI 精确匹配 | 2.1 / RFC 9700 §2.1 | ✅ 逐条等值（loopback 端口特例单列） |
| 令牌不得出现在 URI | RFC 6750 §2.3 / 2.1 | ✅ 400 + 告警 |
| refresh 轮换 + 重放检测 | RFC 9700 §4.14 | ✅ 复用 `family_id` |
| 授权码一次性、短 TTL、绑定 PKCE 与 redirect_uri | RFC 9700 §4.5 | ✅ 60s + 哈希存储 |
| 客户端认证：public 无 secret | 2.1 | ✅ `client_type` 区分 |
| TLS ≥ 1.2 | 2.1 §Transport | ⚠️ §8（反代方案满足） |
| 令牌存储：只存哈希 | RFC 9700 | ✅ `CHAR(64)` |
| 速率限制 | RFC 9700 §4.14 | ✅ 每 key / 每 IP |
| 审计日志 | RFC 9700 | ✅ §6.9 |

---

## 12. 实施计划

### 12.1 Phase 0：安全基座（1–2 天，**必须先做**）

1. `JWT_SECRET` 校验**只加在 `api::start_http_server_with()` 入口**（缺失或 < 32 字节 → 返回 `Err`，仅 HTTP 服务不启动）。**不改** `auth.rs:43` / `user_service.rs:76` / `src/commands/user_commands.rs:116` 的默认值兜底——这三处 Tauri 也在用，见 §16 T-2。
2. 把 `/api/upload/*`、`/api/chat/stream` 移入 `auth_middleware` 内（先只要求登录，不加 perms，避免前端直接坏）。
3. 删除 `auth.rs:149` 的 `database::set_global_schema()` 调用：HTTP 侧不再写全局变量（当前每次 HTTP 请求都会覆盖 Tauri 会话的 schema）。`GLOBAL_SCHEMA` 本身**保留**，`lib.rs:148` 与 Tauri `switch_tenant` 命令照旧使用，语义不变，见 §16 T-3。
4. 新增 `utils/src/api_key.rs` 与 `sha2` 依赖（`utils/Cargo.toml`）。

### 12.2 Phase 1：API Key + 鉴权执行点（5–8 天）

| # | 任务 | 文件 |
|---|---|---|
| 1 | DDL：`sys_api_key` + 权限点补录 | `sql/public_migration.sql`、`public_initial_data.sql` |
| 2 | 模型与 CRUD | `database/src/models.rs`、`service/src/api_key_service.rs`（新） |
| 3 | 凭证分流（`Credential` 枚举） | `api/src/auth.rs` |
| 4 | 认证中间件支持 API Key | `api/src/auth.rs` |
| 5 | `AuthPrincipal` + `perms_for` 缓存 | `api/src/authz.rs`（新） |
| 6 | 路由 → perms 映射补录（105 条全覆盖） | `public_initial_data.sql` |
| 7 | 启动自检 + 未覆盖路由告警 | `api/src/authz.rs`（自检函数）、`api/src/lib.rs` 的 `start_http_server_with()`（**在 HTTP 启动路径调用，不放 `src/main.rs`**，见 §16 T-2） |
| 8 | Key 管理接口 + OpenAPI | `api/src/api_key_routes.rs`（新）、`apidoc` |
| 9 | 前端 Key 管理页 | 现有 settings 页扩展 |

### 12.3 Phase 2：OAuth 2.1 AS（5–8 天）

| # | 任务 | 说明 |
|---|---|---|
| 1 | DDL：`sys_oauth_client` / `sys_oauth_code` / `sys_oauth_token` | §9.2 |
| 2 | 客户端管理接口 | `api/src/oauth_routes.rs` |
| 3 | `/oauth/authorize`（方案 A：Bearer + 同意页） | 先内部可用 |
| 4 | `/oauth/token`（code + refresh） | 复用轮换逻辑 |
| 5 | `/oauth/introspect`、`/oauth/revoke`、metadata | RFC 7662 / 7009 / 8414 |
| 6 | `at_` 令牌接入 `auth.rs` | 与 API Key 同管线 |
| 7 | 传输护栏（§8） | 环境开关 + WARN 横幅 |
| 8 | 可选：会话 Cookie（方案 B） | 接第三方时再做 |

---

## 13. 测试与验收

### 13.1 功能验收（curl）

```bash
# API Key 调用开放接口
curl -H "X-API-Key: ak_live_..." http://127.0.0.1:8080/api/assets/hardware?page=1

# 越权：用只读 key 调删除
curl -X DELETE -H "X-API-Key: ak_live_..." http://127.0.0.1:8080/api/assets/hardware/1
# 期望 403 {"code":403,"message":"权限不足","scope":["asset:hardware:delete"]}

# 禁止 URI 传令牌
curl "http://127.0.0.1:8080/api/assets/hardware?access_token=xxx"
# 期望 400

# 吊销后立即失效
curl -X DELETE -H "Authorization: Bearer $JWT" http://127.0.0.1:8080/api/api-keys/12
curl -H "X-API-Key: ak_live_..." http://127.0.0.1:8080/api/assets/hardware
# 期望 401
```

### 13.2 鉴权测试矩阵

| # | 场景 | 期望 |
|---|---|---|
| 1 | 无凭证调受保护接口 | 401 |
| 2 | 过期 JWT | 401 `invalid_token` |
| 3 | refresh 令牌当 access 用 | 401 |
| 4 | 普通用户调未授权接口 | 403 + scope |
| 5 | 普通用户调未映射路由 | 403（fail-closed） |
| 6 | 超管调任意已映射接口 | 200 |
| 7 | API Key 调管理接口 | 403 |
| 8 | API Key scopes 为空 | 创建即 400 |
| 9 | 创建 key 时 scope 超出创建者权限 | 400 |
| 10 | 租户 A 的 key 访问租户 B 数据 | 403 / 查无此记录 |
| 11 | 部门范围用户读其他部门资产 | 列表不返回该行 |
| 12 | 部门范围用户改其他部门资产 | 404 |
| 13 | 权限缓存：改角色后 60s 内生效 | 生效 |
| 14 | key 吊销后 1s 内失效 | 401 |
| 15 | SSE 未授权 | 403 JSON（非 event-stream） |
| 16 | 启动自检无未保护路由 | 断言通过 |

### 13.3 验证 SQL

```sql
-- 1) 有权限点但没登记路由（补录时最容易漏的一类）
SELECT id, menu_name, menu_type, perms, http_method, http_path FROM public.sys_menu
WHERE deleted = 0 AND perms IS NOT NULL AND http_path IS NULL;

-- 2) 有路由但没权限点（漏填 perms 的按钮/菜单）
SELECT id, menu_name, menu_type, perms, http_method, http_path FROM public.sys_menu
WHERE deleted = 0 AND http_path IS NOT NULL AND (perms IS NULL OR perms = '');

-- 注：菜单数据里 menu_type 1=目录 2=菜单 3=按钮；「代码里存在但库里没登记」的路由
-- 无法用 SQL 查出，由 §6.3 的启动自检输出（AUTHZ_MODE=warn 时打 WARN 日志）

-- 重复的路由映射（同一 method+path 对应多个 perms 需人工确认是否 AND 关系）
SELECT http_method, http_path, count(*), array_agg(perms)
FROM public.sys_menu WHERE http_path IS NOT NULL AND deleted = 0
GROUP BY 1,2 HAVING count(*) > 1;

-- 即将过期的 key
SELECT id, name, key_prefix, expires_at FROM public.sys_api_key
WHERE revoked_at IS NULL AND enable AND expires_at BETWEEN now() AND now() + interval '7 days';

-- key 的 scopes 是否仍在其创建者权限内（角色被降权后的残留）
SELECT k.id, k.name, k.scopes FROM public.sys_api_key k WHERE k.deleted = 0;
```

### 13.4 Tauri 模式回归（每次合并必跑，对应 §16）

| # | 场景 | 期望 |
|---|---|---|
| T-1 | 清空 `JWT_SECRET` 启动 → 桌面端登录、资产列表、上传 | 全部正常；日志出现「HTTP API 未启动：JWT_SECRET 未配置」，窗口不崩 |
| T-2 | 配好 `JWT_SECRET` 启动 → 桌面端仍正常，HTTP 也正常 | 两者互不影响 |
| T-3 | 桌面端切换租户后查询资产列表 | 数据随租户变化（`GLOBAL_SCHEMA` 未被 HTTP 覆盖） |
| T-4 | 先用 HTTP 以另一租户身份请求一次，再回桌面查询 | 桌面数据不受影响（HTTP 不再写 `GLOBAL_SCHEMA`） |
| T-5 | 不设置任何新环境变量（`AUTHZ_MODE`、`API_KEY_*`、`OAUTH_*`） | 桌面端全功能可用，HTTP 鉴权退化为 `warn` 模式 |
| T-6 | 桌面端 `invoke` 任一命令的错误提示 | 与改造前完全一致（不走 401/403 新分支） |

---

## 14. 风险与待决问题

| ID | 风险/问题 | 影响 | 建议 |
|---|---|---|---|
| R1 | 给 `/api/upload/*`、`/api/chat/stream` 补认证属于**行为变更** | 现有前端/使用方可能中断 | Phase 0 先只要求登录，Phase 1 再加 perms；提前通知使用方 |
| R2 | 105 条路由只有 38 条映射，fail-closed 会大面积 403 | 上线即故障 | 先补录再开 fail-closed，中间用「未映射 → 仅校验登录 + 告警」过渡模式（`AUTHZ_MODE=warn`） |
| R3 | HTTP 明文不合规 | 合规审计不通过 | §8 方案 ①；短期用 ③ + 补偿控制 |
| R4 | `allow_origin(Any)` | 浏览器场景 key 可被盗用 | 收紧白名单；API Key 不用于浏览器直连 |
| R5 | `/oauth/authorize` 无会话 Cookie | 标准流程不可用 | 方案 A 先行，外部接入前补方案 B |
| R6 | 权限缓存 60s 窗口 | 降权不是瞬时 | 敏感操作（吊销、封禁）主动清缓存 |
| R7 | Tauri 与 axum 同进程，误加「进程级」改动会连带破坏桌面模式 | 桌面端启动失败 / 登录失效 / 数据串租户 | 全量遵守 §16 不变量；每次合并前跑 §13.4 的 Tauri 回归用例 |
| R8 | 前端若把 HTTP 的 401/403 逻辑套到 `invoke` 通道 | 桌面端出现莫名「权限不足」 | 错误分支按通道隔离（T-9），桌面端错误处理不动 |
| Q1 | 是否实现 `client_credentials` | 与 API Key 功能重叠 | 建议**不做**；确有需求时用 API Key |
| Q2 | 是否引入 JWT access token（OAuth） | 影响资源服务验签方式 | 单体场景 opaque + introspect 足够 |
| Q3 | `sys_menu` 权限点是否要拆出独立 `sys_permission` 表 | 菜单与权限耦合 | 本期不拆，避免大改 |

---

## 15. 参考资料

- draft-ietf-oauth-v2-1-17（OAuth 2.1，2026-10-31）
- RFC 6749 / RFC 6750 / RFC 7009 / RFC 7662 / RFC 8252 / RFC 8414 / RFC 9207
- RFC 9700（BCP 240）OAuth 2.0 Security Best Current Practice
- OWASP API Security Top 10（API1 BOLA / API5 BFLA —— 正是 §6.3、§6.5 要解决的问题）

---

## 16. Tauri 模式隔离硬约束（不变量）

### 16.1 为什么必须单独约束

`src/main.rs` → `assetsplatform_lib::run()`：Tauri Builder 与 `api::start_http_server_with()` 在**同一进程、同一 Tokio 运行时、同一数据库连接池、同一 `GLOBAL_SCHEMA`** 中运行。

```
┌────────────── 同一个 assets-platform 进程 ──────────────┐
│                                                          │
│  Tauri 通道（桌面）                 HTTP 通道（对外）      │
│  前端 invoke()                     前端 fetch / 第三方    │
│      │                                   │               │
│  #[tauri::command]                  axum Router           │
│      │                              auth_middleware       │
│      └────────→ service::* ←────────────┘                │
│                    │                                     │
│                database::*（schema_prefix）               │
└──────────────────────────────────────────────────────────┘
```

两条通道在 `service` / `database` 层汇合，因此：

- **axum 里的改动天然不影响 Tauri**：`invoke()` 不经过 Router、不经过任何中间件。
- **`service` / `database` / 进程启动期的改动会同时影响两边**：这是本方案唯一需要小心的地方。

结论：鉴权做在「HTTP 专属层」= 对 Tauri 零影响；做在「共享层」= 有风险。§16.2 用不变量把边界划清。

### 16.2 不变量清单

| ID | 不变量 | 为什么 | 怎么验 |
|---|---|---|---|
| **T-1** | 鉴权判定只存在于 `api` crate 的 HTTP 层（`auth.rs` / `authz.rs` / 路由 extractor）；`service`、`database` **不出现** `perms` / API Key / OAuth 判定代码 | Tauri 命令直接调 `service`，判定下沉就等于要求桌面端也带 token | `rg -n 'perms\|api_key\|oauth' crates/service/src crates/database/src` 只允许命中数据模型字段 |
| **T-2** | 禁止**进程级** fail-fast。必填项校验放 `api::start_http_server_with()`；失败返回 `Err`，由 `lib.rs:182-188` 捕获并 `tracing::error!`，**Tauri 窗口照常** | `JWT_SECRET` 在 Tauri 侧也在用：`src/commands/user_commands.rs:116`（`get_current_user` 解码）、`service/user_service.rs:76`（签发/校验）。启动 panic 会直接废掉桌面登录 | 清空 `JWT_SECRET` 启动 → 桌面可用、HTTP 不可用（§13.4 T-1） |
| **T-3** | `database::GLOBAL_SCHEMA` 必须保留且语义不变（Tauri 专用：`lib.rs:148` 启动设为默认租户 schema、`switch_tenant` 命令切换）。**只删除 `auth.rs:149` 的 HTTP 侧写入** | 现状是每个 HTTP 请求都把 `GLOBAL_SCHEMA` 改成「该请求用户的租户」，污染同进程的桌面会话（既有 bug，本方案顺手修掉，**不改变 Tauri 行为**） | `rg -n 'set_global_schema'` 只应命中 `lib.rs` 与 Tauri 命令（§13.4 T-3/T-4） |
| **T-4** | `schema_prefix()` 回退链（`CURRENT_SCHEMA` → `GLOBAL_SCHEMA` → `public`）不得修改；`with_schema()` 只在 HTTP handler / middleware 内调用 | 回退链是 Tauri 模式唯一的 schema 来源 | 桌面切租户后查询正确（§13.4 T-3） |
| **T-5** | DDL 一律 additive：`CREATE TABLE IF NOT EXISTS` / `ADD COLUMN IF NOT EXISTS`；不改 `sys_user` / `sys_menu` / `sys_role*` 既有列的类型与语义 | 这些表 Tauri 命令直接读写 | `git diff` 只含新增语句 |
| **T-6** | 新增权限点只填 `perms` / `http_method` / `http_path`，**不填 `command_name`** | `sys_menu.command_name` 是「Tauri 命令 → 权限」映射，前端菜单/按钮直接读它，填错会让桌面端菜单错乱 | `SELECT count(*) FROM sys_menu WHERE command_name IS NOT NULL AND http_path LIKE '/api/%'` 与改造前一致 |
| **T-7** | 新增环境变量必须「缺省即安全」：`AUTHZ_MODE` 缺省 = `warn`（不拒绝）、`API_KEY_*` 缺省 = 不支持 API Key（不崩溃）、`OAUTH_*` 缺省 = 端点未挂载 | 桌面用户不会配这些变量 | 不设任何新变量跑桌面全量回归（§13.4 T-5） |
| **T-8** | 不给 `service` 既有函数改签名；新能力一律新文件（`api_key_service.rs` / `oauth_service.rs`）。`user_service::Claims` 字段只增不减，新增字段必须 `#[serde(default)]` | Tauri 命令按旧签名调用 `service` | `git diff` 中既有 `pub async fn` 签名无变化 |
| **T-9** | 前端按通道隔离错误处理：只有 HTTP 通道（fetch 封装）才走 401/403 新逻辑；`invoke` 通道的错误分支保持原样 | 否则桌面端会把 Tauri 的 `Err(String)` 当成 403 处理 | 桌面端错误提示与改造前一致（§13.4 T-6） |

### 16.3 `JWT_SECRET` 的正确加固姿势（替代「启动 panic」）

```rust
// crates/api/src/lib.rs —— start_http_server_with() 开头，HTTP 唯一入口
let jwt_secret = std::env::var("JWT_SECRET").map_err(|_| {
    anyhow::anyhow!(
        "HTTP API 未启动：JWT_SECRET 未配置（Tauri 桌面模式不受影响，仍使用内置默认值）"
    )
})?;
if jwt_secret.len() < 32 {
    return Err(anyhow::anyhow!("HTTP API 未启动：JWT_SECRET 长度需 ≥ 32 字节"));
}
```

| 场景 | 改造前 | 改造后 |
|---|---|---|
| 桌面模式，`.env.toml` 未配 `jwt_secret` | 用默认串，可用 | **不变**（默认串，可用） |
| 桌面模式，配了 `jwt_secret` | 可用 | 不变 |
| 要对外开 HTTP，未配 secret | 用默认串对外服务（**严重隐患**） | HTTP 拒绝启动 + 明确日志；桌面端照常 |
| 反代只暴露 HTTP 端口（对外形态） | — | 安全目标达成 |

> 一句话：把「必需的强 secret」变成**开 HTTP 的前置条件**，而不是「整个进程的前置条件」。

### 16.4 与 §12 实施计划的对应

Phase 0 的三处改动都遵循本约束：

| 任务 | 通道 | 不变量 |
|---|---|---|
| #27 `/api/upload/*`、`/api/chat/stream` 纳入认证 | 纯 axum 路由调整 | T-1 |
| #26 `JWT_SECRET` 校验 | 移到 `start_http_server_with()` | T-2 / §16.3 |
| #28 去除 HTTP 侧写 `GLOBAL_SCHEMA` | 修既有串租户 bug | T-3 |
| #30 DDL + 权限点补录 | 只加表/行，不填 `command_name` | T-5 / T-6 |
| 新增环境变量 | 缺省即安全 | T-7 |

---

## 附录 A：与现有代码的改动点速查（路径相对 `apps/backend/src-tauri/`）

| 文件 | 改动 |
|---|---|
| `crates/utils/Cargo.toml` | 加 `sha2`（`rand` 若未启用一并加） |
| `crates/utils/src/api_key.rs` | 新建：`generate_api_key` / `hash_api_key` / `parse_prefix` |
| `crates/utils/src/lib.rs` | 导出 `api_key` 模块 |
| `crates/database/src/sql/public_migration.sql` | 追加 §9 DDL + 权限点补录 |
| `crates/database/src/models.rs` | `SysApiKey`、`SysOAuthClient` 等结构体 |
| `crates/service/src/api_key_service.rs` | 新建：CRUD + 校验 + 子集校验 |
| `crates/api/src/auth.rs` | 凭证分流、`AuthPrincipal` 注入、JWT 加固 |
| `crates/api/src/authz.rs` | 新建：`authz_middleware`、`perms_for`、启动自检 |
| `crates/api/src/api_key_routes.rs` | 新建：管理接口 |
| `crates/api/src/lib.rs` | 分层挂载 authz 层、把 upload/chat 纳入认证 |
| `crates/api/src/response.rs` | 403 增加 `scope` 字段 |
| `crates/apidoc/src/lib.rs` | 注册新 tag 与路径 |
