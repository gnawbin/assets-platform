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
