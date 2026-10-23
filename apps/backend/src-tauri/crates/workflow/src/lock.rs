//! 分布式锁提供者（本地内存实现）
//!
//! 基于 wfe-core 的 DistributedLockProvider trait 实现。
//! 单机部署足够，后续可替换为 Redis 锁。

use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::Mutex;
use wfe_core::traits::DistributedLockProvider;

/// 本地内存锁提供者
///
/// 使用 Mutex<HashSet<String>> 实现简单的互斥锁。
/// - 单机环境下足够安全
/// - 后续可替换为 Redis 分布式锁
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

impl Default for LocalLockProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DistributedLockProvider for LocalLockProvider {
    async fn acquire_lock(&self, resource: &str) -> wfe_core::Result<bool> {
        let mut locked = self.locked.lock().unwrap();
        if locked.contains(resource) {
            Ok(false)
        } else {
            locked.insert(resource.to_string());
            Ok(true)
        }
    }

    async fn release_lock(&self, resource: &str) -> wfe_core::Result<()> {
        let mut locked = self.locked.lock().unwrap();
        locked.remove(resource);
        Ok(())
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
    async fn test_lock_acquire_and_release() {
        let lock = LocalLockProvider::new();

        // 加锁
        let acquired = lock.acquire_lock("test-key").await.unwrap();
        assert!(acquired);

        // 重复加锁应失败
        let acquired = lock.acquire_lock("test-key").await.unwrap();
        assert!(!acquired);

        // 释放锁
        lock.release_lock("test-key").await.unwrap();

        // 再次加锁应成功
        let acquired = lock.acquire_lock("test-key").await.unwrap();
        assert!(acquired);

        lock.release_lock("test-key").await.unwrap();
    }

    #[tokio::test]
    async fn test_lock_independent_keys() {
        let lock = LocalLockProvider::new();

        let a = lock.acquire_lock("key-a").await.unwrap();
        let b = lock.acquire_lock("key-b").await.unwrap();
        assert!(a);
        assert!(b);

        lock.release_lock("key-a").await.unwrap();
        lock.release_lock("key-b").await.unwrap();
    }

    #[test]
    fn test_lock_default() {
        let lock = LocalLockProvider::default();
        let _ = lock; // 确保 Default 实现
    }
}
