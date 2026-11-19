//! 信封 / topic 契约测试（无 broker）。

use mq::{Envelope, TopicName};
use serde_json::json;

#[test]
fn envelope_roundtrip_preserves_partition_key() {
    let mut envelope = Envelope::new(
        "snowflake-1",
        "tg.cluster.graph.0",
        "graph.node.upsert",
        json!({"node_id": "n-1"}),
    );
    envelope.partition_key = Some("n-1".to_string());
    envelope.tenant_id = Some(7);
    envelope.seq = Some(3);
    envelope.trace_id = Some("trace-1".to_string());

    let raw = serde_json::to_string(&envelope).expect("序列化失败");
    let decoded: Envelope = serde_json::from_str(&raw).expect("反序列化失败");
    assert_eq!(decoded, envelope);
    assert_eq!(decoded.partition_key.as_deref(), Some("n-1"));
}

#[test]
fn envelope_without_partition_key_is_backward_compatible() {
    let raw = r#"{
        "id": "1",
        "topic": "tg.cluster.graph.0",
        "kind": "graph.node.upsert",
        "tenant_id": null,
        "seq": null,
        "created_at": 0,
        "trace_id": null,
        "payload": {}
    }"#;

    let envelope: Envelope = serde_json::from_str(raw).expect("旧版本信封应可解析");
    assert_eq!(envelope.partition_key, None);
    assert_eq!(envelope.schema_version, 1);
}

#[test]
fn attributes_mirror_trace_tenant_and_schema_version() {
    let mut envelope = Envelope::new("1", "tg.a", "k", json!({}));
    envelope.tenant_id = Some(42);
    envelope.trace_id = Some("trace".to_string());

    let attributes = envelope.attributes();
    assert_eq!(attributes.get("tenant_id").map(String::as_str), Some("42"));
    assert_eq!(
        attributes.get("trace_id").map(String::as_str),
        Some("trace")
    );
    assert_eq!(
        attributes.get("schema_version").map(String::as_str),
        Some("1")
    );
    assert!(!attributes.contains_key("partition_key"));
}

#[test]
fn topic_name_build_and_parse_roundtrip() {
    let uri = TopicName::build("public", "default", "tg.cluster.graph.0").expect("build 失败");
    assert_eq!(uri, "persistent://public/default/tg.cluster.graph.0");

    let parsed = TopicName::parse(&uri).expect("parse 失败");
    assert_eq!(parsed.tenant, "public");
    assert_eq!(parsed.namespace, "default");
    assert_eq!(parsed.name, "tg.cluster.graph.0");
    assert_eq!(parsed.to_uri(), uri);
}

#[test]
fn topic_name_rejects_invalid_inputs() {
    let invalid = [
        ("public", "default", ""),
        ("public", "default", "Graph"),
        ("public", "default", "a/b"),
        ("public", "default", "a b"),
        ("public", "default", "a;b"),
        ("", "default", "a"),
        ("public", "", "a"),
    ];
    for (tenant, namespace, logical) in invalid {
        assert!(
            TopicName::build(tenant, namespace, logical).is_err(),
            "应拒绝：{tenant}/{namespace}/{logical}"
        );
    }

    let too_long = "a".repeat(201);
    assert!(TopicName::build("public", "default", &too_long).is_err());

    let invalid_uris = [
        "",
        "tg.cluster.graph.0",
        "non-persistent://public/default/tg",
        "persistent://public/default",
        "persistent://public/default/",
        "persistent://public//tg",
        "persistent://public/default/tg/extra",
        "persistent://public/default/Tg",
        "persistent://public/default/tg;bad",
    ];
    for uri in invalid_uris {
        assert!(TopicName::parse(uri).is_err(), "应拒绝：{uri}");
    }
}
