//! 工作流业务服务：定义 / 执行的 CRUD、启动、取消与审批。
//!
//! 所有 SQL 通过 `database::schema_prefix()` 绑定当前租户 schema；
//! 引擎相关的操作委托给 `WorkflowManager` 下发的租户运行时。
//! 读取也使用写连接池，避免主从延迟导致“刚保存就查不到 / 刚执行就读不到状态”。

use crate::bpmn::BpmnCompiler;
use crate::projection::{append_node_result, current_status, sync_execution};
use crate::service::WorkflowManager;
use crate::types::*;
use serde_json::{json, Value};
use sqlx::{postgres::PgRow, Row};
use std::collections::HashMap;
use uuid::Uuid;

/// 工作流定义与执行的服务入口。
pub struct WorkflowService;

/// 每页最大条数，防止前端一次拉全表。
const MAX_PAGE_SIZE: i64 = 200;

fn parse_id(id: &str, label: &str) -> Result<i64, String> {
    id.trim()
        .parse::<i64>()
        .map_err(|e| format!("无效的{label}: {id}（{e}）"))
}

fn pool() -> Result<sqlx::PgPool, String> {
    database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {e}"))
}

fn row_string(row: &PgRow, column: &str) -> Option<String> {
    row.try_get::<Option<String>, _>(column).ok().flatten()
}

fn row_time(row: &PgRow, column: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(column)
        .ok()
        .flatten()
}

/// 从 IR 提取节点类型摘要，与前端 `node_types` 展示保持一致。
fn derive_node_types(definition: &WorkflowDefinition) -> Vec<String> {
    let mut types: Vec<String> = Vec::new();
    for node in &definition.nodes {
        let mut value = node.node_type.clone();
        if let Some(config) = node.config.as_object() {
            if let Some(kind) = config.get("kind").and_then(Value::as_str) {
                value = kind.to_string();
            }
            if value == "skill" {
                if let Some(skill) = config.get("skill_id").and_then(Value::as_str) {
                    if !skill.is_empty() {
                        value = format!("skill:{skill}");
                    }
                }
            }
        }
        if !types.iter().any(|item| item == &value) {
            types.push(value);
        }
    }
    types
}

fn map_workflow_row(row: &PgRow) -> Result<StoredWorkflow, String> {
    let definition: Value = row
        .try_get("definition")
        .map_err(|e| format!("读取工作流定义失败: {e}"))?;
    let definition: WorkflowDefinition =
        serde_json::from_value(definition).map_err(|e| format!("解析工作流定义失败: {e}"))?;
    let created_at = row_time(row, "created_at").unwrap_or_else(chrono::Utc::now);
    let updated_at = row_time(row, "updated_at").unwrap_or(created_at);
    Ok(StoredWorkflow {
        id: row.try_get("id").map_err(|e| e.to_string())?,
        user_id: row.try_get::<i64, _>("user_id").unwrap_or(1),
        name: row.try_get::<String, _>("name").unwrap_or_default(),
        description: row_string(row, "description"),
        version: row_string(row, "version").unwrap_or_else(|| "1.0.0".to_string()),
        status: row_string(row, "status").unwrap_or_else(|| "draft".to_string()),
        definition,
        bpmn_xml: row_string(row, "bpmn_xml"),
        node_types: row
            .try_get::<Option<Vec<String>>, _>("node_types")
            .ok()
            .flatten()
            .unwrap_or_default(),
        engine_definition_key: row_string(row, "engine_definition_key"),
        engine_version: row
            .try_get::<Option<i32>, _>("engine_version")
            .ok()
            .flatten(),
        definition_hash: row_string(row, "definition_hash"),
        created_at,
        updated_at,
    })
}

fn map_execution_row(row: &PgRow) -> Result<WorkflowExecution, String> {
    let node_results: Option<Value> = row.try_get("node_results").ok().flatten();
    let node_results = node_results.and_then(|value| match value {
        Value::Array(items) => serde_json::from_value::<Vec<NodeExecutionResult>>(Value::Array(items))
            .map(|items| json!(items))
            .ok(),
        _ => None,
    });
    Ok(WorkflowExecution {
        id: row
            .try_get::<i64, _>("id")
            .map(|value| value.to_string())
            .unwrap_or_default(),
        workflow_id: row
            .try_get::<i64, _>("workflow_id")
            .map(|value| value.to_string())
            .unwrap_or_default(),
        workflow_name: row_string(row, "workflow_name"),
        trigger_type: row_string(row, "trigger_type").unwrap_or_else(|| "manual".to_string()),
        status: row_string(row, "status").unwrap_or_else(|| "running".to_string()),
        input_data: row.try_get::<Option<Value>, _>("input_data").ok().flatten(),
        result_data: row.try_get::<Option<Value>, _>("result_data").ok().flatten(),
        error_message: row_string(row, "error_message"),
        node_results,
        total_duration_ms: row
            .try_get::<Option<i32>, _>("total_duration_ms")
            .ok()
            .flatten(),
        total_tokens: row
            .try_get::<Option<i32>, _>("total_tokens")
            .ok()
            .flatten(),
        total_cost: row
            .try_get::<Option<f64>, _>("total_cost")
            .ok()
            .flatten(),
        created_at: row_time(row, "created_at").unwrap_or_else(chrono::Utc::now),
        finished_at: row_time(row, "finished_at"),
        engine_instance_id: row_string(row, "engine_instance_id"),
        engine_definition_key: row_string(row, "engine_definition_key"),
        current_node_id: row_string(row, "current_node_id"),
        agent_thread_id: row_string(row, "agent_thread_id"),
        waiting_task_id: row_string(row, "waiting_task_id"),
        interrupt_payload: row
            .try_get::<Option<Value>, _>("interrupt_payload")
            .ok()
            .flatten(),
        bpmn_status: row_string(row, "bpmn_status"),
    })
}

/// 执行记录查询列，`total_cost` 转换为 f64 便于前端直接使用。
const EXECUTION_COLUMNS: &str = "e.id, e.workflow_id, w.name AS workflow_name, \
    e.trigger_type, e.status, e.input_data, e.result_data, e.error_message, e.node_results, \
    e.total_duration_ms, e.total_tokens, e.total_cost::float8 AS total_cost, e.created_at, \
    e.finished_at, e.engine_instance_id::text AS engine_instance_id, \
    e.engine_definition_key::text AS engine_definition_key, e.current_node_id, \
    e.agent_thread_id, e.waiting_task_id, e.interrupt_payload, e.bpmn_status";

impl WorkflowService {
    /// 工作流列表（不含完整定义，避免大字段）。
    pub async fn list_workflows(
        keyword: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<WorkflowMeta>, String> {
        let pool = pool()?;
        let prefix = database::schema_prefix();
        let sql = format!(
            "SELECT id, name, description, version, status, node_types, use_count,
                    last_executed_at, created_at, updated_at
             FROM {}workflow
             WHERE deleted = 0
               AND ($1::text IS NULL OR name ILIKE '%' || $1 || '%'
                    OR COALESCE(description, '') ILIKE '%' || $1 || '%')
               AND ($2::text IS NULL OR status = $2)
             ORDER BY updated_at DESC, id DESC",
            prefix
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(keyword.map(str::trim).filter(|value| !value.is_empty()))
            .bind(status.map(str::trim).filter(|value| !value.is_empty()))
            .fetch_all(&pool)
            .await
            .map_err(|e| format!("查询工作流列表失败: {e}"))?;

        let mut list = Vec::with_capacity(rows.len());
        for row in &rows {
            list.push(WorkflowMeta {
                id: row
                    .try_get::<i64, _>("id")
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                name: row.try_get::<String, _>("name").unwrap_or_default(),
                description: row_string(row, "description"),
                version: row_string(row, "version").unwrap_or_else(|| "1.0.0".to_string()),
                status: row_string(row, "status").unwrap_or_else(|| "draft".to_string()),
                node_types: row
                    .try_get::<Option<Vec<String>>, _>("node_types")
                    .ok()
                    .flatten()
                    .unwrap_or_default(),
                use_count: row.try_get::<Option<i32>, _>("use_count").ok().flatten().unwrap_or(0),
                last_executed_at: row_time(row, "last_executed_at"),
                created_at: row_time(row, "created_at").unwrap_or_else(chrono::Utc::now),
                updated_at: row_time(row, "updated_at").unwrap_or_else(chrono::Utc::now),
            });
        }
        Ok(list)
    }

    /// 按 ID 读取完整定义。
    pub async fn get_workflow(workflow_id: &str) -> Result<Option<StoredWorkflow>, String> {
        let id = parse_id(workflow_id, "工作流ID")?;
        let pool = pool()?;
        let prefix = database::schema_prefix();
        let sql = format!(
            "SELECT id, user_id, name, description, version, status, definition, bpmn_xml,
                    node_types, engine_definition_key::text AS engine_definition_key,
                    engine_version, definition_hash, created_at, updated_at
             FROM {}workflow
             WHERE id = $1 AND deleted = 0",
            prefix
        );
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id)
            .fetch_optional(&pool)
            .await
            .map_err(|e| format!("查询工作流失败: {e}"))?;
        row.as_ref().map(map_workflow_row).transpose()
    }

    /// 保存工作流（无 `id` 时新建），返回 `(id, version)`。
    ///
    /// 引擎侧字段（definition_key / hash）不做重置：执行时会用新的
    /// `definition_hash` 判断是否需要重新部署，未变化即复用已有定义。
    pub async fn save_workflow(
        input: SaveWorkflowInput,
        user_id: i64,
    ) -> Result<(String, String), String> {
        if input.name.trim().is_empty() {
            return Err("工作流名称不能为空".to_string());
        }
        let pool = pool()?;
        let prefix = database::schema_prefix();

        let definition = serde_json::to_value(&input.definition)
            .map_err(|e| format!("序列化工作流定义失败: {e}"))?;
        let bpmn_xml = input
            .bpmn_xml
            .clone()
            .or_else(|| input.definition.bpmn_xml.clone());
        let node_types = input
            .node_types
            .clone()
            .unwrap_or_else(|| derive_node_types(&input.definition));
        let version = input
            .definition
            .version
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "1.0.0".to_string());
        let description = input
            .description
            .clone()
            .or_else(|| input.definition.description.clone());
        let status = input
            .status
            .clone()
            .filter(|value| !value.trim().is_empty());

        if let Some(id) = input.id.as_deref().filter(|value| !value.is_empty()) {
            let id = parse_id(id, "工作流ID")?;
            let sql = format!(
                "UPDATE {}workflow
                 SET name = $2, description = $3, definition = $4, node_types = $5,
                     status = COALESCE($6, status), bpmn_xml = COALESCE($7, bpmn_xml),
                     updated_at = NOW()
                 WHERE id = $1 AND deleted = 0
                 RETURNING version",
                prefix
            );
            let row = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(id)
                .bind(input.name.trim())
                .bind(description.clone())
                .bind(definition)
                .bind(node_types)
                .bind(status.clone())
                .bind(bpmn_xml)
                .fetch_optional(&pool)
                .await
                .map_err(|e| format!("更新工作流失败: {e}"))?;
            let Some(row) = row else {
                return Err(format!("工作流 {id} 不存在或已删除"));
            };
            let stored: Option<String> = row.try_get("version").ok();
            return Ok((id.to_string(), stored.unwrap_or(version)));
        }

        let sql = format!(
            "INSERT INTO {}workflow
                (user_id, name, description, version, definition, node_types, status,
                 bpmn_xml, use_count, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, COALESCE($7, 'draft'), $8, 0, NOW(), NOW())
             RETURNING id, version",
            prefix
        );
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(user_id)
            .bind(input.name.trim())
            .bind(description)
            .bind(version)
            .bind(definition)
            .bind(node_types)
            .bind(status)
            .bind(bpmn_xml)
            .fetch_one(&pool)
            .await
            .map_err(|e| format!("新建工作流失败: {e}"))?;
        let id: i64 = row.try_get("id").map_err(|e| e.to_string())?;
        let version: Option<String> = row.try_get("version").ok();
        tracing::info!("工作流已保存: id={}, name={}", id, input.name);
        Ok((
            id.to_string(),
            version.unwrap_or_else(|| "1.0.0".to_string()),
        ))
    }

    /// 软删除工作流定义。
    pub async fn delete_workflow(workflow_id: &str, _user_id: i64) -> Result<(), String> {
        let id = parse_id(workflow_id, "工作流ID")?;
        let pool = pool()?;
        let prefix = database::schema_prefix();
        let sql = format!(
            "UPDATE {}workflow SET deleted = 1, updated_at = NOW()
             WHERE id = $1 AND deleted = 0",
            prefix
        );
        let result = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id)
            .execute(&pool)
            .await
            .map_err(|e| format!("删除工作流失败: {e}"))?;
        if result.rows_affected() == 0 {
            return Err(format!("工作流 {id} 不存在或已删除"));
        }
        Ok(())
    }

    /// 执行工作流：解析 BPMN → 部署（hash 未变则复用）→ 启动实例 → 首次投影。
    pub async fn execute_workflow(
        input: ExecuteWorkflowInput,
        user_id: i64,
    ) -> Result<(String, String), String> {
        let workflow = match input.workflow_id.as_deref().filter(|value| !value.is_empty()) {
            Some(id) => Self::get_workflow(id)
                .await?
                .ok_or_else(|| format!("工作流 {id} 不存在或已删除"))?,
            None => {
                let definition = input
                    .definition
                    .clone()
                    .ok_or_else(|| "缺少 workflowId 或 definition".to_string())?;
                let name = if definition.name.trim().is_empty() {
                    "未命名工作流".to_string()
                } else {
                    definition.name.clone()
                };
                let save = SaveWorkflowInput {
                    id: None,
                    name,
                    description: definition.description.clone(),
                    bpmn_xml: definition.bpmn_xml.clone(),
                    definition,
                    status: Some("draft".to_string()),
                    node_types: None,
                };
                let (id, _) = Self::save_workflow(save, user_id).await?;
                Self::get_workflow(&id)
                    .await?
                    .ok_or_else(|| "保存工作流失败".to_string())?
            }
        };

        let definition = workflow.definition.clone();
        let xml = workflow
            .bpmn_xml
            .clone()
            .or_else(|| definition.bpmn_xml.clone())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "工作流缺少 BPMN XML，请在编辑器中保存后再执行".to_string())?;
        let compiled = BpmnCompiler::compile(&xml, &definition)?;

        let schema = database::current_schema_name();
        let manager = WorkflowManager::global()?;
        let runtime = manager.runtime(&schema).await?;
        let definition_key = Self::ensure_deployed(&runtime, &workflow, &compiled, &schema).await?;

        let pool = pool()?;
        let prefix = database::schema_prefix();
        let execution_id: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {}workflow_execution
                (workflow_id, user_id, trigger_type, input_data, status,
                 engine_definition_key, created_at, updated_at)
             VALUES ($1, $2, 'manual', $3, 'running', $4, NOW(), NOW())
             RETURNING id",
            prefix
        )))
        .bind(workflow.id)
        .bind(user_id)
        .bind(input.input_data.clone())
        .bind(definition_key)
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("创建执行记录失败: {e}"))?;

        let mut variables: HashMap<String, Value> = HashMap::new();
        match input.input_data.clone() {
            Some(Value::Object(map)) => {
                for (key, value) in map {
                    variables.insert(key, value);
                }
            }
            Some(other) => {
                variables.insert("input".to_string(), other);
            }
            None => {}
        }
        let snapshot = ExecutionSnapshot::from_definition(
            workflow.id.to_string(),
            execution_id.to_string(),
            workflow.name.clone(),
            &definition,
        );
        variables.insert(
            "__workflow".to_string(),
            serde_json::to_value(&snapshot).map_err(|e| format!("序列化执行快照失败: {e}"))?,
        );
        variables.insert("business_key".to_string(), json!(execution_id.to_string()));
        if let Some(title) = input.title.filter(|value| !value.trim().is_empty()) {
            variables.insert("title".to_string(), json!(title));
        }

        let instance_id = database::with_schema(schema.clone(), async {
            runtime
                .engine
                .start_instance_with_variables(definition_key, variables)
                .await
        })
        .await
        .map_err(|e| format!("启动流程实例失败: {e}"))?;

        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {}workflow_execution
             SET engine_instance_id = $2, engine_definition_key = $3, updated_at = NOW()
             WHERE id = $1",
            prefix
        )))
        .bind(execution_id)
        .bind(instance_id)
        .bind(definition_key)
        .execute(&pool)
        .await
        .map_err(|e| format!("更新执行记录失败: {e}"))?;

        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {}workflow
             SET use_count = use_count + 1, last_executed_at = NOW(), updated_at = NOW()
             WHERE id = $1",
            prefix
        )))
        .bind(workflow.id)
        .execute(&pool)
        .await
        .map_err(|e| format!("更新工作流统计失败: {e}"))?;

        let execution_id = execution_id.to_string();
        if let Err(error) = sync_execution(&execution_id).await {
            tracing::warn!(
                "首次投影执行状态失败: execution_id={}, error={}",
                execution_id,
                error
            );
        }
        let status = current_status(&execution_id)
            .await
            .unwrap_or_else(|_| "running".to_string());
        tracing::info!(
            "工作流已启动: workflow_id={}, execution_id={}, instance_id={}, status={}",
            workflow.id,
            execution_id,
            instance_id,
            status
        );
        Ok((execution_id, status))
    }

    /// 执行历史（按创建时间倒序）。
    pub async fn list_executions(
        workflow_id: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Vec<WorkflowExecution>, String> {
        let pool = pool()?;
        let prefix = database::schema_prefix();
        let page = page.max(1);
        let page_size = page_size.clamp(1, MAX_PAGE_SIZE);
        let offset = (page - 1) * page_size;
        let workflow_id = match workflow_id.filter(|value| !value.trim().is_empty()) {
            Some(id) => Some(parse_id(id, "工作流ID")?),
            None => None,
        };

        let sql = format!(
            "SELECT {columns}
             FROM {prefix}workflow_execution e
             LEFT JOIN {prefix}workflow w ON w.id = e.workflow_id
             WHERE e.deleted = 0 AND ($1::bigint IS NULL OR e.workflow_id = $1)
             ORDER BY e.created_at DESC, e.id DESC
             LIMIT $2 OFFSET $3",
            columns = EXECUTION_COLUMNS,
            prefix = prefix
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(workflow_id)
            .bind(page_size)
            .bind(offset)
            .fetch_all(&pool)
            .await
            .map_err(|e| format!("查询执行记录失败: {e}"))?;
        rows.iter().map(map_execution_row).collect()
    }

    /// 执行详情（含节点时间线与等待中的审批）。
    pub async fn get_execution(execution_id: &str) -> Result<Option<WorkflowExecution>, String> {
        let id = parse_id(execution_id, "执行记录ID")?;
        let pool = pool()?;
        let prefix = database::schema_prefix();
        let sql = format!(
            "SELECT {columns}
             FROM {prefix}workflow_execution e
             LEFT JOIN {prefix}workflow w ON w.id = e.workflow_id
             WHERE e.id = $1 AND e.deleted = 0",
            columns = EXECUTION_COLUMNS,
            prefix = prefix
        );
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id)
            .fetch_optional(&pool)
            .await
            .map_err(|e| format!("查询执行详情失败: {e}"))?;
        match row {
            Some(row) => {
                // 打开详情时顺带刷新一次投影，前端轮询即可拿到最新状态。
                if let Err(error) = sync_execution(execution_id).await {
                    tracing::debug!("刷新执行投影失败: {}", error);
                }
                map_execution_row(&row).map(Some)
            }
            None => Ok(None),
        }
    }

    /// 取消执行：停止引擎实例并标记为 cancelled。
    pub async fn cancel_execution(execution_id: &str, _user_id: i64) -> Result<(), String> {
        let id = parse_id(execution_id, "执行记录ID")?;
        let pool = pool()?;
        let prefix = database::schema_prefix();

        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT engine_instance_id::text AS engine_instance_id, status
             FROM {}workflow_execution WHERE id = $1 AND deleted = 0",
            prefix
        )))
        .bind(id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| format!("查询执行记录失败: {e}"))?;
        let Some(row) = row else {
            return Err(format!("执行记录 {id} 不存在"));
        };
        let status = row_string(&row, "status").unwrap_or_default();
        if matches!(status.as_str(), "success" | "failed" | "cancelled") {
            return Ok(());
        }

        if let Some(instance_id) = row_string(&row, "engine_instance_id")
            .and_then(|value| Uuid::parse_str(&value).ok())
        {
            let schema = database::current_schema_name();
            let manager = WorkflowManager::global()?;
            let runtime = manager.runtime(&schema).await?;
            let result = database::with_schema(schema.clone(), async {
                runtime.engine.delete_instance(instance_id).await
            })
            .await;
            if let Err(error) = result {
                tracing::warn!(
                    "停止引擎实例失败（可能已归档）: instance_id={}, error={}",
                    instance_id,
                    error
                );
            }
        }

        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {}workflow_execution
             SET status = 'cancelled', bpmn_status = 'cancelled',
                 finished_at = COALESCE(finished_at, NOW()), updated_at = NOW()
             WHERE id = $1 AND deleted = 0",
            prefix
        )))
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| format!("更新执行状态失败: {e}"))?;

        tracing::info!("工作流执行已取消: execution_id={}", id);
        Ok(())
    }

    /// 待办审批任务（来自引擎的 UserTask）。
    pub async fn list_pending_tasks(
        workflow_id: Option<&str>,
        execution_id: Option<&str>,
    ) -> Result<Vec<PendingWorkflowTask>, String> {
        let schema = database::current_schema_name();
        let manager = WorkflowManager::global()?;
        let runtime = manager.runtime(&schema).await?;
        let tasks = runtime.engine.get_pending_user_tasks_enriched().await;
        if tasks.is_empty() {
            return Ok(Vec::new());
        }

        let workflow_filter = match workflow_id.filter(|value| !value.trim().is_empty()) {
            Some(id) => Some(parse_id(id, "工作流ID")?),
            None => None,
        };
        let execution_filter = match execution_id.filter(|value| !value.trim().is_empty()) {
            Some(id) => Some(parse_id(id, "执行记录ID")?),
            None => None,
        };

        let pool = pool()?;
        let prefix = database::schema_prefix();
        let sql = format!(
            "SELECT e.id, e.workflow_id, w.name AS workflow_name
             FROM {prefix}workflow_execution e
             LEFT JOIN {prefix}workflow w ON w.id = e.workflow_id
             WHERE e.engine_instance_id = $1 AND e.deleted = 0
             ORDER BY e.created_at DESC LIMIT 1",
            prefix = prefix
        );

        let mut list = Vec::new();
        for task in tasks {
            let row = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .bind(task.instance_id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| format!("查询审批任务所属执行记录失败: {e}"))?;
            let Some(row) = row else {
                continue;
            };
            let exec_id: i64 = row.try_get("id").unwrap_or_default();
            let wf_id: i64 = row.try_get("workflow_id").unwrap_or_default();
            if workflow_filter.is_some_and(|filter| filter != wf_id) {
                continue;
            }
            if execution_filter.is_some_and(|filter| filter != exec_id) {
                continue;
            }

            let interrupt_payload = runtime
                .engine
                .get_instance_details(task.instance_id)
                .await
                .ok()
                .and_then(|instance| instance.variables.get("agent_interrupt").cloned())
                .filter(|value| !value.is_null());

            list.push(PendingWorkflowTask {
                task_id: task.task_id.to_string(),
                instance_id: task.instance_id.to_string(),
                node_id: task.node_id.clone(),
                assignee: task.assignee.clone(),
                created_at: task.created_at,
                business_key: task.business_key.clone(),
                execution_id: Some(exec_id.to_string()),
                workflow_id: Some(wf_id.to_string()),
                workflow_name: row_string(&row, "workflow_name"),
                interrupt_payload,
            });
        }
        Ok(list)
    }

    /// 完成人工审批任务。
    ///
    /// `variables` 中的 `approved` / `comment` 会额外映射为
    /// `agent_approved` / `approval_comment` 流程变量，
    /// 供后续 `ai-agent-resume` 服务任务恢复 Agent 线程。
    pub async fn complete_user_task(
        task_id: &str,
        variables: Value,
        _user_id: i64,
    ) -> Result<(), String> {
        let task_uuid = Uuid::parse_str(task_id.trim())
            .map_err(|e| format!("无效的审批任务ID: {task_id}（{e}）"))?;
        let schema = database::current_schema_name();
        let manager = WorkflowManager::global()?;
        let runtime = manager.runtime(&schema).await?;

        let task = runtime
            .engine
            .get_pending_user_tasks_enriched()
            .await
            .into_iter()
            .find(|item| item.task_id == task_uuid)
            .ok_or_else(|| format!("审批任务 {task_id} 不存在或已处理"))?;

        let mut variables: HashMap<String, Value> = match variables {
            Value::Object(map) => map.into_iter().collect(),
            Value::Null => HashMap::new(),
            other => {
                let mut map = HashMap::new();
                map.insert("input".to_string(), other);
                map
            }
        };
        if let Some(approved) = variables.get("approved").and_then(Value::as_bool) {
            variables.insert("agent_approved".to_string(), json!(approved));
        }
        if let Some(comment) = variables.get("comment").and_then(Value::as_str) {
            variables.insert("approval_comment".to_string(), json!(comment));
        }
        let approval_output = json!({
            "approved": variables.get("approved").cloned().unwrap_or(Value::Null),
            "comment": variables.get("comment").cloned().unwrap_or(Value::Null),
        });

        database::with_schema(schema.clone(), async {
            runtime.engine.complete_user_task(task_uuid, variables).await
        })
        .await
        .map_err(|e| format!("完成审批任务失败: {e}"))?;

        let pool = pool()?;
        let prefix = database::schema_prefix();
        let execution_id: Option<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT id FROM {}workflow_execution
             WHERE engine_instance_id = $1 AND deleted = 0
             ORDER BY created_at DESC LIMIT 1",
            prefix
        )))
        .bind(task.instance_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| format!("查询审批任务所属执行记录失败: {e}"))?;

        if let Some(execution_id) = execution_id {
            let execution_id = execution_id.to_string();
            let _ = append_node_result(
                &execution_id,
                &task.node_id,
                None,
                "success",
                None,
                Some(&approval_output),
                None,
            )
            .await;
            if let Err(error) = sync_execution(&execution_id).await {
                tracing::warn!("审批后同步执行状态失败: {}", error);
            }
        }

        tracing::info!(
            "审批任务已完成: task_id={}, node_id={}, instance_id={}",
            task_uuid,
            task.node_id,
            task.instance_id
        );
        Ok(())
    }

    /// 按 `definition_hash` 决定是否重新部署，返回引擎定义 key。
    async fn ensure_deployed(
        runtime: &crate::service::TenantWorkflowRuntime,
        workflow: &StoredWorkflow,
        compiled: &crate::bpmn::CompiledBpmn,
        schema: &str,
    ) -> Result<Uuid, String> {
        if workflow.definition_hash.as_deref() == Some(compiled.hash.as_str()) {
            if let Some(key) = workflow
                .engine_definition_key
                .as_deref()
                .and_then(|value| Uuid::parse_str(value).ok())
            {
                return Ok(key);
            }
        }

        let prefix = database::schema_prefix();
        let definition = compiled.definition.clone();
        let (key, version) = database::with_schema(schema.to_string(), async {
            runtime.engine.deploy_definition(definition).await
        })
        .await;

        let pool = pool()?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {}workflow
             SET engine_definition_key = $2, engine_version = $3, definition_hash = $4,
                 updated_at = NOW()
             WHERE id = $1",
            prefix
        )))
        .bind(workflow.id)
        .bind(key)
        .bind(version)
        .bind(compiled.hash.as_str())
        .execute(&pool)
        .await
        .map_err(|e| format!("记录引擎定义版本失败: {e}"))?;

        tracing::info!(
            "工作流定义已部署: workflow_id={}, definition_key={}, version={}",
            workflow.id,
            key,
            version
        );
        Ok(key)
    }
}

