//! serde 辅助：`i64` ↔ 字符串
//!
//! 与 `database::models` 里的 `i64_to_string` / `opt_i64_to_string` 约定一致：
//! 前端 JS 的 Number 只有 53 位精度，Snowflake ID 与 epoch 毫秒都必须序列化成字符串，
//! 否则会被静默截断（见 docs/前端大整数精度丢失问题(Snowflake ID).md）。
//! 这里本地实现一份，避免 ops 反向依赖 database。

use serde::{Deserializer, Serializer, de};

/// 序列化为字符串
pub fn i64_to_string<S>(value: &i64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}

/// 从字符串（或数字）反序列化
pub fn i64_from_string<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(I64Visitor)
}

/// 可选值序列化为字符串（`None` → `null`）
pub fn opt_i64_to_string<S>(value: &Option<i64>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(value) => serializer.serialize_str(&value.to_string()),
        None => serializer.serialize_none(),
    }
}

/// 从字符串（或数字、null）反序列化可选值
pub fn opt_i64_from_string<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(OptI64Visitor)
}

struct I64Visitor;

impl<'de> de::Visitor<'de> for I64Visitor {
    type Value = i64;

    fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("i64 或字符串形式的 i64")
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(value)
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        i64::try_from(value).map_err(|_| E::custom(format!("数值 {value} 超出 i64 范围")))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        value
            .parse::<i64>()
            .map_err(|err| E::custom(format!("无法把 {value:?} 解析为 i64: {err}")))
    }
}

struct OptI64Visitor;

impl<'de> de::Visitor<'de> for OptI64Visitor {
    type Value = Option<i64>;

    fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("i64、字符串形式的 i64 或 null")
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(I64Visitor).map(Some)
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Some(value))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        I64Visitor.visit_u64(value).map(Some)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        I64Visitor.visit_str(value).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Sample {
        #[serde(serialize_with = "i64_to_string", deserialize_with = "i64_from_string")]
        id: i64,
        #[serde(
            serialize_with = "opt_i64_to_string",
            deserialize_with = "opt_i64_from_string"
        )]
        observed_at: Option<i64>,
    }

    #[test]
    fn i64_round_trips_as_string() {
        let value = Sample {
            id: 715_000_000_000_000_001,
            observed_at: Some(1_700_000_000_000),
        };
        let json = serde_json::to_string(&value).expect("序列化失败");
        assert_eq!(
            json,
            r#"{"id":"715000000000000001","observed_at":"1700000000000"}"#
        );
        let back: Sample = serde_json::from_str(&json).expect("反序列化失败");
        assert_eq!(back, value);
    }

    #[test]
    fn numbers_are_accepted_on_deserialize() {
        let parsed: Sample =
            serde_json::from_str(r#"{"id":42,"observed_at":7}"#).expect("反序列化失败");
        assert_eq!(
            parsed,
            Sample {
                id: 42,
                observed_at: Some(7)
            }
        );
    }

    #[test]
    fn null_maps_to_none() {
        let parsed: Sample =
            serde_json::from_str(r#"{"id":"42","observed_at":null}"#).expect("反序列化失败");
        assert_eq!(parsed.observed_at, None);
        assert_eq!(
            serde_json::to_string(&parsed).expect("序列化失败"),
            r#"{"id":"42","observed_at":null}"#
        );
    }

    #[test]
    fn invalid_string_is_rejected() {
        let err = serde_json::from_str::<Sample>(r#"{"id":"abc","observed_at":null}"#)
            .expect_err("非法数值应当报错");
        assert!(
            err.to_string().contains("i64"),
            "错误信息应说明期望类型: {err}"
        );
    }
}
