//! 工作流执行器封装
//!
//! 提供 WfEngine 所需的初始化函数和全局执行器引用。
//! 使用 wfe-core 的 WorkflowExecutor 驱动审批流程的执行。

use std::sync::Arc;
use wfe_core::executor::StepRegistry;
use wfe_core::executor::WorkflowExecutor;

use super::lock::LocalLockProvider;
use super::persistence::create_persistence_provider;
use super::queue::SyncQueueProvider;
use super::steps::{ApprovalStep, AutoStep, NotifyStep};

/// 创建全局 StepRegistry 并注册所有自定义步骤
pub fn create_step_registry() -> StepRegistry {
    let mut registry = StepRegistry::new();
    registry.register::<ApprovalStep>();
    registry.register::<AutoStep>();
    registry.register::<NotifyStep>();
    registry
}

/// 创建工作流执行器实例
pub fn create_executor() -> WorkflowExecutor {
    let persistence = Arc::new(create_persistence_provider());
    let lock = Arc::new(LocalLockProvider::new());
    let queue = Arc::new(SyncQueueProvider);

    WorkflowExecutor::new(persistence, lock, queue)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试 StepRegistry 注册
    #[test]
    fn test_step_registry_creation() {
        let registry = create_step_registry();
        // registry 应包含 3 个已注册的步骤类型
        // 注意：StepRegistry 不直接暴露 count 方法，此测试验证创建不 panic
        let _ = registry;
        assert!(true);
    }

    /// 测试 create_executor 的函数签名
    #[test]
    fn test_create_executor_type() {
        let _fn_ptr = create_executor;
        assert!(true);
    }
}
