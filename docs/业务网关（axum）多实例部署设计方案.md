# 业务网关（axum）多实例部署设计方案

| 项 | 值 |
|---|---|
| 文档版本 | v1.2（设计稿；新增附录 D：不依赖 Redis 的 PostgreSQL 协调面） |
| 日期 | 2026-11-11 |
| 适用范围 | `apps/backend/src-tauri`（axum 0.8.9 + tonic 0.14.6 + sqlx 0.9 + redis 1.8.1（可选，见附录 D） + tokio 1.54） |
| 状态 | 待评审；本轮只产出设计，不含代码改动 |
| 关联文档 | `docs/OAuth2.1与API Key统一鉴权设计方案.md`、`docs/桌面与Web双形态部署指南.md`、`docs/多租户数据权限与租户切换设计方案.md`、`docs/doc-parser 独立部署指南.md`、`TODO.md` |

> 本文描述在现有 Rust 后端中新增一个**可多实例部署的业务网关**：单进程同时承载 HTTP、SSE 与 gRPC，复用现有 OAuth 2.1 / API Key 鉴权管线与多租户 schema 隔离。
>
> 结论先行：**不新建第二套权限体系、不复制业务 handler**——网关内嵌 `api` crate 的路由；把「多实例」真正要解决的共享状态（认证缓存、限流、SSE 广播、清理任务选主、Snowflake worker）全部外置到 Postgres + Redis；gRPC 通过 tonic 0.14 的 `Routes::into_axum_router()` 与 HTTP 合并到同一端口。

---

## 0. 一页速读

1. **形态**：新建 `crates/gateway`（bin `assets-gateway`）。默认 `embedded` 模式内嵌 `api` crate 的业务路由（同进程、零额外跳、复用鉴权与租户 schema）；`proxy` 模式可转发 `doc-parser` 等外部服务。
2. **端口**：单端口 `8080`，同端口承载 HTTP/1.1、SSE 与 h2c gRPC；TLS 由 Ingress / Nginx 终止，进程内只跑 h2c。
3. **gRPC**：tonic `0.14.6`（`Cargo.lock` 已有，来自 `opentelemetry-otlp` 的 `grpc-tonic`）+ `tonic-prost-build` + `protoc-bin-vendored`（**本机没有 `protoc`，必须用 vendored 二进制**）。
4. **鉴权**：复用 `api::auth::AuthPrincipal`（`auth.rs:71`）与 `authz::required_perms`（`authz.rs:344`），把内联在 `auth_middleware`（`auth.rs:360`）里的解析逻辑抽成 `authenticate()`，HTTP 与 gRPC 共用同一份。
5. **多实例**：Postgres 是唯一事实源；Redis 承担认证 L2 缓存、限流、SSE Streams/PubSub 广播、leader 租约；进程内 `DashMap` 只做 L1（TTL ≤ 5s）。若部署环境不能引入 Redis，按**附录 D** 用 PostgreSQL 承接全部跨实例协调职责（阶段划分与人日不变）。
6. **租户隔离**：每个请求都必须走 `database::with_schema(schema, next.run(req))`；**禁止**使用 `database::GLOBAL_SCHEMA`（`auth.rs:377` 的注释已记录该历史 bug）。
7. **SSE**：标准 `event:` / `id:` / `retry:` 字段，15s 注释帧心跳，支持 `Last-Event-ID` 续传，背压队列 256，退出时 25s drain。
8. **一致性**：启动 DDL 加 `pg_advisory_lock`；过期清理任务选主；`SNOWFLAKE_WORKER_ID` 每实例唯一（0..31）。
9. **可观测**：补 `opentelemetry::global::set_text_map_propagator`（当前缺失）、OTel meter 指标（`gw_*`）、`/livez` 与 `/readyz` 分离、`grpc.health.v1.Health`。
10. **工期**：P0–P5 共约 18–23 人日，每阶段都有可执行验收命令（见 §14）。

### 0.1 五个必须在 P0 修复的问题

| # | 问题 | 证据 | 修法 |
|---|---|---|---|
| 1 | SSE 事件格式不标准 | `chat_routes.rs:113` 把 `"event: token\ndata: {...}\n\n"` 预格式化后塞进 `Event::data()`，axum 会把内嵌换行转义成多行 `data:`，标准 `EventSource` 拿不到 `event` 名 | 改为 `Event::default().event("token").json_data(..)` |
| 2 | 限流是进程内计数 | `authz.rs:308` `rate_limit_counters()` 是 `DashMap`，3 实例 = 3 倍额度 | `RateLimiter` trait + Redis 滑动窗口 |
| 3 | 清理任务每实例都跑 | `api/lib.rs:557` `spawn_cleanup_task()` 在启动时无条件 `tokio::spawn` | 抽出 `cleanup_once()`，由 `pg_try_advisory_lock` 选主后调用 |
| 4 | 启动即跑 DDL，无并发保护 | `database/postgres.rs` 启动路径直接执行 `IF NOT EXISTS` DDL | `pg_advisory_lock` 包住整个 DDL 流程 |
| 5 | SSE 连接阻塞优雅退出 | `axum::serve(...).with_graceful_shutdown()` 会等所有连接结束，SSE 永不结束 | 主动向所有 SSE 连接推 `event: shutdown` 并结束流，配合 drain 超时 |

### 0.2 实施阶段一览

| 阶段 | 主要内容 | 预计 | 出口条件 |
|---|---|---|---|
| P0 | crate 骨架、`build_router` 抽取（含 upload/chat）、config/state、`/livez` + `/readyz`、request-id、drain、advisory lock | 2–3d | 网关能起；`/livez` `/readyz` 正常；桌面端 `cargo check` 不受影响 |
| P1 | `authenticate()` 抽取、`AuthStore` 注入、Redis L2 + 版本号 + 失效广播、Snowflake worker 抢位、缓存单飞 | 3–4d | 双实例交叉验证：吊销 1s 内生效；重复 worker id 启动失败 |
| P2 | `RateLimiter` trait + Redis ZSET/Lua、多维限流、`RATE_LIMIT_FAIL_MODE` | 2d | 3 实例下总额度不放大；Redis 挂时按配置 503/放行 |
| P3 | `SseHub`、`/api/events`、`/api/auth/sse-ticket`、事件格式修正、Streams+PubSub、`Last-Event-ID` 续传、背压 | 4–6d | 杀实例后续传不丢事件；两实例连接收到同一广播；票据只能用一次 |
| P4 | proto/codegen、`into_axum_router` 合并、`grpc_auth` + `Status` 映射、health/reflection/gRPC-Web、流式取消 | 4–5d | `grpcurl list` 可用；无 token → `Unauthenticated`；取消后上游流停止 |
| P5 | leader 清理、OTel propagator + `gw_*` 指标、K8s/Nginx/Helm、压测与混沌 | 3d | 滚动更新 0 个 5xx；`SIGTERM` 后 30s 内退出；trace 跨服务连通 |

> 若采用**附录 D** 的「PostgreSQL 单一依赖模式」，把上表中的 Redis 换成 D.0 映射表的对应项即可，阶段划分与人日不变。

---

## 1. 背景、目标与边界

### 1.1 背景

当前后端已经有两套入口共用同一批业务实现：

- 桌面入口：`apps/backend/src-tauri/src/main.rs`（Tauri `invoke` + 内嵌 HTTP）；
- 无头入口：`crates/server/src/main.rs`（仅 axum，产物 `assets-server`）。

两者都通过 `crates/bootstrap` 完成数据库、租户 schema、LLM Router 与 HTTP 服务初始化。`api` crate 目前把「路由组装 + 监听 + 优雅退出」写死在一个函数里（`api/lib.rs:91` `start_http_server_with`），且所有鉴权、限流、清理都假设「单进程」。

要对外提供**多实例、可滚动更新、同时支持浏览器 / App / 服务间调用**的接入层，需要补三件事：

1. 一个可复用的 Router 组装入口（不再由 `api` 自己 bind）；
2. 一个真正的 gRPC 入口（当前仓库没有任何 `.proto`）；
3. 一套把进程内状态外置的方案（认证缓存、限流、SSE 广播、后台任务）。

### 1.2 目标

| 编号 | 目标 | 可验证标准 |
|---|---|---|
| G1 | 单二进制同时提供 HTTP、SSE、gRPC | 同一 `host:port` 上 `curl`、`curl -N`、`grpcurl` 均可用 |
| G2 | 完全复用现有鉴权与多租户隔离 | 401/403 行为与 `assets-server` 一致；跨租户不可见 |
| G3 | 水平扩容无状态化 | 3 实例下限流总额度、SSE 广播、令牌吊销语义与单实例一致 |
| G4 | 滚动更新不中断长连接 | 更新期间 SSE 客户端自动重连、无 5xx；`SIGTERM` 后 30s 内退出 |
| G5 | 可观测 | 每个请求有 `request_id` 与 trace；有连接数/拒绝数/依赖健康指标 |
| G6 | 对桌面端零影响 | `start_http_server_with` 签名与行为不变，Tauri 命令层不改 |

### 1.3 非目标

- N1：不做 GraphQL、不做 WebSocket（SSE 已覆盖服务端推送场景）。
- N2：不在网关内实现 OAuth 授权服务器的新端点；继续复用 `oauth_routes`。
- N3：不替换现有 `assets-server`；两者可并存（见 §3.4）。
- N4：不做跨地域多活；本文的「多实例」指同一集群内多副本 + 共享 PG/Redis。
- N5：不在本轮实现计费、配额结算、灰度路由等 API 管理能力。

### 1.4 术语

| 术语 | 含义 |
|---|---|
| 主体 Principal | 统一身份载体 `AuthPrincipal`：用户 / API Key / OAuth 令牌 |
| 凭证 Credential | 请求携带的原始凭据：JWT、`at_*`、`ak_*`、会话 Cookie、SSE 票据 |
| 租户 schema | 每租户一个 PostgreSQL schema，由 `database::with_schema` 通过 task_local 设置 `search_path` |
| L1 / L2 缓存 | L1 = 进程内 `DashMap`；L2 = Redis |
| drain | 收到退出信号后，先摘流、再主动结束长连接的过程 |
| 伪路径 | 把 gRPC 方法映射成 `grpc:/<package>.<Service>/<Method>`，用于复用 `sys_menu` 权限表 |

---

## 2. 现状盘点

### 2.1 已有能力与代码位置

| 能力 | 位置 | 说明 |
|---|---|---|
| 路由组装 | `api/lib.rs:236` `create_router`（私有） | 公开路由 + 受保护路由 + skill 路由，末尾 `.with_state(state)` |
| 服务启动 | `api/lib.rs:91` `start_http_server_with` | 校验 `JWT_SECRET`、跑路由覆盖自检、`spawn_cleanup_task`、bind、`axum::serve` |
| 认证 | `api/auth.rs:360` `auth_middleware` | JWT / API Key / OAuth 令牌三路分流，注入 `UserContext`、`Claims`、`AuthPrincipal` |
| 凭证分流 | `api/auth.rs:164` `parse_credential` | 按前缀 `at_` / `ak_` / 其它（JWT）分流，无需解码 |
| JWT 校验 | `api/auth.rs:195` `verify_token` | `jsonwebtoken` + `JWT_SECRET`（HS256） |
| 授权快照 | `api/auth.rs:138` `user_authority` | 进程内 `DashMap`，60s TTL（`auth.rs:119`）→ `service::api_key_service::load_user_authority` |
| 功能鉴权 | `api/authz.rs:344` `required_perms` | 代码静态表 + DB `sys_menu.http_method/http_path`，带缓存 |
| Scope 匹配 | `api/authz.rs:416` `matches_scope` | 支持 `system:*` 通配 |
| 限流 | `api/authz.rs:443` `check_rate_limit` | 进程内 `DashMap`，60s 固定窗口 |
| 路由覆盖自检 | `api/authz.rs:656` `audit_route_coverage` | 启动时打印未登记权限点的路由 |
| SSE | `api/chat_routes.rs` | `/api/chat/stream`，`mpsc<String>` + `futures::stream::unfold` |
| 健康检查 | `api/health.rs:49` `healthz` | 带 DB `SELECT 1`（2s 超时） |
| 安全响应头 | `api/security.rs` | HSTS / nosniff / X-Frame-Options / Referrer-Policy |
| 会话 | `service/session_service.rs` | `ss_<token>` Cookie + `cs_<token>` CSRF，DB 存储，`last_seen` 节流 |
| 令牌校验 | `service/oauth_service.rs` `verify_access_token` / `introspect` | 哈希查表 + 客户端启用状态 |
| 清理 | `service/oauth_service.rs` `cleanup_expired_grants`、`service/session_service.rs` `cleanup_expired_sessions` | 由 `api/lib.rs:557` 每小时调用 |
| 租户 schema | `database/lib.rs` `with_schema` / `schema_prefix` | task_local，仅请求内有效 |
| 连接池 | `database/postgres.rs` | 全局池；启动时执行 `IF NOT EXISTS` DDL |

### 2.2 五个必须先修的问题（展开）

**问题 1：SSE 帧被当作 data 字段**

`chat_routes.rs:113`：

```rust
let data = format!("event: token\ndata: {}\n\n", serde_json::json!({"text": text}));
bridge_sse_tx.send(data).await;
```

下游是 `Event::default().data(data)`。axum 的 `Event::field` 会把值里的 `\n` 转义成多行 `data:`，因此客户端实际收到的是：

```
data: event: token
data: data: {"text":"..."}
data:
data:
```

标准 `EventSource` 只会触发 `message` 事件，且 `event.data` 里带着 `event: token` 前缀。用 `fetch` 手写解析的前端可能"碰巧能用"，但这依赖实现细节，必须修正。

**问题 2：限流是进程内计数**

`authz.rs:309` 的 `rate_limit_counters()` 是 `OnceLock<DashMap<i64, (u64, u32)>>`。多实例下每个实例各算一份，`rate_limit_per_min = 60` 的 Key 在 3 实例下实际可打 180 次/分钟。

**问题 3：清理任务每实例都跑**

`api/lib.rs:557` `spawn_cleanup_task()` 在 `start_http_server_with` 内无条件 spawn，首次延迟 60s、之后每小时一次。N 个实例就是 N 份并发 `DELETE`，虽然幂等，但会产生锁竞争与无意义的 WAL。

**问题 4：启动 DDL 无并发保护**

`database/postgres.rs` 在启动路径执行建表/建索引。多实例同时启动时，`CREATE TABLE IF NOT EXISTS` 仍可能因并发 catalog 变更产生 `duplicate key value violates unique constraint "pg_type_typname_nsp_index"` 一类报错。必须用 `pg_advisory_lock` 串行化。

**问题 5：SSE 连接阻塞优雅退出**

`axum::serve` 的 graceful shutdown 语义是「停止 accept，等待所有已建立连接自然结束」。SSE 连接按设计永不结束，因此滚动更新会一直卡到 `terminationGracePeriodSeconds` 被 SIGKILL。

### 2.3 多实例差距矩阵

| 能力 | 当前实现 | 多实例下的问题 | 本方案 |
|---|---|---|---|
| 会话 / 令牌 / 授权码 | Postgres | 无（已是共享存储） | 保持 |
| 认证缓存 | 进程内 DashMap | 各实例 TTL 独立，吊销最长延迟 60s | L1 + Redis L2 + 版本号失效 |
| 限流 | 进程内 DashMap | 额度按实例数放大 | Redis 滑动窗口（Lua） |
| SSE 广播 | 无（只有请求级流） | 无法跨实例推送 | Redis Stream + Pub/Sub |
| SSE 续传 | 无 | 重连丢事件 | `Last-Event-ID` + `XRANGE` |
| 后台清理 | 每实例 | 重复执行 | PG advisory lock 选主 |
| 迁移 | 每实例执行 DDL | 并发冲突 | advisory lock 串行 |
| Snowflake | `SNOWFLAKE_WORKER_ID` 默认 0 | ID 冲突 | 每实例唯一 worker id |
| 配置 | 环境变量 | 无 | 环境变量 + 启动校验 |
| 追踪 | 只有 tracer provider | 跨服务链路断裂 | 全局 propagator + 上游注入 |

---

## 3. 总体架构

### 3.1 拓扑

```text
                       ┌──────────────────────────────────┐
  Browser / App ──TLS──▶│ Ingress / L7 LB                  │
  gRPC client ───h2c───▶│  · TLS 终止（HTTP/2）             │
  gRPC-Web ────────────▶│  · /api/events 关闭 buffering     │
                       │  · /api/events 读超时 ≥ 1h        │
                       └───────────────┬──────────────────┘
                                       │ h2c / HTTP 1.1
        ┌──────────────────────────────┼──────────────────────────────┐
        ▼                              ▼                              ▼
 ┌─────────────┐                ┌─────────────┐                ┌─────────────┐
 │ gateway-0   │                │ gateway-1   │                │ gateway-2   │
 │             │                │             │                │             │
 │ Router      │                │ Router      │                │ Router      │
 │ ├ /api/*    │◀─ api crate ──▶│ ├ ...       │                │ ├ ...       │
 │ ├ /api/events（SSE）          │ │           │                │ │           │
 │ └ /assets.gateway.v1.*（gRPC）│ │           │                │ │           │
 └──────┬──────┘                └──────┬──────┘                └──────┬──────┘
        └────────────────┬─────────────┴─────────────┬────────────────┘
                         ▼                           ▼
                  ┌─────────────┐             ┌─────────────┐
                  │ PostgreSQL  │             │   Redis     │
                  │ 业务表      │             │ 认证 L2     │
                  │ 租户 schema │             │ 限流 / SSE  │
                  │ 会话 / 令牌 │             │ 广播 / 选主 │
                  └─────────────┘             └─────────────┘
```

**关键点**

1. 网关进程内同时持有 axum `Router` 与 tonic `Routes` 合并后的路由，**只监听一个端口**。
2. HTTP 与 gRPC 共享同一个 `AppState`（连接池、Redis、认证器、SSE Hub、退出信号）。
3. 所有跨实例状态只落 Postgres 与 Redis，进程内不保存任何不可重建的数据。
4. Ingress 必须对 `/api/events` 关闭响应缓冲，否则 SSE 会被攒批（§7.7）。

### 3.2 三类请求路径

| 类型 | 入口 | 认证 | 状态 |
|---|---|---|---|
| 普通 HTTP | `/api/**`、`/oauth/**`、`/healthz` | `auth_middleware`（JWT / `at_` / `ak_` / Cookie） | 无状态，任意实例 |
| SSE | `POST /api/chat/stream`、`GET /api/events` | 同上 + 一次性票据 | 连接级状态（本实例） |
| gRPC | `/assets.gateway.v1.*` | `grpc_auth`（同一 `authenticate()`） | unary 无状态；stream 连接级 |

### 3.3 单端口 vs 双端口

| 维度 | 单端口（推荐） | 双端口 |
|---|---|---|
| K8s Service / 探针 | 一套 | 两套 |
| LB 配置 | 同一 upstream 按 path 分流 | 两个 upstream |
| HTTP/2 keepalive 调参 | 不可（`axum::serve` 不暴露） | 可（tonic `Server::builder()`） |
| 每连接并发流限制 | 不可 | 可（`max_concurrent_streams`） |
| gRPC-Web 与 HTTP CORS 隔离 | 需小心 | 干净 |

实现上提供开关：`GATEWAY_GRPC_MODE=merged`（默认）/ `separate`（`GATEWAY_GRPC_PORT` 生效）。**先按 merged 实现**，因为：

- `axum::serve` 底层用 `hyper-util` 的 auto builder，能识别 HTTP/2 prior-knowledge，gRPC 客户端默认就是这种连接方式；
- tonic 0.14 的 `Routes::into_axum_router()` 直接产出 `axum::Router`，合并成本极低；
- 单端口下 axum 中间件可以**直接包裹 gRPC 路由**，从而复用 `auth_middleware` 的同一套逻辑（§6.8）。

### 3.4 与 `assets-server` 的三种关系

| 模式 | 说明 | 优点 | 代价 | 适用 |
|---|---|---|---|---|
| `embedded`（推荐） | 网关内嵌 `api::build_router`，与 `assets-server` 是同一份代码的两个 bin | 零额外跳、无双重鉴权、`with_schema` 直接生效 | 网关与业务同版本发布 | 当前仓库形态 |
| `proxy` | 网关把 `/api/**` 反向代理到独立部署的 `assets-server` | 上游可独立扩缩容 / 换语言 | 多一跳、双份鉴权、SSE 透传需关缓冲、trace 需透传 | 业务服务需要独立发布 |
| `hybrid` | 核心业务内嵌，`/doc-parser/**`、LLM 等转发 | 兼顾两者 | 配置复杂度上升 | 已有 Python sidecar（见 `docs/doc-parser 独立部署指南.md`） |

本设计**默认 `embedded`**，`proxy`/`hybrid` 通过 `GATEWAY_UPSTREAMS` 配置表启用，路由匹配按「最长前缀优先、显式内嵌路由优先于代理前缀」的规则。

---

## 4. 决策记录（ADR）

| 编号 | 决策 | 备选 | 理由 | 后果 |
|---|---|---|---|---|
| D0 | 本轮只产出设计文档，不改代码 | 直接实现 | 先对齐边界与工作量 | 实施从 P0 开始 |
| D1 | 新建 `crates/gateway`，不改 `crates/server` | 直接扩 `server` | `server` 保持「最小无头入口」定位；网关依赖更多（tonic/redis） | 多一个 bin，需加 Dockerfile |
| D2 | 单端口合并 gRPC | 双端口 | 部署与探针简单；axum 中间件可直接包 gRPC | 失去 tonic `Server` 级调参，用开关兜底 |
| D3 | 内嵌 `api` 路由为默认 | 纯反代 | 避免双重鉴权与额外跳；复用 `with_schema` | 需把 `create_router` 提升为 `pub` |
| D4 | TLS 由 Ingress 终止 | 进程内 rustls | `axum::serve` 不支持 TLS；证书轮换交给平台 | 内网需信任 h2c |
| D5 | JWT 同时支持 HS256 与 JWKS | 只 HS256 | 兼容现状；未来多服务可切 RS256 | 需维护两套校验分支 |
| D6 | SSE 历史用 Redis Stream，通知用 Pub/Sub | 纯 Pub/Sub | 只有 Stream 支持 `Last-Event-ID` 续传 | 双写，需控制 MAXLEN |
| D7 | 认证缓存 L1 + L2 + 版本号 | 只 L2 | 单实例零网络开销，多实例秒级一致 | 多一个版本号 key |
| D8 | 限流默认 fail-closed | fail-open | 与 `auth.rs:420` "不得降级为放行"一致 | Redis 故障时写请求 503 |
| D9 | proto 构建期生成 + vendored protoc | 提交生成码 | 本机无 protoc；避免生成码与 proto 漂移 | 构建依赖 `protoc-bin-vendored` |
| D10 | 清理任务选主 | 每实例都跑 | 避免重复 DELETE 与锁竞争 | leader 需保活连接 |

### 4.1 被否决的方案

- **纯反向代理网关（D3 备选）**：需要在网关再做一次鉴权（或信任上游），且 SSE 透传要处理 `Content-Encoding`、`buffering`、`Connection` 等一堆边界；收益不足以抵消复杂度。
- **在网关内重建一套权限模型**：会与 `sys_menu` 双写，违背 `docs/OAuth2.1与API Key统一鉴权设计方案.md` 的「一条管线三类主体」结论。
- **用 WebSocket 替代 SSE**：客户端已有 SSE 实现；WebSocket 需要额外的心跳/重连/多路复用协议设计。

---

## 5. 模块与代码结构

### 5.1 新 crate 目录

仓库的 workspace 成员是 `crates/*`（`Cargo.toml:21`），因此新增 `crates/gateway` 会自动成为成员。

```text
apps/backend/src-tauri/crates/gateway/
├─ Cargo.toml
├─ build.rs                      # protoc-bin-vendored + tonic-prost-build
├─ proto/gateway.proto
└─ src/
   ├─ main.rs                    # 启动顺序 / 信号 / 退出
   ├─ config.rs                  # GatewayConfig::from_env() + 启动校验
   ├─ state.rs                   # AppState（pool / redis / auth / authz / sse / shutdown）
   ├─ router.rs                  # api::build_router + SSE + gRPC + proxy 组装
   ├─ auth/
   │  ├─ mod.rs                  # Authenticator：凭证分流 → AuthPrincipal → schema
   │  ├─ cache.rs                # AuthStore trait：Memory / Redis 两实现
   │  ├─ jwt.rs                  # HS256 + JWKS（RS/ES），iss/aud/leeway
   │  ├─ opaque.rs               # at_ / ak_ / 远端 introspect
   │  ├─ session.rs              # ss_ Cookie + CSRF
   │  └─ ticket.rs               # SSE 一次性票据（Redis GETDEL）
   ├─ authz.rs                   # 路由→权限点、gRPC 伪路径、Redis 限流
   ├─ middleware/
   │  ├─ request_id.rs           # X-Request-Id 生成/透传
   │  ├─ grpc_auth.rs            # gRPC 专用：错误转 Status
   │  ├─ drain.rs                # 在途请求计数 + 超时
   │  └─ security.rs             # 复用 api::security + CSP
   ├─ http/
   │  ├─ proxy.rs                # reqwest 流式转发（SSE 不缓冲）
   │  └─ sse.rs                  # SseHub / 订阅 / 续传 / 心跳 / 背压
   ├─ grpc/
   │  ├─ mod.rs                  # Routes::into_axum_router()
   │  └─ service.rs              # EventGateway 实现
   ├─ background/
   │  ├─ leader.rs               # pg_try_advisory_lock 选主
   │  ├─ cleanup.rs              # 选主后才跑过期清理
   │  └─ sse_bus.rs              # Redis 订阅 → 本地 fan-out
   ├─ health.rs                  # /livez /readyz + tonic-health
   └─ metrics.rs                 # OTel meter 指标
```

### 5.2 对现有 crate 的改动

原则：**只做「提取」不做「重写」**，`start_http_server_with` 的签名与行为保持不变，桌面端与 `assets-server` 都不受影响。

| 文件 | 改动 | 兼容性 |
|---|---|---|
| `api/src/lib.rs` | `create_router` → `pub fn build_router(pool: PgPool, llm_router: Option<Arc<LLMRouter>>) -> anyhow::Result<Router>`；把 upload/chat 子路由抽成 `fn upload_router(pool)` / `fn chat_router(llm_router)` | `start_http_server_with` 内部改调 `build_router` + 自己 bind，对外无变化 |
| `api/src/lib.rs` | `spawn_cleanup_task` 拆分：`pub async fn cleanup_once()` + 原函数改为循环调用它 | 桌面端行为不变 |
| `api/src/auth.rs` | 抽 `pub async fn authenticate(req: &mut Request) -> Result<AuthPrincipal, ApiError>`；`auth_middleware` 改为 `authenticate` + `with_schema` + `next.run` | 行为完全等价 |
| `api/src/auth.rs` | `AuthPrincipal` / `PrincipalKind` 增加 `serde::Serialize, serde::Deserialize` 派生 | 纯增量 |
| `api/src/auth.rs` | `authority_cache` 通过 `pub fn install_auth_store(store: Arc<dyn AuthStore>)` 注入，默认 Memory 实现 | 未注入时等同现状 |
| `api/src/authz.rs` | `check_rate_limit` 通过 `pub fn install_rate_limiter(limiter: Arc<dyn RateLimiter>)` 注入，默认 Memory | 未注入时等同现状 |
| `api/src/chat_routes.rs` | SSE 事件改为 `Event::default().event(..).json_data(..)`；加 `Sse::keep_alive` | 对前端更标准，需前端确认解析方式 |
| 根 `Cargo.toml` | 新增 workspace 依赖（§5.3） | 只增不改 |

> **三点必须注意的实现事实**（已核对源码）：
>
> 1. `create_router` 是私有 `fn`，签名只有 `pool: sqlx::PgPool`（`lib.rs:236`）；`llm_router` **不经过它**，upload / chat 两个子路由是在 `start_http_server_with` 里就地构建后 `merge` 的（`lib.rs:203`）。因此抽 `build_router` 时必须**把这段就地构建一起搬进新函数**，否则网关将缺少 `/api/upload/*` 与 `/api/chat/stream`。
> 2. `cors` / `TraceLayer` / `security_headers` 三层是挂在 `start_http_server_with` 的**最外层**（`lib.rs:207-211`）。`build_router` 应返回**未套这三层**的 Router，让网关自己按 §5.4 组装，避免重复 span 与重复 CORS 头。
> 3. `/healthz` **已存在**于 `public_routes`（`lib.rs:241`）。`Router::merge` 遇到重复路径会 panic，因此网关只能新增 `/readyz`（与可选 `/livez`），**不得**再注册 `/healthz`。

### 5.3 依赖与版本约束

```toml
# 根 Cargo.toml 的 [workspace.dependencies] 追加（版本对齐现有 Cargo.lock）
# gRPC：锁 0.14 系列，与 opentelemetry-otlp 0.33 的 grpc-tonic 共用，避免重复编译
tonic = { version = "0.14.6", features = ["router", "server", "codegen", "transport"] }
tonic-prost = "0.14.6"
tonic-health = "0.14.6"
tonic-reflection = "0.14.6"
tonic-web = "0.14.6"
prost = "0.14.4"
tower = { version = "0.5.3", features = ["util", "timeout", "limit"] }
# axum 追加 http2（当前只有 macros）
axum = { version = "0.8.9", features = ["macros", "http2"] }
# tower-http 追加 request-id / limit / timeout / catch-panic / sensitive-headers
tower-http = { version = "0.7.1", features = [
  "cors", "trace", "request-id", "limit", "timeout", "catch-panic", "sensitive-headers",
] }
bytes = "1"
http = "1"
http-body-util = "0.1"
```

```toml
# crates/gateway/Cargo.toml
[build-dependencies]
tonic-prost-build = "0.14.6"
protoc-bin-vendored = "3.3.0"

[dependencies]
anyhow.workspace = true
api.workspace = true
axum.workspace = true
bootstrap.workspace = true
bytes.workspace = true
chrono.workspace = true
database.workspace = true
dashmap.workspace = true
futures.workspace = true
http.workspace = true
http-body-util.workspace = true
prost.workspace = true
redis.workspace = true
serde.workspace = true
serde_json.workspace = true
service.workspace = true
sqlx.workspace = true
tokio.workspace = true
tokio-util.workspace = true
tonic.workspace = true
tonic-health.workspace = true
tonic-prost.workspace = true
tonic-reflection.workspace = true
tonic-web.workspace = true
tower.workspace = true
tower-http.workspace = true
tracing.workspace = true
utils.workspace = true
uuid.workspace = true
```

**版本约束说明**

- 必须锁 `0.14.x`：`Cargo.lock` 中 `tonic 0.14.6` / `tonic-prost 0.14.6` 由 `opentelemetry-otlp 0.33.1` 的 `grpc-tonic` 引入；若网关用 0.15，会同时编译两份 tonic 与 hyper。
- `tower-http` 0.7 与 axum 内部使用的 0.6 会并存，但两者都基于 `http 1.x`，`Layer` 泛型兼容，只是多编译一份。
- `jsonwebtoken 11.1` 已在 workspace 中（`features = ["aws_lc_rs"]`），JWKS 所需的 `DecodingKey::from_jwk` 可直接使用，无需新增依赖。

### 5.4 启动顺序

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. 日志 + 全局 propagator（必须早于任何 span）
    utils::logging::init_tracing()?;
    opentelemetry::global::set_text_map_propagator(
        opentelemetry_sdk::propagation::TraceContextPropagator::new(),
    );

    // 2. 读取 .env.toml + 环境变量，启动前校验（端口、Redis、JWT、worker id）
    let cfg = gateway::config::GatewayConfig::from_env()?;

    // 3. 数据库迁移（advisory lock 串行化），随后建立连接池
    bootstrap::init_database_and_schema().await?;

    // 4. 共享状态（Redis 连接、认证器、SSE Hub、限流器）
    let state = std::sync::Arc::new(gateway::state::AppState::new(&cfg).await?);

    // 5. 后台任务：Redis 订阅、leader 选主、指标上报
    gateway::background::spawn_all(state.clone());

    // 6. 组装路由并监听
    let listener = tokio::net::TcpListener::bind(cfg.listen_addr()?).await?;
    let app = gateway::router::build(state.clone())?;
    tracing::info!(addr = %cfg.listen_addr()?, "assets-gateway 启动");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(gateway::shutdown::signal(state.shutdown.clone()))
    .await?;
    Ok(())
}
```

**顺序上的硬约束**

1. `set_text_map_propagator` 必须早于任何 `tracing::info!`，否则首批 span 不带 trace 上下文。
2. `init_database_and_schema` 内部必须先拿 advisory lock（§9.2），否则多实例同时启动会撞 DDL。
3. `AppState::new` 要显式 `PING` Redis 并失败即退出（除非配置声明允许降级），避免"启动成功但所有请求 503"。
4. `spawn_all` 必须在开始 accept 之前注册订阅，否则启动瞬间的广播会丢（可接受，但需记录）。

**路由组装骨架**

```rust
pub fn build(state: Arc<AppState>) -> anyhow::Result<Router> {
    // 1) 业务路由：复用 api crate（内部已挂 auth/authz 中间件）
    let business = api::build_router(state.pool.clone(), Some(state.llm_router.clone()))?;

    // 2) 网关自有路由：SSE + 票据（独立 state，独立中间件）
    let events = Router::new()
        .route("/api/events", get(http::sse::subscribe))
        .route("/api/auth/sse-ticket", post(http::sse::issue_ticket))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state.clone(), auth::sse_auth))
        .layer(TraceLayer::new_for_http());

    // 3) gRPC：tonic Routes → axum Router，再包一层 gRPC 专用鉴权
    let grpc = grpc::router(state.clone())?;

    // 4) 可选：反向代理前缀（GATEWAY_UPSTREAMS）
    let proxy = http::proxy::router(state.clone());

    // 5) 网关自有探针：只加 /livez 与 /readyz（/healthz 已由 api crate 注册，重复会 panic）
    let health = health::router(state.clone());

    Ok(Router::new()
        .merge(business)
        .merge(events)
        .merge(grpc)
        .merge(proxy)
        .merge(health)
        .layer(middleware::from_fn(request_id::assign))
        // SSE 会被 Compression 攒批，predicate 里必须排除
        .layer(CompressionLayer::new().compress_when(compressible()))
        .layer(middleware::from_fn(security::headers))
        .layer(TraceLayer::new_for_http()))
}
```

> 注意 `merge` 的顺序不影响匹配优先级，但**重复路径会 panic**。`/api/events` 与 `/api/auth/sse-ticket` 目前不存在于 `create_router`，可以直接加；若未来冲突，改用 `Router::route` 统一注册。

---

## 6. 认证与鉴权

### 6.1 凭证分流

`auth::authenticate()` 的判定顺序（与现有 `auth.rs:360` 保持一致，只增加票据分支）：

```text
0. ?access_token= / ?api_key=        → 400 invalid_request（auth.rs:285 已实现）
1. Authorization: Bearer <token>
     at_*  → OAuth 2.1 访问令牌（不透明，DB 哈希查表）
     ak_*  → API Key（服务主体）
     其它  → JWT（HS256 默认 / RS256·ES256 走 JWKS）
2. Cookie: ss_<session> + cs_<csrf>（仅浏览器路由；unsafe 方法校验 CSRF）
3. ?ticket=<one-time>（仅 /api/events）
4. 全部缺失 → 401 + WWW-Authenticate: Bearer realm="assets"
```

| 凭证 | 解析 | 主体 | 是否可访问需 `claims.sub` 的接口 |
|---|---|---|---|
| JWT | `verify_token`（`auth.rs:195`） | `PrincipalKind::User` + `UserContext` + `Claims` | ✅ |
| OAuth `at_` | `service::oauth_service::verify_access_token` | `PrincipalKind::OAuth` | 视 `user_id` 是否存在 |
| API Key `ak_` | `service::api_key_service::verify_api_key`（`api_key_service.rs:466`） | `PrincipalKind::Service` | ❌（由 `authz` 拒绝） |
| 会话 Cookie | `service::session_service::find_session`（`session_service.rs:117`） | User | ✅ |
| SSE 票据 | Redis `GETDEL` | 票据内存的 `AuthPrincipal` | 继承签发时的主体 |

**实现要求**

- 分流必须**不解码**即可判断（前缀判断），避免对 JWT 做无谓的 base64/JSON 解析。
- 任何"回源失败"都不得降级为放行：`auth.rs:420` 的注释已明确返回 500，网关保持。
- `AuthPrincipal` 必须可序列化（Redis L2、SSE 票据），因此需要补 serde 派生。

### 6.2 JWT 校验

| 配置 | 行为 |
|---|---|
| `JWT_ALG=HS256`（默认） | 使用 `JWT_SECRET`，与现有 `verify_token` 完全一致 |
| `JWT_ALG=RS256` / `ES256` | 从 `JWT_JWKS_URL` 拉 JWKS，按 `kid` 选 key；JWKS 缓存 5 分钟，`kid` 未命中时强制刷新一次 |
| `JWT_ISSUER` / `JWT_AUDIENCE` | 非空则强校验 `iss` / `aud` |
| 时钟偏移 | 允许 30s leeway |
| 必需声明 | `sub`（i64）、`exp`、`iat`；`typ` 若存在必须是 `access` |

```rust
// 伪代码：两条分支共用同一校验结果
pub fn verify(token: &str) -> Result<Claims, ApiError> {
    match alg {
        Alg::Hs256 => verify_hs256(token, secret),
        Alg::Rs256 | Alg::Es256 => {
            let jwk = jwks.get(&kid_of(token)?)?;      // 缓存 + 单飞刷新
            decode(token, &jwk, validation())          // iss/aud/exp/leeway
        }
    }
}
```

> `jsonwebtoken::Validation` 默认校验 `exp`，但 `required_spec_claims` 需要显式加 `sub`；`aud` 只有在 `set_audience` 后才会校验。

### 6.3 不透明令牌与远端内省

| 模式 | 校验方式 | 说明 |
|---|---|---|
| `embedded` | `service::oauth_service::verify_access_token(raw)` | 直接查本库，避免网络跳 |
| `proxy` | `POST {OAUTH_INTROSPECT_URL}` + `client_secret_basic` | RFC 7662；先查 Redis 缓存 |

远端内省注意点：

- 必须校验响应的 `active == true`，并检查 `exp`/`nbf`；
- 响应中的 `scope` 是空格分隔字符串，需 split；
- 内省请求本身要限流，避免令牌喷洒打穿 AS；
- 内省结果缓存 TTL = `min(exp - now, 30s)`，且撤销事件要主动 `DEL`。

### 6.4 API Key

- 校验：`api_key_service::verify_api_key` 返回 `ApiKeyAuth`（`api_key_service.rs:61`），包含 `id / tenant_id / scopes / rate_limit_per_min / key_prefix`；
- 主体：`principal.kind = Service`、`subject = k:<id>`、`user_id = None`；
- 权限点直接使用 `sys_menu.perms` 词表，不另造词汇；
- 限流：优先用 Key 自带的 `rate_limit_per_min`，未配置时回落全局 `GATEWAY_RATE_LIMIT_RPS`；
- 吊销：`revoke_api_key`（`api_key_service.rs:437`）成功后必须 `DEL` L2 缓存并广播失效。

### 6.5 会话与 CSRF

浏览器场景有两个并存机制：

- OAuth 方案 B（浏览器授权）：`ss_<token>` Cookie + `cs_<token>` CSRF，`session_service` 负责签发/轮换/撤销；
- 普通登录：JWT 放在 `Authorization` 头（前端内存持有），不走 Cookie。

网关规则：

| 场景 | 规则 |
|---|---|
| 仅 Cookie 认证 + `GET/HEAD/OPTIONS` | 放行（`SameSite=Lax` 已挡跨站 POST） |
| 仅 Cookie 认证 + 其它方法 | 必须 `cs_` 与 Cookie 双提交匹配，否则 403 |
| Cookie + Bearer 同时存在 | 以 Bearer 为准（避免 CSRF 面扩大） |
| `/api/events` 使用 Cookie | 允许，但要求 `SameSite=Lax` 且只读 |
| HTTPS 环境 | Cookie 名建议加 `__Host-` 前缀（`__Host-ss_`），需与前端同步 |

CSRF 校验失败返回 `403 {"error":"invalid_csrf_token"}`，并记录 `principal.subject`（不打令牌）。

### 6.6 缓存分层与失效

```text
请求 → L1 DashMap（key 含版本号，TTL ≤ 5s）
        └─ miss → L2 Redis（TTL 30~60s）
                  └─ miss → Postgres / JWKS
                        └─ 回源异常：认证 fail-closed
```

| Key | 值 | TTL | 失效方式 |
|---|---|---|---|
| `gw:v1:auth:jwt:<sha256>` | `AuthPrincipal` JSON | 30s | 版本号 + 广播 |
| `gw:v1:auth:at:<sha256>` | 同上 | `min(exp-now, 60s)` | 撤销时 `DEL` + 广播 |
| `gw:v1:auth:ak:<sha256>` | 同上 | 30s | 吊销时 `DEL` + 广播 |
| `gw:v1:auth:neg:<sha256>` | `"1"` | 15s | 不主动失效（短 TTL 兜底） |
| `gw:v1:user:<id>:ver` | 递增整数 | 无 | `INCR` |
| `gw:v1:rl:<scope>:<subject>` | ZSET 滑动窗口 | 窗口长度 | 过期 |
| `gw:v1:sse:ticket:<t>` | `AuthPrincipal` JSON | 30s | `GETDEL` 一次性消费 |
| `gw:v1:sse:stream:<tenant>:<channel>` | Stream | MAXLEN ~1000 | 修剪 |
| `gw:v1:leader:cleanup` | `<instance_id>` | 60s（续约） | 过期抢占 |

**失效流程**

```text
权限/角色变更、API Key 吊销、令牌撤销
  → INCR gw:v1:user:<id>:ver
  → PUBLISH gw:v1:invalidate "<kind>:<id>"
  → 各实例订阅到消息后：清 L1 中对应条目（L2 靠 TTL 或显式 DEL）
```

> 只清 L1 不清 L2 是有意为之：L2 的 TTL 已经足够短，而"删 L2"与"并发回填"存在竞态；用版本号让旧值自然失效更安全。若需要强一致（如管理员封禁），则在业务侧显式 `DEL` L2。

**单飞（single-flight）**：同一 token 并发未命中时，用 `SET gw:v1:lock:<sha256> <id> NX PX 500` 抢锁，抢不到则短暂 sleep 后重查 L2，避免缓存击穿打爆 DB。

**已存在的第二处进程内缓存（多实例必须处理）**

`service::api_key_service` 自己还有一层缓存，**不经过** `api::auth` 的 L1：

| 符号 | 位置 | 行为 | 多实例问题 |
|---|---|---|---|
| `VERIFY_CACHE_TTL` | `api_key_service.rs:30` | 固定 30s | 吊销后其它实例最长 30s 仍放行 |
| `verify_cache()` | `api_key_service.rs:48` | `DashMap<key_hash, (Instant, ApiKeyAuth)>` | 每实例独立 |
| `verify_api_key` | `api_key_service.rs:466` | 命中即返回，未命中回源 DB | 缓存 key 不含吊销状态 |
| `invalidate_api_key` | `api_key_service.rs:534` | **只清本进程** | 广播不到其它实例 |
| `clear_verify_cache` | `api_key_service.rs:540` | 只清本进程 | 同上 |

API Key 吊销通常是安全事件，30s 窗口不可接受。三种处理方式（任选其一，推荐 ①）：

1. **扩大 `invalidate_api_key` 的作用域**：改成「清本地 + `PUBLISH gw:v1:invalidate "ak:<id>"`」，各实例订阅后调用 `invalidate_api_key(id)`。改动最小，且 `revoke_api_key` 的成功路径已经调用它（`api_key_service.rs:455`），不需要改调用点。
2. 网关侧在 `verify_api_key` 之前先查 L2/版本号（`gw:v1:key:<id>:ver`），命中版本不一致则强制回源；
3. 直接关闭该缓存（`VERIFY_CACHE_TTL=0`），用网关统一的 L1+L2 承担，代价是每次未命中都走一次 `verify_api_key` 的 DB 查询。

> 注意 `last_used_throttle()`（`api_key_service.rs:54`）也是进程内 `DashMap`，但它只影响 `last_used_at` 的写入频率，**不影响正确性**，无需处理。

### 6.7 授权策略

复用 `authz::required_perms(method, path)`：

```text
静态兜底表 static_perms_for(method, path)     // 代码内，覆盖常见路由
        ↓ 未命中
DB sys_menu (http_method, http_path) → perms  // 运维可维护
        ↓ 未命中
AUTHZ_MODE=enforce → 403（fail-closed）
AUTHZ_MODE=warn    → 放行 + 告警日志
```

gRPC 方法映射为伪路径（**不需要改 `authz.rs` 的匹配逻辑**）：

| gRPC 方法 | 伪路径 | 建议权限点 |
|---|---|---|
| `EventGateway/GetSnapshot` | `grpc:/assets.gateway.v1.EventGateway/GetSnapshot` | `gateway:event:read` |
| `EventGateway/Watch` | `grpc:/assets.gateway.v1.EventGateway/Watch` | `gateway:event:read` |
| `EventGateway/ChatStream` | `grpc:/assets.gateway.v1.EventGateway/ChatStream` | `gateway:chat:write` |
| `grpc.health.v1.Health/Check` | 不鉴权 | — |

运维只需在 `sys_menu` 里插入上述 `http_path` 与 `perms`，无需改代码；代码侧同时把这三条写进 `static_perms_for` 作为兜底。

**服务身份限制**：`api_key_service` 的主体没有 `user_id`，凡是 handler 需要 `claims.sub` 的接口必须拒绝（`authz.rs` 已有 `is_service_forbidden` 机制，gRPC 同样适用）。

### 6.8 gRPC 鉴权

因为 gRPC 被合并进 axum Router，它就是普通 HTTP 请求，**可以直接套 axum 中间件**，从而复用 `authenticate()`：

```rust
// grpc/mod.rs
pub fn router(state: Arc<AppState>) -> anyhow::Result<Router> {
    let health = tonic_health::server::health_reporter();
    tokio::spawn({
        let health = health.clone();
        async move {
            health.set_serving::<EventGatewayServer<EventGatewaySvc>>().await;
        }
    });

    Ok(tonic::service::Routes::new(EventGatewayServer::new(
        EventGatewaySvc::new(state.clone()),
    ))
    .add_service(health.service())
    .into_axum_router()                       // Router<()>
    .layer(middleware::from_fn_with_state(     // 只作用于 gRPC 路由
        state,
        middleware::grpc_auth,
    )))
}
```

```rust
// middleware/grpc_auth.rs
pub async fn grpc_auth(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Response {
    match state.auth.authenticate(&mut req).await {
        // 与 HTTP 共用同一份 with_schema，保证 tonic handler 内 schema_prefix() 正确
        Ok(principal) => {
            database::with_schema(principal.schema_name.clone(), async {
                req.extensions_mut().insert(principal);
                next.run(req).await
            })
            .await
        }
        Err(err) => grpc_status_response(err),
    }
}
```

**错误必须转成 gRPC 状态**，否则 gRPC 客户端只会看到 `Unknown`：

```rust
fn grpc_status_response(err: ApiError) -> Response {
    let status = match err.status() {
        StatusCode::UNAUTHORIZED => Status::unauthenticated("凭证无效或已过期"),
        StatusCode::FORBIDDEN => Status::permission_denied("权限不足"),
        StatusCode::TOO_MANY_REQUESTS => Status::resource_exhausted("请求过于频繁"),
        StatusCode::BAD_REQUEST => Status::invalid_argument("请求参数非法"),
        _ => Status::unavailable("服务暂时不可用"),
    };
    status.into_http().into_response()
}
```

| 业务语义 | HTTP | gRPC |
|---|---|---|
| 无/坏凭证 | 401 | `Unauthenticated`（附 `www-authenticate` metadata） |
| 权限不足 | 403 | `PermissionDenied` |
| 限流 | 429 | `ResourceExhausted`（附 `retry-after`） |
| 参数错误 | 400 | `InvalidArgument` |
| 找不到 | 404 | `NotFound` |
| DB/Redis 不可用 | 503 | `Unavailable` |
| 未实现 | 501 | `Unimplemented` |
| 超时 | 504 | `DeadlineExceeded` |

> 兜底方案：若 `Status::into_http()` 的 body 无法直接转成 `Response`，改为在每个方法入口读 `req.extensions().get::<AuthPrincipal>()`（中间件已注入），或在 3~5 个方法里各调一次 `state.auth.authenticate_grpc(req.metadata()).await?`。方法数量少，成本可控。

### 6.9 SSE 一次性票据

`EventSource` 无法设置请求头，把 token 放 URL 会被网关/浏览器/日志记录。方案：

```text
POST /api/auth/sse-ticket         （Cookie 或 Bearer 鉴权）
  → 200 {"ticket":"st_<32B>","expires_in":30}
  → Redis SET gw:v1:sse:ticket:<t> <principal_json> NX EX 30

GET /api/events?ticket=st_...
  → GETDEL gw:v1:sse:ticket:<t>
  → 命中：注入 principal，开始推流
  → 未命中：401（已用过/已过期）
```

补充：

- 票据只能是 `st_` 前缀，且**不接受** `at_` / `ak_` / JWT 作为票据；
- 票据与来源 IP 不强绑定（移动网络切换会断），但与 `tenant_id` 绑定；
- 前端也可改用 `fetch` + `ReadableStream` 直接带 `Authorization`，此时不需要票据。

### 6.10 错误语义

- HTTP 统一走 `api::response::ApiError`，保持现有 JSON 结构；
- 401 必须带 `WWW-Authenticate: Bearer realm="assets", error="invalid_token"`；
- 429 必须带 `Retry-After`；
- **不得**在响应体里回显令牌、Cookie、SQL、内部路径；
- 认证失败日志只记录 `token_prefix`（前 8 位）与 `request_id`。

---

## 7. SSE 设计

### 7.1 端点与职责

| 端点 | 方法 | 用途 | 认证 |
|---|---|---|---|
| `/api/chat/stream` | POST | 单次 LLM 流式响应（现有） | Bearer / Cookie |
| `/api/events` | GET | 长连订阅：通知、任务进度、跨实例广播（新增） | 票据 / Cookie / Bearer |
| `/api/auth/sse-ticket` | POST | 签发一次性票据（新增） | Bearer / Cookie |

`/api/chat/stream` 是「请求→响应」模型，用 POST + body 符合语义；`/api/events` 是「订阅」，必须是 GET（`EventSource` 只能 GET）。

### 7.2 事件格式（修正后）

```rust
// 每个事件显式给 event 名与 id，data 只放 JSON
Event::default()
    .event("token")                       // → event: token
    .id(next_event_id())                  // → id: 1754822400000-17
    .json_data(TokenPayload { text })?    // → data: {"text":"..."}
```

最终线格式：

```text
id: 1754822400000-17
event: token
data: {"text":"你好"}

```

| 事件名 | 触发 | data 结构 |
|---|---|---|
| `hello` | 连接建立 | `{"conn_id":"...","server":"gw-1","heartbeat":15000}` |
| `token` | LLM 增量 | `{"text":"..."}` |
| `message` | 通用通知 | `{"kind":"...","payload":{...}}` |
| `task` | 任务进度 | `{"task_id":1,"status":"running","percent":42}` |
| `error` | 流内错误 | `{"code":"upstream_error","message":"..."}` |
| `shutdown` | 实例即将退出 | `{"reason":"deploy","retry_ms":1000}` |
| `done` | 流结束 | `{"reason":"completed"}` |

**关键约束**

- `event` 名只允许 `[a-z_]`，前端按名分发；
- `data` 必须是单行 JSON（由 `json_data` 保证），**严禁**手工拼接 `"event: ...\n"`；
- `id` 必须全局单调（`时间戳-序号`），跨实例可用 `Last-Event-ID` 定位；
- 心跳不用 `data`，用注释帧 `:\n\n`（`Sse::keep_alive()` 默认行为），不触发前端 `onmessage`。

### 7.3 单实例内的连接与背压

```text
GET /api/events
  ├─ 鉴权 → 建 conn_id → 注册到本地 Hub（DashMap<conn_id, mpsc::Sender<Event>>）
  ├─ 按 tenant + 用户订阅 Redis：SUBSCRIBE gw:sse:<tenant> / gw:sse:user:<id>
  ├─ 产生 Stream：
  │     heartbeat(15s)  ─┐
  │     redis_rx         ─┼─▶ select! ─▶ 编码为 SSE 帧 ─▶ HTTP body
  │     shutdown_rx      ─┘
  └─ 客户端断开 → Drop guard 注销 + UNSUBSCRIBE
```

| 参数 | 默认 | 说明 |
|---|---|---|
| `SSE_HEARTBEAT_SECS` | 15 | 小于常见 LB 的 60s idle timeout |
| `SSE_MAX_CONNS_PER_INSTANCE` | 5000 | 超过返回 503 而非堆积 |
| `SSE_MAX_CONNS_PER_USER` | 20 | 防单用户刷连接 |
| `SSE_CHANNEL_CAPACITY` | 256 | 满了**丢弃最旧**并记 `sse_dropped_total` |
| `SSE_DRAIN_TIMEOUT_SECS` | 25 | 退出时最长等待 |
| `SSE_RETRY_MS` | 3000 | `retry:` 字段，客户端重连间隔 |

**背压策略**：广播是「尽力而为」，不能因为一个慢客户端阻塞整个租户的推送。用 `mpsc::Sender::try_send`，`Full` 时丢最旧并累加 `sse_dropped_total{tenant}`；关键事件（如 `task` 终态）走 `send().await` 并单独计数。

### 7.4 跨实例广播

```text
发布方（任意实例）
  ├─ XADD gw:v1:sse:stream:<tenant> MAXLEN ~ 1000 * kind=task payload=...
  └─ PUBLISH gw:sse:<tenant> <event_id>

订阅方（每个实例，进程级单连接）
  ├─ SUBSCRIBE gw:sse:<tenant>   → 收到 event_id
  ├─ 本地 fan-out 给本实例该租户的连接
  └─ （仅当连接请求了历史）XRANGE 补齐
```

为什么 Stream + Pub/Sub 双写：

- Pub/Sub 实时但不持久，实例断连期间的事件会丢；
- Stream 持久但需要游标读，用来支持 `Last-Event-ID` 续传；
- 两者用 `event_id` 关联：Pub/Sub 只负责「叫醒」，正文从 Stream 读（避免大 payload 走 Pub/Sub）。

### 7.5 续传（Last-Event-ID）

```text
GET /api/events?ticket=st_xxx
Last-Event-ID: 1754822400000-17
  → 先 SUBSCRIBE（避免回放期间的窗口丢事件）
  → XRANGE gw:v1:sse:stream:<tenant> (1754822400000-17 +
  → 逐条回放（最多 SSE_REPLAY_MAX=200 条）
  → 回放完切到实时流
```

边界：

- 若 `Last-Event-ID` 早于 Stream 最小 ID（被 `MAXLEN` 修剪），回放从最小 ID 开始，并在 `hello` 事件里标记 `"gap":true`，前端据此做一次全量拉取；
- 若无 `Last-Event-ID`，不回放历史，只推实时；
- Stream 的 key 按租户隔离，**不允许**跨租户回放。

### 7.6 与现有实现的关系

| 现状 | 问题 | 目标 |
|---|---|---|
| `chat_routes.rs` 用 `mpsc<String>` 传预格式化字符串 | 事件名丢失、`data` 被多行转义 | 传结构化 `(event, id, serde_json::Value)` |
| 无心跳 | 中间 LB 60s 断连 | `Sse::keep_alive()` |
| 无 `id` | 无法续传 | 全局递增 id |
| 取消链路只靠 channel 关闭 | 上游 LLM 可能继续生成 | `CancellationToken` + `Drop` 时 cancel |

### 7.7 反向代理 / Ingress 要求

| 项 | 要求 | 原因 |
|---|---|---|
| 响应缓冲 | `proxy_buffering off`（Nginx）/ `nginx.ingress.kubernetes.io/proxy-buffering: "off"` | 否则事件被攒批 |
| 压缩 | 关闭 `/api/events` 的 gzip | 压缩器会持有数据块 |
| 读超时 | `proxy_read_timeout 3600s` | 默认 60s 会断连 |
| HTTP 版本 | 客户端侧 HTTP/1.1 或 HTTP/2 均可 | HTTP/2 下注意 `max_concurrent_streams` |
| `X-Accel-Buffering` | 响应头置 `no`（双保险） | 兼容部分 CDN |
| 连接计数 | Ingress 侧 `upstream-keepalive` 与 `max-conns` 需按 SSE 峰值调 | 长连占用 |

```nginx
location = /api/events {
    proxy_http_version 1.1;
    proxy_set_header Connection "";
    proxy_buffering off;
    proxy_cache off;
    gzip off;
    proxy_read_timeout 3600s;
    proxy_send_timeout 3600s;
    add_header X-Accel-Buffering no;
}
```

---

## 8. gRPC 设计

### 8.1 proto 契约

`crates/gateway/proto/gateway.proto`（`package assets.gateway.v1`）：

```proto
syntax = "proto3";
package assets.gateway.v1;

import "google/protobuf/timestamp.proto";

service EventGateway {
  // 一次性拉取当前快照（未读计数、在线状态等）
  rpc GetSnapshot(GetSnapshotRequest) returns (GetSnapshotResponse);
  // 服务端流：订阅事件
  rpc Watch(WatchRequest) returns (stream Event);
  // 双向流：聊天（替代 SSE 的服务间形态）
  rpc ChatStream(stream ChatRequest) returns (stream ChatChunk);
}

message GetSnapshotRequest { string channel = 1; }

message GetSnapshotResponse {
  repeated Event events = 1;          // 最近 N 条
  uint64 last_event_id = 2;
}

message WatchRequest {
  string channel = 1;
  string last_event_id = 2;           // 为空则只推实时
  repeated string kinds = 3;          // 允许的事件类型，空 = 全部
}

message Event {
  string id = 1;                      // 与 SSE 的 id 同源
  string kind = 2;                    // token / message / task / shutdown / done
  string tenant = 3;
  google.protobuf.Timestamp time = 4;
  bytes payload = 5;                  // JSON（向后兼容优先，不用 Any）
}

message ChatRequest {
  oneof payload {
    ChatStart start = 1;
    string delta = 2;                 // 可选：多轮增量
    bool cancel = 3;
  }
}

message ChatStart {
  int64 model_id = 1;
  string conversation_id = 2;
  string prompt = 3;
}

message ChatChunk {
  string kind = 1;                    // token / error / done
  string text = 2;
  string error_code = 3;
}
```

**设计取舍**

- 用 `bytes payload`（内嵌 JSON）而非 `google.protobuf.Any`：跨语言客户端不必注册 type URL，演进靠 JSON 字段；
- `Event.id` 与 SSE 的 `id` 完全同源，同一份 Stream 数据可同时服务两种传输；
- 不引入 `google.api.http` 注解：本次不生成 REST 映射，避免额外依赖 `annotations.proto`。

### 8.2 生成代码

```rust
// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 本机未安装 protoc，使用随包二进制
    if std::env::var_os("PROTOC").is_none() {
        if let Some(path) = protoc_bin_vendored::protoc_bin_path()? {
            unsafe { std::env::set_var("PROTOC", path) };
        }
    }
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)              // 便于集成测试与本仓库内部调用
        .compile_protos(&["proto/gateway.proto"], &["proto"])?;
    Ok(())
}
```

- `tonic_prost_build` 由 `build-dependencies` 提供，生成 `assets.gateway.v1.rs`；
- 由于 build script 在编译期执行，`protoc-bin-vendored` 只需保证 `PROTOC` 环境变量指向解包后的可执行文件；
- CI 无网络时 `protoc-bin-vendored` 随 crate 下载，天然可用；
- 生成码**不提交**，加入 `.gitignore`（`target/` 已覆盖）。

### 8.3 服务实现要点

| 方法 | 实现 | 注意 |
|---|---|---|
| `GetSnapshot` | 从 Redis `XRANGE` 取最近 N 条 + `XLEN` | 只读，可加 5s 结果缓存 |
| `Watch` | 复用 SSE Hub 的同一条订阅路径，转 `mpsc<Event>` → `ReceiverStream` | 必须响应 `shutdown`，发送 `Event{kind:"shutdown"}` 后正常结束 |
| `ChatStream` | 请求流按 `ChatStart` 建会话，`cancel` 触发 `CancellationToken` | 客户端半关闭不等于取消，需显式 `cancel` |

```rust
type WatchStream = Pin<Box<dyn Stream<Item = Result<Event, Status>> + Send>>;

#[tonic::async_trait]
impl EventGateway for EventGatewaySvc {
    type WatchStream = WatchStream;
    type ChatStreamStream = /* ... */;

    async fn watch(
        &self,
        req: Request<WatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let principal = principal_of(&req)?;               // 中间件注入
        let sub = self.state.sse.subscribe(principal, req.into_inner()).await
            .map_err(|e| Status::unavailable(e.to_string()))?;
        Ok(Response::new(Box::pin(sub.map(Ok))))
    }
}
```

### 8.4 健康检查与反射

- `tonic_health::server::health_reporter()` 注册 `grpc.health.v1.Health`，与 `/livez` 分离：前者反映 gRPC 服务可用（含依赖），后者只反映进程存活；
- `tonic_reflection` 在非生产环境（`GATEWAY_ENV != "prod"`）注册，方便 `grpcurl` 调试；
- 生产关闭反射，避免暴露内部方法名（实际上会暴露，但权限点已经拦住调用）。

### 8.5 gRPC-Web

浏览器无法直接说 gRPC，需要 `tonic-web`：

```rust
let grpc = grpc::router(state.clone())?;            // Router<()>
let with_web = tonic_web::enable(grpc);             // 允许 application/grpc-web+proto
```

注意事项：

- `tonic_web::enable` 只能包裹 `Routes`/`Router`，会改写 `content-type` 与 trailer 编码；
- gRPC-Web 不支持双向流，`ChatStream` 在 Web 侧不可用（前端继续用 SSE）；
- CORS 需允许 `x-grpc-web`、`x-user-agent`、`grpc-timeout` 三个头，并 `expose-headers: grpc-status, grpc-message`。

### 8.6 超时与取消

| 层 | 机制 | 建议 |
|---|---|---|
| 客户端 | `grpc-timeout` header | 前端默认 60s，`Watch` 不设 |
| 网关 | `TimeoutLayer`（仅 unary） | unary 30s，`Watch`/`ChatStream` 排除 |
| 上游 LLM | `tokio::time::timeout` | 单块 60s，整体 300s |
| 取消 | `CancellationToken` 贯穿 handler → service → LLM | 客户端断开必须 cancel |
| 退出 | `shutdown` 广播 + `GracefulShutdown` | 先发 `shutdown` 事件，再 `SIGTERM` 超时强杀 |

> `TimeoutLayer` 对 streaming 响应会在超时点切断 body，因此**不能**包在 `Watch` 上；实现时按 path 前缀区分，或把 unary 与 streaming 拆成两个子 Router 分别挂 layer。

---

## 9. 多实例一致性

### 9.1 不变式

| 编号 | 不变式 | 违反后的表现 | 保障手段 |
|---|---|---|---|
| I1 | 任意实例读到的权限不早于「最后一次失效广播」 | 越权访问 | 版本号 + 短 TTL |
| I2 | 同一事件在全局只有一个 `id`，且单调 | 续传丢/重 | `时间戳-序号` + Redis Stream 生成 ID |
| I3 | 限流在租户/主体维度全局生效 | 实例数 × 配额 | Redis ZSET/Lua |
| I4 | DDL 同一时刻只有一个执行者 | 迁移失败、锁冲突 | `pg_advisory_lock` |
| I5 | 清理任务同一时刻只有一个执行者 | 重复 DELETE、锁竞争 | `pg_try_advisory_lock` 选主 |
| I6 | Snowflake worker id 全局唯一 | ID 冲突主键报错 | 启动强校验 |
| I7 | 实例退出不影响已建立的流 | 用户侧突然断流 | `shutdown` 事件 + drain |

### 9.2 迁移串行化

现状：`bootstrap::init_database_and_schema()` 在启动时执行迁移，多副本会并发 DDL。

```rust
// 使用固定 key（如 0x4153534554 = "ASSET"）
let mut tx = pool.begin().await?;
sqlx::query("SELECT pg_advisory_xact_lock($1)")
    .bind(MIGRATION_LOCK_KEY)      // 常量 i64，全仓库共享
    .execute(&mut *tx)
    .await?;
// 在事务内执行迁移（保证锁与迁移同一事务，异常自动释放）
run_migrations(&mut tx).await?;
tx.commit().await?;
```

要点：

- 用 `pg_advisory_xact_lock`（事务级）而非 session 级，避免连接归还池后锁泄漏；
- 锁 key 必须是常量并写进文档，所有实例一致；
- 迁移期间其它实例会被阻塞（不是失败），因此 `init_database_and_schema` 的超时要放宽到 ≥ 5 分钟；
- 建议同时加 `SET lock_timeout = '300s'`，避免无限等待。

### 9.3 清理任务选主

```text
每 30s：
  SELECT pg_try_advisory_lock(CLEANUP_LOCK_KEY)
    ├─ true  → 本实例成为 leader（锁绑定在该连接上）
    │         每 CLEANUP_INTERVAL 执行一次 cleanup_once()
    │         连接保活：SELECT 1；连接断则锁自动释放
    └─ false → 继续等待（日志降级为 debug）
```

| 参数 | 默认 | 说明 |
|---|---|---|
| `CLEANUP_LEADER_POLL_SECS` | 30 | 抢主轮询 |
| `CLEANUP_INTERVAL_SECS` | 300 | 实际清理周期 |
| `CLEANUP_LEADER_KEEPALIVE_SECS` | 10 | 保活，避免连接被中间件掐断 |

**关键**：`pg_try_advisory_lock` 的锁生命周期与 **session** 绑定，因此 leader 必须独占一条连接，不能归还池。实现上用 `pool.acquire()` 拿 `PoolConnection` 并长期持有；连接失效时释放并重新竞选。

### 9.4 认证缓存一致性

| 事件 | 动作 | 传播时间 |
|---|---|---|
| 用户角色/权限变更 | `INCR gw:v1:user:<id>:ver` + `PUBLISH gw:v1:invalidate` | ≤ 1 RTT |
| API Key 吊销 | `DEL gw:v1:auth:ak:<sha>` + `PUBLISH` | ≤ 1 RTT |
| 令牌撤销 | `DEL gw:v1:auth:at:<sha>` + `PUBLISH` | ≤ 1 RTT |
| 用户登出 | 同上 + `DEL` 会话 key | ≤ 1 RTT |
| 实例漏收广播 | L1 TTL 5s / L2 TTL 30s 兜底 | ≤ 30s |

```rust
// 版本号参与 key，旧版本自然淘汰
let ver: u64 = redis.get(format!("gw:v1:user:{uid}:ver")).await?;
let key = format!("gw:v1:auth:jwt:{h}:v{ver}");
```

> 版本号方案的代价是「每次请求多读一次本地缓存 + 偶尔一次 Redis」。为避免每请求都读 Redis，版本号本身也缓存 1s（可接受：1s 内的权限变更延迟）。

### 9.5 限流（Redis 滑动窗口）

**与现状的冲突（必须显式记录）**：`authz.rs:440-441` 的注释声明「单实例部署，进程内计数即可（设计文档 §6.8 不引入 Redis）」，多实例下配额会被放大 N 倍。本设计**改为 Redis 全局限流**，该注释需同步更新（B2）。

```lua
-- KEYS[1] = gw:v1:rl:<scope>:<subject>
-- ARGV[1] = now_ms, ARGV[2] = window_ms, ARGV[3] = limit, ARGV[4] = member
local now   = tonumber(ARGV[1])
local win   = tonumber(ARGV[2])
local limit = tonumber(ARGV[3])

redis.call('ZREMRANGEBYSCORE', KEYS[1], 0, now - win)
local used = redis.call('ZCARD', KEYS[1])
if used >= limit then
  local oldest = redis.call('ZRANGE', KEYS[1], 0, 0, 'WITHSCORES')
  local retry = 0
  if oldest[2] then retry = win - (now - tonumber(oldest[2])) end
  return {0, used, retry}
end
redis.call('ZADD', KEYS[1], now, ARGV[4])
redis.call('PEXPIRE', KEYS[1], win)
return {1, used + 1, 0}
```

| 维度 | scope | subject | limit 来源 |
|---|---|---|---|
| 全局 | `g` | `all` | `GATEWAY_GLOBAL_RPS`（保护整体） |
| 租户 | `t` | `tenant_id` | `GATEWAY_TENANT_RPS` |
| 主体 | `p` | `u:<id>` / `k:<key_id>` | Key 自带 `rate_limit_per_min`，否则 `GATEWAY_RATE_LIMIT_RPS` |
| 路由 | `r` | `<route>` | 敏感路由（登录、内省）单列 |

要点：

- 使用 `EVALSHA`（首次 `SCRIPT LOAD`，NOSCRIPT 时回填），避免每次传脚本体；
- member 用 `uuid` 或 `实例id-序号`，避免同毫秒 ZSET 成员覆盖；
- **降级策略**：`RATE_LIMIT_FAIL_MODE=closed`（默认，Redis 不可用返回 503）| `open`（放行 + 告警）。默认 closed 与 `auth.rs:420` 的「不降级」原则一致；
- 时钟：全部用 Redis 侧或网关侧 time_ms 均可，但**必须同源**，否则窗口漂移。推荐 `ARGV[1]` 用网关时间，容忍各实例 1s 内偏差。

### 9.6 Snowflake worker id

多实例部署时 `worker_id` 必须唯一，否则会产生重复 ID（主键冲突或数据错乱）。

| 方案 | 实现 | 适用 |
|---|---|---|
| 环境变量（默认） | `SNOWFLAKE_WORKER_ID=0..31`，K8s 用 StatefulSet 序号注入 | 副本数固定 |
| 启动自检 | 启动时 `SET gw:v1:snowflake:w:<id> <instance_id> NX EX 60`，抢不到直接退出并报错 | 所有场景 |
| 心跳续约 | 每 20s `EXPIRE`，实例崩溃 60s 后 id 可被接管 | 所有场景 |

```rust
let ok: Option<String> = redis.set_nx_px(&key, &instance_id, 60_000).await?;
if ok.is_none() {
    anyhow::bail!("Snowflake worker id {id} 已被其他实例占用：{}", owner);
}
```

> 上线前必须核对：现有 `SNOWFLAKE_WORKER_ID` 是否为单实例硬编码（如 1）；若使用 StatefulSet，用 `metadata.name` 末尾序号解析；若使用 Deployment，改用上述自检 + 递增分配（读取 `gw:v1:snowflake:next`）。

### 9.7 事件 ID 与顺序

- 事件 id 采用 Redis Stream 返回的 `<ms>-<seq>`，天然全局单调且跨实例唯一；
- 若某事件不需要持久化（如纯心跳），不使用 id；
- `Last-Event-ID` 的比较依赖 Redis Stream ID 的字典序语义，前端**不要**把它当数字解析；
- 同一租户同一 channel 的顺序由 Stream 保证；跨 channel 无序，前端不要跨 channel 排序。

### 9.8 优雅退出

```text
SIGTERM / Ctrl-C
  ├─ 1. shutdown.cancel()
  ├─ 2. /readyz 立即返回 503（LB 摘流，等待就绪探针周期 ~5s）
  ├─ 3. SSE：广播 event: shutdown（data 含 retry_ms），然后关闭连接
  ├─ 4. gRPC：Watch 流发送 shutdown 事件后正常结束；unary 继续处理
  ├─ 5. axum::serve 的 graceful_shutdown 等待在途请求，最长 SSE_DRAIN_TIMEOUT_SECS=25
  ├─ 6. 释放 advisory lock、UNSUBSCRIBE、关闭 Redis 连接
  └─ 7. 20s 内未完成 → 进程退出（K8s terminationGracePeriodSeconds=40）
```

```rust
pub async fn signal(token: CancellationToken) {
    let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    tokio::select! {
        _ = term.recv() => tracing::info!("收到 SIGTERM"),
        _ = tokio::signal::ctrl_c() => tracing::info!("收到 Ctrl-C"),
    }
    token.cancel();
}
```

**顺序反了会出问题**：必须先摘流再关连接，否则 LB 仍会把新请求打进来，导致 `Connection reset`。

---

## 10. 可观测性

### 10.1 链路追踪（必须先修的问题）

**现状缺陷**：仓库内已用 `tracing` + `opentelemetry-otlp`，但**没有设置全局 propagator**，导致：
- 入站请求不从 `traceparent` / `tracestate` 提取上下文；
- 出站（reqwest / tonic）不会注入 `traceparent`；
- 现象：日志里有 trace_id，但跨服务无法串联，每个服务都是新的 trace（trace 断链）。

```rust
// 修复：进程启动最早处
use opentelemetry::global;
use opentelemetry_sdk::propagation::TraceContextPropagator;

global::set_text_map_propagator(TraceContextPropagator::new());
```

配套要求：

| 项 | 要求 |
|---|---|
| 提取点 | `TraceLayer` 前；gRPC 由 `tonic` 的 `OtelLayer`（或手工从 metadata 提取） |
| 注入点 | `reqwest` 客户端通过 `tracing_opentelemetry` 的中间件自动注入 |
| 采样率 | `OTEL_TRACES_SAMPLER_ARG=0.1`（生产 10%），错误请求强制采样 |
| Service 名 | `OTEL_SERVICE_NAME=assets-gateway`，实例区分用资源属性 `service.instance.id` |
| 传播格式 | 只开 `tracecontext` + `baggage`，不开 b3（避免多格式歧义） |

### 10.2 日志

- 结构化 JSON（`LOG_FORMAT=json`），字段固定：`ts / level / target / msg / trace_id / span_id / request_id / tenant_id / subject / route / status / latency_ms`；
- **禁止**打印：`Authorization`、`Cookie`、`Set-Cookie`、`ticket`、`password`、`client_secret`；
- `RUST_LOG` 默认 `info,hyper=warn,sqlx=warn,h2=warn`；
- 每实例日志带 `instance_id`（= Pod 名），便于定位「同一请求落到哪个实例」。

### 10.3 指标（OTel meter，前缀 `gw_`）

| 指标 | 类型 | 标签 | 用途 |
|---|---|---|---|
| `gw_http_requests_total` | counter | `route, method, status` | 错误率 |
| `gw_http_request_duration_ms` | histogram | `route, method` | P95/P99 |
| `gw_auth_total` | counter | `kind(jwt/at/ak/cookie/ticket), result` | 认证成功率 |
| `gw_auth_cache_total` | counter | `layer(l1/l2), result(hit/miss)` | 缓存命中率 |
| `gw_rate_limited_total` | counter | `scope, route` | 限流触发 |
| `gw_sse_connections` | gauge | `tenant` | 在线连接数 |
| `gw_sse_events_total` | counter | `event` | 推送量 |
| `gw_sse_dropped_total` | counter | `tenant, reason(capacity/closed)` | 背压丢弃 |
| `gw_sse_replay_total` | counter | `result(ok/gap)` | 续传质量 |
| `gw_grpc_requests_total` | counter | `method, code` | gRPC 错误率 |
| `gw_redis_latency_ms` | histogram | `op` | Redis 健康 |
| `gw_leader` | gauge | — | 1=本实例是清理 leader |
| `gw_shutdown_drain_ms` | histogram | — | 退出耗时 |

> 命名统一加 `gw_` 前缀，避免与业务服务（`assets-server`）的指标名冲突；不要在指标里放 `user_id` 等高基数标签。

### 10.4 探针语义

| 端点 | 语义 | 依赖检查 | 用于 |
|---|---|---|---|
| `/healthz` | **沿用现有实现**：进程存活 + DB `SELECT 1` | Postgres（`health.rs:19-27`，超时 2s） | `livenessProbe`（现状） |
| `/livez` | 纯进程存活（新增） | 无 | `livenessProbe`（推荐切换） |
| `/readyz` | 可承接流量（新增） | Postgres + Redis + 未处于 shutdown | `readinessProbe` |
| `grpc.health.v1.Health/Check` | gRPC 可用 | 同 `/readyz` | gRPC LB |

**关键**：现有 `/healthz` **会查 DB**（`health.rs:49` → `db_ping()`），语义上其实是 readiness。这对多实例是危险的：数据库抖动会让**所有 Pod 被 liveness 重启**，把可恢复故障放大成雪崩。因此：

- **不要改 `/healthz` 的行为**（桌面端、现有部署、巡检脚本都依赖它），但 K8s `livenessProbe` 应**改用新增的 `/livez`**；
- `/healthz` 保留给「人看的自检」与外部巡检；
- 网关**不得**再注册 `/healthz`（`lib.rs:241` 已注册，重复会 panic）。

```rust
// /livez：绝不触达任何外部依赖
async fn livez() -> StatusCode { StatusCode::OK }

// /readyz：依赖全通过才 200
async fn readyz(State(state): State<Arc<AppState>>) -> Response {
    if state.shutdown.is_cancelled() {
        return (StatusCode::SERVICE_UNAVAILABLE, "draining").into_response();
    }
    let db_ok = tokio::time::timeout(
        Duration::from_secs(2),
        sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&state.pool),
    ).await.map(|r| r.is_ok()).unwrap_or(false);
    let redis_ok = state.redis.ping().await.is_ok();
    if db_ok && redis_ok { StatusCode::OK.into_response() }
    else { (StatusCode::SERVICE_UNAVAILABLE, "dependency down").into_response() }
}
```

### 10.5 告警

| 告警 | 条件 | 级别 |
|---|---|---|
| 5xx 比例 | `rate(gw_http_requests_total{status=~"5.."}[5m]) > 1%` 持续 5m | P2 |
| 认证失败激增 | `rate(gw_auth_total{result="fail"}[5m]) > 50/s` | P2（可能是喷洒） |
| 限流占比 | `rate(gw_rate_limited_total[5m]) / rate(gw_http_requests_total[5m]) > 10%` | P3 |
| SSE 丢弃 | `rate(gw_sse_dropped_total{reason="capacity"}[5m]) > 0` | P3 |
| SSE 连接数 | `gw_sse_connections > 4000`（单实例） | P2 |
| Redis 错误 | `rate(gw_redis_latency_ms_count{op=~".*"}[5m])` 异常 / 错误日志 | P1 |
| leader 缺失 | `sum(gw_leader) == 0` 持续 10m | P2（清理停摆） |
| 无实例就绪 | `sum(up{job="assets-gateway"}) == 0` | P1 |

---

## 11. 安全设计

### 11.1 传输与网络

| 项 | 要求 |
|---|---|
| 外部 | 强制 HTTPS（Ingress 终止），HSTS 由 Ingress 加 |
| 内部 | h2c（明文），要求集群网络策略限制入站源（只允许 Ingress / 同命名空间） |
| Redis | 启用 `requirepass` + 内网访问；不使用公网 |
| Postgres | TLS（`sslmode=require` 或 verify-full） |
| gRPC 反射 | 生产关闭 |

### 11.2 输入面

- 请求体大小限制：`DefaultBodyLimit::max(GATEWAY_MAX_BODY_MB * 1MB)`，默认 32MB；上传路由单独放宽；
- 头部大小：`hyper` 默认足够，但反向代理侧需同步 `large_client_header_buffers`；
- URL 长度、查询参数个数上限（防 ReDoS / 日志放大）；
- 所有路径参数做白名单校验，禁止 `..`、`%2e%2e`；
- 反代模式下的路径穿越：`GATEWAY_UPSTREAMS` 只做**前缀拼接 + 剩余路径透传**，并对 `..` 段直接 400。

### 11.3 认证边界

| 风险 | 缓解 |
|---|---|
| 令牌放 URL（EventSource 限制） | 一次性票据（§6.9），30s TTL，`GETDEL` |
| JWT 算法混淆 | 显式指定算法，禁用 `none`，不从 header 取 alg 决定校验方式 |
| HS256 密钥泄露 | 密钥长度 ≥ 32 字节；启动校验；支持轮换（`JWT_SECRET` + `JWT_SECRET_PREVIOUS`） |
| JWKS 劫持 | 只允许 `https`；校验 `kid`；响应大小上限 |
| CSRF | 双提交 Cookie + `SameSite=Lax`；`/api/events` 只读 |
| 越权 | 统一走 `authz`，不在 handler 内自判断 |
| 票据重放 | `GETDEL` 原子消费 + 指纹绑定 |
| 内部调用伪造 | 反向代理模式必须带 `X-Gateway-Secret`（HMAC）或 mTLS，上游拒绝非网关来源 |

### 11.4 输出面

- 安全响应头：`X-Content-Type-Options: nosniff`、`Referrer-Policy: strict-origin-when-cross-origin`、`X-Frame-Options: DENY`（仅 API 域名）；
- 错误响应统一结构，不回显堆栈与 SQL；
- CORS：只允许配置内源，`allow_credentials` 与 `*` 不可同时存在；
- Cookie 属性：`HttpOnly; Secure; SameSite=Lax; Path=/`；
- SSE 响应头 `Cache-Control: no-store`，防止中间缓存留数据。

### 11.5 审计

- 认证失败、权限拒绝、限流触发、API Key 使用、票据签发消费，均写结构化审计日志（可异步落表）；
- 审计日志字段：`ts, instance_id, tenant_id, subject, action, resource, result, request_id, ip, ua`；
- 保留期与业务库对齐（默认 90 天），敏感字段脱敏。

---

## 12. 配置

### 12.1 环境变量清单

| 变量 | 默认 | 必填 | 说明 |
|---|---|---|---|
| `GATEWAY_HOST` | `0.0.0.0` | 否 | 监听地址 |
| `GATEWAY_PORT` | `8080` | 否 | 监听端口（与现有 `assets-server` 一致） |
| `GATEWAY_GRPC_MODE` | `merged` | 否 | `merged` / `separate` |
| `GATEWAY_GRPC_PORT` | `9090` | 否 | 仅 `separate` 生效 |
| `GATEWAY_ENV` | `dev` | 否 | `dev` / `staging` / `prod`，影响反射与严格校验 |
| `GATEWAY_INSTANCE_ID` | Pod 名或 `hostname-pid` | 否 | 日志/限流 member/选主标识 |
| `DATABASE_URL` | — | **是** | Postgres 连接串（现有） |
| `REDIS_URL` | — | **是**（多实例） | `redis://[:pass@]host:port/db` |
| `REDIS_POOL_SIZE` | `16` | 否 | 连接池上限 |
| `JWT_SECRET` | — | **是** | HS256 密钥，≥ 32 字节 |
| `JWT_ALG` | `HS256` | 否 | `HS256` / `RS256` / `ES256` |
| `JWT_JWKS_URL` | — | `RS256`/`ES256` 必填 | JWKS 地址（https） |
| `JWT_ISSUER` | 空 | 否 | 非空则强校验 `iss` |
| `JWT_AUDIENCE` | 空 | 否 | 非空则强校验 `aud` |
| `AUTHZ_MODE` | `enforce` | 否 | `enforce` / `warn` |
| `AUTH_CACHE_L1_TTL_MS` | `5000` | 否 | L1 TTL |
| `AUTH_CACHE_L2_TTL_SECS` | `30` | 否 | L2 TTL |
| `GATEWAY_GLOBAL_RPS` | `0`（不限） | 否 | 全局限流 |
| `GATEWAY_TENANT_RPS` | `200` | 否 | 租户级限流 |
| `GATEWAY_RATE_LIMIT_RPS` | `50` | 否 | 主体级缺省 |
| `RATE_LIMIT_FAIL_MODE` | `closed` | 否 | `closed` / `open` |
| `SSE_HEARTBEAT_SECS` | `15` | 否 | 心跳间隔 |
| `SSE_MAX_CONNS_PER_INSTANCE` | `5000` | 否 | 单实例连接上限 |
| `SSE_MAX_CONNS_PER_USER` | `20` | 否 | 单用户连接上限 |
| `SSE_CHANNEL_CAPACITY` | `256` | 否 | 每连接发送队列 |
| `SSE_DRAIN_TIMEOUT_SECS` | `25` | 否 | 退出排空超时 |
| `SSE_RETRY_MS` | `3000` | 否 | `retry:` 字段 |
| `SSE_REPLAY_MAX` | `200` | 否 | 续传最大回放条数 |
| `SSE_STREAM_MAXLEN` | `1000` | 否 | Stream 修剪长度 |
| `SSE_TICKET_TTL_SECS` | `30` | 否 | 票据有效期 |
| `SNOWFLAKE_WORKER_ID` | — | **是** | `0..31`，全局唯一 |
| `CLEANUP_INTERVAL_SECS` | `300` | 否 | 清理周期 |
| `CLEANUP_LEADER_POLL_SECS` | `30` | 否 | 抢主轮询 |
| `GATEWAY_MAX_BODY_MB` | `32` | 否 | 请求体上限 |
| `GATEWAY_CORS_ORIGINS` | 空 | 否 | 逗号分隔白名单 |
| `GATEWAY_UPSTREAMS` | 空 | 否 | `prefix=url,prefix=url` |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | 空 | 否 | 空则只打日志 |
| `OTEL_SERVICE_NAME` | `assets-gateway` | 否 | 服务名 |
| `OTEL_TRACES_SAMPLER_ARG` | `0.1` | 否 | 采样率 |
| `LOG_FORMAT` | `json`（prod）/ `pretty`（dev） | 否 | 日志格式 |
| `RUST_LOG` | `info,hyper=warn,sqlx=warn,h2=warn` | 否 | 过滤规则 |

### 12.2 配置结构与校验

```rust
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub host: String,
    pub port: u16,
    pub grpc: GrpcMode,
    pub env: Env,
    pub instance_id: String,
    pub database_url: String,
    pub redis: RedisConfig,          // url / pool_size / timeouts
    pub jwt: JwtConfig,              // alg / secret / jwks / iss / aud / leeway
    pub authz: AuthzConfig,          // mode
    pub cache: CacheConfig,          // l1_ttl / l2_ttl
    pub rate_limit: RateLimitConfig, // global/tenant/subject / fail_mode
    pub sse: SseConfig,              // heartbeat / limits / replay / ticket_ttl
    pub snowflake_worker_id: u8,
    pub cleanup: CleanupConfig,
    pub body_limit_mb: usize,
    pub cors_origins: Vec<String>,
    pub upstreams: Vec<Upstream>,
    pub otel: OtelConfig,
}
```

**启动即失败（fail fast）** 的校验项：

| 校验 | 触发条件 | 错误信息 |
|---|---|---|
| 端口范围 | `port == 0` 或 `> 65535` | `GATEWAY_PORT 非法` |
| 密钥强度 | `JWT_ALG=HS256` 且 `len(secret) < 32` | `JWT_SECRET 长度不足 32 字节` |
| JWKS 必填 | `JWT_ALG!=HS256` 且 `jwks_url` 为空 | `JWT_JWKS_URL 必填` |
| JWKS 协议 | `jwks_url` 非 `https://` 且 `env=prod` | `生产环境 JWKS 必须使用 https` |
| Redis 必填 | `env=prod` 且 `REDIS_URL` 空 | `生产环境必须配置 REDIS_URL` |
| worker id | `> 31` | `SNOWFLAKE_WORKER_ID 必须在 0..=31` |
| worker 占用 | Redis 抢位失败 | `worker id 已被 <instance> 占用` |
| CORS | 含 `*` 且 `allow_credentials` | `CORS 不允许 * 与凭证共存` |
| 上游 URL | 非法 URL / 前缀不以 `/` 开头 | `GATEWAY_UPSTREAMS 第 N 项非法` |
| SSE 容量 | `capacity < 16` | `SSE_CHANNEL_CAPACITY 过小` |

### 12.3 `.env.toml` 增量

在 `apps/backend/.env.example.toml` 追加（沿用现有 `[section]` 风格）：

```toml
[gateway]
host = "0.0.0.0"
port = 8080
env = "dev"                       # dev / staging / prod
# merged = HTTP + gRPC 同端口；separate = 独立端口
grpc_mode = "merged"
grpc_port = 9090
instance_id = ""                  # 建议注入 POD_NAME
# upstreams = ["/doc-parser=http://127.0.0.1:8090"]   # embedded 模式留空

[gateway.sse]
heartbeat_secs = 15
max_conns_per_instance = 5000
max_conns_per_user = 20
channel_capacity = 256
drain_timeout_secs = 25
retry_ms = 3000
replay_max = 200
stream_maxlen = 1000
ticket_ttl_secs = 30

[gateway.rate_limit]
global_rps = 0
tenant_rps = 200
subject_rps = 50
fail_mode = "closed"              # closed / open

[redis]
# 多实例部署必须配置；单机开发可留空走内存实现
url = "redis://:password@127.0.0.1:6379/0"
pool_size = 16
connect_timeout_secs = 3
command_timeout_secs = 2

[auth]
jwt_secret = "请替换为至少32字节的随机串"
jwt_alg = "HS256"                 # HS256 / RS256 / ES256
# jwt_jwks_url = "https://id.example.com/.well-known/jwks.json"
# jwt_issuer = "https://id.example.com"
# jwt_audience = "assets-api"
authz_mode = "enforce"            # enforce / warn
cache_l1_ttl_ms = 5000
cache_l2_ttl_secs = 30
```

> 环境变量优先级高于 `.env.toml`（与现有 `bootstrap` 的合并策略一致）：K8s 里通常只覆盖 `REDIS_URL`、`JWT_SECRET`、`SNOWFLAKE_WORKER_ID`、`GATEWAY_INSTANCE_ID` 四项。

### 12.4 配置热更新边界

| 项 | 是否热更 | 说明 |
|---|---|---|
| 限流阈值 | ✅ | 每 30s 从 Redis `gw:v1:conf:ratelimit` 读取（可选） |
| CORS 白名单 | ❌ | `CorsLayer` 在构建期固定，需重启 |
| JWT 密钥 | ⚠️ | 支持「当前 + 上一个」双密钥，轮换后重启 |
| SSE 容量 | ❌ | 需重启 |
| 上游列表 | ❌ | 需重启 |
| `AUTHZ_MODE` | ✅ | 读原子变量，管理接口可切换 |

---

## 13. 部署

### 13.1 镜像

```dockerfile
# ---- builder ----
FROM rust:1.90-bookworm AS builder
WORKDIR /src
# protoc 由 protoc-bin-vendored 提供，无需 apt 安装
COPY . .
RUN cargo build --release -p gateway --bin assets-gateway

# ---- runtime ----
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl tzdata && rm -rf /var/lib/apt/lists/*
RUN useradd -r -u 10001 -s /usr/sbin/nologin app
WORKDIR /app
COPY --from=builder /src/target/release/assets-gateway /usr/local/bin/assets-gateway
COPY --from=builder /src/apps/backend/.env.example.toml /app/.env.toml
USER 10001
EXPOSE 8080
ENV RUST_LOG=info,hyper=warn,sqlx=warn,h2=warn GATEWAY_ENV=prod LOG_FORMAT=json
HEALTHCHECK --interval=15s --timeout=3s --start-period=20s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/livez || exit 1
ENTRYPOINT ["/usr/local/bin/assets-gateway"]
```

要点：

- 运行镜像不含 `protoc`、`cargo`、源码；
- 以非 root（uid 10001）运行；
- `HEALTHCHECK` 用 `/livez`（纯存活），**不要**用 `/healthz`（会查 DB，抖动时标记 unhealthy）也不要 `/readyz`；
- 用 `debian:bookworm-slim` 而非 `alpine`：`jsonwebtoken` 的 `aws_lc_rs` 后端在 glibc 上更稳定。

### 13.2 Kubernetes（Deployment）

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: assets-gateway
spec:
  replicas: 3
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxSurge: 1
      maxUnavailable: 0          # 先起后停，配合优雅退出
  selector:
    matchLabels: { app: assets-gateway }
  template:
    metadata:
      labels: { app: assets-gateway }
    spec:
      terminationGracePeriodSeconds: 40
      containers:
        - name: gateway
          image: registry.example.com/assets-gateway:1.0.0
          ports:
            - { name: http, containerPort: 8080 }
          env:
            - name: GATEWAY_ENV
              value: prod
            - name: GATEWAY_INSTANCE_ID
              valueFrom: { fieldRef: { fieldPath: metadata.name } }
            - name: REDIS_URL
              valueFrom: { secretKeyRef: { name: gateway-secret, key: redis-url } }
            - name: JWT_SECRET
              valueFrom: { secretKeyRef: { name: gateway-secret, key: jwt-secret } }
            - name: DATABASE_URL
              valueFrom: { secretKeyRef: { name: gateway-secret, key: database-url } }
          envFrom:
            - configMapRef: { name: gateway-config }
          readinessProbe:
            httpGet: { path: /readyz, port: http }
            initialDelaySeconds: 5
            periodSeconds: 5
            failureThreshold: 3
          livenessProbe:
            httpGet: { path: /livez, port: http }
            initialDelaySeconds: 20
            periodSeconds: 10
            failureThreshold: 3
          resources:
            requests: { cpu: 500m, memory: 512Mi }
            limits:   { cpu: "2",  memory: 2Gi }
          lifecycle:
            preStop:
              exec:
                # 等 LB 摘流（Endpoint 移除）后再退出
                command: ["/bin/sh", "-c", "sleep 8"]
```

**为什么需要 `preStop sleep 8`**：Endpoint 移除与 kube-proxy / Ingress 配置刷新不同步，立即退出会有几秒窗口把请求打到已停止的 Pod，表现为零星 `502` / `Connection reset`。

### 13.3 Worker ID 分配

方案 A：`StatefulSet` + 序号

```yaml
kind: StatefulSet
metadata: { name: assets-gateway }
spec:
  serviceName: assets-gateway
  replicas: 3
  template:
    spec:
      containers:
        - name: gateway
          env:
            - name: POD_NAME
              valueFrom: { fieldRef: { fieldPath: metadata.name } }
            # entrypoint 中解析 hostname 末尾序号并导出 SNOWFLAKE_WORKER_ID
            - name: SNOWFLAKE_WORKER_ID
              value: "0"
```

方案 B：`Deployment` + 启动抢位（§9.6），worker id 在 `1..=31` 内顺序尝试，抢到即用。

| 方案 | 优点 | 缺点 |
|---|---|---|
| StatefulSet + 序号 | 确定、可预测 | Pod 重建慢，滚动发布顺序化 |
| Deployment + 抢位 | 弹性好 | 依赖 Redis，首次启动稍慢 |

### 13.4 Service 与 Ingress

```yaml
apiVersion: v1
kind: Service
metadata:
  name: assets-gateway
spec:
  selector: { app: assets-gateway }
  ports:
    - { name: http, port: 80, targetPort: http, appProtocol: http2 }
```

```yaml
# Ingress A：HTTP 业务 + SSE
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: assets-gateway-http
  annotations:
    nginx.ingress.kubernetes.io/proxy-body-size: "32m"
    nginx.ingress.kubernetes.io/proxy-read-timeout: "3600"
    nginx.ingress.kubernetes.io/proxy-send-timeout: "3600"
    nginx.ingress.kubernetes.io/proxy-buffering: "off"
    nginx.ingress.kubernetes.io/enable-cors: "true"
spec:
  ingressClassName: nginx
  tls:
    - hosts: [api.example.com]
      secretName: api-tls
  rules:
    - host: api.example.com
      http:
        paths:
          - path: /api
            pathType: Prefix
            backend: { service: { name: assets-gateway, port: { number: 80 } } }
          - path: /oauth
            pathType: Prefix
            backend: { service: { name: assets-gateway, port: { number: 80 } } }
---
# Ingress B：gRPC（必须单独一个，因为 backend-protocol 是 Ingress 级注解）
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: assets-gateway-grpc
  annotations:
    nginx.ingress.kubernetes.io/backend-protocol: "GRPC"
    nginx.ingress.kubernetes.io/proxy-read-timeout: "3600"
spec:
  ingressClassName: nginx
  tls:
    - hosts: [grpc.example.com]
      secretName: grpc-tls
  rules:
    - host: grpc.example.com
      http:
        paths:
          - path: /
            pathType: Prefix
            backend: { service: { name: assets-gateway, port: { number: 80 } } }
```

> `backend-protocol: GRPC` 是 **Ingress 级**注解，无法按 path 区分。因此若 HTTP 与 gRPC 共用域名，则要么走 `GATEWAY_GRPC_MODE=separate`（两个 Service 端口 + 两个 Ingress），要么接受 HTTP/2 prior-knowledge 由 nginx 透传（需 `nginx.ingress.kubernetes.io/backend-protocol: HTTP` + `http2` 上游，配置更脆弱）。

### 13.5 容量估算

| 资源 | 单实例经验值 | 说明 |
|---|---|---|
| 空闲内存 | 60–100 MB | Rust 运行时 |
| 每 SSE 连接 | 8–20 KB | 含队列缓冲（256 × 小事件） |
| 每 1000 RPS | 0.3–0.5 核 | 纯转发场景 |
| 每 1000 并发 SSE | ~0.1 核 + 100 MB | 空闲连接几乎不耗 CPU |
| PG 连接 | `max_connections × 实例数 ≤ 数据库上限` | 3 实例 × 10 = 30 |

**易被忽略的瓶颈**：SSE 长连会占满 Ingress 的 `worker-connections`（默认 1024）。8000 并发 SSE 需要 ≥ 8 个 nginx worker 或提高 `worker_connections`，否则表现为新连接排队甚至 502。

### 13.6 发布与回滚

| 步骤 | 操作 | 校验 |
|---|---|---|
| 1 | 构建镜像并打 tag（含 git sha） | `docker run --rm <img> assets-gateway --version` |
| 2 | 部署到 staging（1 副本） | `/readyz` 200 + 冒烟脚本 |
| 3 | 滚动更新生产（`maxUnavailable: 0`） | 观察 `gw_http_requests_total{status=~"5.."}` |
| 4 | 观察 10 分钟 | 5xx < 0.5%，`gw_sse_dropped_total` 无突增 |
| 5 | 回滚 | `kubectl rollout undo deployment/assets-gateway` |

**迁移不可逆时**：先做「扩展型迁移」（加列/加表），下个版本再删旧列。回滚旧版本镜像不能依赖回滚 DDL。

---

## 14. 实施计划

### 14.1 阶段划分

| 阶段 | 目标 | 交付物 | 预估 |
|---|---|---|---|
| **P0** 可编译骨架 | `crates/gateway` 能启动、能转发 | crate + `build_router` 提取 + `/livez` `/readyz` + Dockerfile | 2～3 人日 |
| **P1** 认证一致性 | 多实例认证结果一致 | Redis L2 + 版本号 + 失效广播 + worker id 自检 | 3～4 人日 |
| **P2** 全局限流 | 配额不再 × N | Redis ZSET/Lua + 注入 `RateLimiter` + 指标 | 2 人日 |
| **P3** SSE | 跨实例推送 + 续传 | `sse.rs` + `sse_bus.rs` + 票据 + Ingress 配置 | 4～6 人日 |
| **P4** gRPC | `EventGateway` 上线 | proto + build.rs + `grpc_auth` + health/reflection | 4～5 人日 |
| **P5** 生产化 | 可观测、压测、灰度 | OTel propagator 修复 + 指标 + 压测报告 | 3 人日 |

> P0 必须先做且独立可验证：它把所有「侵入 api crate」的改动一次性做完，后续阶段只新增文件。

### 14.2 P0 验收

```powershell
# 1. 编译
cargo build -p gateway --bin assets-gateway

# 2. 启动（本地 PG，无 Redis 时走内存实现）
$env:DATABASE_URL="postgres://postgres:123456@127.0.0.1:5432/ceshi2"
$env:JWT_SECRET="0123456789abcdef0123456789abcdef"
$env:SNOWFLAKE_WORKER_ID="1"
.\target\debug\assets-gateway.exe

# 3. 探针（PG 正常时期望全 200）
curl.exe -i http://127.0.0.1:8080/livez       # 纯存活，永不依赖外部
curl.exe -i http://127.0.0.1:8080/readyz      # PG + Redis 均通过才 200
curl.exe -i http://127.0.0.1:8080/healthz     # 现有端点，会查 DB

# 4. 业务路由仍然可用（未带凭证应 401 而不是 404）
curl.exe -i http://127.0.0.1:8080/api/users   # 期望 401
```

### 14.3 P1 验收

```powershell
# 起两个实例（不同端口 + 不同 worker id）
$env:GATEWAY_PORT="8080"; $env:SNOWFLAKE_WORKER_ID="1"; Start-Process .\target\debug\assets-gateway.exe
$env:GATEWAY_PORT="8081"; $env:SNOWFLAKE_WORKER_ID="2"; Start-Process .\target\debug\assets-gateway.exe

# 同一 token 打两个实例，结果应一致
curl.exe -s -o NUL -w "%{http_code}`n" -H "Authorization: Bearer $tok" http://127.0.0.1:8080/api/users
curl.exe -s -o NUL -w "%{http_code}`n" -H "Authorization: Bearer $tok" http://127.0.0.1:8081/api/users

# 第三个实例用重复 worker id，必须启动失败
$env:GATEWAY_PORT="8082"; $env:SNOWFLAKE_WORKER_ID="1"
.\target\debug\assets-gateway.exe     # 期望退出码 != 0，日志含 "已被占用"
```

### 14.4 P2 验收

```powershell
# 限流阈值设小，验证全局而非单实例
$env:GATEWAY_RATE_LIMIT_RPS="5"
1..20 | ForEach-Object {
  curl.exe -s -o NUL -w "%{http_code}`n" -H "Authorization: Bearer $tok" http://127.0.0.1:8080/api/users
}
# 期望：约 5 个 200 + 15 个 429（不是每实例 5 个）
```

### 14.5 P3 验收

```powershell
# 1. 取票据
$t = (curl.exe -s -X POST -H "Authorization: Bearer $tok" http://127.0.0.1:8080/api/auth/sse-ticket | ConvertFrom-Json).ticket

# 2. 订阅（实例 A）
curl.exe -N "http://127.0.0.1:8080/api/events?ticket=$t"

# 3. 从实例 B 触发一次通知，A 应收到
curl.exe -s -X POST -H "Authorization: Bearer $tok" -H "Content-Type: application/json" `
  -d '{\"kind\":\"test\"}' http://127.0.0.1:8081/api/dev/notify

# 4. 票据只能用一次
curl.exe -i "http://127.0.0.1:8080/api/events?ticket=$t"    # 期望 401
```

### 14.6 P4 验收

```powershell
# 无反射时用 proto 直接调
grpcurl -plaintext -H "Authorization: Bearer $tok" `
  -d '{"channel":"default"}' 127.0.0.1:8080 assets.gateway.v1.EventGateway/GetSnapshot

# 健康
grpcurl -plaintext 127.0.0.1:8080 grpc.health.v1.Health/Check

# 无凭证应返回 Unauthenticated
grpcurl -plaintext -d '{"channel":"default"}' 127.0.0.1:8080 assets.gateway.v1.EventGateway/GetSnapshot
```

### 14.7 工作量与风险排序

| 项 | 工作量 | 风险 | 备注 |
|---|---|---|---|
| `build_router` 提取 | 小 | 低 | 纯重构，行为等价 |
| `authenticate` 提取 | 小 | 中 | 需保证 `with_schema` 时序不变 |
| Redis 认证缓存 | 中 | 中 | 失效正确性是难点 |
| Redis 限流 | 中 | 低 | 脚本简单，注意时钟源 |
| SSE 跨实例 | 大 | 高 | 双写、续传、背压三处易错 |
| gRPC 合并路由 | 中 | 中 | `into_axum_router` + 中间件顺序 |
| K8s/Ingress | 中 | 中 | gRPC 与 SSE 的注解冲突 |

---

## 15. 风险登记

| 编号 | 风险 | 影响 | 概率 | 缓解 |
|---|---|---|---|---|
| R1 | Redis 单点故障导致认证/限流全挂 | 全站 503 | 中 | Redis 主从 + Sentinel/Cluster；`RATE_LIMIT_FAIL_MODE` 可切 open 应急；认证侧保留短 TTL 本地缓存（降级窗口 5s） |
| R2 | `create_router` 提取破坏桌面端 | 桌面版启动失败 | 中 | 保持 `start_http_server_with` 签名不变；加集成测试对比路由表（`Router` 无法枚举，改为冒烟测 N 个端点） |
| R3 | SSE 事件被中间层攒批 | 前端无实时性 | 高 | Ingress 关 buffering + 响应头 `X-Accel-Buffering: no` + `Cache-Control: no-store` |
| R4 | SSE 连接数打满 Ingress | 新连接 502 | 中 | 单实例连接上限 + 单用户上限 + `worker_connections` 扩容 + 单独域名 |
| R5 | `Last-Event-ID` 超出 Stream 保留范围 | 续传缺数据 | 中 | `hello` 事件带 `gap:true`，前端做一次全量拉取 |
| R6 | 权限变更后旧实例仍放行 | 越权（最长 30s） | 中 | 版本号 + 广播；高敏感操作（封禁）在业务侧强制 `DEL` L2 + 查询 DB |
| R7 | Snowflake worker id 冲突 | 主键冲突、数据错乱 | 中 | 启动抢位自检 + 心跳续约；K8s 用 StatefulSet |
| R8 | 迁移期间多实例并发 DDL | 迁移失败 | 中 | `pg_advisory_xact_lock` |
| R9 | gRPC 错误被包成 HTTP 状态导致客户端 `Unknown` | 排障困难 | 高 | §6.8 的 `Status` 映射；集成测试覆盖 401/403/429 三种 |
| R10 | tonic 版本与 OTel 冲突 | 编译出两份 tonic，二进制膨胀 | 中 | 锁 `0.14.x`；CI 检查 `cargo tree -d` 中 tonic 只出现一次 |
| R11 | 优雅退出顺序错误 | 发布时零星 502 | 高 | `preStop sleep 8` + `/readyz` 先置 503 + `maxUnavailable: 0` |
| R12 | 缓存放大量主体导致 Redis 内存膨胀 | Redis OOM | 低 | 所有 key 必须带 TTL；L2 TTL ≤ 60s；监控 `used_memory` |

### 15.1 已确认的现状缺陷（需在实施中一并修复）

| 编号 | 位置 | 问题 | 修复阶段 |
|---|---|---|---|
| B1 | 全局（`utils::logging::init_tracing` 之后无 propagator） | 未设置 `TraceContextPropagator`，跨服务 trace 断链 | P5（或 P0 顺带） |
| B2 | `authz.rs:440-441` 注释 | 声明「单实例部署，进程内计数即可（不引入 Redis）」，与多实例目标矛盾 | P2 |
| B3 | `chat_routes.rs:113` | SSE 事件用预格式化字符串，缺 `event:` / `id:` | P3 |
| B4 | `auth.rs:420` 附近 | 回源失败返回 500（正确），但缺少结构化错误码 | P1 |
| B5 | `api/src/lib.rs:236` | `create_router` 私有，外部无法复用 | P0 |
| B6 | `api/src/health.rs:49` | `/healthz` 内含 DB 探测，k8s liveness 用它会在 DB 抖动时重启全部副本 | P0（新增 `/livez`） |
| B7 | `api/src/lib.rs:203` | upload / chat 子路由在 `start_http_server_with` 内就地构建，外部无法复用 | P0（并入 `build_router`） |
| B8 | `service/src/api_key_service.rs:534` | `invalidate_api_key` 只清本进程，API Key 吊销在其它实例最长 30s 仍生效 | P1（广播或关闭缓存） |

> 这些缺陷在原文档中已逐条核对过 file:line；实施时若行号漂移，以**符号名**（函数名/注释内容）定位。

---

## 16. 待决事项（需业务/运维确认）

| 编号 | 问题 | 选项 | 建议 | 影响 |
|---|---|---|---|---|
| Q1 | 是否需要真正的多租户 schema 隔离？ | ①单 schema + `tenant_id` 列；②`with_schema` 每请求切 search_path | 沿用现状 ② | 决定认证缓存 key 是否必须含 schema |
| Q2 | 客户端是否已支持 `event:` 命名事件？ | ①是；②只支持 `onmessage` | 若②，`chat/stream` 保留无名事件、`/api/events` 用命名事件 | 决定 P3 是否需要前端同步改造 |
| Q3 | 反向代理模式下上游如何信任网关？ | ①共享密钥头；②mTLS；③内网 ACL | ②（mTLS） | 决定网关是否要加客户端证书 |
| Q4 | 限流故障时是否允许放行？ | ①fail-closed；②fail-open | ①默认，②需业务书面确认 | 决定 `RATE_LIMIT_FAIL_MODE` 默认值 |
| Q5 | SSE 是否需要「离线消息持久化 + 上线补推」？ | ①仅 Stream 保留 1000 条；②落库并记录已读位点 | ①（先），②后续 | 决定是否新增表 |
| Q6 | gRPC 是否需要对外暴露？ | ①仅内网服务间；②对外（含 gRPC-Web） | ① | 决定 Ingress 与反射策略 |
| Q7 | worker id 上限 32 是否够？ | ①够（≤32 副本）；②需扩容 | ① | 若②需改 Snowflake 位分配（会影响存量 ID） |
| Q8 | 是否保留桌面端单机模式（无 Redis）？ | ①保留（内存实现）；②弃用 | ① | 决定 `AuthStore`/`RateLimiter` trait 是否必须双实现 |

---

## 附录 A：代码索引（file:line，已核对）

### A.1 现有代码

| 符号 | 位置 | 与本文关系 |
|---|---|---|
| `start_http_server` | `crates/api/src/lib.rs:51` | 桌面端入口，签名不变 |
| `start_http_server_with` | `crates/api/src/lib.rs:91` | 需拆出 `build_router`（§5.2） |
| upload / chat 子路由就地构建 | `crates/api/src/lib.rs:170-198` | 必须一起搬进 `build_router`（B7） |
| 顶层 `cors` / `TraceLayer` / `security_headers` | `crates/api/src/lib.rs:207-211` | `build_router` 不套这三层（§5.2 注 2） |
| `create_router` | `crates/api/src/lib.rs:236` | 改为 `pub fn build_router`（B5） |
| `/healthz` 注册 | `crates/api/src/lib.rs:241` | 网关不得重复注册（§10.4） |
| `spawn_cleanup_task` | `crates/api/src/lib.rs:557` | 拆出 `cleanup_once()`，由 leader 调用（§9.3） |
| `Claims` | `crates/api/src/auth.rs:26` | JWT 声明结构 |
| `UserContext` | `crates/api/src/auth.rs:47` | 用户上下文（schema/租户） |
| `PrincipalKind` | `crates/api/src/auth.rs:55` | `User` / `OAuth` / `Service` 三主体 |
| `AuthPrincipal` | `crates/api/src/auth.rs:71` | 需补 `Serialize`/`Deserialize`（§6.1） |
| `AuthPrincipal::allows` | `crates/api/src/auth.rs:97` | 通配匹配，gRPC 复用 |
| `Credential` | `crates/api/src/auth.rs:109` | 凭证枚举 |
| `invalidate_user_authority` | `crates/api/src/auth.rs:128` | L1 失效入口，需扩展到 L2 + 广播（§6.6） |
| `clear_user_authority_cache` | `crates/api/src/auth.rs:133` | 全清（测试用） |
| `user_authority` | `crates/api/src/auth.rs:138` | 权限回源，接 `AuthStore` |
| `parse_credential` | `crates/api/src/auth.rs:164` | 凭证前缀分流（§6.1） |
| `verify_token` | `crates/api/src/auth.rs:195` | HS256；网关加 JWKS 分支（§6.2） |
| `auth_middleware` | `crates/api/src/auth.rs:360` | 抽 `authenticate()`（§5.2） |
| 回源失败不降级 | `crates/api/src/auth.rs:420` | 设计原则 D8 的来源 |
| `AuthzMode` | `crates/api/src/authz.rs:47` | `enforce` / `warn`（§6.7） |
| `static_perms_for` | `crates/api/src/authz.rs:320` | 静态权限兜底表，加 gRPC 伪路径 |
| `is_auth_only_route` | `crates/api/src/authz.rs:330` | 只认证不鉴权的路由 |
| `required_perms` | `crates/api/src/authz.rs:344` | 权限点解析（静态 → DB） |
| `matches_scope` | `crates/api/src/authz.rs:416` | scope 通配匹配 |
| `is_service_forbidden` | `crates/api/src/authz.rs:427` | 服务主体禁访判定（§6.7） |
| `is_service_forbidden_route` | `crates/api/src/authz.rs:434` | 路由级版本 |
| `check_rate_limit` | `crates/api/src/authz.rs:443` | **现为进程内实现**，需注入 Redis 实现（§9.5 / B2） |
| `authz_middleware` | `crates/api/src/authz.rs:472` | 鉴权主流程 |
| `audit_route_coverage` | `crates/api/src/authz.rs:656` | 路由覆盖率审计，网关新增路由需纳入 |
| SSE 事件构造 | `crates/api/src/chat_routes.rs:113` | 需改为 `event()`/`id()`/`json_data()`（§7.2 / B3） |
| `healthz` | `crates/api/src/health.rs:49` | 含 `db_ping()`（B6） |
| `insecure_transport` | `crates/api/src/health.rs:32` | 明文 OAuth 告警位 |
| `ApiKeyAuth` | `crates/service/src/api_key_service.rs:61` | API Key 主体信息（§6.4） |
| `VERIFY_CACHE_TTL` | `crates/service/src/api_key_service.rs:30` | 30s，多实例需处理（B8） |
| `verify_cache` | `crates/service/src/api_key_service.rs:48` | 第二处进程内缓存（§6.6） |
| `last_used_throttle` | `crates/service/src/api_key_service.rs:54` | 仅节流 `last_used_at` 写入，不影响正确性 |
| `invalidate_api_key` | `crates/service/src/api_key_service.rs:534` | **只清本进程**，需加广播（B8） |
| `clear_verify_cache` | `crates/service/src/api_key_service.rs:540` | 同上 |
| `revoke_api_key` | `crates/service/src/api_key_service.rs:437` | 吊销后需清缓存 |
| `verify_api_key` | `crates/service/src/api_key_service.rs:466` | `ak_` 校验入口（30s 缓存） |
| `find_session` | `crates/service/src/session_service.rs:117` | Cookie 会话校验（§6.5） |
| workspace members | `apps/backend/src-tauri/Cargo.toml:21` | `crates/*` glob，新增 crate 自动纳入 |

### A.2 需新增的符号（本文定义）

| 符号 | 所在模块 | 章节 |
|---|---|---|
| `GatewayConfig::from_env` / `validate` | `gateway/src/config.rs` | §12 |
| `AppState::new` | `gateway/src/state.rs` | §5.4 |
| `build`（路由组装） | `gateway/src/router.rs` | §5.4 |
| `authenticate` | `api/src/auth.rs`（提取） | §5.2 / §6.1 |
| `AuthStore` trait + `MemoryAuthStore` / `RedisAuthStore` | `api` + `gateway` | §6.6 |
| `RateLimiter` trait + `RedisRateLimiter` | `api` + `gateway` | §9.5 |
| `SseHub::{subscribe, publish, replay}` | `gateway/src/http/sse.rs` | §7.3–7.5 |
| `issue_ticket` / `sse_auth` | `gateway/src/auth/ticket.rs` | §6.9 |
| `grpc_auth` / `grpc_status_response` | `gateway/src/middleware/grpc_auth.rs` | §6.8 |
| `leader_loop` | `gateway/src/background/leader.rs` | §9.3 |
| `livez` / `readyz` | `gateway/src/health.rs` | §10.4 |
| `signal` | `gateway/src/shutdown.rs` | §9.8 |

---

## 附录 B：常用命令

### B.1 构建与静态检查

```powershell
# 只编译网关
cargo build -p gateway --bin assets-gateway

# 检查 tonic 是否出现重复版本（R10）
cargo tree -d -p gateway | Select-String "tonic"

# 完整检查（与 CI 对齐）
cargo clippy -p gateway --all-targets -- -D warnings
cargo fmt --check
```

### B.2 本地多实例

```powershell
# 实例 1
$env:GATEWAY_PORT="8080"; $env:SNOWFLAKE_WORKER_ID="1"; $env:GATEWAY_INSTANCE_ID="gw-1"
Start-Process .\target\debug\assets-gateway.exe

# 实例 2
$env:GATEWAY_PORT="8081"; $env:SNOWFLAKE_WORKER_ID="2"; $env:GATEWAY_INSTANCE_ID="gw-2"
Start-Process .\target\debug\assets-gateway.exe

# 查看端口占用
Get-NetTCPConnection -LocalPort 8080,8081 -State Listen | Select-Object LocalPort,OwningProcess
```

### B.3 探针与冒烟

```powershell
curl.exe -s -o NUL -w "livez=%{http_code}`n"  http://127.0.0.1:8080/livez
curl.exe -s -o NUL -w "readyz=%{http_code}`n" http://127.0.0.1:8080/readyz
curl.exe -s -o NUL -w "healthz=%{http_code}`n" http://127.0.0.1:8080/healthz
curl.exe -s -o NUL -w "unauth=%{http_code}`n" http://127.0.0.1:8080/api/users   # 期望 401
```

### B.4 SSE 手工验证

```powershell
# 取票据
$tok = "<jwt>"
$t = (curl.exe -s -X POST -H "Authorization: Bearer $tok" `
      http://127.0.0.1:8080/api/auth/sse-ticket | ConvertFrom-Json).ticket

# 订阅（-N 关闭缓冲；Ctrl-C 结束）
curl.exe -N "http://127.0.0.1:8080/api/events?ticket=$t"

# 观察响应头是否含 no-store / X-Accel-Buffering
curl.exe -sS -D - -o NUL "http://127.0.0.1:8080/api/events?ticket=$t"

# 票据复用应 401
curl.exe -s -o NUL -w "%{http_code}`n" "http://127.0.0.1:8080/api/events?ticket=$t"
```

### B.5 gRPC 验证

```powershell
# 有反射（非 prod）
grpcurl -plaintext 127.0.0.1:8080 list
grpcurl -plaintext 127.0.0.1:8080 describe assets.gateway.v1.EventGateway

# 健康检查
grpcurl -plaintext 127.0.0.1:8080 grpc.health.v1.Health/Check

# 带凭证调用
grpcurl -plaintext -H "Authorization: Bearer $tok" `
        -d '{\"channel\":\"default\"}' `
        127.0.0.1:8080 assets.gateway.v1.EventGateway/GetSnapshot

# 服务端流（10s 后 Ctrl-C）
grpcurl -plaintext -H "Authorization: Bearer $tok" `
        -d '{\"channel\":\"default\"}' `
        127.0.0.1:8080 assets.gateway.v1.EventGateway/Watch
```

### B.6 Redis 运维命令

```powershell
# 认证缓存命中
redis-cli --scan --pattern "gw:v1:auth:*" | Select-Object -First 10
redis-cli TTL "gw:v1:auth:jwt:<sha256>"

# 版本号
redis-cli GET "gw:v1:user:1:ver"
redis-cli INCR "gw:v1:user:1:ver"          # 手动失效某用户
redis-cli PUBLISH "gw:v1:invalidate" "user:1"

# 限流窗口
redis-cli ZCARD "gw:v1:rl:p:u:1"
redis-cli ZRANGE "gw:v1:rl:p:u:1" 0 -1 WITHSCORES

# SSE Stream
redis-cli XLEN "gw:v1:sse:stream:default"
redis-cli XRANGE "gw:v1:sse:stream:default" - + COUNT 5

# 选主
redis-cli GET "gw:v1:snowflake:w:1"
```

### B.7 数据库一致性检查

```sql
-- 迁移锁占用（应无残留）
SELECT l.locktype, l.objid, a.pid, a.state, a.query
FROM pg_locks l JOIN pg_stat_activity a USING (pid)
WHERE l.locktype = 'advisory';

-- 清理 leader
SELECT a.pid, a.state, a.query
FROM pg_locks l JOIN pg_stat_activity a USING (pid)
WHERE l.locktype = 'advisory' AND a.query ILIKE '%advisory%';

-- gRPC 伪路径权限点是否已入库
SELECT id, name, perms, http_path FROM sys_menu WHERE http_path LIKE 'grpc:%';
```

### B.8 压测（SSE 与 RPS）

```powershell
# 1) HTTP RPS：优先用仓库既有工具；无则用 curl 并发脚本粗略估算
1..50 | ForEach-Object -Parallel {
  curl.exe -s -o NUL -w "%{http_code}`n" http://127.0.0.1:8080/api/users
} -ThrottleLimit 50

# 2) SSE 并发：开 N 个长连，观察 gw_sse_connections
1..500 | ForEach-Object -Parallel {
  curl.exe -N -s -o NUL "http://127.0.0.1:8080/api/events?ticket=$using:t"
} -ThrottleLimit 500
```

> 正式的 RPS / 长连压测建议用 k6（`sse` 脚本）或 `vegeta`，本仓库未内置；上面的 curl 并发只用于**功能性**验证，不能作为容量结论。

---

## 附录 C：参考与变更记录

### C.1 仓库内相关文档

| 文档 | 与本文的关系 |
|---|---|
| `docs/OAuth2.1与API Key统一鉴权设计方案.md` | 三类主体（用户 / OAuth / API Key）与「一条管线」原则的来源 |
| `docs/多租户数据权限与租户切换设计方案.md` | `with_schema` / `schema_prefix()` 语义 |
| `docs/多租户超级管理员跨Schema查询设计方案.md` | 跨 schema 查询的边界，影响认证缓存 key |
| `docs/桌面与Web双形态部署指南.md` | 现有 `assets-server` 的部署形态，网关需与其共存 |
| `docs/doc-parser 独立部署指南.md` | `hybrid` 模式中转发的目标服务 |
| `docs/架构问题与缺陷分析及整改排期.md` | 与本方案 B1–B7 缺陷清单需对齐排期 |
| `docs/前端大整数精度丢失问题(Snowflake ID).md` | 解释为什么 `sub` 与事件 `id` 都用字符串传输 |
| `docs/大文件分片上传与断点续传设计.md` | 上传路由的 body 限制与 `GATEWAY_MAX_BODY_MB` 取值依据 |
| `docs/API-Swagger规范.md` | `extra_routes`（Swagger）与 `build_router` 合并顺序 |

### C.2 外部参考

| 主题 | 参考 |
|---|---|
| axum 0.8 SSE | `axum::response::sse::{Sse, Event, KeepAlive}` |
| tonic 0.14 与 axum 合并 | `tonic::service::Routes::into_axum_router()` |
| tonic-web | `tonic_web::enable(Routes)`，`application/grpc-web+proto` |
| protoc 分发 | `protoc-bin-vendored::protoc_bin_path()` |
| RFC 7662 | OAuth 2.0 Token Introspection |
| RFC 6750 | Bearer Token 使用与 `WWW-Authenticate` 语义 |
| WHATWG HTML | Server-Sent Events（`event:` / `id:` / `retry:` / `Last-Event-ID`） |
| OpenTelemetry | `TraceContextPropagator`、`OTEL_TRACES_SAMPLER_ARG` |
| PostgreSQL | `pg_advisory_lock` / `pg_advisory_xact_lock` / `pg_try_advisory_lock` |
| Redis | `XADD`/`XRANGE`/`XTRIM`、`PUBLISH`/`SUBSCRIBE`、`GETDEL`、`EVALSHA` |

### C.3 术语表

| 术语 | 含义 |
|---|---|
| L1 / L2 | 进程内缓存（`DashMap`）/ 共享缓存（Redis；附录 D 模式下取消 L2） |
| 伪路径 | 把 gRPC 方法映射成 `grpc:/pkg.Service/Method`，复用 HTTP 权限表 |
| 票据（ticket） | `GETDEL` 一次性消费的短时凭证，专供 `EventSource` |
| 选主 | 通过 `pg_try_advisory_lock` 保证同一时刻只有一个清理执行者 |
| 排空（drain） | 退出前等待在途请求完成的过程 |
| gap | 续传时发现历史被修剪，需要前端全量重拉的标记 |

### C.4 变更记录

| 版本 | 日期 | 变更 |
|---|---|---|
| v1.0 | 2026-11-11 | 首版：16 章 + 3 附录。确立单端口（merged gRPC）架构、L1+L2+版本号认证缓存、Redis 全局限流、SSE Stream+Pub/Sub 双写、`pg_advisory_lock` 串行化 |
| v1.1 | 2026-11-11 | 修正 `/healthz` 语义（现有实现含 DB 探测，新增 `/livez`）；补充 `build_router` 必须包含 upload/chat 子路由与不得重复注册 `/healthz`；新增 B8（`api_key_service::verify_cache` 不跨实例）；B1–B8 缺陷清单定稿；P0–P5 阶段与人日对齐 |
| v1.2 | 2026-11-11 | 新增附录 D：不依赖 Redis 的 PostgreSQL 单一依赖模式（LISTEN/NOTIFY + 4 张 sys_gateway_* 表 + advisory lock）；含 10 张跨实例流程图、协调表 DDL、故障自愈矩阵与验收清单；版本号、术语表、一页总结同步更新 |

### C.5 待补充（下一版）

| 项 | 说明 |
|---|---|
| SSE 前端改造清单 | `EventSource` → 命名事件监听的迁移步骤（依赖 Q2 结论） |
| mTLS 双向认证细节 | 反向代理模式下网关到上游的证书签发与轮换（依赖 Q3） |
| 离线消息持久化表设计 | 若 Q5 选②，需补 `sse_message` / `sse_offset` 表结构 |
| 灰度与流量镜像 | 按 `tenant_id` 哈希的灰度策略 |
| 容量压测报告 | P5 产出，届时回填 §13.5 的经验值 |

---

## 附录 D：PostgreSQL 单一依赖模式（不引入 Redis）

> **适用条件**：部署环境不允许新增 Redis（运维限制、等保/合规要求、客户环境只提供 PostgreSQL）。
> **结论**：正文 §6 / §7 / §9 中由 Redis 承担的职责（认证 L2 缓存与失效广播、限流、SSE 广播、票据、worker 抢位）全部由 PostgreSQL 承接。**进程模型、路由、鉴权管线、阶段划分与人日不变**，只把「共享状态落在哪里」从 Redis 换成本就强依赖的 PG。
> **代价**见 D.12：热点路径多一次 DB 往返、SSE 每个事件一次 INSERT；换来少一个中间件、少一个故障域。

### D.0 职责映射（Redis → PostgreSQL）

| 正文中的 Redis 用法 | 本附录替代 | 一致性说明 |
|---|---|---|
| `PUBLISH gw:v1:invalidate` 认证失效广播 | `NOTIFY assets_gw '{"k":"authz","u":42}'` | 等价（毫秒级）；漏通知最坏 = L1 TTL 60s，与改造前单实例行为相同 |
| `gw:v1:auth:authz:u:<id>:v<ver>` L2 快照 + 版本号 | **取消 L2**：PG 本就是唯一真源，只需广播失效 | 少一层缓存、少一处不一致 |
| `PUBLISH gw:sse:<tenant>` + `XADD/XRANGE` Stream | `sys_gateway_event` 日志表 + `NOTIFY`（载荷只带 `id`） | 更好：`BIGSERIAL` 全局单调，天然就是 `Last-Event-ID` 游标 |
| `SET ... NX EX` 一次性票据 | `sys_gateway_sse_ticket` + `DELETE ... RETURNING` | 等价，且跨重启、跨实例有效 |
| `ZSET + Lua`（`EVALSHA`）固定窗口限流 | `sys_gateway_rate_limit` + 单语句 UPSERT | 等价（单语句原子，跨实例总额准确） |
| `SET ... NX PX` worker id 抢位与续约 | `sys_gateway_instance` 租约表 | 等价；崩溃后租约到期可被接管 |
| PubSub 连接管理与重连 | 无（`PgListener` 自带连接，重连由网关侧指数退避） | 少一个客户端依赖 |

> 依赖面收益：不新增 Redis 客户端、不需要 Redis 高可用、不引入第二套监控项；唯一中间件仍是 PostgreSQL——**没有新增故障域**。

### D.1 拓扑与四个共享面（图 1）

```
     浏览器 / 桌面 WebView / 第三方
                 │  HTTPS
                 ▼
        ┌─────────────────┐
        │ nginx / k8s LB  │   SSE：关闭 proxy_buffering、read_timeout ≥ 1h
        └───┬─────────┬───┘
            ▼         ▼
 ┌────────────────┐  ┌────────────────┐
 │ assets-gateway │  │ assets-gateway │   无状态，可水平扩
 │ #1  :8080      │  │ #2  :8081      │
 │ worker_id=1    │  │ worker_id=2    │
 │ ├ api::build_router（auth / authz / chat / upload）
 │ ├ SseHub       │  │ ├ SseHub       │   本地连接表
 │ ├ L1 权限缓存  │  │ ├ L1 权限缓存  │   60s
 │ ├ PgListener ──┼──┼─┼─ PgListener  │   LISTEN assets_gw
 │ └ leader 锁    │  │ └ (standby)    │   pg_try_advisory_lock
 └───────┬────────┘  └───────┬────────┘
         │                   │
         └─────────┬─────────┘
                   ▼
        ┌──────────────────────────────┐
        │ PostgreSQL（唯一硬依赖）      │
        │  LISTEN/NOTIFY  channel=assets_gw
        │  sys_gateway_instance         │  worker id 租约
        │  sys_gateway_sse_ticket       │  一次性票据
        │  sys_gateway_event            │  SSE 事件日志 / 续传
        │  sys_gateway_rate_limit       │  共享限流计数
        │  advisory locks               │  迁移互斥 / 清理选主
        └──────────────────────────────┘
```

| # | 共享面 | 用途 | 一致性模型 |
|---|---|---|---|
| ① | `NOTIFY assets_gw` | 唤醒（不传正文）：认证失效、新 SSE 事件 | 尽力而为，可丢 → 靠 ② 与客户端游标兜底 |
| ② | `sys_gateway_event` | SSE 正文 + `Last-Event-ID` 续传 | 强一致（表） |
| ③ | `sys_gateway_instance` / `sse_ticket` / `rate_limit` | 租约、一次性票据、共享计数 | 强一致（原子 SQL） |
| ④ | `advisory locks` | 迁移互斥、清理选主 | 强一致（会话级） |

### D.2 四张协调表与迁移串行化

新增 `crates/database/src/sql/gateway_migration.sql`（全部 `IF NOT EXISTS`；`execute_sql_content` 走单事务，**不能用 `CREATE INDEX CONCURRENTLY`**）：

```sql
CREATE TABLE IF NOT EXISTS public.sys_gateway_instance (
    worker_id   SMALLINT PRIMARY KEY CHECK (worker_id BETWEEN 0 AND 31),
    instance_id TEXT NOT NULL,
    expires_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS public.sys_gateway_sse_ticket (
    token      TEXT PRIMARY KEY,
    tenant_id  BIGINT,
    principal  JSONB NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_gw_sse_ticket_exp ON public.sys_gateway_sse_ticket (expires_at);

CREATE TABLE IF NOT EXISTS public.sys_gateway_event (
    id         BIGSERIAL PRIMARY KEY,          -- 全局单调，直接当 Last-Event-ID 游标
    tenant_id  BIGINT NOT NULL,
    kind       VARCHAR(32) NOT NULL,
    payload    JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_gw_event_tenant_id ON public.sys_gateway_event (tenant_id, id);
CREATE INDEX IF NOT EXISTS idx_gw_event_created  ON public.sys_gateway_event (created_at);

CREATE TABLE IF NOT EXISTS public.sys_gateway_rate_limit (
    bucket_key   TEXT NOT NULL,
    window_start TIMESTAMPTZ NOT NULL,
    counter      INTEGER NOT NULL,
    PRIMARY KEY (bucket_key, window_start)
);
CREATE INDEX IF NOT EXISTS idx_gw_rate_limit_window ON public.sys_gateway_rate_limit (window_start);
```

`crates/database/src/postgres.rs`：

- 新增 `pub async fn migrate_gateway_tables(pool: &PgPool)`（`include_str!("sql/gateway_migration.sql")` + `execute_sql_content`），在 `init_postgres_database` 执行完 public 迁移脚本之后用 `run_init_step` 调用。
- 新增 `pub const MIGRATION_LOCK_KEY: i64`；`init_postgres_database` 内 `pool.acquire()` → `detach()` 拿独占连接，`SET lock_timeout TO '300s'` → `SELECT pg_advisory_lock($1)` → 执行原有步骤 → unlock → close。用 detach 而不是事务级锁：现有初始化函数全部是 `&PgPool` 多语句 DDL；连接在提前返回或 panic 时 drop 即自动释放，不会把锁带回连接池。

**图 2 · 多实例并发启动**

```
实例#1                                PostgreSQL                          实例#2
  │ init_database_and_schema()                                          │
  │ pool.acquire() → detach()                                           │
  ├─ pg_advisory_lock(MIGRATION_LOCK_KEY) ──────────►│                  │
  │                                                  │◄── 请求同一把锁 ─┤
  │ create schema / tables / migrations（幂等）      │   （阻塞，        │
  │                                                  │ lock_timeout=300s）│
  ├─ pg_advisory_unlock + conn.close() ────────────►│                  │
  │                                                  ├── 获锁 → 幂等 DDL ┤
  │                                                  │   → unlock        │
  │ upsert sys_gateway_instance                      │                  │
  │  (worker_id=1, expires=now()+60s) ──────────────►│                  │
  │                                                  │◄── upsert worker=2 ┤
  │ RETURNING 1 行 → 抢位成功                        │    RETURNING 1 行 │
  │ PgListener.listen(assets_gw) ───────────────────►│◄── listen ────────┤
  │ axum::serve(:8080)                               │   axum::serve(:8081)
  ▼                                                  │                  ▼

冲突分支：ON CONFLICT ... WHERE 条件不满足 → RETURNING 无行
         → 日志「worker id 已被 gw-1 占用」→ 退出码 != 0（不带病启动）
```

worker 租约 SQL（抢位 / 续约共用）：

```sql
INSERT INTO public.sys_gateway_instance (worker_id, instance_id, expires_at)
VALUES ($1, $2, now() + make_interval(secs => $3))
ON CONFLICT (worker_id) DO UPDATE
   SET instance_id = EXCLUDED.instance_id,
       expires_at  = EXCLUDED.expires_at,
       updated_at  = now()
 WHERE public.sys_gateway_instance.instance_id = EXCLUDED.instance_id
    OR public.sys_gateway_instance.expires_at < now()
RETURNING instance_id;
```

### D.3 认证与权限失效（图 3）

```
Admin ──► 实例#1  PUT /api/users/42（改角色）
            │ 1) UPDATE sys_user_role …（提交）
            │ 2) api::auth::invalidate_user_authority(42)
            │      ├─▶ clear_local_user_authority(42)        ← 本实例 L1 立即失效
            │      └─▶ AuthInvalidator::on_invalidate_user(42)
            │              └─ spawn { SELECT pg_notify('assets_gw',
            │                          '{"k":"authz","u":42}') }
            ▼
        PostgreSQL ── NOTIFY 广播给该通道所有 LISTEN 会话 ──┐
            ├──────────────► 实例#1 listener（自己也会收到，幂等清空）
            └──────────────► 实例#2 listener → clear_local_user_authority(42)

用户下一次请求（任意实例）：
  L1 hit(<60s)? ──yes──► 用旧快照（仅通知丢失且 <60s 时发生）
        │no
        ▼
  service::load_user_authority(42) → 新快照 → 写 L1 → authz 用新权限
```

注入点（`api` / `service` 各一处，签名保持向后兼容）：

```rust
// crates/api/src/auth.rs
pub trait AuthInvalidator: Send + Sync {
    fn on_invalidate_user(&self, user_id: i64);
    fn on_clear_all(&self);
}
pub fn install_auth_invalidator(i: Arc<dyn AuthInvalidator>);

pub fn invalidate_user_authority(user_id: i64) {   // 签名不变，现有调用点不用改
    clear_local_user_authority(user_id);           // 新增：只清本地，供 listener 调用（防回环）
    if let Some(i) = auth_invalidator() { i.on_invalidate_user(user_id); }
}

// crates/service/src/api_key_service.rs —— 修 B8（verify_cache 30s 跨实例窗口）
pub fn install_api_key_invalidator(f: Arc<dyn Fn(i64) + Send + Sync>);
```

**图 4 · 单请求鉴权路径**

```
请求 ─► request_id ─► security_headers ─► TraceLayer ─► api::auth_middleware
                                                          │ authenticate(&mut req)
                                                          │   ├ ak_* → verify_api_key（30s 进程内缓存）
                                                          │   ├ at_* → oauth token 缓存
                                                          │   └ JWT  → verify_token → user_authority(u)
                                                          │            ├ L1 hit(<60s) → 直接用
                                                          │            └ miss → SELECT 回源 → 写 L1
                                                          ├ 注入 UserContext / AuthPrincipal
                                                          └ database::with_schema(principal.schema)
                                                                │
                                                          authz_middleware（权限点 / AUTH_ONLY / 拒绝服务身份）
                                                                │
                                                          RateLimiter::check()
                                                                ├ 默认：进程内固定窗口（仅 API Key）
                                                                └ GATEWAY_RATE_LIMIT_ENFORCE_GLOBAL=1 → PG 共享计数
                                                                │
                                                          handler → service → 租户 schema
```

> 关键点：**DB 是唯一真源**，缓存只做加速；通知丢失的最坏后果是「最多 60s 旧权限」，与改造前单实例行为一致。

### D.4 跨实例限流（图 5）

```sql
-- 固定窗口共享计数（单语句原子）
INSERT INTO public.sys_gateway_rate_limit AS rl (bucket_key, window_start, counter)
VALUES ($1, to_timestamp(floor(extract(epoch FROM now()) / $2::int) * $2::int), 1)
ON CONFLICT (bucket_key, window_start) DO UPDATE SET counter = rl.counter + 1
RETURNING counter;
-- bucket_key 形如 p:u:42|r:/api/assets/hardware，维度 g/r/t/p 见正文 §9
-- window_start 取 DB 时钟，天然免疫实例间时钟漂移
```

```
t=0.00s  客户端 ─► 实例#1 ─► UPSERT → c=1 → 200
t=0.05s  客户端 ─► 实例#2 ─► UPSERT → c=2 → 200
t=0.10s  客户端 ─► 实例#1 ─► c=3 → 200
t=0.15s  客户端 ─► 实例#2 ─► c=4 → 200
t=0.20s  客户端 ─► 实例#1 ─► c=5 → 200          (limit=5)
t=0.25s  客户端 ─► 实例#2 ─► c=6 → 429 + Retry-After: 59
t=60.0s  新窗口 → 新行 → c=1 → 200

故障：DB 不可用 → RATE_LIMIT_FAIL_MODE=closed（缺省）返回 429/503；=open 放行并告警
清理：leader 执行 DELETE FROM public.sys_gateway_rate_limit
                       WHERE window_start < now() - interval '10 minutes'
```

### D.5 SSE 建连：票据 + 一次性消费 + 回放（图 6）

```
① 取票（可带 Header，走普通认证）
 浏览器 ── POST /api/auth/sse-ticket (Authorization: Bearer <JWT>) ──► 实例#1
         ◄── 200 {"ticket":"9f3c…","expires_in":30}
              INSERT INTO public.sys_gateway_sse_ticket(token, tenant_id, principal, expires_at)
              VALUES ($1, $2, $3, now() + interval '30 seconds')
             （API Key / OAuth 服务主体 → 403：SSE 只服务用户态）

② 建流（EventSource 不能带 Header，只能 query 传票）
 浏览器 ── GET /api/events?ticket=9f3c…  (Last-Event-ID: 1234) ──► 实例#2
         │ sse_auth 中间件：
         │   DELETE FROM public.sys_gateway_sse_ticket
         │    WHERE token = $1 AND expires_at > now()
         │    RETURNING principal, tenant_id        ← 原子、一次性、跨实例可用
         │   （无效/已用/过期 → 401；租户不匹配 → 403）
         │   database::with_schema(principal.schema) + authz（GET /api/events ∈ AUTH_ONLY）
         │   SseHub::register(conn_id, user, tenant) ← 每实例/每用户连接上限
         │ 回放：SELECT id, kind, payload FROM public.sys_gateway_event
         │        WHERE tenant_id = $1 AND id > $2 ORDER BY id LIMIT 200
         │       若首行 id > $2 + 1 → 事件已被 retention 清理 → hello.gap = true
         ▼
         ◄── event: hello   data: {"cursor":1240,"gap":false}
         ◄── （回放 1235 … 1240）
         ◄── 实时模式（图 7）；每 15s 发送 `:keep-alive` 注释帧
```

### D.6 SSE 跨实例分发（图 7）

```sql
-- 发布：写日志 + 广播，单语句完成（同一事务，commit 时投递）
WITH ins AS (
  INSERT INTO public.sys_gateway_event (tenant_id, kind, payload)
  VALUES ($1, $2, $3) RETURNING id, tenant_id
)
SELECT id, pg_notify('assets_gw',
       json_build_object('k', 'sse', 't', tenant_id, 'id', id)::text) FROM ins;
-- 载荷只有 id（远小于 NOTIFY 8KB 上限），正文由订阅方回表读取
```

```
业务代码（任意实例）publish(tenant=1, kind="asset.changed", payload={…})
        │ 单语句：INSERT + pg_notify
        ▼
   PostgreSQL NOTIFY
        ├────────────────────────┬─────────────────────────┐
        ▼                        ▼                         ▼
  实例#1 listener          实例#2 listener           实例#3 listener
  （发布者也在此收到，不做本地直发 → 不会重复推送）
        │
        └─ SELECT kind, payload FROM public.sys_gateway_event WHERE id = 1241
                              ▼
                     SseHub.broadcast(tenant=1, event)   ← 只推 tenant_id == 1 的连接
                              ├─ try_send 成功 → 写入该连接流
                              └─ 通道满 → 丢最旧 + gw_sse_dropped_total++
```

> 为什么不把 payload 塞进 NOTIFY：8KB 上限 + 丢失不可恢复。只发 `id`、正文回表读，天然支持回放与漏通知补齐。

### D.7 断线续传（图 8）

```
浏览器（已知 last_event_id = 1240）
   │ 网络中断 / 实例#1 被 LB 摘除
   ▼
重连 ─► GET /api/events?ticket=<新票>   Header: Last-Event-ID: 1240 ──► 实例#2
        │ SELECT id, kind, payload FROM public.sys_gateway_event
        │  WHERE tenant_id = 1 AND id > 1240 ORDER BY id ASC LIMIT 200
        ├─ 命中 1241,1242,1243 → 逐条推送（event id 与原事件一致）→ 进入实时模式
        ├─ 最小 id = 1250（1241..1249 已被 retention 删除）→ hello {"cursor":1250,"gap":true}
        └─ 一条都没有 → hello {"cursor":1240,"gap":false}
```

### D.8 优雅退出与清理选主（图 9）

```
实例#1 收到 SIGTERM
  1. cancel.cancel()
  2. Hub：对所有连接 send  event: shutdown  data:{"reason":"restart","retry_after_ms":1000}
  3. 各 SSE 流 return → 连接关闭（客户端按 retry_after 重连到实例#2）
  4. axum::serve(...).with_graceful_shutdown(cancel.cancelled()) → 等在途请求（≤ 25s）
  5. 若本实例是 leader：pg_advisory_unlock(CLEANUP_LOCK_KEY) + 释放 detach 连接
  6. DELETE FROM public.sys_gateway_instance WHERE worker_id = 1 AND instance_id = 'gw-1'
  7. 关闭 PgListener → 正常退出（30s 兜底强杀）

实例#2 的 leader 循环（每 10s 一次）：
   pg_try_advisory_lock(CLEANUP_LOCK_KEY)
     ├─ true  → leader：执行清理 + api::cleanup_once()（每 CLEANUP_INTERVAL_SECS）
     │          └─ SELECT 1 保活；连接断开 → 锁自动释放 → 回到 try 状态
     └─ false → standby，睡 10s

清理内容（leader 独占）：
   · DELETE FROM public.sys_gateway_event      WHERE created_at  < now() - interval '1 hour'
   · DELETE FROM public.sys_gateway_sse_ticket WHERE expires_at  < now()
   · DELETE FROM public.sys_gateway_rate_limit WHERE window_start < now() - interval '10 minutes'
   · DELETE FROM public.sys_gateway_instance   WHERE expires_at  < now() - interval '5 minutes'
```

### D.9 故障自愈矩阵（图 10）

| 场景 | 现象 | 自愈机制 | 时限 |
|---|---|---|---|
| 实例进程崩溃（未释放锁） | LB 探活失败 | advisory lock 随会话断开由 PG 自动释放；worker 租约到期 | 锁：立即；租约：≤ 60s |
| worker 续约失败（网络分区） | 可能与旧实例撞 id | 续约 UPSERT 无返回行 → 记 FATAL → 主动退出，避免双写 | ≤ 20s |
| PgListener 断线 | 本实例收不到跨实例事件与失效通知 | 指数退避重连（1s→30s）；期间 /readyz=503 由 LB 摘除；SSE 靠 Last-Event-ID 补齐 | 重连 ≤ 30s |
| PG 短暂重启 | 全部实例 5xx | sqlx 池自动重连；listener 重建；leader 重新抢锁；/readyz 反映真实状态 | 恢复后 ≤ 30s |
| NOTIFY 丢失（罕见） | 个别 SSE 事件未推、个别权限未即时失效 | SSE：重连带游标补齐；权限：L1 TTL 60s 后回源 | ≤ 60s |
| 事件表 / 限流表膨胀 | 存储增长 | leader 按 SSE_EVENT_RETENTION_SECS 与窗口淘汰 | 每小时 |
| 时钟漂移 | 窗口 / 租约错乱 | 一律用 DB 的 now() 与 extract(epoch FROM now()) | 无 |
| 多实例同时启动 | 迁移并发 | pg_advisory_lock(MIGRATION_LOCK_KEY) 串行化 | 第二个实例等待 ≤ 300s |

### D.10 一图串联

```
                  ┌──────────── 4 个共享面 ────────────┐
请求 / 连接 ─┬─► ① NOTIFY 事件总线（唤醒，不传正文）
             ├─► ② sys_gateway_event（SSE 正文 + Last-Event-ID 续传）
             ├─► ③ sys_gateway_*（票据 / worker 租约 / 限流计数）
             └─► ④ advisory locks（迁移互斥 / 清理选主）
                          │
        ┌─────────────────┴──────────────────┐
        │ 强一致（DB 直读 / 原子写）：租约、票据、限流计数、续传
        │ 最终一致（NOTIFY 广播）：L1 权限失效、在线事件推送
        │ 兜底：L1 TTL 60s、Last-Event-ID 回放、LB 探针
        └────────────────────────────────────┘
```

### D.11 环境变量（替换正文 §12.2 中的 REDIS_* 项）

| 变量 | 缺省 | 说明 |
|---|---|---|
| `GATEWAY_PG_CHANNEL` | `assets_gw` | LISTEN/NOTIFY 通道名；多套环境共库时必须区分 |
| `SNOWFLAKE_WORKER_ID` | `0` | 0..=31；由租约表校验唯一性 |
| `GATEWAY_WORKER_LEASE_SECS` | `60` | 租约时长；续约间隔 = 1/3 |
| `SSE_EVENT_RETENTION_SECS` | `3600` | 事件表保留时长（决定可续传窗口） |
| `SSE_TICKET_TTL_SECS` | `30` | 票据有效期 |
| `SSE_REPLAY_MAX_EVENTS` | `200` | 单次回放上限，超出置 gap = true |
| `GATEWAY_RATE_LIMIT_ENFORCE_GLOBAL` | `false` | 关闭时沿用进程内「仅 API Key」限流，不产生限流 DB 写 |
| `RATE_LIMIT_FAIL_MODE` | `closed` | DB 故障时 closed = 拒绝 / open = 放行 |
| `CLEANUP_INTERVAL_SECS` | `3600` | leader 执行清理的间隔 |
| `AUTHZ_CACHE_TTL_SECS` | `60` | L1 权限快照 TTL（与现状一致，不放大不一致窗口） |

> 同时删除正文 §12 中 `REDIS_URL` / `GATEWAY_REQUIRE_REDIS` 的必填校验；`/readyz` 改为「DB `SELECT 1`（2s 超时）+ PgListener 健康标志 + 未收到 shutdown」。

### D.12 代价与边界

| 项 | 影响 | 缓解 |
|---|---|---|
| 限流每次请求 1 次 DB 写 | 比 Redis 多 1 个 RTT，占用 PG 写容量 | 缺省**关闭**全局维度（沿用进程内 API Key 限流）；开启时只按 p/r 维度生效 |
| SSE 每事件 1 次 INSERT | 比 PUBLISH 重，高吞吐时写压力上升 | `SSE_EVENT_RETENTION_SECS` 控制保留；必要时用 `GATEWAY_SSE_PERSIST=0` 退化为「仅本实例分发、无续传」 |
| NOTIFY 不持久，重连窗口可能丢 | 重连瞬间的事件不主动推 | 客户端带 `Last-Event-ID` 补齐；`hello.gap=true` 提示缺口 |
| NOTIFY 载荷 8KB 上限 | 大事件不能内联 | 只发 `id`，正文回表读（本设计已如此） |
| 每实例多占 PG 连接 | listener 1 条 + leader 1 条（迁移锁仅启动期） | 按「实例数 × 2」预留连接池上限 |
| 多实例共用同一 PG | PG 不可用则全部不可用 | 本就是唯一硬依赖，无新增故障域 |
| 无 L2 缓存 | 每次 L1 miss 都回源 DB | 现状即如此（60s TTL），未变差 |

### D.13 与正文各章的对应关系

| 正文 | 本附录的替换 |
|---|---|
| §6.6 / §6.8 认证 L2、版本号、失效广播 | D.3（取消 L2，只保留 L1 + NOTIFY 失效） |
| §7 SSE Streams + PubSub 双写 | D.5 – D.7（事件表 + NOTIFY + Last-Event-ID） |
| §9.2 限流 Redis ZSET/Lua | D.4（PG 固定窗口 UPSERT） |
| §9.5 worker 抢位 | D.2（租约表） |
| §9.6 清理选主 | D.8（不变，仍是 advisory lock） |
| §12.2 `REDIS_*` 配置与 prod 必填校验 | D.11（整段删除） |
| §15 R12（Redis 内存膨胀） | 不再适用，改为「事件表 / 限流表膨胀」→ D.9 |

### D.14 验收（无 Redis，只需 PG）

```bash
cargo check -p server -p api -p database -p service      # 现有三形态回归
cargo build -p gateway --bin assets-gateway
cargo test  -p gateway -p api
```

1. `/livez` 200、`/readyz` 200、`/healthz` 200、`/api/users/me` 无凭证 401
2. 双实例 `GATEWAY_PORT=8080/8081` + `SNOWFLAKE_WORKER_ID=1/2`：同 token 权限一致；第三实例用 `SNOWFLAKE_WORKER_ID=1` 启动失败（日志含「已被占用」）
3. 实例 A 改角色 → 实例 B 下一次请求立即生效（NOTIFY 生效；`SELECT` 确认 L1 已清）
4. `GATEWAY_RATE_LIMIT_ENFORCE_GLOBAL=1` + 5 rps：跨实例 20 次请求 ≈ 5 × 200 + 15 × 429（带 `Retry-After`）
5. 取票据 → 连 A → 从 B 发布（`GATEWAY_DEV_ENDPOINTS=1`）→ A 收到；票据二次使用 401；断线带 `Last-Event-ID` 重连补齐；`SIGTERM` 后 30s 内退出
6. 四张 `sys_gateway_*` 表在启动后自动创建（覆盖老库升级路径）；`pg_locks` 中 `locktype = advisory` 的 leader 只有一行

验证 SQL：

```sql
SELECT * FROM public.sys_gateway_instance ORDER BY worker_id;
SELECT count(*) FROM public.sys_gateway_event WHERE created_at > now() - interval '10 minutes';
SELECT bucket_key, counter FROM public.sys_gateway_rate_limit ORDER BY window_start DESC LIMIT 20;
SELECT pid, objid, mode FROM pg_locks WHERE locktype = 'advisory';
```

---

## 一页总结

- **形态**：新增 `crates/gateway`（bin `assets-gateway`），单端口 8080 承载 HTTP + SSE + h2c gRPC，默认内嵌 `api::build_router`。
- **改动面**：`api` crate 只做「提取 + 注入点」，`start_http_server_with` 行为不变，桌面端零影响。
- **多实例三件事**：认证缓存靠 L2+版本号、限流靠 Redis Lua、清理任务靠 advisory lock 选主。
- **已确认的 8 个现状缺陷**：B1 trace propagator、B2 单实例限流注释、B3 SSE 事件格式、B4 错误码、B5 `build_router`、B6 liveness 查 DB、B7 upload/chat 路由复用、B8 API Key 缓存不跨实例（清单见 §15.1，其中 B3/B5/B6/B7 在 P0 修，B4/B8 在 P1，B2 在 P2，B1 在 P5）。
- **上线门槛**：`/livez` 与 `/readyz` 分离、SSE buffering 关闭、优雅退出顺序正确、worker id 唯一。
- **零中间件可选项**：环境不允许 Redis 时，按附录 D 用 PostgreSQL（LISTEN/NOTIFY + sys_gateway_event / sse_ticket / instance / rate_limit + advisory lock）承接全部跨实例职责，进程模型与阶段划分不变。

---

### D.15 落地记录（P0–P3 已实现，2026-11-12）

本节记录**代码实际落地**与上文设计的差异，便于后续维护与验收对照。

#### D.15.1 文件清单

| 文件 | 变更 |
|---|---|
| `crates/database/src/sql/gateway_migration.sql` | 新增：4 张 `sys_gateway_*` 表 + 5 个索引，全部 `IF NOT EXISTS`（事务内执行，故不用 `CONCURRENTLY`） |
| `crates/database/src/postgres.rs` | 新增 `MIGRATION_LOCK_KEY`、`migrate_gateway_tables()`、`acquire_migration_lock()` / `release_migration_lock()`；`init_postgres_database` 串行化 DDL 并追加网关表步骤 |
| `crates/api/src/lib.rs` | 抽出 `pub async fn build_router(pool, llm_router, extra_routes) -> Result<Router>`（上传 / SSE 子路由装配搬入）；抽出 `pub async fn cleanup_once()` 供 leader 循环复用 |
| `crates/api/src/auth.rs` | `AuthPrincipal` / `PrincipalKind` 加 `Serialize/Deserialize`；新增 `AuthInvalidator` trait + `install_auth_invalidator()`、`clear_local_user_authority()`、`clear_local_authority_cache()` |
| `crates/service/src/api_key_service.rs` | 新增 `install_api_key_invalidator()`、`clear_local_api_key()`（修 B8 的 30s 跨实例窗口） |
| `crates/gateway/**` | 新增 crate（bin `assets-gateway`）：`config` / `state` / `coord` / `worker` / `sse` / `rate_limit` / `health` / `router` / `lib` / `main` |
| `Cargo.toml`（workspace） | `tower-http` 增加 `compression-gzip`；`workspace.dependencies` 增加 `gateway` |

#### D.15.2 与设计的偏差（均为实现约束导致，行为等价或更保守）

1. **迁移锁用会话级 `pg_advisory_lock`**（非 `pg_advisory_xact_lock`）：现有初始化函数都是 `&PgPool` 的多语句 DDL，无法整体套进一个事务；`acquire().detach()` 取独占连接并设 `lock_timeout='300s'`，获取失败只告警（迁移脚本幂等）。
2. **票据接口无请求体**：`POST /api/auth/sse-ticket` 直接用主体上绑定的租户签发，避免 `Option<Json<_>>` 提取器在前端发空 body 时的歧义；响应 `{"ticket", "expires_in"}`。
3. **限流桶键**：`tok:<凭证哈希>`（有 `Authorization`）/ `ip:<对端地址>`（`GATEWAY_RATE_LIMIT_ENFORCE_GLOBAL=1` 时）。默认只限制带凭证流量，探针路由不挂限流层；`GATEWAY_RATE_LIMIT_FAIL_OPEN=1`（默认）保证计数失败不阻断业务。
4. **限流为固定窗口 60s / 600 次**（`rate_limit.rs` 常量），未做令牌桶：设计目标是「跨实例总额准确」，固定窗口单语句 UPSERT 已满足。
5. **优雅退出新增 `GATEWAY_DRAIN_SECS`**（默认 15s）：收到信号后先置 `/readyz` 为 503、给所有 SSE 连接推 `event: shutdown`（带 `retry_after_ms`），最多等待该时长后强制断开；随后释放 worker 租约。
6. **gzip 用 `tower-http` 的 `CompressionLayer`**（`GATEWAY_GZIP=1` 才挂）：默认谓词已排除 `text/event-stream`，不需要手写排除逻辑。
7. **联调端点**：`GATEWAY_DEV_ENDPOINTS=1` 时挂 `POST /api/gateway/events`（普通认证），用于「A 发布 → B 收到」的验收。
8. **`gateway` 依赖 `bootstrap`**：复用 `init_database_and_schema()`（租户 schema、缓存预热）与 `init_llm_router()`，避免复制启动序列。
9. **SSE 事件正文回表取**：`NOTIFY` 只带 `{k:'sse',t,id}`，各实例按 id 读 `sys_gateway_event`，规避 payload 8000 字节上限；回放先注册连接再查库，并用连接水位去重，避免回放期间的新事件重复或丢失。

#### D.15.3 验证

```bash
cargo check -p gateway                    # 通过（gateway lib + bin）
cargo test  -p gateway --lib              # config 解析 / 租约间隔单测
```

> 离线环境编译注意：`utoipa-swagger-ui` 的 build script 会下载 Swagger UI 静态资源。无外网时可二选一：把 `SWAGGER_UI_DOWNLOAD_URL` 指向本地 zip（`file:///path/to/v5.32.6.zip`），或把 workspace 里的 features 改为 `["axum", "vendored"]`（见 `docs/API-Swagger规范.md` §5）。

