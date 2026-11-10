# 桌面与 Web 双形态部署指南

> 状态：**M1 已落地**（无头服务入口 + 共享初始化 + 监听地址可配置）。
> M2~M5（前端同源化、鉴权收敛、HTTP 路由补齐、安全加固）见本文 §7。

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

```bash
# 开发
pnpm dev:server            # = cd apps/backend && cd src-tauri && cargo run -p server

# 生产（示例）
ASSETS_CONFIG_DIR=/etc/assets-platform \
API_BIND=127.0.0.1 \
API_PORT=3001 \
./assets-server
```

### 3.1 配置文件查找顺序

`assets-server` 依次尝试（命中第一个可读文件即停止）：

1. `$ASSETS_CONFIG_DIR/.env.toml`
2. `./.env.toml`（当前工作目录）
3. `./src-tauri/.env.toml`
4. `/etc/assets-platform/.env.toml`

> 服务端**刻意不使用** `CARGO_MANIFEST_DIR`：编译期绝对路径在换机器 / 容器后必然失效。
> 桌面入口保持原有三条候选路径不变（含 `apps/backend/.env.toml`）。

文件格式见 `apps/backend/.env.example.toml`，`[section] key` 会映射为环境变量 `SECTION_KEY`
（例如 `[api] bind` → `API_BIND`）。也可以完全不写配置文件，直接用环境变量注入。

### 3.2 关键环境变量

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

### 3.3 运行期外部依赖

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
4. **HTTP 路由未覆盖全部功能**：`apps/web/src/utils/adapters/httpAdapter.ts` 的
   `TAURI_ONLY_COMMANDS` 仍登记 32 个命令（LLM 厂商 12、知识资产 7、对话 6、编号 4、RAG 2、上传版本历史 1），
   Web 端访问这些模块会收到明确的「仅支持 Tauri 模式」错误。
5. **前端 API 基址是构建期常量**：`httpAdapter.ts` / `uploadService.ts` / `useChatStream.ts`
   三处硬编码 `http://localhost:3001`，浏览器访问远程部署时需要改造成运行时推导（M2）。

---

## 6. 与桌面形态的隔离原则

- 桌面命令层（123 个 `#[tauri::command]`）与 HTTP 路由层（`crates/api`）共用 `crates/service`，
  新增业务逻辑只写在 service 层，两边都只做「参数转换 + 错误映射」。
- `bootstrap` 只放**与 UI 框架无关**的初始化；Tauri 专属部分（Builder / 插件 / `manage` / `invoke_handler`）
  留在 `src/lib.rs`。
- 桌面形态下 HTTP 服务仍会启动（上传分片、SSE 依赖它）；若只想跑桌面、不开放任何端口，
  可设 `API_BIND=127.0.0.1` 收敛到本机。

---

## 7. 后续里程碑

| 阶段 | 内容 | 桌面影响 |
| --- | --- | --- |
| **M2** | 前端 `apiBase.ts`（运行时推导基址）+ nginx / Dockerfile / systemd 交付物 | 低（桌面走 `127.0.0.1`） |
| **M3** | 上传/SSE 纳入鉴权 + 前端 token 透传；`GLOBAL_SCHEMA` 改为 HTTP 侧 `task_local` + fail-closed | 需回归登录/切租户/上传 |
| **M4** | 补齐 `TAURI_ONLY_COMMANDS` 对应的 HTTP 路由（`crates/api/*_routes.rs`）+ 前端映射登记 + 权限点 | 无 |
| **M5** | CORS 白名单、JWT 密钥强制校验、注册开关、CSP | 需回归 |

详细缺陷条目与验收标准见 `docs/架构问题与缺陷分析及整改排期.md`（P0-1 ~ P0-4、B0-1 ~ B0-3）。
