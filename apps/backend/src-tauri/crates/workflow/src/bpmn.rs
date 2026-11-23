//! BPMN XML 编译：解析、按 IR 重写 topic/assignee、计算定义指纹。

use crate::assignee::{engine_assignee, ApprovalNodeConfig};
use crate::types::{WorkflowDefinition, TOPIC_AGENT, TOPIC_AGENT_RESUME};
use engine_core::domain::{BpmnElement, ProcessDefinition, SequenceFlow};
use serde_json::Value;
use std::collections::HashMap;

/// 编译结果。
#[derive(Debug, Clone)]
pub struct CompiledBpmn {
    pub definition: ProcessDefinition,
    pub hash: String,
}

/// BPMN 编译器。
pub struct BpmnCompiler;

impl BpmnCompiler {
    /// 解析 BPMN XML，并用前端 IR 修正引擎不识别的 `implementation` 标记。
    pub fn compile(xml: &str, ir: &WorkflowDefinition) -> Result<CompiledBpmn, String> {
        if xml.trim().is_empty() {
            return Err("BPMN XML 为空，无法部署".to_string());
        }

        let mut definition =
            bpmn_parser::parse_bpmn_xml(xml).map_err(|e| format!("BPMN 解析失败: {e}"))?;

        let nodes: HashMap<&str, &crate::types::WorkflowNode> =
            ir.nodes.iter().map(|node| (node.id.as_str(), node)).collect();
        let mut reject_branches: Vec<(String, String)> = Vec::new();

        for (node_id, element) in definition.nodes.iter_mut() {
            let Some(node) = nodes.get(node_id.as_str()) else {
                continue;
            };
            let config = node.config.as_object();
            let kind = config
                .and_then(|c| c.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or(node.node_type.as_str());

            match element {
                BpmnElement::ServiceTask { topic, .. } => {
                    if kind == "agent" || node.node_type == "agent" {
                        *topic = TOPIC_AGENT.to_string();
                    } else if kind == "agent_resume"
                        || node.node_type == "agent_resume"
                        || topic == TOPIC_AGENT_RESUME
                    {
                        *topic = TOPIC_AGENT_RESUME.to_string();
                    } else if let Some(explicit) = config
                        .and_then(|c| c.get("topic").or_else(|| c.get("handler")))
                        .and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                    {
                        *topic = explicit.to_string();
                    } else if kind == "llm" || node.node_type == "llm" {
                        *topic = "ai-llm-task".to_string();
                    } else if let Some(skill_id) = config
                        .and_then(|c| c.get("skill_id"))
                        .and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                    {
                        *topic = format!("skill:{skill_id}");
                    }
                }
                BpmnElement::UserTask(assignee) => {
                    if let Some(approval) = nodes
                        .get(node_id.as_str())
                        .and_then(|node| ApprovalNodeConfig::from_value(&node.config))
                    {
                        if let Some(value) = engine_assignee(node_id, &approval) {
                            *assignee = value;
                        }
                        if approval.reject_action_or_default() == "terminate" {
                            reject_branches
                                .push((node_id.clone(), approval.approval_variable.clone()));
                        }
                    } else if let Some(value) = config
                        .and_then(|c| c.get("assignee").or_else(|| c.get("approver")))
                        .and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                    {
                        *assignee = value.to_string();
                    }
                }
                _ => {}
            }
        }

        for (node_id, approval_variable) in reject_branches {
            expand_reject_branch(&mut definition, &node_id, &approval_variable)?;
        }

        let hash = definition_hash(xml, ir);
        Ok(CompiledBpmn { definition, hash })
    }

    /// 仅计算指纹，用于判断是否需要重新部署。
    pub fn hash(xml: &str, ir: &WorkflowDefinition) -> String {
        definition_hash(xml, ir)
    }
}

/// 为审批节点增加「通过继续 / 拒绝终止」分支。
///
/// 保留原 UserTask 节点（边界事件不受影响），只替换其出线：
/// `UserTask -> Decision -> 原目标 / TerminateEndEvent`。
pub fn expand_reject_branch(
    definition: &mut ProcessDefinition,
    node_id: &str,
    approval_variable: &str,
) -> Result<(), String> {
    let decision_id = format!("Decision_{node_id}");
    if definition.nodes.contains_key(&decision_id) {
        return Ok(());
    }
    let outgoing = definition.flows.remove(node_id).unwrap_or_default();
    if outgoing.is_empty() {
        return Ok(());
    }

    let reject_id = format!("Reject_{node_id}");
    definition
        .nodes
        .insert(decision_id.clone(), BpmnElement::ExclusiveGateway { default: None });
    definition
        .nodes
        .insert(reject_id.clone(), BpmnElement::TerminateEndEvent);

    let mut decision_flows = Vec::with_capacity(outgoing.len() + 1);
    for flow in outgoing {
        decision_flows.push(SequenceFlow::conditional(
            flow.target,
            combine_approved_condition(approval_variable, flow.condition.as_deref()),
        ));
    }
    decision_flows.push(SequenceFlow::conditional(
        reject_id.clone(),
        format!("${{{approval_variable} == false}}"),
    ));

    definition
        .flows
        .insert(node_id.to_string(), vec![SequenceFlow::simple(decision_id.clone())]);
    definition.flows.insert(decision_id, decision_flows);
    definition.flows.insert(reject_id, Vec::new());

    let rebuilt = ProcessDefinition::new(
        definition.id.clone(),
        definition.version,
        definition.nodes.clone(),
        definition.flows.clone(),
        definition.listeners.clone(),
        definition.event_listeners.clone(),
        definition.sub_processes.clone(),
    )
    .map_err(|e| format!("审批拒绝分支展开后定义校验失败: {e}"))?;
    *definition = rebuilt;
    Ok(())
}

fn combine_approved_condition(variable: &str, original: Option<&str>) -> String {
    let original = original
        .map(strip_condition_wrapper)
        .filter(|value| !value.trim().is_empty());
    match original {
        Some(original) => format!("${{{variable} == true && ({original})}}"),
        None => format!("${{{variable} == true}}"),
    }
}

fn strip_condition_wrapper(raw: &str) -> &str {
    let trimmed = raw.trim();
    if ((trimmed.starts_with("${") || trimmed.starts_with("#{")) && trimmed.ends_with('}'))
        && trimmed.len() >= 3
    {
        &trimmed[2..trimmed.len() - 1]
    } else {
        trimmed
    }
}

/// FNV-1a 64 位指纹。只用于变更检测，不用于安全签名。
fn definition_hash(xml: &str, ir: &WorkflowDefinition) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut write = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };

    write(xml.as_bytes());
    for node in &ir.nodes {
        write(node.id.as_bytes());
        write(node.node_type.as_bytes());
        write(node.config.to_string().as_bytes());
    }
    for edge in &ir.edges {
        write(edge.source.as_bytes());
        write(edge.target.as_bytes());
        write(edge.label.as_deref().unwrap_or("").as_bytes());
    }

    format!("{hash:016x}")
}
