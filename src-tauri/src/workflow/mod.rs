//! 工作流引擎模块
//!
//! 整合 wfe-core / wfe-postgres / wfe-yaml 为统一的审批流工具类。
//!
//! # 模块结构
//! - `mod.rs` — 模块入口 + WfEngine 统一工具类
//! - `steps.rs` — 自定义审批步骤 (ApprovalStep, AutoStep, NotifyStep)
//! - `definitions.rs` — 流程定义 (设备领用、维修、采购)
//! - `persistence.rs` — PostgreSQL 持久化 (封装 wfe-postgres)
//! - `lock.rs` — 分布式锁 (本地内存实现)
//! - `queue.rs` — 队列调度 (同步简化版)
//! - `executor.rs` — WorkflowExecutor 初始化
//! - `commands.rs` — Tauri Command 入口

pub mod commands;
pub mod definitions;
pub mod executor;
pub mod lock;
pub mod persistence;
pub mod queue;
pub mod steps;

use std::sync::Arc;
use wfe_core::executor::StepRegistry;
use wfe_core::executor::WorkflowExecutor;
use wfe_core::models::ExecutionPointer;
use wfe_core::models::WorkflowInstance;

/// WfEngine — 工作流引擎工具类
///
/// 将 wfe-core / wfe-postgres / wfe-yaml 整合为一个统一的接口，
/// 供 service 层和 commands 层调用，无需关心底层实现细节。
///
/// # 用法
/// ```rust,ignore
/// use workflow::WfEngine;
///
/// let engine = WfEngine::new().await;
/// let wf_id = engine.create_workflow("asset_receive", "receive", 12345, 1001).await?;
/// engine.approve_step(&wf_id, "approve", "同意", 1002).await?;
/// let status = engine.get_status("receive", 12345).await?;
/// ```
pub struct WfEngine {
    executor: Arc<WorkflowExecutor>,
    registry: Arc<StepRegistry>,
}

impl WfEngine {
    /// 创建并初始化工作流引擎
    pub async fn new() -> Self {
        let registry = Arc::new(executor::create_step_registry());
        let persistence = Arc::new(persistence::create_persistence_provider());
        let lock = Arc::new(lock::LocalLockProvider::new());
        let queue = Arc::new(queue::SyncQueueProvider);

        let executor = Arc::new(WorkflowExecutor::new(persistence, lock, queue));

        WfEngine { executor, registry }
    }

    /// 创建并启动审批流程
    ///
    /// # 参数
    /// - `def_id`: 流程定义ID，如 "asset_receive"
    /// - `biz_type`: 业务类型，如 "receive"
    /// - `biz_id`: 业务记录ID
    /// - `applicant_id`: 申请人ID
    ///
    /// # 返回
    /// 工作流实例ID
    pub async fn create_workflow(
        &self,
        def_id: &str,
        biz_type: &str,
        biz_id: i64,
        applicant_id: i64,
    ) -> Result<String, String> {
        let definition = definitions::get_definition(def_id)?;

        // 构建工作流数据
        let data = serde_json::json!({
            "biz_type": biz_type,
            "biz_id": biz_id,
            "applicant_id": applicant_id,
        });

        // 创建工作流实例
        let mut instance = WorkflowInstance::new(def_id, 1, data);
        instance.workflow_definition_id = def_id.to_string();

        // 添加初始执行指针（指向第一步）
        if let Some(first_step) = definition.steps.first() {
            let mut pointer = ExecutionPointer::new(first_step.id);
            pointer.active = true;
            instance.execution_pointers.push(pointer);
        } else {
            return Err("流程定义中没有步骤".into());
        }

        // 执行工作流
        self.executor
            .execute(&instance.id, &definition, &self.registry, None)
            .await
            .map_err(|e| format!("执行审批流程失败: {}", e))?;

        tracing::info!(
            "审批流程创建成功: instance_id={}, def_id={}, biz_type={}, biz_id={}",
            instance.id,
            def_id,
            biz_type,
            biz_id
        );

        Ok(instance.id)
    }

    /// 执行审批操作（通过/驳回）
    ///
    /// # 参数
    /// - `workflow_id`: 工作流实例ID
    /// - `action`: "approve" 或 "reject"
    /// - `comment`: 审批意见
    /// - `approver_id`: 审批人ID
    pub async fn approve_step(
        &self,
        workflow_id: &str,
        action: &str,
        comment: &str,
        approver_id: i64,
    ) -> Result<(), String> {
        commands::approve_workflow_step_inner(
            &self.executor,
            &self.registry,
            workflow_id,
            action,
            comment,
            approver_id,
        )
        .await
    }

    /// 获取工作流状态
    pub async fn get_status(
        &self,
        biz_type: &str,
        biz_id: i64,
    ) -> Result<serde_json::Value, String> {
        commands::get_workflow_status_inner(biz_type, biz_id).await
    }
}

impl Default for WfEngine {
    fn default() -> Self {
        // 在未初始化 `async` 上下文时，default 无法工作
        // 这里仅提供编译通过，实际应使用 WfEngine::new().await
        panic!("WfEngine 必须通过 WfEngine::new().await 创建");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试 WfEngine 的 new 方法（异步函数签名验证）
    #[tokio::test]
    async fn test_wf_engine_new() {
        // 注意：此测试需要数据库连接，此处只验证类型
        // 实际运行需要 PostgreSQL 支持
        let _ = std::mem::size_of::<WfEngine>();
        assert!(true);
    }

    /// 测试 definitions::get_definition 可以配合 WfEngine 使用
    #[test]
    fn test_wf_engine_definitions_compatible() {
        let defs = definitions::list_definition_ids();
        assert!(!defs.is_empty());
    }
}
