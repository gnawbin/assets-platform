//! LanceDB 的 [`VectorStore`] 实现
//!
//! LanceDB 是嵌入式向量库：没有常驻服务、没有 DDL，「表」就是目录下的一张
//! Lance table，因此 `ensure_table` = `create_empty_table`。
//!
//! 只读纪律：`ping` / `describe` / `count` / `query` 只读元数据与数据文件；
//! `ensure_table` / `upsert` / `delete` 只在应用主动写入副本时调用。
//!
//! 本地目录不存在时由 lancedb 自行创建（与打开 SQLite 文件一致），
//! 因此采集侧应只对已存在的副本调用 `describe`。

use async_trait::async_trait;
use futures::TryStreamExt;
use lancedb::Connection;
use lancedb::arrow::arrow_array::RecordBatch;
use lancedb::arrow::arrow_schema::Schema;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use lancedb::table::Table;
use vector::{
    Backend, Distance, Filter, IndexDescription, QueryRequest, SearchHit, StoreCapabilities,
    StoreConfig, StoreDescription, TableDescription, TableSpec, UpsertOutcome, VectorError,
    VectorRecord, VectorResult, VectorStore, batches, check_records,
};

use crate::path::{is_local, storage_options, store_uri};
use crate::schema::{
    HitColumns, ID_COLUMN, distance_from_type, distance_type, filter_sql, hits_from_batches,
    index_kind, records_reader, table_schema, vector_layout,
};

/// `describe` 时最多精确统计多少张表的行数（`count_rows` 要扫元数据）
const MAX_COUNTED_TABLES: usize = 50;

/// LanceDB 向量副本
#[derive(Debug, Clone)]
pub struct LanceVectorStore {
    config: StoreConfig,
    conn: Connection,
    uri: String,
}

impl LanceVectorStore {
    /// 打开数据库目录（本地目录 / 对象存储前缀）
    pub async fn connect(config: StoreConfig) -> VectorResult<Self> {
        let uri = store_uri(&config)?;
        let options = storage_options(&config.auth)?;
        let conn = lancedb::connect(&uri)
            .storage_options(options)
            .execute()
            .await
            .map_err(|error| map_lance(error, "打开 LanceDB"))?;
        Ok(Self { config, conn, uri })
    }

    /// 复用已有连接（服务层自己管连接时使用）
    pub fn from_connection(config: StoreConfig, conn: Connection, uri: impl Into<String>) -> Self {
        Self {
            config,
            conn,
            uri: uri.into(),
        }
    }

    /// 归一化后的数据库 URI
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// 是否本地目录（嵌入式）
    pub fn is_local(&self) -> bool {
        is_local(&self.uri)
    }

    /// 打开一张表
    async fn open(&self, table: &str) -> VectorResult<Table> {
        vector::validate_table_name(table)?;
        self.conn
            .open_table(table)
            .execute()
            .await
            .map_err(|error| map_lance(error, &format!("打开表 {table}")))
    }

    /// 表的向量列与维度
    async fn layout(&self, table: &Table) -> VectorResult<(String, usize)> {
        let schema = table
            .schema()
            .await
            .map_err(|error| map_lance(error, "读取表结构"))?;
        vector_layout(&schema).ok_or_else(|| {
            VectorError::schema("这张表没有 FixedSizeList<float> 向量列，不是向量表".to_string())
        })
    }

    /// 表上向量索引使用的距离度量（没有索引时按余弦，与 `ensure_table` 的默认一致）
    async fn metric(&self, table: &Table, vector_column: &str) -> Distance {
        let Ok(indices) = table.list_indices().await else {
            return Distance::Cosine;
        };
        for config in indices {
            let Some(metric) = config.metric_type else {
                continue;
            };
            let on_column = config.columns.is_empty()
                || config.columns.iter().any(|column| column == vector_column);
            if on_column
                && let Some(distance) = distance_from_type(metric)
            {
                return distance;
            }
        }
        Distance::Cosine
    }
}

/// 从表结构里猜原文列 / 元数据列（与本 crate 建表时的默认列名一致）
fn pick_columns(schema: &Schema) -> (Option<String>, Option<String>) {
    /// 可能存放原文的列名（按优先级）
    const TEXT_CANDIDATES: [&str; 2] = ["content", "text"];
    /// 可能存放元数据的列名
    const METADATA_CANDIDATES: [&str; 1] = ["metadata"];
    let first = |candidates: &[&str]| {
        candidates
            .iter()
            .find(|name| schema.field_with_name(name).is_ok())
            .map(|name| (*name).to_string())
    };
    (first(&TEXT_CANDIDATES), first(&METADATA_CANDIDATES))
}

#[async_trait]
impl VectorStore for LanceVectorStore {
    fn backend(&self) -> Backend {
        Backend::Lance
    }

    fn config(&self) -> &StoreConfig {
        &self.config
    }

    fn capabilities(&self) -> StoreCapabilities {
        // 元数据整体存成一个 JSON 文本列，没有为每个字段建独立列：
        // 只有主键 id 能下推，因此如实上报「不支持元数据过滤」。
        StoreCapabilities {
            metadata_filter: false,
            ..StoreCapabilities::for_backend(Backend::Lance)
        }
    }

    async fn ping(&self) -> VectorResult<()> {
        self.conn
            .table_names()
            .execute()
            .await
            .map_err(|error| map_lance(error, "列出表"))?;
        Ok(())
    }

    async fn describe(&self) -> VectorResult<StoreDescription> {
        let mut description = StoreDescription::new(Backend::Lance)
            .with_server_version(if self.is_local() { "embedded" } else { "object-store" });
        let names = self
            .conn
            .table_names()
            .execute()
            .await
            .map_err(|error| map_lance(error, "列出表"))?;

        for (position, name) in names.into_iter().enumerate() {
            let mut table_description = TableDescription::new(name.clone());
            match self.conn.open_table(&name).execute().await {
                Ok(table) => {
                    match table.schema().await {
                        Ok(schema) => match vector_layout(&schema) {
                            Some((column, dimensions)) => {
                                table_description = table_description
                                    .with_vector_column(column)
                                    .with_dimensions(dimensions);
                            }
                            None => {
                                description = description
                                    .with_warning(format!("表 {name} 没有向量列，已跳过其向量信息"));
                            }
                        },
                        Err(error) => {
                            description = description.with_warning(format!(
                                "读取表 {name} 的结构失败：{}",
                                map_lance(error, "读取表结构")
                            ));
                        }
                    }
                    if position < MAX_COUNTED_TABLES {
                        match table.count_rows(None).await {
                            Ok(count) => {
                                table_description = table_description.with_row_count(count as u64);
                            }
                            Err(error) => {
                                description = description.with_warning(format!(
                                    "统计表 {name} 行数失败：{}",
                                    map_lance(error, "统计行数")
                                ));
                            }
                        }
                    }
                    match table.list_indices().await {
                        Ok(indices) => {
                            for config in indices {
                                let Some(kind) = index_kind(&config.index_type) else {
                                    continue;
                                };
                                let mut index =
                                    IndexDescription::new(config.name.clone()).with_kind(kind);
                                if let Some(metric) = config.metric_type
                                    && let Some(distance) = distance_from_type(metric)
                                {
                                    index = index.with_distance(distance);
                                }
                                for column in &config.columns {
                                    index = index.with_param("column", column.clone());
                                }
                                table_description = table_description.with_index(index);
                            }
                        }
                        Err(error) => {
                            description = description.with_warning(format!(
                                "读取表 {name} 的索引失败：{}",
                                map_lance(error, "读取索引")
                            ));
                        }
                    }
                }
                Err(error) => {
                    description = description.with_warning(format!(
                        "打开表 {name} 失败：{}",
                        map_lance(error, "打开表")
                    ));
                }
            }
            description = description.with_table(table_description);
        }
        Ok(description)
    }

    async fn ensure_table(&self, spec: &TableSpec) -> VectorResult<()> {
        spec.validate()?;
        if let Ok(table) = self.open(&spec.name).await {
            let (column, dimensions) = self.layout(&table).await?;
            if dimensions != spec.dimensions {
                return Err(VectorError::schema(format!(
                    "表 {} 的向量列 {column} 已是 {dimensions} 维，与请求的 {} 维不一致（Lance 只能重建表）",
                    spec.name, spec.dimensions
                )));
            }
            return Ok(());
        }
        let schema = table_schema(spec)?;
        self.conn
            .create_empty_table(&spec.name, schema)
            .execute()
            .await
            .map_err(|error| map_lance(error, &format!("创建表 {}", spec.name)))?;
        Ok(())
    }

    async fn upsert(
        &self,
        spec: &TableSpec,
        records: &[VectorRecord],
    ) -> VectorResult<UpsertOutcome> {
        check_records(spec, records)?;
        let table = self.open(&spec.name).await?;
        let mut affected = 0_u64;
        let mut batch_count = 0_usize;
        for chunk in batches(records) {
            batch_count += 1;
            let mut merge = table.merge_insert(&[ID_COLUMN]);
            merge
                .when_matched_update_all(None)
                .when_not_matched_insert_all();
            let result = merge
                .execute(records_reader(spec, chunk)?)
                .await
                .map_err(|error| map_lance(error, "写入向量"))?;
            // 常规路径有插入/更新明细；RTE 路径只给总行数
            affected += if result.num_rows > 0 {
                result.num_rows
            } else {
                result.num_inserted_rows + result.num_updated_rows
            };
        }
        Ok(UpsertOutcome::new(affected, batch_count))
    }

    async fn delete(&self, table: &str, ids: &[String]) -> VectorResult<u64> {
        if ids.is_empty() {
            return Ok(0);
        }
        let table = self.open(table).await?;
        // 复用过滤构造：单引号转义与 IN 语法与检索过滤保持一致
        let predicate = filter_sql(&Filter::any_of(ID_COLUMN, ids.to_vec()))?;
        let result = table
            .delete(predicate)
            .await
            .map_err(|error| map_lance(error, "删除向量"))?;
        Ok(result.num_deleted_rows)
    }

    async fn count(&self, table: &str) -> VectorResult<u64> {
        let table = self.open(table).await?;
        let rows = table
            .count_rows(None)
            .await
            .map_err(|error| map_lance(error, "统计行数"))?;
        Ok(rows as u64)
    }

    async fn query(&self, table: &str, request: &QueryRequest) -> VectorResult<Vec<SearchHit>> {
        let table = self.open(table).await?;
        let schema = table
            .schema()
            .await
            .map_err(|error| map_lance(error, "读取表结构"))?;
        let (vector_column, dimensions) = vector_layout(&schema).ok_or_else(|| {
            VectorError::schema("这张表没有 FixedSizeList<float> 向量列，不是向量表".to_string())
        })?;
        request.validate(dimensions)?;

        let metric = self.metric(&table, &vector_column).await;
        let (text_column, metadata_column) = pick_columns(&schema);
        let mut select: Vec<String> = vec![ID_COLUMN.to_string()];
        if let Some(column) = &text_column {
            select.push(column.clone());
        }
        if let Some(column) = &metadata_column {
            select.push(column.clone());
        }
        if request.include_vector {
            select.push(vector_column.clone());
        }

        let mut query = table
            .query()
            .nearest_to(request.vector.as_slice())
            .map_err(|error| map_lance(error, "构造向量检索"))?
            .column(&vector_column)
            .distance_type(distance_type(metric))
            .limit(request.top_k)
            .select(Select::columns(&select));
        if let Some(filter) = &request.filter {
            query = query.only_if(filter_sql(filter)?);
        }
        let stream = query
            .execute()
            .await
            .map_err(|error| map_lance(error, "向量检索"))?;
        let batches: Vec<RecordBatch> = stream.try_collect().await.map_err(|error| {
            VectorError::transport(format!("读取 LanceDB 检索结果：{error}"))
        })?;
        Ok(hits_from_batches(
            &batches,
            HitColumns {
                text: text_column.as_deref().unwrap_or_default(),
                metadata: metadata_column.as_deref().unwrap_or_default(),
                vector: request.include_vector.then_some(vector_column.as_str()),
            },
            metric,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// 每个用例独占一个临时目录（LanceDB 是嵌入式，直接开目录即可）
    fn temp_dir() -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        std::env::temp_dir().join(format!("vector-lance-{}-{stamp}", std::process::id()))
    }

    async fn open(dir: &PathBuf) -> LanceVectorStore {
        LanceVectorStore::connect(StoreConfig::new(dir.to_string_lossy().to_string()))
            .await
            .expect("打开本地 LanceDB 失败")
    }

    #[tokio::test]
    async fn empty_directory_reports_no_tables() {
        let dir = temp_dir();
        let store = open(&dir).await;
        assert!(store.is_local());
        store.ping().await.unwrap();
        let description = store.describe().await.unwrap();
        assert!(description.tables.is_empty());
        assert_eq!(description.backend, Backend::Lance);

        // 没有这张表
        assert_eq!(
            store.count("chunks").await.unwrap_err().kind(),
            "table_not_found"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn ensure_upsert_query_delete_round_trip() {
        let dir = temp_dir();
        let store = open(&dir).await;
        let spec = TableSpec::new("chunks", 3);

        // 建表 + 幂等 + 维度冲突
        store.ensure_table(&spec).await.unwrap();
        store.ensure_table(&spec).await.unwrap();
        assert_eq!(
            store
                .ensure_table(&TableSpec::new("chunks", 4))
                .await
                .unwrap_err()
                .kind(),
            "schema"
        );

        // 写入（含原文与元数据）
        let records = vec![
            VectorRecord::new("a", vec![1.0, 0.0, 0.0])
                .with_text("alpha")
                .with_metadata("source", "wiki"),
            VectorRecord::new("b", vec![0.0, 1.0, 0.0]).with_text("beta"),
        ];
        let outcome = store.upsert(&spec, &records).await.unwrap();
        assert_eq!(outcome.affected, 2);
        assert_eq!(outcome.batches, 1);
        assert_eq!(store.count("chunks").await.unwrap(), 2);

        // 同主键覆盖
        store
            .upsert(
                &spec,
                &[VectorRecord::new("a", vec![0.5, 0.5, 0.0]).with_text("alpha2")],
            )
            .await
            .unwrap();
        assert_eq!(store.count("chunks").await.unwrap(), 2);

        // 检索：最相近的排前面
        let hits = store
            .query("chunks", &QueryRequest::new(vec![1.0, 0.0, 0.0], 2))
            .await
            .unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].id, "a");
        assert!(hits[0].score >= hits[1].score);
        assert_eq!(hits[0].text.as_deref(), Some("alpha2"));

        // 主键过滤下推 + 回传向量
        let request = QueryRequest::new(vec![1.0, 0.0, 0.0], 5)
            .with_filter(Filter::eq("id", "b"))
            .with_vector_in_result();
        let hits = store.query("chunks", &request).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "b");
        assert_eq!(hits[0].vector.as_ref().map(Vec::len), Some(3));

        // 元数据字段无法下推
        let request = QueryRequest::new(vec![1.0, 0.0, 0.0], 2)
            .with_filter(Filter::eq("source", "wiki"));
        assert_eq!(
            store.query("chunks", &request).await.unwrap_err().kind(),
            "unsupported"
        );

        // describe 反射
        let description = store.describe().await.unwrap();
        assert_eq!(description.tables.len(), 1);
        assert_eq!(description.tables[0].name, "chunks");
        assert_eq!(description.tables[0].dimensions, Some(3));
        assert_eq!(description.tables[0].row_count, Some(2));
        assert_eq!(description.tables[0].vector_column.as_deref(), Some("embedding"));

        // 删除
        assert_eq!(
            store.delete("chunks", &["a".to_string()]).await.unwrap(),
            1
        );
        assert_eq!(store.count("chunks").await.unwrap(), 1);
        assert_eq!(store.delete("chunks", &[]).await.unwrap(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

fn map_lance(error: lancedb::Error, action: &str) -> VectorError {
    use lancedb::Error;
    match &error {
        Error::TableNotFound { .. } | Error::IndexNotFound { .. } | Error::ColumnNotFound { .. } => {
            VectorError::TableNotFound(format!("{action}：{error}"))
        }
        Error::InvalidInput { .. }
        | Error::InvalidTableName { .. }
        | Error::InvalidExpression { .. } => {
            VectorError::config(format!("{action}：{error}"))
        }
        Error::Schema { .. } | Error::ColumnAlreadyExists { .. } => {
            VectorError::schema(format!("{action}：{error}"))
        }
        Error::Timeout { .. } => VectorError::transport(format!("{action}：{error}")),
        Error::ObjectStore { .. } => {
            // 对象存储未启用（本构建默认不含 aws/gcs/azure feature）时也走这里
            let text = error.to_string();
            if text.to_ascii_lowercase().contains("not supported")
                || text.to_ascii_lowercase().contains("not implemented")
            {
                VectorError::unsupported(format!(
                    "{action}：当前构建的 lancedb 未启用该对象存储（{text}）"
                ))
            } else {
                VectorError::transport(format!("{action}：{text}"))
            }
        }
        _ => VectorError::backend(format!("{action}：{error}")),
    }
}
