//! 向量数据存储抽象（与驱动无关）
//!
//! 本 crate 只定义**契约**，不含任何驱动依赖：
//! - [`store::VectorStore`]：统一的 ping / describe / ensure_table / upsert / delete /
//!   count / query 接口；
//! - [`config::StoreConfig`] / [`config::StoreAuth`]：连接配置，凭证 `Debug` 已掩码；
//! - [`model::TableSpec`] / [`model::VectorRecord`] / [`model::QueryRequest`] /
//!   [`model::SearchHit`]：数据模型；
//! - [`store::StoreDescription`] / [`store::TableDescription`]：零 DDL 反射结果，
//!   供运维采集直接落库展示。
//!
//! # 实现分布
//! - `vector-redis`：Redis / Valkey（RediSearch 向量集，`FT.CREATE` / `FT.SEARCH`）
//! - `vector-pgvector`：PostgreSQL + pgvector（SQL + `hnsw` / `ivfflat` 索引）
//! - `vector-lance`：LanceDB（本地目录或对象存储上的嵌入式副本）
//!
//! 三个实现 crate 各自持有自己的驱动版本，互不牵连，也不回灌到 `crates/ops`。
//!
//! ```bash
//! cargo test -p vector
//! ```
//!
//! # 只读纪律
//! `ping` / `describe` / `count` / `query` 只发读命令（`PING`、`SELECT 1`、
//! 目录列举、`SELECT` 查询），`ensure_table` / `upsert` / `delete` 只在应用
//! **主动写入向量**时调用；采集链路上永远不会触发 DDL 或写入。

pub mod config;
pub mod error;
pub mod model;
pub mod store;

pub use config::{DEFAULT_TIMEOUT_MS, MAX_TIMEOUT_MS, MIN_TIMEOUT_MS, StoreAuth, StoreConfig};
pub use error::{VectorError, VectorResult};
pub use model::{
    Backend, Distance, Filter, IndexKind, IndexSpec, MAX_BATCH, MAX_DIMENSIONS, MAX_TOP_K,
    QueryRequest, SearchHit, TableSpec, UpsertOutcome, VectorRecord, check_records, is_identifier,
    parse_vector_literal, validate_table_name, vector_literal,
};
pub use store::{
    IndexDescription, StoreCapabilities, StoreDescription, TableDescription, VectorStore, batches,
};

/// 装箱后的向量存储（便于在注册表 / 配置中按后端选择实现）
pub type BoxedVectorStore = Box<dyn VectorStore>;
