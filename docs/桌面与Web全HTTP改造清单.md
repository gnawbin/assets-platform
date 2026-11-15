# 桌面与 Web 全 HTTP 化改造清单

> 状态：**待实施**（本文件为施工依据，随实施进度回填）
> 决策：**桌面端数据源统一走 HTTP**，拓扑选定 **T2「远程服务端」**——桌面退化为纯 WebView 壳。
> 目标：Web 与桌面共用同一套 HTTP API；**前端只负责页面渲染与交互，不再承担平台差异与数据通道切换**。
> 关联文档：`docs/桌面与Web双形态部署指南.md`、`docs/架构问题与缺陷分析及整改排期.md`（B2-1 单 API 面）

---

## 0. 决策与结论摘要

### 0.1 已定决策

| 编号 | 决策 | 结论 | 影响 |
|---|---|---|---|
| D1 | 工作流 10 条命令是否纳入首批 | **纳入** | Web 模式当前该模块直接报错，属"已坏未报" |
| D2 | RAG 2 条 + `get_llm_provider` 是否本期 | **推迟到 P2** | 前端无调用点 |
| D3 | 沙箱 10 条是否补前端映射 | **不补** | 后端 11 条路由已就绪，前端零调用，补了是死代码 |
| D4 | 适配器是否保留 Tauri 降级 | **彻底删除** | 前端 0 处使用 dialog/fs/updater |
| D5 | 桌面数据源 | **全 HTTP，T2 远程服务端** | 桌面不再内嵌 axum / DB / S3 / doc-parser |

### 0.2 现状量化（实测，非估算）

| 指标 | 数量 | 出处 |
|---|---|---|
| 后端 `invoke_handler` 注册命令 | **154** | `apps/backend/src-tauri/src/lib.rs` |
| 前端 HTTP 路由映射条目 | **112** | `apps/web/src/utils/adapters/httpAdapter.ts:57-216` |
| 其中无 Tauri 对应（HTTP-only） | **10** | API Key 4 + OAuth 客户端 4 + `logout` + `refresh_token` |
| 已 HTTP 化的 Tauri 命令 | **102** | 112 − 10 |
| 明确标注"仅 Tauri" | **32** | `httpAdapter.ts:234-278`（`TAURI_ONLY_COMMANDS`） |
| **隐式缺口**（既无映射也不在名单） | **20** | 工作流 10 + 沙箱 10 → Web 模式抛「未找到命令」 |
| 校验：102 + 32 + 20 | **154** ✅ | 与注册数一致 |
| **需新增 HTTP 路由** | **42** | 32 + 工作流 10 |
| 其中 UI 实际调用 | **39** | 排除 `chunk_and_vectorize`、`test_rag_retrieval`、`get_llm_provider` |

### 0.3 三条核心结论

1. **全 HTTP 是唯一能同时修好两种形态的路线**：桌面 invoke 模式下 API Key / OAuth 客户端 / `logout` / `refresh_token` 共 10 个能力**今天就是坏的**（无对应 Tauri 命令）；Web 模式下工作流模块**今天就是坏的**（无路由映射）。
2. **42 条新路由零业务逻辑**：每条都是「取 `Extension<UserContext>` → 调既有 service → 返回 `ApiResponse`」，service 层无需改动。
3. **前端可以真正做到"只做页面"**：删除 4 个适配器文件、1 个平台探测函数、1 份手抄映射表、1 个 Tauri 上传实现。

---

## 1. 三方比对结果

### 1.1 复核命令（可自行重跑）

```bash
# 后端注册的全部命令
sed -n '/generate_handler!\[/,/^        \])/p' apps/backend/src-tauri/src/lib.rs \
  | grep -o 'commands::[a-z_0-9]*::[a-z_0-9]*' | sed 's/.*:://' | sort -u > /tmp/cmds.txt

# 前端映射表 / Tauri-only 名单
sed -n '/^export const COMMAND_ROUTE_MAP/,/^};/p' \
  apps/web/src/utils/adapters/httpAdapter.ts \
  | grep -oE '^    [A-Za-z0-9_]+:' | tr -d ' :' | sort -u > /tmp/map.txt
sed -n '/^export const TAURI_ONLY_COMMANDS/,/^\]);/p' \
  apps/web/src/utils/adapters/httpAdapter.ts \
  | grep -oE "'[A-Za-z0-9_]+'" | tr -d "'" | sort -u > /tmp/only.txt

# 差异
comm -23 /tmp/cmds.txt <(sort -u /tmp/map.txt /tmp/only.txt)   # 隐式缺口（20 条）
comm -13 /tmp/cmds.txt /tmp/map.txt                            # HTTP-only 映射（10 条）
```

实测输出：`cmds=154 / map=112 / only=32`。

### 1.2 已 HTTP 化，无需改动（102 条）

按 `httpAdapter.ts` 的 12 个分节组织：认证、用户、角色菜单、部门、租户、资产（硬件/无形）、分类、流程管理（领用/归还/调拨/维修/报废/采购，各 4 条 = 24）、知识树与条目（10）、技能（6）、运维连接、上传（7）、API Key（4）、OAuth 客户端（4）。逐条清单见 `httpAdapter.ts:57-216`。

> 桌面切到 HTTP 后**零改动**自动复用。

### 1.3 需新增 HTTP 路由（42 条）

通用约束（适用于以下全部条目）：

- 用户身份一律取 `Extension<UserContext>`，**忽略请求体里的 `userId`**（范式见 `crates/api/src/chat_routes.rs:15`）；
- 返回统一包一层 `ApiResponse<T>`（前端 `httpAdapter.ts:581-585` 会自动解包 `data`）；
- 每个路由必须同步登记 `crates/api/src/authz.rs` 的 `ROUTE_PERMS` 与 `public_migration.sql` 的 `sys_menu`，否则 `AUTHZ_MODE=enforce` 下 403。

#### 1.3.1 智能问答 — 会话管理（6 条）— ✅ 已实施

| 命令 | 前端调用点 | 目标路由 | 状态 |
|---|---|---|---|
| `create_conversation` | `services/conversationService.ts:87` | `POST /api/conversations` | ✅ |
| `get_conversations` | `services/conversationService.ts:109` | `GET /api/conversations`（query `page,pageSize`） | ✅ |
| `get_conversation_messages` | `services/conversationService.ts:118` | `GET /api/conversations/{convId}/messages`（query `limit`） | ✅ |
| `update_conversation_title` | `services/conversationService.ts:126` | `PUT /api/conversations/{convId}/title`（body `{title}`） | ✅ |
| `delete_conversation` | `services/conversationService.ts:131` | `DELETE /api/conversations/{convId}` | ✅ |
| `send_message` | `services/conversationService.ts:100` | `POST /api/conversations/{convId}/messages`（非流式，body 同命令参数） | ✅ |

> 流式问答已有 `POST /api/chat/stream`（`crates/api/src/chat_routes.rs`），本次不重复建设。
> **越权要求**：会话读取/删除必须按 `user_id` 过滤；`send_message` 需校验会话归属。

**归属校验落地方式（服务端唯一闸门）**

| 主题 | 实现 | 位置 |
|---|---|---|
| 身份来源 | 只用令牌 `sub`；body/query 的 `userId` 仅比对后告警，**不采信、不因此报错**（兼容桌面老前端） | `conversation_routes.rs::resolve_user_id` |
| 归属闸门 | `ensure_conversation_owned(conv_id, user_id)` 一处校验，6 个 handler 共用 | `service/src/conversation_service.rs` |
| 区分「不存在」与「DB 错误」 | `find_conversation()` 返回 `Option<Conversation>`，`None` → 404 | 同上 |
| 防 TOCTOU | 改标题 / 删除把 `user_id` 折进**同一条 UPDATE 的 WHERE**（`id AND user_id AND deleted=0`），`rows_affected()==0` → 404 | `update_conversation_title_for_user`、`delete_conversation_for_user` |
| 防探测 | 他人会话与本就不存在的会话**返回同一段文案 + 404**（不用 403），避免枚举他人会话 id | `CONVERSATION_NOT_FOUND` |
| 权限点 | `GET` 列表/消息 → `chat:conversation:list`；`POST` 建会话/发消息 → `chat:completion:add`；`PUT` 标题 → `chat:conversation:edit`；`DELETE` → `chat:conversation:delete`（`sys_menu` id 231–236） | `authz.rs`、`public_migration.sql` |
| 服务身份 | 6 条路由全部登记在 `SERVICE_FORBIDDEN_ROUTES`（OAuth2 服务令牌没有 `UserContext`，一律 403） | `authz.rs` |

> 实现：`crates/api/src/conversation_routes.rs`（6 handler + 11 条单测），路由装配 `crates/api/src/lib.rs`
> （与 `/api/chat/stream` 共享同一个 `Arc<LLMRouter>`，一次构建两处挂载），Swagger 登记 `crates/apidoc/src/openapi.rs`，
> 前端映射 `httpAdapter.ts`（6 条已从 `TAURI_ONLY_COMMANDS` 移出）。
> 兼容细节：`providerId` 非法 → 静默忽略（与桌面命令一致）；`bindTreeNodeId` 非法 → 400；分页默认 20（列表）/ 100（消息），下限夹到 1；
> 列表 `total` 仍返回**字符串**（对齐 Tauri 命令形状，避免前端精度问题）。

#### 1.3.2 知识资产 OKF（7 条）✅ 已落地

| 命令 | 前端调用点 | 目标路由 |
|---|---|---|
| `list_knowledge_assets` | `services/knowledgeAssetService.ts:63` | `GET /api/knowledge/asset?okfType=` |
| `get_knowledge_asset` | `services/knowledgeAssetService.ts:58` | `GET /api/knowledge/asset/{id}` |
| `create_knowledge_asset` | `services/knowledgeAssetService.ts:76` | `POST /api/knowledge/asset` |
| `update_knowledge_asset` | `services/knowledgeAssetService.ts:90` | `PUT /api/knowledge/asset/{id}` |
| `delete_knowledge_asset` | `services/knowledgeAssetService.ts:95` | `DELETE /api/knowledge/asset/{id}` |
| `get_knowledge_asset_by_tree_node` | `services/knowledgeAssetService.ts:53` | `GET /api/knowledge/asset/by-tree-node/{treeNodeId}` |
| `attach_file_to_knowledge` | `services/knowledgeAssetService.ts:107` | `POST /api/knowledge/asset/{assetId}/attach` |

> 路由歧义（RK-1）**已实机验证不成立**：axum（matchit）静态段优先，`/api/knowledge/asset`、
> `/api/knowledge/asset/by-tree-node/...` 与既有 `/api/knowledge/{id}` 共存正常，
> 单测 `knowledge_asset_routes::tests::knowledge_asset_router_builds` 锁定。
> `attach` 的占位符用 `{assetId}`（而不是 `{id}`），因为命令参数名就是 `assetId`，
> `extractParams` 按同名剥离，否则该字段会残留在 body 里、路径也填不上。

| 关注点 | 结论 | 位置 |
|---|---|---|
| 归属校验 | `knowledge_asset` 是**共享知识库**（无 `user_id` 列），故不做行级归属；只要求登录 + 权限点 | `authz.rs` |
| 审计人 | `created_by` / `updated_by` 取**令牌 `sub`**；body 的 `createdBy` / `updatedBy` 仅比对告警，不采信（防伪造审计人） | `knowledge_asset_routes.rs::resolve_actor_id` |
| 不存在语义 | 服务层新增 `KNOWLEDGE_ASSET_NOT_FOUND`（含「未找到」关键字）→ **404**；此前 `fetch_one` 会把「不存在」报成 500 | `service/src/knowledge_asset_service.rs` |
| 幂等删除 | 删除改为 `WHERE id AND deleted=0`，`rows_affected()==0` → 404（重复删除同样 404） | 同上 |
| 创建默认值 | 新增 `build_knowledge_asset()`，**桌面命令与 HTTP 路由共用**（`confidence=1.0` / `status=draft` / `editor_mode=wysiwyg`），避免两处漂移 | 同上（+3 条单测） |
| 未关联节点 | `GET .../by-tree-node/{treeNodeId}` 返回 `data: null`（对齐命令的 `Option<KnowledgeAsset>`） | — |
| 权限点 | 复用既有 `knowledge:list` / `knowledge:add` / `knowledge:edit` / `knowledge:delete`（`sys_menu` id 291–297） | `authz.rs`、`public_migration.sql` |

> 实现：`crates/api/src/knowledge_asset_routes.rs`（7 handler + 8 条单测）、路由装配 `crates/api/src/lib.rs`、
> Swagger 登记 `crates/apidoc/src/openapi.rs`、前端映射 `httpAdapter.ts`（7 条从 `TAURI_ONLY_COMMANDS` 移出）。
> 兼容细节：`treeNodeId` / `id` / `assetId` 非法 → 400（文案与命令一致：`无效的节点ID` / `无效的ID` / `无效的资产ID`）；
> `createdBy` / `updatedBy` 非法或为空串 → 静默忽略；未知字段（如 `unknownField`）被忽略不报错。

#### 1.3.3 LLM 厂商 / 模型（12 条）✅ 已落地

| 命令 | 前端调用点 | 目标路由 |
|---|---|---|
| `get_llm_providers` | `services/llmProviderService.ts:44` | `GET /api/llm/providers` |
| `get_llm_provider` | **无调用点** | `GET /api/llm/providers/{id}` |
| `create_llm_provider` | `services/llmProviderService.ts:62` | `POST /api/llm/providers` |
| `update_llm_provider` | `services/llmProviderService.ts:75` | `PUT /api/llm/providers/{id}` |
| `delete_llm_provider` | `services/llmProviderService.ts:80` | `DELETE /api/llm/providers/{id}` |
| `fetch_llm_models` | `services/llmProviderService.ts:123` | `POST /api/llm/providers/{providerId}/fetch-models` |
| `get_llm_models` | `services/llmProviderService.ts:85` | `GET /api/llm/models?providerId=` |
| `create_llm_model` | `services/llmProviderService.ts:99` | `POST /api/llm/models` |
| `update_llm_model` | `services/llmProviderService.ts:113` | `PUT /api/llm/models/{id}` |
| `delete_llm_model` | `services/llmProviderService.ts:118` | `DELETE /api/llm/models/{id}` |
| `get_user_llm_setting` | `services/llmProviderService.ts:128` | `GET /api/llm/user-setting?userId=`（服务端忽略，用令牌） |
| `save_user_llm_setting` | `services/llmProviderService.ts:140` | `POST /api/llm/user-setting` |

> `fetch-models` 的占位符是 **`{providerId}`**（不是 `{id}`）：命令参数名就是 `providerId`，
> `extractParams` 按同名剥离，用 `{id}` 会导致路径填不上、`providerId` 残留 body。
> 用户偏好两条**只要求登录**（登记在 `authz::AUTH_ONLY_ROUTES`），因为它是个人配置、没有独立权限点。

| 关注点 | 结论 | 位置 |
|---|---|---|
| API Key 脱敏 | 厂商响应统一 `api_key = "******"`（未配置为 `null`），**不返回密文/明文**；`secret_key` / `extra_config` 直接不出现在响应 DTO 里 | `llm_routes.rs::ProviderView` |
| 回传占位符 | 更新时 `apiKey` 为空串 / 缺省 / `******` → 视为「不修改」，绝不会把占位符当新 Key 加密入库 | `normalize_api_key` |
| 密钥加密 | 桌面命令与 HTTP 路由共用 `build_provider_entity()`（`utils::crypto::encrypt_api_key`，需 `LLM_KEY_ENCRYPT_KEY`） | `service/src/llm_provider_service.rs` |
| 审计人 | `created_by` 取令牌 `sub`；body 的 `createdBy` / `currentUserId` 仅比对告警 | 同上 |
| 偏好归属 | `get/save_user_llm_setting` 一律用令牌 `sub`，query/body 的 `userId` 不采信（防改他人偏好） | `resolve_user_id` |
| 不存在语义 | 新增 `LLM_PROVIDER_NOT_FOUND` / `LLM_MODEL_NOT_FOUND` → **404**；删除加 `deleted=0` 条件并检查 `rows_affected`（重复删除 404） | 服务层 |
| 上游报错 | `fetch-models` 的厂商鉴权失败 / 网络不可达 → **500**（文案含 `kind()`，不伪装成 4xx） | — |
| 权限点 | 读 `knowledge:llm:list`（既有 id 54）；写 `knowledge:llm:edit`（新增，`sys_menu` 298–307） | `authz.rs`、`public_migration.sql` |

> 实现：`crates/api/src/llm_routes.rs`（12 handler + 8 条单测）、路由装配 `crates/api/src/lib.rs`、
> Swagger 登记 `crates/apidoc/src/openapi.rs`、前端映射 `httpAdapter.ts`（12 条从 `TAURI_ONLY_COMMANDS` 移出）。

#### 1.3.4 单据编号（4 条）— ✅ 已实施

| 命令 | 前端调用点 | 目标路由 | 状态 |
|---|---|---|---|
| `get_numbering_rules` | `services/numberingService.ts:41` | `GET /api/numbering/rules` | ✅ |
| `get_numbering_rule` | `services/numberingService.ts:46` | `GET /api/numbering/rules/{bizType}` | ✅ |
| `save_numbering_rule` | `services/numberingService.ts:54` | `POST /api/numbering/rules`（body `{id?,input}`） | ✅ |
| `reset_numbering_sequence` | `services/numberingService.ts:62` | `POST /api/numbering/rules/{bizType}/reset`（body `{resetKey}`） | ✅ |

> 实现：`crates/api/src/numbering_routes.rs`，路由装配 `crates/api/src/lib.rs`，
> 权限点 `system:numbering:list|edit|reset`（`authz.rs:`），Swagger 登记 `crates/apidoc/src/openapi.rs`，
> 前端映射 `httpAdapter.ts`（已从 `TAURI_ONLY_COMMANDS` 移出）。含 6 条单测。

#### 1.3.4.1 请求体形状原则（新增路由必须遵守）

前端 `extractParams()` 只剥离**路径占位符**对应的参数，其余参数原样作为 JSON body。
因此 handler 的 DTO 必须与 Tauri 命令参数**逐字同形**，不能"顺手 REST 化"：

| 命令 | ❌ 想当然的 REST 风格 | ✅ 应采用 |
|---|---|---|
| `save_numbering_rule(id, input)` | `PUT /rules/{bizType}` + 扁平 body | `POST /rules` + `{id?,input}` |
| `reset_numbering_sequence(bizType, resetKey)` | — | `POST /rules/{bizType}/reset` + `{resetKey}` |

代价是 URL 不够"纯 REST"，收益是**前端零参数转换**，42 条路由可以机械落地。


#### 1.3.5 工作流 Zen Engine（10 条）— **当前 Web 模式已坏**

| 命令 | 前端调用点 | 目标路由 |
|---|---|---|
| `list_workflows` | `services/workflowService.ts:219` | `GET /api/workflows` |
| `get_workflow` | `services/workflowService.ts:231` | `GET /api/workflows/{id}` |
| `save_workflow` | `services/workflowService.ts:249` | `POST /api/workflows`（`id` 有值 = 更新） |
| `delete_workflow` | `services/workflowService.ts:260`（现用 POST） | `DELETE /api/workflows/{id}` |
| `execute_workflow` | `services/workflowService.ts:277` | `POST /api/workflows/{id}/execute` |
| `list_executions` | `services/workflowService.ts:299` | `GET /api/workflow-executions` |
| `get_execution` | `services/workflowService.ts:289` | `GET /api/workflow-executions/{executionId}` |
| `cancel_execution` | `services/workflowService.ts:310` | `POST /api/workflow-executions/{executionId}/cancel` |
| `list_pending_tasks` | `services/workflowService.ts:320` | `GET /api/workflow-tasks/pending` |
| `complete_user_task` | `services/workflowService.ts:336` | `POST /api/workflow-tasks/{taskId}/complete` |

> **为何不用 `/api/workflows/executions`**：会与 `/api/workflows/{id}` 的静态段/参数段同级，未来增删路由易踩坑。
> 业务逻辑全部在 `crates/workflow/src/workflow_service.rs`（`WorkflowService`），`src/commands/workflow_commands.rs` 仅做 DTO 转换，HTTP handler 可直接复用。
> 需在 `crates/api/Cargo.toml` 增加 `workflow` 依赖。

#### 1.3.6 RAG（2 条）— UI 未使用，推迟到 P2

| 命令 | 前端调用点 | 目标路由 |
|---|---|---|
| `chunk_and_vectorize` | **无调用点** | `POST /api/rag/chunk-vectorize` → 建议 `202 + taskId` |
| `test_rag_retrieval` | **无调用点** | `POST /api/rag/retrieval-test` |

> `chunk_and_vectorize` 为分钟级任务，同步返回会被 nginx / 网关超时截断，需异步化或 SSE 进度。

#### 1.3.7 上传版本历史（1 条）✅ 已落地

| 命令 | 前端调用点 | 目标路由 |
|---|---|---|
| `upload_get_version_history` | `services/uploadService.ts:128`（**直接 `invoke`，绕过 api 层**） | `GET /api/upload/versions/{fileGroupId}` |

> 复用 `crates/storage/src/upload.rs:706` 的 `UploadManager::get_version_history(&schema, &file_group_id)`。
> 原计划写成 `/api/upload/{uploadId}/versions`，**实际改为 `/api/upload/versions/{fileGroupId}`**：
> 占位符名必须与命令参数名 `fileGroupId` 一致，否则 `extractParams()` 剥不掉它，会既填不满路径又把
> `fileGroupId` 留在 body 里（见 §1.3.4.1 同原则）。静态段 `versions` 与兄弟路由 `/api/upload/{id}/progress`
> 不冲突：`matchit` 静态段优先。

| 关注点 | 结论 | 位置 |
|---|---|---|
| 过滤条件 | 只返回 `deleted = 0 AND status = 'committed'` 的版本，`ORDER BY version DESC`（与命令共用同一 SQL，无逻辑重复） | `storage/src/upload.rs` |
| 响应形状 | 与命令**逐字一致**：`id`（字符串，避免 JS 精度丢失）、`version`、`isLatest`、`originalFilename`、`fileSize`、`fileUrl`、`changeReason`、`createdAt`（RFC3339） | `upload_routes.rs::UploadVersionItem` |
| 复用状态 | 直接使用 `UploadRouterState.upload_mgr`（与 init/chunk/complete 同一实例），无需重复构造 S3 客户端 | `lib.rs` `upload_router` |
| 空结果 | 分组不存在或没有 committed 版本 → **200 + `[]`**（不是 404；命令语义一致） | — |
| 出错语义 | 仅 `schema`/SQL 失败 → **500**（文案沿用 `查询版本历史失败: …`） | — |
| 权限点 | 复用既有 `upload:file:list`（`sys_menu` 新增路由行 308） | `authz.rs`、`public_migration.sql` |
| 前端路径修正 | `uploadService.ts` 的 `HttpUploadAdapter.getVersionHistory` 原本请求 `/upload/version-history/{fileGroupId}`——**后端从未有过该路由**，Web 模式必然 404；现已改为 `/upload/versions/{fileGroupId}` 并对分组 ID 做 `encodeURIComponent` | `services/uploadService.ts:249` |

> 实现：`crates/api/src/upload_routes.rs`（+1 handler、+1 条序列化单测）、路由装配 `crates/api/src/lib.rs`、
> Swagger 登记 `crates/apidoc/src/openapi.rs`、前端映射 `httpAdapter.ts`（从 `TAURI_ONLY_COMMANDS` 移出，
> 该集合现**仅剩 RAG 2 条**）。

### 1.4 仅 HTTP、无 Tauri 命令（10 条）—— 全 HTTP 的额外收益

| 映射 key | 路由 | 桌面 invoke 现状 |
|---|---|---|
| `get_api_keys` | `GET /api/api-keys` | ❌ 命令不存在，功能不可用 |
| `create_api_key` | `POST /api/api-keys` | ❌ |
| `revoke_api_key` | `DELETE /api/api-keys/{id}` | ❌ |
| `rotate_api_key` | `POST /api/api-keys/{id}/rotate` | ❌ |
| `get_oauth_clients` | `GET /api/oauth-clients` | ❌ |
| `create_oauth_client` | `POST /api/oauth-clients` | ❌ |
| `update_oauth_client` | `PUT /api/oauth-clients/{id}` | ❌ |
| `delete_oauth_client` | `DELETE /api/oauth-clients/{id}` | ❌ |
| `logout` | `POST /api/auth/logout` | ❌ |
| `refresh_token` | `POST /api/auth/refresh` | ❌ |

> 这 10 条证明桌面**必须**能走 HTTP——否则这些既有功能在桌面端永远不可用。

### 1.5 后端已有路由、前端零调用（沙箱 10 条）

`crates/api/src/lib.rs:532-564` 已挂 11 条 `/api/sandbox/*` 路由，`public_migration.sql` 已有 280–290 权限点，`runtime`/`service::sandbox_service` 完整；`apps/web/src` 中 `sandbox` 相关调用 **0 处**。

> 处置：**不纳入本期**（决策 D3）。若未来接入，仅需在 `httpAdapter.ts` 补 10 条映射，后端零改动。

---

## 2. 目标架构（T2：远程服务端）

```
┌─────────────────────────────┐         ┌──────────────────────────────────────┐
│ 桌面（Tauri 壳）             │         │ 远端服务端                            │
│  · apps/web/dist（静态）      │ HTTPS   │  nginx：静态 SPA + /api 反代          │
│  · 无 DB / 无 S3 / 无 sidecar│ ──────► │  assets-server / assets-gateway       │
│  · 无 invoke 数据通道         │ Bearer  │  PostgreSQL + MinIO + doc-parser      │
│  · localStorage 存服务器地址  │  +SSE   │  /livez  /readyz  /api/system/version │
└─────────────────────────────┘         └──────────────────────────────────────┘
              ▲
   ┌──────────┴──────────┐
   │ 浏览器（Web 版）      │  同源访问，与桌面共用同一套 /api
   └─────────────────────┘
```

### 2.1 架构前提核对

| 事实 | 证据 | 对 T2 的意义 |
|---|---|---|
| `crates/server` **不托管静态 SPA**（全仓库无 `ServeDir`/`ServeFile`） | 检索仅命中 `tauri.conf.json:10` | 部署必须带 nginx 托管 `apps/web/dist` |
| doc-parser 已由服务端拉起 | `crates/server/src/main.rs:67`、`crates/gateway/src/main.rs:35` | 桌面可安全删除 sidecar |
| `API_BIND` 默认 `0.0.0.0:3001` | `crates/api/src/lib.rs:124-145` | T2 下桌面不再绑定任何端口，隐患消失 |
| 网关有 `/livez`、`/readyz`，api crate 没有 | `build_router` 内无 livez | 需补 S-1（诊断依赖） |
| 前端未使用任何 Tauri 插件能力 | 全仓库 `@tauri-apps/*` 仅 `api/core` 一处（`uploadService.ts:11`） | 删除 invoke 通道无功能损失 |
| 桌面 `setup` 里 `spawn(serve_http)` 后立即 `Ok(())`，`JoinHandle` 被丢弃 | `src-tauri/src/lib.rs:63-72` | 启动竞态（B-1）在 T2 下自然消除 |

### 2.2 为何选 T2 而非 T1（内嵌本地服务）

| 维度 | T1 内嵌本地后端 | **T2 远程服务端（选定）** |
|---|---|---|
| 部署复杂度 | 每个客户端要 PG/MinIO/影子库，运维不可承受 | 服务端一套，客户端零依赖 |
| 多用户协作 | 各自独立库，无协作 | 天然共享 |
| 启动速度 | 等 DB 初始化 | 立即出窗口 |
| 安装包 | 含 DB 驱动、S3 SDK、doc-parser | 只含 WebView 壳 |
| 离线能力 | 有 | **无**（但现状也从未真正可用：本地 PG/MinIO 从未随包分发） |
| 版本一致性 | 客户端/服务端版本各自漂移 | 统一升级 |

> 结论：T2 是**唯一可持续**的拓扑；T1 的"离线"是纸面能力。

### 2.3 可选变体 T2-b：远程托管 SPA（不默认开启）

invoke 通道归零后，桌面可以不打包前端，窗口直接加载 `https://assets.example.com/`：

| 维度 | T2（打包本地 dist，默认） | T2-b（远程 SPA，可选） |
|---|---|---|
| 前端版本漂移 | 需随客户端发版 | 不存在，服务端升级即生效 |
| 壳体积 | 数 MB（含 dist） | 数百 KB |
| 断网表现 | 可控错误页 | 白屏（浏览器级错误） |
| 启动速度 | 快（本地资源） | 依赖网络 |
| 配置 | 现状 | `app.windows[0].url` 指远端 + 放开 CSP |

> 建议：默认 T2，T2-b 仅作为企业内网可选项写入运维手册。

---

## 3. 桌面瘦身清单（`apps/backend/src-tauri/src/lib.rs`）

| 行 | 现状 | T2 处置 |
|---|---|---|
| `:13-22` `load_env()` | 加载 `.env.toml`（PG/S3/JWT/…） | **保留**，但只认 `[desktop]` 段（预置服务器地址、窗口尺寸） |
| `:31` `start_doc_parser()` | 拉起 Python sidecar | **删除**（服务端已有） |
| `:34-36` tracing 初始化 | 本地日志 | 保留（排障必需） |
| `:39` `plugin(tauri_plugin_opener)` | 打开外链 | 保留（唯一合理的原生能力） |
| `:40-42` `manage(SkillRegistryState)` | 桌面私有 Skill 注册表 | **删除**（与 `crates/api/src/skill_routes.rs:15` 的 `SkillRouterState` 本就是两份实例，见 E-1） |
| `:51-57` `init_database_and_schema()` | 本地 PostgreSQL | **删除** |
| `:59-61` `init_llm_router()` | 本地 LLM Router | **删除** |
| `:63-72` `spawn(bootstrap::serve_http)` | 内嵌 axum | **删除**（端口占用、启动竞态 B-1、绑定失败仅日志 B-2 一并消除） |
| `:77-250` `invoke_handler![…]`（154 条） | 全部命令 | **整体删除**，随后删除 `src/commands/` 目录 |
| `Cargo.toml` | 依赖整个工作区 | P2：裁剪至 `tauri` + `opener` + `serde`，显著降低编译时间与包体 |

瘦身后 `lib.rs` 约 40 行：

```rust
pub fn run() {
    load_env();
    init_tracing();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|_app| Ok(()))            // 可选：注入 window.__API_BASE__ 预置地址
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

**收益**：桌面不再需要 `.env.toml` 中的任何凭据、不再初始化数据库、不占用 3001 端口、不存在启动竞态；安装包与编译产物体积下降。

> 迁移期注意：删除 `src/commands/` 前先确认无跨模块引用（`commands::upload` 曾被 `bootstrap` 调用，需处理为直达 `storage::upload`）。

---

## 4. 服务端前置能力（T2 的硬前置）

| 编号 | 能力 | 落点 | 说明 |
|---|---|---|---|
| **S-1** | `GET /livez`（无鉴权） | `crates/api/src/lib.rs` `build_router` | 客户端探活与诊断页依赖；必须**在鉴权中间件之外**挂载 |
| **S-2** | 版本接口 | `GET /api/system/version` 或复用 `/api-docs/openapi.json` 的 `info.version` | 桌面"关于"页比对客户端/服务端版本并提示升级 |
| **S-3** | CORS 白名单含桌面 Origin | `crates/api/src/lib.rs:147-157`、`crates/gateway/src/router.rs:44-73`（后者当前**无 CORS 层**） | 必须内置 `tauri://localhost`、`http://tauri.localhost` |
| **S-4** | MinIO / S3 CORS + `ExposeHeader: ETag` | 部署配置（不入代码库或放 `deploy/minio-cors.json`） | 桌面分片上传读 `ETag`（`uploadService.ts:394-410`），缺失 → `InvalidPart` |
| **S-5** | nginx 托管 SPA + `/api` 反代 + SSE 三件套 | `deploy/nginx.conf`（新建） | `proxy_buffering off; proxy_read_timeout 3600s; X-Accel-Buffering: no` |
| **S-6** | HTTPS | 部署 | 明文 HTTP 会直接泄露 Bearer token |

> S-3 是最容易踩的运维坑：**任何收紧 CORS 的动作都必须保留桌面 Origin**，否则桌面白屏。此项必须写入运维手册，而非仅存在于代码注释。

---

## 5. 前端改造：唯一通道 + 服务器配置

### 5.1 删除清单

| # | 文件 / 符号 | 行 | 说明 |
|---|---|---|---|
| R1 | `utils/adapters/tauriAdapter.ts` | 全文件 | invoke 适配器，HTTP 化后无引用 |
| R2 | `utils/adapters/manager.ts` | 全文件 | 运行时切换逻辑（`setAdapter`/`getAdapter` 归一到 `httpAdapter`） |
| R3 | `utils/adapters/index.ts` | 全文件 | 仅做再导出，改为直接导出 `httpAdapter` |
| R4 | `utils/adapters/types.ts` | `ApiConfig` 中 Tauri 专属字段 | 保留通用类型 |
| R5 | `httpAdapter.ts` `TAURI_ONLY_COMMANDS` | `:234-278` | 42 条全部落地后删除该常量 |
| R6 | `httpAdapter.ts` Tauri-only 抛错分支 | `:520-540` 附近 | 删除 |
| R7 | `httpAdapter.ts` 硬编码基址 | `:36` | 改用 `apiBase.ts` |
| R8 | `useChatStream.ts` 硬编码基址 | `:39` | 改用 `apiBase.ts` |
| R9 | `uploadService.ts` `shouldUseTauri()` | `:311-322` | 删除 |
| R10 | `uploadService.ts` `TauriUploadAdapter` | `:92-160` | 删除 |
| R11 | `uploadService.ts` 顶层 `import { invoke }` | `:11` | 删除（连带 `@tauri-apps/api` 依赖退场） |
| R12 | `uploadService.ts` 硬编码基址 | `:190`、`:353-355` | 改用 `apiBase.ts`；**同时删除 `127.0.0.1:3001` 兜底** |
| R13 | `uploadService.ts:128` `invoke('upload_get_version_history')` | `:128` | 改走 `api.get('upload_get_version_history')` |
| R14 | `app/layout.tsx` 适配器自动探测 | `:33-54` | 删除 `setAdapter` 分支 |
| R15 | `tests/utils/adapters/httpAdapter.test.ts` | `:22`、`:179-196` | `REGISTERED_COMMANDS` 123 → 154；`TAURI_ONLY_COMMANDS` 断言反转为"应为空" |
| R16 | `apps/web/package.json` | `@tauri-apps/api` | 确认无残留引用后移除 |

### 5.2 新增：`apps/web/src/utils/apiBase.ts`

优先级（T2 版）：

```
1. window.__API_BASE__              ← Tauri 可选注入（企业预置地址，免手工填写）
2. localStorage['api_base_url']     ← 用户在配置页填写的
3. process.env.NEXT_PUBLIC_API_BASE_URL
4. window.location.origin + '/api'  ← 浏览器同源（nginx 反代）
5. 无 → 返回 null，触发 ServerConfigGate（不发任何业务请求）
```

对外导出：

```ts
export function getApiBase(): string | null;      // 含 /api 后缀
export function hasServerConfigured(): boolean;
export function setApiBase(url: string): void;    // 校验 + 规范化 + 持久化
export function normalizeApiBase(input: string): string;  // 去尾斜杠、补/去 /api
export function clearApiBase(): void;
```

> **必须删除 `http://127.0.0.1:3001/api` 兜底**。T2 下该兜底会让桌面静默连本机空端口，症状表现为"登录一直转圈"，极难排查。

### 5.3 新增：首次启动 Gate（阻断式）

```
apps/web/src/components/ServerConfigGate.tsx   [新增，'use client']
   └── 挂在 apps/web/src/app/layout.tsx 中包裹 {children}
        · 未配置 → 全屏配置表单（地址 + 测试连接 + 保存），不渲染业务页面
        · 已配置 → 正常渲染 children
```

同时必须短路 `authStore.init()`，否则首屏会先打 `/api/auth/session` 并刷一屏网络错误：

```ts
// store/authStore.ts -> init()
if (!hasServerConfigured()) {
  set({ isInitializing: false, isLoggedIn: false });
  return;
}
```

> 该仓库是 Next.js `output: 'export'` 静态导出，**没有 middleware / 服务端重定向**，所有守卫都必须是客户端组件。

### 5.4 新增：`apps/web/src/app/settings/server/page.tsx`（连接配置与诊断）

| 区域 | 内容 |
|---|---|
| 服务器地址 | 输入框 + 「测试连接」+ 「保存并重载」 |
| 连接诊断 | ① `GET /livez` 可达性 ② `/api-docs/openapi.json` 的 `info.version` ③ 往返延迟（ms）④ 当前页面 Origin ⑤ 解析后的 API 基址 |
| 错误分类 | 超时 / DNS 失败 / TLS 证书 / CORS 拒绝 / 404（路径配错）/ 401（需登录）/ 5xx |
| 辅助 | 「恢复默认」「复制诊断信息」（一键贴给运维）；「最近使用」列表为 P2 |
| Web 版 | 浏览器同源，本页只读展示当前地址 |

入口：`app/settings/page.tsx` 增加「服务端连接」卡片（现有 `settings/*` 已含 users/roles/llm/api-keys 等 12 个子页，风格保持一致）。

### 5.5 地址校验与切换规则

1. 必须以 `http://` 或 `https://` 开头，自动去尾斜杠；
2. 填 `https://host` → 内部补 `/api`；填 `https://host/api` → 不重复补；
3. **非 `localhost`/`127.0.0.1` 的 `http://` → 阻断**，提示改用 HTTPS（防 token 明文）；
4. 保存时**清空 `auth_token` / `refresh_token`**（跨服务器 token 无意义，且会引发 401 死循环），随后 `location.reload()`；
5. `setApiBase` 成功后立即用新地址探活一次，失败则回滚并提示。

### 5.6 前端映射表补录（42 条）

`httpAdapter.ts` 的 `COMMAND_ROUTE_MAP` 按 §1.3 逐条追加，**占位符名称必须与 service 参数名完全一致**，否则 `extractParams()`（`:293-316`）会把参数漏进 body：

```ts
// ✅ 正确：占位符 = 参数名
get_conversation_messages: { method: 'GET', path: '/conversations/{conversationId}/messages' },
// ❌ 错误：参数名是 conversationId，占位符写成 id → extractParams 匹配不到，body 里残留 conversationId
```

同时把 §1.3.5 的 10 条从"隐式缺口"转为显式映射后，Web 模式的工作流模块即可恢复。

---

## 6. 安全取舍（T2 特有）

| 编号 | 问题 | 处置 |
|---|---|---|
| **SEC-1** | CSP 与"用户任意填地址"天然冲突，`connect-src` 无法预枚举 | 推荐 `default-src 'self'; connect-src *; img-src 'self' data: blob: https:;`（比现状 `csp: null` 严格，且不阻断自填地址）。若企业统一预置地址，收紧为精确 `connect-src https://api.corp.com` |
| **SEC-2** | 远端 CORS 是否放行桌面 Origin | 服务端默认白名单内置；**并写入运维手册**（见 S-3） |
| **SEC-3** | token 存 localStorage（`authStore.ts:77-80`） | T2 下 WebView 存储按应用隔离，风险低于浏览器共享域；终态建议 refresh token 走 httpOnly（P2） |
| **SEC-4** | 多租户 `GLOBAL_SCHEMA` 进程级单例 | T2 把多用户集中到单进程，**P0-2 从"桌面可容忍"升级为必修**：HTTP 侧必须 `task_local` 化 + fail-closed |

> SEC-4 是 T2 相比 T1 唯一的新增风险点，必须与本期同批交付，否则多租户并发会串数据。

---

## 7. 部署交付物

| 文件 | 状态 | 内容要点 |
|---|---|---|
| `deploy/nginx.conf` | 新建 | `root apps/web/dist; try_files $uri $uri/ /index.html;`；`location /api/ { proxy_pass http://127.0.0.1:3001; proxy_buffering off; proxy_read_timeout 3600s; }`；显式传递 `X-Accel-Buffering: no` |
| `deploy/docker-compose.yml` | 新建 | postgres / minio / assets-server / nginx；healthcheck 调 `/livez` |
| `deploy/Dockerfile.server` | 新建 | 多阶段：`cargo build -p server --release` → distroless |
| `deploy/minio-cors.json` | 新建 | `AllowedOrigins` 含桌面 Origin；`ExposeHeaders: ["ETag"]` |
| `apps/backend/.env.example.toml` | 拆分 | 分 `[server]` 与 `[desktop]` 两段，桌面段不再包含任何凭据 |
| `docs/运维手册` | 补充 | S-3（CORS 必须含桌面 Origin）、S-4（MinIO ETag）、S-6（HTTPS）三条红线 |

---

## 8. 测试改造

| 文件 | 改动 |
|---|---|
| `apps/web/src/tests/utils/adapters/httpAdapter.test.ts:22` | `REGISTERED_COMMANDS` 由硬编码 123 改为 **154**（否则"全量覆盖"断言是空转） |
| 同上 `:179-196` | `TAURI_ONLY_COMMANDS` 断言**反转**：迁移完成后该集合应为空 |
| 新增 | `apiBase.test.ts`：优先级、`/api` 去重、http 明文阻断、未配置返回 null |
| 新增 | Rust 集成测试：42 条路由各自的 401（无令牌）/ 403（无权限）/ 200 最小用例 |
| 回归 | 上传分片（ETag 非空）、SSE 逐字输出、切租户后数据隔离 |

> 现存的 `REGISTERED_COMMANDS=123` 是全前端测试里最危险的一条——它让"路由覆盖完整性"长期处于假通过状态。

---

## 9. 分批与工作量

| 批次 | 内容 | 依赖 | 人日 |
|---|---|---|---|
| **P0-A** | 42 条路由 + `ROUTE_PERMS` + `sys_menu` 种子 + 前端映射 + 测试修正 | 无（桌面/Web 同时受益，可独立发版） | 5-6 |
| **P0-B** | `GET /livez` + 版本接口 + CORS 白名单（api + gateway） | P0-A | 0.5 |
| **P0-C** | `apiBase.ts` + `ServerConfigGate` + `settings/server` 诊断页 + 删除 R1–R16 | P0-B | 2.5 |
| **P0-D** | 桌面瘦身（`lib.rs` / `src/commands/` / 内嵌 axum 全删） | 必须与 P0-C 同版本 | 1.5 |
| **P1** | 部署交付物（nginx / Docker / compose / MinIO CORS / systemd / env 拆分） | P0-B | 2-3 |
| **P2** | `Cargo.toml` 裁剪、OpenAPI→TS 代码生成、httpOnly refresh、最近服务器列表 | — | 3-4 |
| **P2'** | 多租户 `GLOBAL_SCHEMA` → `task_local`（SEC-4） | 与 P0-C 同批（安全前置） | 2 |

**合计**：约 12-14 人日（不含 P2'）。

> **P0-C 与 P0-D 必须同一版本发布**：
> - 只删适配器不改桌面 → 桌面首屏全部 500；
> - 只改桌面不删适配器 → 桌面仍走 invoke 打到已删除的命令。

---

## 10. 风险登记

| # | 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|---|
| RK-1 | `/api/knowledge/asset` 与 `/api/knowledge/{id}` 路由歧义 | 低 | 知识资产模块 404 | ✅ **已验证不成立**（2026-11-14 实机冒烟：静态段优先，`/api/knowledge/{id}` 仍命中旧 handler；单测 `knowledge_asset_router_builds` 锁定）。方案 B `/api/knowledge-assets` 未启用 |
| RK-2 | 前端参数名与占位符不一致导致参数丢失 | 中 | 局部功能报错 | 逐条对照 service 签名；补 `extractParams` 单测 |
| RK-3 | 忘记登 `ROUTE_PERMS` 与 `sys_menu` | 高 | `AUTHZ_MODE=enforce` 下 403 | 以 `AUTHZ_MODE=enforce` 跑一遍全量冒烟作为发版门槛 |
| RK-4 | 运维收紧 CORS 未含桌面 Origin | 中 | 桌面白屏 | 写入运维手册 + 诊断页显式报「CORS 拒绝」 |
| RK-5 | nginx 未关 `proxy_buffering` | 中 | SSE "卡住" | 配置模板固化 + 验收用例覆盖 |
| RK-6 | 桌面删除命令后遗留 `Cargo.toml` 无用依赖 | 高 | 编译慢/包体大 | P2 专用收尾任务 |
| RK-7 | 多租户串数据（SEC-4） | 中 | **数据泄露** | P2' 与 P0-C 同批，fail-closed |

---

## 11. 验收清单

**功能正向**

- [ ] Web 与桌面登录同一账号，数据完全一致
- [ ] 工作流：列表 / 编辑 / 保存 / 执行 / 运行记录 / 待办任务 全部可用（**当前 Web 模式不可用**）
- [ ] API Key 与 OAuth 客户端页面可用（**当前桌面模式不可用**）
- [ ] 知识资产、LLM 厂商与模型、单据编号、会话管理、上传版本历史可用
- [ ] 大文件分片上传成功，`ETag` 非空，complete 无 `InvalidPart`
- [ ] 智能问答逐字流式输出

**负向 / 边界**

- [ ] 首次启动无配置 → 显示配置页，DevTools 无业务请求外发
- [ ] 错地址（404）→ 诊断明确提示"地址可能缺少 /api"
- [ ] 公网 `http://` → 阻断并提示改用 HTTPS
- [ ] 服务端未放行 `tauri://localhost` → 诊断报「CORS 拒绝」并给出所需响应头
- [ ] 断网后点击任意菜单 → 全局提示"无法连接服务器"+ 重试，不白屏
- [ ] 切换服务器 → 清 token → 回登录页 → 登录成功
- [ ] 桌面 DevTools Network 中**仅见 fetch/XHR，无 invoke**

**代码卫生**

- [ ] `apps/web/src` 中 `invoke(` 出现次数为 0
- [ ] `TAURI_ONLY_COMMANDS` 已删除
- [ ] `grep -rn "127.0.0.1:3001" apps/web/src` 无结果
- [ ] `authz.rs` `ROUTE_PERMS` 覆盖全部新增路由
- [ ] `cargo clippy -p api`、`pnpm -C apps/web test` 通过

---

## 12. 实施进度

| 批次 | 模块 | 路由数 | 状态 | 产物 |
|---|---|---|---|---|
| P0-0 | 认证阻塞项：`jsonwebtoken` 11 的 `aud` 校验导致 HTTP 全站 401 | — | ✅ 已修复 | `crates/api/src/auth.rs::jwt_validation()` + 5 条回归单测 |
| P0-A | 单据编号 | 4 | ✅ 完成 | `numbering_routes.rs` + `lib.rs` + `authz.rs` + `openapi.rs` + `httpAdapter.ts` |
| P0-A | 会话管理 | 6 | ✅ 完成 | `conversation_routes.rs`（含 11 条单测）+ `conversation_service.rs`（归属闸门 + 4 条归属单测）+ `lib.rs` + `authz.rs` + `openapi.rs` + `httpAdapter.ts` + `sys_menu` 231–236 |
| P0-A | 知识资产 OKF | 7 | ✅ 完成 | `knowledge_asset_routes.rs`（含 8 条单测）+ `knowledge_asset_service.rs`（`KNOWLEDGE_ASSET_NOT_FOUND` + `build_knowledge_asset` 共用构造器 + 3 条单测）+ `lib.rs` + `authz.rs` + `openapi.rs` + `httpAdapter.ts` + `sys_menu` 291–297 |
| P0-A | LLM 厂商/模型 | 12 | ✅ 完成 | `llm_routes.rs`（含 8 条单测，`ProviderView` 脱敏）+ `llm_provider_service.rs`（`build_provider_entity` + 2 个 NOT_FOUND 常量 + 3 条单测）+ `lib.rs` + `authz.rs` + `openapi.rs` + `httpAdapter.ts` + `sys_menu` 298–307 |
| P0-A | 工作流 | 10 | ⬜ 待做 | 需加 `workflow` 依赖 |
| P0-A | 上传版本历史 | 1 | ✅ 完成 | `upload_routes.rs`（+1 handler + 1 条 camelCase 单测，复用 `UploadRouterState.upload_mgr`）+ `lib.rs` + `authz.rs`（复用 `upload:file:list`）+ `openapi.rs` + `httpAdapter.ts`（`TAURI_ONLY_COMMANDS` 仅剩 RAG 2 条）+ `sys_menu` 308 |
| P0-A | RAG | 2 | ⬜ 推迟 P2 | — |
| P0-B | `/livez` + 版本 + CORS | — | ⬜ 待做 | — |
| P0-C | `apiBase.ts` + Gate + 诊断页 + 删适配器 | — | ⬜ 待做 | — |
| P0-D | 桌面瘦身 | — | ⬜ 待做 | — |

**已落地验证（2026-11-13）**

| 校验 | 结果 |
|---|---|
| `cargo check -p api --tests` | ✅ 9.16s，无新增告警 |
| `cargo test -p api --lib` | ✅ **61 passed / 0 failed**（含新增 6 条 `numbering_routes` 单测；`authz::route_perms_has_no_duplicate_keys`、`route_perms_entries_are_well_formed` 均通过） |
| `cargo check -p apidoc --tests` / `cargo test -p apidoc --lib` | ✅ 4 passed（`path_template_params_are_consistent`、`spec_is_serializable_and_documented`） |
| `pnpm -C apps/web test -- httpAdapter` | ✅ 4 passed（映射表覆盖率 / 无重叠 / 路径合法 / 命名风格） |
| 实机冒烟（`assets-server` + `next dev`，库 `ceshi8`） | ✅ 登录 200 → `GET /api/numbering/rules` **200**（返回 2 条规则）→ `GET /api/numbering/rules/receive` 200；`/api-docs/openapi.json` 共 **84** 条路径且含 `/api/numbering/*`；`http://127.0.0.1:1480/login` 200 |
| 端口占用实测（两个实例抢 3001） | ✅ 实例 A 正常起（`healthz` 200、日志仅 1 条「启动于」）；实例 B 只输出「端口已被占用…`ss -ltnp`/`pkill`/`[api] port`」提示，**无**「启动于」假成功日志，退出码 1 |
| `cargo test -p api --lib`（含 `bind_error_message*` 2 条新单测） | ✅ **68 passed / 0 failed** |


> 冒烟过程中发现并修复了 **P0-0**：修复前同样这三步会得到 `200 → 401 → 401`（日志 `JWT 验证失败: InvalidAudience`）。
> 启动方式见 `docs/桌面与Web双形态部署指南.md` §3.0。

**已落地验证（2026-11-14 · 会话管理 6 条）**

| 校验 | 结果 |
|---|---|
| `cargo check -p api --tests` | ✅ 无 error（仅既有 `process_routes.rs` 未使用 import 告警） |
| `cargo test -p api --lib` | ✅ **79 passed / 0 failed**（含 `conversation_routes::tests` **11 条**） |
| `cargo test -p service conversation` | ✅ **4 passed**（`ownership_tests`：本人可访问 / 他人被拒 / 缺失与越权不可区分 / 文案仍可映射 404） |
| `cargo test -p apidoc` | ✅ 4 passed（`{convId}` 路径模板与 OpenAPI 参数一致） |
| `pnpm -C apps/web test -- httpAdapter` | ✅ 4 passed（映射表覆盖率 / 无重叠 / 路径合法 / 命名风格） |
| 实机冒烟（`assets-server`，库 `ceshi8`，schema `single`） | ✅ 见下表 13 项全绿 |

实机冒烟明细（admin 建一条会话，另建普通用户 convtest2 做越权测试）：

| # | 场景 | 结果 |
|---|---|---|
| 1 | admin `GET /api/conversations` | **200**，返回自己的会话 |
| 2 | admin `GET /api/conversations/{own}/messages` | **200**，`data: []` |
| 3 | convtest2 `GET …/{admin 的}/messages` | **404** `会话未找到（不存在或不属于当前用户）` |
| 4 | convtest2 `PUT …/{admin 的}/title` | **404**，且 DB 标题**未被改** |
| 5 | convtest2 `DELETE …/{admin 的}` | **404**，且 DB `deleted` 仍为 **0** |
| 6 | convtest2 `GET /api/conversations` | **200**，`items: []`（**看不到** admin 的会话） |
| 7 | admin `PUT …/{own}/title` | **200**，DB 标题已更新 |
| 8 | admin `GET /api/conversations?userId=99999` | **200**，仍返回**自己的**列表（伪造 `userId` 被忽略） |
| 9 | admin `DELETE …/{own}` | **200**，DB `deleted=1`（软删） |
| 10 | 删除后再 `GET …/{own}/messages` | **404** |
| 11 | 不存在的 id `GET …/99999999/messages` | **404**（文案与 #3 **完全一致**，无法据此判断他人会话是否存在） |
| 12 | 非法 convId `GET …/abc/messages` | **400** `无效的会话ID: …` |
| 13 | 无令牌 `GET /api/conversations` | **401** `缺少 Authorization 请求头` |
| 14 | `sys_menu` 231–236 | ✅ 启动即自动补录 6 条（`chat:conversation:list/edit/delete` + `chat:completion:add`） |
| 15 | `/api-docs/openapi.json` | ✅ 共 **88** 条路径，会话 4 个 path 覆盖 6 个方法 |

> 冒烟脚本：`/tmp/smoke_conv.sh`（一次性验证用，未入库）。注意普通用户必须带租户（`sys_user.tenant_id`），
> 否则 `resolve_user_schema` 兜底为 `public`，会报 `relation "conversation" does not exist`——这是既有租户机制，不是本模块缺陷。

**已落地验证（2026-11-14 · 知识资产 OKF 7 条）**

| 校验 | 结果 |
|---|---|
| `cargo check -p api --tests` | ✅ 无 error |
| `cargo test -p api --lib` | ✅ **87 passed / 0 failed**（含 `knowledge_asset_routes::tests` **8 条**） |
| `cargo test -p service knowledge` | ✅ **3 passed**（`build_applies_command_defaults` / `build_keeps_optional_fields` / `not_found_message_maps_to_404`） |
| `cargo test -p apidoc` | ✅ 4 passed |
| `pnpm -C apps/web test -- httpAdapter` | ✅ 4 passed |
| 实机冒烟（库 `ceshi8`，schema `single`，`knowledge_tree` 需先有节点：FK `tree_node_id → knowledge_tree(id)`） | ✅ 18 项全绿 |

| # | 场景 | 结果 |
|---|---|---|
| 1 | `POST /api/knowledge/asset`（body 内伪造 `createdBy:"999"`） | **200**，`data.id` 返回字符串；DB `created_by=1`（**令牌 sub**，伪造值被忽略） |
| 2 | `GET /api/knowledge/asset?okfType=concept` | **200**，含本资产 |
| 3 | `GET /api/knowledge/asset?okfType=rule` | **200**，不含本资产（过滤生效） |
| 4 | `GET /api/knowledge/asset/{id}` | **200**，形状与 Tauri 命令一致（`id`/`tree_node_id` 为字符串，`okf_type=concept`） |
| 5 | `GET /api/knowledge/asset/by-tree-node/9001` | **200**，返回该节点下的资产 |
| 6 | `GET /api/knowledge/asset/by-tree-node/9002`（未关联） | **200** `data: null` |
| 7 | 既有 `GET /api/knowledge/9001` | 命中**旧** handler（`查询知识条目失败…`），证明 `/asset` 静态段未遮蔽参数段 |
| 8 | `PUT /api/knowledge/asset/{id}`（伪造 `updatedBy:"999"`） | **200**，DB `title/status` 已改、`updated_by=1` |
| 9 | `PUT /api/knowledge/asset/99999999` | **404** `知识资产未找到（不存在或已删除）` |
| 10 | `POST /api/knowledge/asset/{id}/attach` | **200**，返回含 `file_url/file_name/file_size/file_mime/file_md5` |
| 11 | `POST /api/knowledge/asset/99999999/attach` | **404** |
| 12 | `GET /api/knowledge/asset/99999999` | **404** |
| 13 | `GET /api/knowledge/asset/abc` | **400** `无效的ID: …` |
| 14 | 无令牌 `GET /api/knowledge/asset` | **401** |
| 15 | `DELETE` → 再 `DELETE` → `GET` | **200** / **404** / **404**，DB `deleted=1`（软删 + 幂等） |
| 16 | `sys_menu` 291–297 | ✅ 启动即自动补录 7 条（`knowledge:list/add/edit/delete`） |
| 17 | `/api-docs/openapi.json` | ✅ 共 **92** 条路径，`/api/knowledge/asset*` 4 个 path 覆盖 7 个方法 |
| 18 | 冒烟数据清理 | ✅ 资产与临时节点均已删除（`left assets@9001=0`） |

> 冒烟脚本：`/tmp/smoke_asset.sh`（一次性验证用，未入库）。
> 注意 `knowledge_asset.id` 是 `GENERATED ALWAYS AS IDENTITY`，INSERT 不写 id；而 `knowledge_tree.id` **非自增**，冒烟需显式指定。

**已落地验证（2026-11-14 · LLM 厂商/模型/偏好 12 条）**

| 校验 | 结果 |
|---|---|
| `cargo check -p api --tests` | ✅ 无 error |
| `cargo test -p api --lib` | ✅ **95 passed / 0 failed**（含 `llm_routes::tests` **8 条**） |
| `cargo test -p service llm` | ✅ **16 passed**（含 `llm_provider_service::tests` **3 条**：密钥加密 / 空 Key 保留 / 404 文案） |
| `cargo test -p apidoc` | ✅ 4 passed |
| `pnpm -C apps/web test -- httpAdapter` | ✅ 4 passed |
| 实机冒烟（带 `LLM_KEY_ENCRYPT_KEY` 启动，库 `ceshi8`） | ✅ 12 项全绿 |

| # | 场景 | 结果 |
|---|---|---|
| 1 | `POST /api/llm/providers`（伪造 `createdBy:"999"`） | **200**，响应 `api_key="******"`；DB 密文（长度 64，**非**明文）、`created_by=1`、响应中 `sk-` 出现 **0** 次 |
| 2 | `GET /api/llm/providers` | **200**，全部厂商 `api_key` 均为 `******`，无 `sk-` 泄露 |
| 3 | `GET /api/llm/providers/{id}` | **200**，同样脱敏 |
| 4 | `PUT` 回传 `apiKey:"******"` | **200**，**密文未变**（占位符不会被当成新 Key）；`provider_name`/`enable` 正常更新 |
| 5 | `PUT` 传新 Key | **200**，密文已变、响应仍脱敏 |
| 6 | 不存在厂商 `GET` / `PUT` / `fetch-models` | **404** `LLM 厂商未找到（不存在或已删除）` |
| 7 | 模型 `POST` → `GET ?providerId=` → `PUT` → `DELETE` → 再 `DELETE` | **200 / 200（含新模型）/ 200（`model_type=vision`、`enable=false`）/ 200 / 404** |
| 8 | 偏好 `GET`（伪造 `userId=999`）→ `POST` → 再 `GET` | 前 **200/null**、存 **200**（DB `user_id=1`，伪造值被忽略）、后 **200** 返回该偏好 |
| 9 | `GET /api/llm/providers/abc` / 无令牌 | **400** `无效的ID` / **401** |
| 10 | `sys_menu` 298–307 | ✅ 启动即自动补录 10 条（读 `knowledge:llm:list`、写 `knowledge:llm:edit`） |
| 11 | `/api-docs/openapi.json` | ✅ 共 **98** 条路径，`/api/llm*` 6 个 path 覆盖 12 个方法 |
| 12 | 冒烟数据清理 | ✅ 厂商 / 模型 / 偏好均已删除 |

> 冒烟脚本：`/tmp/smoke_llm.sh`（一次性验证用，未入库）。
> 生产环境必须设置 `LLM_KEY_ENCRYPT_KEY`（≥32 字节），否则 `encrypt_api_key` 失败会**回退明文入库**（与桌面命令一致，属既有行为）。

**已落地验证（2026-11-14 · 上传版本历史 1 条）**

| 校验 | 结果 |
|---|---|
| `cargo check -p api --tests` | ✅ 无 error（仅既有告警） |
| `cargo test -p api --lib` | ✅ **96 passed / 0 failed**（含 `upload_routes::tests::version_item_is_camel_case`） |
| `cargo test -p apidoc` | ✅ 4 passed（`path_template_params_are_consistent` 对 `{fileGroupId}` 亦通过） |
| `pnpm -C apps/web test -- httpAdapter` | ✅ 4 passed（`REGISTERED_COMMANDS` 123 条全部被映射表或 Tauri-only 覆盖） |
| `pnpm -C apps/web test -- uploadService` | ✅ **24 passed**（新增 4 条 `getVersionHistory` 用例：路径正确 / 带令牌 / ID 编码 / 失败抛错） |
| 实机冒烟（库 `ceshi8`，schema `single`，`file_uploads` 手工造 4 行） | ✅ 14 项全绿 |

| # | 场景 | 结果 |
|---|---|---|
| 1 | 造数：同组 `v1 committed` / `v2 committed(is_latest)` / `v3 pending` / `v9 committed 但 deleted=1` | ✅ 4 行入库（唯一索引 `(file_group_id, version)` 允许同组多 `is_latest`） |
| 2 | `GET /api/upload/versions/{group}` | **200**，返回 **2** 条（`pending` 与 `deleted` 均被过滤） |
| 3 | 排序 | 首条 `version=2`（`ORDER BY version DESC`） |
| 4 | 字段形状 | `id,version,isLatest,originalFilename,fileSize,fileUrl,changeReason,createdAt` 齐全 |
| 5 | 字段命名 | 无任何含下划线的键（纯 camelCase，与命令一致） |
| 6 | `id` 类型 | **字符串** `"990002"`（非数字，避免 JS 精度丢失） |
| 7 | `isLatest` | `true`（v2） |
| 8 | `createdAt` | RFC3339（`2026-11-13T07:49:07.809372+00:00`） |
| 9 | 不存在的分组 `GET …/no-such-group-xyz` | **200 + `[]`**（空数组而非 404） |
| 10 | 无令牌 `GET …/{group}` | **401** |
| 11 | `sys_menu` id 308 | ✅ 已补录 `GET /api/upload/versions/{fileGroupId}` → `upload:file:list` |
| 12 | `/api-docs/openapi.json` | ✅ 共 **99** 条路径（较 LLM 模块 +1），含 `/api/upload/versions/{fileGroupId}` |
| 13 | 兄弟路由未被遮蔽 | ✅ `GET /api/upload/999999/progress` 仍命中旧 handler（`上传记录不存在`，500 为既有语义）；`GET /api/upload/versions/abc` 为 **200 + `[]`**（`fileGroupId` 是字符串，不做数字校验） |
| 14 | 冒烟数据清理 | ✅ `file_uploads` 冒烟行已删除 |
| 15 | 前端路径修正 | ✅ `HttpUploadAdapter.getVersionHistory` 由废弃的 `/upload/version-history/{id}` 改为 `/upload/versions/{id}`（此前 Web 模式必 404） |

> 冒烟脚本：`/tmp/smoke_upl.sh`（一次性验证用，未入库）。
> 注意 `file_uploads` 无自增（`id` 为 `bigint` 无默认值），冒烟需显式给 `id`；`created_by` 在 DTO 里是 `i64`（非 Option），造数时不能留空。






---

## 13. 变更记录

| 日期 | 内容 |
|---|---|
| 2026-11-13 | 初版：三方比对（154/112/32/20）、42 条路由清单、T2 定稿（桌面纯壳）、S-1~S-6 前置、R1–R16 前端删除清单、分批与验收 |
| 2026-11-13 | P0-A 第 1 模块落地：单据编号 4 条路由（含 6 条单测）+ 权限点 + Swagger + 前端映射；补「请求体形状原则」1.3.4.1；新增 §12 实施进度 |
| 2026-11-13 | **P0-0 修复**：`auth.rs::verify_token` 改用 `jwt_validation()`（显式 `set_audience`/`set_issuer`），解决 `jsonwebtoken` 11 下「令牌带 `aud` → 全部受保护接口 401」；新增 5 条回归单测（带 aud 通过 / 历史令牌放行 / 受众不符拒绝 / refresh 拒绝 / 校验参数与签发参数一致） |
| 2026-11-13 | 实机冒烟通过（`assets-server` + `next dev`）：登录 → 编号规则读写 200、Swagger 84 路径、Web `/login` 200；启动步骤沉淀到 `docs/桌面与Web双形态部署指南.md` §3.0 |
| 2026-11-13 | 启动日志修正：`crates/api/src/lib.rs` 改为**先 bind 再打印**「HTTP API 服务启动于」；新增 `bind_listener()` / `bind_error_message()`，端口占用时直接给出 `ss -ltnp` / `pkill` / 「改 `.env.toml` 的 `[api] port` 而非 `API_PORT`」提示（+2 条单测）。指南 §3.0 增补「端口占用与多实例」 |
| 2026-11-14 | **P0-A 第 2 模块落地：会话管理 6 条路由**。新增 `crates/api/src/conversation_routes.rs`（6 handler + 11 条单测）；`lib.rs` 把 LLM Router 构建重构为一次构建、两处挂载（`chat_router` / `conversation_router`）；服务层新增**归属闸门** `ensure_conversation_owned()`、`find_conversation()` 与 3 个 `*_for_user` 方法（改标题/删除把 `user_id` 折进同一条 UPDATE 的 WHERE，规避 TOCTOU），旧 `"无权访问此会话"` 字符串全部替换；`authz.rs` 补 6 条 `ROUTE_PERMS` + 6 条 `SERVICE_FORBIDDEN_ROUTES`；`public_migration.sql` 补 `sys_menu` 231–236；`openapi.rs` 登记 6 条；`httpAdapter.ts` 补 6 条映射并从 `TAURI_ONLY_COMMANDS` 移出。**安全口径**：身份只取令牌 `sub`，越权返回 **404**（与"不存在"同文案，避免探测）；body/query 的 `userId` 仅比对告警 |
| 2026-11-14 | 会话管理验证全绿：`cargo test -p api --lib` **79 passed**、`cargo test -p service conversation` **4 passed**、`cargo test -p apidoc` **4 passed**、`pnpm test -- httpAdapter` **4 passed**；实机冒烟 15 项（含两用户越权、伪造 `userId`、软删后 404、`sys_menu` 自动补录、Swagger 88 路径） |
| 2026-11-14 | **P0-A 第 3 模块落地：知识资产 OKF 7 条路由**。新增 `crates/api/src/knowledge_asset_routes.rs`（7 handler + 8 条单测）；服务层把 `fetch_one` 换成 `fetch_optional` + 新增 `KNOWLEDGE_ASSET_NOT_FOUND`（→404）、删除改为 `deleted=0` 条件下 UPDATE 并检查 `rows_affected`（幂等 404）、抽出 `build_knowledge_asset()` 供**桌面命令与 HTTP 路由共用**（消除默认值漂移）；`knowledge_asset_commands.rs` 改为调用共用构造器；`authz.rs` 补 7 条 `ROUTE_PERMS`（复用 `knowledge:*`）；`public_migration.sql` 补 `sys_menu` 291–297；`openapi.rs` 登记 7 条；`httpAdapter.ts` 补 7 条映射并移出 `TAURI_ONLY_COMMANDS`。**RK-1 路由歧义实测不成立** |
| 2026-11-14 | 知识资产验证全绿：`cargo test -p api --lib` **87 passed**、`cargo test -p service knowledge` **3 passed**、`cargo test -p apidoc` 4 passed、`pnpm test -- httpAdapter` 4 passed；实机冒烟 18 项（CRUD + 类型过滤 + 按节点查询 + 绑定文件 + 404/400/401 + 软删幂等 + Swagger 92 路径 + 伪造审计人被忽略） |
| 2026-11-14 | **P0-A 第 4 模块落地：LLM 厂商/模型/用户偏好 12 条路由**。新增 `crates/api/src/llm_routes.rs`（12 handler + 8 条单测，`ProviderView` **强制脱敏** `api_key=******` 且不含 `secret_key`/`extra_config`）；更新时回传占位符视为「不修改」（`normalize_api_key`）；服务层抽出 `build_provider_entity()` 供桌面命令与 HTTP 共用，新增 `LLM_PROVIDER_NOT_FOUND`/`LLM_MODEL_NOT_FOUND`（→404），删除加 `deleted=0` 条件与 `rows_affected` 检查；`authz.rs` 补 10 条 `ROUTE_PERMS` + 2 条 `AUTH_ONLY_ROUTES`（用户偏好仅需登录）；`public_migration.sql` 补 `sys_menu` 298–307；`openapi.rs` 登记 12 条；`httpAdapter.ts` 补 12 条映射并移出 `TAURI_ONLY_COMMANDS` |
| 2026-11-14 | LLM 验证全绿：`cargo test -p api --lib` **95 passed**、`cargo test -p service llm` **16 passed**、`cargo test -p apidoc` 4 passed、`pnpm test -- httpAdapter` 4 passed；实机冒烟 12 项（密钥密文入库不回显、占位符不覆盖真实 Key、偏好取令牌、404/400/401、Swagger 98 路径） |
| 2026-11-14 | **P0-A 第 6 模块落地：上传版本历史 1 条路由**。`upload_routes.rs` 新增 `get_version_history`（复用 `UploadRouterState.upload_mgr`，无重复构造 S3 客户端）+ `UploadVersionItem`（camelCase 逐字对齐命令，`id` 为字符串、`createdAt` RFC3339）+ 1 条序列化单测；`lib.rs` 在 `upload_router` 挂 `GET /api/upload/versions/{fileGroupId}`（占位符复用命令参数名，`extractParams` 才能剥离；静态段不遮蔽 `/api/upload/{id}/progress`）；`authz.rs` 复用既有 `upload:file:list`；`public_migration.sql` 补 `sys_menu` 308；`openapi.rs` 登记；`httpAdapter.ts` 加映射并移出 `TAURI_ONLY_COMMANDS`（**该集合现仅剩 RAG 2 条**）。**路由从原计划的 `/api/upload/{uploadId}/versions` 改为 `/api/upload/versions/{fileGroupId}`** |
| 2026-11-14 | 上传版本历史验证全绿：`cargo test -p api --lib` **96 passed**、`cargo test -p apidoc` 4 passed、`pnpm test -- httpAdapter` 4 passed、`pnpm test -- uploadService` **24 passed**；实机冒烟 15 项（pending/deleted 过滤、倒序、纯 camelCase、`id` 字符串、空分组 200+`[]`、401、兄弟路由未遮蔽、Swagger 99 路径、冒烟数据已清理）。同时修掉前端 `HttpUploadAdapter.getVersionHistory` 指向**不存在**的 `/upload/version-history/{id}`（Web 模式此前必 404），改为 `/upload/versions/{id}` + `encodeURIComponent`，并补 4 条单测 |

---

## 附：本轮涉及的代码锚点一览

| 主题 | 文件:行 |
|---|---|
| 桌面初始化与 154 命令 | `apps/backend/src-tauri/src/lib.rs:13-250` |
| HTTP 服务与 CORS | `crates/api/src/lib.rs:124-169`、`:147-157` |
| 网关路由（无 CORS） | `crates/gateway/src/router.rs:44-73` |
| 用户上下文范式 | `crates/api/src/chat_routes.rs:15` |
| 启动绑定与端口占用提示 | `crates/api/src/lib.rs:87-114`（`bind_listener` / `bind_error_message`）、`:204-205`（先 bind 后打印） |
| Skill 双实例 | `crates/api/src/skill_routes.rs:15` vs `src-tauri/src/lib.rs:40` |
| 前端映射表 | `apps/web/src/utils/adapters/httpAdapter.ts:57-216`（映射）、`:234-278`（Tauri-only）、`:293-316`（extractParams） |
| 前端硬编码基址 | `httpAdapter.ts:36`、`useChatStream.ts:39`、`uploadService.ts:190,353` |
| Tauri 上传实现 | `uploadService.ts:11,92-160,128,311-322,394-410` |
| 适配器探测 | `app/layout.tsx:33-54` |
| 认证状态 | `store/authStore.ts:67-80`（登录持久化） |
| JWT 校验（P0-0 修复点） | `crates/api/src/auth.rs:236-265`（`jwt_validation`）、`:253-269`（`verify_token`） |
| 测试假通过 | `tests/utils/adapters/httpAdapter.test.ts:22,179-196` |
| 沙箱既有路由 | `crates/api/src/lib.rs:532-564` |






