//! Tauri Command 入口（审批操作）
//!
//! 提供审批流程的创建、审批操作、状态查询等 Tauri Command。
//! 通过 WfEngine 工具类统一调用 wfe-core 的功能。

use serde_json::json;
use wfe_core::executor::StepRegistry;
use wfe_core::executor::WorkflowExecutor;
use wfe_core::models::{Event, ExecutionPointer, WorkflowInstance};
use wfe_core::traits::{EventRepository, WorkflowRepository};

use super::definitions::get_definition;
use crate::database;

fn persistence_stub() -> ! {
    panic!("wfe-postgres removed until sqlx 0.9 support is available");
}

/// 创建工作流实例并启动审批流程
#[tauri::command]
pub async fn wf_create_workflow(
    def_id: String,
    biz_type: String,
    biz_id: i64,
    applicant_id: i64,
) -> Result<String, String> {
    let definition = get_definition(&def_id)?;
    todo!("wfe-postgres 升级到支持 sqlx 0.9.0 后重新启用")
}

/// 审批操作：通过/驳回
#[tauri::command]
pub async fn wf_approve_step(
    workflow_id: String,
    action: String, // "approve" | "reject"
    comment: String,
    approver_id: i64,
) -> Result<(), String> {
    todo!("wfe-postgres 升级到支持 sqlx 0.9.0 后重新启用")
}

/// 查询审批状态
#[tauri::command]
pub async fn wf_get_workflow_status(
    biz_type: String,
    biz_id: i64,
) -> Result<serde_json::Value, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    // 查询工作流实例
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT id, status, data::text, created_at, updated_at
         FROM {}wf_instance
         WHERE biz_type = $1 AND biz_id = $2
         ORDER BY created_at DESC LIMIT 1",
        prefix
    )))
    .bind(&biz_type)
    .bind(biz_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("查询失败: {}", e))?;

    match row {
        Some(r) => {
            use sqlx::Row;
            let workflow_id: String = r.try_get("id").unwrap_or_default();
            let status: String = r.try_get("status").unwrap_or_default();

            // 查询审批记录
            let records = sqlx::query(sqlx::AssertSqlSafe(format!(
                "SELECT step_id, step_name, approver_id, action, comment, created_at
                 FROM {}wf_approval_record
                 WHERE workflow_id = $1
                 ORDER BY created_at",
                prefix
            )))
            .bind(&workflow_id)
            .fetch_all(&pool)
            .await
            .map_err(|e| format!("查询审批记录失败: {}", e))?;

            let approval_records: Vec<serde_json::Value> = records
                .iter()
                .map(|rec| {
                    use sqlx::Row;
                    json!({
                        "step_id": rec.try_get::<i32, _>("step_id").unwrap_or(0),
                        "step_name": rec.try_get::<String, _>("step_name").unwrap_or_default(),
                        "approver_id": rec.try_get::<i64, _>("approver_id").unwrap_or(0),
                        "action": rec.try_get::<String, _>("action").unwrap_or_default(),
                        "comment": rec.try_get::<String, _>("comment").unwrap_or_default(),
                        "created_at": rec.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at")
                            .map(|t| t.to_rfc3339())
                            .unwrap_or_default(),
                    })
                })
                .collect();

            Ok(json!({
                "workflow_id": workflow_id,
                "status": status,
                "approval_records": approval_records,
            }))
        }
        None => Ok(json!({
            "workflow_id": null,
            "status": "none",
            "approval_records": [],
        })),
    }
}

/// 内部审批操作函数（供 WfEngine 调用）
pub(crate) async fn approve_workflow_step_inner(
    executor: &WorkflowExecutor,
    registry: &StepRegistry,
    workflow_id: &str,
    action: &str,
    comment: &str,
    approver_id: i64,
) -> Result<(), String> {
    todo!("wfe-postgres 升级到支持 sqlx 0.9.0 后重新启用")
}

/// 内部查询函数（供 WfEngine 调用）
pub(crate) async fn get_workflow_status_inner(
    biz_type: &str,
    biz_id: i64,
) -> Result<serde_json::Value, String> {
    wf_get_workflow_status(biz_type.to_string(), biz_id).await
}
