# bpmninja + adk-graph 工作流引擎改造方案

> 状态：设计评审稿（尚未实施）  
> 日期：2026-10-26  
> 适用模块：`apps/backend/src-tauri/crates/workflow`、`apps/backend/src-tauri/src/commands`、`apps/web/src/components/workflow`、`apps/web/src/services/workflowService.ts`  
> 关联文档：`docs/wfe-core 审批流集成设计方案.md`（历史方案，本方案取代其引擎选型）  
> 核心结论：**用 bpmninja 的 `engine-core` + `bpmn-parser` 替换当前占位执行路径；用 adk-graph 承载 AI Agent 循环；Agent interrupt 通过显式 BPMN UserTask 形成人工审批链。**

---

## 1. 背景与目标

### 1.1 背景

当前 `apps/web/src/components/workflow` 已经具备基于 bpmn-js 的流程图设计能力，并能把 BPMN 转换为 `WorkflowDefinition` IR 保存到 `workflow` 表。但 `apps/backend/src-tauri/crates/workflow` 仍然是一个占位实现：

- 使用数据库轮询 + 简单状态更新模拟执行；
- 没有真正的 BPMN token 流转、网关、并行、定时器、消息事件；
- AI Agent 节点没有实际执行器；
- 前端 `workflowService.ts` 在命令失败时会回退到 mock，真实错误容易被掩盖；
- `wfe-core` 依赖仍在 `Cargo.toml` 中，但其步骤模型与 bpmn-js 的 BPMN 语义不一致。

本方案将执行层替换为：

- **bpmninja**：BPMN 2.0 流程编排、token 流转、UserTask / ServiceTask / Gateway / Timer / Message / Signal；
- **adk-graph 2.3.0**：Rust 侧 Agent StateGraph、工具循环、中断与恢复；
- **Tauri 进程内 Worker**：消费 bpmninja ServiceTask，调用 Agent，再把结果写回流程变量；
- **PostgreSQL 持久化**：工作流定义、运行实例、待办任务、Agent checkpoint 全部落库，应用重启可恢复。

### 1.2 目标

1. 用 bpmninja 替换 `crates/workflow` 的占位执行路径，不再依赖 `wfe-core`。
2. BPMN `ServiceTask` 上的 `ai-agent-task` 由 Rust worker 执行，worker 内部运行 adk-graph `StateGraph`。
3. Agent 的 `interrupt` 映射为 BPMN `UserTask` 审批；审批完成后恢复同一 Agent 线程。
4. 支持多租户：每个 tenant schema 拥有独立的引擎实例、worker、持久化和待办查询。
5. 支持应用重启恢复：定义、运行实例、ServiceTask 锁、UserTask、Agent checkpoint 不丢失。
6. 前端可完成：设计 → 保存 → 执行 → 轮询状态 → 查看节点结果 → 审批 → 取消。

### 1.3 非目标（本期不做）

- 不追求完整 Camunda 8 / Zeebe 兼容；只保证当前编辑器产生的 BPMN 子集可执行。
- 不实现分布式多节点 worker 调度；单机 Tauri 进程内 worker 即可，多实例部署留出接口。
- 不在本期实现 MCP、复杂 RAG 重排、Agent 长期记忆、成本核算。
- 不实现动态创建 BPMN 节点；审批节点必须在流程图中显式存在。
- 不迁移资产领用/调拨等旧审批业务表；本方案先解决“可配置工作流 + AI Agent”执行底座。

---

## 2. 关键结论与前置条件

### 2.1 bpmninja 不是 crates.io 单一 crate

实际仓库：`https://github.com/maatini/bpmninja`，相关 crate：

| crate | 版本 | 作用 | 引入方式 |
|---|---|---|---|
| `engine-core` | 0.7.25 | BPMN 引擎、token、任务、持久化 port | git commit 固定或 vendor |
| `bpmn-parser` | 0.7.25 | BPMN 2.0 XML → `ProcessDefinition` | git commit 固定或 vendor |

crates.io 上的同名 `engine-core` 是另一个项目，**不能** `cargo add engine-core`。

### 2.2 不使用 engine-server + NATS

bpmninja 的 `engine-server` 默认 `REQUIRE_NATS=true`，依赖 NATS JetStream 做持久化与事件。桌面端不应额外部署 NATS。

本方案在 Tauri 进程内直接嵌入：

```text
WorkflowEngine::new().with_persistence(Arc<PostgresWorkflowPersistence>)
```

worker 通过 `fetch_and_lock_service_tasks` 主动拉取任务，不依赖 NATS。

### 2.3 工具链

bpmninja `0.7.25` 使用 `edition = "2024"`，`rust-version = "1.99"`。当前本机 `rustc 1.96.0`，必须：

```bash
rustup update stable
rustc --version   # 期望 1.99.1
```

或者把 `engine-core` / `bpmn-parser` vendor 到仓库并降低 MSRV。推荐前者，维护成本最低。

### 2.4 adk-graph

`adk-graph 2.3.0` 已在本项目 `Cargo.lock` 中，MSRV 1.95，满足要求。当前默认使用内存 checkpointer，应用重启会丢失 Agent 断点，因此需要自研 PostgreSQL `Checkpointer`。

### 2.5 推荐决策

| 决策 | 推荐值 | 原因 |
|---|---|---|
| bpmninja 交付 | 固定 git commit + 升级 Rust 1.99.1 | 代码量最小，升级可控 |
| 持久化 | 自研 `PostgresWorkflowPersistence` | 不引入 NATS，桌面端可离线运行 |
| 人工审批 | 显式 BPMN `UserTask` 链 | 不改 bpmninja，流程可视化、可审计 |
| Agent 中断恢复 | `PostgresCheckpointer` | 重启后仍可恢复同一 thread |
| 前端 mock | 改为显式环境变量开关 | 避免真实错误被 mock 掩盖 |

---

## 3. 总体架构

```text
React / bpmn-js
   │  invoke(save_workflow / execute_workflow / ...)
   ▼
Tauri workflow_commands
   │
   ▼
WorkflowService
   ├─ workflow / workflow_execution CRUD
   ├─ BpmnCompiler：解析、规范化 topic/assignee、部署
   └─ TenantWorkflowRuntime
          ├─ bpmninja engine_core::WorkflowEngine
          ├─ PostgresWorkflowPersistence
          ├─ WorkflowWorker
          │     ├─ fetch_and_lock_service_tasks
          │     ├─ ai-agent-task  ──► AgentGraphRunner
          │     ├─ ai-agent-resume ──► 恢复 adk-graph
          │     └─ complete_service_task
          └─ WorkflowProjector
                └─ 同步 InstanceState → workflow_execution / node_results

人工审批：
bpmninja UserTask → PendingUserTask → 前端审批 → complete_user_task
                                      → BPMN 继续到 ai-agent-resume
```

### 3.1 分层职责

| 层 | 职责 | 不做什么 |
|---|---|---|
| 前端 bpmn-js | BPMN 图、节点属性、导入导出 | 不执行流程、不判断业务规则 |
| Tauri commands | 参数校验、租户解析、调用 service | 不直接操作引擎 |
| WorkflowService | 定义 CRUD、部署、启动、查询、取消 | 不运行 Agent 循环 |
| bpmninja engine | token 流转、任务、网关、定时器、事件 | 不调用 LLM |
| WorkflowWorker | 拉取 ServiceTask、执行 Agent、回写变量 | 不渲染 UI |
| adk-graph | Agent 状态机、工具循环、interrupt | 不持久化 BPMN 实例 |
| Postgres | 定义、实例、任务、checkpoint、执行投影 | 不承载运行中对象（内存引擎为准） |

---

## 4. 已验证的依赖接口

### 4.1 bpmninja 引擎 API

```rust
WorkflowEngine::new()
WorkflowEngine::with_persistence(Arc<dyn WorkflowPersistence>)

deploy_definition(ProcessDefinition) -> (Uuid, i32)
start_instance(Uuid) -> EngineResult<Uuid>
start_instance_with_variables(Uuid, HashMap<String, Value>) -> EngineResult<Uuid>
start_instance_latest(&str, HashMap<String, Value>) -> EngineResult<(Uuid, Uuid)>

fetch_and_lock_service_tasks(&self, worker_id: &str, max_tasks: usize, topics: &[String], lock_duration: i64) -> Vec<PendingServiceTask>
fetch_and_lock_service_tasks_with_durations(&self, worker_id: &str, max_tasks: usize, topics: &[(String, i64)]) -> Vec<PendingServiceTask>
complete_service_task(task_id: Uuid, worker_id: &str, variables: HashMap<String, Value>) -> EngineResult<()>
fail_service_task(task_id: Uuid, worker_id: &str, retries: Option<i32>, error_message: Option<String>, error_details: Option<String>) -> EngineResult<()>
extend_lock(task_id: Uuid, worker_id: &str, additional_duration: i64) -> EngineResult<()>
complete_user_task(task_id: Uuid, variables: HashMap<String, Value>) -> EngineResult<()>

get_instance_details(Uuid) -> EngineResult<ProcessInstance>
get_instance_or_archived(Uuid) -> EngineResult<ProcessInstance>
list_instances() -> Vec<ProcessInstance>
list_instances_page(offset: usize, limit: Option<usize>) -> InstancePage
get_pending_user_tasks_enriched() -> Vec<PendingUserTask>
get_pending_service_tasks_enriched() -> Vec<PendingServiceTask>
get_audit_log(Uuid) -> EngineResult<Vec<String>>

suspend_instance(Uuid) -> EngineResult<()>
resume_instance(Uuid) -> EngineResult<()>
delete_instance(Uuid) -> EngineResult<()>
```

实例状态 `InstanceState`：

```rust
Running
WaitingOnUserTask { task_id }
WaitingOnServiceTask { task_id }
WaitingOnTimer { timer_id }
WaitingOnMessage { message_id }
WaitingOnSignal { catch_id }
WaitingOnCondition { catch_id }
WaitingOnEventBasedGateway
ParallelExecution { active_token_count }
WaitingOnCallActivity { sub_instance_id, token }
Completed
CompletedWithError { error_code }
Suspended { previous_state }
```

### 4.2 BPMN 解析规则

`ServiceTask` 的 topic 取值顺序：

```text
data-topic
→ data-handler
→ camunda:topic
→ 节点 id
```

`implementation="##Skill"` **不会**被 bpmninja 识别。必须写 `data-topic` / `data-handler` / `camunda:topic`，或部署前根据 IR 重写 `ProcessDefinition.nodes`。

`UserTask` 的 assignee 取值顺序：

```text
data-assignee
→ camunda:assignee
→ "unassigned"
```

`BpmnElement` 定义：

```rust
ServiceTask { topic: String, multi_instance: Option<MultiInstanceDef> }
UserTask(String) // assignee
```

### 4.3 adk-graph API

```rust
StateGraph::with_channels(&[...])
    .add_node_fn(...)
    .add_edge(START, "...")
    .add_conditional_edges(...)
    .compile()

CompiledGraph::with_checkpointer(...)
CompiledGraph::with_recursion_limit(...)
CompiledGraph::invoke(State, ExecutionConfig)
CompiledGraph::stream(State, ExecutionConfig, StreamMode::Debug)
CompiledGraph::get_state(thread_id)
CompiledGraph::update_state(thread_id, updates)

NodeOutput::with_interrupt(...)
NodeOutput::interrupt_with_data(message, data)

GraphError::Interrupted(InterruptedExecution {
    interrupt,
    state,
    thread_id,
    checkpoint_id,
})
```

`MemoryCheckpointer` 只在内存里，应用重启会丢 Agent 断点。需要实现 `adk_graph::checkpoint::Checkpointer` 的 PostgreSQL 版本。

---

## 5. 核心设计契约

### 5.1 AI Agent 任务标记

BPMN 中保留 `ServiceTask`，并显式写 topic：

```xml
<bpmn:serviceTask id="Task_Agent"
  implementation="ai-agent-task"
  data-topic="ai-agent-task" />
```

`implementation` 只用于前端分类，**不能**作为引擎 topic。实际 topic 以 `data-topic` 或后端重写结果为准。

节点 IR 配置扩展：

```ts
interface EngineFields {
  kind: "agent" | "approval" | "service" | "script" | "none";
  agentId?: string;
  model?: string;
  temperature?: number;
  maxTokens?: number;
  prompt?: string;
  tools?: string;
  knowledgeIds?: string;
  interruptPolicy?: "never" | "before_tool" | "before_final";
  maxIterations?: number;
  timeoutSec?: number;
  approver?: string;
  assignee?: string;
  retries?: number;
}
```

### 5.2 部署前规范化 topic 与 assignee

`BpmnCompiler` 在 `deploy_definition` 之前，根据 IR 的 `node.config` 重写：

```rust
match element {
    ServiceTask { topic, .. } if kind == Agent => *topic = "ai-agent-task",
    ServiceTask { topic, .. } if kind == Resume => *topic = "ai-agent-resume",
    UserTask(assignee) => *assignee = config.assignee,
    _ => {}
}
```

这样即使 bpmn-js 没有保留 `data-topic`，引擎里的 topic 仍然正确。

### 5.3 执行配置快照

`execute_workflow` 启动实例时写入流程变量：

```json
{
  "__workflow": {
    "workflow_id": "123",
    "execution_id": "456",
    "workflow_name": "合同智能分析",
    "node_configs": {
      "Task_Agent": {
        "type": "agent",
        "config": { "prompt": "...", "tools": "knowledge_search" }
      }
    }
  },
  "business_key": "456"
}
```

bpmninja 会把 `business_key` 从变量中取出并写入 `ProcessInstance.business_key`。Worker 从 `PendingServiceTask.variables_snapshot` 中读取节点配置，**不读取数据库中的最新 workflow 版本**，保证运行中实例不受编辑影响。

### 5.4 人工审批链

推荐采用显式 BPMN `UserTask` 链，不修改 bpmninja：

```text
AI ServiceTask
   ↓
ExclusiveGateway（agent_status == waiting_approval）
   ├─ true  → UserTask（camunda:assignee）
   │            ↓
   │          ai-agent-resume ServiceTask
   │            ↓
   │          原后续节点
   └─ false → 原后续节点 / EndEvent
```

Worker 遇到 Agent interrupt 时立即完成当前 ServiceTask，不长期持有锁：

```rust
complete_service_task(task_id, worker_id, vars)
```

```json
{
  "agent_status": "waiting_approval",
  "agent_thread_id": "execution-456:Task_Agent",
  "agent_checkpoint_id": "...",
  "agent_interrupt": {
    "message": "需要人工确认",
    "data": { "tool": "http_request", "arguments": { } }
  },
  "agent_approved": null
}
```

用户审批后：

```rust
complete_user_task(task_id, hashmap! {
    "agent_approved" => json!(true),
    "approval_comment" => json!("同意"),
})
```

流程继续到 `ai-agent-resume`，Worker 恢复同一线程：

```rust
graph.invoke(
    State::from([("approval", json!({ "approved": true }))]),
    ExecutionConfig::new(thread_id).with_resume_from(checkpoint_id),
)
```

如果要求“Worker 动态创建流程图中不存在的 UserTask”，必须 fork `engine-core` 增加 `create_user_task` API；本方案不采用。

### 5.5 状态映射

| bpmninja `InstanceState` | 前端 execution.status | 说明 |
|---|---|---|
| `Running` / `ParallelExecution` | `running` | 正在执行 |
| `WaitingOnServiceTask` | `running` | worker 尚未完成 |
| `WaitingOnUserTask` | `waiting` | 等待人工审批 |
| `WaitingOnTimer` / `WaitingOnMessage` / `WaitingOnSignal` / `WaitingOnCondition` / `WaitingOnEventBasedGateway` | `waiting` | 等待外部事件 |
| `WaitingOnCallActivity` | `waiting` | 子流程执行中 |
| `Completed` | `success` | 正常结束 |
| `CompletedWithError` | `failed` | 错误结束事件 |
| `Suspended` | `waiting` | 暂停，保留 bpmn_status |

同时保留 `bpmn_status` 原始枚举字符串，避免前端状态机丢失细节。

---

## 6. 数据模型与迁移

### 6.1 扩展 workflow 表

```sql
ALTER TABLE {schema}.workflow ADD COLUMN IF NOT EXISTS bpmn_xml TEXT;
ALTER TABLE {schema}.workflow ADD COLUMN IF NOT EXISTS engine_definition_key UUID;
ALTER TABLE {schema}.workflow ADD COLUMN IF NOT EXISTS engine_version INT;
ALTER TABLE {schema}.workflow ADD COLUMN IF NOT EXISTS definition_hash TEXT;
```

### 6.2 扩展 workflow_execution 表

```sql
ALTER TABLE {schema}.workflow_execution ADD COLUMN IF NOT EXISTS engine_instance_id UUID;
ALTER TABLE {schema}.workflow_execution ADD COLUMN IF NOT EXISTS engine_definition_key UUID;
ALTER TABLE {schema}.workflow_execution ADD COLUMN IF NOT EXISTS current_node_id VARCHAR(255);
ALTER TABLE {schema}.workflow_execution ADD COLUMN IF NOT EXISTS agent_thread_id TEXT;
ALTER TABLE {schema}.workflow_execution ADD COLUMN IF NOT EXISTS waiting_task_id UUID;
ALTER TABLE {schema}.workflow_execution ADD COLUMN IF NOT EXISTS interrupt_payload JSONB;
ALTER TABLE {schema}.workflow_execution ADD COLUMN IF NOT EXISTS bpmn_status TEXT;
ALTER TABLE {schema}.workflow_execution ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
```

### 6.3 bpmninja 持久化表

```sql
CREATE TABLE IF NOT EXISTS {schema}.bpmn_engine_object (
    kind TEXT NOT NULL,
    id UUID NOT NULL,
    instance_id UUID,
    definition_key UUID,
    business_key TEXT,
    topic TEXT,
    worker_id TEXT,
    lock_expiration TIMESTAMPTZ,
    data JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (kind, id)
);

CREATE TABLE IF NOT EXISTS {schema}.bpmn_engine_history (
    id UUID PRIMARY KEY,
    instance_id UUID NOT NULL,
    event_type TEXT,
    node_id TEXT,
    timestamp TIMESTAMPTZ NOT NULL,
    data JSONB NOT NULL
);

CREATE TABLE IF NOT EXISTS {schema}.bpmn_engine_file (
    object_key TEXT PRIMARY KEY,
    data BYTEA NOT NULL
);

CREATE TABLE IF NOT EXISTS {schema}.bpmn_engine_completed (
    instance_id UUID PRIMARY KEY,
    data JSONB NOT NULL
);
```

### 6.4 Agent checkpoint 与运行记录

```sql
CREATE TABLE IF NOT EXISTS {schema}.workflow_agent_checkpoint (
    thread_id TEXT NOT NULL,
    checkpoint_id TEXT NOT NULL,
    data JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (thread_id, checkpoint_id)
);

CREATE TABLE IF NOT EXISTS {schema}.workflow_agent_run (
    id BIGSERIAL PRIMARY KEY,
    execution_id BIGINT NOT NULL,
    node_id TEXT NOT NULL,
    engine_task_id UUID,
    thread_id TEXT NOT NULL,
    checkpoint_id TEXT,
    status TEXT NOT NULL,
    config JSONB,
    state JSONB,
    interrupt JSONB,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ
);
```

### 6.5 迁移策略

- 新租户：`init_tenant_tables` 之后调用 `migrate_tenant_tables(pool, schema)`。
- 已有租户：启动时遍历 `public.sys_tenant` 的 `schema_name`，逐个执行 `tenant_workflow_migration.sql`。
- 默认 schema 初始化路径同样补一次。
- 迁移必须幂等：统一使用 `IF NOT EXISTS`。
- 启动时清理过期 ServiceTask 锁：把 `lock_expiration < NOW()` 的 `worker_id`、`lock_expiration` 置空，由 worker 重新拉取。

---

## 7. 后端模块与命令契约

### 7.1 workflow crate 目录

```text
crates/workflow/src/
  lib.rs
  types.rs
  bpmn.rs              # 解析、topic/assignee 规范化、部署
  service.rs           # CRUD、执行、查询、取消
  runtime.rs           # 每租户 runtime 与全局注册表
  worker.rs            # ServiceTask 拉取与分发
  projection.rs        # InstanceState -> workflow_execution
  persistence/
    mod.rs
    postgres.rs        # WorkflowPersistence 实现
    checkpoint.rs      # adk-graph Checkpointer 实现
  agent/
    mod.rs
    config.rs
    graph.rs
    llm.rs
    rag.rs
    tools.rs
    runner.rs
```

删除旧的 `commands.rs`、`definitions.rs`、`executor.rs`、`lock.rs`、`queue.rs`、`steps.rs` 及其测试。

### 7.2 核心职责

| 模块 | 职责 |
|---|---|
| `BpmnCompiler` | `parse_bpmn_xml`、按 IR 重写 topic/assignee、计算 hash、按需 `deploy_definition` |
| `WorkflowService` | 定义/执行 CRUD、启动实例、取消、查询待办 |
| `TenantWorkflowRuntime` | 每个 schema 独立 engine、worker、projector、取消令牌表 |
| `WorkflowWorker` | 拉取并锁定 ServiceTask、心跳续锁、调用 Agent、完成/失败任务 |
| `WorkflowProjector` | 订阅或轮询引擎状态，投影到业务表 |
| `PostgresWorkflowPersistence` | 实现 bpmninja 全部 persistence 方法 |
| `PostgresCheckpointer` | 实现 adk-graph checkpoint 存取 |
| `AgentGraphRunner` | 构建并运行 StateGraph，处理 interrupt/resume |

### 7.3 Tauri 命令契约

Tauri 默认使用 camelCase 参数名：

| 命令 | 参数 | 返回 |
|---|---|---|
| `list_workflows` | `{ page?, pageSize?, keyword?, status? }` | `{ list, total }` |
| `get_workflow` | `{ workflowId }` | `WorkflowMeta + definition` |
| `save_workflow` | `{ id?, name, description?, definition, status?, bpmnXml, nodeTypes, currentUserId? }` | `{ id, version }` |
| `delete_workflow` | `{ workflowId, currentUserId? }` | `{ success }` |
| `execute_workflow` | `{ workflowId, inputData?, currentUserId? }` | `{ executionId, status }` |
| `list_executions` | `{ workflowId?, page?, pageSize? }` | `{ list, total }` |
| `get_execution` | `{ executionId }` | `WorkflowExecutionDetail` |
| `cancel_execution` | `{ executionId, currentUserId? }` | `{ success }` |
| `list_pending_workflow_tasks` | `{ workflowId?, executionId? }` | `WorkflowPendingTask[]` |
| `complete_workflow_user_task` | `{ taskId, variables, currentUserId? }` | `{ success }` |

`save_workflow` 前端目前混用 `bpmnXml` 与 `node_types`，需要统一为 `nodeTypes`。

### 7.4 执行流程

1. 保存未持久化的 definition。
2. `BpmnCompiler` 解析 BPMN，按 IR 重写 topic/assignee。
3. 计算 `definition_hash`；变化时才部署，未变化复用 `engine_definition_key`。
4. 插入 `workflow_execution`，获得 `execution_id`。
5. 组装变量：`input_data` 顶层变量、`__workflow.node_configs`、`business_key = execution_id`。
6. `start_instance_with_variables`。
7. Worker 异步消费；前端轮询 `get_execution`。

---

## 8. Agent 执行器设计

### 8.1 StateGraph 结构

```text
START
  ↓
prepare     # 组装输入、RAG 检索、系统提示词
  ↓
llm         # LLMRouter.chat
  ↓
route（条件边）
  ├─ tool  → tools → llm（循环）
  └─ final → finalize → END
```

### 8.2 Agent State

```rust
struct AgentState {
    messages: Vec<ChatMessage>,
    input: Value,
    scratchpad: Vec<Value>,
    tool_results: Vec<Value>,
    iterations: usize,
    final_output: Option<Value>,
    approval: Option<ApprovalDecision>,
    trace: Vec<Value>,
}
```

`messages` 必须可序列化，checkpoint 才能恢复。

### 8.3 配置

```rust
struct AgentTaskConfig {
    agent_id: Option<String>,
    model: Option<String>,
    temperature: Option<f32>,
    max_tokens: Option<i32>,
    prompt: String,
    tools: Vec<String>,
    knowledge_ids: Vec<i64>,
    top_k: usize,
    output_schema: Option<Value>,
    max_iterations: usize,
    timeout_secs: u64,
    interrupt_policy: InterruptPolicy,
    retries: i32,
}
```

### 8.4 LLM 工具协议

现有 `LLMChatRequest` 没有 function calling，Agent 使用 JSON 协议：

```json
{"action":"tool","tool":"knowledge_search","input":{"query":"合同违约责任"}}
```

```json
{"action":"final","output":{"summary":"...","risks":["..."]}}
```

- 提示词要求模型只输出 JSON，不要 Markdown 代码块。
- 解析失败时，把原文当作 `final_output` 文本，避免流程卡死。
- 工具不在 allowlist 时返回错误文本，由模型决定下一步。
- 每次 LLM 响应累计 token，写入 `workflow_agent_run.state`。

### 8.5 RAG 与工具

| 工具 | 实现 | 默认 |
|---|---|---|
| `knowledge_search` | `RAGRetriever::retrieve(&RetrieveParams)` | 开启 |
| `current_time` | `chrono::Local::now()` | 开启 |
| `http_request` | `reqwest` + 域名 allowlist | 关闭 |
| `skill:<name>` | `SkillRegistry` + `execute_skill` | 关闭，运行时未就绪时明确报错 |

多个 `knowledgeIds` 时逐个调用 `RAGRetriever`，按 chunk id 去重，再按相似度截断 `top_k`。

### 8.6 interrupt 与恢复

中断策略：

- `never`：不中断。
- `before_tool`：执行任何工具前中断。
- `before_final`：输出最终答案前中断。

中断节点：

```rust
Ok(NodeOutput::interrupt_with_data(
    "agent_approval",
    json!({ "tool": tool_name, "arguments": args, "reason": reason }),
))
```

`invoke` 返回 `GraphError::Interrupted` 时：

1. 从 `InterruptedExecution` 取出 `thread_id`、`checkpoint_id`、`interrupt`、`state`。
2. 写入 `workflow_agent_run`。
3. `complete_service_task` 写入 `agent_status=waiting_approval` 等变量。
4. BPMN 进入 UserTask。

审批后：

```rust
graph.invoke(
    State::from([("approval", json!({ "approved": true, "comment": "同意" }))]),
    ExecutionConfig::new(thread_id).with_resume_from(checkpoint_id),
)
```

如果 `checkpoint_id` 在数据库不存在，标记任务失败并给出明确错误，不静默重新开始。

### 8.7 超时、取消与重试

- `tokio::time::timeout(AgentTaskConfig.timeout_secs, graph.invoke(...))`。
- `cancel_execution` 通过 `CancellationToken` 通知 worker；worker 在工具调用前后检查。
- 取消后先停止 Agent，再 `delete_instance`；`workflow_execution.status = cancelled`。
- 失败重试次数来自节点配置，最终调用 `fail_service_task`。
- 长任务每 20 秒 `extend_lock`，默认锁 60 秒。

### 8.8 幂等

`workflow_agent_run.engine_task_id` 唯一索引。worker 重复拉到同一 task 时：

- 已成功：直接 `complete_service_task` 写回已有结果；
- 等待审批：重新写入 interrupt 变量，不重复调用 LLM；
- 运行中且锁属于当前 worker：跳过。

### 8.9 安全边界

- `http_request` 默认关闭，必须配置 allowlist。
- 系统提示词与用户输入分开，避免工具参数注入。
- Agent 输出只作为流程变量，不直接执行 SQL/Shell。
- `skill` 工具只调用 `SkillRegistry` 中注册过的 skill，且当前 `execute_skill` 仍为模拟实现，未就绪前默认关闭。

---

## 9. 前端改造

### 9.1 类型扩展

- `WorkflowNodeType` 增加 `agent`、`approval`。
- `ElementKind` 增加 `agent`、`approval`。
- `WorkflowExecution.status` 增加 `waiting`。
- `NodeExecutionResult.status` 增加 `waiting`。
- `EngineFields` 增加 5.1 中的 Agent / 审批字段。

### 9.2 BPMN 映射

- `nodeKinds.ts` 增加 AI Agent 任务 preset。
- `classifyBpmnElement`：`implementation === "ai-agent-task"` 或 `engineKind === "agent"` 返回 `agent`。
- `EXECUTABLE_BPMN_TYPES` 增加 `bpmn:UserTask`；当前缺少该项，审批节点会在 IR 转换时被丢弃。
- `UserTask` 映射为 `approval`。
- 删除 `skill` / `workflow` 的占位警告。
- `definitionToBpmn` / `bpmnToDefinition` 保留 `agent`、`approval`。

### 9.3 节点属性面板

Agent 分支：

- Agent ID
- 模型、温度、max tokens
- Prompt
- 工具列表
- 知识库/知识树节点来源
- 最大迭代次数
- 超时秒数
- 中断策略
- 审批人
- 重试次数

UserTask 分支：

- Assignee
- 审批表单字段（MVP 可先用 JSON 变量）

ScriptTask 明确标注 bpmninja 只支持 Rhai，避免继续用 Python/JavaScript 占位。

### 9.4 workflowService

- 移除“任何错误都回退 mock”的 `catch`。
- 改为 `NEXT_PUBLIC_WORKFLOW_MOCK=true` 才启用 mock。
- 增加 `listPendingWorkflowTasks`、`completeWorkflowUserTask`。
- `getExecution` / `listExecutions` 支持 `waiting`。
- Tauri 参数统一 camelCase。

### 9.5 执行时间线与审批

- `running` / `waiting` 时每 1.5 秒轮询 `get_execution`。
- 时间线显示 `waiting` 节点与待审批按钮。
- 审批弹窗调用 `complete_workflow_user_task`，然后刷新 execution。
- `applyExecutionState` 增加 `waiting` 的 CSS 状态。
- 取消按钮调用 `cancel_execution`，确认后停止轮询。

### 9.6 Agent 输出展示

- `node_results` 展示 Agent 的 `summary`、`tool_calls`、`tokens`。
- `interrupt_payload` 展示待确认的工具、参数、原因。
- 不在前端执行任何工具。

---

## 10. HTTP 模式

桌面端是主路径。若需要 Web 模式，`api` crate 增加：

| 方法 | 路径 |
|---|---|
| GET | `/api/workflows` |
| GET | `/api/workflows/{id}` |
| POST | `/api/workflows` |
| PUT | `/api/workflows/{id}` |
| DELETE | `/api/workflows/{id}` |
| POST | `/api/workflows/{id}/execute` |
| GET | `/api/workflow-executions` |
| GET | `/api/workflow-executions/{id}` |
| POST | `/api/workflow-executions/{id}/cancel` |
| GET | `/api/workflow-tasks/pending` |
| POST | `/api/workflow-tasks/{taskId}/complete` |

`httpAdapter` 增加命令到路由的映射；如果本期只做桌面端，则把 workflow 命令加入 `TAURI_ONLY_COMMANDS`，并明确报错，不静默 mock。

---

## 11. 分阶段实施计划

### Phase 0：工具链与依赖

- 升级 Rust 1.99.1。
- bpmninja 固定 commit，或 vendor。
- `workflow` 增加 `engine-core`、`bpmn-parser`、`adk-graph`、`uuid`、`futures`、`tokio-util`、`sqlx json`。
- 删除 `wfe-core`、`wfe-yaml`。
- 验收：`cargo check -p workflow` 通过，`cargo tree` 没有 crates.io 同名 `engine-core`。

### Phase 1：迁移与持久化

- 新增 tenant workflow migration。
- 实现 `PostgresWorkflowPersistence` 全部方法。
- 实现 `PostgresCheckpointer`。
- 启动时清理过期 ServiceTask 锁。
- 验收：纯 Rust 单测覆盖 save/load/list/delete、history、completed instance。

### Phase 2：BPMN 编译与引擎运行时

- `BpmnCompiler`。
- `TenantWorkflowRuntime`、`WorkflowService`。
- 定义/执行 CRUD 命令。
- `WorkflowProjector`。
- 验收：无 Agent 的 Start → ServiceTask → End 流程可以执行并完成。

### Phase 3：Worker 与审批

- `WorkflowWorker` 拉取、锁、心跳、完成/失败。
- UserTask 查询与 `complete_user_task`。
- 取消执行。
- 验收：人工审批链可以暂停、重启、继续。

### Phase 4：AgentGraphRunner

- StateGraph、LLM 协议、RAG、工具。
- interrupt / resume。
- Agent run 幂等。
- 验收：`before_final` 中断后审批恢复，输出一致。

### Phase 5：前端

- 类型、preset、属性面板。
- 执行轮询、时间线、审批弹窗。
- 移除默认 mock。
- 验收：设计 → 保存 → 执行 → 审批 → 完成全链路可用。

### Phase 6：HTTP 与回归

- HTTP 路由（如需要）。
- 多租户、重启恢复、取消、失败重试测试。

---

## 12. 测试与验收

### 12.1 后端单元测试

| 范围 | 用例 |
|---|---|
| `bpmn.rs` | topic/assignee 重写、definition hash、网关分支 |
| `persistence` | save/load/list/delete、history、completed instance、过期锁清理 |
| `checkpoint` | save/load/load_by_id/list/delete、JSON 往返 |
| `agent` | 纯文本输出、工具调用、无效 JSON、超迭代、超时 |
| `agent` | 中断保存、审批恢复、拒绝恢复 |
| `projection` | InstanceState 到前端状态映射 |

### 12.2 集成测试

- 无 Agent：Start → ServiceTask → End。
- Agent 成功：Start → ai-agent-task → End。
- Agent 中断：ai-agent-task → Gateway → UserTask → ai-agent-resume → End。
- Agent 拒绝：审批拒绝后走失败/结束分支。
- 重启恢复：UserTask 等待期间重启进程，审批仍可继续。
- 取消：运行中 cancel，worker 停止，状态为 cancelled。
- 多租户：两个 schema 各自执行、互不可见。

### 12.3 前端测试

- `bpmnMapping.test.ts` 覆盖 agent、approval。
- `workflowService` 不再默认 mock。
- 审批弹窗、等待状态、轮询清理。

### 12.4 命令

```bash
cd apps/backend/src-tauri
cargo check -p workflow
cargo check --workspace
cargo test -p workflow
```

```bash
cd apps/web
pnpm test -- bpmnMapping
pnpm build
```

### 12.5 手工 E2E

1. 画 Start → AI Agent ServiceTask → End，保存。
2. 执行，确认 worker 拉到 `ai-agent-task`。
3. 配置 LLM provider，确认 Agent 循环、RAG 上下文、最终输出。
4. 配置 `before_final`，确认执行变 `waiting`，审批页出现 UserTask。
5. 同意后确认进入 `ai-agent-resume` 并完成。
6. 关闭并重启应用，确认未完成 UserTask 和 Agent checkpoint 仍在。
7. 取消执行，确认 worker 停止、状态为 cancelled。

---

## 13. 风险与取舍

| 风险 | 影响 | 处理 |
|---|---|---|
| bpmninja MSRV 1.99 | 当前工具链编译失败 | 升级 1.99.1；或 vendor 后降 MSRV |
| 不是 crates.io 包 | 误加同名 crate | 固定 git commit 或 vendor，检查 `cargo tree` |
| NATS 依赖 | 桌面端额外服务 | 不用 engine-server，嵌入 engine-core + 自研 persistence |
| `implementation` 不被识别 | AI 节点不执行 | 部署前按 IR 重写 topic/assignee |
| UserTask 不在 IR 可执行集合 | 审批节点丢失 | 前端扩展 approval；后端以 BPMN 为准 |
| 重启丢 Agent checkpoint | 无法恢复审批 | 自研 PostgresCheckpointer |
| LLM 无 function calling | 工具协议不稳定 | JSON 协议 + 失败降级为文本 |
| Skill/Python 运行时占位 | Agent 假装成功 | 默认关闭 skill 工具，明确报错 |
| ScriptTask 语言不匹配 | 脚本执行失败 | bpmninja 只支持 Rhai，UI 标注并校验 |
| 多租户隔离 | 数据串租户 | 每 schema 独立 runtime/persistence/worker |
| 前端 mock 掩盖错误 | E2E 假成功 | 移除默认 mock，显式开关 |

---

## 14. 待确认决策

### 14.1 bpmninja 依赖交付

- A. 固定 git commit + 升级 Rust 1.99.1 + 自研 Postgres persistence（推荐）
- B. 把 `engine-core` / `bpmn-parser` vendor 到仓库，离线构建并降低 MSRV
- C. 使用 `engine-server` + NATS 边车（不推荐桌面端）

### 14.2 人工审批方式

- A. 显式 BPMN `UserTask` 审批链（推荐，不改 bpmninja）
- B. fork `engine-core`，Worker 动态创建 UserTask（维护成本高）

### 14.3 本期范围

- A. 只做 Tauri 桌面端，HTTP 模式后续补（推荐）
- B. 同时补齐 HTTP 路由与 `httpAdapter`

### 14.4 默认建议

**A + A + A**：固定 commit、显式 UserTask、先桌面端。

---

## 15. 实施前置检查清单

- [ ] Rust 1.99.1 可用
- [ ] bpmninja commit 已固定，`cargo tree` 无 crates.io 同名 `engine-core`
- [ ] PostgreSQL 租户 schema 可执行迁移
- [ ] LLM provider 已配置，能完成一次普通 chat
- [ ] RAGRetriever 可用（至少一个知识树节点）
- [ ] 前端 bpmn-js 能保存 UserTask 节点
- [ ] `NEXT_PUBLIC_WORKFLOW_MOCK` 默认关闭

---

## 16. 变更记录

| 日期 | 内容 |
|---|---|
| 2026-10-26 | 初稿：bpmninja + adk-graph 可行性、架构、数据模型、实施阶段 |

> 说明：本文档是设计稿，未包含代码改动。实施前请先确认第 14 节的决策。