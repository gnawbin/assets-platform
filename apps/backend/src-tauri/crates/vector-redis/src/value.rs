//! RESP 回复的取值helper
//!
//! RediSearch 的 `FT.SEARCH` / `FT.INFO` 回复是异质数组（整数 + 键 + 字段数组），
//! 无法直接反序列化成 `Vec<String>`，因此这里做一层薄薄的取值封装，
//! 让 store 层不必到处都是 `match Value`。

use redis::Value;

/// 把标量回复取成字符串（整数 / 简单字符串 / 二进制串）
pub fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::Int(number) => Some(number.to_string()),
        Value::SimpleString(text) => Some(text.clone()),
        Value::BulkString(bytes) => Some(String::from_utf8_lossy(bytes).to_string()),
        Value::VerbatimString { text, .. } => Some(text.clone()),
        Value::Okay => Some("OK".to_string()),
        _ => None,
    }
}

/// 取数组元素；非数组返回 `None`
pub fn sequence(value: &Value) -> Option<&[Value]> {
    match value {
        Value::Array(items) => Some(items),
        _ => None,
    }
}

/// 取二进制内容（向量 blob 等）；非二进制返回 `None`
pub fn bytes(value: &Value) -> Option<&[u8]> {
    match value {
        Value::BulkString(bytes) => Some(bytes),
        _ => None,
    }
}

/// 把「名值交替」的扁平数组拍平成 `(name, Option<value>)` 列表
///
/// 嵌套值（`FT.INFO` 的 `attributes` 等）记为 `None`，由调用方跳过。
pub fn name_value_pairs(items: &[Value]) -> Vec<(String, Option<String>)> {
    let mut pairs = Vec::with_capacity(items.len() / 2);
    let mut index = 0;
    while index < items.len() {
        let Some(name) = scalar(&items[index]) else {
            index += 1;
            continue;
        };
        let value = items.get(index + 1).and_then(scalar);
        pairs.push((name, value));
        index += 2;
    }
    pairs
}

/// `FT.SEARCH` 回复里的一条命中
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawHit {
    /// Redis key（形如 `chunks:doc-1`）
    pub key: String,
    /// 字段名 → 值（`content` / `metadata` / `score` …）
    pub fields: Vec<(String, String)>,
}

impl RawHit {
    /// 取字段（大小写不敏感）
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// 解析 `FT.SEARCH` 回复：`[total, key1, [f, v, …], key2, […], …]`
pub fn parse_search_reply(reply: &Value) -> (u64, Vec<RawHit>) {
    let Some(items) = sequence(reply) else {
        return (0, Vec::new());
    };
    let total = items.first().and_then(|value| scalar(value)).and_then(|text| text.parse::<u64>().ok()).unwrap_or(0);
    let mut hits = Vec::new();
    let mut index = 1;
    while index < items.len() {
        let Some(key) = scalar(&items[index]) else {
            index += 1;
            continue;
        };
        let mut fields = Vec::new();
        if let Some(entries) = items.get(index + 1).and_then(sequence) {
            let mut position = 0;
            while position + 1 < entries.len() {
                if let (Some(name), Some(value)) = (scalar(&entries[position]), scalar(&entries[position + 1])) {
                    fields.push((name, value));
                }
                position += 2;
            }
        }
        hits.push(RawHit { key, fields });
        index += 2;
    }
    (total, hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bulk(text: &str) -> Value {
        Value::BulkString(text.as_bytes().to_vec())
    }

    #[test]
    fn scalar_and_sequence_cover_scalars() {
        assert_eq!(scalar(&Value::Int(7)), Some("7".to_string()));
        assert_eq!(scalar(&bulk("abc")), Some("abc".to_string()));
        assert_eq!(scalar(&Value::SimpleString("OK".to_string())), Some("OK".to_string()));
        assert_eq!(scalar(&Value::Nil), None);
        assert!(sequence(&Value::Int(1)).is_none());
        assert_eq!(sequence(&Value::Array(vec![Value::Int(1)])).map(<[Value]>::len), Some(1));
    }

    #[test]
    fn name_value_pairs_skip_nested() {
        let items = vec![
            bulk("index_name"),
            bulk("chunks_idx"),
            bulk("attributes"),
            Value::Array(vec![bulk("embedding")]),
            bulk("num_docs"),
            bulk("42"),
        ];
        let pairs = name_value_pairs(&items);
        assert_eq!(pairs[0], ("index_name".to_string(), Some("chunks_idx".to_string())));
        assert_eq!(pairs[1], ("attributes".to_string(), None));
        assert_eq!(pairs[2], ("num_docs".to_string(), Some("42".to_string())));
    }

    #[test]
    fn search_reply_is_parsed() {
        let reply = Value::Array(vec![
            Value::Int(2),
            bulk("chunks:doc-1"),
            Value::Array(vec![bulk("content"), bulk("hello"), bulk("score"), bulk("0.125")]),
            bulk("chunks:doc-2"),
            Value::Array(vec![bulk("content"), bulk("world")]),
        ]);
        let (total, hits) = parse_search_reply(&reply);
        assert_eq!(total, 2);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].key, "chunks:doc-1");
        assert_eq!(hits[0].field("score"), Some("0.125"));
        assert_eq!(hits[1].field("content"), Some("world"));
        assert_eq!(hits[1].field("score"), None);

        // 畸形回复：空数组 / 只有总数
        assert_eq!(parse_search_reply(&Value::Array(vec![])), (0, Vec::new()));
        assert_eq!(parse_search_reply(&Value::Array(vec![Value::Int(0)])).0, 0);
        assert_eq!(parse_search_reply(&Value::Nil), (0, Vec::new()));
    }
}
