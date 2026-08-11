# Zeph 可改造点详细分析报告

> 本文档基于 `docs/Zeph+AgentGateway智能体架构改造方案.md` 四阶段路线图，结合当前代码库（`apps/backend/src-tauri/src/`）的**实际状态**，逐一列出每个模块的现有状态、Zeph 改造目标、改动量和优先级。

---

## 一、改造全景一览

```
┌─────────────────────────────────────────────────────────────┐
│  三层改造                                              ▲    │
│                                                         │    │
│  第一层：现有桩模块直接填充 ── P0（立即执行）              │    │
│    1. python_runtime.rs → 子进程 JSON 协议                 │    │
│    2. skill_sandbox.rs   → 进程级资源限制                  │    │
│    3. skill_commands.rs  → 调用真实运行时                 │    │
│                                                         │    │
│  第二层：新建 Zeph 模块 ── P0/P1                          │    │
│    4. zeph/mcp/server.rs → MCP stdio/SSE 服务            │    │
│    5. lib.rs            → --mcp-mode 启动参数              │    │
│    6. Cargo.toml        → lancedb 等依赖                  │    │
│                                                         │    │
│  第三层：增强已有模块 ── P1/P2                            │    │
│    7. workflow/        → 参考实现 A2A Task                │    │
│    8. rag_service.rs   → LanceDB 本地向量适配器            │    │
│    9. llm_gateway_service.rs → 直接复用（零改动）          │    │
│                                                         │    │
│  ┌─────────────────── 已就绪的 WorkflowEditor 前端 ─────┐ │
│  │  Phase 2 编排器前端已完成，后端就绪后即可对接         │ │
│  └────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

---

## 二、第一层：现有桩模块直接填充（P0）

### 1. engine/python_runtime.rs — Python 子进程运行时

| 维度 | 说明 |
|------|------|
| **当前状态** | 纯桩模块，`init()` 第 59-68 行为空函数 |
| **现有代码** | `PythonRuntime` 结构体 + `OnceLock` 全局单例 + 配置项（venv_path/python_home/site_packages_path） |
| **Zeph 改造目标** | 实现子进程 stdin/stdout JSON 协议执行 Python Skill |
| **技术方案** | `tokio::process::Command` 启动 Python 进程 → stdin 写入 JSON 请求 → stdout 读取 JSON 响应 → 30s 超时 → 超时 kill |
| **复用资产** | 现有的 `PythonRuntimeConfig`（venv 路径等）可直接用 |
| **工作量** | **2-3 周** |
| **优先级** | 🥇 P0 |
| **验证方式** | `cargo run` 执行 skill 返回真实 Python 执行结果而非 mock |

**关键决策**：子进程方案（非 PyO3），原因：
- 不需要解决 C Python API 兼容性问题
- 进程崩溃不影响 Rust 主进程
- 可对每个进程做 CPU/内存限制

### 2. engine/skill_sandbox.rs — 进程级资源限制

| 维度 | 说明 |
|------|------|
| **当前状态** | 只做了 Python import 黑白名单检查（`allowed_imports` / `blocked_imports`） |
| **现有代码** | `SkillSandbox` 结构体含 `max_execution_time`/`max_output_size`/黑白名单 |
| **Zeph 改造目标** | 增加操作系统级别的资源限制（CPU/内存/网络） |
| **技术方案** | Windows: `Job Object` API 限制子进程 CPU/内存；Linux: `setrlimit` + cgroup |
| **改动量** | **1 周**（框架已建，加限制逻辑） |
| **优先级** | 🥇 P0 |
| **验证方式** | 执行死循环 Skill 被 30s 超时终止；内存泄露 Skill 被限制 |

### 3. commands/skill_commands.rs — 执行真实运行时

| 维度 | 说明 |
|------|------|
| **当前状态** | `execute_skill` 命令没调用真实运行时，返回 mock 结果 |
| **Zeph 改造目标** | 连接到 `python_runtime` 执行真实 Skill |
| **改动量** | **小改**（去掉 TODO 注释，调用运行时） |
| **优先级** | 🥇 P0 |
| **验证方式** | Tauri IPC `execute_skill` 返回真实结果 |

---

## 三、第二层：新建 Zeph 模块（P0/P1）

### 4. zeph/mcp/server.rs — MCP 协议服务器

| 维度 | 说明 |
|------|------|
| **当前状态** | 不存在，需要新建 `apps/backend/src-tauri/src/zeph/` 目录 |
| **Zeph 改造目标** | 实现 MCP JSON-RPC stdio/SSE 协议服务 |
| **需要实现的方法** | `initialize` / `tools/list` / `tools/call` / `notifications/` |
| **复用资产** | `skill_registry.rs` 的 15 个 Skill 直接映射为 `tools/list` 返回的工具 |
| **技术方案** | 自实现 JSON-RPC（不依赖 mcp-sdk 第三方库），stdin/stdout 行协议 |
| **工作量** | **2-3 周** |
| **优先级** | 🥇 P0 |
| **验证方式** | Claude Desktop 通过 MCP stdio 连接到本应用，调用 RAG 问答等 Skill |

**目录结构**：
```
apps/backend/src-tauri/src/
├── zeph/                    ← 新增
│   ├── mod.rs
│   └── mcp/
│       ├── mod.rs
│       └── server.rs
```

### 5. lib.rs — `--mcp-mode` 启动参数

| 维度 | 说明 |
|------|------|
| **当前状态** | Tauri 标准启动，无 MCP 模式 |
| **Zeph 改造目标** | 新增 CLI 参数 `--mcp-mode stdio|sse`，控制是否启动 MCP 服务 |
| **技术方案** | 使用 `clap` 或 Tauri 自带的 CLI 解析 |
| **改动量** | **半周** |
| **优先级** | 🥇 P0 |

### 6. Cargo.toml — 新增依赖

| 依赖 | 用途 | Phase |
|------|------|-------|
| `lancedb = "0.16"` | 本地向量库，替代 SurrealDB 向量检索 | Phase 1 |
| `rusqlite = { version = "0.32", features = ["bundled"] }` | 会话/任务持久化 | Phase 2 |
| `tokio-tungstenite = "0.24"` | SSE/WebSocket 支持 | Phase 3 |
| `wasmtime = "28.0"` | WASM 沙箱 | Phase 4 |

**注意**：所有依赖通过 Cargo features 条件编译，默认不开启 Zeph 功能。

---

## 四、第三层：增强已有模块（P1/P2）

### 7. workflow/ → 参考实现 A2A Task 状态机

| 维度 | 说明 |
|------|------|
| **当前状态** | 已集成 wfe-core 审批流引擎 |
| **Zeph 改造目标** | 不修改 workflow 模块，而是参考其 DAG 调度 + 状态机模式实现 A2A Task |
| **现有可参考的代码** | `workflow/executor.rs` 流程执行器、`workflow/definitions.rs` 流程定义 |
| **工作量** | Phase 2，4-6 周 |
| **优先级** | 🥈 P1 |

### 8. rag_service.rs → LanceDB 本地向量适配器

| 维度 | 说明 |
|------|------|
| **当前状态** | 使用 SurrealDB 做向量检索，依赖后端 HTTP 服务 |
| **现有代码** | `TextChunker` 文本切片、向量化调用 LLM 接口、语义检索 |
| **Zeph 改造目标** | 新增 LanceDB 适配器（保留现有 SurrealDB 路径），实现完全本地化 |
| **技术方案** | 在 `rag_service.rs` 中新增 `LanceDBAdapter` 结构体，通过 Trait 抽象切换 |
| **工作量** | **2 周** |
| **优先级** | 🥈 P1 |
| **验证方式** | 无网络环境下 RAG 问答正常运行 |

### 9. llm_gateway_service.rs — 直接复用（零改动）

| 维度 | 说明 |
|------|------|
| **当前状态** | 已支持 7+ 厂商、负载均衡、熔断、重试 |
| **Zeph 改造目标** | 无改动，Skill 引擎和 MCP 服务直接调用 |
| **角色** | **Zeph 最大的"免费"资产** — 零投入即可获得多厂商 LLM 能力 |

---

## 五、优先级排序与交付路线

### P0：3 周闭环（单机 MCP 可用）

```
Week 1-2:  python_runtime.rs 子进程方案 + skill_sandbox.rs 资源限制
           + commands/skill_commands.rs 接入真实运行时
             → 验证：Tauri IPC execute_skill 返回真实结果

Week 2-3:  zeph/mcp/server.rs MCP stdio 实现
           + lib.rs --mcp-mode 启动参数
             → 验证：Claude Desktop 连接成功，调用 RAG Skill
```

### P1：再 2 周（本地 RAG 全面就绪）

```
Week 4-5:  rag_service.rs LanceDB 适配器
             → 验证：无网络 RAG 问答 + 本地向量检索
```

### P2：再 4-6 周（Agent 编排）

```
Week 6-11: A2A 子 Agent + 编排引擎 + SQLite 持久化
             → 验证：多 Agent 协同完成"分析→摘要→翻译"
```

### P3：再 6-8 周（分布式集群）

```
Week 12-19: AgentGateway 独立网关 + 注册发现 + 协议转换 + 鉴权限流
              → 验证：多 Zeph 节点注册，OpenAPI 调用远端工具
```

### P4：持续（安全沙箱）

```
持续: WASM 沙箱 + OTel 补全 + Prometheus 指标
```

---

## 六、与 WorkflowEditor 前端的对接

当前已完成的 WorkflowEditor 前端（`/knowledge/workflow/`）是 Phase 2 编排器的 UI 层：

```
前端 (已就绪)             后端 (待实现)               执行层
─────────────────────────────────────────────────────────
WorkflowEditor              workflow_service.rs      Python Sidecar
  ↓ 拖拽编排                   ↓ CRUD                 ↓ langgraph
  ↓ 导出 JSON                 ↓ 触发执行               ↓ StateGraph
  ↓ 查看执行历史              ↓ 记录结果               ↓ 节点依次执行
```

| 前端 | 后端状态 | 说明 |
|------|---------|------|
| `WorkflowEditor` DAG 画布 | ✅ 已实现 | 拖拽/连线/配置/导入导出 |
| `workflowService.ts` 8 个 API | ⚠️ mock 数据 | 需要实现后端 Rust CRUD |
| `ExecutionTimeline` 执行历史 | ⚠️ mock 数据 | 需要实现后端执行引擎 |

WorkflowEditor 和 Zeph Phase 1 是并行关系，互不阻塞：
- Phase 1 专注 **单 Skill 执行**（MCP tools/call）
- WorkflowEditor + Phase 2 专注 **多 Skill 编排**（DAG 管线）

---

## 七、架构决策：不需要 LangChain

### 7.1 LangChain 解决了什么

LangChain 本质上是一个"胶水代码框架"，它帮你处理 LLM 应用中的标准化需求，但本项目已有 Rust 实现替代：

| LangChain 解决的问题 | LangChain 的做法 | 本项目已有的实现 |
|---------------------|-----------------|----------------|
| **多厂商 API 统一调用** | `ChatOpenAI` / `ChatClaude` 统一接口 | ✅ `llm_gateway_service.rs`（842 行），7+ 厂商适配 + 负载均衡 + 熔断 |
| **提示词模板** | `PromptTemplate` 变量替换 | ❌ 提示词在客户端（前端）拼好传过来，不需要服务端模板 |
| **输出解析** | `StructuredOutputParser` JSON 校验 | ❌ 需求简单，`serde_json` 直接解析即可 |
| **Chain（顺序链条）** | A → B → C 顺序调用 | ❌ 这正是 Zeph 要解决的复杂编排场景 |
| **Agent（工具调用）** | LLM 自主决定调哪个函数 | ❌ 暂时不需要，人工编排 DAG 更可控 |
| **Memory（对话历史）** | 自动管理上下文窗口 | ✅ 已有 `conversation_service` + PostgreSQL 持久化 |
| **RAG（检索增强）** | 文档加载→切片→向量化→检索 | ✅ 已有 `rag_service.rs`（202 行），更轻量 |

### 7.2 关键差异

```
LangChain 的方案：          本项目已经做的方式：

对话管理:                    conversation_service + PostgreSQL
  ConversationBufferMemory     直接查数据库，更可靠

LLM 调用:                    llm_gateway_service
  ChatOpenAI / ChatClaude       842 行 Rust，7+ 厂商，有负载均衡 + 熔断

RAG:                         rag_service + SurrealDB
  Chroma + ParentDocumentRetriever  202 行 Rust，更轻量，无额外依赖

工具调用:                    Zeph MCP 协议 + 编排引擎（Phase 1-2）
  AgentExecutor + Tool            不依赖 LangChain，自实现 JSON-RPC
```

### 7.3 结论

```
当前 LLM 架构（Rust 直接调 OpenAI 兼容 API）
   ↓
已经解决了"简单问答"场景
   ↓
不需要 LangChain
   ↓
用户点：简单→直接调 LLM（已有），复杂→Zeph 编排（待实现）
   ↓
LangChain 唯一有价值的场景（Agent 自主决策调 tool）
   当前没有这个需求，未来有需要也可在 Zeph 上层对接
```

### 7.4 与 Zeph 的对接

Zeph 不替换 `llm_gateway_service`，而是将其包装为 MCP tool：

```
现有模块                              Zeph MCP Tool
────────────────────────────────────────────────────
llm_gateway_service.chat()       →  llm-chat (调用任意厂商)
llm_gateway_service.embedding()   →  llm-embed (向量化)
rag_service.retrieve()            →  rag-qa (知识库检索)
skill_registry 的 15 个 Skill     →  tools/list 直接返回
workflow/ 引擎                     →  参考 A2A Task 状态机
```

---

> **文档版本**：v1.0
> **创建日期**：2026-07-23
> **参考文档**：`docs/Zeph+AgentGateway智能体架构改造方案.md`、`docs/Zeph Agent Runtime 可行性分析报告.md`、`docs/知识库模块/AI工作流编排器设计方案.md`
