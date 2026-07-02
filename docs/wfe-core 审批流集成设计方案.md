# wfe-core 审批流集成设计方案

> 使用 wfe-core 工作流引擎替换当前硬编码的审批流系统
> 版本：wfe-core = "1.10.0"

---

## 一、当前架构问题

### 1.1 现状

```
当前每个流程都有自己的表 + 硬编码状态机：

asset_receive    → status: 0=待审批 1=已同意 2=已驳回 3=已领用 4=已归还
asset_return     → status: 0=待确认 1=已完成
asset_transfer   → status: 0=待审批 1=已调拨 2=已驳回
asset_repair     → status: 0=待维修 1=维修中 2=已完成 3=无法维修
asset_scrap      → status: 0=待审批 1=已批准 2=已驳回 3=已报废
asset_purchase   → status: 0=待审批 1=采购中 2=已完成 3=已驳回

AssetApproval 表用于简单记录：{biz_type, biz_id, step, approver_id, approve_status}
```

### 1.2 问题

- ❌ **状态机硬编码** — 每个流程的审批流转逻辑写在 service 层，不可配置
- ❌ **难以支持复杂流程** — 会签、条件分支、超时、并行审批都不支持
- ❌ **审批人与流程深度耦合** — 无法动态指定审批链（比如按资产类别、金额路由）
- ❌ **无历史追踪** — AssetApproval 只记录最终结果，没有完整的执行轨迹
- ❌ **不可复用** — 每个流程的 CRUD 都要重复写相似的代码

---

## 二、wfe-core 设计思想

### 2.1 核心概念

```
┌──────────────────────────────────────────────┐
│               WorkflowDefinition               │
│  ┌────────┐   ┌────────┐   ┌────────┐         │
│  │ Step 0 │──▶│ Step 1 │──▶│ Step 2 │──▶END   │
│  │ 申请人  │   │ 上级审批│   │ 设备管理员│       │
│  └────────┘   └────────┘   └────────┘         │
│       │                                            │
│       └── 条件分支：金额 > 5000 → 加总经理审批      │
└──────────────────────────────────────────────┘
         │ 实例化
         ▼
┌──────────────────────────────────────────────┐
│              WorkflowInstance                  │
│  id: "wf-123", status: Runnable               │
│  execution_pointers: [                        │
│    {step_id: 0, status: Running},             │
│    {step_id: 1, status: Pending},             │
│  ]                                            │
│  data: { applicant: "张三", amount: 8000 }     │
└──────────────────────────────────────────────┘
```

### 2.2 wfe-core 核心 API

| 组件 | 作用 | 对应我们的实现 |
|------|------|--------------|
| `StepBody` trait | 每个审批步骤的逻辑 | 自定义审批步骤 |
| `WorkflowBuilder` | 定义流程步骤链 | 按流程类型构建 |
| `WorkflowExecutor` | 执行工作流实例 | 启动/推进审批 |
| `ExecutionResult` | 步骤执行结果控制 | next/persist/sleep |
| `PersistenceProvider` trait | 持久化工作流实例 | PostgreSQL 实现 |
| `QueueProvider` trait | 异步队列调度 | 直接同步执行（MVP） |
| `DistributedLockProvider` trait | 分布式锁 | 简化实现 |
| `StepRegistry` | 注册自定义步骤 | 启动时注册 |

---

## 三、数据模型设计

### 3.1 数据库新增表

```sql
-- tenant_tables.sql 追加

-- 工作流定义表（编译后的流程模板）
CREATE TABLE IF NOT EXISTS {schema}.wf_definition (
    id VARCHAR(100) PRIMARY KEY,        -- "asset_receive", "asset_repair" ...
    name VARCHAR(200),
    version INT NOT NULL DEFAULT 1,
    definition_json JSONB NOT NULL,      -- WorkflowDefinition 序列化
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- 工作流实例表（运行的审批流程）
CREATE TABLE IF NOT EXISTS {schema}.wf_instance (
    id VARCHAR(64) PRIMARY KEY,          -- 工作流实例ID
    wf_definition_id VARCHAR(100) NOT NULL,
    version INT NOT NULL DEFAULT 1,
    status VARCHAR(20) NOT NULL DEFAULT 'Runnable',
        -- Runnable / Complete / Terminated / Suspended
    data JSONB DEFAULT '{}',             -- 工作流数据（申请信息、审批意见等）
    biz_type VARCHAR(30) NOT NULL,       -- "receive" / "return" / "transfer" / ...
    biz_id BIGINT NOT NULL,              -- 关联业务表ID（asset_receive.id 等）
    next_execution BIGINT,               -- 下次执行时间戳
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- 执行指针表（当前流程执行到哪一步了）
CREATE TABLE IF NOT EXISTS {schema}.wf_execution_pointer (
    id VARCHAR(64) PRIMARY KEY,
    workflow_id VARCHAR(64) NOT NULL REFERENCES {schema}.wf_instance(id) ON DELETE CASCADE,
    step_id INT NOT NULL,
    step_name VARCHAR(200),
    status VARCHAR(20) NOT NULL DEFAULT 'Pending',
        -- Pending / Running / Complete / Failed / Sleeping / WaitingForEvent
    active BOOLEAN NOT NULL DEFAULT TRUE,
    start_time TIMESTAMP WITH TIME ZONE,
    end_time TIMESTAMP WITH TIME ZONE,
    persistence_data JSONB,              -- 步骤持久化数据
    event_name VARCHAR(100),             -- 等待的事件
    event_key VARCHAR(100),
    event_published BOOLEAN DEFAULT FALSE,
    event_data JSONB,
    predecessor_id VARCHAR(64),          -- 前驱指针ID
    scope VARCHAR(20) DEFAULT 'root'
);

CREATE INDEX idx_wf_pointer_workflow ON {schema}.wf_execution_pointer(workflow_id);
CREATE INDEX idx_wf_instance_biz ON {schema}.wf_instance(biz_type, biz_id);

-- 审批记录表（保留审批历史，可替代旧 AssetApproval 表）
CREATE TABLE IF NOT EXISTS {schema}.wf_approval_record (
    id BIGINT PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
    workflow_id VARCHAR(64) NOT NULL,
    step_id INT NOT NULL,
    step_name VARCHAR(200),
    approver_id BIGINT NOT NULL,          -- 审批人
    action VARCHAR(20) NOT NULL,          -- "approve" / "reject" / "transfer"
    comment TEXT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE INDEX idx_approval_workflow ON {schema}.wf_approval_record(workflow_id);
```

### 3.2 旧表改动（最小化）

```sql
-- 所有业务表（asset_receive 等）保留不动
-- 只需增加一个 workflow_id 字段关联 wf_instance
ALTER TABLE {schema}.asset_receive
ADD COLUMN IF NOT EXISTS workflow_id VARCHAR(64);

ALTER TABLE {schema}.asset_repair
ADD COLUMN IF NOT EXISTS workflow_id VARCHAR(64);

ALTER TABLE {schema}.asset_transfer
ADD COLUMN IF NOT EXISTS workflow_id VARCHAR(64);

ALTER TABLE {schema}.asset_scrap
ADD COLUMN IF NOT EXISTS workflow_id VARCHAR(64);

ALTER TABLE {schema}.asset_purchase
ADD COLUMN IF NOT EXISTS workflow_id VARCHAR(64);

-- 旧 AssetApproval 表暂不删除，逐步迁移到 wf_approval_record
-- 新审批统一写入 wf_approval_record
```

---

## 四、目录结构与工具类设计

### 4.1 整合目录结构

```
src-tauri/src/workflow/              # [新增] 工作流模块（整合 wfe-core / wfe-postgres / wfe-yaml）
├── mod.rs                           # 模块入口 + WfEngine 统一工具类
├── steps.rs                         # 自定义审批 StepBody 实现
├── definitions.rs                   # 流程定义：设备领用、维修、采购等
├── persistence.rs                   # PostgreSQL PersistenceProvider 实现
├── lock.rs                          # LockProvider 实现（本地锁）
├── queue.rs                         # QueueProvider 实现（同步简化版）
├── executor.rs                      # 封装 WorkflowExecutor 启动
├── commands.rs                      # Tauri Command（审批操作）
└── tests/
    ├── mod.rs                       # 测试模块入口
    ├── steps_test.rs                # 步骤单元测试
    ├── definitions_test.rs          # 流程定义测试（含 YAML 解析测试）
    ├── executor_test.rs             # 执行器测试
    ├── persistence_test.rs          # 持久化层测试（mock）
    └── integration_test.rs          # 集成测试
```

### 4.2 WfEngine 统一工具类

```rust
// workflow/mod.rs 核心设计

/// WfEngine — 工作流引擎工具类
///
/// 将 wfe-core / wfe-postgres / wfe-yaml 整合为一个统一的接口，
/// 供 service 层和 commands 层调用，无需关心底层实现细节。
///
/// # 功能
/// - 初始化和管理工作流引擎生命周期
/// - 提供统一的审批流程创建、执行、事件发布接口
/// - 封装 WorkflowBuilder 的流程定义管理
/// - 提供 YAML 定义加载能力（wfe-yaml）
pub struct WfEngine {
    executor: Arc<WorkflowExecutor>,
    registry: Arc<StepRegistry>,
    persistence: Arc<PostgresPersistenceProvider>,
}
```

### 4.3 整合要点

| 组件 | 来源 | 职责 | 关键 API |
|------|------|------|----------|
| `StepBody` / `StepRegistry` | `wfe-core` | 定义审批步骤行为 | `registry.register::<T>()` |
| `WorkflowBuilder` | `wfe-core` | 构建流程定义 | `WorkflowBuilder::new() ... build()` |
| `WorkflowExecutor` | `wfe-core` | 执行工作流实例 | `executor.execute()` |
| `PersistenceProvider` | **wfe-postgres** | PostgreSQL 持久化 | 已实现，直接复用 |
| `QueueProvider` | 自定义 | 同步队列（本地实现） | 简化版，不依赖外部队列 |
| `DistributedLockProvider` | 自定义 | 本地互斥锁 | 简化版，后续可替换 |
| YAML 定义加载 | **wfe-yaml** | 从 YAML 文件加载流程 | `yaml::from_reader()` 或 git 加载 |

---

## 五、审批步骤实现

### 5.1 自定义审批步骤

```rust
// workflow/steps.rs

use async_trait::async_trait;
use wfe_core::models::ExecutionResult;
use wfe_core::traits::step::{StepBody, StepExecutionContext};
use serde::{Serialize, Deserialize};

/// 审批步骤：需要指定审批人角色，等待审批事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalStepConfig {
    /// 审批角色标识
    pub role: String,
        // "applicant"        — 发起人
        // "applicant_superior" — 发起人上级
        // "dept_head"         — 部门负责人
        // "asset_manager"     — 设备管理员
        // "finance"           — 财务
        // "admin"             — 管理员
        // "system"            — 系统自动
    /// 审批人ID（可选，不指定则按角色动态查找）
    pub approver_id: Option<i64>,
    /// 审批标题
    pub title: String,
    /// 超时时间（秒），0 表示不超时
    pub timeout_seconds: u64,
}

/// 审批事件：用户在前端点击"通过"或"拒绝"
pub struct ApprovalEvent {
    pub workflow_id: String,
    pub step_id: usize,
    pub action: String,    // "approve" | "reject"
    pub comment: String,
    pub user_id: i64,
}

/// 通用审批步骤（等待人工审批事件）
#[derive(Default)]
pub struct ApprovalStep {
    /// 步骤配置（通过 step_config 传入）
    pub config: Option<ApprovalStepConfig>,
}

#[async_trait]
impl StepBody for ApprovalStep {
    async fn run(&mut self, ctx: &StepExecutionContext<'_>) -> wfe_core::Result<ExecutionResult> {
        let config: ApprovalStepConfig = ctx.step.step_config
            .as_ref()
            .and_then(|c| serde_json::from_value(c.clone()).ok())
            .unwrap_or(ApprovalStepConfig {
                role: "unknown".into(),
                approver_id: None,
                title: "审批".into(),
                timeout_seconds: 0,
            });

        // 检查是否已被审批事件唤醒
        if ctx.execution_pointer.event_published {
            // 事件已到达，记录审批结果
            let event_data = ctx.execution_pointer.event_data
                .as_ref()
                .ok_or_else(|| wfe_core::WfeError::Execution("缺少审批事件数据".into()))?;

            let action = event_data["action"].as_str().unwrap_or("reject");

            if action == "reject" {
                // 驳回：工作流结束
                return Err(wfe_core::WfeError::Execution("审批被驳回".into()));
            }

            // 通过：继续下一步
            return Ok(ExecutionResult::next());
        }

        // 未审批：等待审批事件
        let event_key = format!("{}-approval-{}", ctx.workflow.id, ctx.step.id);

        Ok(ExecutionResult::wait_for_event(
            "approval.event",
            &event_key,
            chrono::Utc::now(),
        ))
    }
}

/// 系统自动步骤（无需人工审批）
#[derive(Default)]
pub struct AutoStep;

#[async_trait]
impl StepBody for AutoStep {
    async fn run(&mut self, _ctx: &StepExecutionContext<'_>) -> wfe_core::Result<ExecutionResult> {
        // 自动执行的逻辑，比如发送通知、更新状态等
        Ok(ExecutionResult::next())
    }
}

/// 通知步骤（发送通知给指定人）
#[derive(Default)]
pub struct NotifyStep;

#[async_trait]
impl StepBody for NotifyStep {
    async fn run(&mut self, ctx: &StepExecutionContext<'_>) -> wfe_core::Result<ExecutionResult> {
        // 从 workflow.data 中解析通知信息
        let data = &ctx.workflow.data;
        let notify_user_id = data["notify_user_id"].as_i64().unwrap_or(0);

        // TODO: 调用通知服务（站内信/邮件/钉钉）
        tracing::info!(
            "发送通知: workflow={}, user_id={}",
            ctx.workflow.id, notify_user_id
        );

        Ok(ExecutionResult::next())
    }
}
```

### 5.2 流程定义

```rust
// workflow/definitions.rs

use wfe_core::builder::WorkflowBuilder;
use wfe_core::models::WorkflowDefinition;
use serde_json::json;

use super::steps::{ApprovalStep, ApprovalStepConfig, AutoStep, NotifyStep};

/// 工作流数据（存储在 workflow.data 中）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalWorkflowData {
    /// 业务类型
    pub biz_type: String,
    /// 业务ID
    pub biz_id: i64,
    /// 申请人ID
    pub applicant_id: i64,
    /// 申请人部门ID
    pub department_id: i64,
    /// 上级ID（审批链第一步）
    pub superior_id: i64,
    /// 设备管理员ID
    pub asset_manager_id: i64,
    /// 申请金额（条件分支用）
    pub amount: Option<f64>,
    /// 资产分类ID（动态路由用）
    pub category_id: Option<i64>,
}

/// ============ 设备领用流程 ============

pub fn asset_receive_workflow() -> WorkflowDefinition {
    let mut builder = WorkflowBuilder::<ApprovalWorkflowData>::new();

    builder
        .start_with::<ApprovalStep>()
            .name("申请人提交申请")
            .config(json!({
                "role": "applicant",
                "title": "设备领用申请",
                "timeout_seconds": 0
            }))
        .then::<ApprovalStep>()
            .name("上级审批")
            .config(json!({
                "role": "applicant_superior",
                "title": "设备领用 - 上级审批",
                "timeout_seconds": 604800  // 7天超时
            }))
        .then::<ApprovalStep>()
            .name("设备管理员审批")
            .config(json!({
                "role": "asset_manager",
                "title": "设备领用 - 设备管理员审批",
                "timeout_seconds": 604800
            }))
        .then::<NotifyStep>()
            .name("通知领用人")
            .config(json!({
                "notify_type": "站内信"
            }))
        .then::<AutoStep>()
            .name("更新资产状态")
        .end_workflow()
        .build("asset_receive", 1)
}

/// ============ 设备维修流程 ============

pub fn asset_repair_workflow() -> WorkflowDefinition {
    WorkflowBuilder::<ApprovalWorkflowData>::new()
        .start_with::<ApprovalStep>()
            .name("申请人提交维修申请")
            .config(json!({
                "role": "applicant",
                "title": "设备维修申请"
            }))
        .then::<ApprovalStep>()
            .name("部门负责人审批")
            .config(json!({
                "role": "dept_head",
                "title": "设备维修 - 部门审批",
                "timeout_seconds": 604800
            }))
        .then::<ApprovalStep>()
            .name("设备管理员确认")
            .config(json!({
                "role": "asset_manager",
                "title": "设备维修 - 管理员确认维修方案",
                "timeout_seconds": 604800
            }))
        .then::<NotifyStep>()
            .name("通知申请人")
        .end_workflow()
        .build("asset_repair", 1)
}

/// ============ 资产采购流程（带条件分支）============

pub fn asset_purchase_workflow() -> WorkflowDefinition {
    // 5000 元以下：简单审批
    // 5000 元以上：加总经理审批
    WorkflowBuilder::<ApprovalWorkflowData>::new()
        .start_with::<ApprovalStep>()
            .name("申请人提交采购申请")
            .config(json!({
                "role": "applicant",
                "title": "资产采购申请"
            }))
        .then::<ApprovalStep>()
            .name("部门负责人审批")
            .config(json!({
                "role": "dept_head",
                "title": "采购 - 部门审批",
                "timeout_seconds": 604800
            }))
        // TODO: 这里需要实现条件分支，wfe-core 支持 IfStep
        // 当前简化为两步审批，后续使用 then_if 进行金额判断
        .then::<ApprovalStep>()
            .name("财务审批")
            .config(json!({
                "role": "finance",
                "title": "采购 - 财务审批",
                "timeout_seconds": 604800
            }))
        .then::<NotifyStep>()
            .name("通知采购执行")
        .end_workflow()
        .build("asset_purchase", 1)
}
```

---

## 六、持久化实现

### 6.1 PersistenceProvider（PostgreSQL）

使用 `wfe-postgres` crate 提供的现成实现，避免重复造轮子。

```rust
// workflow/persistence.rs

use async_trait::async_trait;
use wfe_core::traits::{
    DistributedLockProvider, PersistenceProvider, QueueProvider,
};
use wfe_postgres::provider::PostgresPersistenceProvider;
use wfe_postgres::provider::PostgresPersistenceOptions;

use crate::database;

/// 创建 PostgreSQL 持久化提供者实例
///
/// 使用 wfe-postgres 提供的 PostgresPersistenceProvider，
/// 传入 database::schema_prefix() 实现多租户 schema 隔离。
pub fn create_persistence_provider() -> PostgresPersistenceProvider {
    let schema = database::schema_prefix()
        .trim_end_matches('.')
        .to_string();

    let options = PostgresPersistenceOptions {
        schema: if schema.is_empty() { "public".into() } else { schema },
        // 使用现有的 database::get_write_pool() 和 database::get_read_pool()
        write_pool: database::get_write_pool().expect("无法获取写连接池"),
        read_pool: database::get_read_pool().expect("无法获取读连接池"),
    };

    PostgresPersistenceProvider::new(options)
}
```

### 6.2 本地锁提供者

```rust
// workflow/lock.rs

use async_trait::async_trait;
use wfe_core::traits::DistributedLockProvider;
use std::collections::HashSet;
use std::sync::Mutex;

/// 简单的内存锁（单机足够，后续可替换为 Redis 锁）
pub struct LocalLockProvider {
    locked: Mutex<HashSet<String>>,
}

impl LocalLockProvider {
    pub fn new() -> Self {
        Self {
            locked: Mutex::new(HashSet::new()),
        }
    }
}

#[async_trait]
impl DistributedLockProvider for LocalLockProvider {
    async fn acquire_lock(&self, key: &str) -> wfe_core::Result<bool> {
        let mut locked = self.locked.lock().unwrap();
        if locked.contains(key) {
            Ok(false)
        } else {
            locked.insert(key.to_string());
            Ok(true)
        }
    }

    async fn release_lock(&self, key: &str) -> wfe_core::Result<()> {
        let mut locked = self.locked.lock().unwrap();
        locked.remove(key);
        Ok(())
    }
}
```

### 6.3 同步队列提供者

```rust
// workflow/queue.rs

use async_trait::async_trait;
use wfe_core::models::QueueType;
use wfe_core::traits::QueueProvider;

/// 同步 QueueProvider：直接在当前线程执行
/// MVP 不进行异步调度，WorkflowExecutor 会直接再次执行
pub struct SyncQueueProvider;

#[async_trait]
impl QueueProvider for SyncQueueProvider {
    async fn queue_work(
        &self,
        _workflow_id: &str,
        _queue_type: QueueType,
    ) -> wfe_core::Result<()> {
        // MVP：不进行异步调度，WorkflowExecutor 会直接再次执行
        Ok(())
    }

    async fn dequeue_work(
        &self,
        _queue_type: QueueType,
    ) -> wfe_core::Result<Option<(String, String)>> {
        Ok(None)
    }

    async fn dequeue_work_by_id(
        &self,
        _queue_type: QueueType,
        _workflow_id: &str,
    ) -> wfe_core::Result<Option<(String, String)>> {
        Ok(None)
    }
}
```

---

## 七、WfEngine 工具类与执行器

### 7.1 WfEngine 工具类（workflow/mod.rs）

```rust
// workflow/mod.rs

use std::sync::Arc;
use wfe_core::executor::step_registry::StepRegistry;
use wfe_core::executor::WorkflowExecutor;
use wfe_core::models::{ExecutionPointer, WorkflowInstance, WorkflowStatus};
use serde_json::json;

mod steps;
mod definitions;
mod persistence;
mod lock;
mod queue;
mod executor;
pub mod commands;

pub use steps::*;
pub use definitions::*;
pub use persistence::*;
pub use lock::*;
pub use queue::*;
pub use executor::*;

use crate::database;

/// WfEngine — 工作流引擎工具类
///
/// 整合 wfe-core / wfe-postgres / wfe-yaml 为统一接口。
/// 用法：
/// ```rust
/// let engine = WfEngine::new().await;
/// let wf_id = engine.create_workflow("asset_receive", data).await?;
/// engine.approve_step(&wf_id, 1, "approve", "同意", 1001).await?;
/// ```
pub struct WfEngine {
    executor: Arc<WorkflowExecutor>,
    registry: Arc<StepRegistry>,
}

impl WfEngine {
    /// 创建并初始化工作流引擎
    pub async fn new() -> Self {
        let registry = Arc::new(create_step_registry());
        let persistence = Arc::new(create_persistence_provider());
        let lock = Arc::new(LocalLockProvider::new());
        let queue = Arc::new(SyncQueueProvider);

        let executor = Arc::new(
            WorkflowExecutor::new(persistence, lock, queue)
        );

        WfEngine { executor, registry }
    }

    /// 创建并启动审批流程
    pub async fn create_workflow(
        &self,
        def_id: &str,
        biz_type: &str,
        biz_id: i64,
        applicant_id: i64,
    ) -> Result<String, String> {
        let definition = get_definition(def_id)?;
        let instance_id = format!("wf-{}-{}-{}", def_id, biz_id, chrono::Utc::now().timestamp());

        let data = json!({
            "biz_type": biz_type,
            "biz_id": biz_id,
            "applicant_id": applicant_id,
        });

        let mut instance = WorkflowInstance::new(&instance_id, 1, data);
        instance.workflow_definition_id = def_id.to_string();

        let pointer = ExecutionPointer::new(definition.steps[0].id);
        instance.execution_pointers.push(pointer);

        self.executor
            .execute(&instance_id, &definition, &self.registry, None)
            .await
            .map_err(|e| format!("执行审批流程失败: {}", e))?;

        Ok(instance_id)
    }

    /// 执行审批操作（通过/驳回）
    pub async fn approve_step(
        &self,
        workflow_id: &str,
        action: &str,       // "approve" | "reject"
        comment: &str,
        approver_id: i64,
    ) -> Result<(), String> {
        approve_workflow_step_inner(
            &self.executor,
            &self.registry,
            workflow_id,
            action,
            comment,
            approver_id,
        ).await
    }

    /// 获取工作流状态
    pub async fn get_status(
        &self,
        biz_type: &str,
        biz_id: i64,
    ) -> Result<serde_json::Value, String> {
        get_workflow_status_inner(biz_type, biz_id).await
    }
}
```

---

## 八、设备领用完整流程示例

### 8.1 流程定义

```
[Step 0: 申请人提交]
    type: ApprovalStep
    config: { role: "applicant", title: "设备领用申请" }
    ▼
[Step 1: 上级审批]
    type: ApprovalStep
    config: { role: "applicant_superior", title: "上级审批" }
    ▼
[Step 2: 设备管理员审批]
    type: ApprovalStep
    config: { role: "asset_manager", title: "设备管理员审批" }
    ▼
[Step 3: 通知领用人]
    type: NotifyStep
    config: { notify_type: "站内信" }
    ▼
[Step 4: 更新资产状态]
    type: AutoStep
    ▼
[END]
```

### 8.2 时序图

```
用户             前端              Tauri/Rust            wfe-core           PostgreSQL
 │                │                  │                     │                  │
 ├─ 提交领用申请 ─▶                   │                     │                  │
 │                │  invoke          │                     │                  │
 │                │─────────────────▶│                     │                  │
 │                │                  │  start_receive      │                  │
 │                │                  │────────────────────▶│                  │
 │                │                  │                     │  create_instance │
 │                │                  │                     │─────────────────▶│
 │                │                  │                     │  execute step 0  │
 │                │                  │                     │─────────────────▶│
 │                │                  │                     │  更新为 Waiting  │
 │                │                  │◀────────────────────│                  │
 │◀─ 显示待审批 ──│                  │                     │                  │
 │                │                  │                     │                  │
 │ 上级登录审批 ──▶                   │                     │                  │
 │                │  approve_step    │                     │                  │
 │                │─────────────────▶│                     │                  │
 │                │                  │  发布审批事件        │                  │
 │                │                  │────────────────────▶│                  │
 │                │                  │  execute step 1     │                  │
 │                │                  │────────────────────▶│                  │
 │                │                  │  执行 step 1 →      │                  │
 │                │                  │  下一步 = step 2    │                  │
 │◀─ 显示已审批 ──│                  │                     │                  │
 │                │                  │                     │                  │
 │领用人收到通知 ─▶                   │                     │                  │
 │                │                  │  execute step 3     │                  │
 │                │                  │  (NotifyStep)       │                  │
 │                │                  │  execute step 4     │                  │
 │                │                  │  (AutoStep: 更新状态)│                  │
 │                │                  │   资产状态 → "在用"   │                  │
 │                │                  │  Complete           │                  │
```

---

## 九、Cargo.toml 变更

```toml
[dependencies]
# 工作流引擎（已添加）
wfe-core = "1.10.0"
wfe-postgres = "1.10.0"
wfe-yaml = "1.10.0"

# 异步 trait
async-trait = "0.1"

# 已有的
sqlx = { version = "0.8", features = ["postgres", "runtime-tokio", "chrono", "json"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1", features = ["full"] }
chrono = { version = "0.4", features = ["serde"] }
once_cell = "1.21"
```

---

## 十、测试方案

### 10.1 单元测试（不需要数据库）

| 测试文件 | 测试内容 |
|----------|----------|
| `steps_test.rs` | ApprovalStep 配置解析、等待事件/恢复执行/驳回逻辑 |
| `definitions_test.rs` | WorkflowBuilder 构建、流程定义序列化/反序列化 |
| `lock_test.rs` | LocalLockProvider 加锁/解锁/并发 |
| `queue_test.rs` | SyncQueueProvider |

### 10.2 集成测试（需要内存数据库或 Mock）

| 测试文件 | 测试内容 |
|----------|----------|
| `executor_test.rs` | WfEngine 初始化、创建流程、审批推进 |
| `persistence_test.rs` | 使用 `sqlx::PgPool` mock 测试持久化层 |
| `integration_test.rs` | 完整审批流程端到端：创建 → 审批通过 → 完成 / 驳回 |

### 10.3 测试覆盖场景

1. ✅ 正常审批通过流程
2. ✅ 审批驳回流程
3. ✅ 多级审批链推进
4. ✅ 工作流实例创建与持久化
5. ✅ 审批事件发布与消费
6. ✅ 流程定义 YAML 序列化/反序列化
7. ✅ LocalLockProvider 并发安全

---

## 十一、实施路线图

| 阶段 | 内容 | 前置 |
|------|------|------|
| **Phase 1** | 创建 workflow 模块 + WfEngine 工具类 | 无 |
| **Phase 2** | 实现审批步骤 + 流程定义 + 持久化层 | Phase 1 |
| **Phase 3** | 实现审批 Command + 事件驱动 | Phase 2 |
| **Phase 4** | 编写单元测试 + 集成测试 | Phase 3 |
| **Phase 5** | 迁移设备领用流程（第一个业务验证） | Phase 4 |
| **Phase 6** | 迁移其余流程（归还/调拨/维修/报废/采购） | Phase 5 |
| **Phase 7** | 前端组件 + 待审批页面 | Phase 6 |
| **Phase 8** | 旧 AssetApproval 表数据迁移 + 下线 | Phase 7 |

---

## 十二、关键设计决策

| 决策 | 原因 |
|------|------|
| 使用 wfe-core 而非自己造轮子 | wfe-core 已实现完整的 DAG 工作流引擎，含条件/并行/Saga/补偿 |
| 使用 wfe-postgres 现成 PersistenceProvider | 避免重复实现 wf_instance/wf_execution_pointer 的 CRUD |
| LocalLockProvider | 单机部署足够，后续可替换 Redis |
| SyncQueueProvider（同步执行） | MVP 简化，审批流程无长时间等待步骤 |
| 业务表增加 workflow_id 字段 | 保持松耦合，业务表和工作流实例双向可查 |
| 保留旧表 asset_receive 等 | 业务数据不动，只增加审批流控制层 |
| 将审批事件建模为 wfe Event | 利用 wfe-core 的事件订阅机制实现异步审批 |
| 审批步骤用 step_config 传参 | 无需为每个步骤创建单独的 struct |
| 前端被动拉取审批状态 | MVP 用轮询，后续可加 WebSocket 推送 |
| 整合为 WfEngine 工具类 | 对外提供统一 API，隐藏 wfe-core/wfe-postgres/wfe-yaml 实现细节 |

---

## 十三、与旧 AssetApproval 的对比

| 能力 | 旧系统 (AssetApproval) | 新系统 (wfe-core) |
|------|----------------------|------------------|
| 审批流程定义 | 硬编码在 service 层 | 声明式 WorkflowDefinition |
| 多级审批 | 有限（step 字段） | 任意多级，支持条件分支 |
| 会签 | ❌ | ✅ (Parallel/SequenceStep) |
| 超时处理 | ❌ | ✅ (ErrorBehavior + sleep) |
| 审批历史 | biz_type/biz_id 模糊关联 | workflow_id 精确关联 |
| 动态审批人 | ❌ | ✅ (role 策略 + 运行时解析) |
| 条件路由 | ❌ | ✅ (IfStep + DecideStep) |
| 子流程 | ❌ | ✅ (SubWorkflowStep) |
| 补偿/回退 | ❌ | ✅ (SagaContainer + compensation_step) |
| 状态机可视化 | ❌ | ✅ (to_dot() 输出 Graphviz) |
| 统一工具类 API | ❌ | ✅ (WfEngine 封装) |