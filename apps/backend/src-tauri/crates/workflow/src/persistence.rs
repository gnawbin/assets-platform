//! bpmninja `WorkflowPersistence` 与 adk-graph `Checkpointer` 的 PostgreSQL 实现。
//!
//! 引擎对象统一存放于 `bpmn_engine_object(kind, id, data JSONB)`，
//! 通过 `kind` 区分 token / instance / definition / 待办任务 / 定时器等；
//! 历史、归档实例、文件分别落到 `bpmn_engine_history`、`bpmn_engine_completed`、
//! `bpmn_engine_file`。所有 SQL 均绑定当前租户 schema，Agent 线程快照存放于
//! `workflow_agent_checkpoint`。

use adk_graph::checkpoint::{Checkpointer, RetentionPolicy};
use adk_graph::error::{GraphError, Result as GraphResult};
use adk_graph::state::Checkpoint;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use engine_core::domain::{EngineError, EngineResult, ProcessDefinition, Token};
use engine_core::history::HistoryEntry;
use engine_core::persistence::{
    BucketEntry, BucketEntryDetail, CompletedInstanceQuery, HistoryQuery, StorageInfo,
    WorkflowPersistence,
};
use engine_core::runtime::{
    PendingConditionalCatch, PendingMessageCatch, PendingServiceTask, PendingSignalCatch,
    PendingTimer, PendingUserTask, ProcessInstance,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

/// 引擎对象类别，对应 `bpmn_engine_object.kind`。
mod object_kind {
    pub const TOKEN: &str = "token";
    pub const INSTANCE: &str = "instance";
    pub const DEFINITION: &str = "definition";
    pub const USER_TASK: &str = "user_task";
    pub const SERVICE_TASK: &str = "service_task";
    pub const TIMER: &str = "timer";
    pub const MESSAGE_CATCH: &str = "message_catch";
    pub const SIGNAL_CATCH: &str = "signal_catch";
    pub const CONDITIONAL_CATCH: &str = "conditional_catch";
}

/// BPMN XML 在 `bpmn_engine_file` 中的键前缀。
const BPMN_FILE_PREFIX: &str = "bpmn/";

fn persistence_error(context: &str, error: impl std::fmt::Display) -> EngineError {
    EngineError::PersistenceError(format!("{context}: {error}"))
}

fn parse_uuid(value: &str) -> Option<Uuid> {
    Uuid::parse_str(value.trim()).ok()
}

/// 从序列化后的对象里抽取需要建索引的列（topic / worker / 锁过期时间）。
fn object_meta(payload: &Value) -> (Option<String>, Option<String>, Option<DateTime<Utc>>) {
    let topic = payload
        .get("topic")
        .and_then(Value::as_str)
        .map(str::to_string);
    let worker_id = payload
        .get("worker_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let lock_expiration = payload
        .get("lock_expiration")
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc));
    (topic, worker_id, lock_expiration)
}

fn object_string(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(Value::as_str).map(str::to_string)
}

fn object_uuid(payload: &Value, key: &str) -> Option<Uuid> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .and_then(parse_uuid)
}

/// 监控接口的 bucket 名称 → 引擎对象类别。
fn bucket_kind(bucket_name: &str) -> Option<&'static str> {
    match bucket_name {
        "instances" | "instance" => Some(object_kind::INSTANCE),
        "definitions" | "definition" => Some(object_kind::DEFINITION),
        "tokens" | "token" => Some(object_kind::TOKEN),
        "user_tasks" | "user_task" => Some(object_kind::USER_TASK),
        "service_tasks" | "service_task" => Some(object_kind::SERVICE_TASK),
        "timers" | "timer" => Some(object_kind::TIMER),
        "message_catches" | "message_catch" => Some(object_kind::MESSAGE_CATCH),
        "signal_catches" | "signal_catch" => Some(object_kind::SIGNAL_CATCH),
        "conditional_catches" | "conditional_catch" => Some(object_kind::CONDITIONAL_CATCH),
        _ => None,
    }
}

/// 基于 PostgreSQL 的 bpmninja 持久化实现（按租户 schema 隔离）。
#[derive(Debug, Clone)]
pub struct PostgresWorkflowPersistence {
    schema: String,
}

impl PostgresWorkflowPersistence {
    /// 创建指定租户 schema 的持久化实例。
    pub fn new(schema: impl Into<String>) -> Self {
        Self {
            schema: schema.into(),
        }
    }

    /// 当前实例绑定的 schema 名称。
    pub fn schema(&self) -> &str {
        &self.schema
    }
}


impl PostgresWorkflowPersistence {
    /// 写入或更新一个引擎对象。
    async fn upsert<T: Serialize>(
        &self,
        kind: &str,
        id: Uuid,
        instance_id: Option<Uuid>,
        data: &T,
    ) -> EngineResult<()> {
        let payload =
            serde_json::to_value(data).map_err(|e| persistence_error("序列化引擎对象失败", e))?;
        let (topic, worker_id, lock_expiration) = object_meta(&payload);
        let definition_key = object_uuid(&payload, "definition_key");
        let business_key = object_string(&payload, "business_key");
        let schema = self.schema.clone();
        let kind = kind.to_string();

        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| persistence_error("获取数据库连接失败", e))?;
            let sql = format!(
                "INSERT INTO {}bpmn_engine_object
                     (kind, id, instance_id, definition_key, business_key, topic, worker_id, lock_expiration, data)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                 ON CONFLICT (kind, id) DO UPDATE
                 SET instance_id = EXCLUDED.instance_id,
                     definition_key = EXCLUDED.definition_key,
                     business_key = EXCLUDED.business_key,
                     topic = EXCLUDED.topic,
                     worker_id = EXCLUDED.worker_id,
                     lock_expiration = EXCLUDED.lock_expiration,
                     data = EXCLUDED.data,
                     updated_at = NOW()",
                database::schema_prefix()
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&kind)
                .bind(id)
                .bind(instance_id)
                .bind(definition_key)
                .bind(business_key)
                .bind(topic)
                .bind(worker_id)
                .bind(lock_expiration)
                .bind(payload)
                .execute(&pool)
                .await
                .map_err(|e| persistence_error("写入引擎对象失败", e))?;
            Ok(())
        })
        .await
    }

    /// 读取单个引擎对象。
    ///
    /// 供按 id 精确读取（如审批前回填实例）使用；当前调用方多走 `list_*` 批量接口。
    #[allow(dead_code)]
    async fn load<T: DeserializeOwned>(&self, kind: &str, id: Uuid) -> EngineResult<Option<T>> {
        let schema = self.schema.clone();
        let kind = kind.to_string();
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| persistence_error("获取数据库连接失败", e))?;
            let sql = format!(
                "SELECT data FROM {}bpmn_engine_object WHERE kind = $1 AND id = $2",
                database::schema_prefix()
            );
            let row = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&kind)
                .bind(id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| persistence_error("读取引擎对象失败", e))?;
            match row {
                Some(row) => {
                    let value: Value = row
                        .try_get("data")
                        .map_err(|e| persistence_error("解析引擎对象失败", e))?;
                    serde_json::from_value(value)
                        .map(Some)
                        .map_err(|e| persistence_error("反序列化引擎对象失败", e))
                }
                None => Ok(None),
            }
        })
        .await
    }

    /// 读取某一类别的全部引擎对象。
    async fn list_all<T: DeserializeOwned>(&self, kind: &str) -> EngineResult<Vec<T>> {
        let schema = self.schema.clone();
        let kind = kind.to_string();
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| persistence_error("获取数据库连接失败", e))?;
            let sql = format!(
                "SELECT data FROM {}bpmn_engine_object WHERE kind = $1",
                database::schema_prefix()
            );
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&kind)
                .fetch_all(&pool)
                .await
                .map_err(|e| persistence_error("读取引擎对象列表失败", e))?;
            rows.iter()
                .map(|row| {
                    let value: Value = row
                        .try_get("data")
                        .map_err(|e| persistence_error("解析引擎对象失败", e))?;
                    serde_json::from_value(value)
                        .map_err(|e| persistence_error("反序列化引擎对象失败", e))
                })
                .collect()
        })
        .await
    }

    /// 读取某个流程实例下的某一类别对象。
    async fn list_by_instance<T: DeserializeOwned>(
        &self,
        kind: &str,
        instance_id: Uuid,
    ) -> EngineResult<Vec<T>> {
        let schema = self.schema.clone();
        let kind = kind.to_string();
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| persistence_error("获取数据库连接失败", e))?;
            let sql = format!(
                "SELECT data FROM {}bpmn_engine_object WHERE kind = $1 AND instance_id = $2",
                database::schema_prefix()
            );
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&kind)
                .bind(instance_id)
                .fetch_all(&pool)
                .await
                .map_err(|e| persistence_error("读取引擎对象列表失败", e))?;
            rows.iter()
                .map(|row| {
                    let value: Value = row
                        .try_get("data")
                        .map_err(|e| persistence_error("解析引擎对象失败", e))?;
                    serde_json::from_value(value)
                        .map_err(|e| persistence_error("反序列化引擎对象失败", e))
                })
                .collect()
        })
        .await
    }

    /// 删除单个引擎对象。
    async fn remove(&self, kind: &str, id: Uuid) -> EngineResult<()> {
        let schema = self.schema.clone();
        let kind = kind.to_string();
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| persistence_error("获取数据库连接失败", e))?;
            let sql = format!(
                "DELETE FROM {}bpmn_engine_object WHERE kind = $1 AND id = $2",
                database::schema_prefix()
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&kind)
                .bind(id)
                .execute(&pool)
                .await
                .map_err(|e| persistence_error("删除引擎对象失败", e))?;
            Ok(())
        })
        .await
    }
}

#[async_trait]
impl WorkflowPersistence for PostgresWorkflowPersistence {
    async fn save_token(&self, instance_id: Uuid, token: &Token) -> EngineResult<()> {
        self.upsert(object_kind::TOKEN, token.id, Some(instance_id), token)
            .await
    }

    async fn load_tokens(&self, instance_id: Uuid) -> EngineResult<Vec<Token>> {
        self.list_by_instance(object_kind::TOKEN, instance_id).await
    }

    async fn delete_token(&self, _instance_id: Uuid, token_id: Uuid) -> EngineResult<()> {
        self.remove(object_kind::TOKEN, token_id).await
    }

    async fn save_instance(&self, instance: &ProcessInstance) -> EngineResult<()> {
        self.upsert(object_kind::INSTANCE, instance.id, Some(instance.id), instance)
            .await
    }

    async fn list_instances(&self) -> EngineResult<Vec<ProcessInstance>> {
        self.list_all(object_kind::INSTANCE).await
    }

    async fn delete_instance(&self, id: &str) -> EngineResult<()> {
        let Some(instance_id) = parse_uuid(id) else {
            return Ok(());
        };
        let schema = self.schema.clone();
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| persistence_error("获取数据库连接失败", e))?;
            let prefix = database::schema_prefix();
            let sql = format!(
                "DELETE FROM {}bpmn_engine_object WHERE id = $1 OR instance_id = $1",
                prefix
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(instance_id)
                .execute(&pool)
                .await
                .map_err(|e| persistence_error("删除流程实例失败", e))?;
            let sql = format!(
                "DELETE FROM {}bpmn_engine_completed WHERE instance_id = $1",
                prefix
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(instance_id)
                .execute(&pool)
                .await
                .map_err(|e| persistence_error("删除归档实例失败", e))?;
            Ok(())
        })
        .await
    }

    async fn save_definition(&self, definition: &ProcessDefinition) -> EngineResult<()> {
        self.upsert(object_kind::DEFINITION, definition.key, None, definition)
            .await
    }

    async fn list_definitions(&self) -> EngineResult<Vec<ProcessDefinition>> {
        self.list_all(object_kind::DEFINITION).await
    }

    async fn delete_definition(&self, key: &str) -> EngineResult<()> {
        if let Some(def_key) = parse_uuid(key) {
            return self.remove(object_kind::DEFINITION, def_key).await;
        }
        let schema = self.schema.clone();
        let key = key.to_string();
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| persistence_error("获取数据库连接失败", e))?;
            let sql = format!(
                "DELETE FROM {}bpmn_engine_object WHERE kind = $1 AND data ->> 'id' = $2",
                database::schema_prefix()
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(object_kind::DEFINITION)
                .bind(&key)
                .execute(&pool)
                .await
                .map_err(|e| persistence_error("删除流程定义失败", e))?;
            Ok(())
        })
        .await
    }

    async fn save_user_task(&self, task: &PendingUserTask) -> EngineResult<()> {
        self.upsert(
            object_kind::USER_TASK,
            task.task_id,
            Some(task.instance_id),
            task,
        )
        .await
    }

    async fn delete_user_task(&self, task_id: Uuid) -> EngineResult<()> {
        self.remove(object_kind::USER_TASK, task_id).await
    }

    async fn list_user_tasks(&self) -> EngineResult<Vec<PendingUserTask>> {
        self.list_all(object_kind::USER_TASK).await
    }

    async fn save_service_task(&self, task: &PendingServiceTask) -> EngineResult<()> {
        self.upsert(
            object_kind::SERVICE_TASK,
            task.id,
            Some(task.instance_id),
            task,
        )
        .await
    }

    async fn delete_service_task(&self, task_id: Uuid) -> EngineResult<()> {
        self.remove(object_kind::SERVICE_TASK, task_id).await
    }

    async fn list_service_tasks(&self) -> EngineResult<Vec<PendingServiceTask>> {
        self.list_all(object_kind::SERVICE_TASK).await
    }

    async fn save_timer(&self, timer: &PendingTimer) -> EngineResult<()> {
        self.upsert(
            object_kind::TIMER,
            timer.id,
            Some(timer.instance_id),
            timer,
        )
        .await
    }

    async fn delete_timer(&self, timer_id: Uuid) -> EngineResult<()> {
        self.remove(object_kind::TIMER, timer_id).await
    }

    async fn list_timers(&self) -> EngineResult<Vec<PendingTimer>> {
        self.list_all(object_kind::TIMER).await
    }

    async fn save_message_catch(&self, catch: &PendingMessageCatch) -> EngineResult<()> {
        self.upsert(
            object_kind::MESSAGE_CATCH,
            catch.id,
            Some(catch.instance_id),
            catch,
        )
        .await
    }

    async fn delete_message_catch(&self, catch_id: Uuid) -> EngineResult<()> {
        self.remove(object_kind::MESSAGE_CATCH, catch_id).await
    }

    async fn list_message_catches(&self) -> EngineResult<Vec<PendingMessageCatch>> {
        self.list_all(object_kind::MESSAGE_CATCH).await
    }

    async fn save_signal_catch(&self, catch: &PendingSignalCatch) -> EngineResult<()> {
        self.upsert(
            object_kind::SIGNAL_CATCH,
            catch.id,
            Some(catch.instance_id),
            catch,
        )
        .await
    }

    async fn delete_signal_catch(&self, catch_id: Uuid) -> EngineResult<()> {
        self.remove(object_kind::SIGNAL_CATCH, catch_id).await
    }

    async fn list_signal_catches(&self) -> EngineResult<Vec<PendingSignalCatch>> {
        self.list_all(object_kind::SIGNAL_CATCH).await
    }

    async fn save_conditional_catch(&self, catch: &PendingConditionalCatch) -> EngineResult<()> {
        self.upsert(
            object_kind::CONDITIONAL_CATCH,
            catch.id,
            Some(catch.instance_id),
            catch,
        )
        .await
    }

    async fn delete_conditional_catch(&self, catch_id: Uuid) -> EngineResult<()> {
        self.remove(object_kind::CONDITIONAL_CATCH, catch_id).await
    }

    async fn list_conditional_catches(&self) -> EngineResult<Vec<PendingConditionalCatch>> {
        self.list_all(object_kind::CONDITIONAL_CATCH).await
    }

    async fn save_file(&self, object_key: &str, data: &[u8]) -> EngineResult<()> {
        let schema = self.schema.clone();
        let object_key = object_key.to_string();
        let data = data.to_vec();
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| persistence_error("获取数据库连接失败", e))?;
            let sql = format!(
                "INSERT INTO {}bpmn_engine_file (object_key, data) VALUES ($1, $2)
                 ON CONFLICT (object_key) DO UPDATE SET data = EXCLUDED.data",
                database::schema_prefix()
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&object_key)
                .bind(data)
                .execute(&pool)
                .await
                .map_err(|e| persistence_error("写入引擎文件失败", e))?;
            Ok(())
        })
        .await
    }

    async fn load_file(&self, object_key: &str) -> EngineResult<Vec<u8>> {
        let schema = self.schema.clone();
        let object_key = object_key.to_string();
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| persistence_error("获取数据库连接失败", e))?;
            let sql = format!(
                "SELECT data FROM {}bpmn_engine_file WHERE object_key = $1",
                database::schema_prefix()
            );
            let row = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&object_key)
                .fetch_optional(&pool)
                .await
                .map_err(|e| persistence_error("读取引擎文件失败", e))?;
            match row {
                Some(row) => row
                    .try_get::<Vec<u8>, _>("data")
                    .map_err(|e| persistence_error("解析引擎文件失败", e)),
                None => Err(EngineError::PersistenceError(format!(
                    "引擎文件不存在: {object_key}"
                ))),
            }
        })
        .await
    }

    async fn delete_file(&self, object_key: &str) -> EngineResult<()> {
        let schema = self.schema.clone();
        let object_key = object_key.to_string();
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| persistence_error("获取数据库连接失败", e))?;
            let sql = format!(
                "DELETE FROM {}bpmn_engine_file WHERE object_key = $1",
                database::schema_prefix()
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&object_key)
                .execute(&pool)
                .await
                .map_err(|e| persistence_error("删除引擎文件失败", e))?;
            Ok(())
        })
        .await
    }

    async fn save_bpmn_xml(&self, definition_key: &str, xml: &str) -> EngineResult<()> {
        let object_key = format!("{BPMN_FILE_PREFIX}{definition_key}.bpmn");
        self.save_file(&object_key, xml.as_bytes()).await
    }

    async fn load_bpmn_xml(&self, definition_key: &str) -> EngineResult<String> {
        let object_key = format!("{BPMN_FILE_PREFIX}{definition_key}.bpmn");
        let data = self.load_file(&object_key).await?;
        String::from_utf8(data)
            .map_err(|e| persistence_error("解析 BPMN XML 失败", e))
    }

    async fn list_bpmn_xml_ids(&self) -> EngineResult<Vec<String>> {
        let schema = self.schema.clone();
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| persistence_error("获取数据库连接失败", e))?;
            let sql = format!(
                "SELECT object_key FROM {}bpmn_engine_file WHERE object_key LIKE 'bpmn/%'",
                database::schema_prefix()
            );
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .fetch_all(&pool)
                .await
                .map_err(|e| persistence_error("读取 BPMN 文件列表失败", e))?;
            rows.iter()
                .map(|row| {
                    let key: String = row
                        .try_get("object_key")
                        .map_err(|e| persistence_error("解析 BPMN 文件键失败", e))?;
                    Ok(key
                        .trim_start_matches(BPMN_FILE_PREFIX)
                        .trim_end_matches(".bpmn")
                        .to_string())
                })
                .collect()
        })
        .await
    }

    async fn get_storage_info(&self) -> EngineResult<Option<StorageInfo>> {
        Ok(None)
    }

    async fn append_history_entry(&self, entry: &HistoryEntry) -> EngineResult<()> {
        let payload =
            serde_json::to_value(entry).map_err(|e| persistence_error("序列化历史记录失败", e))?;
        let event_type = serde_json::to_value(&entry.event_type)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| format!("{:?}", entry.event_type));
        let schema = self.schema.clone();
        let id = entry.id;
        let instance_id = entry.instance_id;
        let node_id = entry.node_id.clone();
        let timestamp = entry.timestamp;
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| persistence_error("获取数据库连接失败", e))?;
            let sql = format!(
                "INSERT INTO {}bpmn_engine_history (id, instance_id, event_type, node_id, timestamp, data)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (id) DO NOTHING",
                database::schema_prefix()
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(id)
                .bind(instance_id)
                .bind(event_type)
                .bind(node_id)
                .bind(timestamp)
                .bind(payload)
                .execute(&pool)
                .await
                .map_err(|e| persistence_error("写入历史记录失败", e))?;
            Ok(())
        })
        .await
    }

    async fn query_history(&self, query: HistoryQuery) -> EngineResult<Vec<HistoryEntry>> {
        let schema = self.schema.clone();
        let instance_id = query.instance_id;
        let from = query.from;
        let to = query.to;
        let loaded: EngineResult<Vec<HistoryEntry>> = database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| persistence_error("获取数据库连接失败", e))?;
            let sql = format!(
                "SELECT data FROM {}bpmn_engine_history
                 WHERE instance_id = $1
                   AND ($2::timestamptz IS NULL OR timestamp >= $2)
                   AND ($3::timestamptz IS NULL OR timestamp <= $3)
                 ORDER BY timestamp ASC",
                database::schema_prefix()
            );
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(instance_id)
                .bind(from)
                .bind(to)
                .fetch_all(&pool)
                .await
                .map_err(|e| persistence_error("读取历史记录失败", e))?;
            rows.iter()
                .map(|row| {
                    let value: Value = row
                        .try_get("data")
                        .map_err(|e| persistence_error("解析历史记录失败", e))?;
                    serde_json::from_value(value)
                        .map_err(|e| persistence_error("反序列化历史记录失败", e))
                })
                .collect()
        })
        .await;

        let mut entries = loaded?;
        entries.retain(|entry| {
            query
                .event_types
                .as_ref()
                .map(|types| types.iter().any(|item| item == &entry.event_type))
                .unwrap_or(true)
                && query
                    .node_id
                    .as_ref()
                    .map(|node| entry.node_id.as_deref() == Some(node.as_str()))
                    .unwrap_or(true)
                && query
                    .actor_type
                    .as_ref()
                    .map(|actor| actor == &entry.actor_type)
                    .unwrap_or(true)
        });
        let offset = query.offset.unwrap_or(0);
        if offset > 0 {
            entries = entries.into_iter().skip(offset).collect();
        }
        if let Some(limit) = query.limit {
            entries.truncate(limit);
        }
        Ok(entries)
    }

    async fn save_completed_instance(&self, instance: &ProcessInstance) -> EngineResult<()> {
        let payload = serde_json::to_value(instance)
            .map_err(|e| persistence_error("序列化归档实例失败", e))?;
        let schema = self.schema.clone();
        let instance_id = instance.id;
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| persistence_error("获取数据库连接失败", e))?;
            let sql = format!(
                "INSERT INTO {}bpmn_engine_completed (instance_id, data) VALUES ($1, $2)
                 ON CONFLICT (instance_id) DO UPDATE SET data = EXCLUDED.data",
                database::schema_prefix()
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(instance_id)
                .bind(payload)
                .execute(&pool)
                .await
                .map_err(|e| persistence_error("写入归档实例失败", e))?;
            Ok(())
        })
        .await
    }

    async fn query_completed_instances(
        &self,
        query: CompletedInstanceQuery,
    ) -> EngineResult<Vec<ProcessInstance>> {
        let schema = self.schema.clone();
        let loaded: EngineResult<Vec<ProcessInstance>> =
            database::with_schema(schema, async move {
                let pool = database::get_write_pool()
                    .map_err(|e| persistence_error("获取数据库连接失败", e))?;
                let sql = format!(
                    "SELECT data FROM {}bpmn_engine_completed",
                    database::schema_prefix()
                );
                let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                    .fetch_all(&pool)
                    .await
                    .map_err(|e| persistence_error("读取归档实例失败", e))?;
                rows.iter()
                    .map(|row| {
                        let value: Value = row
                            .try_get("data")
                            .map_err(|e| persistence_error("解析归档实例失败", e))?;
                        serde_json::from_value(value)
                            .map_err(|e| persistence_error("反序列化归档实例失败", e))
                    })
                    .collect()
            })
            .await;

        let mut instances = loaded?;
        instances.retain(|instance| {
            query
                .definition_key
                .map(|key| instance.definition_key == key)
                .unwrap_or(true)
                && query
                    .business_key
                    .as_ref()
                    .map(|key| &instance.business_key == key)
                    .unwrap_or(true)
                && query
                    .from
                    .map(|from| instance.completed_at.map(|at| at >= from).unwrap_or(false))
                    .unwrap_or(true)
                && query
                    .to
                    .map(|to| instance.completed_at.map(|at| at <= to).unwrap_or(false))
                    .unwrap_or(true)
                && query
                    .state_filter
                    .as_ref()
                    .map(|filter| state_matches(instance, filter))
                    .unwrap_or(true)
        });
        let offset = query.offset.unwrap_or(0);
        if offset > 0 {
            instances = instances.into_iter().skip(offset).collect();
        }
        if let Some(limit) = query.limit {
            instances.truncate(limit);
        }
        Ok(instances)
    }

    async fn get_completed_instance(&self, id: &str) -> EngineResult<Option<ProcessInstance>> {
        let Some(instance_id) = parse_uuid(id) else {
            return Ok(None);
        };
        let schema = self.schema.clone();
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| persistence_error("获取数据库连接失败", e))?;
            let sql = format!(
                "SELECT data FROM {}bpmn_engine_completed WHERE instance_id = $1",
                database::schema_prefix()
            );
            let row = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(instance_id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| persistence_error("读取归档实例失败", e))?;
            match row {
                Some(row) => {
                    let value: Value = row
                        .try_get("data")
                        .map_err(|e| persistence_error("解析归档实例失败", e))?;
                    serde_json::from_value(value)
                        .map(Some)
                        .map_err(|e| persistence_error("反序列化归档实例失败", e))
                }
                None => Ok(None),
            }
        })
        .await
    }

    async fn get_bucket_entries(
        &self,
        bucket_name: &str,
        offset: usize,
        limit: usize,
    ) -> EngineResult<Vec<BucketEntry>> {
        let schema = self.schema.clone();
        let bucket = bucket_name.to_string();
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| persistence_error("获取数据库连接失败", e))?;
            let prefix = database::schema_prefix();
            if bucket == "files" || bucket == "bpmn_xml" {
                let condition = if bucket == "bpmn_xml" {
                    "WHERE object_key LIKE 'bpmn/%'"
                } else {
                    ""
                };
                let sql = format!(
                    "SELECT object_key, octet_length(data) AS size FROM {prefix}bpmn_engine_file
                     {condition} ORDER BY object_key LIMIT $1 OFFSET $2"
                );
                let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                    .bind(limit as i64)
                    .bind(offset as i64)
                    .fetch_all(&pool)
                    .await
                    .map_err(|e| persistence_error("读取存储桶失败", e))?;
                return rows
                    .iter()
                    .map(|row| {
                        Ok(BucketEntry {
                            key: row
                                .try_get::<String, _>("object_key")
                                .map_err(|e| persistence_error("解析存储桶键失败", e))?,
                            size_bytes: row
                                .try_get::<Option<i32>, _>("size")
                                .ok()
                                .flatten()
                                .map(|size| size.max(0) as u64),
                            created_at: None,
                        })
                    })
                    .collect();
            }
            let Some(kind) = bucket_kind(&bucket) else {
                return Ok(Vec::new());
            };
            let sql = format!(
                "SELECT id, data, updated_at FROM {prefix}bpmn_engine_object
                 WHERE kind = $1 ORDER BY updated_at DESC LIMIT $2 OFFSET $3"
            );
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(kind)
                .bind(limit as i64)
                .bind(offset as i64)
                .fetch_all(&pool)
                .await
                .map_err(|e| persistence_error("读取存储桶失败", e))?;
            rows.iter()
                .map(|row| {
                    let value: Value = row
                        .try_get("data")
                        .map_err(|e| persistence_error("解析存储桶条目失败", e))?;
                    Ok(BucketEntry {
                        key: row
                            .try_get::<Uuid, _>("id")
                            .map_err(|e| persistence_error("解析存储桶键失败", e))?
                            .to_string(),
                        size_bytes: Some(
                            serde_json::to_string(&value)
                                .map(|text| text.len() as u64)
                                .unwrap_or(0),
                        ),
                        created_at: row
                            .try_get::<Option<DateTime<Utc>>, _>("updated_at")
                            .ok()
                            .flatten(),
                    })
                })
                .collect()
        })
        .await
    }

    async fn get_bucket_entry_detail(
        &self,
        bucket_name: &str,
        key: &str,
    ) -> EngineResult<BucketEntryDetail> {
        let schema = self.schema.clone();
        let bucket = bucket_name.to_string();
        let key = key.to_string();
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| persistence_error("获取数据库连接失败", e))?;
            let prefix = database::schema_prefix();
            if bucket == "files" || bucket == "bpmn_xml" {
                let sql = format!("SELECT data FROM {prefix}bpmn_engine_file WHERE object_key = $1");
                let row = sqlx::query(sqlx::AssertSqlSafe(sql))
                    .bind(&key)
                    .fetch_optional(&pool)
                    .await
                    .map_err(|e| persistence_error("读取存储桶条目失败", e))?;
                let Some(row) = row else {
                    return Err(EngineError::PersistenceError(format!("文件不存在: {key}")));
                };
                let raw: Vec<u8> = row
                    .try_get("data")
                    .map_err(|e| persistence_error("解析文件内容失败", e))?;
                return Ok(BucketEntryDetail {
                    key,
                    data: String::from_utf8_lossy(&raw).to_string(),
                    encoding: "utf8".to_string(),
                });
            }
            let Some(kind) = bucket_kind(&bucket) else {
                return Err(EngineError::PersistenceError(format!(
                    "未知存储桶: {bucket}"
                )));
            };
            let Some(id) = parse_uuid(&key) else {
                return Err(EngineError::PersistenceError(format!("无效的对象键: {key}")));
            };
            let sql =
                format!("SELECT data FROM {prefix}bpmn_engine_object WHERE kind = $1 AND id = $2");
            let row = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(kind)
                .bind(id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| persistence_error("读取存储桶条目失败", e))?;
            let Some(row) = row else {
                return Err(EngineError::PersistenceError(format!("对象不存在: {key}")));
            };
            let value: Value = row
                .try_get("data")
                .map_err(|e| persistence_error("解析存储桶条目失败", e))?;
            Ok(BucketEntryDetail {
                key,
                data: serde_json::to_string_pretty(&value)
                    .map_err(|e| persistence_error("格式化存储桶条目失败", e))?,
                encoding: "utf8".to_string(),
            })
        })
        .await
    }
}

/// 判断归档实例是否匹配 `completed` / `error` 过滤条件。
fn state_matches(instance: &ProcessInstance, filter: &str) -> bool {
    let state = serde_json::to_string(&instance.state)
        .unwrap_or_default()
        .to_lowercase();
    match filter.to_lowercase().as_str() {
        "completed" => state.contains("completed"),
        "error" | "failed" => state.contains("error") || state.contains("failed"),
        _ => true,
    }
}

fn graph_error(context: &str, error: impl std::fmt::Display) -> GraphError {
    GraphError::CheckpointError(format!("{context}: {error}"))
}

/// 基于 PostgreSQL 的 adk-graph 检查点存储（按租户 schema 隔离）。
///
/// 每个 Agent 线程的快照落在 `workflow_agent_checkpoint`，
/// 进程重启后 `load(thread_id)` 即可恢复中断现场。
#[derive(Debug, Clone)]
pub struct PostgresCheckpointer {
    schema: String,
}

impl PostgresCheckpointer {
    /// 创建指定租户 schema 的检查点存储。
    pub fn new(schema: impl Into<String>) -> Self {
        Self {
            schema: schema.into(),
        }
    }

    /// 当前实例绑定的 schema 名称。
    pub fn schema(&self) -> &str {
        &self.schema
    }

    async fn query_checkpoints(
        &self,
        sql_tail: &'static str,
        value: String,
    ) -> GraphResult<Vec<Checkpoint>> {
        let schema = self.schema.clone();
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| graph_error("获取数据库连接失败", e))?;
            let sql = format!(
                "SELECT data FROM {}workflow_agent_checkpoint {sql_tail}",
                database::schema_prefix()
            );
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&value)
                .fetch_all(&pool)
                .await
                .map_err(|e| graph_error("读取检查点失败", e))?;
            rows.iter()
                .map(|row| {
                    let payload: Value = row
                        .try_get("data")
                        .map_err(|e| graph_error("解析检查点失败", e))?;
                    serde_json::from_value(payload)
                        .map_err(|e| graph_error("反序列化检查点失败", e))
                })
                .collect()
        })
        .await
    }

    async fn delete_by_checkpoint_id(&self, checkpoint_id: &str) -> GraphResult<()> {
        let schema = self.schema.clone();
        let checkpoint_id = checkpoint_id.to_string();
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| graph_error("获取数据库连接失败", e))?;
            let sql = format!(
                "DELETE FROM {}workflow_agent_checkpoint WHERE checkpoint_id = $1",
                database::schema_prefix()
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&checkpoint_id)
                .execute(&pool)
                .await
                .map_err(|e| graph_error("删除检查点失败", e))?;
            Ok(())
        })
        .await
    }
}

#[async_trait]
impl Checkpointer for PostgresCheckpointer {
    async fn save(&self, checkpoint: &Checkpoint) -> GraphResult<String> {
        let payload =
            serde_json::to_value(checkpoint).map_err(|e| graph_error("序列化检查点失败", e))?;
        let schema = self.schema.clone();
        let thread_id = checkpoint.thread_id.clone();
        let checkpoint_id = checkpoint.checkpoint_id.clone();
        let created_at = checkpoint.created_at;
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| graph_error("获取数据库连接失败", e))?;
            let sql = format!(
                "INSERT INTO {}workflow_agent_checkpoint (thread_id, checkpoint_id, data, created_at)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (thread_id, checkpoint_id) DO UPDATE
                 SET data = EXCLUDED.data, created_at = EXCLUDED.created_at",
                database::schema_prefix()
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&thread_id)
                .bind(&checkpoint_id)
                .bind(payload)
                .bind(created_at)
                .execute(&pool)
                .await
                .map_err(|e| graph_error("写入检查点失败", e))?;
            Ok(checkpoint_id)
        })
        .await
    }

    async fn load(&self, thread_id: &str) -> GraphResult<Option<Checkpoint>> {
        let mut rows = self
            .query_checkpoints(
                "WHERE thread_id = $1 ORDER BY created_at DESC, checkpoint_id DESC LIMIT 1",
                thread_id.to_string(),
            )
            .await?;
        Ok(rows.pop())
    }

    async fn load_by_id(&self, checkpoint_id: &str) -> GraphResult<Option<Checkpoint>> {
        let mut rows = self
            .query_checkpoints(
                "WHERE checkpoint_id = $1 ORDER BY created_at DESC LIMIT 1",
                checkpoint_id.to_string(),
            )
            .await?;
        Ok(rows.pop())
    }

    async fn list(&self, thread_id: &str) -> GraphResult<Vec<Checkpoint>> {
        self.query_checkpoints(
            "WHERE thread_id = $1 ORDER BY created_at ASC, checkpoint_id ASC",
            thread_id.to_string(),
        )
        .await
    }

    async fn delete(&self, thread_id: &str) -> GraphResult<()> {
        let schema = self.schema.clone();
        let thread_id = thread_id.to_string();
        database::with_schema(schema, async move {
            let pool = database::get_write_pool()
                .map_err(|e| graph_error("获取数据库连接失败", e))?;
            let sql = format!(
                "DELETE FROM {}workflow_agent_checkpoint WHERE thread_id = $1",
                database::schema_prefix()
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(&thread_id)
                .execute(&pool)
                .await
                .map_err(|e| graph_error("删除检查点失败", e))?;
            Ok(())
        })
        .await
    }

    async fn prune(&self, thread_id: &str, policy: &RetentionPolicy) -> GraphResult<usize> {
        let checkpoints = self.list(thread_id).await?;
        let expired = policy.expired(&checkpoints);
        for checkpoint_id in &expired {
            self.delete_by_checkpoint_id(checkpoint_id).await?;
        }
        Ok(expired.len())
    }
}
