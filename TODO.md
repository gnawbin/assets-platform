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

### file_uploads 表文件版本号设计

通过 `file_group_id` 将同一文件的不同版本归组，`version` 标记版本号。

**新增字段：**
| 字段 | 类型 | 说明 |
|------|------|------|
| `file_group_id` | `VARCHAR(36)` | UUID，同一文件不同版本共用（如替换 report.pdf 时用同一 group_id） |
| `version` | `INTEGER DEFAULT 1` | 版本号，从 1 开始递增 |
| `is_latest` | `BOOLEAN DEFAULT true` | 是否为当前最新版本 |
| `change_reason` | `VARCHAR(500)` | 变更原因，如"更新合同条款" |
| `file_md5` | `VARCHAR(64)` | 文件 MD5，用于判断是否真的变更 |
| `UNIQUE (file_group_id, version)` | 约束 | 同一组内版本号唯一 |

**版本替换流程：**
```
第一次上传：file_group_id = "uuid", version = 1, is_latest = true
第二次替换：step1: UPDATE is_latest=false; step2: INSERT version=2, is_latest=true
```

**S3 Object Key：**
```
旧：uploads/{YYYY-MM}/{uuid}-{filename}
新：uploads/{YYYY-MM}/{file_group_id}/v{version}/{uuid}-{filename}
```

**新增 API：**
- `get_version_history(file_group_id)` → 返回版本列表
- `rollback_to_version(file_group_id, target_version)` → 回滚到指定版本
- 清理策略：保留最近 N 个版本，软删除/硬删除旧版本

### 涉及的表

| 表 | 位置 | 操作 | 说明 |
|----|------|------|------|
| `file_uploads` | `tenant_tables.sql` | ✅ **直接修改** | 新增版本字段（file_group_id/version/is_latest/change_reason/file_md5）+ context 字段（context_type/context_id/commit_at）+ status 增加 pending/committed。**file_uploads 本身就是附件表**，一个业务实体可以通过 context_id 关联多个上传文件，每个文件支持多版本 |
| `knowledge_asset` | `tenant_tables.sql` | 🔗 **仅作为 context 目标** | 通过 `file_uploads.context_id = knowledge_asset.id` 实现一对多关系 |
| `knowledge_tree` | `tenant_tables.sql` | 🔗 间接关联 | 链路：`file_uploads.context_id → knowledge_asset.id → knowledge_tree.id` |
| `asset_documents` | `tenant_tables.sql` | 📌 未来可复用 | 同一套 file_uploads 机制可用于资产文档附件上传 |

### 实施优先级

按依赖关系分为 4 个阶段，每阶段可独立上线：

#### Phase 1：后端基础设施（先做，3 项）
依赖链：DB → Rust 模型 → S3 Key

- [ ] **P1-17.** 数据库表 `file_uploads` 扩展现有字段（`tenant_tables.sql`）
- [ ] **P1-18.** Rust `FileUploadRecord` 模型扩展（`storage/upload.rs`）
- [ ] **P1-25.** S3 Object Key 生成逻辑修改（`storage/upload.rs`）

#### Phase 2：后端核心逻辑（上述完成后，2 项）
依赖链：UploadManager → Tauri 命令

- [ ] **P2-19.** `UploadManager` 重构（`storage/upload.rs`）：init/start_upload/complete/commit 方法拆分 + 版本逻辑
- [ ] **P2-20.** Tauri 命令新增（`commands/upload_commands.rs`）：commit_upload、get_version_history、rollback_version

#### Phase 3：前端基础设施（上述完成后，2 项）
依赖链：Service → Hook

- [ ] **P3-21.** 前端 `uploadService.ts` 改造：新增参数和方法
- [ ] **P3-22.** 前端 `useChunkedUpload.ts` 改造：新增参数，流程改为两步提交

#### Phase 4：前端 UI 集成（上述完成后，2 项）
依赖链：Component → 页面集成

- [ ] **P4-23.** 前端 `FileUploader.tsx` 改造：新增 context/fileGroupId/changeReason 属性
- [ ] **P4-24.** 知识库页面集成测试：传入 context，验证完整上传链路
