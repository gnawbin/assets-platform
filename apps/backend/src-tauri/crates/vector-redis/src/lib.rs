//! Redis / Valkey 向量集实现（RediSearch `FT.CREATE` / `FT.SEARCH`）
//!
//! 本 crate 只做一件事：把 [`vector::VectorStore`] 落到 Redis / Valkey 上。
//! 驱动（`redis`）只在这里出现，`crates/vector` 与 `crates/ops` 都不感知。
//!
//! # 存储形态
//! - 记录：HASH `{prefix}{table}:{id}`（原文 / 元数据 JSON / FLOAT32 向量二进制）；
//! - 索引：RediSearch 向量索引 `{prefix}{table}`；
//! - 元信息：侧车 HASH `{prefix}{table}:__meta__`（维度、度量、列名、索引类型）。
//!
//! # 只读纪律
//! 采集链路只走 [`vector::VectorStore::describe`]：`INFO` + `FT._LIST` + `HGETALL`
//! + `FT.SEARCH ... LIMIT 0 0`，全部是读命令；`FT.CREATE` / `HSET` / `DEL`
//! 只在应用主动写入向量时使用（采集账号无需这些权限）。
//!
//! # 已知限制
//! 元数据整体存成一个 JSON 文本字段，没有为每个字段声明 TAG，因此
//! [`vector::QueryRequest::filter`] 无法下推，会返回 `Unsupported`；
//! [`vector::StoreCapabilities::metadata_filter`] 如实上报 `false`。
//!
//! ```bash
//! cargo test -p vector-redis
//! ```

pub mod info;
pub mod store;
pub mod url;
pub mod value;

pub use info::{
    ClusterNode, InfoSections, KeyspaceDb, info_field, parse_cluster_nodes, parse_ft_info,
    parse_info, parse_keyspace,
};
pub use store::{RedisVectorStore, parse_vector_blob, record_id, vector_blob};
pub use url::{DEFAULT_PORT, connect_url, key_prefix, percent_encode};
pub use value::{RawHit, parse_search_reply};
