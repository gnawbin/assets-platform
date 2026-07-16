//! 队列提供者（同步执行简化版）
//!
//! 基于 wfe-core 的 QueueProvider trait 实现。
//! MVP 阶段使用同步队列，不进行异步调度。
//! WorkflowExecutor 会直接在当前线程继续执行。

use async_trait::async_trait;
use wfe_core::models::QueueType;
use wfe_core::traits::QueueProvider;

/// 同步队列提供者
///
/// 所有操作均为空操作（no-op），WorkflowExecutor 会同步继续执行。
/// 后续可替换为真正的消息队列（如 RabbitMQ / Redis Stream）。
pub struct SyncQueueProvider;

impl SyncQueueProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SyncQueueProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl QueueProvider for SyncQueueProvider {
    async fn queue_work(&self, _id: &str, _queue: QueueType) -> wfe_core::Result<()> {
        // MVP：不进行异步调度，WorkflowExecutor 会直接再次执行
        Ok(())
    }

    async fn dequeue_work(&self, _queue: QueueType) -> wfe_core::Result<Option<String>> {
        Ok(None)
    }

    fn is_dequeue_blocking(&self) -> bool {
        false
    }

    async fn start(&self) -> wfe_core::Result<()> {
        Ok(())
    }

    async fn stop(&self) -> wfe_core::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sync_queue_provider() {
        let queue = SyncQueueProvider::new();

        // queue_work 应返回 Ok
        let result = queue.queue_work("test-wf", QueueType::Workflow).await;
        assert!(result.is_ok());

        // dequeue_work 应返回 None
        let result = queue.dequeue_work(QueueType::Workflow).await.unwrap();
        assert!(result.is_none());

        // is_dequeue_blocking 应返回 false
        assert!(!queue.is_dequeue_blocking());

        // start/stop 应返回 Ok
        assert!(queue.start().await.is_ok());
        assert!(queue.stop().await.is_ok());
    }

    #[test]
    fn test_sync_queue_default() {
        let queue = SyncQueueProvider::default();
        let _ = queue;
    }
}
