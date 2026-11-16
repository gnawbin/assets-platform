# OAuth 2.1 Bearer Token 待测待开发清单

> 关联文档：《OAuth2.1与API Key统一鉴权设计方案.md》§5.1 / §6.7 / §7.2 / §7.7 / §13.2；《桌面与Web全HTTP改造清单.md》§12；《业务网关（axum）多实例部署设计方案.md》

| 项 | 内容 |
|---|---|
| 文档版本 | v1.0 / 2026-11-16 |
| 目标 | 把「OAuth 2.1 Bearer Token」从「能跑」推到「可对外、可回归」：补齐测试 → 修复已知行为缝 → 页面 ↔ 后端接口全部对齐 |
| 当前基线 | `cargo test -p api --lib` 96 通过；`cargo test -p api --lib oauth` 26 通过；`cargo test -p apidoc` 4 通过；前端 jest（`httpAdapter` + `uploadService`）28 通过；`tsc --noEmit` 0 错误；Swagger 99 paths |
| 不在本文范围 | 方案 B（Cookie 浏览器授权流）的完整实现与测试；多租户 schema 隔离（SEC-4）；`test_rag_retrieval` / `chunk_and_vectorize` 的 HTTP 化（仍属 `TAURI_ONLY_COMMANDS`） |

## 0. 状态图例

| 标记 | 含义 |
|---|---|
| ✅ | 已覆盖且已在本机验证 |
| ⚠️ | 部分覆盖（测了相邻能力，关键分支未测） |
| ❌ | 未覆盖 / 未实现 |
| 🚧 | 待开发（代码尚未写） |

## 1. 结论摘要

1. **测试是绿的，但绿的不是 Bearer 这条路径。** 26 条 OAuth 单测集中在「签发侧」（`sys_oauth_client` 解析、PKCE、令牌轮换、重放处置）。与「把 `at_` 令牌当资源凭证用」直接相关的只有 `authz` 的 2 条（`oauth_principal_is_scoped_and_never_super_admin` 等）；`parse_credential` 的 `at_` / `rt_` 分流 **0 条断言**，`verify_access_token` **0 条断言**（需 DB），`crates/api` **无 `tests/` 集成目录**。
2. **对外发令牌前必须处理的 3 个后端缝**：资源侧不校验用户状态（D-2）、禁用 / 删除用户不吊销 OAuth 令牌（D-3）、OAuth 令牌无限流（D-4）。
3. **Web 模式的 2 个高优先缺口**：base URL 双 `/api`（D-6）、上传通道无 401 续期（D-7）；另有 2 处错误处理与注释缺口（D-5 不透出后端文案且二次 401 不清会话、D-8 中间件注释过时）。详见 §6.5。

### 1.1 风险排序

| # | 问题 | 触发条件 | 后果 | 级别 |
|---|---|---|---|---|
| R1 | 默认 `NEXT_PUBLIC_API_BASE_URL` 与映射表路径重复 `/api` | 未设置该 env 的新环境 | 所有页面请求 `/api/api/...` → 404 | **高** |
| R2 | 上传通道（`HttpUploadAdapter`）无 401 续期 | 大文件分片过程跨过令牌过期点 | 上传中途整批失败，需手动重来 | **高** |
| R3 | `verify_access_token` 不校验用户状态 | 禁用 / 删除用户后 | `at_` 令牌仍可用 ≤15 分钟 | 中 |
| R4 | 禁用 / 删除用户不吊销 `sys_oauth_token` | 同上 | 令牌行长期留在库中，审计与合规口径不清 | 中 |
| R5 | OAuth 令牌无速率限制 | 单客户端高频调用 | 可打满后端 | 中 |
| R6 | 前端不透出后端错误文案 | 403 / 业务错误 | 运维看到整串原始 JSON | 低 |
| R7 | `at_` 分流零单测（测试债） | 后续改动 | 分流被静默改坏 → 全站 401 或误放行 | 中 |

## 2. 先分清四条链路（「测错对象」的根源）

| # | 链路 | 调用方 | 凭证 | 入口 | authz 语义 |
|---|---|---|---|---|---|
| L1 | 页面 / WebView → 业务接口 | 登录用户 | 用户 JWT（`eyJ…`），存 `localStorage.auth_token` | `Credential::Jwt` | 注入 `UserContext` + `Claims`；`AUTHZ_MODE=warn` 时只告警 |
| L2 | **第三方系统 → 业务接口** | 服务 | **OAuth `at_` 访问令牌** | `Credential::AccessToken` | 不注入 `UserContext` / `Claims`，**始终 enforce** |
| L3 | 管理页 → `/api/oauth-clients*` | 登录用户 | 用户 JWT | `Credential::Jwt` | `system:oauthclient:*` |
| L4 | 第三方 → `/oauth/authorize` | 用户 | JWT（方案 A）或 `ap_session` Cookie（方案 B） | `oauth_browser::bearer_principal` / `cookie_session` | 只接受用户主体；`at_` / `ak_` 一律 `access_denied` |

> 已被误读两次的点：`oauth_routes::tests::bearer_header_is_not_treated_as_client_credentials` 测的是 **L4 的 `client_secret_basic` 解析**（`POST /oauth/token` 的客户端认证），与 L2 的资源访问完全无关。引用它来证明「Bearer 已测」是错的。

## 3. 现状事实基线（F 组，可逐条核对）

| ID | 事实 | 证据（`file:line`） |
|---|---|---|
| F-1 | `at_` 分流在 `parse_credential`：`ak_` → API Key；`at_` → `AccessToken`；`rt_` → 401「刷新令牌只能用于 /oauth/token」；其余 → JWT | `api/src/auth.rs:205-233`（`at_` 在 :220-222，`rt_` 在 :225-230） |
| F-2 | `parse_credential` 全仓仅 2 处调用 | `api/src/auth.rs:423`、`api/src/oauth_browser.rs:138` |
| F-3 | 令牌校验条件 = 哈希命中 + `token_type=access` + `revoked_at IS NULL` + 未过期 + 客户端 `enable` 且未软删；**不含用户状态** | `service/src/oauth_service.rs:1421-1455` |
| F-4 | TTL：access 默认 **900s**（夹取 60–86400）；refresh 默认 **30 天**（夹取 300–7776000） | `oauth_service.rs:44`、`:49` |
| F-5 | 用户状态只在签发 / 刷新 / 会话处校验 | `ensure_user_active`（`:955`）被 `:984`、`:1251`、`:1374` 与 `oauth_browser.rs:179` 调用 |
| F-6 | 禁用 / 删除用户只吊销 **JWT 刷新令牌**，不碰 `sys_oauth_token` | `user_service.rs:501`；调用点 `:1011`（改状态）、`:1100`（删除）、`:1155` |
| F-7 | 非用户主体语义：始终 enforce；`system:*` / 需用户归属的写接口 / `required.is_empty()` → 403；限流只对 `Service` 生效 | `api/src/authz.rs:578`、`:595-638`、`:641-654` |
| F-8 | 挑战头：401 → `Bearer realm="assets-platform"`；403 → 追加 `error="insufficient_scope"` 与 `scope="…"`；体为 `{code,message,scope?}` | `api/src/response.rs:58-70`、`:146-168`；单测见 `:198-260`（三条用例分别覆盖 401 挑战头、带 scope 的 403、无 scope 的 403） |
| F-9 | `/oauth/*` 需 `OAUTH_ENABLED` 或 `OAUTH_ALLOW_INSECURE_HTTP` 才挂载，否则 404；`/api/oauth-clients*` 始终挂载 | `oauth_routes.rs:47-60`、`api/src/lib.rs:350-359`、`:673-681` |
| F-10 | 管理权限点 + 菜单：`sys_menu` 205–208 带 `http_method` / `http_path`；页面 id 49 = `/settings/oauth-clients` | `public_migration.sql:751-757`、`:761` |
| F-11 | Swagger 已注册 10 个 OAuth handler | `crates/apidoc/src/openapi.rs:215-224` |
| F-12 | 前端令牌注入：`localStorage.auth_token` → `Authorization: Bearer …` | `httpAdapter.ts:336-346`、`:533-540` |
| F-13 | 前端 401 续期：认证端点白名单跳过；刷新失败 `clearAuthSession()`；响应只认 `data.token` / `data.refresh_token` | `httpAdapter.ts:351-356`、`:402-408`、`:550-570`、`:437-451` |
| F-14 | 前端错误处理：响应体原文塞进 `Error.message`；成功侧解包 `data` | `httpAdapter.ts:572-577`、`:583-589` |
| F-15 | **base URL 双约定**：`httpAdapter` 默认 `http://localhost:3001/api` + 路径 `/api/...`（→ `/api/api/`）；`uploadService` 默认同名 + 路径 `/upload/...`（正确）。仓库根与 `apps/web` 均**无** `.env*` 文件 | `httpAdapter.ts:36`、`uploadService.ts:192` |
| F-16 | 上传通道独立：自带 `withAuth`、自带 `baseUrl`、**无 401 续期**、body 手写 snake_case | `uploadService.ts:180-183`、`:201-211` |
| F-17 | OAuth 管理 DTO 为 snake_case，与 module 6（上传版本历史）的 camelCase DTO 并存 | `oauth_service.rs:114-128`、`oauthClientService.ts:129-146` |
| F-18 | `crates/api/tests` 目录不存在，全部为 `--lib` 单测 | — |
| F-19 | `auth_middleware` 文档注释过时：只写 `ak_` 与「其它 → JWT」，漏 `at_` / `rt_` | `api/src/auth.rs:413-415` |

## 4. 待测清单

### 4.1 A 组：后端纯函数（无 DB，`cargo test -p api --lib` 即可跑）

| ID | 用例 | 期望 | 状态 | 落点 |
|---|---|---|---|---|
| A-1 | `parse_credential("at_" + 43 位)` | `Ok(Credential::AccessToken(_))` | ✅ | `api/src/auth.rs` 的 `mod tests`（`access_token_prefix_is_routed_to_oauth`） |
| A-2 | `parse_credential("rt_" + 43 位)` | `Err`，message 含「刷新令牌」 | ✅ | 同上（`refresh_token_prefix_is_rejected_with_actionable_message`） |
| A-3 | `parse_credential("ak_live_…")` | `Ok(Credential::ApiKey(_))` | ✅ | 同上（`api_key_prefix_is_routed_to_api_key` + 反向用例 `malformed_api_key_is_rejected_instead_of_falling_back_to_jwt`） |
| A-4 | `parse_credential("")` / `"   "` | `Err`（「认证令牌为空」） | ✅ | 同上（`empty_credential_is_rejected`，覆盖 `""` / 空格 / 制表换行） |
| A-5 | `parse_credential("eyJ…")` | `Ok(Credential::Jwt(_))` | ✅ | 同上（`jwt_is_routed_to_jwt_branch`，用与签发侧同形的 `Claims` 真签一个 JWT） |
| A-6 | `bearer_principal(Bearer at_…)` | `OauthError{ error: "access_denied" }`，**不触库** | ✅ | `api/src/oauth_browser.rs`（`bearer_principal_rejects_oauth_access_token`） |
| A-7 | `bearer_principal(Bearer ak_…)` | 同上 `access_denied` | ✅ | 同上（`bearer_principal_rejects_api_key`） |
| A-8 | `bearer_principal` 缺头 / `"Token xxx"` | `invalid_request`（格式提示） | ✅ | 同上（`bearer_principal_requires_bearer_scheme`） |
| A-9 | `bearer_principal(Bearer 坏 JWT)` | `invalid_token` | ✅ | 同上（`bearer_principal_rejects_invalid_jwt`） |
| A-10 | `ApiError` 的 401 / 403 `WWW-Authenticate` | 已覆盖 | ✅ | `api/src/response.rs:198-260` |

> 实现提示：A-1…A-5 只依赖 `parse_credential`，无 async、无 DB；`at_` / `rt_` 令牌可用 `utils::oauth` 的生成函数拼出，避免手写字面量写错长度（长度错会被当作 JWT 从而返回 `Ok(Jwt)`，这正是这些用例要防的）。

### 4.2 B 组：后端契约（需 DB 或进程内 router）

| ID | 用例 | 期望 | 状态 |
|---|---|---|---|
| B-1 | `at_`（scope 含 `asset:hardware:list`）→ `GET /api/assets/hardware` | 200 | ❌ |
| B-2 | `at_` → 有权限点但 scope 范围外的路由 | 403 + `scope` 数组 + `insufficient_scope` 头 | ❌ |
| B-3 | `at_` → `/api/oauth-clients` | 403（被 `system:*` 拦） | ❌ |
| B-4 | `at_` → 仅需登录的接口（`required.is_empty()`） | 403「该接口需要用户身份」 | ❌ |
| B-5 | `at_` → 需用户归属的写接口 | 403 | ❌ |
| B-6 | 令牌过期 / 已 revoke / 客户端被禁用 | 401 | ❌ |
| B-7 | `rt_` 当 Bearer 用 | 401 + 文案含「刷新令牌」 | ❌ |
| B-8 | 用户被禁用后 `at_` 是否仍可用 | **现状：可用 ≤15 min**；D-2 修复后应变 401 | ❌ |
| B-9 | 无令牌 / 格式错 | 401 + `WWW-Authenticate: Bearer` | ❌ |
| B-10 | 令牌放 query（`?access_token=`） | 400 | ❌ |

> B 组若 `api` 侧尚未拆分出可直接 `oneshot` 的 router 构造（P0-B 的 `bind_http()` / `serve(listener)` 拆分），先落到 §4.4 的 curl 脚本，B-6…B-10 标为「脚本覆盖」，避免为了测试改架构。

### 4.3 C 组：前端（jest）

| ID | 用例 | 期望 | 状态 |
|---|---|---|---|
| C-1 | `httpAdapter.get('get_categories')` 且存在 `localStorage.auth_token` | fetch 收到 `Authorization: Bearer token-abc` | ✅ |
| C-2 | 首次 401 → 调 `/api/auth/refresh` → 重放一次 | 第二次带新令牌，且只刷新一次 | ✅ |
| C-3 | 刷新失败 | 清 `auth_token` / `refresh_token` / `auth_user` 并跳 `/login` | ✅ |
| C-4 | 403 响应体 `{code,message,scope}` | `Error.message` 含后端 message | ✅ |
| C-5 | 未设 env 时拼 URL | 不出现 `/api/api/` | ✅ |
| C-6 | `oauthClientService` 4 个命令在 `COMMAND_ROUTE_MAP` 中，且请求键名与后端 snake_case 一致 | 静态断言通过 | ✅ |

> 落点（2026-11-16）：C-1…C-5 在 `apps/web/src/tests/utils/adapters/httpAdapter.test.ts`
> （`认证与续期` 4 条 + `URL 拼装` 3 条 + `extractServerMessage` 5 条）；
> C-6 在 `apps/web/src/tests/services/oauthClientService.test.ts`（5 条：映射表 1 + 4 个动作的 URL/body 形状）。
> 上传通道的同类断言在 `tests/services/uploadService.test.ts`（28 条，含 401 续期 4 条）。

> C-1…C-5 属「先写失败用例，再改实现」；落地时若不想引入红测试，可把 C-4 / C-5 与 D-5 / D-6 放同一个 PR（PR-2）。

### 4.4 E 组：端到端冒烟（真库 + 真进程，脚本化）

| ID | 场景 | 期望 |
|---|---|---|
| E-1 | admin 登录拿 JWT | 200，`data.token` |
| E-2 | 建 confidential client（scopes=`asset:hardware:list`） | 返回一次性 `client_secret` |
| E-3 | `GET /oauth/authorize`（Bearer JWT + PKCE S256） | 返回同意信息（`client_name` / `scopes` / `state` / `issuer`） |
| E-4 | `POST /oauth/authorize` approve | 返回 `redirect_to`（含 code） |
| E-5 | `POST /oauth/token`（`client_secret_basic` + code + verifier） | `token_type=Bearer`、`at_` / `rt_`、`Cache-Control: no-store` |
| E-6 | `at_` → `GET /api/assets/hardware` | 200 |
| E-7 | `at_` → `/api/users`、`/api/oauth-clients` | 403（带 `scope` 或「需要用户身份」） |
| E-8 | `rt_` → 资源接口 | 401 + 「刷新令牌」 |
| E-9 | 刷新令牌轮换后重放旧 `rt_` | 旧 `rt_` 失效且整族撤销 |
| E-10 | `/oauth/introspect` 内省他人令牌 | `active:false`（不泄露存在性） |
| E-11 | `/oauth/revoke` 后用 `at_` | 401 |
| E-12 | 关闭客户端（`enable=false`）后用 `at_` | 401 |
| E-13 | 未开 `OAUTH_ENABLED` | `/oauth/token` 404，`/api/oauth-clients` 200 |

> 编号说明：E 组是**待测用例**，§5 的 D 组是**待开发项**，两套编号独立，勿混用。

> 冒烟纪律（沿用 module 6 的教训）：脚本必须自带清理（`sys_oauth_client` / `sys_oauth_code` / `sys_oauth_token` 按 `client_id` 前缀删除），且在**非生产库**执行；`sys_oauth_client` 有 `created_by`，删除时按 `id` 精确删。

## 5. 待开发清单

### D-1 Bearer 分流零单测（测试债）

- **现状**：`parse_credential`（`api/src/auth.rs:205-233`）的 4 个分支无任何断言；`bearer_principal` 的拒绝分支同样无断言。
- **影响**：分流是「全站 401 / 误放行」的总开关，却是唯一没有单测守门的鉴权环节。
- **方案**：补齐 §4.1 的 A-1…A-9（9 条）。
- **验收**：`cargo test -p api --lib` 96 → 105+；`cargo test -p api --lib oauth` 26 → 35+。
- **状态**：✅ **已实现**（2026-11-16）：A-1…A-5 落在 `api/src/auth.rs` 的 `mod tests`（6 条），
  A-6…A-9 落在 `api/src/oauth_browser.rs`（4 条 `#[tokio::test]`，均在触库前返回）。
  `cargo test -p api --lib` **96 → 106 passed**。

### D-2 `verify_access_token` 不校验用户状态

- **现状**：F-3，只查令牌本身与客户端状态。
- **影响**：禁用 / 删除用户后，其 `at_` 令牌在 access TTL（默认 900s，最长 86400s）内仍可调用业务接口。签发侧有 `ensure_user_active`，资源侧没有。
- **方案**：
  - **A（推荐）**：在同一条 SQL 上 JOIN `public.sys_user u`，条件加 `u.status = 1 AND (u.deleted IS NULL OR u.deleted = 0)` —— 零额外往返，与已有的 `sys_oauth_client` JOIN 同构。
  - B：进程内 60s 用户状态缓存（复用 `AUTHORITY_CACHE_TTL` 的思路），可降频但多一次查询，且引入「禁用后最长 60s 仍可用」的新窗口。
- **验收**：B-8 反转为「禁用后立即 401」；`cargo test -p service --lib oauth` 不回归。

### D-3 禁用 / 删除用户不吊销 `sys_oauth_token`

- **现状**：F-6，`revoke_all_user_refresh_tokens` 只清 JWT 刷新令牌。
- **方案**：新增 `oauth_service::revoke_all_user_tokens(user_id)`：
  ```sql
  UPDATE public.sys_oauth_token SET revoked_at = now()
  WHERE user_id = $1 AND revoked_at IS NULL
  ```
  在 `update_user`（`status != 1`）与 `delete_user` 之后调用，失败只 `warn!`（与现有 `revoke_all_user_refresh_tokens` 的容错风格一致）；**用 `revoked_at` 而非删除行**，保留审计痕迹。
- **验收**：E-11 扩展方向补一例「删除用户后其 `at_` 立即 401」；D-2 若未做，D-3 也能把窗口从「≤15 min」压到「下一次令牌校验即失效」。

### D-4 OAuth 令牌无速率限制

- **现状**：F-7，OAuth 主体 `rate_limit_per_min = None`，限流分支只对 `PrincipalKind::Service` 生效。
- **方案**：`sys_oauth_client` 增 `rate_limit_per_min INT NOT NULL DEFAULT 600`（幂等 `ALTER ... IF NOT EXISTS` 写法），`AuthPrincipal` 填充该值，`authz` 的限流分支由 `kind == Service` 放宽为 `kind != User`，限流 key 用 `token_id`。
- **验收**：超限返回 429（现有 429 已有挑战头逻辑可复用）；API Key 现有行为不变。

### D-5 前端不透出后端错误文案 + 二次 401 不清会话

- **现状**：F-14。`httpAdapter.ts:572-577` 抛出的是 `HTTP 403: Forbidden - {原始 JSON}`；且重放后仍 401 时不会清会话。
- **方案**：
  1. `!response.ok` 时先 `await response.json()`（失败回落 `await response.text()`），取 `message` 作为 `Error.message`，缺省回落 `statusText`；
  2. 403 时把 `scope` 挂到错误对象（`(err as ApiErrorLike).scope = body.scope`），便于页面提示「缺少权限点」；
  3. 若本次请求已经续期重放过一次且仍 401 → `clearAuthSession()`。
- **验收**：C-4 通过；页面 toast 显示「权限不足」而非 JSON 串。
- **状态**：✅ **前端已实现**（2026-11-16）
  - `httpAdapter.ts` 新增 `extractServerMessage()`：识别 `{code,message[,scope]}`、OAuth `{error,error_description}`、≤300 字符纯文本，HTML 错误页（nginx 502）返回 `null` 交回状态行；
  - 失败时抛 `buildServerError()` 构造的错误对象，`scope` 挂在 `err.scope`（`ApiErrorWithScope` 已导出）；
  - 续期后仍 401 → `clearAuthSession()`，消息追加「登录状态已失效，请重新登录」。
  - 用例：`tests/utils/adapters/httpAdapter.test.ts`（`extractServerMessage` 5 条 + 403 文案/`scope` 1 条）。

### D-6 base URL 双约定（默认值必然拼出 `/api/api/`）

- **现状**：F-15。`httpAdapter` 的默认 base 自带 `/api`，而映射表路径也以 `/api/` 开头；仓库内无 `.env*`，所以默认值就是实际生效值。
- **方案**：随 P0-C 的 `apps/web/src/utils/apiBase.ts` 统一：
  - `getApiOrigin()` → `http://127.0.0.1:3001`（可被 `NEXT_PUBLIC_API_BASE_URL` 覆盖成任意 origin）
  - `getApiBase()` → `getApiOrigin() + '/api'`
  - `httpAdapter` 用 `getApiOrigin()`（路径自带 `/api`，映射表不动）；`uploadService` 用 `getApiBase()`（路径不带 `/api`）。
- **验收**：C-5 通过；未设 env 时 `get_categories` 打到 `http://127.0.0.1:3001/api/categories`；Tauri 模式不受影响（走 `invoke`）。
- **状态**：✅ **已实现**（2026-11-16，随「全 HTTP 单通道」改造）
  - 新增 `apps/web/src/utils/apiBase.ts`：`getApiBase()`（**含** `/api`）/ `getApiOrigin()`（**不含**）/ `normalizeApiBase()` / `setApiBase()` / `clearApiBase()`，优先级 `window.__API_BASE__` → `localStorage['api_base_url']` → `NEXT_PUBLIC_API_BASE_URL` → 桌面与本机 3001 兜底 → 同源 `/api`；
  - `httpAdapter.buildUrl()` 改用 `getApiOrigin()` 拼映射表路径（路径自带 `/api`），**`/api/api` 已消失**；
  - `uploadService` / `useChatStream` 同步改为运行时解析（不再硬编码 3001）；
  - 用例：`tests/utils/apiBase.test.ts`（规范化 6 条 + 优先级 6 条 + 读写 3 条）、`httpAdapter.test.ts`（URL 拼装 3 条，断言 `not.toContain('/api/api')`）。
- **补充**（2026-11-16，P0-C 收尾）：地址不再只能靠构建期 env —— 新增 `/settings/server` 配置与诊断页
  （`components/ServerConnectionForm.tsx` + `components/ServerConfigGate.tsx`），用户可在运行时改基址：
  - `getApiBaseSource()` 标记来源（`injected` / `storage` / `env` / `tauri-default` / `dev-default` / `same-origin` / `none`），
    诊断页直接展示「这个地址是哪来的」；
  - `setApiBase()` 保存时清空 5 个会话键（含 `available_tenants` / `selected_tenant_id`），**杜绝跨服务器令牌导致的 401 死循环**（与 D-5 的二次 401 清会话口径一致）；
  - 非回环 `http://` 默认抛 `InsecureTransportError`（令牌明文风险），表单二次确认后才以 `allowInsecure` 保存；
  - 用例：`apiBase.test.ts` 31 条（新增来源判定 4 / 不安全地址 3 / 保存 4 / `probeServer` 5）、`ServerConnectionForm.test.tsx` 6 条、`ServerConfigGate.test.tsx` 4 条。

### D-7 上传通道无 401 续期

- **现状**：F-16。`withAuth` 只加头；大文件上传是全部通道里**最可能跨过令牌过期点**的，却唯独它没有续期。
- **方案**：包一层 `authorizedFetch(url, init)`：`401` → 复用 `httpAdapter` 导出的 `refreshAccessToken()` → 重放一次；仍失败则 `clearAuthSession()` 并抛出可读错误。
- **硬约束**：S3 presigned PUT **绝不加 `Authorization`**（`uploadService.ts:178` 已有警告注释）——续期只作用于本项目 API（`init` / `complete` / `abort` / `versions`），不作用于 presigned URL。
- **验收**：新增 uploadService 用例「401 → 刷新 → 重放成功」；现有 24 条不回归。
- **状态**：✅ **已实现**（2026-11-16）
  - `HttpUploadAdapter` 新增私有 `request()`：`401 && 有令牌` → `refreshAccessToken()` → 重放一次（替换 `Authorization`）；续期失败或重放仍 401 → `clearAuthSession()`；
  - 复用 `httpAdapter` 导出的 `refreshAccessToken` / `clearAuthSession` / `extractServerMessage`，错误消息优先用后端文案（`初始化上传失败: 上传会话已过期`）；
  - S3 presigned PUT 仍走裸 `fetch`，**不带** `Authorization`（用例已锁定）；
  - 用例：`tests/services/uploadService.test.ts` 24 → **28 条**（新增 4 条：续期重放成功 / 续期失败清会话 / 无令牌不续期 / presigned 不参与续期）。

### D-8 `auth_middleware` 文档注释过时

- **方案**：改写 `api/src/auth.rs:413-415`，把四条分支（`ak_` / `at_` / `rt_` / JWT）写全，并注明「`rt_` 在资源路径一律 401」的理由（设计文档 §13.2 #3）。零行为变更。
- **状态**：✅ **已实现**（2026-11-16）：注释补齐 `ak_` / `at_` / `rt_` / JWT 四条分支及各自的主体类型，
  并指向本文档 A-1…A-5 单测；`?access_token=` 拒绝理由也写进注释。

### D-9 无集成测试目录 / 无端到端脚本

- **方案**：先加 `scripts/smoke/oauth_bearer.sh`（覆盖 §4.4 的 E-1…E-13，含自清理），再评估 `crates/api/tests/oauth_bearer.rs`（需要 DB 的用例标 `#[ignore]`，用 `cargo test -- --ignored` 触发）。
- **验收**：脚本 13/13 绿；脚本二次运行结果一致（幂等）。

### D-10（记录，不实施）DTO 命名风格不统一

- 上传模块用 camelCase（module 6 决策），OAuth 模块用 snake_case。两者各自与前端契约一致，改动成本 > 收益。**结论**：仅在新接口上统一为 camelCase，并写入《桌面与Web全HTTP改造清单.md》的规范段；历史接口不动。

## 6. 页面 ↔ 后端接口对齐

### 6.1 管理页 `/settings/oauth-clients` 对齐矩阵

| 页面动作 | 前端调用 | 命令名 | 后端路由 | 权限点（`sys_menu`） | 请求体 | 响应消费 |
|---|---|---|---|---|---|---|
| 列表 | `getOauthClients()` | `get_oauth_clients` | `GET /api/oauth-clients` | `system:oauthclient:list`（205） | — | `OauthClientView[]`（snake_case，`id` 为字符串） |
| 新建 | `createOauthClient(params)` | `create_oauth_client` | `POST /api/oauth-clients` | `system:oauthclient:add`（206） | `{client_name, client_type?, redirect_uris, allowed_scopes, grant_types?, access_token_ttl?, refresh_token_ttl?}` | `CreatedOauthClient`（一次性 `client_secret`；public client 为 `null`） |
| 编辑 / 启停 | `updateOauthClient(id, params)` | `update_oauth_client` | `PUT /api/oauth-clients/{id}` | `system:oauthclient:edit`（207） | `{id, ...}`；`enable:false` 会同步撤销该客户端全部令牌 | `OauthClientView` |
| 删除 | `deleteOauthClient(id)` | `delete_oauth_client` | `DELETE /api/oauth-clients/{id}` | `system:oauthclient:delete`（208） | `{id}` | 无 `data` |
| 菜单入口 | — | — | `/settings/oauth-clients` | 页面 id 49 | — | — |

对齐结论：

- 路径占位符 `{id}` 与命令参数名 `id` 一致，`extractParams` 会把它从 body 剥离（与 module 6 的 `fileGroupId` 规则相同）✅
- body 键名 snake_case 与后端 serde 字段一致 ✅
- `client_secret` 仅创建响应返回一次，页面已显式提示「关闭后不可再查看」✅
- 状态码为 401/403 时依赖 D-5 的文案透出，否则页面只能显示原始 JSON ⚠️

### 6.2 登录 / 会话链路（L1）Bearer 契约

| 环节 | 前端行为 | 后端契约 | 对齐 |
|---|---|---|---|
| 登录 | `POST /api/auth/login` | `ApiResponse<{token, refresh_token, user}>` | ✅ |
| 注入 | 每次请求加 `Authorization: Bearer <auth_token>` | `extract_bearer` → `parse_credential` | ✅ |
| 续期 | `POST /api/auth/refresh {refresh_token}` → 读 `data.token` / `data.refresh_token` | 同上形状 | ✅ |
| 登出 | `POST /api/auth/logout {refresh_token}`（`keepalive`） | 撤销该刷新令牌 | ✅ |
| 兜底 | 清 5 个 localStorage key → 跳 `/login` | — | ⚠️（二次 401 不清，见 D-5） |

### 6.3 响应契约对齐（页面会碰到的 4 种形状）

| 形状 | 用于 | 字段 | 页面处理 |
|---|---|---|---|
| `ApiResponse<T>` | 所有 `/api/**` 成功 | `{code:0, message:"success", data:T}` | `httpAdapter.ts:583-589` 解包 `data` ✅ |
| `ApiError` | 所有 `/api/**` 失败 | `{code, message, scope?}` + `WWW-Authenticate` | ⚠️ 未解析，整串进 toast（D-5） |
| `OauthErrorBody` | `/oauth/**` 失败 | `{error, error_description}`（RFC 6749 §5.2） | 无页面调用；第三方需按 RFC 解析 |
| `OauthTokenResponse` | `/oauth/token` 成功 | `{access_token, token_type:"Bearer", expires_in, refresh_token, scope}` + `Cache-Control: no-store` | 无页面调用 |

### 6.4 前端 Bearer 契约规范（新页面照抄）

1. 业务请求一律 `Authorization: Bearer <localStorage.auth_token>`；**禁止**把令牌放 query（后端 400）。
2. 401 → 最多静默续期一次；仍 401 → 清会话跳登录（D-5）。
3. 403 → 读 `message` 与 `scope`，并识别 `WWW-Authenticate` 中的 `error="insufficient_scope"`。
4. 页面**永不**保存 `at_` / `rt_`：OAuth 令牌只给第三方服务端用，且后端对非用户主体一律 403。
5. base URL 只有一个来源（`utils/apiBase.ts`），origin 与 `/api` 前缀的归属只允许一种约定（D-6）。
6. 自建 fetch 通道（上传等）必须复用同一套头构造与 401 续期（D-7）。

### 6.5 未对齐项汇总

| # | 现状 | 应有 | 对应 |
|---|---|---|---|
| M-1 | ~~`httpAdapter` 默认 base 重复 `/api`~~ | origin + 路径自带 `/api` | D-6 ✅ |
| M-2 | ~~403 / 业务错误文案丢失~~ | 透出 `message` 与 `scope` | D-5 ✅ |
| M-3 | ~~上传通道无 401 续期~~ | 与主通道一致 | D-7 ✅ |
| M-4 | ~~二次 401 不清会话~~ | 清会话并跳登录 | D-5 ✅ |

> 2026-11-16：M-1…M-4 均已在前端落地（同时把前端从「Tauri IPC + HTTP 双通道」收敛为纯 HTTP 单通道），
> 剩余的是 E 组端到端冒烟（D-9）与后端 D-1…D-4。

## 7. 验收命令

```bash
# 后端
cd apps/backend/src-tauri && cargo test -p api --lib
cd apps/backend/src-tauri && cargo test -p api --lib oauth
cd apps/backend/src-tauri && cargo test -p service --lib oauth
cd apps/backend/src-tauri && cargo test -p apidoc

# 前端
cd apps/web && ./node_modules/.bin/jest src/tests/utils/apiBase.test.ts
cd apps/web && ./node_modules/.bin/jest src/tests/utils/adapters/httpAdapter.test.ts
cd apps/web && ./node_modules/.bin/jest src/tests/services/uploadService.test.ts
cd apps/web && ./node_modules/.bin/jest src/tests/hooks/useChunkedUpload.test.ts
cd apps/web && ./node_modules/.bin/tsc --noEmit

# 端到端（E-1…E-13 全绿才算通过，脚本需支持重复执行）
bash scripts/smoke/oauth_bearer.sh
```

## 8. 建议的 PR 切分

| PR | 内容 | 行为变更 |
|---|---|---|
| PR-1 | D-1（A-1…A-9）+ D-8 + C-6 | 无（纯测试 + 注释）— **已完成（2026-11-16）**：`cargo test -p api --lib` 106 passed；C-6 用例 5 条 |
| PR-2 | D-5 + D-6 + D-7 + C-1…C-5 | 前端行为变（错误文案 / base URL / 续期）— **已完成（2026-11-16）**：C-1…C-5 用例 12 条 |
| PR-3 | D-2 + D-3 + D-4 + B-6…B-10（含幂等 DDL） | 后端安全行为变 |
| PR-4 | D-9 冒烟脚本 + 本文档 §4 状态回填 | 无 |

## 9. 边界与不做

- 不做方案 B（Cookie 浏览器授权流）的完整实现与测试，仅保留 `/oauth/authorize` 的双流程分流现状。
- 不做「客户端日配额」「按 scope 计费」等配额体系，只做速率限制。
- 不重构 `sys_oauth_token` 表结构；D-4 只给 `sys_oauth_client` 加一列。
- 不统一历史 DTO 命名（D-10 仅记录）。
- 不改 `/oauth/*` 的挂载开关语义（缺省 404 是安全设计，不是缺陷）。

## 10. 附录：证据索引

**后端**

- `apps/backend/src-tauri/crates/api/src/auth.rs`：`parse_credential` 205-233、`extract_bearer` 367、`auth_middleware` 注释 410-418、中间件主体 419 起
- `crates/api/src/authz.rs`：非用户主体 enforce 578、服务主体禁区 595-638、限流 641-654、OAuth 主体单测 1094-1114
- `crates/api/src/response.rs`：`ApiError` 58-70、挑战头 146-168、相关单测 198-260
- `crates/api/src/oauth_routes.rs`：开关 47-60、`basic_credentials` 173-201、`authenticate_request_client` 204-233、`token_response`（`Cache-Control: no-store`）249-269、`bearer_header_is_not_treated_as_client_credentials` 975
- `crates/api/src/oauth_browser.rs`：`bearer_principal` / `cookie_session` 127-183、`issue_session_cookies` 469-472
- `crates/api/src/oauth_error.rs`：1-60（`OauthErrorBody` / `WWW-Authenticate` 规则）
- `crates/api/src/lib.rs`：`/oauth/*` 挂载条件 350-359、`/api/oauth-clients*` 675-681
- `crates/service/src/oauth_service.rs`：TTL 39-62、DTO 114-197、`ensure_user_active` 955-976、`verify_access_token` 1421-1455
- `crates/service/src/user_service.rs`：`revoke_all_user_refresh_tokens` 501、调用点 1011 / 1100 / 1155
- `crates/apidoc/src/openapi.rs`：OAuth handler 注册 215-224
- `crates/database/src/sql/public_migration.sql`：`sys_menu` 205-208 的 `http_path` 751-757、页面 id 49 的 761

**前端**

- `apps/web/src/utils/adapters/httpAdapter.ts`：默认 base 36、`COMMAND_ROUTE_MAP` 57 起（OAuth 客户端 4 条在 142-146）、令牌读写 336-346、认证端点白名单 351-356、刷新 402-408、清会话 437-451、注入头 533-540、401 重放 550-570、错误抛出与解包 572-589
- `apps/web/src/services/uploadService.ts`：`withAuth` 180-183、`baseUrl` 188-193、snake_case body 201-211
- `apps/web/src/services/oauthClientService.ts`：129-146（4 条命令的请求/响应类型）
- `apps/web/src/app/settings/oauth-clients/page.tsx`：列表调用 214、删除 359、`OAUTH_ENABLED` 说明 413、一次性密钥弹窗 560-580（保存提示 564-570）
核查过的引用：`parse_credential` 205-233、`auth.rs` 注释 410-418、中间件 419、`authz` 单测 1094、`oauth_routes` `basic_credentials` 173-201 / `authenticate_request_client` 204-233 / `token_response` 249-269 / 单测 975、`oauth_service` `ensure_user_active` 955 / `verify_access_token` 1421、`user_service` `revoke_all_user_refresh_tokens` 501、`sys_menu` 205-208 + id 49、`httpAdapter` 36/66/422/437、`page.tsx` 214/359/413/560-580。

## 下一步（未开始）

PR-1（D-1 + D-8 + C-6）与 PR-2（D-5 + D-6 + D-7 + C-1…C-5）已于 2026-11-16 完成并全绿
（`cargo test -p api --lib` **106 passed**、`jest` 前端相关套件全绿、`tsc --noEmit` 通过）。

建议下一步：
1. **PR-3**（后端安全行为变）：D-2（`verify_access_token` JOIN 用户状态）、D-3（禁用/删除用户吊销 `sys_oauth_token`）、D-4（OAuth 令牌限流）；B-6…B-10 随 D-2/D-3 一并验。
2. 之后再回到被推迟的 **Module 5（工作流 10 条路由）**，或补 **PR-4** 的 `scripts/smoke/oauth_bearer.sh`（E-1…E-13）。


