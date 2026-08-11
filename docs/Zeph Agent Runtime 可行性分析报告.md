# Zeph Agent Runtime 可行性分析报告

## 一、背景

本文档基于 `docs/Zeph+AgentGateway智能体架构改造方案.md` 四阶段路线图，结合项目现有代码库的实际状态，对 Zeph Agent Runtime 在本项目中的落地可行性进行系统评估。

## 二、项目现有基础评估

### 2.1 架构就绪度（★★★★★）

| 层面 | 现状 | 支撑 Zeph 的理由 |
|------|------|------------------|
| **架构文档** | `docs/Zeph+AgentGateway智能体架构改造方案.md` 已存在 | 四阶段路线图 (Phase 1-4) 已经定义清晰，可直接执行 |
| **Skill 引擎** | `engine/skill_registry.rs` 已注册 15 个内置 Skill | MCP `tools/list` 可直接映射为这些 Skill |
| **LLM 网关** | `llm_gateway_service.rs` 已实现在线/本地多厂商适配 | Skill 执行可直接复用 LLM 调用能力，无需重新实现 |
| **RAG 引擎** | `rag_service.rs` 已实现文档切片+向量检索 | 对应 Phase 1 MCP tool: `rag-qa` 立即可用 |
| **Web/桌面双模** | `lib.rs` 同时启动 Tauri IPC + Axum HTTP | MCP SSE 可复用现有 Axum 端口，无需额外 HTTP 框架 |
| **工作流引擎** | `wfe-core` 已集成 | A2A Task 状态机可参考其 DAG 调度逻辑 |
| **可观测性** | OpenTelemetry + tracing 已完整集成 | MCP 链路追踪零额外投入 |
| **Cargo 依赖** | serde/serde_json/reqwest/tokio/axum 均已存在 | MCP JSON-RPC 协议无需新增基础设施依赖 |

### 2.2 依赖就绪度（Cargo.toml 分析）

直接可用的关键依赖（无需新增）：

| 依赖 | 用于 Zeph 的用途 |
|------|-----------------|
| `serde` + `serde_json` | MCP JSON-RPC 消息序列化/反序列化 |
| `tokio` + `axum` | MCP SSE 端点 HTTP 服务 |
| `reqwest` | AgentGateway 节点注册/心跳上报 |
| `futures` + `async-trait` | Agent 编排异步任务管理 |
| `tracing` + `opentelemetry` | MCP 调用全链路追踪 |

### 2.3 现有代码复用表

| 现有模块 | 复用方式 | 是否需要重写 |
|---------|---------|------------|
| `engine/skill_registry.rs` | 直接复用，扩展编排字段 | 小改动 |
| `service/llm_gateway_service.rs` | 直接复用，Gateway 可直接引用 | 无需 |
| `service/rag_service.rs` | 改 LanceDB 适配器 | 中等改造 |
| `workflow/` 模块 | 参考其调度逻辑实现 A2A Task | 参考 |
| `api/` 路由 | Gateway 中复用于 OpenAPI 出口 | 复用 |
| `commands/` (IPC) | 包装为 MCP tools/list 返回的工具 | 包装 |
| `database/` | 本地模式保留，集群模式可选 | 无需 |

## 三、四阶段工作量与风险评估

### 3.1 Phase 1：单机本地闭环（8-10周）

| 模块 | 当前状态 | 需要投入 | 工作量评估 |
|------|---------|---------|-----------|
| `python_runtime.rs` | 桩模块 (TODO) | 改为子进程 stdin/stdout JSON 协议 | **2-3周** |
| `skill_sandbox.rs` | 框架已搭建，功能未实现 | 进程级资源限制（CPU/内存） | **1周** |
| `rag_service.rs` | 使用 SurrealDB 向量库 | 新增 LanceDB 适配器 | **2周** |
| MCP stdio server | 不存在 | 实现 JSON-RPC: initialize/tools/list/tools/call | **2-3周** |
| CLI 参数 `--mcp-mode` | 不存在 | `lib.rs` 新增启动参数逻辑 | **1周** |

**Phase 1 关键风险：Python 运行时**

- **风险**：PyO3 集成复杂，Python 版本兼容性难以保证，Cargo 编译频繁失败
- **缓解方案**：采用 **子进程 stdin/stdout JSON 协议**（非 PyO3），隔离性好，进程崩溃不影响 Rust 主进程，30s 超时可防无限阻塞
- **备选方案**：若子进程方案延迟不可接受，可在需求明确后切回 PyO3

### 3.2 Phase 2：Agent 通信与编排（4-6周）

| 模块 | 当前状态 | 需要投入 |
|------|---------|---------|
| `zeph/a2a/agent_card.rs` | 不存在 | 新建 |
| `zeph/a2a/task.rs` | 不存在 | 新建（参考 wfe-core 状态机） |
| `zeph/a2a/protocol.rs` | 不存在 | 新建 |
| `engine/orchestrator.rs` | 不存在 | 新建（DAG 编排引擎） |
| SQLite 持久化 | 不存在 | 新建 |

### 3.3 Phase 3：分布式网关 AgentGateway（6-8周）

| 模块 | 需要投入 |
|------|---------|
| `apps/agent-gateway/` 独立 Rust 项目 | 新建 |
| 注册发现 + 心跳检测 | 新建 |
| MCP ↔ A2A ↔ OpenAPI 协议转换 | 新建 |
| 工具联邦（聚合所有节点工具） | 新建 |
| 鉴权限流（API Key + 按工具限流） | 新建 |

### 3.4 Phase 4：安全沙箱与运维增强（持续）

| 模块 | 需要投入 |
|------|---------|
| `engine/sandbox/wasm_runtime.rs` (wasmtime) | 新建 |
| OpenTelemetry 全链路追踪补全 | 补全 |
| Prometheus 指标暴露 | 新建 |
| 管理后台 Web UI | 新建 |

## 四、关键风险与缓解措施

### 风险 1：Python 运行时集成
- **影响**：Phase 1 核心依赖，15 个内置 Skill 需要 Python 执行环境
- **概率**：高
- **缓解**：子进程方案（非 PyO3），解耦 Python 版本兼容问题

### 风险 2：破坏现有业务功能
- **影响**：改造可能导致现有资产/审批/知识库功能异常
- **概率**：低
- **缓解**：
  - Zeph 代码全部放在 `zeph/` 子目录下，不侵入现有业务模块
  - 通过 `--mcp-mode` CLI 参数开关，默认为 `off`
  - 所有 Skill 执行走独立上下文，不影响数据库连接池

### 风险 3：编译体积与时间膨胀
- **影响**：新增依赖（wasmtime / LanceDB）可能导致编译时间大幅增加
- **概率**：中
- **缓解**：
  - 通过 Cargo features 条件编译，默认不包含 Zeph 依赖
  - wasmtime / LanceDB 放在 Phase 1/Phase 4 按需引入
  - 设置 `[profile.dev.package."*"] opt-level = 2` 已优化第三方库编译

### 风险 4：MCP 协议规范演进
- **影响**：MCP 协议仍在快速迭代（截至 2026年）
- **概率**：中
- **缓解**：
  - 自实现 JSON-RPC，不依赖 mcp-sdk 第三方库
  - 核心逻辑通过 Trait 抽象，协议变更仅影响 adapter 层

## 五、不可行替代方案的排除

| 方案 | 排除理由 |
|------|---------|
| **完全从零写新 Rust 项目** | 现有 15 个 Skill / LLM 网关 / RAG 引擎 / 工作流引擎 全部浪费 |
| **用 Python 重写 Agent 层** | 引入第二种后端语言，运维复杂度剧增，且无法复用 Rust 基础设施（内存安全 / 高性能 / Tauri 集成） |
| **直接用 Claude Desktop 的 MCP 生态替代 Zeph** | Claude Desktop 无法管理多租户、无法编排子 Agent、无法分布式部署，属于能力降级 |

## 六、总体评估结论

| 维度 | 评分 | 说明 |
|------|------|------|
| **可行性指数** | ★★★★☆ (4/5) | 技术基础扎实，无明显不可逾越的技术障碍 |
| **业务接入成本** | 低 | 0 行业务代码变更，仅需 `--mcp-mode` 启动参数 |
| **ROI** | 高 | 打通 MCP 协议 → Claude Desktop / Cursor / Windsurf 可调用现有全部 15 个 Skill |
| **交付风险** | 低 | 4 个独立 Phase，每个可独立发布价值，依赖关系弱 |
| **长期扩展性** | 高 | Phase 3 集群模式可支持企业级分布式部署，Phase 4 WASM 沙箱可安全运行第三方工具 |

### 推荐启动策略

**立即启动 Phase 1（单机本地闭环）**，约 8-10 周交付：

```
Week 1-3:  MCP stdio server + Python 子进程运行时
Week 4-5:  LanceDB 集成 + RAG 管道适配
Week 6-7:  Skill 沙箱 + 资源限制
Week 8-10: 集成测试 + 文档 + Claude Desktop 联调
```

Phase 1 交付后可实现：
- Claude Desktop 通过 MCP stdio 调用本地 RAG 问答
- 文档解析（PDF/DOCX → Markdown）
- 代码审查 / 翻译 / 摘要等 15 个内置 Skill 全部可用
- 现有资产管理系统功能完全不受影响

后续 Phase 2（Agent 编排）、Phase 3（分布式 Gateway）、Phase 4（WASM 沙箱）按业务需求择机推进。

---

> **文档版本**：v1.0  
> **创建日期**：2026-07-22  
> **参考文档**：`docs/Zeph+AgentGateway智能体架构改造方案.md`、`docs/现有项目框架总结.md`