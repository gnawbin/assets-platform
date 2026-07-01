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

## 四、审批步骤实现

### 4.1 目录结构

```
src-tauri/src/workflow/              # [新增] 工作流模块
├── mod.rs                           # 模块入口
├── definitions.rs                   # 流程定义：设备领用、维修、采购等
├── steps.rs                         # 自定义审批 StepBody 实现
├── persistence.rs                   # PostgreSQL PersistenceProvider 实现
├── queue.rs                         # QueueProvider 实现（同步简化版）
├── lock.rs                          # LockProvider 实现（本地锁）
├── executor.rs                      # 封装 WorkflowExecutor 启动
└── commands.rs                      # Tauri Command（审批操作）
```

### 4.2 自定义审批步骤

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

### 4.3 流程定义

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

## 五、持久化实现

### 5.1 PersistenceProvider（PostgreSQL）

```rust
// workflow/persistence.rs

use async_trait::async_trait;
use wfe_core::models::{
    Event, ExecutionError, ExecutionPointer, Subscription, WorkflowInstance, WorkflowStatus,
};
use wfe_core::traits::{
    DistributedLockProvider, EventRepository, PersistenceProvider, QueueProvider,
    SubscriptionRepository, WorkflowRepository,
};

use crate::database;

/// PostgreSQL 实现的持久化提供者
pub struct PostgresPersistenceProvider;

fn schema_prefix() -> String {
    let schema = database::postgres::get_current_schema();
    format!("{}.", schema)
}

#[async_trait]
impl WorkflowRepository for PostgresPersistenceProvider {
    async fn create_new_workflow(&self, instance: &WorkflowInstance) -> wfe_core::Result<()> {
        let pool = database::get_write_pool()
            .map_err(|e| wfe_core::WfeError::Persistence(e))?;
        let prefix = schema_prefix();

        let sql = format!(
            r#"INSERT INTO {}wf_instance
               (id, wf_definition_id, version, status, data, next_execution, created_at, updated_at)
               VALUES ($1, $2, $3, $4, $5::jsonb, $6, NOW(), NOW())"#,
            prefix
        );

        sqlx::query(&sql)
            .bind(&instance.id)
            .bind(&instance.workflow_definition_id)
            .bind(instance.version as i32)
            .bind(format!("{:?}", instance.status))
            .bind(&serde_json::to_value(&instance.data)
                .map_err(|e| wfe_core::WfeError::Persistence(e.to_string()))?)
            .bind(instance.next_execution)
            .execute(&pool)
            .await
            .map_err(|e| wfe_core::WfeError::Persistence(e.to_string()))?;

        // 写入执行指针
        for pointer in &instance.execution_pointers {
            self.create_pointer(&instance.id, pointer).await?;
        }

        Ok(())
    }

    async fn get_workflow_instance(&self, workflow_id: &str) -> wfe_core::Result<WorkflowInstance> {
        let pool = database::get_read_pool()
            .map_err(|e| wfe_core::WfeError::Persistence(e))?;
        let prefix = schema_prefix();

        let sql = format!(
            "SELECT id, wf_definition_id, version, status, data, next_execution
             FROM {}wf_instance WHERE id = $1",
            prefix
        );

        let row = sqlx::query(&sql)
            .bind(workflow_id)
            .fetch_one(&pool)
            .await
            .map_err(|e| wfe_core::WfeError::Persistence(e.to_string()))?;

        use sqlx::Row;
        let status_str: String = row.try_get("status").unwrap_or_default();
        let status = match status_str.as_str() {
            "Runnable" => WorkflowStatus::Runnable,
            "Complete" => WorkflowStatus::Complete,
            "Terminated" => WorkflowStatus::Terminated,
            "Suspended" => WorkflowStatus::Suspended,
            _ => WorkflowStatus::Runnable,
        };

        let data_val: serde_json::Value = row.try_get("data").unwrap_or(json!({}));

        let mut instance = WorkflowInstance {
            id: row.try_get("id").unwrap_or_default(),
            workflow_definition_id: row.try_get("wf_definition_id").unwrap_or_default(),
            version: row.try_get::<i32, _>("version").unwrap_or(1) as u32,
            status,
            data: data_val,
            execution_pointers: Vec::new(),
            next_execution: row.try_get("next_execution").unwrap_or(None),
            complete_time: None,
        };

        // 加载执行指针
        let pointers = self.get_pointers(workflow_id).await?;
        instance.execution_pointers = pointers;

        Ok(instance)
    }

    async fn persist_workflow(&self, instance: &WorkflowInstance) -> wfe_core::Result<()> {
        let pool = database::get_write_pool()
            .map_err(|e| wfe_core::WfeError::Persistence(e))?;
        let prefix = schema_prefix();

        // 更新实例状态
        let sql = format!(
            r#"UPDATE {}wf_instance SET
               status = $2, data = $3::jsonb, next_execution = $4, updated_at = NOW()
               WHERE id = $1"#,
            prefix
        );

        sqlx::query(&sql)
            .bind(&instance.id)
            .bind(format!("{:?}", instance.status))
            .bind(&serde_json::to_value(&instance.data)
                .map_err(|e| wfe_core::WfeError::Persistence(e.to_string()))?)
            .bind(instance.next_execution)
            .execute(&pool)
            .await
            .map_err(|e| wfe_core::WfeError::Persistence(e.to_string()))?;

        // 更新所有指针
        for pointer in &instance.execution_pointers {
            self.upsert_pointer(&instance.id, pointer).await?;
        }

        Ok(())
    }

    // ... 其他方法类似实现：create_pointer, get_pointers, upsert_pointer 等
}

/// 简单的内存锁（单机足够，后续可替换为 Redis 锁）
pub struct LocalLockProvider {
    locked: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl LocalLockProvider {
    pub fn new() -> Self {
        Self {
            locked: std::sync::Mutex::new(std::collections::HashSet::new()),
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

/// 同步 QueueProvider：直接在当前线程执行
pub struct SyncQueueProvider;

#[async_trait]
impl QueueProvider for SyncQueueProvider {
    async fn queue_work(&self, _workflow_id: &str, _queue_type: wfe_core::models::QueueType)
        -> wfe_core::Result<()>
    {
        // MVP：不进行异步调度，WorkflowExecutor 会直接再次执行
        Ok(())
    }

    async fn dequeue_work(
        &self,
        _queue_type: wfe_core::models::QueueType,
    ) -> wfe_core::Result<Option<(String, String)>> {
        Ok(None)
    }
}
```

---

## 六、启动和执行

### 6.1 初始化

```rust
// workflow/executor.rs

use std::sync::Arc;
use wfe_core::executor::step_registry::StepRegistry;
use wfe_core::executor::WorkflowExecutor;
use wfe_core::models::{ExecutionPointer, WorkflowInstance, WorkflowStatus};
use serde_json::json;

use super::definitions::*;
use super::persistence::{PostgresPersistenceProvider, LocalLockProvider, SyncQueueProvider};
use super::steps::{ApprovalStep, AutoStep, NotifyStep};
use crate::database;
use crate::database::models::SysUser;

/// 全局审批执行器
static WORKFLOW_EXECUTOR: once_cell::sync::OnceCell<WorkflowExecutor> = once_cell::sync::OnceCell::new();

/// 初始化工作流引擎
pub fn init_workflow_engine() {
    let persistence = Arc::new(PostgresPersistenceProvider);
    let lock = Arc::new(LocalLockProvider::new());
    let queue = Arc::new(SyncQueueProvider);

    let executor = WorkflowExecutor::new(persistence, lock, queue);

    WORKFLOW_EXECUTOR.set(executor).ok();
}

/// 获取全局执行器
fn get_executor() -> &'static WorkflowExecutor {
    WORKFLOW_EXECUTOR.get().expect("工作流引擎未初始化")
}

/// 构建全局 StepRegistry
pub fn create_step_registry() -> StepRegistry {
    let mut registry = StepRegistry::new();
    registry.register::<ApprovalStep>();
    registry.register::<AutoStep>();
    registry.register::<NotifyStep>();
    registry
}
```

### 6.2 启动审批流程

```rust
// 发起设备领用申请时，启动工作流

pub async fn start_receive_workflow(
    receive: &AssetReceive,
    applicant: &SysUser,
    superior_id: i64,
    asset_manager_id: i64,
) -> Result<String, String> {
    let definition = asset_receive_workflow();
    let registry = create_step_registry();

    // 构建工作流数据
    let data = serde_json::json!({
        "biz_type": "receive",
        "biz_id": receive.id,
        "applicant_id": applicant.id,
        "department_id": applicant.department_id,
        "superior_id": superior_id,
        "asset_manager_id": asset_manager_id,
        "apply_reason": receive.reason,
    });

    // 创建工作流实例
    let instance_id = format!("wf-recv-{}", receive.id);
    let mut instance = WorkflowInstance::new(&instance_id, 1, data);
    instance.workflow_definition_id = "asset_receive".to_string();

    // 添加初始执行指针（指向第一步）
    let pointer = ExecutionPointer::new(definition.steps[0].id);
    instance.execution_pointers.push(pointer);

    // 持久化实例
    let persistence = PostgresPersistenceProvider;
    persistence.create_new_workflow(&instance)
        .await
        .map_err(|e| format!("创建审批流程失败: {}", e))?;

    // 执行第一步
    let executor = get_executor();
    executor.execute(&instance_id, &definition, &registry, None)
        .await
        .map_err(|e| format!("执行审批流程失败: {}", e))?;

    Ok(instance_id)
}
```

### 6.3 审批操作（前端触发）

```rust
// workflow/commands.rs

use super::definitions::*;
use super::steps::ApprovalEvent;
use super::persistence::PostgresPersistenceProvider;
use super::executor::{get_executor, create_step_registry};
use crate::database;

/// 审批操作：通过/驳回
#[tauri::command]
pub async fn approve_workflow_step(
    workflow_id: String,
    action: String,       // "approve" | "reject"
    comment: String,
    user_id: String,
) -> Result<(), String> {
    let uid: i64 = user_id.parse().map_err(|e| format!("无效用户ID: {}", e))?;

    // 1. 从数据库加载工作流实例
    let persistence = PostgresPersistenceProvider;
    let instance = persistence.get_workflow_instance(&workflow_id)
        .await
        .map_err(|e| format!("加载审批流程失败: {}", e))?;

    // 2. 找到当前活跃的审批步骤
    let active_pointer = instance.execution_pointers
        .iter()
        .find(|p| p.active && p.status == wfe_core::models::PointerStatus::WaitingForEvent)
        .ok_or("没有待审批的步骤")?;

    // 3. 发布审批事件
    let event = wfe_core::models::Event::new(
        "approval.event",
        &format!("{}-approval-{}", workflow_id, active_pointer.step_id),
        serde_json::json!({
            "action": action,
            "comment": comment,
            "user_id": uid,
        }),
    );

    // 使用 PersistenceProvider 创建事件
    persistence.create_event(&event)
        .await
        .map_err(|e| format!("创建审批事件失败: {}", e))?;
    persistence.publish_event(&event.id)
        .await
        .map_err(|e| format!("发布审批事件失败: {}", e))?;

    // 4. 记录审批记录
    let pool = database::get_write_pool()
        .map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = schema_prefix();
    sqlx::query(&format!(
        r#"INSERT INTO {}wf_approval_record
           (workflow_id, step_id, step_name, approver_id, action, comment, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, NOW())"#,
        prefix
    ))
    .bind(&workflow_id)
    .bind(active_pointer.step_id as i32)
    .bind(&active_pointer.step_name)
    .bind(uid)
    .bind(&action)
    .bind(&comment)
    .execute(&pool)
    .await
    .map_err(|e| format!("记录审批日志失败: {}", e))?;

    // 5. 重新执行工作流（推进到下一步）
    let definition = match instance.workflow_definition_id.as_str() {
        "asset_receive" => asset_receive_workflow(),
        "asset_repair" => asset_repair_workflow(),
        "asset_purchase" => asset_purchase_workflow(),
        other => return Err(format!("未知的工作流定义: {}", other)),
    };

    let registry = create_step_registry();
    let executor = get_executor();

    executor.execute(&workflow_id, &definition, &registry, None)
        .await
        .map_err(|e| format!("推进审批流程失败: {}", e))?;

    // 6. 如果是驳回，将业务表状态更新为"已驳回"
    if action == "reject" {
        update_biz_status_rejected(&instance).await?;
    }

    Ok(())
}

/// 更新业务表状态为已驳回
async fn update_biz_status_rejected(instance: &WorkflowInstance) -> Result<(), String> {
    let biz_type = instance.data["biz_type"].as_str().unwrap_or("");
    let biz_id = instance.data["biz_id"].as_i64().unwrap_or(0);
    let pool = database::get_write_pool()
        .map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = schema_prefix();

    match biz_type {
        "receive" => {
            sqlx::query(&format!(
                "UPDATE {}asset_receive SET status = 2, updated_at = NOW() WHERE id = $1",
                prefix
            ))
            .bind(biz_id)
            .execute(&pool)
            .await
            .map_err(|e| format!("更新状态失败: {}", e))?;
        }
        "repair" => {
            sqlx::query(&format!(
                "UPDATE {}asset_repair SET status = 0, updated_at = NOW() WHERE id = $1",
                prefix
            ))
            .bind(biz_id)
            .execute(&pool)
            .await
            .map_err(|e| format!("更新状态失败: {}", e))?;
        }
        // ... 其他业务表
        _ => {}
    }
    Ok(())
}
```

### 6.4 查询审批状态

```rust
/// 查询某个业务记录的审批流程状态
#[tauri::command]
pub async fn get_workflow_status(
    biz_type: String,
    biz_id: String,
) -> Result<serde_json::Value, String> {
    let bid: i64 = biz_id.parse().map_err(|e| format!("无效ID: {}", e))?;
    let pool = database::get_read_pool()
        .map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = schema_prefix();

    // 查询工作流实例
    let row = sqlx::query(&format!(
        "SELECT id, status, data::text, created_at, updated_at
         FROM {}wf_instance
         WHERE biz_type = $1 AND biz_id = $2
         ORDER BY created_at DESC LIMIT 1",
        prefix
    ))
    .bind(&biz_type)
    .bind(bid)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("查询失败: {}", e))?;

    match row {
        Some(r) => {
            use sqlx::Row;
            let workflow_id: String = r.try_get("id").unwrap_or_default();
            let status: String = r.try_get("status").unwrap_or_default();

            // 查询审批记录
            let records = sqlx::query(&format!(
                "SELECT step_id, step_name, approver_id, action, comment, created_at
                 FROM {}wf_approval_record
                 WHERE workflow_id = $1
                 ORDER BY created_at",
                prefix
            ))
            .bind(&workflow_id)
            .fetch_all(&pool)
            .await
            .map_err(|e| format!("查询审批记录失败: {}", e))?;

            let approval_records: Vec<serde_json::Value> = records.iter().map(|rec| {
                use sqlx::Row;
                json!({
                    "step_id": rec.try_get::<i32, _>("step_id").unwrap_or(0),
                    "step_name": rec.try_get::<String, _>("step_name").unwrap_or_default(),
                    "approver_id": rec.try_get::<i64, _>("approver_id").unwrap_or(0),
                    "action": rec.try_get::<String, _>("action").unwrap_or_default(),
                    "comment": rec.try_get::<String, _>("comment").unwrap_or_default(),
                    "created_at": rec.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").map(|t| t.to_rfc3339()).unwrap_or_default(),
                })
            }).collect();

            Ok(json!({
                "workflow_id": workflow_id,
                "status": status,
                "approval_records": approval_records,
            }))
        }
        None => Ok(json!({
            "workflow_id": null,
            "status": "none",
            "approval_records": [],
        })),
    }
}
```

---

## 七、前端组件

### 7.1 目录结构

```
src/
└── components/
    └── ApprovalFlow/               # [新增] 审批流组件
        ├── index.tsx               # 审批流程展示组件
        ├── ApprovalTimeline.tsx    # 审批时间线
        ├── ApprovalAction.tsx      # 通过/驳回按钮
        ├── ApprovalComment.tsx     # 审批意见输入
        └── types.ts                # 类型定义

    └── pages/
        └── MyApprovals.tsx         # [新增] 我的待审批
        └── ApprovalDetail.tsx      # [新增] 审批详情
```

### 7.2 前端类型

```typescript
// types/workflow.ts

export interface WorkflowStatus {
  workflow_id: string | null;
  status: 'Runnable' | 'Complete' | 'Terminated' | 'Suspended' | 'none';
  approval_records: ApprovalRecord[];
}

export interface ApprovalRecord {
  step_id: number;
  step_name: string;
  approver_id: string;
  action: 'approve' | 'reject';
  comment: string;
  created_at: string;
}

export interface ApprovalAction {
  workflow_id: string;
  action: 'approve' | 'reject';
  comment: string;
  user_id: string;
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
# 新增工作流引擎
wfe-core = { version = "1.10.0", default-features = false }
# 注意：wfe-core 默认依赖 opentelemetry，用 default-features = false 关闭

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

## 十、实施路线图

| 阶段 | 内容 | 工期 | 前置 |
|------|------|------|------|
| **Phase 1** | 新增数据库表 + Rust struct | 1天 | 无 |
| **Phase 2** | 实现 PostgreSQL PersistenceProvider | 2天 | Phase 1 |
| **Phase 3** | 实现审批步骤 + 流程定义 | 1天 | Phase 2 |
| **Phase 4** | 实现审批 Command + 事件驱动 | 1天 | Phase 3 |
| **Phase 5** | 迁移设备领用流程（第一个业务验证） | 1天 | Phase 4 |
| **Phase 6** | 迁移其余 5 个流程（归还/调拨/维修/报废/采购） | 2天 | Phase 5 |
| **Phase 7** | 前端组件 + 待审批页面 | 2天 | Phase 5 |
| **Phase 8** | 旧 AssetApproval 表数据迁移 + 下线 | 1天 | Phase 7 |

**总计：约 11 天**

---

## 十一、关键设计决策

| 决策 | 原因 |
|------|------|
| 使用 wfe-core 而非自己造轮子 | wfe-core 已实现完整的 DAG 工作流引擎，含条件/并行/Saga/补偿 |
| 实现 PersistenceProvider 用 PostgreSQL | wfe-sqlite 不适合多租户场景，PostgreSQL 是现有基础设施 |
| LocalLockProvider | 单机部署足够，后续可替换 Redis |
| SyncQueueProvider（同步执行） | MVP 简化，审批流程无长时间等待步骤 |
| 业务表增加 workflow_id 字段 | 保持松耦合，业务表和工作流实例双向可查 |
| 保留旧表 asset_receive 等 | 业务数据不动，只增加审批流控制层 |
| 将审批事件建模为 wfe Event | 利用 wfe-core 的事件订阅机制实现异步审批 |
| 审批步骤用 step_config 传参 | 无需为每个步骤创建单独的 struct |
| 前端被动拉取审批状态 | MVP 用轮询，后续可加 WebSocket 推送 |

---

## 十二、与旧 AssetApproval 的对比

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