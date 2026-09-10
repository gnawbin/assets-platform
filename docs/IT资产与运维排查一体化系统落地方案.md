# IT资产与运维排查一体化系统 · 落地方案（评审稿）

> **编制依据**：本方案依据《IT资产与运维排查一体化系统设计文档（最终定稿）》编制。
> **落地形态**：面向现有仓库 `assets-platform`（Tauri v2 桌面壳 + Rust 分层 crates + Next.js 前端 + PostgreSQL 多租户 + RBAC + 审批流 + LLM 多厂商网关）做**增量扩展**，而非另起炉灶。
> **状态**：评审稿 · **日期**：2026-09-09

**目录**
0. 总体结论
1. 现状盘点：可复用 vs 要新建
2. 资产分类与数据模型改造
3. 采集发现引擎（assets-ops）
4. 日志排查 + AI 故障总结
5. 可控启停与工单管控
6. 安全与权限
7. 数据库变更清单
8. 前端改造要点
9. 与"最终定稿"一致性核对
10. 实施节奏与任务拆分
11. 风险与待确认事项

---

## 0. 总体结论（先看这条）

**默认路线：在现有 assets-platform 上做增量扩展，而不是推翻重建。**

1. 设计文档要求的能力中，资产台账、分类树、审批/工单、RBAC 权限、审计留痕、LLM 调用、文件上传等在仓库中**已存在且较成熟**，直接复用。
2. 与"最终定稿"存在三处技术选型差异，处理如下（详见 §9）：
   - **主数据库不迁 SurrealDB**：延续 PostgreSQL + sqlx（多租户 schema 隔离、读写分离、审批/RBAC 全部现成）；SurrealDB 继续仅作向量库。
   - **本地 LLM 不硬绑 llama.cpp**：复用现有 LLM 多厂商网关（`llm_gateway_service`），把 Ollama/llama.cpp 注册为本地端点即可，同时保留云端大模型选项。
   - **采集器需要常驻宿主**：桌面 Tauri 不适合 7×24 采集调度。新增采集引擎 crate，支持两种跑法：① 随 Axum HTTP 服务进程常驻调度；② 独立 collector 二进制，部署在可触达 Docker/K8s 的网络位置。
3. 实施节奏对齐"最终定稿"第 8 章版本规划：
   **V1.0** 5 大分类落地 + Docker/Swarm/K8s 自动采集 + 台账可视化 → **V1.1** 日志按需排查 + AI 故障自动总结 + 运维历史审计 → **V1.2** 可控启停 + 工单联动 + 停机窗口 + 权限精细化。

---

## 1. 现状盘点：可复用 vs 要新建

| 设计文档能力 | 现有仓库可复用 | 需新建 |
|---|---|---|
| 资产主数据 / 台账 | `assets` 主表 + `asset_category` 分类树 + 多租户 PG + 编号规则 | 5 大一级分类体系、运维字段扩展、短时动态资产规则 |
| 资产状态 / 软删除 | 资产 `status`、`deleted` 软删 | 状态枚举升级为文档 8 态；资源销毁统一置「已下线」 |
| 权限 / 鉴权 | JWT + 角色/菜单权限（Tauri 命令层 + Axum HTTP 双通道） | 运维专属角色/权限点、连接凭证分级 |
| 工单审批 | `AssetApproval` + wfe 审批流（领用/归还/调拨/维修/报废/采购） | 变更工单 / 停机窗口工单类型与强制前置校验 |
| AI 能力 | LLM 多厂商网关 + 加密 API Key + 模型配置页 | 日志脱敏 + 故障总结固定 Prompt 流程 |
| 审计 | 操作留痕（created_by 等） | append-only 高危审计表、采集流水事件 |
| 前端 | Mantine + 台账/流程/设置/聊天页面体系 | 5 大类台账改造、日志排查工作台、链路溯源、连接管理页 |
| Docker / Swarm / K8s 采集 | —（暂无相关代码） | **新增 `assets-ops` crate**：bollard + kube-rs 连接器、调度器、对账器 |
| 日志拉取 / AI 总结 | — | 日志接口（含 K8s previous 崩溃日志）、脱敏、AI 总结落库 |

---

## 2. 资产分类与数据模型改造（V1.0 核心）

### 2.1 一级大类改造（分类数据与资产数据两处一致升级）

现有 `asset_category` 只有 `fixed` / `intangible` 两个根分支，`assets.asset_type` 只有 `hardware` / `intangible`。改造方案：

- `asset_category`：一级分类（parent_id=0）收敛为文档 **5 大一级分类**，并新增 `category_scope` 标注大类性质：
  - **实物硬件资产（physical，财务固定资产｜可折旧｜可入账）**：计算设备 / 网络设备 / 存储硬件 / 机房动力环境 / 办公与工业外设
  - **软件资产（software，许可/授权｜部分固资、大部分费用化）**：商业授权软件 / 开源软件
  - **IT 逻辑资产（logical，系统核心｜无折旧｜纯运维资产）**：虚拟化（KVM/VMware）→ 单机 Docker（容器/镜像/网络/卷）→ Docker Swarm（集群/Node/Service/Task）→ K8s（集群/Node/Namespace/工作负载/Pod/Service/Ingress/PV·PVC/ConfigMap·Secret）
  - **云租赁资产（cloud，费用化｜无固定资产）**：云 ECS/RDS、托管 K8s、OSS、云中间件 / SaaS
  - **数据资产（data，运维台账类）**：业务数据库实例、生产数据集、文件资源、系统核心配置
- **短时动态资产标记**：容器/Pod/Swarm Task 归属 IT 逻辑资产下的"短时动态资产"子分类（`is_short_lived=1`）。
- **存量兼容策略**：迁移脚本把旧 `fixed` 各分支映射到"实物硬件资产"下对应子类、旧 `intangible` 映射到"软件资产"下，数据保留不物理删除、历史可追溯；旧 `asset_type` 取值做代码层别名兼容，避免一次性大改造成存量不可用。

### 2.2 `assets` 主表运维字段扩展（与文档 3.1 对齐）

现有表已含 `asset_no / purchase_date / purchase_price / expire_date / status / deleted` 等字段。**新增运维核心字段（全资产生效，留空不做强制校验）**：

> `resource_uid`（资源唯一 ID：容器 ID / K8s UID / Swarm ID）、`env_tag`（生产/测试/开发）、`mgmt_ip`（管理 IP）、`ops_owner_id`（运维负责人）、`backup_owner_id`（备份负责人）、`biz_owner`（业务归属）、`maintenance_window`（jsonb 停机窗口）、`monitor_status`（监控接入状态）、`fixed_asset_id`（关联固定资产 ID，逻辑节点→物理机全链路）、`dependency_asset_ids`（依赖资产 ID）、`source_connection_id`（采集来源连接器）、`is_short_lived`（短时动态资产）、`last_seen_at`（最后发现时间）、`discovered`（手动/自动来源）

- 财务字段区（采购/原值/折旧/维保/报废等）仅对 **physical** 大类生效；其余大类留空，不做强制校验。
- 短时动态资产：服务端强校验**禁止录入**采购、原值、折旧、维保、报废字段；前端直接隐藏财务字段区。

### 2.3 状态与软删除规则

- 状态枚举升级为文档 8 态：**正常运行 / 维修中 / 待变更 / 待下线 / 已下线 / 已报废 / 告警屏蔽 / 冻结**（含与现有数值状态的映射迁移）。
- 资源销毁不物理删除：采集发现资源消失 → `status=已下线`，保留审计与历史追溯；用户手动删除走既有 `deleted` 软删并写审计。
- **不另设"运维资产"大类**：运维是资产扩展属性，不是资产分类；通过 physical=固资、logical/cloud/data=运维台账的建模天然保证一套主数据同时服务财务、IT、运维。

---

## 3. 采集发现引擎（V1.0 · 新 crate `assets-ops`）

### 3.1 代码落点（沿用现有 crates 分层规范）

```
apps/backend/src-tauri/crates/assets-ops/
├── Cargo.toml                    # 依赖：bollard、kube-rs、tokio、tracing
└── src/
    ├── lib.rs                    # CollectorRegistry + 启停统一入口
    ├── model.rs                  # 归一化资源模型 ResourceRecord（采集产物）
    ├── connector/mod.rs          # Connector trait：collect / watch / health
    ├── connector/docker.rs       # bollard：单机容器/镜像/网络/卷
    ├── connector/swarm.rs        # bollard swarm API：集群/Node/Service/Task（仅连 Manager）
    ├── connector/k8s.rs          # kube-rs：Watch 优先、异常降级轮询
    ├── reconciler.rs             # 快照 diff → 新增/变更/已下线 upsert
    ├── scheduler.rs              # tokio 定时轮询 + watch 事件分发
    ├── log_fetcher.rs            # 按需拉日志（tail/行数/字符上限/previous）
    ├── operator.rs               # 启停动作统一入口（Docker/Swarm/K8s）
    └── ai_summary.rs             # 日志脱敏 + 固定 Prompt + 调 LLM 网关
```

配套接线：

- `assets-api/src/ops_routes.rs`（Axum HTTP REST）+ Tauri `src/commands/ops_commands.rs`（桌面 IPC），**共享同一 service 层逻辑，防止命令层绕过校验**。
- 前端 `apps/web/src/app/ops/*`、`src/services/opsService.ts`；`packages/shared` 增加 ops 共享类型。

### 3.2 关键设计决策

| 点 | 决策 |
|---|---|
| 资源唯一身份 | `resource_uid = provider + endpoint + kind + namespace/name`，幂等 upsert 的唯一键 |
| 对账（Reconciler） | 每轮将"该连接器历史发现的资产"与新快照 diff：新增→insert；元数据/状态变化→update；消失→**标记已下线**，不 DELETE |
| Docker / Swarm 采集 | 定时轮询（文档 4.1.1）；Swarm 仅连接 Manager 节点，Worker 仅采本地容器 |
| K8s 采集 | kube-rs 优先 Watch 增量监听，异常自动降级为轮询；ServiceAccount 仅授 `get/list/watch` 只读权限 |
| 调度宿主 | Axum 服务进程内常驻调度（推荐）或独立 collector 二进制；多实例用 PG advisory lock 防重复采集 |
| 分类挂载 | kind → IT 逻辑资产下二级分类 code 映射表；Node/主机类资源按 hostname/资源 ID 自动或人工绑定 `fixed_asset_id`，实现容器→节点→物理机全链路溯源 |
| 云资产 | 预留 `provider=cloud` 连接器插槽，具体云 API 采集在 V1.0 之后评估 |
| 采集配置 | 存入 `ops_connection`（§7），凭证 AES-GCM 加密落库，页面提供"测试连接/启停采集" |

---


## 4. 日志排查 + AI 故障总结（V1.1）

1. **按需拉取，无后台批量轮询**（文档 4.3.1）：资产详情页"日志排查"按钮触发
   - Docker/Swarm：`logs`（tail 200~500 行 + timestamps）
   - K8s：`logs(namespace/pod/container, tail, previous=true)`，适配 CrashLoopBackOff 崩溃容器场景
   - 行数与字符硬上限、并发拉取限量，防止 LLM 过载与资源占用
2. **原始日志不入库**：仅内存/前端临时展示，防数据库膨胀（与文档 4.3.1 一致）
3. **脱敏先行**：内置正则清单（Bearer Token、密钥、明文账号密码、连接串）→ `***`，在送 LLM 前执行（文档 5.2）
4. **AI 故障总结**：固定 Prompt 模板——识别报错关键字 → 推导 1~2 个核心根因 → 输出可执行排查步骤；调用 `llm_gateway`，默认路由本地 Ollama/llama.cpp 端点模型
5. **结果落地**：AI 结论文本写入 `ops_history`（运维历史子表）；前端提供"**一键同步工单问题描述**"
6. **前端**：日志查看组件（高亮/折叠/时间线）+ AI 结论卡片 + 一键建单按钮

---

## 5. 可控启停与工单管控（V1.2）

### 5.1 动作集（文档 4.2）

| 场景 | 允许动作 | 说明 |
|---|---|---|
| Docker 单机容器 | start / stop / restart | — |
| Docker Swarm | 修改 Service 副本数（0 / 原值） | 不直接操作单个 Task |
| K8s | 修改 Deployment/StatefulSet 副本数（0 / 原值） | **禁止直接删除 Pod**（会自动重建） |

### 5.2 强制前置校验（顺序执行，任一不过即拒绝）

1. JWT 登录鉴权（文档 5.1：所有运维/日志/启停接口必须登录）
2. 角色权限 = 运维管理员（含租户与资源范围校验）
3. **已审批变更工单**绑定校验（复用 `AssetApproval`/wfe 扩展 biz_type，或新增变更工单表，落地时二选一）
4. **停机窗口校验**：生产资产仅允许在 `maintenance_window` 内操作
5. 前端二次确认（必须填写工单号 + 操作原因）

通过后：**先写 append-only 审计 → 再执行 → 结果回写 assets + ops_history**；操作全程留痕（文档 4.2.3）。

### 5.3 并发与失败处理

- 同一资源操作加锁（复用 workflow lock / PG advisory lock），防止并发启停冲突。
- 执行失败记录错误并回滚（恢复到执行前状态），不留下不一致状态。
- 默认**关闭裸操作权限**，所有启停动作必须走到工单+窗口流程（文档 4.2 前提）。

---

## 6. 安全与权限（贯穿三个版本）

1. **客户端权限最小化**：
   - K8s 三套 RBAC 制品（存放 `script/`）：① 采集账号仅 `get/list/watch` 只读；② 日志排查账号单独授 `pods/log`；③ 启停账号仅 `scale` 且需单独审批授权。
   - Docker API **没有细粒度权限**（重点坑）：应用层强制**连接分级**——发现与日志走"只读连接"；启停走"受控连接"并强制过 §5 全量校验；socket 建议 TLS / 限网段暴露。
2. **禁止 SSH 登录容器**：统一 docker exec / kubectl exec 内核命名空间接入，杜绝容器内 SSHD（文档第 6 章避坑规则）。
3. **K8s 严禁连接节点 docker.sock 采集**：一律经 kube-rs 走 APIServer。
4. **接口安全**：所有运维/日志/启停接口挂 JWT + 独立权限点；Tauri 命令层与 Axum 层共享同一 service 做同样校验，防命令层绕过。
5. **审计不可删除**：`ops_audit_log` 从代码与数据库双侧保证只 INSERT 不可 UPDATE/DELETE；高危操作留存不可删。
6. **配置与密钥**：连接串/kubeconfig/Token 复用 `assets-utils` AES-GCM 加密落库，界面不明文回显。
7. **可观测**：沿用 OpenTelemetry，高危操作单独打 trace + 审计日志。

---


## 7. 数据库变更清单（新增迁移 SQL）

新增 `apps/backend/src-tauri/crates/assets-database/src/sql/ops_module_migration.sql`（风格对齐既有 `knowledge_module_migration.sql`；public / 租户 schema 归属按现有租户隔离规范执行）。

| 变更对象 | 内容 | 所在 |
|---|---|---|
| `asset_category` | 新增 5 大一级分类及二级/三级子类预置数据（IT 逻辑资产各分类含 Docker/Swarm/K8s 资源类型） | 迁移 SQL + tenant_initial 数据 |
| `assets` 增列 | §2.2 运维核心字段（一次性 `ALTER` 加齐） | 迁移 SQL |
| `ops_connection` | 采集连接配置：provider（docker/swarm/k8s/cloud）、endpoint、auth_type、**加密凭证**、轮询间隔、启用开关、最近采集时间/错误、归属租户 | 新表 |
| `ops_history` | 资产运维历史子表：关联资产 ID、操作类型（故障排查/启停/变更/巡检/AI总结）、操作人/时间、关联工单 ID、AI 结论、备注 | 新表 |
| `ops_audit_log` | 高危操作审计（append-only）：用户、租户、动作、目标类型/ID、脱敏请求参数、结果、耗时、IP/UA | 新表 |
| `ops_discovery_event` | 采集流水（新增/变更/已下线），用于采集审计与排查，定期归档清理 | 新表 |
| 审批工单扩展 | `AssetApproval` biz_type 增加"变更/停机"类型（或新增变更工单表，落地时二选一） | 扩展/新表 |

约束要点：主键沿用 Snowflake；所有 i64 ID 序列化为字符串，规避前端大整数精度丢失（仓库已有约定）；`ops_audit_log` 建 DB 层禁止 UPDATE/DELETE 的兜底（如 trigger 报错）。

---

## 8. 前端改造要点

1. **台账页改造**：顶部 5 大一级分类 Tab；固定资产视图（财务字段）与运维台账视图（运维字段）双模式切换；IT 逻辑资产下容器/Pod/Task 显示"短时动态"角标且隐藏财务字段区。
2. **链路溯源**：资源详情页提供"链路"视图（Pod→Node→物理服务器→固定资产），落地后升级可视化拓扑。
3. **运维工作台**：日志查看面板 + AI 总结卡片 + 启停操作面板（展示前置校验状态、工单选择器、停机窗口提示）。
4. **管理页面**：`ops/history`（运维历史）、`ops/audit`（审计）、`settings/ops-connections`（采集连接管理：增删改/测试连接/启停采集）。
5. **权限接入**：系统菜单 + 角色权限点新增运维模块；service/types 双通道（HTTP fetch / Tauri invoke）同步提供。

---


## 9. 与"最终定稿"一致性核对

✅ **已对齐的能力点**

- 资产 5 大一级分类、"无运维资产大类"原则、固资折旧与逻辑资产台账分离、短时资产禁填财务字段、销毁不删库（置已下线）、Node↔物理机全链路。
- 采集选型 bollard + kube-rs；Swarm 仅连 Manager；K8s Watch 优先 + 异常降级轮询；SA 只读最小权限。
- 日志按需拉取、200~500 行限制、崩溃容器 `previous=true`、原始日志不持久化。
- AI 固定 Prompt、结论落运维历史、一键同步工单描述。
- 启停前置校验（权限 + 已审批工单 + 停机窗口）+ 全程审计、默认关闭裸操作权限。
- 禁止 SSH 进容器（统一 exec）、K8s 严禁连节点 docker.sock（一律 APIServer）。

⚠️ **技术选型差异与处理建议**

| 最终定稿 | 仓库现状 | 本方案建议 |
|---|---|---|
| 主库 SurrealDB | PostgreSQL + sqlx（多租户/审批/RBAC 现成） | **保留 PG 承载主数据**；SurrealDB 维持向量库定位 |
| 本地 LLM（llama.cpp） | LLM 多厂商网关 + Ollama 适配 | 复用网关，Ollama/llama.cpp 注册为本地端点 |
| 服务端系统假设 | 桌面 Tauri + 可选 Axum 服务 | 采集调度支持服务常驻 / 独立 collector 两种形态 |
| Docker Socket 只读采集 | — | 应用层连接分级 + 受控连接校验（Docker API 无细粒度权限） |

---

## 10. 实施节奏与任务拆分

### V1.0 —— 分类落地 + 自动采集 + 台账可视化
1. 分类体系迁移 SQL（5 大类 + IT 逻辑资产分类 + 旧数据映射）并做存量数据对账。
2. `assets` 运维字段扩展 + 表单/接口改造 + 状态枚举升级。
3. 前端台账页 5 大类改造 + 短时动态资产展示规则。
4. `ops_connection` 连接管理 + 连通性测试（UI + API）。
5. `assets-ops` crate 骨架 + **Docker 采集**（容器/镜像/网络/卷）。
6. **Swarm / K8s 采集** + 对账入库 + 台账展示 + 链路溯源基础。
7. 编译/集成验证：bollard + kube-rs 特性裁剪进桌面包体积评估。

### V1.1 —— 日志排查 + AI 总结 + 审计
8. 日志拉取接口（Docker/K8s，含 previous 崩溃日志）与安全上限。
9. 日志脱敏组件 + 固定 Prompt + LLM 网关接入（AI 故障总结）。
10. AI 结论落 `ops_history`、一键同步工单；运维工作台 UI + 历史页 + 审计页。
11. 采集流水 `ops_discovery_event` 与全链路 OTel 观测补齐。

### V1.2 —— 可控启停 + 工单管控 + 权限精细化
12. 启停动作引擎（Docker/Swarm scale/K8s scale）+ 失败回滚。
13. 前置校验链路：角色权限 → 变更工单（biz_type 扩展）→ 停机窗口 → 二次确认。
14. 高危操作审计硬化（append-only 双保险）+ 权限点/菜单细化。
15. 演练与验收：变更演练、回滚演练、审计抽检。

每个阶段交付：迁移 SQL + Rust 单测 + 前端 Jest/Storybook + 设计文档同步。

---

## 11. 风险与待确认事项

| # | 事项 | 说明 / 建议决策 | 影响 |
|---|---|---|---|
| 1 | **部署形态** | 桌面单机 vs 服务器常驻 Web？本方案按"两种跑法都支持"设计，需确认主形态以定调度宿主优先级 | 采集调度、连接凭证管理方式 |
| 2 | 存量数据迁移 | 现有固定资产/无形资产分类迁移口径：旧分类映射进 5 大类、保留历史，是否接受 | V1.0 建表与数据对账 |
| 3 | 采集目标网络 | Docker/K8s 集群与部署机网络拓扑、凭证保管（kubeconfig / SA Token 轮换） | 连接器实现与安全 |
| 4 | bollard + kube-rs 编译 | 新增依赖对 Tauri 桌面包体积/编译时长的影响（kube-rs 特性裁剪） | V1.0 开工前先做编译验证 |
| 5 | 云资产连接器 | 文档有云租赁大类但未给云 API 细节；默认 V1.0 后置，需确认 | 范围 |
| 6 | 审批二选一 | 变更/停机工单：扩展现有 AssetApproval biz_type，还是新增变更工单表 | §5 前置校验实现 |

**前提声明**：本方案部分内容基于仓库代码与已有文档核对得出，如与近期实际分支有出入，以代码现状为准并在实施中修订。

---

*（完）— 评审通过后按 §10 排期进入 V1.0 开发。*

