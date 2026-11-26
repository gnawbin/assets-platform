//! 审批人配置解析：用户 / 部门 / 表单字段 → 执行快照中的审批组计划。
//!
//! 审批组计划在流程启动前解析，运行中不再查询组织架构；
//! 引擎只看到 `group:<nodeId>` 占位 assignee，具体投票由
//! `approval_group` 模块处理。

use crate::types::{ApprovalGroupPlan, ApprovalMember, WorkflowDefinition};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

const DEFAULT_APPROVAL_TYPE: &str = "single";
const DEFAULT_REJECT_ACTION: &str = "terminate";

/// IR 中一个审批节点的配置视图。
#[derive(Debug, Clone)]
pub struct ApprovalNodeConfig {
    pub mode: String,
    pub approval_type: String,
    pub reject_action: String,
    pub approval_variable: String,
    pub assignee: Option<String>,
    pub assignee_ids: Vec<String>,
    pub dept_id: Option<String>,
    pub dept_scope: String,
    pub include_children: bool,
    pub form_field: Option<String>,
}

impl ApprovalNodeConfig {
    pub fn from_value(config: &Value) -> Option<Self> {
        let object = config.as_object()?;
        let kind = object.get("kind").and_then(Value::as_str).unwrap_or("");
        if !kind.is_empty() && kind != "approval" {
            return None;
        }
        if kind.is_empty()
            && !object.contains_key("assignee_mode")
            && !object.contains_key("approval_type")
        {
            return None;
        }

        let mode = string_field(object, "assignee_mode")
            .or_else(|| string_field(object, "approval_mode"))
            .unwrap_or_else(|| "user".to_string());
        let approval_type = string_field(object, "approval_type")
            .unwrap_or_else(|| DEFAULT_APPROVAL_TYPE.to_string());
        let reject_action = string_field(object, "reject_action").unwrap_or_else(|| {
            if mode == "department"
                || mode == "form"
                || approval_type == "all"
                || approval_type == "any"
            {
                DEFAULT_REJECT_ACTION.to_string()
            } else {
                "continue".to_string()
            }
        });
        let approval_variable = string_field(object, "approval_variable")
            .or_else(|| string_field(object, "variable"))
            .unwrap_or_else(|| "approved".to_string());
        let assignee = string_field(object, "assignee");
        let assignee_ids = parse_id_list(object.get("assignee_ids"))
            .into_iter()
            .chain(parse_id_list(object.get("assigneeIds")))
            .chain(assignee.clone().into_iter())
            .collect::<Vec<_>>();
        let dept_id = string_field(object, "assignee_dept_id")
            .or_else(|| string_field(object, "assigneeDeptId"));
        let dept_scope = string_field(object, "assignee_dept_scope")
            .or_else(|| string_field(object, "assigneeDeptScope"))
            .unwrap_or_else(|| "leader".to_string());
        let include_children = object
            .get("assignee_dept_include_children")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let form_field = string_field(object, "assignee_form_field")
            .or_else(|| string_field(object, "assigneeFormField"));

        Some(Self {
            mode,
            approval_type,
            reject_action,
            approval_variable,
            assignee,
            assignee_ids: dedupe_ids(assignee_ids),
            dept_id,
            dept_scope,
            include_children,
            form_field,
        })
    }
}

impl ApprovalNodeConfig {
    /// 是否需要在引擎侧使用 group token。
    pub fn is_group(&self) -> bool {
        self.mode == "department"
            || self.mode == "form"
            || self.approval_type == "all"
            || self.approval_type == "any"
            || self.assignee_ids.len() > 1
    }

    pub fn reject_action_or_default(&self) -> String {
        if self.reject_action == "continue" {
            "continue".to_string()
        } else {
            "terminate".to_string()
        }
    }
}

fn string_field(object: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

/// 解析数组 / 逗号分隔字符串 / 单个 ID。
pub fn parse_id_list(value: Option<&Value>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    extract_ids_from_value(value)
}

fn extract_ids_from_value(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) => text
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect(),
        Value::Number(number) => vec![number.to_string()],
        Value::Object(map) => ["user_id", "userId", "id", "value"]
            .iter()
            .find_map(|key| map.get(*key))
            .map(extract_ids_from_value)
            .unwrap_or_default(),
        Value::Array(items) => items.iter().flat_map(extract_ids_from_value).collect(),
        _ => Vec::new(),
    }
}

fn dedupe_ids(ids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty() && seen.insert(id.clone()))
        .collect()
}

/// 引擎侧 assignee：多人/动态审批使用 group token，其余使用用户 ID。
pub fn engine_assignee(node_id: &str, config: &ApprovalNodeConfig) -> Option<String> {
    if config.is_group() {
        Some(format!("group:{node_id}"))
    } else {
        config
            .assignee
            .clone()
            .or_else(|| config.assignee_ids.first().cloned())
            .filter(|value| !value.trim().is_empty())
    }
}

/// 从流程变量中读取点路径，例如 `form.approver_id` / `items[0].id`。
pub fn read_variable_path(variables: &HashMap<String, Value>, path: &str) -> Option<Value> {
    let mut current: Option<&Value> = None;
    for raw_segment in path.split('.') {
        let mut segment = raw_segment;
        let mut index = None;
        if let Some(open) = raw_segment.find('[') {
            if raw_segment.ends_with(']') {
                segment = &raw_segment[..open];
                index = raw_segment[open + 1..raw_segment.len() - 1]
                    .parse::<usize>()
                    .ok();
            }
        }

        current = match current {
            None => variables.get(segment),
            Some(value) => value.as_object().and_then(|map| map.get(segment)),
        };
        current?;

        if let Some(index) = index {
            current = current
                .and_then(|value| value.as_array().and_then(|items| items.get(index)));
        }
    }
    current.cloned()
}

/// 解析审批组计划；失败会阻止流程启动。
pub async fn resolve_approval_groups(
    definition: &WorkflowDefinition,
    variables: &HashMap<String, Value>,
    tenant_id: Option<i64>,
) -> Result<HashMap<String, ApprovalGroupPlan>, String> {
    let mut result = HashMap::new();

    for node in &definition.nodes {
        if node.node_type != "approval" {
            let kind = node.config.get("kind").and_then(Value::as_str);
            if kind != Some("approval") {
                continue;
            }
        }
        let Some(config) = ApprovalNodeConfig::from_value(&node.config) else {
            continue;
        };
        if !config.is_group() {
            continue;
        }

        let members = match config.mode.as_str() {
            "department" => resolve_department(&node.label, &config, tenant_id).await?,
            "form" => {
                let field = config
                    .form_field
                    .as_deref()
                    .ok_or_else(|| format!("审批节点「{}」未配置表单字段路径", node.label))?;
                let value = read_variable_path(variables, field).ok_or_else(|| {
                    format!("审批节点「{}」的表单字段「{}」为空", node.label, field)
                })?;
                let ids = dedupe_ids(extract_ids_from_value(&value));
                if ids.is_empty() {
                    return Err(format!(
                        "审批节点「{}」的表单字段「{}」未解析到审批人",
                        node.label, field
                    ));
                }
                load_members(&node.label, &ids, tenant_id).await?
            }
            _ => {
                if config.assignee_ids.is_empty() {
                    return Err(format!("审批节点「{}」未选择审批人", node.label));
                }
                load_members(&node.label, &config.assignee_ids, tenant_id).await?
            }
        };

        if members.is_empty() {
            return Err(format!("审批节点「{}」未解析到有效审批人", node.label));
        }

        let approval_type = match config.approval_type.as_str() {
            "all" | "any" => config.approval_type.clone(),
            _ if members.len() > 1 => "all".to_string(),
            _ => "single".to_string(),
        };
        result.insert(
            node.id.clone(),
            ApprovalGroupPlan {
                approval_type,
                reject_action: config.reject_action_or_default(),
                members,
            },
        );
    }

    Ok(result)
}

async fn resolve_department(
    label: &str,
    config: &ApprovalNodeConfig,
    tenant_id: Option<i64>,
) -> Result<Vec<ApprovalMember>, String> {
    let tenant_id = tenant_id
        .ok_or_else(|| format!("审批节点「{label}」使用部门审批，但无法确定当前租户"))?;
    let department_id = config
        .dept_id
        .as_deref()
        .ok_or_else(|| format!("审批节点「{label}」未选择部门"))?
        .parse::<i64>()
        .map_err(|e| format!("审批节点「{label}」部门ID无效: {e}"))?;

    let department_name =
        service::department_service::get_department_name(tenant_id, department_id)
            .await
            .unwrap_or_else(|| department_id.to_string());

    if config.dept_scope == "members" {
        let members = service::department_service::get_department_members(
            tenant_id,
            department_id,
            config.include_children,
        )
        .await?;
        if members.is_empty() {
            return Err(format!("部门「{department_name}」没有可用成员"));
        }
        return Ok(members
            .into_iter()
            .map(|member| ApprovalMember {
                user_id: member.id.to_string(),
                user_name: member.name,
            })
            .collect());
    }

    let leader =
        service::department_service::get_department_leader(tenant_id, department_id).await?;
    let Some(leader) = leader else {
        return Err(format!("部门「{department_name}」未配置负责人"));
    };
    Ok(vec![ApprovalMember {
        user_id: leader.id.to_string(),
        user_name: leader.name,
    }])
}

async fn load_members(
    label: &str,
    ids: &[String],
    tenant_id: Option<i64>,
) -> Result<Vec<ApprovalMember>, String> {
    let tenant_id = tenant_id.ok_or_else(|| {
        format!("审批节点「{label}」使用多人/动态审批，但无法确定当前租户")
    })?;
    let numeric_ids = ids
        .iter()
        .map(|id| {
            id.parse::<i64>()
                .map_err(|e| format!("审批节点「{label}」审批人ID无效: {id}（{e}）"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {e}"))?;
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, COALESCE(NULLIF(real_name, ''), username)
         FROM public.sys_user
         WHERE id = ANY($1) AND tenant_id = $2
           AND status <> 0
           AND (deleted IS NULL OR deleted = 0)
         ORDER BY id ASC",
    )
    .bind(&numeric_ids)
    .bind(tenant_id)
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("查询审批人失败: {e}"))?;

    let mut names = rows.into_iter().collect::<HashMap<i64, String>>();
    let mut members = Vec::with_capacity(ids.len());
    for id in ids {
        let numeric = id.parse::<i64>().unwrap_or_default();
        let Some(name) = names.remove(&numeric) else {
            return Err(format!("审批节点「{label}」的审批人 {id} 不存在或已停用"));
        };
        members.push(ApprovalMember {
            user_id: id.clone(),
            user_name: name,
        });
    }
    Ok(members)
}



