//! 工作流对外 DTO 与执行快照。
//!
//! 这些类型同时用于 Tauri 命令、HTTP 路由和数据库 JSON 列，字段名保持
//! snake_case，与前端 `workflowService.ts` 的历史契约一致。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// 工作流节点的 IR 类型。
pub const NODE_TYPE_TRIGGER: &str = "trigger";
pub const NODE_TYPE_AGENT: &str = "agent";
pub const NODE_TYPE_APPROVAL: &str = "approval";
pub const NODE_TYPE_SKILL: &str = "skill";
pub const NODE_TYPE_LLM: &str = "llm";
pub const NODE_TYPE_CONDITION: &str = "condition";
pub const NODE_TYPE_CODE: &str = "code";
pub const NODE_TYPE_WORKFLOW: &str = "workflow";
pub const NODE_TYPE_OUTPUT: &str = "output";

/// AI Agent 服务任务 topic。
pub const TOPIC_AGENT: &str = "ai-agent-task";
/// Agent 审批通过后的恢复服务任务 topic。
pub const TOPIC_AGENT_RESUME: &str = "ai-agent-resume";

/// 工作流节点位置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodePosition {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
}

/// 执行 IR 节点。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub position: NodePosition,
    #[serde(default)]
    pub config: Value,
}

/// 执行 IR 连线。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// 工作流运行配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowConfig {
    #[serde(default)]
    pub max_execution_time: Option<u64>,
    #[serde(default)]
    pub retry_on_failure: Option<bool>,
    #[serde(default)]
    pub max_retries: Option<u32>,
}

/// 完整工作流定义（前端 IR）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub nodes: Vec<WorkflowNode>,
    #[serde(default)]
    pub edges: Vec<WorkflowEdge>,
    #[serde(default)]
    pub variables: Option<HashMap<String, String>>,
    #[serde(default)]
    pub config: Option<WorkflowConfig>,
    /// 标准 BPMN 2.0 XML。加载时优先使用，保存时与 IR 双写。
    #[serde(default)]
    pub bpmn_xml: Option<String>,
}

/// 工作流列表项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowMeta {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub version: String,
    pub status: String,
    pub node_types: Vec<String>,
    pub use_count: i32,
    #[serde(default)]
    pub last_executed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 单个节点执行结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeExecutionResult {
    pub node_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
}

/// 工作流执行记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowExecution {
    pub id: String,
    pub workflow_id: String,
    #[serde(default)]
    pub workflow_name: Option<String>,
    pub trigger_type: String,
    pub status: String,
    #[serde(default)]
    pub input_data: Option<Value>,
    #[serde(default)]
    pub result_data: Option<Value>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub node_results: Option<Value>,
    #[serde(default)]
    pub total_duration_ms: Option<i32>,
    #[serde(default)]
    pub total_tokens: Option<i32>,
    #[serde(default)]
    pub total_cost: Option<f64>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub engine_instance_id: Option<String>,
    #[serde(default)]
    pub engine_definition_key: Option<String>,
    #[serde(default)]
    pub current_node_id: Option<String>,
    #[serde(default)]
    pub agent_thread_id: Option<String>,
    #[serde(default)]
    pub waiting_task_id: Option<String>,
    #[serde(default)]
    pub interrupt_payload: Option<Value>,
    #[serde(default)]
    pub bpmn_status: Option<String>,
}

/// 待办工作流任务（UserTask）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingWorkflowTask {
    pub task_id: String,
    pub instance_id: String,
    pub node_id: String,
    pub assignee: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub business_key: Option<String>,
    #[serde(default)]
    pub execution_id: Option<String>,
    #[serde(default)]
    pub workflow_id: Option<String>,
    #[serde(default)]
    pub workflow_name: Option<String>,
    #[serde(default)]
    pub interrupt_payload: Option<Value>,
}

/// 单个节点的配置快照。运行中实例只依赖该快照，不受最新模板编辑影响。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSnapshot {
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub config: Value,
}

/// 写入流程变量 `__workflow` 的执行配置快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSnapshot {
    pub workflow_id: String,
    pub execution_id: String,
    pub workflow_name: String,
    pub node_configs: HashMap<String, NodeSnapshot>,
}

impl ExecutionSnapshot {
    /// 从 IR 提取节点配置快照。
    pub fn from_definition(
        workflow_id: impl Into<String>,
        execution_id: impl Into<String>,
        workflow_name: impl Into<String>,
        definition: &WorkflowDefinition,
    ) -> Self {
        let node_configs = definition
            .nodes
            .iter()
            .map(|node| {
                (
                    node.id.clone(),
                    NodeSnapshot {
                        node_type: node.node_type.clone(),
                        config: node.config.clone(),
                    },
                )
            })
            .collect();
        Self {
            workflow_id: workflow_id.into(),
            execution_id: execution_id.into(),
            workflow_name: workflow_name.into(),
            node_configs,
        }
    }

    /// 从流程变量中读取快照。
    pub fn from_variables(variables: &HashMap<String, Value>) -> Option<Self> {
        variables
            .get("__workflow")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }
}

/// 数据库中的 workflow 行。
#[derive(Debug, Clone)]
pub struct StoredWorkflow {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub version: String,
    pub status: String,
    pub definition: WorkflowDefinition,
    pub bpmn_xml: Option<String>,
    pub node_types: Vec<String>,
    pub engine_definition_key: Option<String>,
    pub engine_version: Option<i32>,
    pub definition_hash: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// `save_workflow` 的入参。
#[derive(Debug, Clone, Deserialize)]
pub struct SaveWorkflowInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub definition: WorkflowDefinition,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub bpmn_xml: Option<String>,
    #[serde(default)]
    pub node_types: Option<Vec<String>>,
}

/// `execute_workflow` 的入参。
#[derive(Debug, Clone, Deserialize)]
pub struct ExecuteWorkflowInput {
    #[serde(default)]
    pub workflow_id: Option<String>,
    #[serde(default)]
    pub definition: Option<WorkflowDefinition>,
    #[serde(default)]
    pub input_data: Option<Value>,
    #[serde(default)]
    pub title: Option<String>,
}

/// 状态映射结果。
#[derive(Debug, Clone, Copy)]
pub struct MappedStatus {
    pub status: &'static str,
    pub bpmn_status: &'static str,
}

/// 将 bpmninja 的实例状态映射为前端状态。
pub fn map_instance_state(state: &engine_core::runtime::InstanceState) -> MappedStatus {
    use engine_core::runtime::InstanceState;

    match state {
        InstanceState::Running | InstanceState::ParallelExecution { .. } => MappedStatus {
            status: "running",
            bpmn_status: "running",
        },
        InstanceState::WaitingOnUserTask { .. } => MappedStatus {
            status: "waiting",
            bpmn_status: "waiting_user_task",
        },
        InstanceState::WaitingOnServiceTask { .. } => MappedStatus {
            status: "running",
            bpmn_status: "waiting_service_task",
        },
        InstanceState::WaitingOnTimer { .. } => MappedStatus {
            status: "waiting",
            bpmn_status: "waiting_timer",
        },
        InstanceState::WaitingOnMessage { .. } => MappedStatus {
            status: "waiting",
            bpmn_status: "waiting_message",
        },
        InstanceState::WaitingOnSignal { .. } => MappedStatus {
            status: "waiting",
            bpmn_status: "waiting_signal",
        },
        InstanceState::WaitingOnCondition { .. } => MappedStatus {
            status: "waiting",
            bpmn_status: "waiting_condition",
        },
        InstanceState::WaitingOnEventBasedGateway => MappedStatus {
            status: "waiting",
            bpmn_status: "waiting_event_gateway",
        },
        InstanceState::WaitingOnCallActivity { .. } => MappedStatus {
            status: "waiting",
            bpmn_status: "waiting_call_activity",
        },
        InstanceState::Completed => MappedStatus {
            status: "success",
            bpmn_status: "completed",
        },
        InstanceState::CompletedWithError { .. } => MappedStatus {
            status: "failed",
            bpmn_status: "completed_with_error",
        },
        InstanceState::Suspended { .. } => MappedStatus {
            status: "waiting",
            bpmn_status: "suspended",
        },
    }
}
