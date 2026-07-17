# Zeph + AgentGateway 智能体架构改造方案

## 一、改造目标

将现有 Tauri 桌面应用演进为 **Zeph Agent 节点运行时**，兼顾单机本地开发与分布式集群部署，实现：

1. **单机模式**：MCP stdio 对接 Claude Desktop + 本地 LanceDB RAG + Python Skill 引擎
2. **集群模式**：AgentGateway 统一管控多节点，MCP/OpenAPI/A2A 多协议接入
3. **业务代码一套复用**，仅通过启动配置切换模式

---

## 二、总体架构

```
┌─────────────────────────────────────────────┐
│   接入层                                      │
│   Claude Desktop / Ollama / 第三方业务       │
└──────────────────┬──────────────────────────┘
                   │
┌──────────────────▼──────────────────────────┐
│   【可选】AgentGateway 中心网关（集群启用）   │
│   协议转换：MCP↔A2A↔OpenAPI                  │
│   工具联邦 / 鉴权限流 / 可观测               │
└──────────────────┬──────────────────────────┘
                   │
┌──────────────────▼──────────────────────────┐
│   Zeph Agent 节点（在现有 backend 内实现）   │
│   ├─ MCP 模块：stdio / SSE 协议服务          │
│   ├─ A2A 模块：AgentCard + Task 状态机      │
│   ├─ Skill 引擎：Python 运行时 + 编排        │
│   ├─ WASM 沙箱：第三方工具安全隔离           │
│   └─ RAG 适配：LanceDB 本地向量检索          │
└──────────────────┬──────────────────────────┘
                   │
┌──────────────────▼──────────────────────────┐
│   底层资源                                    │
│   Ollama / PostgreSQL / LanceDB / SQLite     │
└─────────────────────────────────────────────┘
```

---

## 三、四阶段迭代路线

### Phase 1：单机本地闭环（4-6周）

**交付**：Python Skill 运行时 + MCP stdio + 真实 RAG 检索

| 模块 | 改动 |
|------|------|
| `engine/python_runtime.rs` | 实现 Python 子进程执行，stdin/stdout JSON 协议，30s 超时 |
| `engine/skill_sandbox.rs` | 进程级资源限制（CPU/内存），禁止网络访问 |
| `rag_service.rs` | 集成 LanceDB，向量化检索替代纯 SQL 查询 |
| **新增** `zeph/mcp/server.rs` | MCP JSON-RPC 实现：initialize/tools/list/tools/call |
| **新增** `zeph/mcp/mod.rs` | 模块组织 |
| `commands/skill_commands.rs` | 去掉 execute_skill 的 TODO，调用真实 Python 运行时 |
| `lib.rs` | 新增 CLI 参数 `--mcp-mode stdio`，启动 MCP 服务 |
| `Cargo.toml` | 新增依赖：`lancedb`, `mcp-sdk`(或自实现) |
| 配置 | 新增 `config.toml`，支持模式切换 |

**验证**：
- `cargo run -- --mcp-mode stdio` 启动后，Claude Desktop 通过 stdio 调用 Skill
- 执行"RAG 问答"Skill 返回真实知识库结果
- 对话和工具调用写入 SQLite 持久化

**目录结构变化**：
```
apps/backend/src-tauri/src/
├── zeph/                    ← 新增
│   ├── mod.rs
│   └── mcp/
│       ├── mod.rs
│       └── server.rs
├── engine/
│   ├── python_runtime.rs    ← 重写
│   └── skill_sandbox.rs     ← 新增资源限制
└── service/
    └── rag_service.rs       ← 改为 LanceDB 检索
```

---

### Phase 2：Agent 通信与编排（4-6周）

**交付**：A2A 子 Agent 通信 + Skill 编排引擎 + 对话持久化

| 模块 | 改动 |
|------|------|
| **新增** `zeph/a2a/agent_card.rs` | AgentCard 元数据自动生成 |
| **新增** `zeph/a2a/task.rs` | Task 状态机：提交→处理→需补充→完成/失败/取消 |
| **新增** `zeph/a2a/protocol.rs` | A2A JSON-RPC 协议实现 |
| `engine/skill_registry.rs` | SkillMeta 增加 `steps: Vec<SkillStep>` 字段 |
| **新增** `engine/orchestrator.rs` | DAG 编排引擎，支持串行/并行/条件分支 |
| **新增** 子 Agent 拆分 | 检索/摘要/翻译 各独立为 A2A Agent |
| **新增** SQLite 持久化 | 会话历史、任务状态的断点续跑 |

---

### Phase 3：分布式网关 AgentGateway（6-8周）

**交付**：独立网关二进制 + 多节点注册 + 协议转换

| 模块 | 说明 |
|------|------|
| **新增** `apps/agent-gateway/` | 独立 Rust 项目 |
| 注册发现 | Zeph 启动时 POST /register 上报 AgentCard + 工具列表 |
| 心跳检测 | 每 10s ping，3 次失败标记离线 |
| 协议转换 | MCP SSE ↔ A2A Task ↔ OpenAPI REST |
| 工具联邦 | 聚合所有节点工具，对外统一 /tools/list |
| 鉴权限流 | API Key + 按工具限流 + IP 白名单 |

**目录结构**：
```
apps/
├── backend/
│   └── src-tauri/     ← Zeph 节点（已有）
└── agent-gateway/     ← 新增
    ├── Cargo.toml
    └── src/
        ├── main.rs
        ├── registry/  ← 节点注册管理
        ├── protocol/  ← MCP / A2A / OpenAPI 转换
        └── admin/     ← 鉴权限流
```

---

### Phase 4：安全沙箱与运维增强（持续）

| 模块 | 说明 |
|------|------|
| `engine/sandbox/wasm_runtime.rs` | 集成 wasmtime，WASM 隔离第三方工具 |
| `utils/observability.rs` | OpenTelemetry 全链路追踪补全 |
| `api/metrics.rs` | Prometheus 指标暴露 |
| 管理后台 Web UI | 节点列表、工具调用统计、告警配置 |

---

## 四、核心数据流

### 单机模式：Claude → Zeph MCP → LanceDB

```
Claude Desktop
  │  stdio JSON-RPC
  ▼
Zeph MCP Server (stdio)
  │  tools/list → 返回所有 Skill
  │  tools/call {name:"rag-qa", args:{top_k:5}}
  ▼
Skill 引擎
  │  Python 子进程执行检索脚本
  ▼
LanceDB 向量检索 → 返回文档片段
  │
  ▼
Claude 获得上下文，生成回答
```

### 集群模式：业务系统 → Gateway → Zeph

```
业务系统
  │  OpenAPI REST POST /v1/tools/call
  ▼
AgentGateway
  │  鉴权→限流→协议转换为 A2A Task
  │  路由到对应 Zeph 节点
  ▼
Zeph 节点
  │  执行 MCP 工具/硬件采集
  │  结果逐层回传
  ▼
业务系统收到 JSON 响应
```

---

## 五、现有代码复用表

| 现有模块 | 复用方式 | 是否需要重写 |
|---------|---------|------------|
| `skill_registry.rs` | 直接复用，扩展编排字段 | 小改动 |
| `llm_gateway_service.rs` | 直接复用，Gateway 可直接引用 | 无需 |
| `rag_service.rs` | 改 LancdDB 适配器 | 中等改造 |
| `workflow/` 模块 | 参考其调度逻辑实现 A2A Task | 参考 |
| `api/` 路由 | Gateway 中复用于 OpenAPI 出口 | 复用 |
| `commands/` | 映射为 MCP tools/list 返回的工具 | 包装 |
| `database/` | 本地模式保留，集群模式可选 | 无需 |

---

## 六、新增 Cargo 依赖

```
# Phase 1
lancedb = "0.16"              # 本地向量库
mcp-sdk = { git = "..." }     # MCP 协议 或 自实现 JSON-RPC

# Phase 2
serde = "1.0"
serde_json = "1.0"
rusqlite = { version = "0.32", features = ["bundled"] }  # 任务持久化

# Phase 3
reqwest = { version = "0.12", features = ["json"] }
tokio-tungstenite = "0.24"    # SSE/WebSocket 支持

# Phase 4
wasmtime = "28.0"             # WASM 沙箱
opentelemetry = "0.32"
opentelemetry-otlp = "0.32"
prometheus = "0.13"
```

---

## 七、关键设计原则

1. **模式透明**：`ZephAgent` 结构体无感知本地/集群模式，由启动参数 `--mode local|cluster` 决定是否连接 Gateway
2. **协议中立**：核心逻辑不依赖 MCP/A2A/OpenAPI 任何协议，通过 Trait 抽象
3. **渐进增强**：每个 Phase 可独立交付使用，无需等全部完成
4. **向下兼容**：现有 Tauri CUD 业务功能（资产管理/审批等）不受影响

---

## 八、验证标准

| 阶段 | 验证方式 |
|------|---------|
| Phase 1 | Claude Desktop 通过 MCP 调用本地 RAG 问答 + SQL 查询 |
| Phase 2 | 多子 Agent 协同完成"文档分析→摘要→翻译"管线 |
| Phase 3 | 2 台以上 Zeph 节点注册到 Gateway，通过 OpenAPI 调用远端工具 |
| Phase 4 | WASM 工具执行被限制在沙箱内，不访问主机文件系统 |