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

### 改动清单
- [ ] 17. **数据库表 `file_uploads` 扩展**（`tenant_tables.sql`）：新增 `context_type VARCHAR(50)`、`context_id BIGINT`、`commit_at TIMESTAMP` 字段；status 增加 `pending`/`committed` 状态
- [ ] 18. **Rust `FileUploadRecord` 模型扩展**（`storage/upload.rs`）：新增 `context_type`、`context_id` 字段；状态机改为 pending → uploading → completed → committed
- [ ] 19. **`UploadManager` 重构**（`storage/upload.rs`）：`init()` 改为只占位（status=pending，不调 S3）；新增 `start_upload()`（pending→uploading，创建 S3 MultipartUpload）；`complete()` 只改 status=completed（不自动关联）；新增 `commit()`（completed→committed，关联 context）
- [ ] 20. **Tauri 命令新增**（`commands/upload_commands.rs`）：新增 `commit_upload` 命令
- [ ] 21. **前端 `uploadService.ts` 改造**：`init()` 新增 `context` 参数；新增 `startUpload()`、`commit()` 方法
- [ ] 22. **前端 `useChunkedUpload.ts` 改造**：`options` 新增 `context` 参数；`start()` 流程改为 init(占位) → startUpload(创建S3) → 并发上传 → complete → commit
- [ ] 23. **前端 `FileUploader.tsx` 改造**：`props` 新增 `context` 属性，透传到 `useChunkedUpload`
- [ ] 24. **知识库页面集成测试**：在上传文件时传入 `context={{ type: 'knowledge', id: selectedNodeId }}`
