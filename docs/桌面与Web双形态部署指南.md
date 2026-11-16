# 桌面与 Web 双形态部署指南

> 状态：**M1 已落地**（无头服务入口 + 共享初始化 + 监听地址可配置）；**M2~M6 施工清单已定稿**（2026-11-13）。
> 拓扑决策：**T2 远程服务端** —— 桌面退化为纯 WebView 壳，数据全走 HTTP。
> 逐条路由 / 逐文件删改 / 验收用例见 **`docs/桌面与Web全HTTP改造清单.md`**。


同一份仓库同时交付两种形态，**不需要维护两条代码分支**：

| 形态 | 前端 | 后端入口 | 依赖图形环境 |
| --- | --- | --- | --- |
| 桌面版 | 内嵌进 Tauri 壳（`apps/web/dist`） | `apps/backend/src-tauri/src/main.rs`（`assets-platform`） | 是（WebView2 / webkit2gtk） |
| Web 版 | nginx 托管静态产物 | `apps/backend/src-tauri/crates/server/src/main.rs`（`assets-server`） | **否** |

两者共用 `crates/*`（database / service / api / workflow …）与 `crates/bootstrap` 的启动初始化，
业务逻辑只有一份实现。

---

## 1. 代码结构（M1 新增）

```text
apps/backend/src-tauri/
├─ src/main.rs                        # [既有] 桌面 Tauri 入口
├─ src/lib.rs                         # [改] 初始化改为调用 bootstrap（行为等价）
└─ crates/
   ├─ bootstrap/                      # [新增] 桌面与无头服务共用的启动初始化
   │  ├─ src/config.rs                #   .env.toml → 环境变量（候选路径由调用方传入）
   │  └─ src/lib.rs                   #   数据库/租户 schema、LLM Router、HTTP 服务
   └─ server/                         # [新增] 无头 HTTP 服务（bin: assets-server）
      └─ src/main.rs
```

`bootstrap` **不依赖 tauri**，因此 `cargo build -p server` 不会编译根包：
不需要 `apps/web/dist`、不需要 WebKit / WebView2、不需要 CMake（`crates/ast` 仅被根包依赖）。

---

## 2. 构建

```bash
# 桌面版（产物：apps/backend/src-tauri/target/release/bundle/**）
pnpm build:backend

# Web 版后端（产物：apps/backend/src-tauri/target/release/assets-server[.exe]）
pnpm build:server

# Web 版前端（产物：apps/web/dist）
pnpm build:web
```

> 桌面版构建仍会执行 `tauri.conf.json` 的 `beforeBuildCommand`（`pnpm --prefix ../web build`），
> 因此 `apps/web` 与 `packages/shared` 必须在工作区内可用；服务端构建没有这个约束。

---

## 3. 运行

### 3.0 本地开发快速启动（2026-11-13 实测通过）

前置：PostgreSQL（5432）与 S3/MinIO（9000）已运行，`apps/backend/.env.toml` 已存在。

```bash
# 0) 一次性：创建 .env.toml 里 [postgres] database 指定的库（本例 ceshi8）
#    库不存在时 init_database 直接报错退出；库内表结构与种子数据由后端自动创建
PGPASSWORD=123456 psql -h 127.0.0.1 -U postgres -c 'CREATE DATABASE ceshi8'

# 1) 后端 HTTP（终端 A）——所有 /api/* 由它提供，后台形态无需图形环境
pnpm dev:server
#    配置自动发现：cargo 的 cwd 是 src-tauri，候选表包含上一级 ../.env.toml，
#    即 apps/backend/.env.toml（与桌面 tauri dev 用的是同一份），无需任何环境变量。
#    配置在别处 / 并排多实例时再显式指定：ASSETS_CONFIG_DIR="$PWD/apps/backend" pnpm dev:server（见 §3.2）

# 2) 前端 Web（终端 B）——浏览器里自动选 http 适配器（非 Tauri 环境）
pnpm dev:web                                  # http://localhost:1480
#    指向其它后端：NEXT_PUBLIC_API_BASE_URL=http://<host>:3001/api pnpm dev:web
#    也可以先启动再改：浏览器打开 http://localhost:1480/settings/server 填地址并保存（免重启）

# 3) 桌面壳（可选，终端 C）—— 同一份前端，走 HTTP 到内置的 127.0.0.1:3001
pnpm tauri-dev
```

首次启动会自动执行建表 / 迁移 / 种子数据（`crates/database/src/sql/*.sql`，`include_str!` 进二进制），
默认账号 `admin` / `admin123`。启动成功的关键日志：

```text
数据库初始化完成 → 已设置默认租户 schema → LLM Router 初始化完成
→ HTTP API 服务启动于 http://0.0.0.0:3001
```

冒烟（三条都应返回 200，第 2 条返回规则数组）：

```bash
curl -s -X POST http://127.0.0.1:3001/api/auth/login \
     -H 'Content-Type: application/json' -d '{"username":"admin","password":"admin123"}'
curl -s -H "Authorization: Bearer <token>" http://127.0.0.1:3001/api/numbering/rules
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:1480/login
```

停止：各终端 `Ctrl+C`；后台残留可 `pkill -f target/debug/assets-server`。

#### 端口占用与多实例

`pnpm dev:server` 报 `绑定 TCP 监听地址 0.0.0.0:3001 失败: 端口已被占用` 时，说明已有实例在跑
（常见于自己之前 `nohup`/`setsid` 起的、或 IDE 里没关掉的旧进程）：

```bash
ss -ltnp | grep :3001                 # 看占用进程与 pid
pkill -f target/debug/assets-server   # 停止残留实例
```

启动器**只在绑定成功之后**才打印 `HTTP API 服务启动于 …`，看到该行即代表端口确实拿到了；
绑定失败时只有上面那条带排查命令的报错，不会再出现误导性的「启动于」日志。

想并排跑第二个实例：`API_PORT=3002 pnpm dev:server` **不生效**——`bootstrap::config` 读
`.env.toml` 时对每个键无条件 `set_var`，`[api] port = 3001` 会把环境变量盖回去。正确做法：

```bash
mkdir -p /tmp/assets-dev2 && cp apps/backend/.env.toml /tmp/assets-dev2/.env.toml
sed -i 's/^port = 3001/port = 3002/' /tmp/assets-dev2/.env.toml       # [api] port
sed -i 's/^worker_id = 1/worker_id = 2/' /tmp/assets-dev2/.env.toml   # [snowflake] 必须唯一
ASSETS_CONFIG_DIR=/tmp/assets-dev2 pnpm dev:server
```

⚠️ 两个实例连同一个库时 `[snowflake] worker_id` 必须不同，否则并发写会撞主键。
前端指向第二个实例：`NEXT_PUBLIC_API_BASE_URL=http://127.0.0.1:3002/api pnpm dev:web`。

#### 3.0.1 运行中切换后端地址（`/settings/server`）

不想重启前端 / 重新构建时，直接改地址即可（保存后整页重载生效）：

| 场景 | 操作 |
|---|---|
| Web / 桌面同进程调试 | 打开 `/settings/server` → 填 `http://192.168.1.10:3001` → 「测试连接」→「保存并重载」 |
| 桌面端首次启动连不上内置服务 | 首屏直接出现「先连接服务器」表单（Gate），填好即进系统 |
| 只想看当前连的是谁 | 同页「连接诊断」：`/healthz` 可达性、`db` 状态、往返延迟、`openapi` 版本、地址**来源** |
| 排查远程不可达 | 点「复制诊断信息」贴给运维（含 Origin / UA / 延迟 / 状态码） |

注意：
1. 保存会清掉本地 `auth_token` / `refresh_token` / 用户与租户缓存（跨服务器的令牌无意义），需要重新登录；
2. 非 `localhost` / `127.0.0.1` 的 `http://` 地址会先弹**二次确认**（令牌明文传输风险），确认后才保存；
3. `NEXT_PUBLIC_API_BASE_URL` / `window.__API_BASE__` / 同源反代属于**部署固定**地址，页面保存只覆盖当前浏览器；
4. 浏览器的 `fetch` 无法区分「端口不通 / DNS 失败 / 证书无效 / CORS 被拒」，诊断页统一显示「网络不可达 / CORS」，
   需结合服务端日志与 `curl -v http://<host>:3001/healthz` 进一步定位。

#### 3.0.2 启动即退出：`数据库初始化失败`（本质是配置没被加载）

典型日志（2026-11-16 修复前，`pnpm dev:server` 必现）：

```text
WARN  bootstrap::config: 未找到 .env TOML 配置文件，将使用环境变量与各模块默认值
WARN  database::postgres: 自动创建数据库失败: …password authentication failed for user "postgres"
ERROR assets_server: 数据库初始化失败，服务终止: Failed to create PostgreSQL pool: …
```

成因不在数据库：cargo 的 cwd 是 `apps/backend/src-tauri`，唯一配置在上一级 `apps/backend/.env.toml`，
而候选表当时没有 `../.env.toml` → 配置整体没加载 → PG 账号密码回落默认值 → 无头形态「失败即退出」。

现在候选表已包含 `../.env.toml`（§3.2），正常情况下启动日志里能看到：

```text
INFO bootstrap::config: 已加载 .env TOML 配置文件 config=../.env.toml
```

若仍失败，按启动日志里的两条新提示逐项核对：

| 检查 | 命令 / 判据 |
|---|---|
| 配置到底有没有加载 | `grep -a '已加载 .env TOML' <日志>`；无此行即配置没找到 |
| 强制指定配置目录 | `ASSETS_CONFIG_DIR="$PWD/apps/backend" pnpm dev:server` |
| 库是否存在 | `[postgres] database` 指定的库必须先建好（见本节第 0 步） |
| PG 是否可达 | `PGPASSWORD=123456 psql -h 127.0.0.1 -U postgres -c 'select 1'` |

> 失败时 `assets-server` 会额外打印一行 `排查顺序：① 是否加载 .env.toml（含当前 cwd）② PG 主机/端口/库名/密码 ③ 库是否已创建 ④ 服务端是否可达`，
> 以及所有尝试过的候选路径（`tried=…`），排查时可直接复制。

### 3.1 生产 / 手动启动

```bash
# 开发
pnpm dev:server            # = cd apps/backend && cd src-tauri && cargo run -p server

# 生产（示例）
ASSETS_CONFIG_DIR=/etc/assets-platform \
API_BIND=127.0.0.1 \
API_PORT=3001 \
./assets-server
```

### 3.2 配置文件查找顺序

`assets-server` 依次尝试（命中第一个可读文件即停止）：

1. `$ASSETS_CONFIG_DIR/.env.toml`
2. `./.env.toml`（当前工作目录 / 部署目录）
3. `./src-tauri/.env.toml`
4. `../.env.toml`（**仓库开发布局**：cwd = `apps/backend/src-tauri` → `apps/backend/.env.toml`）
5. `apps/backend/.env.toml`（从仓库根启动时）
6. `/etc/assets-platform/.env.toml`

> 服务端**刻意不使用** `CARGO_MANIFEST_DIR`：编译期绝对路径在换机器 / 容器后必然失效。
> 桌面入口保持原有三条候选路径不变（含 `apps/backend/.env.toml`）。
>
> 第 4 条是 2026-11-16 补的：此前候选表只有 1/2/3/6，`pnpm dev:server`（cwd = `src-tauri`）
> 读不到上一级的 `apps/backend/.env.toml`，会静默回落默认值并在数据库认证处退出
> （详见 §3.0.2）。`[api] port` 等键由 `.env.toml` **无条件覆盖**同名环境变量，所以并排多实例
> 也必须靠换配置文件（或 `ASSETS_CONFIG_DIR`）而非 `API_PORT=…`。

文件格式见 `apps/backend/.env.example.toml`，`[section] key` 会映射为环境变量 `SECTION_KEY`
（例如 `[api] bind` → `API_BIND`）。也可以完全不写配置文件，直接用环境变量注入。

### 3.3 关键环境变量

| 变量 | 缺省 | 说明 |
| --- | --- | --- |
| `API_BIND` | `0.0.0.0` | HTTP 监听网卡地址。**生产建议 `127.0.0.1` + nginx 反代**；非回环地址会打印告警 |
| `API_PORT` | `3001` | HTTP 端口 |
| `API_DOCS_ENABLED` | `true` | 是否暴露 `/swagger-ui`、`/api-docs/openapi.json`；生产建议 `false` |
| `JWT_SECRET` | 代码内默认值 | **必须替换**，否则签名保护形同虚设 |
| `PG_HOST/PG_PORT/PG_DATABASE/PG_USERNAME/PG_PASSWORD` | `localhost/5432/assets_platform/postgres/postgres` | PostgreSQL（schema 多租户） |
| `PG_READ_HOSTS` | 空 | 只读从库，格式 `host:port:weight,...`（由 `[postgres] read_replicas` 自动生成） |
| `S3_ENDPOINT/S3_REGION/S3_ACCESS_KEY/S3_SECRET_KEY/S3_BUCKET/S3_PUBLIC_URL` | 无 | **缺少时 HTTP 服务启动即失败**（`S3Config::from_env` 报错） |
| `DOC_PARSER_ENABLED` | `true` | 是否拉起 Python 侧车；独立部署 doc-parser 时设为 `false` |
| `DOC_PARSER_BACKEND` | `remote` | `local` = 进程内 Rust 解析；`remote` = Python 侧车 |
| `OTEL_ENABLED` 等 | `true` / `localhost:4317` | OpenTelemetry 导出（`docker-compose.yml` 提供 collector） |

### 3.4 运行期外部依赖

PostgreSQL（必需）、S3/MinIO（必需）、SurrealDB（知识库向量，按需）、
doc-parser（按需）、OTel Collector（可选）。

数据库建表 / 迁移 SQL 以 `include_str!` 编译进二进制（`crates/database/src/sql/*.sql`），
**部署机不需要额外的 SQL 文件**。

---

## 4. 验证清单

```bash
# 1. 服务起来了（非 404 即说明路由已挂载）
curl -i http://127.0.0.1:3001/api-docs/openapi.json      # API_DOCS_ENABLED=true 时
# 2. 登录拿 JWT
curl -s -X POST http://127.0.0.1:3001/api/auth/login \
     -H 'Content-Type: application/json' \
     -d '{"username":"admin","password":"admin123"}'
# 3. 带 token 调受保护接口
curl -s http://127.0.0.1:3001/api/users/me -H "Authorization: Bearer <token>"
# 4. 优雅退出：Ctrl+C / SIGTERM 应打印「收到关闭信号，HTTP 服务正在关闭...」
```

桌面侧回归（重构后必须跑）：

```bash
pnpm dev:backend     # 窗口正常打开、登录、资产列表、上传、切租户
```

---

## 5. 已知限制（M1 之后仍需处理，**不要直接暴露到公网**）

1. **多租户并发串数据（P0-2）**：`GLOBAL_SCHEMA` 是进程级单例，HTTP 鉴权中间件每个请求都会覆盖它
   （`crates/api/src/auth.rs`）。桌面单用户场景可用，**Web 多用户场景必须先做 schema `task_local` 化**。
2. **上传 / SSE 未鉴权（P0-1）**：`/api/upload/*` 与 `/api/chat/stream` 在 `auth_middleware` 之外，
   `created_by` 硬编码为 `1`；前端 `HttpUploadAdapter` 与 `useChatStream` 也未带 `Authorization` 头。
3. **CORS / CSP / JWT 默认密钥（P0-3）**：`CorsLayer` 为 `Any`，需收敛白名单。
4. **HTTP 路由未覆盖全部功能（缺 42 条）**：`TAURI_ONLY_COMMANDS` 登记 32 条，另有 20 条
   **既无映射也不在名单**（工作流 10 + 沙箱 10），Web 端访问工作流会直接报「未找到命令」。
   反向还有 10 条映射无对应 Tauri 命令（API Key 4、OAuth 客户端 4、`logout`、`refresh_token`），
   即**桌面 invoke 模式下这些功能当前不可用**。清单见 `docs/桌面与Web全HTTP改造清单.md` §1。
5. **前端 API 基址是构建期常量**：`httpAdapter.ts:36` / `uploadService.ts:190,353` / `useChatStream.ts:39`
   硬编码 `http://127.0.0.1:3001`，浏览器访问远程部署时必须改造成运行时推导（`utils/apiBase.ts`）。
6. **静态资源无托管方**：`crates/server` 不托管 SPA（全仓库无 `ServeDir`），Web 版必须由 nginx 提供
   `apps/web/dist` 与 `/api` 反代，SSE 需 `proxy_buffering off`。
7. **`jsonwebtoken` 11 的 aud 校验导致 HTTP 全站 401（已修复）**：签发侧自 Phase 3 起固定写
   `iss` / `aud`，而 `crates/api/src/auth.rs::verify_token` 用的是 `Validation::default()`
   （`validate_aud = true` 且 `aud = None`）。`jsonwebtoken` 11 在「令牌带 `aud` 而校验未设 audience」时
   直接返回 `InvalidAudience`，表现为**登录成功、随后每个受保护接口都 401「无效的认证令牌」**——
   桌面 invoke 不经过该校验，所以此前只在 Web/HTTP 形态暴露。
   修复：新增 `jwt_validation()`，用 `set_audience` / `set_issuer` 与签发侧共用
   `service::user_service::jwt_audience()` / `jwt_issuer()`；历史无 `aud` 令牌仍宽容放行，
   `JWT_STRICT_ISS_AUD` 语义不变。回归用例见 `auth.rs` 的 `tests::token_with_audience_passes_verification`。


---

## 6. 与桌面形态的隔离原则

- 桌面命令层（123 个 `#[tauri::command]`）与 HTTP 路由层（`crates/api`）共用 `crates/service`，
  新增业务逻辑只写在 service 层，两边都只做「参数转换 + 错误映射」。
- `bootstrap` 只放**与 UI 框架无关**的初始化；Tauri 专属部分（Builder / 插件 / `manage` / `invoke_handler`）
  留在 `src/lib.rs`。
- **过渡期**（当前）：桌面形态下 HTTP 服务仍会启动（上传分片、SSE 依赖它）；若只想跑桌面、不开放任何端口，
  可设 `API_BIND=127.0.0.1` 收敛到本机。
- **终态（T2 全 HTTP）**：桌面退化为纯 WebView 壳，**不再启动任何 HTTP 服务、不连本地数据库、不拉 doc-parser**，
  数据一律来自用户配置的远端地址（`apps/web/src/utils/apiBase.ts`）。删改清单见
  `docs/桌面与Web全HTTP改造清单.md` §3、§5。


---

## 7. 后续里程碑

| 阶段 | 内容 | 桌面影响 |
| --- | --- | --- |
| **M2** | 补齐 42 条 HTTP 路由（`crates/api/*_routes.rs`）+ `ROUTE_PERMS` + `sys_menu` + 前端映射 | 无（桌面/Web 同时受益） |
| **M3** | `GET /livez` + 版本接口 + CORS 白名单（api 与 gateway）；上传/SSE 纳入鉴权 + token 透传 | 需回归登录/切租户/上传 |
| **M4** | 前端 `utils/apiBase.ts` + `ServerConfigGate` + `settings/server` 诊断页；删除 tauri/http 双适配器 | 桌面开始需要配置服务器地址 |
| **M5** | 桌面瘦身（T2）：删除内嵌 axum / DB / LLM / doc-parser / 154 个 invoke 命令 | **与 M4 同版本发布** |
| **M6** | 部署交付物（nginx / Dockerfile / compose / MinIO CORS / systemd / env 拆分）+ `GLOBAL_SCHEMA` → `task_local` fail-closed | 需回归 |

施工级清单（逐条路由、逐文件删改、验收用例）见 **`docs/桌面与Web全HTTP改造清单.md`**。


详细缺陷条目与验收标准见 `docs/架构问题与缺陷分析及整改排期.md`（P0-1 ~ P0-4、B0-1 ~ B0-3）。
