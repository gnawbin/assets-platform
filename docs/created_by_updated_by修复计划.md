# `created_by` / `updated_by` 未保存当前操作用户 ID 修复计划

## 问题概述

系统中多个模块在创建/更新记录时，`created_by` 和 `updated_by` 字段未能正确记录当前操作用户的 ID。主要存在以下两类问题：

1. **硬编码** — 某些 service 层直接将值写为 `1i64`，与实际用户无关
2. **依赖前端传递** — 某些 command 层将 `createdBy`/`updatedBy` 设为可选参数（`Option`），前端不传则写 `null` 或 `None`，不做服务端自动填充
3. **HTTP API 路由未利用 UserContext** — `auth_middleware` 已经通过 JWT 解析出当前用户 ID 并注入到请求扩展中，但路由处理器没有提取使用

---

## 涉及模块及具体问题

### 一、固定资产模块（`assets_service.rs` + `asset_routes.rs` + `asset_commands.rs`）

**Service 层 — `assets_service.rs`：**

所有 `created_by` / `updated_by` 被**硬编码为 `1i64`**：

| 方法 | 位置 | 硬编码值 |
|------|------|---------|
| `insert_hardware_asset()` | assets 主表 INSERT | `$16 -> .bind(1i64)` |
| `insert_hardware_asset()` | hard_assets 扩展表 INSERT | `$13 -> .bind(1i64)` |
| `update_hardware_asset()` | assets 主表 UPDATE | `$15 -> .bind(1i64)` |
| `update_hardware_asset()` | hard_assets 扩展表 UPDATE | `$12 -> .bind(1i64)` |
| `update_hardware_asset()` | hard_assets 扩展表 INSERT（不存在时） | `$13 -> .bind(1i64)` |
| `insert_intangible_asset()` | assets 主表 INSERT | `$16 -> .bind(1i64)` |
| `insert_intangible_asset()` | intangible_assets 扩展表 INSERT | `$22 -> .bind(1i64)` |
| `update_intangible_asset()` | assets 主表 UPDATE | `$15 -> .bind(1i64)` |
| `update_intangible_asset()` | intangible_assets 扩展表 UPDATE | `$21 -> .bind(1i64)` |
| `update_intangible_asset()` | intangible_assets 扩展表 INSERT（不存在时） | `$22 -> .bind(1i64)` |

**Input 结构体缺少 `created_by` / `updated_by` 字段：**

- `HardwareAssetInput` — 没有任何 user_id 相关字段
- `IntangibleAssetInput` — 同上
- `HardwareAssetView` / `IntangibleAssetView` — 虽然有 `created_by` / `updated_by`，但只是用于查询展示

**HTTP API 路由层 — `asset_routes.rs`：**

- `insert_hardware_asset()` / `update_hardware_asset()` / `insert_intangible_asset()` / `update_intangible_asset()` 均未从 `Extension<UserContext>` 提取 `user_id`
- 直接传递 input 到 service 层，没有注入当前用户信息

**Tauri Command 层 — `asset_commands.rs`：**

- `insert_hardware_asset()` / `update_hardware_asset()` / `insert_intangible_asset()` / `update_intangible_asset()` 同样直接传递 input，没有从上下文中获取当前用户 ID

---

### 二、知识资产模块（`knowledge_asset_service.rs` + `knowledge_asset_commands.rs`）

**Service 层 — `knowledge_asset_service.rs`：**

| 方法 | 问题 |
|------|------|
| `create_knowledge_asset()` | 依赖传入的 `asset.created_by` 值，不做服务端强制填充 |
| `update_knowledge_asset()` | 依赖传入的 `updated_by` 参数，如果不传则为 `None`，不更新 updated_by |
| `attach_file_to_asset()` | 完全没有设置 `updated_by`，只更新了文件相关字段和 `updated_at` |
| `delete_knowledge_asset()` | 没有设置 `updated_by`（但软删除通常不需要保留更新人，可接受） |

**Tauri Command 层 — `knowledge_asset_commands.rs`：**

- `create_knowledge_asset()` 中 `createdBy` 参数为 `Option<String>`，前端不传则为 `None`
- `update_knowledge_asset()` 中 `updatedBy` 参数为 `Option<String>`，前端不传则为 `None`
- `attach_file_to_knowledge()` 没有传递 `updated_by` 参数

---

### 三、知识树模块（`knowledge_commands.rs`）

根据 search 结果，`knowledge_commands.rs` 中的 `insert_knowledge_node` 和 `update_knowledge_node` 同样接受 `created_by` / `updated_by` 作为 `Option<String>` 参数，前端不传则不填充。

---

### 四、知识条目模块（`knowledge_service.rs` 中的 `insert_knowledge` / `update_knowledge`）

同样接受 `created_by` / `updated_by` 作为 `Option<String>` 参数，前端不传则不填充。

---

### 五、部门管理模块（`department_commands.rs` + `department_routes.rs`）

**Tauri Command 层 — `department_commands.rs`：**

- `insert_department()` 接受 `created_by: Option<i64>` 参数
- `update_department()` 接受 `updated_by: Option<i64>` 参数

**HTTP API 路由层 — `department_routes.rs`：**

待确认是否从 `Extension<UserContext>` 提取了 `user_id`。

---

### 六、分类管理模块（`category_commands.rs` + `category_routes.rs`）

同上，待确认是否从 `Extension<UserContext>` 提取了 `user_id`。

---

### 七、流程管理模块（`process_commands.rs` + `process_routes.rs`）

所有流程（领用、归还、调拨、维修、报废、采购）的 INSERT/UPDATE 操作中，`created_by` / `updated_by` 同样可能依赖前端传递。

---

### 八、LLM 厂商配置模块（`llm_provider_commands.rs`）

`create_llm_provider()` 和 `update_llm_provider()` 同样接受 `created_by` / `updated_by` 作为可选参数。

---

### 九、大文件上传模块（`upload_commands.rs`）

第 18 行存在硬编码：
```rust
let created_by: i64 = 1;
```

---

## 修复方案

### 总体原则

1. **服务端负责填充当前用户 ID** — 禁止依赖前端传递 `created_by` / `updated_by`
2. **HTTP API 路由** — 从 `Extension<UserContext>` 提取 `user_id`，传入 service 层
3. **Tauri Command** — 通过 Tauri 的 AppHandle state 或登录态缓存获取当前用户 ID，或要求前端必须传递（改为非 `Option`）

### 第一阶段：修复 Service 层

#### 1.1 `assets_service.rs`

为所有插入/更新方法添加 `current_user_id: i64` 参数：

```rust
pub async fn insert_hardware_asset(
    input: HardwareAssetInput,
    current_user_id: i64,
) -> Result<HardwareAssetView, String>
```

移除所有 `.bind(1i64)`，替换为 `.bind(current_user_id)`。

同时，在 `insert` 语句中 `created_by` 和 `updated_by` 都设为 `current_user_id`，在 `update` 语句中只更新 `updated_by = current_user_id`。

#### 1.2 `knowledge_asset_service.rs`

- `create_knowledge_asset()` — 将 `asset.created_by` 改为必填（移除 service 层的 `Option` 语义）
- `update_knowledge_asset()` — 将 `updated_by` 改为必填（`i64` 而非 `Option<i64>`）
- `attach_file_to_asset()` — 添加 `updated_by: i64` 参数

#### 1.3 其他模块 Service

各部门、分类、流程、LLM 等模块的 service 方法同样添加 `current_user_id: i64` 参数。

### 第二阶段：修复 HTTP API 路由层

#### 2.1 `asset_routes.rs`

在每个处理器中通过 axum 的 `Extension` 提取 `UserContext`：

```rust
use axum::Extension;
use crate::api::auth::UserContext;

pub async fn insert_hardware_asset(
    Extension(ctx): Extension<UserContext>,
    Json(input): Json<HardwareAssetInput>,
) -> Result<Json<ApiResponse<HardwareAssetView>>, ApiError> {
    // 将 ctx.user_id 传入 service
    match service::assets_service::insert_hardware_asset(input, ctx.user_id).await {
        // ...
    }
}
```

#### 2.2 `department_routes.rs` / `category_routes.rs` / `process_routes.rs` 等

同上，从 `Extension<UserContext>` 提取 `user_id` 并传入 service 层。

### 第三阶段：修复 Tauri Command 层

#### 3.1 获取当前用户 ID 的策略

Tauri v2 中，可以通过以下方式获取当前登录用户：

1. **方案 A（推荐）**：在登录成功后，将当前用户信息缓存在一个全局的 `CURRENT_USER`（`Arc<Mutex<Option<UserInfo>>>`）中，作为 Tauri State 管理。Command 中通过 `tauri::State` 获取。
2. **方案 B（临时）**：将 `createdBy` / `updatedBy` 参数改为必填（`String` 而非 `Option<String>`），由前端传入。

建议使用方案 A，因为前端已经有一个 `authStore` 存储了当前用户信息，登录后可以通过 Tauri invoke 设置后端状态。

#### 3.2 具体修改

- `knowledge_asset_commands.rs` — 从 state 获取当前用户 ID，强制填充 `created_by` / `updated_by`
- `knowledge_commands.rs` — 同上
- `department_commands.rs` / `category_commands.rs` / `process_commands.rs` / `llm_provider_commands.rs` — 同上
- `upload_commands.rs` — 使用 state 中的当前用户 ID 替换硬编码 `1`

### 第四阶段：前端配合修改

1. **Tauri 初始加载时** — 在登录成功或应用启动时，通过 `invoke('set_current_user', { userId: ... })` 将当前用户 ID 传给后端
2. **移除前端对 createdBy/updatedBy 的手动传递**（如果现有前端代码在传这些参数）

---

## 受影响文件清单

### Service 层
| 文件 | 修改内容 |
|------|---------|
| `src-tauri/src/service/assets_service.rs` | 添加 `current_user_id` 参数，移除硬编码 |
| `src-tauri/src/service/knowledge_asset_service.rs` | `attach_file_to_asset()` 添加 `updated_by`，其他方法改为必填 |
| `src-tauri/src/service/department_service.rs` | 添加 `current_user_id` 参数 |
| `src-tauri/src/service/category_service.rs` | 添加 `current_user_id` 参数 |
| `src-tauri/src/service/process_service.rs` | 添加 `current_user_id` 参数 |
| `src-tauri/src/service/llm_provider_service.rs` | 添加 `current_user_id` 参数 |
| `src-tauri/src/service/user_service.rs` | 确保 `created_by` / `updated_by` 正确传递 |

### HTTP API 路由层
| 文件 | 修改内容 |
|------|---------|
| `src-tauri/src/api/asset_routes.rs` | 从 `Extension<UserContext>` 提取 `user_id` |
| `src-tauri/src/api/department_routes.rs` | 同上 |
| `src-tauri/src/api/category_routes.rs` | 同上 |
| `src-tauri/src/api/process_routes.rs` | 同上 |

### Tauri Command 层
| 文件 | 修改内容 |
|------|---------|
| `src-tauri/src/commands/knowledge_asset_commands.rs` | 从 state 获取当前用户，强制填充 |
| `src-tauri/src/commands/knowledge_commands.rs` | 同上 |
| `src-tauri/src/commands/department_commands.rs` | 同上 |
| `src-tauri/src/commands/category_commands.rs` | 同上 |
| `src-tauri/src/commands/process_commands.rs` | 同上 |
| `src-tauri/src/commands/llm_provider_commands.rs` | 同上 |
| `src-tauri/src/commands/user_commands.rs` | 同上 |
| `src-tauri/src/commands/upload_commands.rs` | 替换硬编码 `1` 为当前用户 |
| `src-tauri/src/commands/role_commands.rs` | 替换 `.bind(Some(1))` 为当前用户 |

### 新增文件
| 文件 | 内容 |
|------|------|
| `src-tauri/src/commands/current_user_state.rs` | （建议）定义全局当前用户 State |

---

## 实施顺序

1. **第 1 步** — 新增 `CurrentUserState` 全局状态（Tauri State 管理）
2. **第 2 步** — 新增 `set_current_user` / `clear_current_user` command
3. **第 3 步** — 修复 `assets_service.rs`（硬编码最严重）
4. **第 4 步** — 修复 `asset_routes.rs`（HTTP API 路由）
5. **第 5 步** — 修复 `knowledge_asset_service.rs` + `knowledge_asset_commands.rs`
6. **第 6 步** — 修复 `upload_commands.rs`（硬编码）
7. **第 7 步** — 修复 `department_service.rs` + routes + commands
8. **第 8 步** — 修复 `process_service.rs` + routes + commands
9. **第 9 步** — 修复 `llm_provider_service.rs` + commands
10. **第 10 步** — 修复 `user_commands.rs` / `role_commands.rs`
11. **第 11 步** — 前端适配（登录成功后调用 `set_current_user`）