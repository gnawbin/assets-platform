# 前端大整数精度丢失问题（Snowflake ID）

## 问题描述

系统使用 Snowflake 算法生成主键 ID（如 `1825529876933619712`），这些 ID 是 64 位整数。JavaScript 的数字类型（Number）基于 IEEE 754 双精度浮点数，能够安全表示的整数范围是 `[-2^53, 2^53]`，即 `[-9007199254740991, 9007199254740991]`。

Snowflake ID 通常有 18-19 位数字（例如 `1825529876933619712`），已经超出了 JS Number 的安全整数范围（16 位）。因此使用 `Number()` 直接转换 ID 字符串会导致精度丢失。

### 错误示例

```typescript
// ❌ 错误：Number() 会导致精度丢失
const userId = Number(user.id);       // 1825529876933619712 → 1825529876933619700
await getUserTenants(userId);         // 传给后端的 userId 在数据库中不存在
```

## 解决方案

### 后端：始终使用 `i64_to_string` 序列化 ID

Rust 后端模型中的所有 ID 字段都使用 `i64_to_string` / `opt_i64_to_string` 序列化器，将 i64 序列化为字符串再传给前端：

```rust
pub fn i64_to_string<S>(value: &i64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}
```

### 前端：禁止使用 `Number()` 转换 ID

前端接收到的 ID 是 string 类型（尽管 TypeScript 的类型定义写的是 `number`），**永远不要使用 `Number()` 转换**：

```typescript
// ✅ 正确：直接传递原始 ID
const userTenants = await getUserTenants(user.id);

// ✅ 正确：需要字符串时使用 String()
String(user.id);
```

### 所有 Service 函数：统一通过 `String()` 序列化

所有 Service 层函数应该接收 `number | string` 类型，内部统一 `String()` 转换：

```typescript
export function assignUserTenants(userId: number | string, ...) {
    return api.post('assign_user_tenants', {
        userId: String(userId),  // 确保发送给后端的是字符串
        ...
    });
}
```

### 后端 Command：接收 String 再解析为 i64

Tauri Command 函数接收字符串参数，解析为 i64：

```rust
#[tauri::command]
pub async fn get_user_tenants(userId: String) -> Result<Vec<TenantResponse>, String> {
    let user_id: i64 = userId.parse().map_err(|e| format!("无效的用户ID: {}", e))?;
    // ...
}
```

## 总原则

| 场景 | 做法 |
|------|------|
| 从后端（Tauri invoke）接收数据 | 直接使用返回的 ID 值（TypeScript 类型可定义为 number，但运行时实际是 string） |
| 向前端 state 存储 ID | 保持原值，不做 `Number()` 转换 |
| 向前端 Service 传递 ID | 直接传递，Service 内部用 `String()` 统一序列化 |
| 向 Tauri Command 传递参数 | 用 `String()` 包裹确保字符串类型 |
| 计算或比较 ID | 使用 `String(id) === String(otherId)`，不要用 `===` 直接比较（可能 number vs string） |

## 受影响文件

- `src-tauri/src/database/models.rs` - `i64_to_string` / `opt_i64_to_string` 序列化器
- 所有 Service TS 文件（`src/services/*.ts`） - 参数类型应为 `number | string`，内部 `String()` 转换
- 所有后端 Command 文件（`src-tauri/src/commands/*.rs`） - 接收 String 参数，parse 为 i64