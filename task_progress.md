# 任务进度

## 已完成
- [x] 分析代码，确认问题
- [x] 修改 `role_commands.rs` - insert_role 改为接收独立参数
- [x] 修改 `permissionService.ts` - 调整 insertRole 调用方式
- [x] 修改 `permissions/page.tsx` - 移除超级管理员选项，tenant_id 必填

## 知识树模块（已完成）
- [x] 1. 数据库：修改 asset_knowledge 表（asset_id 改为可选）+ 新增 knowledge_tree 表
- [x] 2. Rust 数据模型：新增 KnowledgeTree 等模型
- [x] 3. Rust Service：knowledge_service.rs
- [x] 4. Rust Command：knowledge_commands.rs
- [x] 5. Rust API：knowledge_routes.rs
- [x] 6. 注册 Command 和路由到 lib.rs 和 api/mod.rs
- [x] 7. 前端 Service：knowledgeService.ts
- [x] 8. 前端页面：知识树页面（左侧树 + 右侧内容区）
- [x] 9. 侧边栏菜单：添加知识库菜单项（Sidebar.tsx + public_initial_data.sql）


