//! LanceDB 向量副本实现（嵌入式：本地目录 / 对象存储前缀）
//!
//! 本 crate 只做一件事：把 [`vector::VectorStore`] 落到 LanceDB 上。
//! 驱动（`lancedb` + `arrow`）只在这里出现，`crates/vector` 与 `crates/ops`
//! 都不感知；`crates/ops` 只依赖与协议无关的模型。
//!
//! # 定位
//! LanceDB 副本是**可重建**的向量副本（方案里 `vector_role=replica`），
//! 权威库是 PostgreSQL + pgvector（`vector_role=archive`）。因此这里：
//! - 不追求跨表事务（Lance 单表提交，失败可见）；
//! - 本地目录直接落盘，不需要独立服务；
//! - 表就是 Lance table，`ensure_table` = `create_empty_table`。
//!
//! # 只读纪律
//! 采集链路只走 [`vector::VectorStore::describe`]：`table_names` + `open_table`
//! + `schema` + `count_rows` + `list_indices`，全部是读操作。
//!
//! # 已知限制
//! - 元数据整体存成一个 JSON 文本列，只有主键 `id` 能下推过滤；
//!   [`vector::StoreCapabilities::metadata_filter`] 如实上报 `false`。
//! - 对象存储（`s3://` / `gs://` / `az://`）需要在根 `Cargo.toml` 为 `lancedb`
//!   打开对应 feature（`aws` / `gcs` / `azure`）；未开启时连接会报
//!   `Unsupported`，不会静默退化成空库。
//! - LanceDB Cloud（`db://`）需要 `remote` feature，本构建不支持。
//!
//! ```bash
//! cargo test -p vector-lance
//! ```

pub mod path;
pub mod schema;
pub mod store;

pub use path::{is_local, storage_options, store_uri};
pub use schema::{
    DISTANCE_COLUMN, ID_COLUMN, HitColumns, distance_from_type, distance_type, filter_sql,
    hits_from_batches, index_kind, records_batch, table_schema, vector_layout,
};
pub use store::LanceVectorStore;
