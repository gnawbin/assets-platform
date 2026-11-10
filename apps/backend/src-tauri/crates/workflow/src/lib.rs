//! 工作流引擎模块
//!
//! 执行层使用 bpmninja 的 `engine-core` + `bpmn-parser`，AI Agent 节点由
//! adk-graph `StateGraph` 承载，并通过 interrupt + BPMN UserTask 形成人工审批链。
//!
//! # 模块结构
//! - `types` — 对外 DTO、执行快照与实例状态映射
//! - `bpmn` — BPMN 解析、topic/assignee 规范化、定义指纹
//! - `service` — 每租户引擎运行时与 ServiceTask Worker
//! - `workflow_service` — 定义 / 执行 CRUD、审批、取消
//! - `projection` — 引擎状态 → `workflow_execution` 投影
//! - `persistence` — bpmninja 持久化与 adk-graph checkpointer
//! - `agent` — adk-graph Agent 执行器（LLM 循环 + 工具 + 中断恢复）
//!
//! # 用法（Tauri 命令层）
//! ```rust,ignore
//! use workflow::WorkflowService;
//!
//! let (id, version) = WorkflowService::save_workflow(input, user_id).await?;
//! let (execution_id, status) = WorkflowService::execute_workflow(input, user_id).await?;
//! ```

pub mod agent;
pub mod bpmn;
pub mod persistence;
pub mod projection;
pub mod service;
pub mod types;
pub mod workflow_service;

pub use agent::{AgentConfig, AgentGraphRunner, AgentInvocation, AgentOutcome};
pub use bpmn::{BpmnCompiler, CompiledBpmn};
pub use persistence::{PostgresCheckpointer, PostgresWorkflowPersistence};
pub use service::{TenantWorkflowRuntime, WorkflowManager};
pub use workflow_service::WorkflowService;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{NodePosition, WorkflowDefinition, WorkflowNode};
    use serde_json::json;

    fn sample_definition(prompt: &str) -> WorkflowDefinition {
        WorkflowDefinition {
            name: "测试流程".to_string(),
            nodes: vec![WorkflowNode {
                id: "agent_1".to_string(),
                node_type: "agent".to_string(),
                label: "AI Agent".to_string(),
                position: NodePosition::default(),
                config: json!({ "kind": "agent", "prompt": prompt }),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn definition_hash_changes_with_node_config() {
        let first = BpmnCompiler::hash("<bpmn/>", &sample_definition("总结"));
        let second = BpmnCompiler::hash("<bpmn/>", &sample_definition("总结并审批"));
        assert_ne!(first, second, "节点配置变化必须导致定义指纹变化");
        assert_eq!(
            first,
            BpmnCompiler::hash("<bpmn/>", &sample_definition("总结")),
            "相同输入必须得到相同指纹"
        );
    }

    #[test]
    fn execution_snapshot_roundtrips_through_variables() {
        let definition = sample_definition("总结");
        let snapshot = types::ExecutionSnapshot::from_definition("7", "42", "测试流程", &definition);
        let mut variables = std::collections::HashMap::new();
        variables.insert("__workflow".to_string(), json!(snapshot));

        let parsed = types::ExecutionSnapshot::from_variables(&variables).expect("快照应可解析");
        assert_eq!(parsed.execution_id, "42");
        assert_eq!(parsed.workflow_id, "7");
        assert!(parsed.node_configs.contains_key("agent_1"));
    }

    fn approval_definition_xml() -> &'static str {
        r###"<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL"
                  id="Definitions_1" targetNamespace="http://bpmn.io/schema/bpmn">
  <bpmn:process id="Process_1" isExecutable="true">
    <bpmn:startEvent id="Start_1" name="开始" />
    <bpmn:serviceTask id="Agent_1" name="AI Agent" implementation="##Agent" />
    <bpmn:userTask id="Approve_1" name="人工审批" data-assignee="bob" />
    <bpmn:endEvent id="End_1" name="结束" />
    <bpmn:sequenceFlow id="Flow_1" sourceRef="Start_1" targetRef="Agent_1" />
    <bpmn:sequenceFlow id="Flow_2" sourceRef="Agent_1" targetRef="Approve_1" />
    <bpmn:sequenceFlow id="Flow_3" sourceRef="Approve_1" targetRef="End_1" />
  </bpmn:process>
</bpmn:definitions>"###
    }

    fn node(id: &str, node_type: &str, config: serde_json::Value) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            node_type: node_type.to_string(),
            label: id.to_string(),
            position: NodePosition::default(),
            config,
        }
    }

    #[test]
    fn compile_rewrites_agent_topic_and_approval_assignee() {
        let definition = WorkflowDefinition {
            name: "审批流程".to_string(),
            nodes: vec![
                node(
                    "Agent_1",
                    "agent",
                    json!({
                        "kind": "agent",
                        "prompt": "总结合同要点",
                        "max_iterations": 3,
                        "interrupt_policy": "before_tool",
                    }),
                ),
                node(
                    "Approve_1",
                    "approval",
                    json!({ "kind": "approval", "assignee": "alice", "variable": "approved" }),
                ),
            ],
            ..Default::default()
        };

        let compiled = BpmnCompiler::compile(approval_definition_xml(), &definition)
            .expect("标准 BPMN 应可编译");

        match compiled.definition.nodes.get("Agent_1") {
            Some(engine_core::domain::BpmnElement::ServiceTask { topic, .. }) => {
                assert_eq!(topic, types::TOPIC_AGENT, "Agent 节点必须改写为 Agent topic");
            }
            other => panic!("Agent_1 应为 ServiceTask，实际为 {other:?}"),
        }

        match compiled.definition.nodes.get("Approve_1") {
            Some(engine_core::domain::BpmnElement::UserTask(assignee)) => {
                assert_eq!(assignee, "alice", "IR config 的审批人必须覆盖 XML");
            }
            other => panic!("Approve_1 应为 UserTask，实际为 {other:?}"),
        }
    }

    #[test]
    fn compile_keeps_data_assignee_when_ir_has_no_assignee() {
        let definition = WorkflowDefinition {
            name: "审批流程".to_string(),
            nodes: vec![node("Approve_1", "approval", json!({ "kind": "approval" }))],
            ..Default::default()
        };

        let compiled = BpmnCompiler::compile(approval_definition_xml(), &definition)
            .expect("标准 BPMN 应可编译");

        match compiled.definition.nodes.get("Approve_1") {
            Some(engine_core::domain::BpmnElement::UserTask(assignee)) => {
                assert_eq!(assignee, "bob", "IR 未设置审批人时应沿用 BPMN 的 data-assignee");
            }
            other => panic!("Approve_1 应为 UserTask，实际为 {other:?}"),
        }
    }
}
