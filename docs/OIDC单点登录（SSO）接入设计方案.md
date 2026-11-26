# OIDC 单点登录（SSO）接入设计方案

| 项 | 值 |
|---|---|
| 文档版本 | v0.1（设计稿） |
| 日期 | 2026-11-18 |
| 适用范围 | `apps/backend/src-tauri`（Tauri v2 + axum 0.8.9 + sqlx 0.9 + utoipa 6）、`apps/web`（Next.js 16） |
| 状态 | 待评审，评审通过后进入 Phase A 实施 |
| 关联文档 | `docs/OAuth2.1与API Key统一鉴权设计方案.md`（§3.2 明确「不做 OIDC」）、`docs/多租户数据权限与租户切换设计方案.md`、`docs/前端大整数精度丢失问题(Snowflake ID).md`、`TODO.md`（§认证与鉴权改造） |

> 本文档描述**本系统作为 OIDC RP（Relying Party / OIDC 客户端）**接入企业 IdP（Keycloak / Entra ID / Okta / Authing 等）的方案。
> 结论先行：当前项目**没有 OIDC**；本方案**不推翻** `OAuth2.1与API Key统一鉴权设计方案.md` §3.2 的结论——那条非目标针对的是「本系统作为 AS 对外签发 `id_token`」，本方案的方向相反：**由外部 IdP 签发 `id_token`，本系统只做校验并换发自有 JWT**。

---

## 0. 一页速读

1. **现状无 OIDC**：全仓（排除构建产物）`oidc` / `openid` 关键词 0 命中；`id_token` 仅出现在 `api/src/auth.rs:348` 的 query 参数黑名单里。已实现的 `/.well-known/oauth-authorization-server`（`api/src/oauth_routes.rs:621`）只宣告 OAuth 元数据，`response_types_supported = ["code"]`，无 `userinfo_endpoint` / `id_token_signing_alg_values_supported`。
2. **只做 RP 侧，不做 OP 侧**：第三方集成已经由 API Key + OAuth 2.1 AS 满足（`crates/service/src/oauth_service.rs`），没有对外签发 `id_token` 的业务需求。
3. **id_token 不是会话凭证**：SSO 成功后一律**换发本系统 JWT 对**（`service::user_service` 现有签发 + `sys_user_refresh_token` 轮换族），`id_token` 校验完即丢弃，避免「双令牌体系」。
4. **复用已有会话设施**：浏览器 Cookie 会话直接复用 `sys_user_session` + `api/src/oauth_session.rs`（`ap_session` / `ap_csrf`），不新建第二套会话模型。
5. **身份必须显式绑定**：新增 `public.sys_user_identity(issuer, subject)` 唯一索引；未绑定时 **fail-closed 拒绝登录**（不按用户名/邮箱猜人），管理员显式绑定后才可登录。
6. **缺省全关**：`OIDC_ENABLED` 未设置时不注册任何 OIDC 路由（404），与 `OAUTH_ENABLED` 同款「缺省即安全」策略（`api/src/lib.rs:376`）。
7. **Tauri 桌面模式零影响**：所有校验与 fail-fast 只放在 HTTP 启动路径，不进 `tauri::Builder` 主流程（沿用原文档 §16 T-2 约束）。

---

## 1. 范围

### 1.1 目标

| ID | 目标 | 验收信号 |
|---|---|---|
| O1 | 浏览器端可用企业 IdP 完成 SSO 登录 | 点击「企业账号登录」→ IdP 认证 → 回到 `apps/web` 并拿到本系统 JWT |
| O2 | 桌面端（Tauri）可用 loopback 回调完成 SSO | RFC 8252 流程，回调 `http://127.0.0.1:<随机端口>/oidc/callback` 可用 |
| O3 | 身份与本地账号一一对应且可审计 | `sys_user_identity` 有绑定记录；未绑定用户登录被拒 |
| O4 | 不影响既有登录方式 | SSO 关闭 / IdP 宕机时，用户名密码登录与 API Key / OAuth 2.1 全部照常 |
| O5 | 密钥与令牌不落明文日志 | 日志中无 `code` / `id_token` / `client_secret`；`id_token` 不落库 |

### 1.2 非目标

| 项 | 原因 |
|---|---|
| 本系统作为 **OP / OIDC Provider** 对外签发 `id_token` | 第三方接入已有 API Key + OAuth 2.1 AS；发 `id_token` 需要额外维护 JWKS 与签名密钥轮换 |
| OIDC **动态客户端注册**（RFC 7591）、`request` / `request_uri`（RFC 9101）、mTLS（RFC 8705）、DPoP | 与现有非目标保持一致，企业 IdP 场景用静态注册足够 |
| **Front-Channel / Back-Channel Logout**（OIDC Session Management） | Phase C 再评估；Phase A 只做 RP-Initiated Logout |
| IdP 侧的用户/组织架构同步（SCIM） | 属人力资源域，另立方案 |
| 把 IdP 的 group/role 直接映射成本系统角色 | Phase A 只做身份，不做自动授权；角色仍由本系统 RBAC 管理 |

### 1.3 判定口径（与「不做 OIDC」的关系）

| 结论 | 出处 | 是否被本方案推翻 |
|---|---|---|
| 「不做 OIDC（`id_token` / `userinfo` / 完整 discovery）」 | `docs/OAuth2.1与API Key统一鉴权设计方案.md:116` | ❌ 不推翻。该结论约束的是**本系统 AS 的能力边界** |
| 「`nonce` 忽略」 | 同上 `:618` | ❌ 不推翻。本系统 AS 仍忽略 `nonce`；RP 侧会自己生成并校验 `nonce` |

---

## 2. 现状盘点

### 2.1 认证与令牌

| 能力 | 现状 | 位置 | OIDC 所需缺口 |
|---|---|---|---|
| JWT 签发 | HS256，`JWT_SECRET`（缺省回落默认串）；`iss` / `aud` 可配 | `service/src/user_service.rs:163-250`；`jwt_issuer()` / `jwt_audience()` `:42-57` | 无需改动（SSO 复用它签发） |
| JWT 校验 | `decode::<Claims>` + `Validation` 强制 `aud` / `iss` | `api/src/auth.rs:246-287` | 需**新增独立**的 IdP 校验器（RS256/ES256 + JWKS），**不得复用** HS256 分支 |
| Claims 结构 | `sub` / `username` / `exp` / `iat` / `typ` / `jti` / `iss` / `aud` | `service/src/user_service.rs:13-34` | 无需改动 |
| 刷新令牌轮换 | `jti` + `family_id` + 重放整族撤销 | `user_service.rs:256-360`、`public_tables.sql:280` | 无需改动 |
| 凭证前缀分流 | `eyJ` → JWT、`ak_` → API Key、`at_` → OAuth 令牌 | `api/src/auth.rs:205-233` | 无需改动（SSO 不引入新前缀） |
| query 传令牌拦截 | 拒绝 `access_token` / `id_token` / `token` / `api_key` | `api/src/auth.rs:343-361` | 无需改动；RP 回调**只能用 `code` + `state`** |

### 2.2 会话与 Cookie

| 能力 | 现状 | 位置 |
|---|---|---|
| 会话表 | `sys_user_session(session_hash, user_id, tenant_id, csrf_hash, ip, user_agent, expires_at, revoked_at, last_seen_at)`，只存 SHA-256 | `public_migration.sql:722-748` |
| Cookie 工具 | `ap_session`（HttpOnly + SameSite=Lax + Path=/，TLS 下 Secure）、`ap_csrf`（双提交） | `api/src/oauth_session.rs:19-113` |
| 会话签发 | `issue_session_cookies(headers, user_id, tenant_id)` | `api/src/oauth_browser.rs:472` |
| 登录/登出接线 | 登录成功后追加 `Set-Cookie`；登出撤销会话 + 清 Cookie | `api/src/user_routes.rs:96-126`、`:187-210` |
| 会话服务 | `create_session` / `find_session` / `rotate_csrf_token` / `revoke_session` / `revoke_all_user_sessions` / `cleanup_expired_sessions` | `service/src/session_service.rs:74-221` |

> 结论：**SSO 不需要新建会话表**，`issue_session_cookies` 直接复用即可。

### 2.3 路由与鉴权自检

| 事实 | 位置 | 影响 |
|---|---|---|
| 公开路由在 `create_router` 里手工组装，OAuth 端点按 `is_enabled()` 条件挂载 | `api/src/lib.rs:367-388`、`:790-794` | 新增 OIDC 公开路由须挂在这里，且同样条件挂载 |
| 鉴权自检用 `PUBLIC_ROUTES_DECL` 对账 | `api/src/authz.rs:144-157`、`audit_route_coverage()` `:761-788` | **必须**登记新公开路由，否则 `cargo test -p api authz`（`:939` `auth_only_and_public_routes_are_not_in_perm_table`）与启动自检会报不一致 |
| `auth.rs` 另有一份 `PUBLIC_ROUTES` 常量（仅 1 条，已是双份事实来源） | `api/src/auth.rs:579` | 本方案**不再**往这份加，改为统一在 `authz.rs` 的 `PUBLIC_ROUTES_DECL` 登记（顺手标注该常量待删） |
| 安全响应头中间件（HSTS / nosniff / X-Frame-Options / Referrer-Policy） | `api/src/security.rs:205-250` | 回调页无需额外处理 |
| CORS `allow_origin(Any)` + `allow_headers(Any)` | `api/src/lib.rs:192-201` | Phase A 不改；若前端跨域调 `/api/auth/oidc/*` 需评估收紧（见 §12 R6） |

### 2.4 依赖与前端

| 项 | 现状 | 结论 |
|---|---|---|
| `reqwest` | 工作区 `0.13.5`（`default-features = false` + `rustls`），由 `llm-anthropic` / `llm-openai` / `sandbox-cube` 各自持有 | 直接复用，`api`/`service` 增加 `reqwest.workspace = true` 即可，**不引入新库** |
| `jsonwebtoken` | `11.1.0`，`default-features = false, features = ["aws_lc_rs"]` | `aws_lc_rs` provider 支持 RS256 / ES256 / EdDSA 验签，满足 IdP 常见算法（当前代码只用 HS256） |
| OIDC 专用 crate | 无 `openidconnect` / `oauth2` | 手写 discovery + JWKS 缓存即可，避免引入 `oauth2` 与既有 `utils::oauth` 语义冲突 |
| 前端令牌存储 | `localStorage`（`access_token` / `refresh_token`）+ 401 自动 refresh | SSO 成功后沿用同一套存储与拦截器 |
| 登录页 | `apps/web/src/app/login/page.tsx`、`services/authService.ts` | 增加「企业账号登录」入口 |
| 路由登记 | `apps/web/src/utils/adapters/httpAdapter.ts` | 登记新增端点（与 `oauthClientService` 的既有做法一致） |

---

## 3. 总体设计

### 3.1 角色与信任边界

```
浏览器/桌面  --(1) /api/auth/oidc/login-->  本系统(RP)  --(2) 302-->  IdP (OP)
     |                                          |                      |
     |                                    (3) state/nonce/PKCE 落库     |
     |  <--(4) 302 code+state--------------------|<--(5) 用户认证--------|
     |                                          |
     +--(6) GET /api/auth/oidc/callback?code&state-->  本系统
                                                |
                                         (7) POST token_endpoint（code+verifier+client_secret）
                                         (8) 拉 JWKS → 验 id_token（iss/aud/nonce/exp/签名）
                                         (9) 查 sys_user_identity(issuer, sub)
                                        (10) 签发本系统 JWT 对 + ap_session Cookie
```

信任边界：

- **IdP 是身份的权威来源**，但只信任其 `id_token` 的 `sub`；
- 本系统的 RBAC（`sys_menu.perms` + `authz_middleware`）**不因 SSO 而放松**，仍按本地角色判权；
- `id_token` / IdP 的 `access_token` **不得**透传给前端或存入 `localStorage`。

### 3.2 两条登录路径

| 路径 | 客户端 | redirect_uri | 机密性 | 说明 |
|---|---|---|---|---|
| A. Web（浏览器） | `apps/web` | `https://<api域名>/api/auth/oidc/callback` | confidential（有 `client_secret`） | 回调落在后端，避免 CORS 暴露 `client_secret`；前端只做落地页 |
| B. Desktop（Tauri） | 桌面壳 | `http://127.0.0.1:<随机端口>/oidc/callback` | public + PKCE（RFC 8252 §7.3） | 端口随机，匹配时**忽略端口**（与 `oauth_service` 的回调校验同款特例，需单独函数并写注释） |

> 两条路径都强制 PKCE S256（含 confidential client）。`client_secret` 只用于 `token_endpoint` 的 `client_secret_basic` / `client_secret_post`。

### 3.3 关键决策

| 决策 | 选择 | 理由 / 备选 |
|---|---|---|
| SSO 成功后的会话形态 | 本系统 JWT 对 + `ap_session` Cookie | 与现有 `httpAdapter`、`authStore`、`authz_middleware` 零改造；备选「只发 Cookie 不发 JWT」需重写前端拦截器 |
| `id_token` 用途 | 只做一次性身份校验 | 避免两套令牌；`id_token` 短 TTL 且不含本系统权限位 |
| `userinfo` 端点 | Phase B 可选，仅取 `name` / `picture` 展示 | 身份判定一律用 `sub`，不用 `email`（`email` 可能变更或未验证） |
| 首次登录未绑定 | **拒绝**（403 `oidc_not_linked`） | fail-closed；`OIDC_AUTO_LINK_BY_EMAIL=true` 才按邮箱匹配（默认关） |
| 账号创建 | `OIDC_JIT_ENABLED` 默认 false；开启时落指定租户 + 默认角色 | 防止 IdP 任意用户自动成为本系统用户 |
| 与 OAuth 2.1 AS 的关系 | 完全隔离（不同 URL 前缀、不同表、不同开关） | RP 的 `/api/auth/oidc/*` 不回写 `sys_oauth_*`；AS 的 `/oauth/*` 不感知 `sys_user_identity` |

### 3.4 时序（Web 路径，文本版）

```
用户点击「企业账号登录」
  → GET /api/auth/oidc/login?return_to=/hardware
     · 生成 state(32B) / nonce(32B) / code_verifier(43-128)
     · INSERT sys_oidc_login_state(state_hash, nonce, verifier, return_to, expires_at=now+10min)
     · Set-Cookie: ap_oidc=ss_<43>; HttpOnly; SameSite=Lax; Path=/; Max-Age=600
     · 302 → IdP authorization_endpoint?response_type=code&client_id=...
             &redirect_uri=...&scope=openid+profile+email&state=...
             &nonce=...&code_challenge=...&code_challenge_method=S256
  → IdP 认证
  → 302 → /api/auth/oidc/callback?code=...&state=...
     · 校验 ap_oidc Cookie 与 state_hash 存在且未过期（常数时间比较）
     · POST token_endpoint（code + code_verifier + client_secret_basic）
     · 拉 JWKS（kid 匹配）→ 验 id_token 签名与声明（iss/aud/azp/exp/iat/nbf/nonce，时钟偏移 ≤ 60s）
     · SELECT user_id FROM sys_user_identity WHERE issuer=$1 AND subject=$2
        · 未命中 → 403 oidc_not_linked（页面提示联系管理员）
     · ensure_user_active(user_id)（复用 oauth_service::ensure_user_active）
     · 签发 JWT 对（复用 user_service）+ issue_session_cookies
     · 302 → return_to（白名单校验，默认 /）
```

---

## 4. 端点设计

所有端点位于 `/api/auth/oidc/*`，**错误体沿用现有 `ApiResponse {code,message}`**（这是本系统自有登录接口，不是 OAuth 协议端点，**不得**套用 `oauth_error` 的 `{error,error_description}`）。

| 端点 | 方法 | 认证 | 说明 |
|---|---|---|---|
| `/api/auth/oidc/login` | GET | 无 | 生成 state/nonce/PKCE → 302 到 IdP；参数 `return_to`（站内相对路径，白名单校验） |
| `/api/auth/oidc/callback` | GET | 无（靠 `ap_oidc` Cookie + state） | 换 token、验 `id_token`、建本地会话、302 回前端 |
| `/api/auth/oidc/providers` | GET | 无 | 登录页展示可用 IdP（只返回 `id` / `display_name`，不返回 secret） |
| `/api/auth/oidc/logout` | POST | 需登录 | RP-Initiated Logout：撤销会话 + 撤销刷新令牌族；配了 `OIDC_END_SESSION_ENDPOINT` 时返回 `redirect_to` |
| `/api/oidc/identities/{userId}` | GET / POST / DELETE | 管理端 JWT + `system:user:edit` | 查询 / 绑定 / 解绑（Phase B，管理界面用） |

### 4.1 `/api/auth/oidc/callback` 错误码

| 场景 | HTTP | code | 前端表现 |
|---|---|---|---|
| `state` 缺失 / 过期 / 不匹配 | 400 | `oidc_invalid_state` | 「登录已超时，请重试」 |
| IdP 返回 `error=access_denied` | 302 | — | 跳回登录页并提示用户取消 |
| `token_endpoint` 调用失败 / 非 200 | 502 | `oidc_token_exchange_failed` | 「身份服务暂不可用」 |
| `id_token` 验签或声明校验失败 | 401 | `oidc_invalid_id_token` | 同上，记 WARN（不含令牌内容） |
| `issuer` 与配置不一致 | 401 | `oidc_issuer_mismatch` | 同上 |
| 身份未绑定本地账号 | 403 | `oidc_not_linked` | 「账号未开通，请联系管理员」 |
| 本地账号 `status != 1` | 403 | `oidc_user_disabled` | 「账号已禁用」 |
| `return_to` 不在白名单 | 302 | — | 回落到 `/`（不报错，避免探测） |

### 4.2 `return_to` 白名单规则

- 只允许**站内相对路径**（以单个 `/` 开头，拒绝 `//`、`\`、`:`、`@`、控制字符）；
- 先 `percent-decode` 再校验一次（防 `/%2F%2Fevil.com`）；
- 实现为独立纯函数 + 单测（参考 `oauth_service` 中回调地址校验的写法）。

---

## 5. 数据模型

> 遵循原文档 §16 T-5「加法式 DDL」：只新增表/索引，**不修改既有表结构**；DDL 追加到 `crates/database/src/sql/public_migration.sql`（由 `database/src/postgres.rs:757` 通过 `include_str!` 在启动时幂等执行）。

### 5.1 `sys_user_identity`（IdP 身份 ↔ 本地账号）

```sql
-- OIDC RP：外部 IdP 身份与本地账号的绑定关系（设计文档 §5.1）
--   `(issuer, subject)` 唯一：同一 IdP 的一个用户只能绑一个本地账号；
--   `(user_id, issuer)` 唯一：一个本地账号对同一 IdP 只保留一条绑定，换绑即 UPDATE。
CREATE TABLE IF NOT EXISTS public.sys_user_identity (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    issuer VARCHAR(255) NOT NULL,
    subject VARCHAR(255) NOT NULL,
    email VARCHAR(255),
    display_name VARCHAR(255),
    created_at TIMESTAMPTZ DEFAULT now(),
    last_login_at TIMESTAMPTZ
);

COMMENT ON TABLE public.sys_user_identity IS '外部 OIDC IdP 身份绑定（issuer + subject 唯一）';

COMMENT ON COLUMN public.sys_user_identity.issuer IS 'IdP 的 issuer（与 id_token.iss 等值比较）';

COMMENT ON COLUMN public.sys_user_identity.subject IS 'IdP 的 subject（id_token.sub，唯一身份标识）';

COMMENT ON COLUMN public.sys_user_identity.email IS '最近一次登录时的 email（仅展示/排查，不参与身份判定）';

CREATE UNIQUE INDEX IF NOT EXISTS uk_sys_user_identity_issuer_subject
    ON public.sys_user_identity (issuer, subject);

CREATE UNIQUE INDEX IF NOT EXISTS uk_sys_user_identity_user_issuer
    ON public.sys_user_identity (user_id, issuer);

CREATE INDEX IF NOT EXISTS idx_sys_user_identity_user
    ON public.sys_user_identity (user_id);
```

- 主键 `id` 用 `utils::snowflake::next_id()`（与 `sys_oauth_*` 一致）；
- 不建外键（与 `sys_api_key` / `sys_oauth_*` 风格一致），由服务层保证引用完整性；
- **不在 `sys_user` 上加列**，避免污染 `SysUser` 模型与 Tauri 命令的返回结构。

### 5.2 `sys_oidc_login_state`（一次性登录态）

```sql
-- OIDC RP：authorize 阶段的一次性登录态（state / nonce / PKCE verifier）
--   只存 state 的 SHA-256（与 utils::oauth::hash_secret 同策略）；
--   nonce 需要原文参与 id_token 比对，故明文存储（TTL 10 分钟且一次性）。
CREATE TABLE IF NOT EXISTS public.sys_oidc_login_state (
    id BIGINT PRIMARY KEY,
    state_hash CHAR(64) NOT NULL,
    nonce VARCHAR(128) NOT NULL,
    code_verifier VARCHAR(128) NOT NULL,
    redirect_uri TEXT NOT NULL,
    return_to VARCHAR(512),
    client_ip INET,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS uk_sys_oidc_login_state_hash
    ON public.sys_oidc_login_state (state_hash);

CREATE INDEX IF NOT EXISTS idx_sys_oidc_login_state_expires
    ON public.sys_oidc_login_state (expires_at);
```

- TTL 10 分钟、一次性（`used_at` 置位即失效）；
- 清理复用 `spawn_cleanup_task()`（`api/src/lib.rs:834`）：新增 `oidc_service::cleanup_login_states()`（`used_at` / 过期后保留 24h），不挂在请求路径上。

### 5.3 权限点

Phase B 的绑定管理界面复用既有 `system:user:edit`，**不新增权限点**（避免 `sys_menu` id 段、`ROUTE_PERMS`、`public_initial_data.sql` 三处同步的成本）。

---

## 6. 身份映射与账号生命周期

### 6.1 绑定策略矩阵

| `OIDC_AUTO_LINK_BY_EMAIL` | `OIDC_JIT_ENABLED` | 未命中绑定时的行为 | 适用 |
|---|---|---|---|
| false（默认） | false（默认） | 403 `oidc_not_linked` | 生产推荐：管理员先在「用户管理」绑定 |
| true | false | 按 `email` 精确匹配**唯一**本地账号 → 自动写绑定；匹配 0 条或多条则拒绝 | 存量用户批量迁移期 |
| false | true | 在 `OIDC_JIT_TENANT_ID` + `OIDC_JIT_DEFAULT_ROLE_ID` 下建号并绑定 | 小规模、IdP 即权威账号源 |
| true | true | 先按邮箱匹配，未命中再建号 | 不推荐（隐式建号风险） |

自动绑定 / 建号的额外前置条件（无论哪一档）：

1. `id_token.email_verified == true`（缺少该声明视为未验证 → 拒绝自动绑定）；
2. 本地账号 `status = 1` 且未软删除；
3. 邮箱在 `sys_user` 中**唯一**（`SELECT count(*) = 1`）。

### 6.2 解绑 / 禁用 / 删除

| 事件 | 处理 |
|---|---|
| 管理员解绑 | `DELETE sys_user_identity` + `revoke_all_user_sessions(user_id)` + 撤销该用户全部刷新令牌族 |
| 用户禁用（`status=0`） | 登录时 `ensure_user_active` 拒绝；已存在会话按既有逻辑失效 |
| 用户删除（软删） | 保留绑定行（审计），登录时因账号不存在 / 已删被拒 |
| IdP 侧删除用户 | 本系统**不感知**；下次 SSO 因 `sub` 仍匹配到原账号（如需同步见 §1.2 SCIM 非目标） |

### 6.3 多租户归属

- SSO **不改变**租户归属：仍取 `sys_user.tenant_id`；
- 登录响应与密码登录保持同形（`id` / `tenant_id` / `available_tenants`），前端 `authStore` 无需分支；
- `id_token` 中的 `tenant` / `org` 类声明 **不参与**本系统租户判定（避免 IdP 配置变更引发越权）。

---

## 7. 安全设计

### 7.1 `id_token` 校验清单（顺序敏感）

| # | 校验项 | 规则 | 失败码 |
|---|---|---|---|
| 1 | 结构 | `header.payload.signature` 三段，`alg` ∈ 配置白名单（默认 `RS256`、`ES256`）；**显式拒绝 `none`** | `oidc_invalid_id_token` |
| 2 | 签名 | 按 `kid` 从 JWKS 取公钥验签；`kid` 缺失且 JWKS 仅一把 key 时可回退 | 同上 |
| 3 | `iss` | 与 `OIDC_ISSUER` **精确字符串相等** | `oidc_issuer_mismatch` |
| 4 | `aud` | 必须包含 `OIDC_CLIENT_ID` | `oidc_invalid_id_token` |
| 5 | `azp` | 多 `aud` 时必填且等于 `client_id` | 同上 |
| 6 | `exp` / `iat` / `nbf` | `now` 与三者比较，允许 `±OIDC_CLOCK_SKEW_SECS`（默认 60s）；`exp` 必填 | 同上 |
| 7 | `nonce` | 与 `sys_oidc_login_state.nonce` **常数时间比较** | 同上 |
| 8 | `sub` | 非空、长度 ≤ 255 | 同上 |
| 9 | `at_hash` | 若存在则校验（可选，默认不开） | 同上 |

> 校验实现放在**独立函数**（`service::oidc_service::validate_id_token(...)`），入参为「token + JWKS + 期望值」，**纯函数化**便于单测；**禁止**与 `api::auth::verify_token`（HS256）共用 `Validation`。

### 7.2 JWKS 缓存与轮换

| 项 | 方案 |
|---|---|
| 获取 | 首次使用时 `GET OIDC_JWKS_URI`（可由 discovery 得到），`reqwest` + `rustls`，超时 `OIDC_HTTP_TIMEOUT_SECS`（默认 10s） |
| 缓存 | 进程内 `DashMap`（与 `auth.rs::authority_cache()` 同风格）+ TTL `OIDC_JWKS_CACHE_SECS`（默认 3600s） |
| 轮换 | 验签遇未知 `kid` → **强制刷新一次** JWKS（限频：同一 `kid` 60s 内最多 1 次），仍失败则拒绝 |
| 失败 | JWKS 不可用 → SSO 登录失败（fail-closed），但**不**影响既有 JWT 校验与密码登录 |
| 多实例 | 每实例独立缓存；无需广播（JWKS 是幂等只读资源） |

### 7.3 state / nonce / PKCE

| 项 | 规则 |
|---|---|
| `state` | 32 字节随机（`base64url`），**只存 SHA-256**；与 `ap_oidc` Cookie 双绑定（Cookie 值与服务端记录一一对应） |
| `nonce` | 32 字节随机，明文入库（需参与比对），一次性 |
| `code_verifier` | 43–128 字符（复用 `utils::oauth::is_valid_code_verifier`），S256 强制（`utils::oauth::code_challenge_s256`） |
| 生命周期 | TTL 10 分钟；回调成功 / 失败都置 `used_at`（失败也作废，防重放） |
| 重放 | `used_at` 非空再次回调 → 400 `oidc_invalid_state`，并 WARN 记录 IP |

### 7.4 传输与 Cookie

| 项 | 规则 |
|---|---|
| IdP 端点 | 必须 `https://`；`http://` 仅允许 `localhost` / `127.0.0.1`（本地 IdP 调试），且需 `OIDC_ALLOW_INSECURE_HTTP=true` 显式开启并打 WARN 横幅 |
| 回调 Cookie | `ap_oidc=ss_<43>; HttpOnly; SameSite=Lax; Path=/; Max-Age=600`，TLS 下 `Secure`（复用 `oauth_session::cookie_secure()`） |
| 会话 Cookie | 复用 `ap_session`（`oauth_session::build_session_cookie`），**不新造** |
| CSRF | 状态转换靠 `state`；`/api/auth/oidc/logout` 走 `POST` + `ap_csrf` 双提交 |
| 缓存 | 登录 / 回调响应 `Cache-Control: no-store` |

### 7.5 日志脱敏

- **禁止**记录：`code`、`id_token`、IdP 的 `access_token`、`client_secret`、`code_verifier`；
- 允许记录：`state` 前 8 字符、`issuer`、`sub` 的 SHA-256 前 12 字符、HTTP 状态码、耗时；
- 返回前端的错误只给固定文案 + `code`，不透传 IdP 原始报文（避免泄漏 IdP 内部结构）。

### 7.6 防爆破与限流

- `/api/auth/oidc/login` 与 `/api/auth/oidc/callback` 按 IP 限流，阈值 60 次/分钟（与 API Key 限流设施同源）；
- 连续 10 次 `oidc_invalid_state` / `oidc_invalid_id_token` 的 IP 记 WARN（不自动封禁，避免 NAT 误伤）。

### 7.7 桌面端（Tauri）注意点

| 点 | 说明 |
|---|---|
| 回调端口 | 随机端口 → 匹配时忽略端口（RFC 8252 §7.3），单独函数 `matches_loopback_redirect()` 并写注释（同 `oauth_service` 既有约定） |
| 打开浏览器 | 用 `@tauri-apps/plugin-opener`（已在 `apps/web/package.json` 依赖中）打开系统浏览器，**不用**内嵌 WebView（IdP 普遍禁止） |
| 本地回调接收 | 桌面端临时起本地监听接收 `code`，再转发给后端 `/api/auth/oidc/callback`；`client_secret` 只留在服务端 |
| Cookie | 桌面端不依赖 `ap_session`；JWT 走既有桌面登录路径（与密码登录一致） |

---

## 8. 配置项

| 变量 | 必填 | 默认 | 说明 |
|---|---|---|---|
| `OIDC_ENABLED` | 否 | 未设置 = 关闭 | 关闭时 `/api/auth/oidc/*` 全部 404（同 `OAUTH_ENABLED` 语义，`api/src/lib.rs:376`） |
| `OIDC_ISSUER` | 开启时必填 | — | 例：`https://keycloak.example.com/realms/corp`；与 `id_token.iss` 精确比较 |
| `OIDC_DISCOVERY_URL` | 否 | `${OIDC_ISSUER}/.well-known/openid-configuration` | 首次使用时拉取；失败仅影响 SSO |
| `OIDC_CLIENT_ID` | 是 | — | IdP 侧注册的 client_id |
| `OIDC_CLIENT_SECRET` | 否 | 空 = public client | 空时强制 PKCE；非空时 `client_secret_basic` |
| `OIDC_SCOPES` | 否 | `openid profile email` | 必须包含 `openid`，否则启动时 WARN 并视为配置错误 |
| `OIDC_REDIRECT_URI` | 是 | — | 必须与 IdP 注册值**完全一致**；loopback 例外见 §7.7 |
| `OIDC_POST_LOGIN_REDIRECT` | 否 | `/` | 登录成功后前端落地页（仍受 `return_to` 白名单约束） |
| `OIDC_END_SESSION_ENDPOINT` | 否 | 空 | RP-Initiated Logout 时返回给前端 |
| `OIDC_AUTO_LINK_BY_EMAIL` | 否 | `false` | 见 §6.1 |
| `OIDC_JIT_ENABLED` | 否 | `false` | 见 §6.1 |
| `OIDC_JIT_TENANT_ID` | JIT 开启时必填 | — | 新建用户的 `sys_user.tenant_id` |
| `OIDC_JIT_DEFAULT_ROLE_ID` | 否 | 空 = 不分配角色 | 新建用户默认角色（空则登录后无菜单） |
| `OIDC_CLOCK_SKEW_SECS` | 否 | `60` | 时间声明容差 |
| `OIDC_HTTP_TIMEOUT_SECS` | 否 | `10` | JWKS / `token_endpoint` 调用超时 |
| `OIDC_JWKS_CACHE_SECS` | 否 | `3600` | JWKS 缓存 TTL |
| `OIDC_ALLOW_INSECURE_HTTP` | 否 | 未设置 | 允许 http IdP（仅本地调试），开启时启动日志打 WARN |

校验时机：**只在 HTTP 启动路径**（`api/src/lib.rs::start_http_server_with`）做「`OIDC_ENABLED` 为真但必填项缺失 → 返回 `Err` 拒绝启动 HTTP」，**不 panic、不影响 Tauri 进程**（沿用原文档 §16 T-2 / T-6）。

---

## 9. 代码落点（文件级改动清单）

| 文件 | 改动 | 类型 |
|---|---|---|
| `crates/database/src/sql/public_migration.sql` | 追加 §5 两张表 + 索引 + `COMMENT ON`（幂等） | 追加 |
| `crates/service/src/oidc_service.rs` | 新增：discovery 缓存、JWKS 缓存、token 交换、`validate_id_token`、身份查询 / 绑定、`cleanup_login_states` | 新增文件 |
| `crates/service/src/lib.rs` | `pub mod oidc_service;` | 1 行 |
| `crates/api/src/oidc_routes.rs` | 新增：`login` / `callback` / `providers` / `logout` + `public_oidc_routes()` + `is_enabled()` | 新增文件 |
| `crates/api/src/lib.rs` | `create_router` 里按 `oidc_routes::is_enabled()` merge 公开路由（`:367-388`）；`spawn_cleanup_task` 增加清理调用（`:834`） | 小改 |
| `crates/api/src/authz.rs` | `PUBLIC_ROUTES_DECL` 增加 3 条（`:144-157`）；`AUTH_ONLY_ROUTES` 增加 `POST /api/auth/oidc/logout` | 小改 |
| `crates/api/src/auth.rs:579` | 不再维护第二份清单；加 `TODO` 注释指向 `authz.rs` | 注释 |
| `crates/api/src/health.rs` | `/healthz` 增加 `oidc: {enabled, issuer_ok}`（只报布尔，不泄漏配置） | 小改 |
| `crates/api/Cargo.toml` / `crates/service/Cargo.toml` | 增加 `reqwest.workspace = true` | 依赖 |
| `apps/web/src/services/oidcService.ts` | 新增：`loginUrl()` / `providers()` / `logout()` | 新增文件 |
| `apps/web/src/app/login/page.tsx` | 增加「企业账号登录」按钮（`providers` 探测失败则不展示） | 小改 |
| `apps/web/src/app/auth/oidc/callback/page.tsx` | 新增回调落地页：展示结果 / 错误码 | 新增文件 |
| `apps/web/src/utils/adapters/httpAdapter.ts` | 登记新端点（公开路由不附加 `Authorization`） | 小改 |
| `apps/web/src/store/authStore.ts` | SSO 结果落地（与密码登录同构，`id` 按字符串处理，遵守雪花 ID 精度约定） | 小改 |

**明确不改**：`crates/api/src/oauth_routes.rs`、`oauth_browser.rs`、`oauth_session.rs`、`crates/utils/src/oauth.rs`（只**调用**，不改语义）；`sys_user` 表结构；Tauri `invoke` 命令签名。

---

## 10. 实施阶段与验收信号

| 阶段 | 内容 | 工作量 | 验收信号 |
|---|---|---|---|
| **Phase A** | DDL + `oidc_service` 核心（discovery / JWKS / 验签 / 换 token）+ `login` / `callback` + 浏览器路径 + 绑定（SQL 脚本级） | 3–5 天 | 用 Keycloak 本地实例完成一次 SSO，拿到本系统 JWT 且 `/api/users/me` 200 |
| **Phase B** | 管理界面（绑定 / 解绑）+ `providers` + RP-Initiated Logout + `OIDC_AUTO_LINK_BY_EMAIL` / `OIDC_JIT_*` 开关 | 2–3 天 | 解绑后登录被拒（403 `oidc_not_linked`） |
| **Phase C** | Tauri loopback 回调 + 前端 SSO 入口 + 清理任务 + `/healthz` 状态 | 2 天 | 桌面端完成 SSO；`OIDC_ENABLED` 关闭时端点 404 |
| **Phase D**（可选） | Back-Channel Logout、`userinfo` 展示、多 IdP 并存 | — | 待产品确认 |

上线开关顺序：先 `OIDC_ENABLED=false` 合入代码（零行为变化）→ 配好 IdP 与绑定关系 → 再开开关。

---

## 11. 测试计划

### 11.1 单测（纯函数，`cargo test -p service -p api`）

| 用例 | 断言 |
|---|---|
| `return_to` 白名单 | 接受 `/hardware`；拒绝 `//evil`、`https://evil`、`/%2F%2Fevil`、`/\evil`、空串 |
| `validate_id_token` 矩阵 | 错误 `alg`（含 `none`）/ 错误 `iss` / 缺 `aud` / 多 `aud` 缺 `azp` / `exp` 过期 / `iat` 超前超过偏移 / `nonce` 不匹配 / 签名错误 / 未知 `kid` → 各自错误码 |
| JWKS 选择 | `kid` 命中；`kid` 未命中触发刷新；刷新后仍未命中 → 失败 |
| PKCE | 复用 `utils::oauth` 的 RFC 7636 测试向量（已有 `pkce_known_vector_from_rfc7636_appendix_b`） |
| loopback 匹配 | `http://127.0.0.1:1234/cb` 与登记值端口不同仍匹配；公网 `http://` 不匹配 |
| 绑定规则 | 邮箱匹配 0 / 1 / 多条；`email_verified=false` 拒绝自动绑定 |
| 路由自检 | `cargo test -p api authz`（`authz.rs:939`）通过；`PUBLIC_ROUTES_DECL` 含新增 3 条 |

### 11.2 集成 / 契约测试

- 本地起一个**最小 IdP stub**（axum 路由：`/.well-known/openid-configuration`、`/jwks`、`/token`），用临时 RSA 密钥签发 `id_token`，覆盖「成功 / 过期 / 错误 `nonce` / 错误签名」四条路径；
- 断言：成功路径返回本系统 JWT + `Set-Cookie: ap_session`；失败路径不产生 `sys_user_session` 行。

### 11.3 手测用例

| # | 场景 | 期望 |
|---|---|---|
| 1 | 已绑定用户浏览器 SSO | 登录成功，`/api/users/me` 200 |
| 2 | 未绑定用户 SSO | 403 `oidc_not_linked`，无会话产生 |
| 3 | 用户取消授权（`error=access_denied`） | 回登录页，无错误码弹窗 |
| 4 | `state` 被篡改 | 400 `oidc_invalid_state` |
| 5 | IdP 停机 | 502 `oidc_token_exchange_failed`；密码登录仍可用 |
| 6 | `OIDC_ENABLED` 未设置 | `/api/auth/oidc/login` 404 |
| 7 | 关闭开关后已登录用户 | 既有 JWT 与会话不受影响 |
| 8 | 解绑后再 SSO | 403 `oidc_not_linked` |

---

## 12. 风险与待确认问题

### 12.1 风险

| ID | 风险 | 影响 | 缓解 |
|---|---|---|---|
| R1 | 反代后 `issuer` / `redirect_uri` 与实际不一致 | 登录 302 回不来 | 显式配置 `OIDC_REDIRECT_URI`（不按 Host 推导）；仅在 `TRUST_PROXY_HEADERS=true` 时信任 `X-Forwarded-*`（`api/src/security.rs:127`） |
| R2 | 多租户下「SSO 用户属于哪个租户」不明确 | 登录后看不到数据 | 由 `sys_user.tenant_id` 决定；JIT 必须显式配 `OIDC_JIT_TENANT_ID`，否则拒绝建号 |
| R3 | 邮箱自动绑定被滥用（IdP 允许改邮箱 / 未验证） | 账号接管 | 默认关闭；强制 `email_verified` + 唯一匹配；每次自动绑定写审计日志 |
| R4 | IdP 令牌算法 / 密钥轮换 | 登录失败 | 算法白名单 + 未知 `kid` 触发限频刷新；JWKS 不可用时 fail-closed 但不影响密码登录 |
| R5 | 桌面端把 `client_secret` 打包进二进制 | 密钥泄漏 | 桌面端只用 public client + PKCE；confidential 流程只在服务端 |
| R6 | CORS `allow_origin(Any)`（`api/src/lib.rs:193`）与 Cookie 会话叠加 | 跨站请求携带 Cookie 的风险 | `ap_session` 已是 `SameSite=Lax`；如需跨域前端，改为显式白名单（与 OAuth 文档 §14 R4 同一议题） |
| R7 | 新增公开路由未登记 `PUBLIC_ROUTES_DECL` | CI 测试失败 / 自检告警 | §9 已列为必改项，并加测试 |

### 12.2 待确认问题

| # | 问题 | 候选答案 |
|---|---|---|
| Q1 | 目标 IdP 是哪一个（Keycloak / Entra ID / Okta / 自研）？ | 影响 discovery 细节、`azp` 行为、是否支持 `end_session_endpoint` |
| Q2 | SSO 用户是否允许 JIT 建号？默认租户与默认角色如何定？ | 建议 Phase A 关闭 JIT，先手工绑定 |
| Q3 | 是否保留密码登录作为兜底（IdP 故障时）？ | 建议保留（本方案默认保留） |
| Q4 | Web 的回调地址放在后端还是前端域名？ | 建议后端（`/api/auth/oidc/callback`），前端只做落地页 |
| Q5 | 是否需要 IdP 组 → 本系统角色的映射？ | 建议 Phase D 再评估，先只做身份 |

---

## 附录 A：与「本项目不做 OIDC」结论的一致关系

`docs/OAuth2.1与API Key统一鉴权设计方案.md` §3.2 第 116 行的非目标，约束的是**本系统作为 OAuth 2.1 授权服务器**的能力边界：

- 该 AS 面向**第三方系统代用户访问本系统资源**，用 `authorization_code + PKCE` 换 opaque `at_` 令牌；
- 为本系统「增加 OIDC Provider 能力」既无业务需求，又会引入 JWKS / 签名密钥轮换 / `userinfo` 等额外面。

本方案方向相反：**本系统作为 RP**，去消费企业 IdP 的 `id_token`，且校验完立即丢弃。因此：

| 问题 | 答案 |
|---|---|
| 原结论是否失效？ | 否，仍然成立 |
| 本方案是否需要在原文档中加引用？ | 是，已在 §3.2 非目标下补充一行指向本文档 |
| 两套开关是否互相影响？ | 否，`OAUTH_ENABLED` 与 `OIDC_ENABLED` 完全独立 |
