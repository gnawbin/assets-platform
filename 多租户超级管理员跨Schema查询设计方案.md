# 多租户数据权限与租户切换设计方案（合并版）

> 解决普通用户可分配多个租户、超级管理员可自由切换租户的问题
> 基于当前 PostgreSQL Schema 隔离架构
> 使用 DashMap 缓存 + tokio::task_local! 实现请求级 schema 隔离

---

## 一、问题现状

### 1.1 当前架构

```
┌─────────────────────────────────────────────┐
│               public schema                  │
│  sys_tenant  │  sys_user  │  sys_menu  ...  │  ← 全局共享
├─────────────────────────────────────────────┤
│               single schema (租户A)          │
│  assets  │  hard_assets  │  asset_receive   │  ← schema_prefix() 隔离
├─────────────────────────────────────────────┤
│               tenant_b schema (租户B)        │
│  assets  │  hard_assets  │  asset_receive   │  ← schema_prefix() 隔离
└─────────────────────────────────────────────┘
```

### 1.2 现有问题

| 问题 | 说明 |
|------|------|
| `sys_user.tenant_id` 是单值字段 | 一个用户只能关联一个租户 |
| 普通用户没有数据权限概念 | 目前只能看到自己 `tenant_id` 对应的那个 schema |
| `CURRENT_SCHEMA` 是进程级全局变量 | Tauri 单进程多请求，并发场景下请求间互相覆盖 |
| 超级管理员视角受限 | 虽然能调 `switch_tenant()`，但没有界面操作 |

### 1.3 当前全局变量缺陷

```rust
static CURRENT_SCHEMA: OnceLock<RwLock<String>>;  // 进程级全局变量 ❌
```

Tauri 单进程多请求（async 运行时跨线程），所有请求共享同一进程：

```
┌──── 用户A (tenant_b) ────┐
│ 请求1: set_current_schema("tenant_b")  │  → CURRENT_SCHEMA = "tenant_b"
└──────────────────────────────┘
┌──── 用户B (tenant_c) ────┐
│ 请求2: set_current_schema("tenant_c")  │  → CURRENT_SCHEMA = "tenant_c"  ← 覆盖了用户A
└──────────────────────────────┘
┌──── 用户A 下一个请求 ────┐
│ schema_prefix() 返回 "tenant_c."    │  → ❌ 用户A读到用户B的数据
└──────────────────────────────┘
```

---

## 二、核心设计思路

### 2.1 改造目标

1. **废除全局 `CURRENT_SCHEMA`**，解决并发安全问题
2. **不改任何业务 service 函数签名**（44 个函数不需加 `&str schema` 参数）
3. **不改前端 HTTP 请求**（不传 `tenant_id` header/query）
4. **支持多租户分配**：用户可关联多个租户，但当前活跃的只有一个

### 2.2 架构原理

```
请求到达 axum 路由
   │
   ▼
auth middleware（解析 JWT → user_id）
   │ 查 DashMap<user_id, tenant_id> 缓存
   │ 查 DB 获取 schema_name（缓存未命中时）
   │ 设置 tokio::task_local!(CURRENT_SCHEMA)
   ▼
route handler → 调用 service
   │ ─ schema_prefix() 从 task_local 读取当前 schema
   │ ─ 所有 SQL 自动拼上 {schema}.table_name
   │ ─ 函数签名完全不变
   ▼
返回响应
```

### 2.3 核心变化

| 旧方案 | 新方案 |
|--------|--------|
| `OnceLock<RwLock<String>>` 全局变量 | `tokio::task_local!` 请求级变量 |
| `set_current_schema()` 改全局状态 | auth middleware 统一设 task_local |
| service 函数要加 `schema: &str` 参数 | **service 完全不需要改** |
| 前端要传 `tenant_id` | **前端不需要传** |
| **各尽不同** | **DashMap<user_id, tenant_id>** 缓存当前活跃租户 |

---

## 三、详细设计方案

### 3.1 数据库：新增 sys_user_tenant 关联表

```sql
-- 用户-租户 多对多关联表
CREATE TABLE IF NOT EXISTS public.sys_user_tenant (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES public.sys_user(id) ON DELETE CASCADE,
    tenant_id BIGINT NOT NULL REFERENCES public.sys_tenant(id) ON DELETE CASCADE,
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(user_id, tenant_id)
);

COMMENT ON TABLE public.sys_user_tenant IS '用户-租户关联表（多对多）';
COMMENT ON COLUMN public.sys_user_tenant.user_id IS '用户ID';
COMMENT ON COLUMN public.sys_user_tenant.tenant_id IS '租户ID';
```

**迁移策略**：
- 现有 `sys_user.tenant_id` 字段保留不动（向后兼容）
- 首次部署时，为每个已有用户自动插入一条 `sys_user_tenant` 记录，值等于其 `tenant_id`
- 超级管理员（`is_super_admin=true`）不插入关联表，登录时直接返回所有租户

### 3.2 database 层重构

#### 3.2.1 postgres.rs：删除全局变量，新增缓存

```rust
// ====== 删除以下内容 ======
static CURRENT_SCHEMA: OnceLock<RwLock<String>> = OnceLock::new();
pub fn get_current_schema() -> String { ... }
pub fn set_current_schema(schema: &str) { ... }

// ====== 新增以下内容 ======
use dashmap::DashMap;

/// 租户 ID → schema_name 缓存（应用启动时预加载）
static SCHEMA_CACHE: OnceLock<DashMap<i64, String>> = OnceLock::new();

/// 用户 ID → 当前选中租户 ID 缓存（登录/切换时更新）
static USER_TENANT_CACHE: OnceLock<DashMap<i64, i64>> = OnceLock::new();

/// 根据租户ID获取 schema 名称（先查缓存，未命中则查DB）
pub async fn get_schema_by_tenant_id(pool: &PgPool, tenant_id: i64) -> Result<String, String> {
    // 先查缓存
    let cache = SCHEMA_CACHE.get_or_init(|| DashMap::new());
    if let Some(schema) = cache.get(&tenant_id) {
        return Ok(schema.clone());
    }
    // 缓存未命中，查数据库
    let schema = sqlx::query_scalar::<_, String>(
        "SELECT schema_name FROM public.sys_tenant WHERE id = $1 AND enable = true"
    )
    .bind(tenant_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("查询租户 schema 失败: {}", e))?
    .ok_or_else(|| "租户不存在或已禁用".to_string())?;
    // 写入缓存
    cache.insert(tenant_id, schema.clone());
    Ok(schema)
}

/// 应用启动时预加载 SCHEMA_CACHE
pub async fn preload_schema_cache(pool: &PgPool) -> Result<()> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, schema_name FROM public.sys_tenant WHERE enable = true AND schema_name IS NOT NULL"
    )
    .fetch_all(pool)
    .await?;
    let cache = SCHEMA_CACHE.get_or_init(|| DashMap::new());
    for (id, schema) in rows {
        cache.insert(id, schema);
    }
    Ok(())
}
```

#### 3.2.2 database/mod.rs: tokio::task_local! 替代全局变量

```rust
use std::cell::RefCell;

/// 每个请求的 schema 上下文（tokio task local）
tokio::task_local! {
    static CURRENT_SCHEMA: String;
}

/// 设置当前请求的 schema（在 auth middleware 中调用）
pub async fn set_current_schema(schema: &str) {
    let _ = CURRENT_SCHEMA.scope(schema.to_string(), async {}).await;
}

/// 获取 schema 前缀（例如 "tenant_b."）
///
/// - 如果 schema 为 "public" 或空，返回空字符串
/// - 否则返回 "{schema}." 格式的前缀
pub fn schema_prefix() -> String {
    let schema = CURRENT_SCHEMA.try_with(|s| s.clone()).unwrap_or_else(|_| "public".to_string());
    if schema == "public" || schema.is_empty() {
        String::new()
    } else {
        format!("{}.", schema)
    }
}
```

> **注意**：`set_current_schema` 在 middleware 中调用，使用 `tokio::spawn` 包装以创建新的 task local 作用域。实际实现时，auth middleware 会在 `Extension` 中携带 `UserContext`，route handler 通过 `with_current_schema` 包裹后续调用。

**更稳妥的实现方式**：在 middleware 中将 schema 存在 `Extension<UserContext>` 中，然后提供一个工具函数，在 route handler 中包装 task local：

```rust
/// 在请求上下文中执行带 schema 的操作
pub async fn with_schema<F, Fut, T>(schema: String, f: F) -> T
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = T>,
{
    CURRENT_SCHEMA.scope(schema, f).await
}
```

route handler 中使用：

```rust
pub async fn get_categories_handler(
    Extension(ctx): Extension<UserContext>,
) -> Result<Json<ApiResponse<Vec<AssetCategory>>>, ApiError> {
    with_schema(ctx.schema.clone(), async {
        let categories = assets_categories_service::get_categories().await?;
        Ok(Json(ApiResponse::success(categories)))
    }).await
}
```

### 3.3 Auth Middleware（新建）

```rust
/// 用户上下文（注入到每个请求的 Extension 中）
#[derive(Debug, Clone)]
pub struct UserContext {
    pub user_id: i64,
    pub username: String,
    pub is_super_admin: bool,
    pub tenant_id: Option<i64>,
    pub schema_name: String,
}

/// JWT 认证中间件
///
/// 1. 从 Authorization header 解析 JWT
/// 2. 从 JWT 获取 user_id
/// 3. 查 USER_TENANT_CACHE 获取当前选中租户
/// 4. 查 SCHEMA_CACHE / DB 获取 schema_name
/// 5. 注入 UserContext 到 Extension
pub async fn auth_middleware<B>(
    mut req: Request<B>,
    next: Next<B>,
) -> Result<Response, StatusCode> {
    // 解析 JWT
    let token = req.headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let claims = decode_jwt(token)?;

    // 查 USER_TENANT_CACHE 获取用户当前选中租户
    let cache = USER_TENANT_CACHE.get_or_init(|| DashMap::new());
    let tenant_id = cache.get(&claims.user_id).map(|v| *v);

    // 查 schema_name
    let schema_name = match tenant_id {
        Some(tid) => get_schema_name(tid),
        None => "public".to_string(),
    };

    // 注入 UserContext
    let ctx = UserContext {
        user_id: claims.user_id,
        username: claims.username,
        is_super_admin: claims.is_super_admin,
        tenant_id,
        schema_name: schema_name.clone(),
    };
    req.extensions_mut().insert(ctx);

    // 设 task_local（通过 with_schema 包装后续处理）
    Ok(with_schema(schema_name, async {
        next.run(req).await
    }).await)
}
```

### 3.4 登录接口增强（user_service.rs）

```rust
// LoginResponse 增加字段
pub struct LoginResponse {
    // ... 现有字段
    pub available_tenants: Vec<TenantInfo>,  // 新增
}

pub struct TenantInfo {
    pub id: i64,
    pub tenant_name: String,
    pub schema_name: Option<String>,
    pub is_current: bool,
}

// 登录逻辑增强
pub async fn login(username: &str, password: &str) -> Result<LoginResponse, String> {
    // ... 原有身份验证逻辑不变 ...

    // 新增：查询用户可访问的租户列表
    let available_tenants = if user.is_super_admin {
        // 超级管理员：返回所有启用的租户
        sqlx::query_as::<_, SysTenant>(
            "SELECT id, tenant_name, parent_id, is_leaf, schema_name, enable, create_at, updated_at
             FROM public.sys_tenant WHERE enable = true ORDER BY id ASC"
        )
        .fetch_all(&pool)
        .await?
    } else {
        // 普通用户：从 sys_user_tenant 关联表查询
        sqlx::query_as::<_, SysTenant>(
            "SELECT t.id, t.tenant_name, t.parent_id, t.is_leaf, t.schema_name, t.enable, t.create_at, t.updated_at
             FROM public.sys_user_tenant ut
             JOIN public.sys_tenant t ON t.id = ut.tenant_id
             WHERE ut.user_id = $1 AND t.enable = true
             ORDER BY t.id ASC"
        )
        .bind(user.id)
        .fetch_all(&pool)
        .await?
    };

    // 将 SysTenant 转换为 TenantInfo，标记当前租户
    let current_tenant_id = user.tenant_id;
    let info: Vec<TenantInfo> = available_tenants.into_iter().map(|t| TenantInfo {
        id: t.id,
        tenant_name: t.tenant_name,
        schema_name: t.schema_name,
        is_current: Some(t.id) == current_tenant_id,
    }).collect();

    // 更新 USER_TENANT_CACHE
    let cache = USER_TENANT_CACHE.get_or_init(|| DashMap::new());
    if let Some(tid) = user.tenant_id {
        cache.insert(user.id, tid);
    }

    // ... 返回 LoginResponse，携带 available_tenants ...
}
```

### 3.5 switch_tenant 增强（tenant_service.rs）

```rust
pub async fn switch_tenant(user_id: i64, tenant_id: i64) -> Result<TenantInfo, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    // 1. 检查用户是否是超级管理员
    let is_super_admin: bool = sqlx::query_scalar(
        "SELECT is_super_admin FROM public.sys_user WHERE id = $1"
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("查询用户失败: {}", e))?
    .ok_or("用户不存在")?;

    // 2. 校验权限
    if !is_super_admin {
        let has_access: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM public.sys_user_tenant WHERE user_id = $1 AND tenant_id = $2)"
        )
        .bind(user_id)
        .bind(tenant_id)
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("查询权限失败: {}", e))?
        .unwrap_or(false);

        if !has_access {
            return Err("无权访问该租户".to_string());
        }
    }

    // 3. 查询 schema_name
    let schema_name = crate::database::postgres::get_schema_by_tenant_id(&pool, tenant_id).await?;

    // 4. 更新 USER_TENANT_CACHE
    let cache = USER_TENANT_CACHE.get_or_init(|| DashMap::new());
    cache.insert(user_id, tenant_id);

    // 5. 查询租户信息返回
    let tenant = sqlx::query_as::<_, SysTenant>(
        "SELECT id, tenant_name, parent_id, is_leaf, schema_name, enable, create_at, updated_at
         FROM public.sys_tenant WHERE id = $1"
    )
    .bind(tenant_id)
    .fetch_one(&pool)
    .await
    .map_err(|e| format!("查询租户信息失败: {}", e))?;

    Ok(TenantInfo {
        id: tenant.id,
        tenant_name: tenant.tenant_name,
        schema_name: tenant.schema_name,
        is_current: true,
    })
}
```

### 3.6 assign_user_tenants（tenant_service.rs 新增）

```rust
/// 为用户分配租户（覆盖式）
pub async fn assign_user_tenants(
    user_id: i64,
    tenant_ids: &[i64],
    current_user_id: i64,
) -> Result<(), String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    // 事务：删除旧关联 → 插入新关联
    let mut tx = pool.begin().await.map_err(|e| format!("开启事务失败: {}", e))?;

    sqlx::query("DELETE FROM public.sys_user_tenant WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("删除旧关联失败: {}", e))?;

    for tenant_id in tenant_ids {
        sqlx::query(
            "INSERT INTO public.sys_user_tenant (id, user_id, tenant_id, created_by)
             VALUES ($1, $2, $3, $4)"
        )
        .bind(crate::utils::snowflake::next_id() as i64)
        .bind(user_id)
        .bind(tenant_id)
        .bind(current_user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("插入关联失败: {}", e))?;
    }

    tx.commit().await.map_err(|e| format!("提交事务失败: {}", e))?;
    Ok(())
}
```

---

## 四、前端改动

### 4.1 authStore.ts 扩展

```typescript
// 新增类型
export interface TenantInfo {
  id: number;
  tenant_name: string;
  schema_name: string | null;
  is_current: boolean;
}

// LoginResult 扩展
export interface LoginResult extends UserInfo {
  token: string;
  available_tenants: TenantInfo[];
}

// AuthState 扩展
interface AuthState {
  user: UserInfo | null;
  token: string | null;
  isLoggedIn: boolean;
  availableTenants: TenantInfo[];
  selectedTenantId: number | null;
  login: (result: LoginResult) => void;
  logout: () => void;
  switchTenant: (tenantId: number) => void;
  init: () => Promise<void>;
}
```

### 4.2 登录页（login/page.tsx）

登录成功后，如果 `available_tenants` 有值，自动将第一个或 `tenant_id` 对应的设为当前选中。

### 4.3 设置页（settings/page.tsx）

在现有设置项顶部新增租户切换器：

```tsx
// 仅 availableTenants.length > 1 时显示
<Card mb="md" withBorder padding="md">
  <Title order={4}>租户切换</Title>
  <Text size="sm" c="dimmed" mb="sm">
    切换后将查看不同租户的业务数据
  </Text>
  <Radio.Group value={String(selectedTenantId)} onChange={handleSwitch}>
    <Stack gap="xs">
      {availableTenants.map(t => (
        <Radio key={t.id} value={String(t.id)} label={t.tenant_name} />
      ))}
    </Stack>
  </Radio.Group>
</Card>
```

### 4.4 用户管理编辑表单

新增 MultiSelect 用于分配用户可访问的租户。

---

## 五、涉及文件清单

### 阶段 1：数据库 & Models（2 个文件）

| # | 文件 | 操作 |
|---|------|------|
| 1 | `src-tauri/src/database/sql/public_tables.sql` | 追加 `public.sys_user_tenant` 表定义 |
| 2 | `src-tauri/src/database/models.rs` | 新增 `SysUserTenant` + `TenantInfo` struct |

### 阶段 2：database 层重构（2 个文件）

| # | 文件 | 操作 |
|---|------|------|
| 3 | `src-tauri/src/database/postgres.rs` | 删除 `CURRENT_SCHEMA`、`get_current_schema()`、`set_current_schema()` |
| 4 | `src-tauri/src/database/postgres.rs` | 新增 `SCHEMA_CACHE`、`USER_TENANT_CACHE`、`get_schema_by_tenant_id()`、`preload_schema_cache()` |
| 5 | `src-tauri/src/database/mod.rs` | `schema_prefix()` 改为读取 `tokio::task_local!`；新增 `with_schema()` 工具函数 |

### 阶段 3：Middleware（新建 1 个文件）

| # | 文件 | 操作 |
|---|------|------|
| 6 | `src-tauri/src/middleware/auth.rs` | **新建** JWT 解析 + USER_TENANT_CACHE 查询 + UserContext 注入 |
| 7 | `src-tauri/src/api/mod.rs` 或 `lib.rs` | 注册 auth middleware 到 router |

### 阶段 4：Service 层（2 个文件）

| # | 文件 | 操作 |
|---|------|------|
| 8 | `src-tauri/src/service/user_service.rs` | `login()` 返回 `available_tenants`；`LoginResponse` 新增字段；登录后更新 `USER_TENANT_CACHE` |
| 9 | `src-tauri/src/service/tenant_service.rs` | `switch_tenant()` 增加 `user_id` 参数 + 权限校验 + 更新缓存 |
| 10 | `src-tauri/src/service/tenant_service.rs` | 新增 `assign_user_tenants()` |
| — | 其余 5 个业务 service | **不需要修改** |

### 阶段 5：Routes 层（3 个文件）

| # | 文件 | 操作 |
|---|------|------|
| 11 | `src-tauri/src/api/tenant_routes.rs` | 新增 `POST /api/tenants/switch` + `POST /api/tenants/assign` |
| 12 | `src-tauri/src/api/user_routes.rs` | 新增 `GET /api/users/{id}/tenants` 查询可访问租户 |
| 13 | `src-tauri/src/api/upload_routes.rs` | 删除 `get_current_schema()`，改为从 `Extension<UserContext>` 取 schema |

### 阶段 6：前端（4 个文件）

| # | 文件 | 操作 |
|---|------|------|
| 14 | `src/store/authStore.ts` | 扩展 `LoginResult`、新增 `availableTenants`、`selectedTenantId`、`switchTenant()` |
| 15 | `src/app/login/page.tsx` | 登录后初始化 `selectedTenantId` |
| 16 | `src/app/settings/page.tsx` | 新增租户切换 Radio |
| 17 | 用户编辑表单组件 | 新增租户 MultiSelect 多选分配 |

### 阶段 7：服务启动初始化（1 个文件）

| # | 文件 | 操作 |
|---|------|------|
| 18 | `src-tauri/src/database/postgres.rs` 中 `init_postgres_database()` | 最后增加 `preload_schema_cache(&pool)` 调用 |
| 19 | `src-tauri/src/database/postgres.rs` | 删除最后的 `set_current_schema()` 调用（不再需要） |

---

## 六、核心优势

| 对比项 | 旧方案（全局变量 + service 改签名） | 本方案（DashMap + task_local） |
|--------|-----------------------------------|-------------------------------|
| 并发安全 | ❌ 全局变量线程不安全 | ✅ `tokio::task_local!` 请求级隔离 |
| Service 签名改动 | 44 个函数要加 `&str schema` | **0 个函数改动** |
| 路由层改动 | 每个路由都要解析 tenant_id | middleware 统一处理 |
| 前端改动 | 每个请求都要带 tenant_id | **不需要传任何租户参数** |
| 缓存预热 | 无 | DashMap 启动预加载 |
| 实现复杂度 | 高（改动面大） | **低** |
| 工作量 | ~6h | **~4h** |

---

## 七、实施计划

| # | 阶段 | 内容 | 文件数 | 工时 |
|---|------|------|--------|------|
| 1 | 数据库 | 追加 sys_user_tenant 表 + models | 2 | 0.5h |
| 2 | database 层 | 删除全局变量 + 新增 DashMap + task_local | 2 | 1h |
| 3 | Middleware | 新建 auth middleware（JWT + 缓存查询） | 2 | 1h |
| 4 | Service 层 | user_service 增强 + tenant_service 增强 | 2 | 1h |
| 5 | Routes 层 | tenant_routes + user_routes + upload_routes | 3 | 0.5h |
| 6 | 前端 | authStore + login + settings + 用户编辑 | 4 | 1.5h |
| 7 | 启动初始化 | preload_cache + 清理旧调用 | 1 | 0.3h |
| 8 | 编译验证 + 测试 | cargo build + npm build | — | 0.5h |
| | **总计** | | **~16** | **~6.3h** |

### 实施顺序

```
第1步：SQL + Models（不会影响编译）
     ↓
第2步：database 层重构（postgres.rs + mod.rs）
     ↓
第3步：Middleware（新建 auth.rs）
     ↓
第4步：Service 层增强（user_service + tenant_service）
     ↓
第5步：Routes 层改动（tenant_routes + upload_routes）
     ↓
第6步：前端改动（authStore + login + settings）
     ↓
第7步：编译验证 → 测试
```

> **注意**：第 2 步删除全局 `CURRENT_SCHEMA` 后，`upload_routes.rs` 和 `init_postgres_database()` 中调用 `get_current_schema()` / `set_current_schema()` 的地方会编译报错，在第 5 步修复。