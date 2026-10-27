//! 工作流 Tauri Command
//!
//! 与前端 `apps/web/src/services/workflowService.ts` 的调用契约一一对应：
//! 命令名、参数名（camelCase ↔ snake_case 由 Tauri 自动映射）和返回结构都保持稳定。
//!
//! 业务逻辑全部在 `workflow` crate 的 `WorkflowService` 中，本模块只做
//! 参数解析、默认值补全与 DTO 形态转换。

use serde::Serialize;
use serde_json::Value;
use workflow::types::{
    ExecuteWorkflowInput, PendingWorkflowTask, SaveWorkflowInput, WorkflowDefinition,
    WorkflowExecution, WorkflowMeta,
};
use workflow::WorkflowService;

/// `save_workflow` 的返回：前端拿到 `id` 后跳转编辑器。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedWorkflow {
    pub id: String,
    pub version: String,
}

/// `execute_workflow` 的返回：前端用 `executionId` 轮询执行详情。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartedExecution {
    pub execution_id: String,
    pub status: String,
}

/// 命令层的操作人：未登录/未传时回退到 1 号用户，与其它 command 保持一致。
fn current_user(raw: Option<String>) -> i64 {
    raw.and_then(|id| id.trim().parse::<i64>().ok())
        .unwrap_or(1)
}

fn blank_to_none(value: Option<String>) -> Option<String> {
    value.filter(|item| !item.trim().is_empty())
}

/// 工作流列表（元数据，不含定义）。
#[tauri::command]
pub async fn list_workflows(
    keyword: Option<String>,
    status: Option<String>,
) -> Result<Vec<WorkflowMeta>, String> {
    WorkflowService::list_workflows(keyword.as_deref(), status.as_deref()).await
}

/// 读取单个工作流的完整定义（含 BPMN XML），供编辑器加载。
///
/// 定义内缺少 `bpmn_xml` 时用独立列回填，保证 bpmn-js 能还原画布。
#[tauri::command]
pub async fn get_workflow(id: String) -> Result<Option<WorkflowDefinition>, String> {
    let Some(stored) = WorkflowService::get_workflow(&id).await? else {
        return Ok(None);
    };
    let mut definition = stored.definition;
    if definition.bpmn_xml.is_none() {
        definition.bpmn_xml = stored.bpmn_xml;
    }
    if definition.name.trim().is_empty() {
        definition.name = stored.name;
    }
    if definition.description.is_none() {
        definition.description = stored.description;
    }
    if definition.version.is_none() {
        definition.version = Some(stored.version);
    }
    Ok(Some(definition))
}

/// 保存工作流（IR + BPMN XML 双写），返回 `{ id, version }`。
#[tauri::command]
pub async fn save_workflow(
    id: Option<String>,
    name: String,
    description: Option<String>,
    definition: WorkflowDefinition,
    status: Option<String>,
    bpmn_xml: Option<String>,
    node_types: Option<Vec<String>>,
    current_user_id: Option<String>,
) -> Result<SavedWorkflow, String> {
    let input = SaveWorkflowInput {
        id: blank_to_none(id),
        name,
        description: blank_to_none(description),
        definition,
        status: blank_to_none(status),
        bpmn_xml: blank_to_none(bpmn_xml),
        node_types,
    };
    let (id, version) = WorkflowService::save_workflow(input, current_user(current_user_id)).await?;
    Ok(SavedWorkflow { id, version })
}

/// 删除工作流（软删除，同时清理引擎侧定义缓存）。
#[tauri::command]
pub async fn delete_workflow(
    id: String,
    current_user_id: Option<String>,
) -> Result<(), String> {
    WorkflowService::delete_workflow(&id, current_user(current_user_id)).await
}

/// 执行工作流：编译 BPMN → 部署定义 → 启动引擎实例。
#[tauri::command]
pub async fn execute_workflow(
    workflow_id: Option<String>,
    definition: Option<WorkflowDefinition>,
    input_data: Option<Value>,
    title: Option<String>,
    current_user_id: Option<String>,
) -> Result<StartedExecution, String> {
    let input = ExecuteWorkflowInput {
        workflow_id: blank_to_none(workflow_id),
        definition,
        input_data,
        title: blank_to_none(title),
    };
    let (execution_id, status) =
        WorkflowService::execute_workflow(input, current_user(current_user_id)).await?;
    Ok(StartedExecution {
        execution_id,
        status,
    })
}

/// 执行历史列表（按创建时间倒序，分页）。
#[tauri::command]
pub async fn list_executions(
    workflow_id: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
) -> Result<Vec<WorkflowExecution>, String> {
    WorkflowService::list_executions(
        workflow_id.as_deref(),
        page.unwrap_or(1),
        page_size.unwrap_or(20),
    )
    .await
}

/// 执行详情（含节点时间线与等待中的审批），读取时会刷新一次投影。
#[tauri::command]
pub async fn get_execution(execution_id: String) -> Result<Option<WorkflowExecution>, String> {
    WorkflowService::get_execution(&execution_id).await
}

/// 取消执行：终止引擎实例并标记为 cancelled。
#[tauri::command]
pub async fn cancel_execution(
    execution_id: String,
    current_user_id: Option<String>,
) -> Result<(), String> {
    WorkflowService::cancel_execution(&execution_id, current_user(current_user_id)).await
}

/// 待办审批任务（引擎 UserTask，按工作流 / 执行记录过滤）。
#[tauri::command]
pub async fn list_pending_tasks(
    workflow_id: Option<String>,
    execution_id: Option<String>,
) -> Result<Vec<PendingWorkflowTask>, String> {
    WorkflowService::list_pending_tasks(workflow_id.as_deref(), execution_id.as_deref()).await
}

/// 完成人工审批任务，`variables` 中的 `approved` / `comment` 会驱动 Agent 恢复。
#[tauri::command]
pub async fn complete_user_task(
    task_id: String,
    variables: Option<Value>,
    current_user_id: Option<String>,
) -> Result<(), String> {
    WorkflowService::complete_user_task(
        &task_id,
        variables.unwrap_or(Value::Null),
        current_user(current_user_id),
    )
    .await
}
