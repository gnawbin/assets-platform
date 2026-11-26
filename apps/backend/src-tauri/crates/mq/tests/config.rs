//! 配置与命名契约测试（无 broker，不需要环境变量）。

use mq::{MqConfig, SubscriptionScope};

fn config() -> MqConfig {
    MqConfig {
        instance_id: "host-a-0".to_string(),
        ..MqConfig::default()
    }
}

#[test]
fn instance_scope_subscription_contains_instance_id() {
    let cfg = config();
    assert_eq!(
        cfg.subscription_name("graph-replicate").unwrap(),
        "assets-host-a-0-graph-replicate"
    );
}

#[test]
fn shared_scope_subscription_ignores_instance_id() {
    let cfg = MqConfig {
        subscription_scope: SubscriptionScope::Shared,
        ..config()
    };
    assert_eq!(
        cfg.subscription_name("graph-replicate").unwrap(),
        "assets-graph-replicate"
    );
}

#[test]
fn purpose_is_validated() {
    let cfg = config();
    for purpose in ["", "Graph", "a/b", "a b", "a;b"] {
        assert!(
            cfg.subscription_name(purpose).is_err(),
            "purpose 应被拒绝：{purpose}"
        );
    }
}

#[test]
fn producer_name_is_unique_per_topic_and_bus() {
    let cfg = config();
    let topic_a = "persistent://public/default/tg.a.graph.0";
    let topic_b = "persistent://public/default/tg.a.graph.1";

    let first = cfg.producer_name(topic_a, 1);
    let same = cfg.producer_name(topic_a, 1);
    let other_topic = cfg.producer_name(topic_b, 1);
    let other_bus = cfg.producer_name(topic_a, 2);

    assert_eq!(first, same);
    assert_ne!(first, other_topic);
    assert_ne!(first, other_bus);
    assert!(first.contains("host-a-0"));
    assert!(first.contains(&std::process::id().to_string()));
}

#[test]
fn debug_redacts_token() {
    let cfg = MqConfig {
        token: Some("super-secret".to_string()),
        ..config()
    };
    let rendered = format!("{cfg:?}");
    assert!(!rendered.contains("super-secret"));
    assert!(rendered.contains("<redacted>"));
}
