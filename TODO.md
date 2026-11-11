# 架构修复清单

## 🔴 严重问题
- [x] 1. 修复 `postgres.rs` 中 `init_postgres_tables()` 的表结构与 `models.rs` 实体不匹配
- [x] 2. 补充 HTTP API 路由中缺失的端点（reset_password, get_role_menu_ids, assign_role_menus）
- [x] 3. 修复 `api/user_routes.rs` 中字段映射错误（display_name → real_name）
- [x] 4. 统一数据库连接池管理，消除双重管理

## 🟡 中等问题
- [x] 5. 统一前后端参数命名（permissionService.ts 与后端一致）
- [x] 6. 从 JWT 中提取当前用户，替代硬编码 `Some(1)`
- [x] 7. 修复 `api/role_routes.rs` 中路由层直接构造 Role 对象的问题
- [x] 8. 为受保护路由添加 JWT 用户信息提取

## 🟢 轻微问题
- [x] 9. 修复 `AssetCategory.parent_id` 类型为 `Option<i64>`
- [x] 10. 为 `postgres.rs` 中的表添加 `deleted` 字段
- [x] 11. 清理未使用的依赖（opa-wasm, aes, cbc, hmac, pbkdf2, sha2, zeroize）
- [x] 12. 删除无意义的模板测试代码
- [x] 13. 修复 `register_placeholder` 返回格式
- [x] 14. 清理未使用的 `database/sql/` 目录

## 🐛 已修复 Bug
- [x] 15. **Tauri v2 invoke 参数名 camelCase 问题**：`menuService.ts` 中调用 `get_user_menus` 时传参使用 `user_id`（snake_case），但 Tauri v2 的 `invoke()` 要求参数名使用 camelCase，导致 Rust 后端收到 `None`，左侧菜单不显示。修复：将 `{ user_id }` 改为 `{ userId }`。

## ⚠️ 已知问题
- [ ] 16. **前后端长整数精度失真**：数据库主键为 `bigint`（i64），通过 Tauri invoke 序列化为 JSON 时，JavaScript 的 Number 类型无法精确表示超过 2^53 的整数。需要统一处理：Rust 端将 i64 序列化为字符串，前端使用时再转回。涉及所有表的主键 ID 字段。

## 📤 上传流程改进：两步提交（Two-Phase Commit）

### 问题
当前上传流程：选择文件 → init（创建 S3 MultipartUpload） → 直传 S3 → complete（合并）→ 回调里关联业务。文件直接上传到 S3 没有业务上下文，且没有"先占位后上传"机制。

### 改进目标
两步提交：先占位（在数据库创建 record，status=pending，关联 context）→ 再上传 → 上传完成后 commit 关联业务实体。

### file_uploads 表文件版本号设计

通过 `file_group_id` 将同一文件的不同版本归组，`version` 标记版本号。

**新增字段：**
| 字段 | 类型 | 说明 |
|------|------|------|
| `file_group_id` | `VARCHAR(36)` | UUID，同一文件不同版本共用（如替换 report.pdf 时用同一 group_id） |
| `version` | `INTEGER DEFAULT 1` | 版本号，从 1 开始递增 |
| `is_latest` | `BOOLEAN DEFAULT true` | 是否为当前最新版本 |
| `change_reason` | `VARCHAR(500)` | 变更原因，如"更新合同条款" |
| `file_md5` | `VARCHAR(64)` | 文件 MD5，用于判断是否真的变更 |
| `UNIQUE (file_group_id, version)` | 约束 | 同一组内版本号唯一 |

**版本替换流程：**
```
第一次上传：file_group_id = "uuid", version = 1, is_latest = true
第二次替换：step1: UPDATE is_latest=false; step2: INSERT version=2, is_latest=true
```

**S3 Object Key：**
```
旧：uploads/{YYYY-MM}/{uuid}-{filename}
新：uploads/{YYYY-MM}/{file_group_id}/v{version}/{uuid}-{filename}
```

**新增 API：**
- `get_version_history(file_group_id)` → 返回版本列表
- `rollback_to_version(file_group_id, target_version)` → 回滚到指定版本
- 清理策略：保留最近 N 个版本，软删除/硬删除旧版本

### 涉及的表

| 表 | 位置 | 操作 | 说明 |
|----|------|------|------|
| `file_uploads` | `tenant_tables.sql` | ✅ **直接修改** | 新增版本字段（file_group_id/version/is_latest/change_reason/file_md5）+ context 字段（context_type/context_id/commit_at）+ status 增加 pending/committed。**file_uploads 本身就是附件表**，一个业务实体可以通过 context_id 关联多个上传文件，每个文件支持多版本 |
| `knowledge_asset` | `tenant_tables.sql` | 🔗 **仅作为 context 目标** | 通过 `file_uploads.context_id = knowledge_asset.id` 实现一对多关系 |
| `knowledge_tree` | `tenant_tables.sql` | 🔗 间接关联 | 链路：`file_uploads.context_id → knowledge_asset.id → knowledge_tree.id` |
| `asset_documents` | `tenant_tables.sql` | 📌 未来可复用 | 同一套 file_uploads 机制可用于资产文档附件上传 |

### 实施优先级

按依赖关系分为 4 个阶段，每阶段可独立上线：

#### Phase 1：后端基础设施（先做，3 项）
依赖链：DB → Rust 模型 → S3 Key

- [ ] **P1-17.** 数据库表 `file_uploads` 扩展现有字段（`tenant_tables.sql`）
- [ ] **P1-18.** Rust `FileUploadRecord` 模型扩展（`storage/upload.rs`）
- [ ] **P1-25.** S3 Object Key 生成逻辑修改（`storage/upload.rs`）

#### Phase 2：后端核心逻辑（上述完成后，2 项）
依赖链：UploadManager → Tauri 命令

- [ ] **P2-19.** `UploadManager` 重构（`storage/upload.rs`）：init/start_upload/complete/commit 方法拆分 + 版本逻辑
- [ ] **P2-20.** Tauri 命令新增（`commands/upload_commands.rs`）：commit_upload、get_version_history、rollback_version

#### Phase 3：前端基础设施（上述完成后，2 项）
依赖链：Service → Hook

- [ ] **P3-21.** 前端 `uploadService.ts` 改造：新增参数和方法
- [ ] **P3-22.** 前端 `useChunkedUpload.ts` 改造：新增参数，流程改为两步提交

#### Phase 4：前端 UI 集成（上述完成后，2 项）
依赖链：Component → 页面集成

- [ ] **P4-23.** 前端 `FileUploader.tsx` 改造：新增 context/fileGroupId/changeReason 属性
- [ ] **P4-24.** 知识库页面集成测试：传入 context，验证完整上传链路

## 🔐 认证与鉴权改造（设计见 `docs/OAuth2.1与API Key统一鉴权设计方案.md`）

### Phase 0：安全基座（必须先做，1–2 天）
- [x] **26.** `JWT_SECRET` 校验**只加在 `api::start_http_server_with()` 入口**（缺失/过短 → 返回 `Err`，仅 HTTP 不启动；**不改** `auth.rs:43`、`user_service.rs:76`、`src/commands/user_commands.rs:116` 的默认值兜底——Tauri 也在用，见设计文档 §16 T-2/§16.3）
  - 已实现：`crates/api/src/lib.rs` 入口处校验（缺失 / < 32 字节 → `Err`），桌面端兜底逻辑零改动。
- [x] **27.** `/api/upload/*`（7 条）、`/api/chat/stream` 纳入 `auth_middleware`（当前在 `lib.rs:121` 于中间件之外 merge，匿名可访问）
  - 已实现：两个独立 `Router` 各自 `with_state()` 后挂 `.layer(middleware::from_fn(auth_middleware))`。
  - `chat_routes` 改为以令牌 `sub` 为准（body `user_id` 不一致时告警并以令牌为准），避免冒用他人身份。
  - 前端同步带令牌：`uploadService.ts`（8 处 `/api/upload/*` fetch，S3 presigned PUT 不加）、`useChatStream.ts`（SSE），统一走 `utils/authToken.ts`。
- [x] **28.** 删除 `auth.rs:149` 的 `set_global_schema()` 调用（**仅删 HTTP 侧**；`GLOBAL_SCHEMA` 保留给 Tauri，`lib.rs:148` 与 `switch_tenant` 命令语义不变，见设计文档 §16 T-3）
  - 已实现：`auth.rs` 中间件不再写 `GLOBAL_SCHEMA`；另补 `database::global_schema_name()`，在 HTTP 的 `login` / `switch_tenant` 调用共享 service 前后保存 / 恢复快照（service 内部会写 `GLOBAL_SCHEMA`，不改其签名）。
- [x] **29.** 新增 `utils/src/api_key.rs`（`generate_api_key` / `hash_api_key` / `parse_prefix`）+ `sha2` 依赖
  - 已实现：`ak_<env>_<prefix16>_<secret43>`，入库 `SHA-256` hex；含 base32/格式/前缀解析单测；`sha2` 加入 workspace 依赖与 `utils` crate。

### Phase 1：API Key + 鉴权执行点（5–8 天）
- [x] **30.** DDL：`sys_api_key` + `sys_menu` 权限点补录（12 个新权限点：`system:apikey:*` 4 个、`upload:file:add/list/delete` 3 个、`chat:completion:add`、`system:oauthclient:*` 4 个）
  - 已实现：`public_migration.sql` 新增 `sys_api_key`（`IF NOT EXISTS` 幂等 DDL + `uk_sys_api_key_hash` / 租户 / 前缀索引）与 44 条 `sys_menu` 权限点（API Key 4 + 页面 1 + OAuth 客户端 4 + 技能 4 + 注册 2 + 上传 3 + 对话 1 + 用户 5 + 角色 3 + 部门 3 + 租户 2 + 知识 3 + 流程 9），id 段 200–278 与既有 1–165 不冲突，`command_name` 全部为 NULL（不污染 Tauri 映射）。
- [x] **31.** 路由 → perms 映射补录：105 条路由全覆盖（当前仅 38 条带 `http_path`），中间用 `AUTHZ_MODE=warn` 过渡后再切 `enforce`
  - 已实现：`api/src/authz.rs::ROUTE_PERMS` 覆盖约 140 条路由模板（含 `/api/upload/*`、`/api/chat/stream`、`/api/skills*`、`/api/api-keys*`），DB（`sys_menu.http_method` + `http_path`）优先、代码兜底，60s 缓存。
  - `AUTHZ_MODE` 默认 `warn`（JWT 仅告警），**Service principal 恒为 enforce**，非法取值回落 `warn`；未登记路由在 enforce 下 403（fail-closed）。
- [x] **32.** `api/src/authz.rs`：`AuthPrincipal` + `(method, MatchedPath) → perms` 判定 + 60s 权限缓存 + 启动自检（未映射路由告警；**自检在 `start_http_server_with()` 内调用**，不 panic、不影响 Tauri）
  - 已实现：`AuthPrincipal`（`PrincipalKind::User/Service`）+ `matches_scope` 通配（`*` / `asset:*`）+ 每分钟限流；`audit_route_coverage()` 只输出 `tracing` 日志（`RouteCoverageReport` 统计 code-only / db-only 路由），不 panic。
  - 服务身份约束：`SERVICE_FORBIDDEN_PREFIXES`（`system:` / `upload:`）+ `SERVICE_FORBIDDEN_ROUTES`（强制 `UserContext` 的资产写接口）一律 403；`ops_routes` 三个写 handler 改为 `Option<Extension<UserContext>>`，使「采集上报」模板可被 API Key 调用（`created_by` 记 NULL）而不是 500。
  - 单测：`authz` 15 个 + `response` 3 个（403 携带 `scope` 与 `WWW-Authenticate`）。
- [x] **33.** API Key 管理接口（`api/src/api_key_routes.rs`，scopes ⊆ 创建者权限）+ 前端 Key 管理页
  - 已实现：`api_key_routes.rs` 4 个 handler（scopes ⊆ 创建者权限、禁止 `system:*` / `upload:*` / 用户角色类高危点、跨租户 404、API Key 自身不可管理 Key）+ `service/src/api_key_service.rs`（创建 / 列表 / 轮换（旧 key ≤24h 过期）/ 吊销 / 校验，30s 校验缓存、60s `last_used_at` 节流、DB 故障 fail-closed）。
  - 前端：`services/apiKeyService.ts` + `app/settings/api-keys/page.tsx`（一次性明文弹窗 + 复制、scope 模板、有效期 / 限流、轮换 / 吊销、桌面模式提示），`httpAdapter.ts` 补 4 条路由，设置首页加入口。


### Phase 2：OAuth 2.1 授权服务器（5–8 天，外部第三方接入前）
- [x] **34.** `/oauth/authorize`（方案 A：Bearer + 同意页）、`/oauth/token`（code + PKCE S256 + refresh 轮换）、`/oauth/introspect`、`/oauth/revoke`、`/.well-known/oauth-authorization-server`
  - 已实现：`service/src/oauth_service.rs`（授权码 / 令牌 / 内省 / 撤销 / 客户端 CRUD + PKCE 与回调地址校验的纯函数单测 10 个）、`api/src/oauth_routes.rs`（端点 + 客户端管理 + `client_secret_basic` 解析 + 查询参数编码单测）、`api/src/oauth_error.rs`（RFC 6749 §5.2 错误体，与 `ApiResponse` 隔离）。
  - 方案 A 落地形态：`GET /oauth/authorize` 返回**同意信息 JSON**（client_name / scopes / state / issuer），`POST /oauth/authorize` 提交 `approve` 后返回 `{redirect_to}`（由调用方 302）。不做 HTML 页面与浏览器重定向 —— Bearer 无法随浏览器跳转携带（§7.3 方案 A 的限制）。
  - PKCE：只支持 `S256`（`plain` 直接拒绝），`code_challenge` 必填；`code_verifier` 校验 43–128 位字符集，比较走常数时间；授权码只存 SHA-256、TTL 60s、一次性，PKCE 失败即作废该 code。
  - 令牌：`at_` / `rt_` 前缀（与 `ak_` / JWT 分流互不冲突），只存哈希；刷新轮换复用 `family_id` 语义（重放撤销整族）；授权码重放则撤销该 client + user 的全部未撤销令牌；`/oauth/token` 响应带 `Cache-Control: no-store`。
  - `at_` 接入 `auth.rs`：`PrincipalKind::OAuth`，主体 `u:<id>@client:<client_id>`，**不注入** `UserContext` / `Claims`，因此用户态接口与 `system:*` / `upload:*` 由 `authz` 一律 403（非用户主体始终 enforce）。
  - 缺省不挂载（§16 T-7）：需 `OAUTH_ENABLED=true`（或 `OAUTH_ALLOW_INSECURE_HTTP=true`）才注册 `/oauth/authorize`、`/oauth/token`、`/oauth/introspect`、`/oauth/revoke`、`/.well-known/oauth-authorization-server`；`/api/oauth-clients*` 管理接口始终挂载（`system:oauthclient:*` + 租户隔离 + scope ⊆ 创建者权限）。
  - DDL：`sys_oauth_client` / `sys_oauth_code` / `sys_oauth_token`（`public_migration.sql`，幂等；`family_id UUID` 读写用 `$n::uuid` / `family_id::text`，因 sqlx 未启用 `uuid` feature）；`sys_menu` 205–208 补 `http_method` / `http_path`，新增页面 id 49（`/settings/oauth-clients`）。
  - 前端：`services/oauthClientService.ts` + `app/settings/oauth-clients/page.tsx`（一次性密钥弹窗、scope 模板、启用/禁用、删除），`httpAdapter.ts` 登记 4 条路由，设置首页加入口。
  - 清理：`oauth_service::cleanup_expired_grants()` 供定时任务调用（授权码保留 24h、令牌过期后保留 30d），不在请求路径上自动执行。


### 🔒 Tauri 模式隔离回归（每阶段收尾必跑，设计文档 §16 / §13.4）

- [ ] **35.** 清空 `JWT_SECRET` 启动 → 桌面端登录/资产列表/上传全部正常，仅 HTTP 不可用
- [ ] **36.** 桌面端切换租户 → 数据随租户变化；再用 HTTP 以另一租户身份请求后回桌面查询 → 数据不受影响（HTTP 不再写 `GLOBAL_SCHEMA`）
- [ ] **37.** 不设置任何新环境变量（`AUTHZ_MODE` / `API_KEY_*` / `OAUTH_*`）→ 桌面端全功能可用
- [ ] **38.** 新增权限点未污染 `sys_menu.command_name`；Tauri `invoke` 错误提示与改造前一致

