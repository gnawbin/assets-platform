//! PostgreSQL + pgvector 的 [`VectorStore`] 实现
//!
//! 只读纪律：`ping` / `describe` / `count` / `query` 只发 `SELECT`（含 `pg_catalog`
//! 反射），`ensure_table` / `upsert` / `delete` 只在应用主动写入向量时调用。
//! 采集账号只需 `CONNECT` + `USAGE` + `SELECT` 权限。

use async_trait::async_trait;
use sqlx::Row;
use sqlx::postgres::{PgPool, PgPoolOptions};
use vector::{
    Backend, Distance, IndexDescription, QueryRequest, SearchHit, StoreCapabilities, StoreConfig,
    StoreDescription, TableDescription, TableSpec, UpsertOutcome, VectorError, VectorRecord,
    VectorResult, VectorStore, batches, check_records, parse_vector_literal, vector_literal,
};

use crate::reflect::{
    build_filter_sql, build_upsert_sql, dims_from_typmod, distance_from_ops,
    index_kind_from_access_method, index_params_from_ddl, qualified_table, quote_ident,
    split_qualified, with_clause,
};
use crate::url::connect_url;

/// 连接池连接数上限：采集只读，2 条足够，避免占满业务连接
const MAX_CONNECTIONS: u32 = 2;
/// `describe` 时最多精确统计多少张表的行数（避免在大库上跑几十个 `count(*)`）
const MAX_COUNTED_TABLES: usize = 50;
/// 默认 schema
const DEFAULT_SCHEMA: &str = "public";
/// 可以充当「原文」的列名（按优先级）
const TEXT_COLUMN_CANDIDATES: [&str; 2] = ["content", "text"];
/// 可以充当「元数据」的列名
const METADATA_COLUMN_CANDIDATES: [&str; 1] = ["metadata"];

/// 反射全部向量列
const VECTOR_COLUMNS_SQL: &str = r"
SELECT n.nspname AS schema_name,
       c.relname AS table_name,
       a.attname AS column_name,
       a.atttypmod AS typmod
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
JOIN pg_attribute a ON a.attrelid = c.oid
JOIN pg_type t ON t.oid = a.atttypid
WHERE c.relkind IN ('r', 'p')
  AND a.attnum > 0
  AND NOT a.attisdropped
  AND n.nspname NOT IN ('pg_catalog', 'information_schema')
  AND t.typname IN ('vector', 'halfvec')
ORDER BY n.nspname, c.relname, a.attnum";

/// 反射单张表的索引（只关心 hnsw / ivfflat）
const TABLE_INDEXES_SQL: &str = r"
SELECT ic.relname AS index_name,
       am.amname AS access_method,
       COALESCE(pg_get_indexdef(ic.oid), '') AS ddl
FROM pg_index ix
JOIN pg_class ic ON ic.oid = ix.indexrelid
JOIN pg_class tc ON tc.oid = ix.indrelid
JOIN pg_namespace n ON n.oid = tc.relnamespace
JOIN pg_am am ON am.oid = ic.relam
WHERE n.nspname = $1
  AND tc.relname = $2
  AND am.amname IN ('hnsw', 'ivfflat')
ORDER BY ic.relname";

/// PostgreSQL + pgvector 向量存储
#[derive(Debug, Clone)]
pub struct PgVectorStore {
    config: StoreConfig,
    pool: PgPool,
    schema: String,
}

impl PgVectorStore {
    /// 建立连接池（连接数固定 2 条，`acquire_timeout` 取配置超时）
    pub async fn connect(config: StoreConfig) -> VectorResult<Self> {
        let url = connect_url(&config)?;
        let pool = PgPoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .acquire_timeout(config.timeout())
            .connect(&url)
            .await
            .map_err(|error| map_sqlx(error, "连接 PostgreSQL"))?;
        Ok(Self::from_pool(config, pool))
    }

    /// 复用调用方已有的连接池（服务层通常自己管池）
    pub fn from_pool(config: StoreConfig, pool: PgPool) -> Self {
        let schema = config
            .namespace
            .clone()
            .unwrap_or_else(|| DEFAULT_SCHEMA.to_string());
        Self {
            config,
            pool,
            schema,
        }
    }

    /// 连接池
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// 默认 schema
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// 生成 `"schema"."table"`；表名带 schema 前缀时以表名为准
    fn qualified(&self, table: &str) -> VectorResult<String> {
        qualified_table(&self.schema, table)
    }
}

/// 把 sqlx 错误分类成 [`VectorError`]
fn map_sqlx(error: sqlx::Error, action: &str) -> VectorError {
    match &error {
        sqlx::Error::RowNotFound => VectorError::TableNotFound(format!("{action}：结果为空")),
        sqlx::Error::Io(_) | sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => {
            VectorError::transport(format!("{action}：{error}"))
        }
        _ => {
            let Some(database_error) = error.as_database_error() else {
                return VectorError::backend(format!("{action}：{error}"));
            };
            let code = database_error
                .code()
                .map(|code| code.to_string())
                .unwrap_or_default();
            match code.as_str() {
                "42P01" => VectorError::TableNotFound(format!("{action}：{database_error}")),
                "42703" | "42804" => VectorError::schema(format!("{action}：{database_error}")),
                "28P01" | "28000" => {
                    VectorError::config(format!("{action}：认证失败（{database_error}）"))
                }
                _ => VectorError::backend(format!("{action}：[{code}] {database_error}")),
            }
        }
    }
}

/// 从 `SELECT version()` 里取出纯版本号
fn server_version(raw: &str) -> String {
    raw.split_whitespace()
        .nth(1)
        .unwrap_or(raw)
        .trim_end_matches(',')
        .to_string()
}

/// 索引名：`{table}_{column}_idx`，截断到 63 字节
fn index_name_for(table: &str, column: &str) -> String {
    let base = split_qualified(table).map_or(table, |(_, name)| name);
    let name = format!("{base}_{column}_idx");
    let mut name: String = name.chars().take(63).collect();
    if !vector::is_identifier(&name) {
        name = format!("{column}_idx");
    }
    name
}

/// 记录的元数据序列化成 JSON 文本（配合 `$n::jsonb` 绑定）
fn record_metadata(record: &VectorRecord) -> String {
    serde_json::to_string(&record.metadata).unwrap_or_else(|_| "{}".to_string())
}

/// 单表检索所需的反射结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableMeta {
    /// 向量维度
    pub dimensions: usize,
    /// 向量列名
    pub vector_column: String,
    /// 距离度量（来自索引操作符类，缺省余弦）
    pub distance: Distance,
    /// 原文列（可能不存在）
    pub text_column: Option<String>,
    /// 元数据列（可能不存在）
    pub metadata_column: Option<String>,
}

impl PgVectorStore {
    /// 列出表的全部列名
    async fn table_columns(&self, schema: &str, table: &str) -> VectorResult<Vec<String>> {
        const SQL: &str = r"
SELECT a.attname AS column_name
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
JOIN pg_attribute a ON a.attrelid = c.oid
WHERE n.nspname = $1
  AND c.relname = $2
  AND a.attnum > 0
  AND NOT a.attisdropped
ORDER BY a.attnum";
        let rows = sqlx::query(SQL)
            .bind(schema)
            .bind(table)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| map_sqlx(error, "读取表列"))?;
        let mut columns = Vec::with_capacity(rows.len());
        for row in rows {
            let name: String = row
                .try_get("column_name")
                .map_err(|error| map_sqlx(error, "解析列名"))?;
            columns.push(name);
        }
        Ok(columns)
    }

    /// 读取已有向量列的维度（`atttypmod - 4`）
    async fn existing_dimensions(
        &self,
        schema: &str,
        table: &str,
        column: &str,
    ) -> VectorResult<Option<usize>> {
        const SQL: &str = r"
SELECT a.atttypmod AS typmod
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
JOIN pg_attribute a ON a.attrelid = c.oid
WHERE n.nspname = $1
  AND c.relname = $2
  AND a.attname = $3
  AND NOT a.attisdropped";
        let typmod: Option<i32> = sqlx::query_scalar(SQL)
            .bind(schema)
            .bind(table)
            .bind(column)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| map_sqlx(error, "读取列维度"))?;
        Ok(typmod.and_then(dims_from_typmod))
    }

    /// 表上第一个向量索引的距离度量（没有索引时按余弦）
    async fn table_distance(&self, schema: &str, table: &str) -> VectorResult<Distance> {
        let rows = sqlx::query(TABLE_INDEXES_SQL)
            .bind(schema)
            .bind(table)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| map_sqlx(error, "读取索引定义"))?;
        for row in rows {
            let ddl: String = row
                .try_get("ddl")
                .map_err(|error| map_sqlx(error, "解析索引定义"))?;
            if let Some(distance) = distance_from_ops(&ddl) {
                return Ok(distance);
            }
        }
        Ok(Distance::Cosine)
    }

    /// 反射单表结构；表不存在或没有向量列时返回 `Ok(None)`
    pub async fn table_meta(&self, table: &str) -> VectorResult<Option<TableMeta>> {
        let (schema_name, table_name) = split_qualified(table)
            .map_or((self.schema.as_str(), table), |parts| parts);
        const SQL: &str = r"
SELECT a.attname AS column_name,
       a.atttypmod AS typmod
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
JOIN pg_attribute a ON a.attrelid = c.oid
JOIN pg_type t ON t.oid = a.atttypid
WHERE n.nspname = $1
  AND c.relname = $2
  AND a.attnum > 0
  AND NOT a.attisdropped
  AND t.typname IN ('vector', 'halfvec')
ORDER BY a.attnum
LIMIT 1";
        let row = sqlx::query(SQL)
            .bind(schema_name)
            .bind(table_name)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| map_sqlx(error, "探测向量列"))?;
        let Some(row) = row else {
            return Ok(None);
        };
        let vector_column: String = row
            .try_get("column_name")
            .map_err(|error| map_sqlx(error, "解析向量列名"))?;
        let typmod: i32 = row
            .try_get("typmod")
            .map_err(|error| map_sqlx(error, "解析向量列维度"))?;
        let Some(dimensions) = dims_from_typmod(typmod) else {
            return Err(VectorError::schema(format!(
                "表 {schema_name}.{table_name} 的 {vector_column} 列没有声明维度，无法做相似度检索"
            )));
        };
        let columns = self.table_columns(schema_name, table_name).await?;
        let text_column = TEXT_COLUMN_CANDIDATES
            .iter()
            .find(|candidate| columns.iter().any(|column| column == *candidate))
            .map(|candidate| (*candidate).to_string());
        let metadata_column = METADATA_COLUMN_CANDIDATES
            .iter()
            .find(|candidate| columns.iter().any(|column| column == *candidate))
            .map(|candidate| (*candidate).to_string());
        let distance = self.table_distance(schema_name, table_name).await?;
        Ok(Some(TableMeta {
            dimensions,
            vector_column,
            distance,
            text_column,
            metadata_column,
        }))
    }
}

#[async_trait]
impl VectorStore for PgVectorStore {
    fn backend(&self) -> Backend {
        Backend::PgVector
    }

    fn config(&self) -> &StoreConfig {
        &self.config
    }

    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities::for_backend(Backend::PgVector)
    }

    async fn ping(&self) -> VectorResult<()> {
        let one: i32 = sqlx::query_scalar("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|error| map_sqlx(error, "PING"))?;
        if one != 1 {
            return Err(VectorError::backend("SELECT 1 返回了非 1 的结果"));
        }
        Ok(())
    }

    async fn describe(&self) -> VectorResult<StoreDescription> {
        let version: String = sqlx::query_scalar("SELECT version()")
            .fetch_one(&self.pool)
            .await
            .map_err(|error| map_sqlx(error, "读取 PostgreSQL 版本"))?;
        let mut description =
            StoreDescription::new(Backend::PgVector).with_server_version(server_version(&version));

        let extension: Option<String> =
            sqlx::query_scalar("SELECT extversion FROM pg_extension WHERE extname = 'vector'")
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| map_sqlx(error, "检查 pgvector 扩展"))?;
        if extension.is_none() {
            return Ok(description.with_warning(
                "目标库未安装 pgvector 扩展，无法列举向量表（需 DBA 执行 CREATE EXTENSION vector）",
            ));
        }

        let rows = sqlx::query(VECTOR_COLUMNS_SQL)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| map_sqlx(error, "反射向量列"))?;
        let mut tables: Vec<TableDescription> = Vec::new();
        for row in rows {
            let schema_name: String = row
                .try_get("schema_name")
                .map_err(|error| map_sqlx(error, "解析 schema 名"))?;
            let table_name: String = row
                .try_get("table_name")
                .map_err(|error| map_sqlx(error, "解析表名"))?;
            let column_name: String = row
                .try_get("column_name")
                .map_err(|error| map_sqlx(error, "解析列名"))?;
            let typmod: i32 = row
                .try_get("typmod")
                .map_err(|error| map_sqlx(error, "解析列维度"))?;
            let qualified_name = format!("{schema_name}.{table_name}");
            match tables.iter_mut().find(|table| table.name == qualified_name) {
                Some(table) => {
                    if table.dimensions.is_none() {
                        table.dimensions = dims_from_typmod(typmod);
                    }
                }
                None => {
                    let table =
                        TableDescription::new(qualified_name).with_vector_column(column_name);
                    tables.push(match dims_from_typmod(typmod) {
                        Some(dimensions) => table.with_dimensions(dimensions),
                        None => table,
                    });
                }
            }
        }

        for (index, table) in tables.iter_mut().enumerate() {
            if index >= MAX_COUNTED_TABLES {
                description = description.with_warning(format!(
                    "向量表超过 {MAX_COUNTED_TABLES} 张，仅统计前 {MAX_COUNTED_TABLES} 张的行数与索引"
                ));
                break;
            }
            let (schema_name, table_name) = match split_qualified(&table.name) {
                Some((schema, name)) => (schema.to_string(), name.to_string()),
                None => (self.schema.clone(), table.name.clone()),
            };
            let count_sql = format!(
                "SELECT count(*) FROM {}",
                qualified_table(&schema_name, &table_name)?
            );
            match sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(count_sql))
                .fetch_one(&self.pool)
                .await
            {
                Ok(count) => table.row_count = Some(u64::try_from(count).unwrap_or(0)),
                Err(error) => {
                    description =
                        description.with_warning(format!("统计 {} 行数失败：{error}", table.name));
                }
            }
            match sqlx::query(TABLE_INDEXES_SQL)
                .bind(&schema_name)
                .bind(&table_name)
                .fetch_all(&self.pool)
                .await
            {
                Ok(index_rows) => {
                    for row in index_rows {
                        let index_name: String = row
                            .try_get("index_name")
                            .map_err(|error| map_sqlx(error, "解析索引名"))?;
                        let access_method: String = row
                            .try_get("access_method")
                            .map_err(|error| map_sqlx(error, "解析索引类型"))?;
                        let ddl: String = row
                            .try_get("ddl")
                            .map_err(|error| map_sqlx(error, "解析索引定义"))?;
                        let Some(kind) = index_kind_from_access_method(&access_method) else {
                            continue;
                        };
                        let mut described = IndexDescription::new(index_name).with_kind(kind);
                        if let Some(distance) = distance_from_ops(&ddl) {
                            described = described.with_distance(distance);
                            if table.distance.is_none() {
                                table.distance = Some(distance);
                            }
                        }
                        for (key, value) in index_params_from_ddl(&ddl) {
                            described = described.with_param(key, value);
                        }
                        table.indices.push(described);
                    }
                }
                Err(error) => {
                    description =
                        description.with_warning(format!("读取 {} 的索引失败：{error}", table.name));
                }
            }
        }

        for table in tables {
            description = description.with_table(table);
        }
        Ok(description)
    }

    async fn ensure_table(&self, spec: &TableSpec) -> VectorResult<()> {
        spec.validate()?;
        let qualified = self.qualified(&spec.name)?;
        self.ensure_extension().await?;
        if let Some((schema_name, _)) = split_qualified(&spec.name) {
            let sql = format!("CREATE SCHEMA IF NOT EXISTS {}", quote_ident(schema_name));
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .execute(&self.pool)
                .await
                .map_err(|error| map_sqlx(error, "创建 schema"))?;
        }

        let vector = &spec.vector_column;
        let metadata = &spec.metadata_column;
        let mut ddl = format!(
            "CREATE TABLE IF NOT EXISTS {qualified} (id text PRIMARY KEY, \
             {vector} vector({dimensions}) NOT NULL)",
            dimensions = spec.dimensions
        );
        ddl.pop();
        if !spec.text_column.is_empty() {
            let text = &spec.text_column;
            ddl.push_str(&format!(", {text} text"));
        }
        ddl.push_str(&format!(", {metadata} jsonb NOT NULL DEFAULT '{{}}'::jsonb)"));
        sqlx::query(sqlx::AssertSqlSafe(ddl))
            .execute(&self.pool)
            .await
            .map_err(|error| map_sqlx(error, "创建向量表"))?;

        let (schema_name, table_name) = match split_qualified(&spec.name) {
            Some((schema, name)) => (schema.to_string(), name.to_string()),
            None => (self.schema.clone(), spec.name.clone()),
        };
        if let Some(existing) = self
            .existing_dimensions(&schema_name, &table_name, vector)
            .await?
        {
            if existing != spec.dimensions {
                return Err(VectorError::schema(format!(
                    "表 {} 的向量列 {vector} 已是 {existing} 维，与请求的 {} 维不一致（需人工迁移后再写入）",
                    spec.name, spec.dimensions
                )));
            }
        }

        if let Some(access_method) = spec.index.kind.pgvector_access_method() {
            let index_name = index_name_for(&spec.name, vector);
            let sql = format!(
                "CREATE INDEX IF NOT EXISTS {index_name} ON {qualified} USING {access_method} \
                 ({vector} {ops}){params}",
                ops = spec.distance.pgvector_ops(),
                params = with_clause(&spec.index.parameters)
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .execute(&self.pool)
                .await
                .map_err(|error| map_sqlx(error, "创建向量索引"))?;
        }
        Ok(())
    }

    async fn upsert(
        &self,
        spec: &TableSpec,
        records: &[VectorRecord],
    ) -> VectorResult<UpsertOutcome> {
        check_records(spec, records)?;
        let qualified = self.qualified(&spec.name)?;
        let mut affected = 0_u64;
        let mut batch_count = 0_usize;
        for chunk in batches(records) {
            batch_count += 1;
            let sql = build_upsert_sql(&qualified, spec, chunk.len());
            let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
            for record in chunk {
                query = query
                    .bind(record.id.as_str())
                    .bind(vector_literal(&record.vector));
                if !spec.text_column.is_empty() {
                    query = query.bind(record.text.as_deref());
                }
                query = query.bind(record_metadata(record));
            }
            let result = query
                .execute(&self.pool)
                .await
                .map_err(|error| map_sqlx(error, "写入向量"))?;
            affected += result.rows_affected();
        }
        Ok(UpsertOutcome::new(affected, batch_count))
    }

    async fn delete(&self, table: &str, ids: &[String]) -> VectorResult<u64> {
        if ids.is_empty() {
            return Ok(0);
        }
        if ids.len() > vector::MAX_BATCH {
            return Err(VectorError::config(format!(
                "单次删除最多 {} 个主键，实际 {} 个",
                vector::MAX_BATCH,
                ids.len()
            )));
        }
        let qualified = self.qualified(table)?;
        let sql = format!("DELETE FROM {qualified} WHERE id = ANY($1)");
        let result = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(ids.to_vec())
            .execute(&self.pool)
            .await
            .map_err(|error| map_sqlx(error, "删除向量"))?;
        Ok(result.rows_affected())
    }

    async fn count(&self, table: &str) -> VectorResult<u64> {
        let qualified = self.qualified(table)?;
        let sql = format!("SELECT count(*) FROM {qualified}");
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .fetch_one(&self.pool)
            .await
            .map_err(|error| map_sqlx(error, "统计行数"))?;
        Ok(u64::try_from(count).unwrap_or(0))
    }

    async fn query(&self, table: &str, request: &QueryRequest) -> VectorResult<Vec<SearchHit>> {
        let qualified = self.qualified(table)?;
        let Some(meta) = self.table_meta(table).await? else {
            return Err(VectorError::TableNotFound(table.to_string()));
        };
        request.validate(meta.dimensions)?;

        let mut params: Vec<String> = vec![vector_literal(&request.vector)];
        let where_clause = match (&request.filter, &meta.metadata_column) {
            (Some(filter), Some(metadata_column)) => Some(format!(
                " WHERE {}",
                build_filter_sql(filter, &mut params, metadata_column)?
            )),
            (Some(_), None) => {
                return Err(VectorError::schema(format!(
                    "表 {table} 没有元数据列，无法下推过滤条件"
                )));
            }
            (None, _) => None,
        };

        let operator = meta.distance.pgvector_operator();
        let vector = &meta.vector_column;
        let mut select = format!("id, {vector} {operator} $1::vector AS distance");
        if let Some(text_column) = &meta.text_column {
            select.push_str(&format!(", {text_column}::text AS content_text"));
        }
        if let Some(metadata_column) = &meta.metadata_column {
            select.push_str(&format!(", {metadata_column}::text AS metadata_text"));
        }
        if request.include_vector {
            select.push_str(&format!(", {vector}::text AS vector_text"));
        }
        let sql = format!(
            "SELECT {select} FROM {qualified}{where_clause} \
             ORDER BY {vector} {operator} $1::vector LIMIT {top_k}",
            where_clause = where_clause.unwrap_or_default(),
            top_k = request.top_k
        );
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for param in &params {
            query = query.bind(param.as_str());
        }
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|error| map_sqlx(error, "向量检索"))?;

        let mut hits = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row
                .try_get("id")
                .map_err(|error| map_sqlx(error, "解析主键"))?;
            let distance: f64 = row
                .try_get("distance")
                .map_err(|error| map_sqlx(error, "解析距离"))?;
            let mut hit = SearchHit::from_distance(id, distance as f32, meta.distance);
            if meta.text_column.is_some() {
                hit.text = row
                    .try_get::<Option<String>, _>("content_text")
                    .map_err(|error| map_sqlx(error, "解析原文"))?;
            }
            if meta.metadata_column.is_some() {
                let raw = row
                    .try_get::<Option<String>, _>("metadata_text")
                    .map_err(|error| map_sqlx(error, "解析元数据"))?;
                if let Some(raw) = raw {
                    if let Ok(map) =
                        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&raw)
                    {
                        hit.metadata = map.into_iter().collect();
                    }
                }
            }
            if request.include_vector {
                let raw = row
                    .try_get::<Option<String>, _>("vector_text")
                    .map_err(|error| map_sqlx(error, "解析向量"))?;
                if let Some(raw) = raw {
                    hit.vector = Some(parse_vector_literal(&raw)?);
                }
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}

impl PgVectorStore {
    /// 确保 pgvector 扩展可用（`CREATE EXTENSION IF NOT EXISTS vector`）
    ///
    /// 采集账号通常没有建扩展权限，因此失败时会再查一次 `pg_extension`：
    /// 扩展其实已装好就放行，只有确实没装才报错。建扩展属于**首次初始化**，
    /// 采集链路（`describe`）不会走到这里。
    async fn ensure_extension(&self) -> VectorResult<()> {
        const INSTALLED_SQL: &str = "SELECT extversion FROM pg_extension WHERE extname = 'vector'";
        if let Err(error) = sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
            .execute(&self.pool)
            .await
        {
            let installed: Option<String> = sqlx::query_scalar(INSTALLED_SQL)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| map_sqlx(error, "检查 pgvector 扩展"))?;
            if installed.is_none() {
                return Err(map_sqlx(error, "创建 pgvector 扩展"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_version_extracts_number() {
        assert_eq!(
            server_version("PostgreSQL 16.3 (Debian 16.3-1.pgdg120+1) on x86_64-pc-linux-gnu"),
            "16.3"
        );
        assert_eq!(server_version("PostgreSQL 15.4,"), "15.4");
        assert_eq!(server_version("weird"), "weird");
    }

    #[test]
    fn index_name_is_identifier_and_bounded() {
        assert_eq!(
            index_name_for("public.chunks", "embedding"),
            "chunks_embedding_idx"
        );
        assert_eq!(
            index_name_for("chunks", "embedding"),
            "chunks_embedding_idx"
        );
        let long = index_name_for(&"t".repeat(60), &"e".repeat(20));
        assert!(long.len() <= 63, "{long}");
        assert!(vector::is_identifier(&long), "{long}");
    }

    #[test]
    fn metadata_serializes_as_json_object() {
        let record = VectorRecord::new("a", vec![0.1])
            .with_metadata("lang", "zh")
            .with_metadata("n", 3);
        assert_eq!(record_metadata(&record), r#"{"lang":"zh","n":3}"#);
        assert_eq!(record_metadata(&VectorRecord::new("b", vec![0.1])), "{}");
    }

    #[test]
    fn sqlx_errors_are_classified() {
        assert_eq!(
            map_sqlx(sqlx::Error::RowNotFound, "读取").kind(),
            "table_not_found"
        );
        assert_eq!(
            map_sqlx(sqlx::Error::Configuration("boom".into()), "连接").kind(),
            "backend"
        );
    }

    #[test]
    fn table_meta_carries_reflection_result() {
        let meta = TableMeta {
            dimensions: 768,
            vector_column: "embedding".to_string(),
            distance: Distance::L2,
            text_column: Some("content".to_string()),
            metadata_column: None,
        };
        assert_eq!(meta.dimensions, 768);
        assert_eq!(meta.distance.pgvector_operator(), "<->");
        assert_eq!(meta.text_column.as_deref(), Some("content"));
        assert!(meta.metadata_column.is_none());
    }
}

