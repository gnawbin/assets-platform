//! 工作流 HTTP 路由。
//!
//! 与前端 `workflowService.ts` 的命令契约保持一致；当前用户从
//! `AuthPrincipal` 获取，禁止回退到固定的 1 号用户。

use axum::{extract::{Path, Query}, Extension, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use workflow::types::{
    ApprovalDecisionResult, ExecuteWorkflowInput, PendingWorkflowTask, SaveWorkflowInput,
    WorkflowDefinition, WorkflowExecution, WorkflowMeta,
};
use workflow::WorkflowService;

use super::auth::AuthPrincipal;
use super::response::{ApiError, ApiResponse};

fn current_user(principal: &AuthPrincipal) -> Result<i64, ApiError> {
    principal
        .user_id
        .ok_or_else(|| ApiError::unauthorized("当前凭证不是用户身份，无法执行工作流操作"))
}

fn blank_to_none(value: Option<String>) -> Option<String> {
    value.filter(|item| !item.trim().is_empty())
}

#[derive(Debug, Deserialize)]
pub struct WorkflowListQuery {
    pub keyword: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWorkflowRequest {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub definition: WorkflowDefinition,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default, alias = "bpmn_xml")]
    pub bpmn_xml: Option<String>,
    #[serde(default, alias = "node_types")]
    pub node_types: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedWorkflow {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteWorkflowRequest {
    #[serde(default)]
    pub workflow_id: Option<String>,
    #[serde(default)]
    pub definition: Option<WorkflowDefinition>,
    #[serde(default)]
    pub input_data: Option<Value>,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartedExecution {
    pub execution_id: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct ExecutionListQuery {
    #[serde(rename = "workflowId")]
    pub workflow_id: Option<String>,
    pub page: Option<i64>,
    #[serde(rename = "pageSize")]
    pub page_size: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct PendingTaskQuery {
    #[serde(rename = "workflowId")]
    pub workflow_id: Option<String>,
    #[serde(rename = "executionId")]
    pub execution_id: Option<String>,
    #[serde(rename = "includeAll")]
    pub include_all: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct CompleteTaskRequest {
    #[serde(default)]
    pub variables: Option<Value>,
}

pub async fn list_workflows(
    Query(query): Query<WorkflowListQuery>,
) -> Result<Json<ApiResponse<Vec<WorkflowMeta>>>, ApiError> {
    let list = WorkflowService::list_workflows(query.keyword.as_deref(), query.status.as_deref())
        .await
        .map_err(ApiError::internal_error)?;
    Ok(Json(ApiResponse::success(list)))
}

pub async fn get_workflow(
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<Option<WorkflowDefinition>>>, ApiError> {
    let Some(stored) = WorkflowService::get_workflow(&id)
        .await
        .map_err(ApiError::internal_error)?
    else {
        return Ok(Json(ApiResponse::success(None)));
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
    Ok(Json(ApiResponse::success(Some(definition))))
}

pub async fn save_workflow(
    Extension(principal): Extension<AuthPrincipal>,
    Json(req): Json<SaveWorkflowRequest>,
) -> Result<Json<ApiResponse<SavedWorkflow>>, ApiError> {
    let user_id = current_user(&principal)?;
    let input = SaveWorkflowInput {
        id: blank_to_none(req.id),
        name: req.name,
        description: blank_to_none(req.description),
        definition: req.definition,
        status: blank_to_none(req.status),
        bpmn_xml: blank_to_none(req.bpmn_xml),
        node_types: req.node_types,
    };
    let (id, version) = WorkflowService::save_workflow(input, user_id)
        .await
        .map_err(ApiError::internal_error)?;
    Ok(Json(ApiResponse::success(SavedWorkflow { id, version })))
}

pub async fn delete_workflow(
    Extension(principal): Extension<AuthPrincipal>,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let user_id = current_user(&principal)?;
    WorkflowService::delete_workflow(&id, user_id)
        .await
        .map_err(ApiError::internal_error)?;
    Ok(Json(ApiResponse::success(())))
}

pub async fn execute_workflow(
    Extension(principal): Extension<AuthPrincipal>,
    Json(req): Json<ExecuteWorkflowRequest>,
) -> Result<Json<ApiResponse<StartedExecution>>, ApiError> {
    let user_id = current_user(&principal)?;
    let input = ExecuteWorkflowInput {
        workflow_id: blank_to_none(req.workflow_id),
        definition: req.definition,
        input_data: req.input_data,
        title: blank_to_none(req.title),
    };
    let (execution_id, status) = WorkflowService::execute_workflow(input, user_id)
        .await
        .map_err(ApiError::internal_error)?;
    Ok(Json(ApiResponse::success(StartedExecution {
        execution_id,
        status,
    })))
}

pub async fn list_executions(
    Query(query): Query<ExecutionListQuery>,
) -> Result<Json<ApiResponse<Vec<WorkflowExecution>>>, ApiError> {
    let list = WorkflowService::list_executions(
        query.workflow_id.as_deref(),
        query.page.unwrap_or(1),
        query.page_size.unwrap_or(20),
    )
    .await
    .map_err(ApiError::internal_error)?;
    Ok(Json(ApiResponse::success(list)))
}

pub async fn get_execution(
    Path(execution_id): Path<String>,
) -> Result<Json<ApiResponse<Option<WorkflowExecution>>>, ApiError> {
    let execution = WorkflowService::get_execution(&execution_id)
        .await
        .map_err(ApiError::internal_error)?;
    Ok(Json(ApiResponse::success(execution)))
}

pub async fn cancel_execution(
    Extension(principal): Extension<AuthPrincipal>,
    Path(execution_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let user_id = current_user(&principal)?;
    WorkflowService::cancel_execution(&execution_id, user_id)
        .await
        .map_err(ApiError::internal_error)?;
    Ok(Json(ApiResponse::success(())))
}

pub async fn list_pending_tasks(
    Extension(principal): Extension<AuthPrincipal>,
    Query(query): Query<PendingTaskQuery>,
) -> Result<Json<ApiResponse<Vec<PendingWorkflowTask>>>, ApiError> {
    let user_id = current_user(&principal)?;
    let list = WorkflowService::list_pending_tasks(
        query.workflow_id.as_deref(),
        query.execution_id.as_deref(),
        Some(user_id),
        query.include_all.unwrap_or(false),
    )
    .await
    .map_err(ApiError::internal_error)?;
    Ok(Json(ApiResponse::success(list)))
}

pub async fn complete_user_task(
    Extension(principal): Extension<AuthPrincipal>,
    Path(task_id): Path<String>,
    Json(req): Json<CompleteTaskRequest>,
) -> Result<Json<ApiResponse<ApprovalDecisionResult>>, ApiError> {
    let user_id = current_user(&principal)?;
    let result = WorkflowService::complete_user_task(
        &task_id,
        req.variables.unwrap_or(Value::Null),
        user_id,
    )
    .await
    .map_err(ApiError::internal_error)?;
    Ok(Json(ApiResponse::success(result)))
}

