# HTTP API 的 OpenAPI / Swagger 规范

本仓库的后端 HTTP API（`apps/backend/src-tauri/crates/api`）已接入
[`utoipa`](https://docs.rs/utoipa) 6 + [`utoipa-swagger-ui`](https://docs.rs/utoipa-swagger-ui) 10，
由独立 crate **`crates/apidoc`** 汇总规范并提供 Swagger UI 页面。

## 1. 访问地址

HTTP 服务默认监听 `http://127.0.0.1:3001`（可用环境变量 `API_PORT` 覆盖）：

| 地址 | 内容 |
|---|---|
| `http://127.0.0.1:3001/swagger-ui` | Swagger UI 页面（可直接调试接口） |
| `http://127.0.0.1:3001/api-docs/openapi.json` | OpenAPI 3.1 规范 JSON（可导入 Apifox / Postman / 代码生成器） |

调试受保护接口：

1. `POST /api/auth/login` 拿到 `data.token`（公开接口，不需要认证）；
2. 点页面右上角 **Authorize**，填入 token（无需手写 `Bearer ` 前缀）；
3. 页面开启了 `persist_authorization`，刷新后 token 仍保留在浏览器 localStorage。

关闭文档（例如生产环境收敛）：

```bash
API_DOCS_ENABLED=false   # 或 0
```

关闭后 `/swagger-ui`、`/api-docs/openapi.json` 不再挂载（返回 404），其余接口不受影响。

## 2. 架构与依赖方向

```
apidoc ──▶ api ──▶ service ──▶ database
```

- **`crates/api`**：handler 与请求/响应 DTO 的**标注**写在定义处
  （`#[utoipa::path]` / `#[derive(ToSchema)]` / `#[derive(IntoParams)]`），
  规范与实现同源，避免「文档一套、代码一套」。
- **`crates/apidoc`**：只做两件事 —— 用 `#[derive(OpenApi)]` 汇总 `paths(...)` 并
  生成 `ApiDoc`；用 `SwaggerUi` 提供 `/swagger-ui` 与 `/api-docs/openapi.json`。
- `api` **不**依赖 `apidoc`（否则成环）。文档路由由调用方注入：

  ```rust
  // apps/backend/src-tauri/src/lib.rs
  let docs = apidoc::docs_enabled().then(apidoc::docs_router);
  api::start_http_server_with(pool, Some(llm_router), docs).await
  ```

  `start_http_server_with` 的 `extra_routes` 在**顶层** `merge`，与 `/api/upload/*`、
  `/api/chat/stream` 一样位于 `auth_middleware` 之外，所以看文档不需要先登录；
  接口本身的鉴权仍然由各自的 handler / 中间件保证。

## 3. 新增 / 修改接口的 checklist

接一个新接口时，只需三步（缺一步不会报错，但文档会不完整）：

1. **DTO 加 schema**
   ```rust
   #[derive(Debug, Deserialize, utoipa::ToSchema)]
   pub struct CreateFooRequest { /* ... */ }
   ```
   - 查询参数结构体用 `#[derive(utoipa::IntoParams)]`，泛型里不能包 `HashMap`
     （`Query<HashMap<String, String>>` 不会被自动解析，需要手写 `params(...)`）。
   - 递归结构（树）必须在子节点字段上加 `#[schema(no_recursion)]`，否则生成规范时会 panic。

2. **handler 加 path 标注**
   ```rust
   #[utoipa::path(
       post,
       path = "/api/foo",
       tag = "…",
       security(("bearer_auth" = [])),
       request_body = CreateFooRequest,
       responses(
           (status = 200, description = "创建成功", body = ApiResponse<FooView>),
           (status = 400, description = "参数错误", body = ApiError),
           (status = 401, description = "未认证", body = ApiError),
       )
   )]
   pub async fn insert_foo(/* ... */) -> Result<Json<ApiResponse<FooView>>, ApiError> { … }
   ```
   - 启用了 `axum_extras`：`Path<T>` / `Query<T>` 会自动转成路径 / 查询参数，
     **不要**再手写同名 `params(...)`（会重复）。路径参数名以 `path = "…/{id}"` 里的占位符为准。
   - 公开接口（`/api/auth/login`、`/api/auth/register`）不写 `security(...)`。
   - 返回 `ApiResponse<()>` 的接口（删除类）不写 `body`，用
     `description` 说明「删除成功（data 为 null）」—— utoipa 没有为 `()` 实现 `ToSchema`。
   - SSE 接口见 `chat_routes::chat_stream`：`content_type = "text/event-stream"`。

3. **在 `crates/apidoc/src/openapi.rs` 的 `paths(...)` 里登记该 handler**
   ```rust
   api::foo_routes::insert_foo,
   ```

新增业务分组时同步在 `ApiDoc` 的 `tags(...)` 里加一条中文说明。

校验（无需数据库）：

```bash
cd apps/backend/src-tauri
cargo check -p apidoc   # 标注写错（类型不满足 ToSchema、宏语法）都会在此暴露
cargo test  -p apidoc   # 真正生成规范并序列化，捕获递归 panic 等问题
```

## 4. 统一约定

- **响应结构**：`{ "code": 0, "message": "success", "data": ... }`；
  失败时 `code` 与 HTTP 状态码一致，响应体为 `{ "code": 400, "message": "..." }`。
- **安全方案**：`bearer_auth`（HTTP bearer + JWT），在 `ApiDoc` 里通过
  `impl Modify for BearerAuthAddon` 注册。
- **ID 约定**：雪花 ID 在响应 JSON 中统一序列化为**字符串**（避免 JS 精度丢失），
  请求侧同时接受字符串与数字（`database::models::i64_from_string`）。
  规范里这类字段当前渲染为 `integer`，以示例与本节说明为准。
- **tag 划分**：认证、用户、注册审核、资产分类、固定资产、无形资产、部门、角色、菜单、
  租户、知识库、流程管理-{领用,归还,调拨,维修,报废,采购}、运维采集、大文件上传、智能问答、技能。

## 5. 构建注意事项

`utoipa-swagger-ui` 在 **build script** 阶段下载 Swagger UI 静态资源（Windows 上默认走
`reqwest`），首次编译需要外网。离线 / 内网构建时改成 vendored 资源：

```toml
# apps/backend/src-tauri/Cargo.toml
utoipa-swagger-ui = {version = "10.0.2", features = ["axum", "vendored"]}
```

或用环境变量指向本地 zip：

```bash
SWAGGER_UI_DOWNLOAD_URL=file:///D:/deps/swagger-ui-5.32.6.zip
```

版本必须成对：`utoipa-swagger-ui 10.x` 依赖 `utoipa ^6`，两个大版本并存会导致
`SwaggerUi::url` 的 `OpenApi` 参数类型不匹配。

## 6. 令牌生命周期（access / refresh）

### 6.1 端点

| 方法 | 路径 | 鉴权 | 说明 |
|---|---|---|---|
| POST | `/api/auth/login` | 公开 | 返回 `token`（访问令牌）+ `refresh_token`（刷新令牌）+ `expires_in` |
| POST | `/api/auth/refresh` | 公开 | 用 `refresh_token` 换新的一对令牌（旧 refresh 立即失效） |
| POST | `/api/auth/logout` | 公开 | 撤销当前 `refresh_token`（幂等） |

刷新 / 登出不能要求 `Authorization` 头 —— 访问令牌过期后仍必须能调用，安全性由
`refresh_token` 自身（JWT 签名 + DB 白名单）保证。

### 6.2 有效期

| 环境变量 | 默认 | 作用 |
|---|---|---|
| `JWT_SECRET` | 无（回落内置值，**生产必须配置**） | HS256 签名密钥 |
| `JWT_ACCESS_TTL_SECS` | `604800`（7 天） | 访问令牌有效期；生产建议 1800–7200 |
| `JWT_REFRESH_TTL_SECS` | `2592000`（30 天） | 刷新令牌有效期 |

### 6.3 轮换与失效规则

- **轮换**：每次刷新都会吊销旧 jti（`revoked_at` + `replaced_by`）并签发新 jti，同属一个
  `family_id`（= 一次登录派生的令牌链）。客户端必须用响应里的新 `refresh_token` 覆盖本地值。
- **重放检测**：已吊销的 jti 再次出现 → 判定令牌泄露 → **整条 family 全部吊销**，返回 401，
  用户必须重新登录。
- **立即失效的触发点**：
  - `POST /api/auth/logout`（撤销当前 refresh）；
  - 用户被禁用（`update_user` 且 status≠1）、改密（`reset_password`）、删除（`delete_user`）
    → 撤销该用户**全部** refresh（`revoke_all_user_refresh_tokens`）。
- **refresh 不能当 access 用**：`Claims.typ` 区分，`auth_middleware` 只接受 `typ = "access"`
  （历史令牌无 `typ` 字段，按 access 处理，升级不强制重登）。
- **清理**：每次刷新顺带删除 `expires_at < now() - 7 days` 的历史行。

数据表：`public.sys_user_refresh_token`（`database/src/sql/public_tables.sql` 建表 +
`public_migration.sql` 幂等补表）。

### 6.4 手工验证

```bash
BASE=http://127.0.0.1:3001

# 1) 登录，取出 token / refresh_token
LOGIN=$(curl -s -X POST $BASE/api/auth/login -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"***"}')
TOKEN=$(echo "$LOGIN" | jq -r '.data.token')
REFRESH=$(echo "$LOGIN" | jq -r '.data.refresh_token')

# 2) 刷新（无需 Authorization）→ 拿到新的一对
NEW=$(curl -s -X POST $BASE/api/auth/refresh -H 'Content-Type: application/json' \
  -d "{\"refresh_token\":\"$REFRESH\"}")
echo "$NEW" | jq -r '.data.token'

# 3) 再用旧 refresh 刷一次 → 401，且整条 family 被吊销（重放检测）
curl -s -X POST $BASE/api/auth/refresh -H 'Content-Type: application/json' \
  -d "{\"refresh_token\":\"$REFRESH\"}" | jq

# 4) refresh 当 access 用 → 401（typ 校验）
curl -s $BASE/api/users -H "Authorization: Bearer $REFRESH" | jq
```

### 6.5 前端行为（`apps/web`）

- `authStore.login()` 保存 `auth_token` + `refresh_token`；`logout()` 先调后端撤销再清本地。
- `httpAdapter` 收到 **401** → 用 `refresh_token` 静默续期（`refreshAccessToken()`，
  并发去重）→ 自动重放一次原请求；刷新失败则 `clearAuthSession()` 清空并跳 `/login`。
- 刷新成功后派发 `auth:tokens-refreshed` 事件，`authStore` 同步内存中的 `token`。
- 应用启动（`authStore.init()`）发现访问令牌过期时，先尝试刷新，失败才登出。
- 认证端点（login/register/refresh/logout）不参与自动续期，避免递归。

