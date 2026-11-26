#![cfg(feature = "memory")]
//! 内存实现的多客户端语义测试（无 broker）。

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use mq::{
    Bus, Envelope, Inbound, MessageHandler, MqConfig, MqError, MqInitialPosition, MqSubType,
    SubscriptionSpec,
};
use serde_json::json;
use tokio::sync::mpsc;

struct Collect {
    tx: mpsc::UnboundedSender<Envelope>,
}

#[async_trait]
impl MessageHandler for Collect {
    async fn handle(&self, msg: Inbound) -> Result<(), MqError> {
        let _ = self.tx.send(msg.envelope);
        Ok(())
    }
}

fn spec(cfg: &MqConfig, purpose: &str) -> SubscriptionSpec {
    SubscriptionSpec {
        topic: "tg.cluster.graph.0".to_string(),
        subscription: cfg.subscription_name(purpose).expect("订阅名非法"),
        sub_type: MqSubType::KeyShared,
        initial_position: MqInitialPosition::Earliest,
        read_compacted: false,
    }
}

async fn recv_n(rx: &mut mpsc::UnboundedReceiver<Envelope>, count: usize) -> Vec<Envelope> {
    let mut messages = Vec::with_capacity(count);
    for _ in 0..count {
        let message = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("等待消息超时")
            .expect("消息通道已关闭");
        messages.push(message);
    }
    messages
}

fn envelope(index: usize) -> Envelope {
    let mut envelope = Envelope::new(
        format!("snowflake-{index}"),
        "tg.cluster.graph.0",
        "graph.node.upsert",
        json!({"index": index}),
    );
    envelope.partition_key = Some("node-1".to_string());
    envelope
}

#[tokio::test]
async fn different_subscriptions_receive_full_fanout() {
    let cfg = MqConfig::default();
    let bus = mq::memory::MemoryBus::new(cfg.clone());
    let publisher = bus.publisher();

    let (first_tx, mut first_rx) = mpsc::unbounded_channel();
    let (second_tx, mut second_rx) = mpsc::unbounded_channel();
    let first = bus
        .subscribe(spec(&cfg, "consumer-a"), Arc::new(Collect { tx: first_tx }))
        .expect("订阅 A 失败");
    let second = bus
        .subscribe(
            spec(&cfg, "consumer-b"),
            Arc::new(Collect { tx: second_tx }),
        )
        .expect("订阅 B 失败");

    for index in 0..3 {
        publisher.publish(envelope(index)).await.expect("发布失败");
    }

    let first_messages = recv_n(&mut first_rx, 3).await;
    let second_messages = recv_n(&mut second_rx, 3).await;
    assert_eq!(first_messages.len(), 3);
    assert_eq!(second_messages.len(), 3);
    assert_eq!(bus.stats().subscriptions, 2);
    assert_eq!(bus.stats().producers, 1);

    first.shutdown().await.expect("关闭订阅 A 失败");
    second.shutdown().await.expect("关闭订阅 B 失败");
    bus.close().await.expect("关闭总线失败");
}

#[tokio::test]
async fn duplicate_subscription_is_rejected_locally() {
    let cfg = MqConfig::default();
    let bus = mq::memory::MemoryBus::new(cfg.clone());
    let (tx, _rx) = mpsc::unbounded_channel();
    let subscription = spec(&cfg, "consumer-a");

    let handle = bus
        .subscribe(subscription.clone(), Arc::new(Collect { tx }))
        .expect("首次订阅失败");

    let (second_tx, _second_rx) = mpsc::unbounded_channel();
    let Err(error) = bus.subscribe(subscription, Arc::new(Collect { tx: second_tx })) else {
        panic!("同订阅名第二次订阅必须被拒绝");
    };
    assert!(matches!(error, MqError::AlreadySubscribed { .. }));

    handle.shutdown().await.expect("关闭订阅失败");
    bus.close().await.expect("关闭总线失败");
}

#[tokio::test]
async fn same_subscription_with_different_spec_is_conflict() {
    let cfg = MqConfig::default();
    let bus = mq::memory::MemoryBus::new(cfg.clone());
    let (tx, _rx) = mpsc::unbounded_channel();
    let subscription = spec(&cfg, "consumer-a");

    let handle = bus
        .subscribe(subscription.clone(), Arc::new(Collect { tx }))
        .expect("首次订阅失败");

    let mut conflicting = subscription;
    conflicting.sub_type = MqSubType::Shared;
    let (second_tx, _second_rx) = mpsc::unbounded_channel();
    let Err(error) = bus.subscribe(conflicting, Arc::new(Collect { tx: second_tx })) else {
        panic!("同名不同配置必须被拒绝");
    };
    assert!(matches!(error, MqError::SubscriptionConflict { .. }));

    handle.shutdown().await.expect("关闭订阅失败");
    bus.close().await.expect("关闭总线失败");
}

#[tokio::test]
async fn close_is_idempotent_and_rejects_new_work() {
    let cfg = MqConfig::default();
    let bus = mq::memory::MemoryBus::new(cfg.clone());
    let (tx, _rx) = mpsc::unbounded_channel();
    let handle = bus
        .subscribe(spec(&cfg, "consumer-a"), Arc::new(Collect { tx }))
        .expect("订阅失败");

    bus.close().await.expect("第一次关闭失败");
    bus.close().await.expect("重复关闭必须幂等");
    assert!(!bus.health().connected);

    let publish = bus.publisher().publish(envelope(1)).await;
    assert!(matches!(publish, Err(MqError::Closed)));

    let (second_tx, _second_rx) = mpsc::unbounded_channel();
    let subscribe = bus.subscribe(
        spec(&cfg, "consumer-b"),
        Arc::new(Collect { tx: second_tx }),
    );
    assert!(matches!(subscribe, Err(MqError::Closed)));

    handle.shutdown().await.expect("关闭订阅失败");
}

#[tokio::test]
async fn invalid_topic_is_rejected() {
    let cfg = MqConfig::default();
    let bus = mq::memory::MemoryBus::new(cfg.clone());
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut subscription = spec(&cfg, "consumer-a");
    subscription.topic = "Bad/Topic".to_string();

    let result = bus.subscribe(subscription, Arc::new(Collect { tx }));
    assert!(matches!(result, Err(MqError::InvalidTopic(_))));

    bus.close().await.expect("关闭总线失败");
}

#[tokio::test]
async fn health_reports_instance_and_scope() {
    let cfg = MqConfig {
        instance_id: "host-a-0".to_string(),
        ..MqConfig::default()
    };
    let bus = mq::memory::MemoryBus::new(cfg);
    let health = bus.health();
    assert!(!health.enabled);
    assert!(health.connected);
    assert_eq!(health.instance_id, "host-a-0");
    assert_eq!(health.subscription_scope, mq::SubscriptionScope::Instance);
    bus.close().await.expect("关闭总线失败");
}
