//! BPMN XML 编译：解析、按 IR 重写 topic/assignee、计算定义指纹。

use crate::types::{WorkflowDefinition, TOPIC_AGENT, TOPIC_AGENT_RESUME};
use engine_core::domain::{BpmnElement, ProcessDefinition};
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
                    if let Some(value) = config
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

        let hash = definition_hash(xml, ir);
        Ok(CompiledBpmn { definition, hash })
    }

    /// 仅计算指纹，用于判断是否需要重新部署。
    pub fn hash(xml: &str, ir: &WorkflowDefinition) -> String {
        definition_hash(xml, ir)
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
