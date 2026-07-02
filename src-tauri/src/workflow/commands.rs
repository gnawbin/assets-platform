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
use super::persistence::create_persistence_provider;
use crate::database;

/// 创建工作流实例并启动审批流程
#[tauri::command]
pub async fn wf_create_workflow(
    def_id: String,
    biz_type: String,
    biz_id: i64,
    applicant_id: i64,
) -> Result<String, String> {
    let definition = get_definition(&def_id)?;
    let persistence = create_persistence_provider();

    // 构建工作流数据
    let data = json!({
        "biz_type": biz_type,
        "biz_id": biz_id,
        "applicant_id": applicant_id,
    });

    // 创建工作流实例
    let mut instance = WorkflowInstance::new(&def_id, 1, data);
    instance.workflow_definition_id = def_id.clone();

    // 添加初始执行指针（指向第一步）
    if let Some(first_step) = definition.steps.first() {
        let mut pointer = ExecutionPointer::new(first_step.id);
        pointer.active = true;
        instance.execution_pointers.push(pointer);
    } else {
        return Err("流程定义中没有步骤".into());
    }

    // 持久化实例
    let instance_id = persistence
        .create_new_workflow(&instance)
        .await
        .map_err(|e| format!("创建审批流程失败: {}", e))?;

    // 执行工作流
    let executor = super::executor::create_executor();
    let registry = super::executor::create_step_registry();
    executor
        .execute(&instance_id, &definition, &registry, None)
        .await
        .map_err(|e| format!("执行审批流程失败: {}", e))?;

    tracing::info!(
        "审批流程创建成功: instance_id={}, def_id={}, biz_type={}, biz_id={}",
        instance_id,
        def_id,
        biz_type,
        biz_id
    );

    Ok(instance_id)
}

/// 审批操作：通过/驳回
#[tauri::command]
pub async fn wf_approve_step(
    workflow_id: String,
    action: String, // "approve" | "reject"
    comment: String,
    approver_id: i64,
) -> Result<(), String> {
    let persistence = create_persistence_provider();

    // 1. 加载工作流实例
    let instance = persistence
        .get_workflow_instance(&workflow_id)
        .await
        .map_err(|e| format!("加载审批流程失败: {}", e))?;

    // 2. 记录审批记录到业务表（直接写入 wf_approval_record）
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    // 找到当前活跃的审批步骤
    let active_pointer = instance
        .execution_pointers
        .iter()
        .find(|p| p.active)
        .ok_or("没有活跃的审批步骤")?;

    // 插入审批记录
    let step_id = active_pointer.step_id as i64;
    let step_name = active_pointer.step_name.clone().unwrap_or_default();

    sqlx::query(&format!(
        r#"INSERT INTO {}wf_approval_record
           (workflow_id, step_id, step_name, approver_id, action, comment, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, NOW())"#,
        prefix
    ))
    .bind(&workflow_id)
    .bind(step_id)
    .bind(&step_name)
    .bind(approver_id)
    .bind(&action)
    .bind(&comment)
    .execute(&pool)
    .await
    .map_err(|e| format!("记录审批日志失败: {}", e))?;

    // 3. 发布审批事件
    let event_key = format!("{}-approval-{}", workflow_id, active_pointer.step_id);
    let event = Event::new(
        "approval.event",
        &event_key,
        json!({
            "action": action,
            "comment": comment,
            "user_id": approver_id,
        }),
    );

    persistence
        .create_event(&event)
        .await
        .map_err(|e| format!("创建审批事件失败: {}", e))?;

    // 4. 重新执行工作流（推进到下一步）
    let definition = get_definition(&instance.workflow_definition_id)?;
    let executor = super::executor::create_executor();
    let registry = super::executor::create_step_registry();

    executor
        .execute(&workflow_id, &definition, &registry, None)
        .await
        .map_err(|e| format!("推进审批流程失败: {}", e))?;

    tracing::info!(
        "审批操作完成: workflow_id={}, action={}, approver_id={}",
        workflow_id,
        action,
        approver_id
    );

    Ok(())
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
    let row = sqlx::query(&format!(
        "SELECT id, status, data::text, created_at, updated_at
         FROM {}wf_instance
         WHERE biz_type = $1 AND biz_id = $2
         ORDER BY created_at DESC LIMIT 1",
        prefix
    ))
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
            let records = sqlx::query(&format!(
                "SELECT step_id, step_name, approver_id, action, comment, created_at
                 FROM {}wf_approval_record
                 WHERE workflow_id = $1
                 ORDER BY created_at",
                prefix
            ))
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
    let persistence = create_persistence_provider();

    // 加载工作流实例
    let instance = persistence
        .get_workflow_instance(workflow_id)
        .await
        .map_err(|e| format!("加载审批流程失败: {}", e))?;

    // 找到当前活跃的审批步骤
    let active_pointer = instance
        .execution_pointers
        .iter()
        .find(|p| p.active)
        .ok_or("没有活跃的审批步骤")?;

    // 发布审批事件
    let event_key = format!("{}-approval-{}", workflow_id, active_pointer.step_id);
    let event = Event::new(
        "approval.event",
        &event_key,
        json!({
            "action": action,
            "comment": comment,
            "user_id": approver_id,
        }),
    );

    persistence
        .create_event(&event)
        .await
        .map_err(|e| format!("创建审批事件失败: {}", e))?;

    // 重新执行工作流
    let definition = get_definition(&instance.workflow_definition_id)?;

    executor
        .execute(workflow_id, &definition, registry, None)
        .await
        .map_err(|e| format!("推进审批流程失败: {}", e))?;

    Ok(())
}

/// 内部查询函数（供 WfEngine 调用）
pub(crate) async fn get_workflow_status_inner(
    biz_type: &str,
    biz_id: i64,
) -> Result<serde_json::Value, String> {
    wf_get_workflow_status(biz_type.to_string(), biz_id).await
}
