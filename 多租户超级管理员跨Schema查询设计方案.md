# 多租户数据权限与租户切换设计方案

> 解决普通用户可分配多个租户、超级管理员可自由切换租户的问题
> 基于当前 PostgreSQL Schema 隔离架构

---

## 一、问题现状

### 当前架构

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

### 现有问题

| 问题 | 说明 |
|------|------|
| `sys_user.tenant_id` 是单值字段 | 一个用户只能关联一个租户 |
| 普通用户没有数据权限概念 | 目前只能看到自己 `tenant_id` 对应的那个 schema |
| 超级管理员视角也受限 | 虽然能调 `switch_tenant()`，但没有界面操作 |

---

## 二、需求

### 2.1 数据权限

| 角色 | 可访问的租户 |
|------|-------------|
| **超级管理员** | 所有租户（`enable=true`） |
| **普通管理员/用户** | 被分配的 1 到 N 个指定租户（通过关联表配置） |

### 2.2 交互方式

不在每个页面加租户选择器，只在 **用户设置/用户信息区域** 提供一个租户切换 radio/select，切换后：
1. 后端调用 `set_current_schema()`
2. 前端页面数据自动切换到新租户

---

## 三、设计方案

### 概要

```
用户登录 → 查询用户可访问的租户列表
         ↓
返回 available_tenants（登录响应中携带）
         ↓
前端设置页 → 租户选择器 radio
         ↓ 切换
调 switch_tenant API（带权限校验）
         ↓
set_current_schema("tenant_b")
         ↓
所有业务查询通过 schema_prefix() 自动走新 schema
```

**无需修改任何业务 SQL！** `schema_prefix()` 重构已经铺好路了。

---

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

### 3.2 后端：登录接口增强

**LoginResponse 增加字段：**

```rust
pub struct LoginResponse {
    // ... 现有字段
    pub available_tenants: Vec<TenantInfo>,  // 新增
}

pub struct TenantInfo {
    pub id: i64,
    pub tenant_name: String,
    pub schema_name: Option<String>,
    pub is_current: bool,  // 是否是当前登录的租户
}
```

**登录逻辑改动（`user_service.rs:login`）：**

```rust
// 查询用户可访问的租户列表
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
```

### 3.3 后端：switch_tenant 增加权限校验

```rust
pub async fn switch_tenant(user_id: i64, tenant_id: i64) -> Result<String, String> {
    let pool = get_write_pool().map_err(|e| format!("数据库连接失败: {}", e))?;

    // 1. 检查用户是否是超级管理员
    let is_super_admin: bool = sqlx::query_scalar(
        "SELECT is_super_admin FROM public.sys_user WHERE id = $1"
    )
    .bind(user_id)
    .fetch_optional(&pool)?
    .ok_or("用户不存在")?;

    // 2. 校验权限
    if !is_super_admin {
        let has_access: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM public.sys_user_tenant WHERE user_id = $1 AND tenant_id = $2)"
        )
        .bind(user_id)
        .bind(tenant_id)
        .fetch_one(&pool)?
        .unwrap_or(false);

        if !has_access {
            return Err("无权访问该租户".to_string());
        }
    }

    // 3. 切换 schema（原有逻辑不变）
    // ... 查询 schema_name → set_current_schema() ...
}
```

### 3.4 后端：用户管理增加租户分配功能

在用户表单中支持分配多个租户（多选）：

```rust
/// 为用户分配租户（覆盖式）
pub async fn assign_user_tenants(user_id: i64, tenant_ids: &[i64]) -> Result<(), String> {
    let pool = get_write_pool()...;

    // 事务：删除旧关联 → 插入新关联
    let mut tx = pool.begin().await...;

    sqlx::query("DELETE FROM public.sys_user_tenant WHERE user_id = $1")
        .bind(user_id).execute(&mut *tx).await...;

    for tenant_id in tenant_ids {
        sqlx::query(
            "INSERT INTO public.sys_user_tenant (id, user_id, tenant_id, created_by)
             VALUES ($1, $2, $3, $4)"
        )
        .bind(next_id() as i64)
        .bind(user_id)
        .bind(tenant_id)
        .bind(current_user_id)
        .execute(&mut *tx).await...;
    }

    tx.commit().await...;
    Ok(())
}
```

---

## 四、涉及文件清单

### 阶段 1：数据库

| 文件 | 操作 |
|------|------|
| `src-tauri/src/database/sql/public_tables.sql` | 追加 `public.sys_user_tenant` 表定义 |
| `src-tauri/src/database/models.rs` | 新增 `SysUserTenant` struct |

### 阶段 2：Service 层

| 文件 | 操作 |
|------|------|
| `user_service.rs` | 登录接口返回 `available_tenants` |
| `tenant_service.rs` | `switch_tenant()` 增加权限校验参数 |
| `tenant_service.rs` | 新增 `assign_user_tenants()` 分配租户 |

### 阶段 3：路由/命令层

| 文件 | 操作 |
|------|------|
| `user_routes.rs` / `user_commands.rs` | `switch_tenant` 传递 `user_id` |
| （用户管理路由） | 新增分配租户接口 |

### 阶段 4：前端

| 文件 | 操作 |
|------|------|
| `src/app/settings/page.tsx` | 在设置页添加租户切换 radio 选择器 |
| 用户管理编辑表单 | 添加可选租户多选框 |

---

## 五、核心优势

| 对比项 | 旧方案（UNION ALL 视图） | 本方案（租户切换） |
|--------|------------------------|-------------------|
| 复杂度 | 13 个视图 + 13 个 `_all()` 函数 | 1 张关联表 + 1 个切换 API |
| schema_prefix() | 需要改 | **完全兼容，无需改动** |
| 数据修改 | 只读，无法跨租户改 | 切换后可正常增删改 |
| 用户理解 | 混合数据难理解 | 切换到哪个租户就看到哪个 |
| 工作量 | ~4.5h | **~2h** |

---

## 六、实施计划

| # | 内容 | 工作量 |
|---|------|--------|
| 1 | `public_tables.sql` 追加 `sys_user_tenant` 表 | 0.3h |
| 2 | `models.rs` 新增 `SysUserTenant` + `TenantInfo` | 0.3h |
| 3 | `user_service.rs` 登录接口返回 `available_tenants` | 0.5h |
| 4 | `tenant_service.rs` 增强 `switch_tenant()` 权限校验 | 0.3h |
| 5 | `tenant_service.rs` 新增 `assign_user_tenants()` | 0.3h |
| 6 | 用户管理路由 + 分配租户接口 | 0.5h |
| 7 | 前端设置页租户切换 select + 调用 API | 1h |
| 8 | 前端用户编辑表单租户多选 | 0.5h |
| 9 | 编译验证 + 测试 | 0.5h |

总预估：**4.2h**