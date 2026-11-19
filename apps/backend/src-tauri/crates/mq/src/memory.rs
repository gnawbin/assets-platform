//! 内存消息总线（默认 feature `memory`）。
//!
//! 仅用于单机 / 单测：基于 `tokio::broadcast`，语义是**纯 fan-out**：
//! - 无持久化、无 ack、无 DLQ；
//! - 订阅只能收到订阅之后发布的消息；
//! - 慢消费者 `Lagged` 时跳过消息（与 Pulsar 的 at-least-once 不同）。
//!
//! 多客户端语义：
//! - 不同订阅名 → 各自收到全量；
//! - 同订阅名二次订阅 → [`MqError::AlreadySubscribed`]（不会像 Pulsar 那样静默分摊）。

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use tokio::sync::{broadcast, watch};

use crate::{
    Bus, Envelope, Inbound, MessageHandler, MqConfig, MqError, MqHealth, MqStats, Publisher,
    Receipt, Subscription, SubscriptionSpec, TopicName,
};

/// 内存总线实例（`PULSAR_ENABLED=false` 时由 `mq::from_config` 装配）
pub struct MemoryBus {
    inner: Arc<MemoryInner>,
    publisher: Arc<MemoryPublisher>,
}

impl MemoryBus {
    /// 创建内存总线（不建立任何网络连接）
    #[must_use]
    pub fn new(cfg: MqConfig) -> Self {
        let inner = Arc::new(MemoryInner::new(cfg));
        let publisher = Arc::new(MemoryPublisher {
            inner: Arc::clone(&inner),
        });
        Self { inner, publisher }
    }
}

#[async_trait]
impl Bus for MemoryBus {
    fn publisher(&self) -> Arc<dyn Publisher> {
        self.publisher.clone()
    }

    fn subscribe(
        &self,
        spec: SubscriptionSpec,
        handler: Arc<dyn MessageHandler>,
    ) -> Result<Arc<dyn Subscription>, MqError> {
        if self.inner.is_closed() {
            return Err(MqError::Closed);
        }
        if spec.subscription.trim().is_empty() {
            return Err(MqError::Config("订阅名不能为空".to_string()));
        }

        let topic = TopicName::build(
            &self.inner.cfg.tenant,
            &self.inner.cfg.namespace,
            &spec.topic,
        )?;

        let mut subscriptions = lock(&self.inner.subscriptions);
        // 拿锁后二次检查：close() 先置 closed 再 drain，避免竞态下漏关订阅
        if self.inner.is_closed() {
            return Err(MqError::Closed);
        }
        if let Some(existing) = subscriptions.get(&spec.subscription) {
            return Err(if existing.spec == spec {
                MqError::AlreadySubscribed {
                    subscription: spec.subscription.clone(),
                }
            } else {
                MqError::SubscriptionConflict {
                    subscription: spec.subscription.clone(),
                }
            });
        }

        let Some(sender) = self.inner.channel(&topic) else {
            return Err(MqError::Closed);
        };
        let receiver = sender.subscribe();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let handle = Arc::new(MemorySubscription {
            name: spec.subscription.clone(),
            inner: Arc::downgrade(&self.inner),
            shutdown_tx,
            task: Mutex::new(None),
        });
        let task = tokio::spawn(run_consumer(
            spec.subscription.clone(),
            receiver,
            handler,
            shutdown_rx,
        ));
        *lock(&handle.task) = Some(task);

        subscriptions.insert(
            spec.subscription.clone(),
            SubscriptionEntry {
                spec,
                handle: Arc::clone(&handle),
            },
        );
        drop(subscriptions);

        Ok(handle)
    }

    fn health(&self) -> MqHealth {
        MqHealth {
            enabled: self.inner.cfg.enabled,
            connected: !self.inner.is_closed(),
            brokers: 0,
            instance_id: self.inner.cfg.instance_id.clone(),
            subscription_scope: self.inner.cfg.subscription_scope,
        }
    }

    fn stats(&self) -> MqStats {
        MqStats {
            producers: lock(&self.inner.channels).len(),
            subscriptions: lock(&self.inner.subscriptions).len(),
            pending_publishes: 0,
        }
    }

    async fn close(&self) -> Result<(), MqError> {
        if self.inner.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        let handles = {
            let mut subscriptions = lock(&self.inner.subscriptions);
            subscriptions
                .drain()
                .map(|(_, entry)| entry.handle)
                .collect::<Vec<_>>()
        };
        for handle in handles {
            let _ = tokio::time::timeout(self.inner.cfg.op_timeout, handle.shutdown()).await;
        }

        lock(&self.inner.channels).clear();
        Ok(())
    }
}

struct MemoryInner {
    cfg: MqConfig,
    channels: Mutex<HashMap<String, broadcast::Sender<Envelope>>>,
    subscriptions: Mutex<HashMap<String, SubscriptionEntry>>,
    closed: AtomicBool,
    message_seq: AtomicU64,
}

impl MemoryInner {
    fn new(cfg: MqConfig) -> Self {
        Self {
            cfg,
            channels: Mutex::new(HashMap::new()),
            subscriptions: Mutex::new(HashMap::new()),
            closed: AtomicBool::new(false),
            message_seq: AtomicU64::new(0),
        }
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// 取得（或创建）topic 对应的广播通道；已关闭返回 `None`
    fn channel(&self, topic: &str) -> Option<broadcast::Sender<Envelope>> {
        if self.is_closed() {
            return None;
        }
        let mut channels = lock(&self.channels);
        // 拿锁后二次检查：避免 close() 清理后又被并发 publish 重新创建
        if self.is_closed() {
            return None;
        }
        let capacity = self.cfg.receiver_queue_size.max(1);
        Some(
            channels
                .entry(topic.to_string())
                .or_insert_with(|| {
                    let (sender, _receiver) = broadcast::channel(capacity);
                    sender
                })
                .clone(),
        )
    }
}

struct SubscriptionEntry {
    spec: SubscriptionSpec,
    handle: Arc<MemorySubscription>,
}

struct MemoryPublisher {
    inner: Arc<MemoryInner>,
}

#[async_trait]
impl Publisher for MemoryPublisher {
    async fn publish(&self, msg: Envelope) -> Result<Receipt, MqError> {
        if self.inner.is_closed() {
            return Err(MqError::Closed);
        }

        let topic = TopicName::build(
            &self.inner.cfg.tenant,
            &self.inner.cfg.namespace,
            &msg.topic,
        )?;
        let Some(sender) = self.inner.channel(&topic) else {
            return Err(MqError::Closed);
        };

        if sender.send(msg).is_err() {
            tracing::debug!(topic = %topic, "内存总线无活跃订阅者，消息丢弃（无持久化）");
        }

        let sequence = self.inner.message_seq.fetch_add(1, Ordering::Relaxed) + 1;
        Ok(Receipt {
            topic,
            message_id: format!("memory:{sequence}"),
        })
    }

    async fn flush(&self) -> Result<(), MqError> {
        Ok(())
    }
}

struct MemorySubscription {
    name: String,
    inner: Weak<MemoryInner>,
    shutdown_tx: watch::Sender<bool>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

#[async_trait]
impl Subscription for MemorySubscription {
    async fn shutdown(&self) -> Result<(), MqError> {
        let _ = self.shutdown_tx.send(true);

        let inner = self.inner.upgrade();
        let timeout = inner
            .as_ref()
            .map_or(Duration::from_secs(30), |inner| inner.cfg.op_timeout);

        // 先取走 JoinHandle，确保 MutexGuard 在 await 前释放（future 需要 Send）
        let task = lock(&self.task).take();
        if let Some(mut task) = task
            && tokio::time::timeout(timeout, &mut task).await.is_err()
        {
            task.abort();
            tracing::warn!(
                subscription = %self.name,
                "内存订阅关闭超时，已强制中止"
            );
        }

        if let Some(inner) = inner {
            lock(&inner.subscriptions).remove(&self.name);
        }
        Ok(())
    }
}

impl Drop for MemorySubscription {
    fn drop(&mut self) {
        if let Ok(mut task) = self.task.lock()
            && let Some(task) = task.take()
        {
            task.abort();
        }
    }
}

async fn run_consumer(
    name: String,
    mut receiver: broadcast::Receiver<Envelope>,
    handler: Arc<dyn MessageHandler>,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            next_message = receiver.recv() => {
                match next_message {
                    Ok(envelope) => {
                        let inbound = Inbound {
                            attributes: envelope.attributes(),
                            envelope,
                        };
                        if let Err(error) = handler.handle(inbound).await {
                            tracing::warn!(
                                subscription = %name,
                                %error,
                                "内存总线消费失败（无重投 / 无 DLQ）"
                            );
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(
                            subscription = %name,
                            skipped,
                            "内存总线消费滞后，已跳过消息（无持久化）"
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
