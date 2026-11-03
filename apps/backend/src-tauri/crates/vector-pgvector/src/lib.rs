//! PostgreSQL + pgvector 向量存储实现
//!
//! 本 crate 只做一件事：把 [`vector::VectorStore`] 落到 PostgreSQL + pgvector 上。
//! 驱动（`sqlx`）只在这里出现，`crates/vector` 与 `crates/ops` 都不感知。
//!
//! # 只读纪律
//! - 采集链路只走 [`vector::VectorStore::describe`]：全部是 `SELECT` + `pg_catalog`
//!   反射，零 DDL；
//! - `ensure_table` / `upsert` / `delete` 只在应用主动写入向量时调用；
//! - 采集角色只需 `CONNECT` + `USAGE` + `SELECT`，不需要建表 / 建扩展权限。
//!
//! # 与 sqlx 的约定
//! 本 crate 不启用 sqlx 的 `json` feature：向量与元数据在 SQL 里以 `::text`
//! 取出、以 `$n::vector` / `$n::jsonb` 绑定，既避免额外特性，也让写入只发一条
//! 多行 `INSERT ... ON CONFLICT DO UPDATE`（批量 1000 条/次）。
//!
//! ```bash
//! cargo test -p vector-pgvector
//! ```

pub mod reflect;
pub mod store;
pub mod url;

pub use reflect::{
    build_filter_sql, build_upsert_sql, dims_from_typmod, distance_from_ops,
    index_kind_from_access_method, index_params_from_ddl, is_vector_type, qualified_table,
    quote_ident, split_qualified, with_clause,
};
pub use store::{PgVectorStore, TableMeta};
pub use url::{connect_url, percent_encode, with_credentials};
