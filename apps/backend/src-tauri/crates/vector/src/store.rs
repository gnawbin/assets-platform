//! 向量存储 trait、能力矩阵与「描述」类型
//!
//! 三个实现（`vector-redis` / `vector-pgvector` / `vector-lance`）都实现
//! [`VectorStore`]。运维采集只需调用 [`VectorStore::describe`] 即可拿到
//! 表 / 维度 / 行数 / 索引，不必感知各后端差异。

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::StoreConfig;
use crate::error::VectorResult;
use crate::model::{
    Backend, Distance, IndexKind, MAX_BATCH, QueryRequest, SearchHit, TableSpec, UpsertOutcome,
    VectorRecord,
};

/// 后端能力矩阵（界面据此灰掉不支持的操作，避免用户白填参数）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreCapabilities {
    /// 支持 ANN 索引（HNSW / IVF 等）
    pub ann_index: bool,
    /// 支持元数据过滤下推
    pub metadata_filter: bool,
    /// 支持事务（写入失败可整体回滚）
    pub transactions: bool,
    /// 嵌入式存储（数据在本地目录，不需要独立服务）
    pub embedded: bool,
    /// 支持服务端 DDL（`CREATE TABLE` / `FT.CREATE`）
    pub server_side_ddl: bool,
}

impl StoreCapabilities {
    /// 各后端的能力默认值
    pub fn for_backend(backend: Backend) -> Self {
        match backend {
            // RediSearch 提供向量索引与 TAG/NUMERIC 过滤，但单条命令无事务语义
            Backend::Redis => Self {
                ann_index: true,
                metadata_filter: true,
                transactions: false,
                embedded: false,
                server_side_ddl: true,
            },
            // pgvector 拥有完整的事务与 SQL DDL
            Backend::PgVector => Self {
                ann_index: true,
                metadata_filter: true,
                transactions: true,
                embedded: false,
                server_side_ddl: true,
            },
            // LanceDB 是嵌入式（本地目录 / 对象存储），建表不走 DDL
            Backend::Lance => Self {
                ann_index: true,
                metadata_filter: true,
                transactions: false,
                embedded: true,
                server_side_ddl: false,
            },
        }
    }

    /// 人类可读的能力摘要
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        parts.push(if self.ann_index {
            "ANN 索引"
        } else {
            "仅暴力检索"
        });
        if self.metadata_filter {
            parts.push("元数据过滤");
        }
        if self.transactions {
            parts.push("事务");
        }
        if self.embedded {
            parts.push("嵌入式");
        }
        if self.server_side_ddl {
            parts.push("服务端建表");
        }
        parts.join(" · ")
    }
}

/// 索引描述
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexDescription {
    /// 索引名
    pub name: String,
    /// 索引类型（后端无法识别时为 `None`）
    pub kind: Option<IndexKind>,
    /// 距离度量
    pub distance: Option<Distance>,
    /// 原始参数（`m`、`lists`、`num_partitions` 等）
    pub parameters: BTreeMap<String, String>,
}

impl IndexDescription {
    /// 以索引名构造
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// 设置索引类型
    pub fn with_kind(mut self, kind: IndexKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// 设置距离度量
    pub fn with_distance(mut self, distance: Distance) -> Self {
        self.distance = Some(distance);
        self
    }

    /// 追加一个原始参数
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.parameters.insert(key.into(), value.into());
        self
    }
}

/// 表描述：一次 `describe` 的产出单元
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableDescription {
    /// 表名（pgvector 为 `schema.table`）
    pub name: String,
    /// 向量维度（无法推断时为 `None`）
    pub dimensions: Option<usize>,
    /// 行数（后端不提供精确值时为 `None`）
    pub row_count: Option<u64>,
    /// 距离度量
    pub distance: Option<Distance>,
    /// 向量列名
    pub vector_column: Option<String>,
    /// 该表上的索引
    pub indices: Vec<IndexDescription>,
}

impl TableDescription {
    /// 以表名构造
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// 设置维度
    pub fn with_dimensions(mut self, dimensions: usize) -> Self {
        self.dimensions = Some(dimensions);
        self
    }

    /// 设置行数
    pub fn with_row_count(mut self, row_count: u64) -> Self {
        self.row_count = Some(row_count);
        self
    }

    /// 设置距离度量
    pub fn with_distance(mut self, distance: Distance) -> Self {
        self.distance = Some(distance);
        self
    }

    /// 设置向量列名
    pub fn with_vector_column(mut self, column: impl Into<String>) -> Self {
        self.vector_column = Some(column.into());
        self
    }

    /// 追加一个索引
    pub fn with_index(mut self, index: IndexDescription) -> Self {
        self.indices.push(index);
        self
    }

    /// 是否已建 ANN 索引（`Flat` 不算）
    pub fn has_ann_index(&self) -> bool {
        self.indices
            .iter()
            .any(|index| matches!(index.kind, Some(kind) if kind != IndexKind::Flat))
    }
}

/// 存储描述：`describe` 的完整结果
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreDescription {
    /// 后端类型
    pub backend: Backend,
    /// 服务端版本（LanceDB 等无版本概念时为 `None`）
    pub server_version: Option<String>,
    /// 表清单
    pub tables: Vec<TableDescription>,
    /// 采集期发现的告警（配置风险、能力缺失等）
    pub warnings: Vec<String>,
}

impl StoreDescription {
    /// 以空表清单构造
    pub fn new(backend: Backend) -> Self {
        Self {
            backend,
            server_version: None,
            tables: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// 设置服务端版本
    pub fn with_server_version(mut self, version: impl Into<String>) -> Self {
        self.server_version = Some(version.into());
        self
    }

    /// 追加一张表
    pub fn with_table(mut self, table: TableDescription) -> Self {
        self.tables.push(table);
        self
    }

    /// 追加一条告警
    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }

    /// 全部表的向量总行数（缺行数的表按 0 计）
    pub fn total_rows(&self) -> u64 {
        self.tables
            .iter()
            .map(|table| table.row_count.unwrap_or(0))
            .sum()
    }
}

/// 把一批记录按 [`MAX_BATCH`] 切片，避免单次请求过大
pub fn batches<T>(items: &[T]) -> impl Iterator<Item = &[T]> {
    items.chunks(MAX_BATCH)
}

/// 向量存储统一接口
///
/// 只读方法（`ping` / `describe` / `count` / `query`）对采集与检索都安全；
/// 写方法（`ensure_table` / `upsert` / `delete`）只在应用主动写入向量时使用。
#[async_trait]
pub trait VectorStore: Send + Sync + std::fmt::Debug {
    /// 后端类型
    fn backend(&self) -> Backend;

    /// 连接配置（`Debug` 已掩码，可安全用于日志与审计）
    fn config(&self) -> &StoreConfig;

    /// 后端能力（默认取 [`StoreCapabilities::for_backend`]）
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities::for_backend(self.backend())
    }

    /// 连通性探测（只读：`PING` / `SELECT 1` / 打开目录）
    async fn ping(&self) -> VectorResult<()>;

    /// 反射表 / 维度 / 行数 / 索引（运维采集的唯一入口，零 DDL）
    async fn describe(&self) -> VectorResult<StoreDescription>;

    /// 确保表存在且结构匹配（幂等；已存在且结构一致时为空操作）
    async fn ensure_table(&self, spec: &TableSpec) -> VectorResult<()>;

    /// 批量写入（同主键覆盖）
    async fn upsert(
        &self,
        spec: &TableSpec,
        records: &[VectorRecord],
    ) -> VectorResult<UpsertOutcome>;

    /// 按主键删除，返回受影响行数
    async fn delete(&self, table: &str, ids: &[String]) -> VectorResult<u64>;

    /// 表行数
    async fn count(&self, table: &str) -> VectorResult<u64>;

    /// 相似度检索
    async fn query(&self, table: &str, request: &QueryRequest) -> VectorResult<Vec<SearchHit>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_match_backend_traits() {
        let redis = StoreCapabilities::for_backend(Backend::Redis);
        assert!(redis.ann_index && !redis.transactions && !redis.embedded);
        assert!(redis.server_side_ddl);
        assert!(redis.label().contains("ANN 索引"));

        let pg = StoreCapabilities::for_backend(Backend::PgVector);
        assert!(pg.transactions && pg.server_side_ddl && !pg.embedded);

        let lance = StoreCapabilities::for_backend(Backend::Lance);
        assert!(lance.embedded && !lance.server_side_ddl);
        assert!(lance.label().contains("嵌入式"));
    }

    #[test]
    fn table_description_helpers_behave() {
        let table = TableDescription::new("public.chunks")
            .with_dimensions(1024)
            .with_row_count(42)
            .with_distance(Distance::Cosine)
            .with_vector_column("embedding");
        assert!(!table.has_ann_index());
        assert_eq!(table.dimensions, Some(1024));

        let table = table.with_index(
            IndexDescription::new("chunks_embedding_idx")
                .with_kind(IndexKind::Hnsw)
                .with_distance(Distance::Cosine)
                .with_param("m", "16"),
        );
        assert!(table.has_ann_index());
        assert_eq!(
            table.indices[0].parameters.get("m"),
            Some(&"16".to_string())
        );

        let flat_only = TableDescription::new("small")
            .with_index(IndexDescription::new("small_flat").with_kind(IndexKind::Flat));
        assert!(!flat_only.has_ann_index(), "Flat 不算 ANN 索引");
    }

    #[test]
    fn store_description_aggregates_rows_and_warnings() {
        let description = StoreDescription::new(Backend::Lance)
            .with_server_version("0.41.0")
            .with_table(TableDescription::new("chunks").with_row_count(10))
            .with_table(TableDescription::new("docs"))
            .with_warning("未建索引，查询将退化为暴力扫描");
        assert_eq!(description.total_rows(), 10);
        assert_eq!(description.warnings.len(), 1);
        assert_eq!(description.backend, Backend::Lance);
    }

    #[test]
    fn batches_split_by_limit() {
        let records: Vec<u16> = (0..(MAX_BATCH as u16 + 5)).collect();
        let sizes: Vec<usize> = batches(&records).map(<[u16]>::len).collect();
        assert_eq!(sizes, vec![MAX_BATCH, 5]);
    }
}