//! 引擎状态投影：bpmninja 实例状态 → `workflow_execution` 业务表。
//!
//! Worker 与命令层在状态变化后调用这些函数，前端轮询 `get_execution` 即可看到
//! 最新的节点时间线与等待中的审批任务。投影只读引擎状态，不修改引擎。

use crate::service::WorkflowManager;
use crate::types::map_instance_state;
use chrono::Utc;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

/// 追加或更新一个节点的执行结果（按 `node_id` 幂等）。
///
/// 同一节点多次上报时只保留一条记录，后到的状态覆盖先前的状态，
/// 这样中断→审批→恢复的节点不会在时间线里重复出现。
#[allow(clippy::too_many_arguments)]
pub async fn append_node_result(
    execution_id: &str,
    node_id: &str,
    node_type: Option<&str>,
    status: &str,
    input: Option<&Value>,
    output: Option<&Value>,
    duration_ms: Option<i64>,
) -> Result<(), String> {
    let execution_id: i64 = execution_id
        .parse()
        .map_err(|e| format!("无效的执行记录ID: {execution_id}（{e}）"))?;
    let pool = database::get_write_pool().map_err(|e| e.to_string())?;
    let prefix = database::schema_prefix();

    let existing: Option<Option<Value>> = sqlx::query_scalar::<_, Option<Value>>(
        sqlx::AssertSqlSafe(format!(
            "SELECT node_results FROM {}workflow_execution WHERE id = $1 AND deleted = 0",
            prefix
        )),
    )
    .bind(execution_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("读取节点执行结果失败: {e}"))?;

    let Some(existing) = existing else {
        return Err(format!("执行记录 {execution_id} 不存在"));
    };

    let mut results: Vec<Value> = existing
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();

    let index = results
        .iter()
        .position(|item| item.get("node_id").and_then(Value::as_str) == Some(node_id));
    let mut entry = match index {
        Some(index) => results.remove(index),
        None => json!({ "node_id": node_id }),
    };
    if !entry.is_object() {
        entry = json!({ "node_id": node_id });
    }

    if let Value::Object(map) = &mut entry {
        map.insert("status".to_string(), json!(status));
        if let Some(node_type) = node_type {
            map.insert("node_type".to_string(), json!(node_type));
        }
        if let Some(input) = input {
            map.insert("input".to_string(), input.clone());
        }
        if let Some(output) = output {
            map.insert("output".to_string(), output.clone());
        }
        if let Some(duration_ms) = duration_ms {
            map.insert("duration_ms".to_string(), json!(duration_ms));
        }
    }

    results.push(entry);

    let sql = format!(
        "UPDATE {}workflow_execution
         SET node_results = $2,
             current_node_id = CASE WHEN $3 = 'running' THEN $4 ELSE current_node_id END,
             updated_at = NOW()
         WHERE id = $1 AND deleted = 0",
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(execution_id)
        .bind(Value::Array(results))
        .bind(status)
        .bind(node_id)
        .execute(&pool)
        .await
        .map_err(|e| format!("写入节点执行结果失败: {e}"))?;
    Ok(())
}

/// 把引擎中的最新实例状态同步到 `workflow_execution`。
///
/// 同时补齐等待中的审批任务、Agent 中断负载与实例结果，
/// 让前端无需直接访问引擎即可渲染时间线。
pub async fn sync_execution(execution_id: &str) -> Result<(), String> {
    let execution_id: i64 = execution_id
        .parse()
        .map_err(|e| format!("无效的执行记录ID: {execution_id}（{e}）"))?;
    let pool = database::get_write_pool().map_err(|e| e.to_string())?;
    let prefix = database::schema_prefix();

    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT engine_instance_id, status FROM {}workflow_execution
         WHERE id = $1 AND deleted = 0",
        prefix
    )))
    .bind(execution_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("读取执行记录失败: {e}"))?;

    let Some(row) = row else {
        return Ok(());
    };
    let instance_id: Option<Uuid> = row
        .try_get("engine_instance_id")
        .map_err(|e| format!("读取 engine_instance_id 失败: {e}"))?;
    let Some(instance_id) = instance_id else {
        return Ok(());
    };
    let current_status: String = row.try_get("status").unwrap_or_default();
    if current_status == "cancelled" {
        return Ok(());
    }

    let schema = database::current_schema_name();
    let manager = WorkflowManager::global()?;
    let runtime = manager.runtime(&schema).await?;

    let state = runtime
        .engine
        .get_instance_state(instance_id)
        .await
        .map_err(|e| format!("读取流程实例状态失败: {e}"))?;
    let mapped = map_instance_state(&state);

    let instance = runtime
        .engine
        .get_instance_details(instance_id)
        .await
        .map_err(|e| format!("读取流程实例失败: {e}"))?;

    let pending = runtime
        .engine
        .get_pending_user_tasks_enriched()
        .await
        .into_iter()
        .find(|task| task.instance_id == instance_id);

    let waiting_task_id = pending.as_ref().map(|task| task.task_id.to_string());
    let current_node_id = pending
        .as_ref()
        .map(|task| task.node_id.clone())
        .or_else(|| Some(instance.current_node.clone()).filter(|node| !node.is_empty()));
    let agent_thread_id = instance
        .variables
        .get("agent_thread_id")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let interrupt_payload = instance
        .variables
        .get("agent_interrupt")
        .filter(|value| !value.is_null())
        .cloned();
    let result_data = instance
        .variables
        .get("result")
        .cloned()
        .or_else(|| instance.variables.get("agent_output").cloned());
    let finished = matches!(mapped.status, "success" | "failed");

    let sql = format!(
        "UPDATE {}workflow_execution
         SET status = $2,
             bpmn_status = $3,
             current_node_id = $4,
             waiting_task_id = $5,
             agent_thread_id = $6,
             interrupt_payload = $7,
             result_data = CASE WHEN $8 THEN $9 ELSE result_data END,
             finished_at = CASE WHEN $8 THEN COALESCE(finished_at, NOW()) ELSE finished_at END,
             total_duration_ms = CASE WHEN $8 THEN COALESCE(
                 total_duration_ms,
                 GREATEST(0, (EXTRACT(EPOCH FROM (NOW() - created_at)) * 1000)::INT)
             ) ELSE total_duration_ms END,
             updated_at = NOW()
         WHERE id = $1 AND deleted = 0",
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(execution_id)
        .bind(mapped.status)
        .bind(mapped.bpmn_status)
        .bind(current_node_id)
        .bind(waiting_task_id)
        .bind(agent_thread_id)
        .bind(interrupt_payload)
        .bind(finished)
        .bind(result_data)
        .execute(&pool)
        .await
        .map_err(|e| format!("同步执行状态失败: {e}"))?;

    if finished {
        tracing::info!(
            "工作流执行结束: execution_id={}, status={}, bpmn_status={}",
            execution_id,
            mapped.status,
            mapped.bpmn_status
        );
    }
    Ok(())
}

/// 读取执行记录当前的对外状态，供命令层返回。
pub async fn current_status(execution_id: &str) -> Result<String, String> {
    let execution_id: i64 = execution_id
        .parse()
        .map_err(|e| format!("无效的执行记录ID: {execution_id}（{e}）"))?;
    let pool = database::get_read_pool().map_err(|e| e.to_string())?;
    let prefix = database::schema_prefix();
    let status: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT status FROM {}workflow_execution WHERE id = $1 AND deleted = 0",
        prefix
    )))
    .bind(execution_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("读取执行状态失败: {e}"))?;
    Ok(status.unwrap_or_else(|| "running".to_string()))
}

/// 生成毫秒时间戳差，供节点耗时统计使用。
pub fn elapsed_ms(start: chrono::DateTime<Utc>) -> i64 {
    (Utc::now() - start).num_milliseconds()
}
