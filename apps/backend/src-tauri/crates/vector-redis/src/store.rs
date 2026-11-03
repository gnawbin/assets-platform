//! Redis / Valkey（RediSearch 向量集）的 [`VectorStore`] 实现
//!
//! 存储形态：
//! - 每条记录是一个 HASH：`{prefix}{table}:{id}`，字段为原文 / 元数据 JSON / 向量二进制；
//! - 索引是 RediSearch 向量索引：`FT.CREATE {prefix}{table} ... PREFIX 1 {prefix}{table}:`；
//! - 维度 / 度量 / 向量列名等元信息另存一个侧车 HASH `{prefix}{table}:__meta__`
//!   （`FT.INFO` 的回复是异质嵌套数组，侧车 HASH 让反射既准确又省事）。
//!
//! 只读纪律：`ping` / `describe` / `count` / `query` 只用 `PING`、`INFO`、
//! `FT._LIST`、`FT.SEARCH`、`HGETALL` 这些读命令；采集链路不需要 `FT.CREATE`。

use std::collections::HashMap;

use async_trait::async_trait;
use redis::aio::ConnectionManager;
use redis::{RedisError, Value, cmd};
use vector::{
    Backend, Distance, IndexDescription, IndexKind, QueryRequest, SearchHit, StoreCapabilities,
    StoreConfig, StoreDescription, TableDescription, TableSpec, UpsertOutcome, VectorError,
    VectorRecord, VectorResult, VectorStore, batches, check_records,
};

use crate::info::{info_field, parse_info};
use crate::url::{connect_url, key_prefix};
use crate::value::{parse_search_reply, sequence};

/// 侧车元信息 HASH 的后缀
const META_SUFFIX: &str = "__meta__";
/// KNN 查询里距离字段的别名
const SCORE_FIELD: &str = "__score";

/// Redis / Valkey 向量存储
#[derive(Debug, Clone)]
pub struct RedisVectorStore {
    config: StoreConfig,
    conn: ConnectionManager,
    prefix: String,
}

impl RedisVectorStore {
    /// 建立连接（`ConnectionManager` 自带多路复用与自动重连）
    pub async fn connect(config: StoreConfig) -> VectorResult<Self> {
        let url = connect_url(&config)?;
        let client = redis::Client::open(url.as_str())
            .map_err(|error| VectorError::config(format!("Redis 端点无法解析：{error}")))?;
        let conn = ConnectionManager::new(client)
            .await
            .map_err(|error| map_redis(error, "连接 Redis"))?;
        let prefix = key_prefix(&config);
        Ok(Self {
            config,
            conn,
            prefix,
        })
    }

    /// 连接句柄（服务层可复用同一条连接池）
    pub fn connection(&self) -> &ConnectionManager {
        &self.conn
    }

    /// 索引名 = key 前缀 + 表名
    fn index_name(&self, table: &str) -> VectorResult<String> {
        vector::validate_table_name(table)?;
        Ok(format!("{}{table}", self.prefix))
    }

    /// 记录的 key 前缀：`{prefix}{table}:`
    fn hash_prefix(&self, table: &str) -> String {
        format!("{}{table}:", self.prefix)
    }

    /// 单条记录的 key：`{prefix}{table}:{id}`
    fn hash_key(&self, table: &str, id: &str) -> String {
        format!("{}{table}:{id}", self.prefix)
    }

    /// 侧车元信息 key
    fn meta_key(&self, table: &str) -> String {
        format!("{}{table}:{META_SUFFIX}", self.prefix)
    }

    /// 读侧车元信息（不存在时返回空表）
    async fn read_meta(&self, table: &str) -> VectorResult<HashMap<String, String>> {
        cmd("HGETALL")
            .arg(self.meta_key(table))
            .query_async(&mut self.conn.clone())
            .await
            .map_err(|error| map_redis(error, "读取索引元信息"))
    }

    /// 索引是否已存在（`FT._LIST` 是官方列索引命令）
    async fn index_exists(&self, index: &str) -> VectorResult<bool> {
        let indexes: Vec<String> = cmd("FT._LIST")
            .query_async(&mut self.conn.clone())
            .await
            .map_err(|error| map_redis(error, "列出向量索引"))?;
        Ok(indexes.iter().any(|name| name == index))
    }

    /// 索引里的文档数（`FT.SEARCH ... LIMIT 0 0` 只回总数）
    async fn index_count(&self, index: &str) -> VectorResult<u64> {
        let reply: Value = cmd("FT.SEARCH")
            .arg(index)
            .arg("*")
            .arg("LIMIT")
            .arg(0)
            .arg(0)
            .query_async(&mut self.conn.clone())
            .await
            .map_err(|error| map_redis(error, "统计索引文档数"))?;
        Ok(parse_search_reply(&reply).0)
    }
}

/// 把 f32 向量打成 RediSearch 需要的 FLOAT32 小端二进制
pub fn vector_blob(vector: &[f32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    blob
}

/// 从 `FT.SEARCH` 返回的 key 里剥出记录 id
pub fn record_id(key: &str, hash_prefix: &str) -> String {
    key.strip_prefix(hash_prefix).unwrap_or(key).to_string()
}

/// 把 redis 错误分类成 [`VectorError`]
fn map_redis(error: RedisError, action: &str) -> VectorError {
    let text = error.to_string();
    let lower = text.to_ascii_lowercase();
    if error.is_timeout() || error.is_io_error() || error.is_connection_dropped() {
        return VectorError::transport(format!("{action}：{text}"));
    }
    if lower.contains("unknown index") || lower.contains("no such index") {
        return VectorError::TableNotFound(format!("{action}：{text}"));
    }
    if lower.contains("unknown command") || lower.contains("unknown module") {
        return VectorError::unsupported(format!(
            "{action}：服务端未加载 RediSearch / valkey-search 模块（{text}）"
        ));
    }
    if lower.contains("noperm") || lower.contains("wrongpass") || lower.contains("invalid password")
    {
        return VectorError::config(format!("{action}：认证或权限不足（{text}）"));
    }
    VectorError::backend(format!("{action}：{text}"))
}

/// 把 FLOAT32 小端二进制还原成 f32 向量
pub fn parse_vector_blob(blob: &[u8]) -> Option<Vec<f32>> {
    if blob.len() % 4 != 0 {
        return None;
    }
    Some(
        blob.chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect(),
    )
}

#[async_trait]
impl VectorStore for RedisVectorStore {
    fn backend(&self) -> Backend {
        Backend::Redis
    }

    fn config(&self) -> &StoreConfig {
        &self.config
    }

    fn capabilities(&self) -> StoreCapabilities {
        // 本实现把元数据整体存成一个 TEXT 字段（JSON），没有为每个字段声明 TAG，
        // 因此元数据过滤无法下推；如实上报，避免界面允许用户填了却没效果。
        StoreCapabilities {
            metadata_filter: false,
            ..StoreCapabilities::for_backend(Backend::Redis)
        }
    }

    async fn ping(&self) -> VectorResult<()> {
        let pong: String = cmd("PING")
            .query_async(&mut self.conn.clone())
            .await
            .map_err(|error| map_redis(error, "PING"))?;
        if pong.eq_ignore_ascii_case("PONG") {
            Ok(())
        } else {
            Err(VectorError::backend(format!("PING 返回了 {pong}")))
        }
    }

    async fn describe(&self) -> VectorResult<StoreDescription> {
        let info_text: String = cmd("INFO")
            .query_async(&mut self.conn.clone())
            .await
            .map_err(|error| map_redis(error, "读取 INFO"))?;
        let sections = parse_info(&info_text);
        let version = info_field(&sections, "server", "redis_version")
            .unwrap_or("unknown")
            .to_string();
        let mut description = StoreDescription::new(Backend::Redis).with_server_version(version);

        let modules = info_field(&sections, "modules", "module").unwrap_or_default();
        if !modules.to_ascii_lowercase().contains("search") {
            return Ok(description
                .with_warning("服务端未加载 RediSearch / valkey-search 模块，向量索引不可用"));
        }

        let indexes: Vec<String> = cmd("FT._LIST")
            .query_async(&mut self.conn.clone())
            .await
            .map_err(|error| map_redis(error, "列出向量索引"))?;
        for index in indexes {
            let mut table = TableDescription::new(index.clone());
            match self.read_meta(&index).await {
                Ok(meta) => {
                    if let Some(dimensions) = meta
                        .get("dimensions")
                        .and_then(|value| value.parse::<usize>().ok())
                    {
                        table = table.with_dimensions(dimensions);
                    }
                    if let Some(distance) = meta
                        .get("distance")
                        .and_then(|value| value.parse::<Distance>().ok())
                    {
                        table = table.with_distance(distance);
                    }
                    if let Some(column) = meta.get("vector_column") {
                        table = table.with_vector_column(column.clone());
                    }
                    if let Some(kind) = meta
                        .get("index_kind")
                        .and_then(|value| value.parse::<IndexKind>().ok())
                    {
                        table =
                            table.with_index(IndexDescription::new(index.clone()).with_kind(kind));
                    }
                }
                Err(error) => {
                    description =
                        description.with_warning(format!("读取 {index} 的元信息失败：{error}"));
                }
            }
            match self.index_count(&index).await {
                Ok(count) => table = table.with_row_count(count),
                Err(error) => {
                    description =
                        description.with_warning(format!("统计 {index} 文档数失败：{error}"));
                }
            }
            description = description.with_table(table);
        }
        Ok(description)
    }

    async fn ensure_table(&self, spec: &TableSpec) -> VectorResult<()> {
        spec.validate()?;
        let index = self.index_name(&spec.name)?;
        if self.index_exists(&index).await? {
            if let Ok(meta) = self.read_meta(&spec.name).await {
                if let Some(existing) = meta
                    .get("dimensions")
                    .and_then(|value| value.parse::<usize>().ok())
                {
                    if existing != spec.dimensions {
                        return Err(VectorError::schema(format!(
                            "索引 {index} 已是 {existing} 维，与请求的 {} 维不一致（RediSearch 只能重建索引）",
                            spec.dimensions
                        )));
                    }
                }
            }
            return Ok(());
        }

        let algorithm = spec.index.kind.redis_algorithm().ok_or_else(|| {
            VectorError::unsupported(format!(
                "RediSearch 只支持 FLAT / HNSW 索引，不支持 {}",
                spec.index.kind
            ))
        })?;
        let mut vector_args: Vec<String> = vec![
            "TYPE".to_string(),
            "FLOAT32".to_string(),
            "DIM".to_string(),
            spec.dimensions.to_string(),
            "DISTANCE_METRIC".to_string(),
            spec.distance.redis_metric().to_string(),
        ];
        for (key, value) in &spec.index.parameters {
            vector_args.push(key.to_ascii_uppercase());
            vector_args.push(value.clone());
        }

        let mut command = cmd("FT.CREATE");
        command
            .arg(&index)
            .arg("ON")
            .arg("HASH")
            .arg("PREFIX")
            .arg(1)
            .arg(self.hash_prefix(&spec.name))
            .arg("SCHEMA");
        if !spec.text_column.is_empty() {
            command.arg(&spec.text_column).arg("TEXT");
        }
        command.arg(&spec.metadata_column).arg("TEXT");
        command
            .arg(&spec.vector_column)
            .arg("VECTOR")
            .arg(algorithm)
            .arg(vector_args.len())
            .arg(&vector_args);
        if let Err(error) = command.query_async::<Value>(&mut self.conn.clone()).await {
            let text = error.to_string().to_ascii_lowercase();
            if !text.contains("index already exists") {
                return Err(map_redis(error, "创建向量索引"));
            }
        }

        // 侧车元信息：失败只影响 describe 的完整度，不影响索引本身
        let _ = cmd("HSET")
            .arg(self.meta_key(&spec.name))
            .arg("dimensions")
            .arg(spec.dimensions)
            .arg("distance")
            .arg(spec.distance.as_str())
            .arg("vector_column")
            .arg(&spec.vector_column)
            .arg("text_column")
            .arg(&spec.text_column)
            .arg("metadata_column")
            .arg(&spec.metadata_column)
            .arg("index_kind")
            .arg(spec.index.kind.as_str())
            .query_async::<i64>(&mut self.conn.clone())
            .await;
        Ok(())
    }

    async fn upsert(
        &self,
        spec: &TableSpec,
        records: &[VectorRecord],
    ) -> VectorResult<UpsertOutcome> {
        check_records(spec, records)?;
        let mut affected = 0_u64;
        let mut batch_count = 0_usize;
        for chunk in batches(records) {
            batch_count += 1;
            let mut pipe = redis::pipe();
            for record in chunk {
                let key = self.hash_key(&spec.name, &record.id);
                let metadata =
                    serde_json::to_string(&record.metadata).unwrap_or_else(|_| "{}".to_string());
                let mut command = pipe.cmd("HSET").arg(key);
                if !spec.text_column.is_empty() {
                    command = command
                        .arg(&spec.text_column)
                        .arg(record.text.clone().unwrap_or_default());
                }
                command
                    .arg(&spec.metadata_column)
                    .arg(metadata)
                    .arg(&spec.vector_column)
                    .arg(vector_blob(&record.vector));
            }
            pipe.ignore()
                .query_async::<()>(&mut self.conn.clone())
                .await
                .map_err(|error| map_redis(error, "写入向量"))?;
            affected += chunk.len() as u64;
        }
        Ok(UpsertOutcome::new(affected, batch_count))
    }

    async fn delete(&self, table: &str, ids: &[String]) -> VectorResult<u64> {
        if ids.is_empty() {
            return Ok(0);
        }
        vector::validate_table_name(table)?;
        let mut command = cmd("DEL");
        for id in ids {
            command.arg(self.hash_key(table, id));
        }
        let removed: i64 = command
            .query_async(&mut self.conn.clone())
            .await
            .map_err(|error| map_redis(error, "删除向量"))?;
        Ok(u64::try_from(removed).unwrap_or(0))
    }

    async fn count(&self, table: &str) -> VectorResult<u64> {
        let index = self.index_name(table)?;
        self.index_count(&index).await
    }

    async fn query(&self, table: &str, request: &QueryRequest) -> VectorResult<Vec<SearchHit>> {
        if request.filter.is_some() {
            return Err(VectorError::unsupported(
                "Redis 向量集把元数据整体存成 JSON 文本，未声明 TAG 字段，无法下推过滤条件",
            ));
        }
        let index = self.index_name(table)?;
        let meta = self.read_meta(table).await.unwrap_or_default();
        let Some(dimensions) = meta
            .get("dimensions")
            .and_then(|value| value.parse::<usize>().ok())
        else {
            return Err(VectorError::TableNotFound(format!(
                "{index}（缺少侧车元信息 {META_SUFFIX}，请先调用 ensure_table）"
            )));
        };
        request.validate(dimensions)?;

        let distance = meta
            .get("distance")
            .and_then(|value| value.parse::<Distance>().ok())
            .unwrap_or(Distance::Cosine);
        let vector_column = meta
            .get("vector_column")
            .cloned()
            .unwrap_or_else(|| TableSpec::DEFAULT_VECTOR_COLUMN.to_string());
        let text_column = meta
            .get("text_column")
            .cloned()
            .unwrap_or_else(|| TableSpec::DEFAULT_TEXT_COLUMN.to_string());
        let metadata_column = meta
            .get("metadata_column")
            .cloned()
            .unwrap_or_else(|| TableSpec::DEFAULT_METADATA_COLUMN.to_string());

        let knn = format!(
            "*=>[KNN {} @{vector_column} $vec AS {SCORE_FIELD}]",
            request.top_k
        );
        let mut fields: Vec<&str> = vec![SCORE_FIELD, text_column.as_str(), metadata_column.as_str()];
        if !text_column.is_empty() && text_column == metadata_column {
            fields.dedup();
        }

        let reply: Value = cmd("FT.SEARCH")
            .arg(&index)
            .arg(&knn)
            .arg("PARAMS")
            .arg(2)
            .arg("vec")
            .arg(vector_blob(&request.vector))
            .arg("SORTBY")
            .arg(SCORE_FIELD)
            .arg("LIMIT")
            .arg(0)
            .arg(request.top_k)
            .arg("RETURN")
            .arg(fields.len())
            .arg(&fields)
            .arg("DIALECT")
            .arg(2)
            .query_async(&mut self.conn.clone())
            .await
            .map_err(|error| map_redis(error, "向量检索"))?;

        let hash_prefix = self.hash_prefix(table);
        let (_, raw_hits) = parse_search_reply(&reply);
        let mut hits = Vec::with_capacity(raw_hits.len());
        for raw_hit in raw_hits {
            let id = record_id(&raw_hit.key, &hash_prefix);
            let raw_distance = raw_hit
                .field(SCORE_FIELD)
                .and_then(|value| value.parse::<f32>().ok())
                .unwrap_or(0.0);
            let mut hit = SearchHit::from_distance(id, raw_distance, distance);
            hit.text = raw_hit.field(&text_column).map(ToString::to_string);
            if let Some(raw) = raw_hit.field(&metadata_column) {
                if let Ok(map) =
                    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(raw)
                {
                    hit.metadata = map.into_iter().collect();
                }
            }
            if request.include_vector {
                let stored: Value = cmd("HMGET")
                    .arg(&raw_hit.key)
                    .arg(&vector_column)
                    .query_async(&mut self.conn.clone())
                    .await
                    .map_err(|error| map_redis(error, "读取向量"))?;
                if let Some(bytes) = sequence(&stored)
                    .and_then(|items| items.first())
                    .and_then(crate::value::bytes)
                {
                    hit.vector = parse_vector_blob(bytes);
                }
            }
            hits.push(hit);
        }
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_blob_round_trips() {
        let vector = vec![0.5_f32, -1.25, 3.0];
        let blob = vector_blob(&vector);
        assert_eq!(blob.len(), 12);
        assert_eq!(blob[..4], 0.5_f32.to_le_bytes());
        assert_eq!(parse_vector_blob(&blob).unwrap(), vector);
    }

    #[test]
    fn vector_blob_handles_bad_input() {
        assert!(parse_vector_blob(&[]).unwrap().is_empty());
        // 长度不是 4 的倍数 → None（不 panic）
        assert!(parse_vector_blob(&[0, 1, 2]).is_none());
    }

    #[test]
    fn record_id_strips_prefix() {
        assert_eq!(record_id("chunks:doc-1", "chunks:"), "doc-1");
        assert_eq!(record_id("tenant_a:chunks:doc-1", "tenant_a:chunks:"), "doc-1");
        // 前缀不匹配时原样返回
        assert_eq!(record_id("other:doc-1", "chunks:"), "other:doc-1");
    }

    #[test]
    fn redis_errors_are_classified() {
        // RediSearch 模块的错误在 redis-rs 里是 Extension（不是内置 Server 错误）
        let missing = RedisError::from((redis::ErrorKind::Extension, "Unknown Index name"));
        assert_eq!(map_redis(missing, "检索").kind(), "table_not_found");

        let no_module = RedisError::from((
            redis::ErrorKind::Extension,
            "ERR unknown command 'FT.SEARCH'",
        ));
        assert_eq!(map_redis(no_module, "检索").kind(), "unsupported");

        let auth = RedisError::from((redis::ErrorKind::AuthenticationFailed, "NOPERM"));
        assert_eq!(map_redis(auth, "PING").kind(), "config");
    }
}

