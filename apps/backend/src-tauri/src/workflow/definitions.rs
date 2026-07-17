//! 流程定义
//!
//! 使用 wfe-core 的 WorkflowBuilder 构建各业务审批流程的定义。
//! 当前支持：
//! - asset_receive: 设备领用
//! - asset_repair: 设备维修
//! - asset_purchase: 资产采购（含条件分支预留）

use serde::{Deserialize, Serialize};
use serde_json::json;
use wfe_core::builder::WorkflowBuilder;
use wfe_core::models::WorkflowDefinition;

use super::steps::{ApprovalStep, AutoStep, NotifyStep};

/// 工作流数据（存储在 workflow.data 中）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalWorkflowData {
    /// 业务类型
    pub biz_type: String,
    /// 业务ID
    pub biz_id: i64,
    /// 申请人ID
    pub applicant_id: i64,
    /// 申请人部门ID
    pub department_id: Option<i64>,
    /// 上级ID（审批链第一步）
    pub superior_id: Option<i64>,
    /// 设备管理员ID
    pub asset_manager_id: Option<i64>,
    /// 申请金额（条件分支用）
    pub amount: Option<f64>,
    /// 资产分类ID（动态路由用）
    pub category_id: Option<i64>,
}

/// 设备领用流程
///
/// 流程：申请人提交 → 上级审批 → 设备管理员审批 → 通知领用人 → 更新资产状态
pub fn asset_receive_workflow() -> WorkflowDefinition {
    WorkflowBuilder::<ApprovalWorkflowData>::new()
        .start_with::<ApprovalStep>()
        .name("申请人提交申请")
        .config(json!({
            "role": "applicant",
            "title": "设备领用申请",
            "timeout_seconds": 0
        }))
        .then::<ApprovalStep>()
        .name("上级审批")
        .config(json!({
            "role": "applicant_superior",
            "title": "设备领用 - 上级审批",
            "timeout_seconds": 604800 // 7天超时
        }))
        .then::<ApprovalStep>()
        .name("设备管理员审批")
        .config(json!({
            "role": "asset_manager",
            "title": "设备领用 - 设备管理员审批",
            "timeout_seconds": 604800
        }))
        .then::<NotifyStep>()
        .name("通知领用人")
        .config(json!({
            "notify_type": "站内信"
        }))
        .then::<AutoStep>()
        .name("更新资产状态")
        .end_workflow()
        .build("asset_receive", 1)
}

/// 设备维修流程
///
/// 流程：申请人提交 → 部门负责人审批 → 设备管理员确认 → 通知申请人
pub fn asset_repair_workflow() -> WorkflowDefinition {
    WorkflowBuilder::<ApprovalWorkflowData>::new()
        .start_with::<ApprovalStep>()
        .name("申请人提交维修申请")
        .config(json!({
            "role": "applicant",
            "title": "设备维修申请"
        }))
        .then::<ApprovalStep>()
        .name("部门负责人审批")
        .config(json!({
            "role": "dept_head",
            "title": "设备维修 - 部门审批",
            "timeout_seconds": 604800
        }))
        .then::<ApprovalStep>()
        .name("设备管理员确认")
        .config(json!({
            "role": "asset_manager",
            "title": "设备维修 - 管理员确认维修方案",
            "timeout_seconds": 604800
        }))
        .then::<NotifyStep>()
        .name("通知申请人")
        .end_workflow()
        .build("asset_repair", 1)
}

/// 资产采购流程
///
/// 流程：申请人提交 → 部门负责人审批 → 财务审批 → 通知采购执行
/// TODO: 使用 if_step 实现金额条件分支（>5000 加总经理审批）
pub fn asset_purchase_workflow() -> WorkflowDefinition {
    WorkflowBuilder::<ApprovalWorkflowData>::new()
        .start_with::<ApprovalStep>()
        .name("申请人提交采购申请")
        .config(json!({
            "role": "applicant",
            "title": "资产采购申请"
        }))
        .then::<ApprovalStep>()
        .name("部门负责人审批")
        .config(json!({
            "role": "dept_head",
            "title": "采购 - 部门审批",
            "timeout_seconds": 604800
        }))
        .then::<ApprovalStep>()
        .name("财务审批")
        .config(json!({
            "role": "finance",
            "title": "采购 - 财务审批",
            "timeout_seconds": 604800
        }))
        .then::<NotifyStep>()
        .name("通知采购执行")
        .end_workflow()
        .build("asset_purchase", 1)
}

/// 根据流程定义ID获取对应的 WorkflowDefinition
pub fn get_definition(def_id: &str) -> Result<WorkflowDefinition, String> {
    match def_id {
        "asset_receive" => Ok(asset_receive_workflow()),
        "asset_repair" => Ok(asset_repair_workflow()),
        "asset_purchase" => Ok(asset_purchase_workflow()),
        other => Err(format!("未知的工作流定义: {}", other)),
    }
}

/// 获取所有已注册的流程定义ID列表
pub fn list_definition_ids() -> Vec<String> {
    vec![
        "asset_receive".to_string(),
        "asset_repair".to_string(),
        "asset_purchase".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试设备领用流程定义的构建
    #[test]
    fn test_asset_receive_workflow_build() {
        let def = asset_receive_workflow();
        assert_eq!(def.id, "asset_receive");
        assert_eq!(def.version, 1);
        assert!(!def.steps.is_empty());
        assert_eq!(def.steps.len(), 5, "设备领用应有5个步骤");
    }

    /// 测试设备维修流程定义的构建
    #[test]
    fn test_asset_repair_workflow_build() {
        let def = asset_repair_workflow();
        assert_eq!(def.id, "asset_repair");
        assert!(!def.steps.is_empty());
        assert_eq!(def.steps.len(), 4, "设备维修应有4个步骤");
    }

    /// 测试采购流程定义的构建
    #[test]
    fn test_asset_purchase_workflow_build() {
        let def = asset_purchase_workflow();
        assert_eq!(def.id, "asset_purchase");
        assert!(!def.steps.is_empty());
        assert_eq!(def.steps.len(), 4, "资产采购应有4个步骤");
    }

    /// 测试 get_definition 查找
    #[test]
    fn test_get_definition() {
        let def = get_definition("asset_receive").unwrap();
        assert_eq!(def.id, "asset_receive");

        let err = get_definition("unknown_def").unwrap_err();
        assert!(err.contains("未知的工作流定义"));
    }

    /// 测试 list_definition_ids
    #[test]
    fn test_list_definition_ids() {
        let ids = list_definition_ids();
        assert!(ids.contains(&"asset_receive".to_string()));
        assert!(ids.contains(&"asset_repair".to_string()));
        assert!(ids.contains(&"asset_purchase".to_string()));
    }

    /// 测试流程定义 JSON 序列化/反序列化
    #[test]
    fn test_workflow_definition_serde() {
        let def = asset_receive_workflow();
        let json = serde_json::to_value(&def).unwrap();
        assert_eq!(json["id"], "asset_receive");

        let deserialized: WorkflowDefinition = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.id, def.id);
        assert_eq!(deserialized.steps.len(), def.steps.len());
    }

    /// 测试 ApprovalWorkflowData 序列化
    #[test]
    fn test_approval_workflow_data_serde() {
        let data = ApprovalWorkflowData {
            biz_type: "receive".into(),
            biz_id: 12345,
            applicant_id: 1001,
            department_id: Some(2001),
            superior_id: Some(1002),
            asset_manager_id: Some(1003),
            amount: None,
            category_id: None,
        };

        let json = serde_json::to_value(&data).unwrap();
        assert_eq!(json["biz_type"], "receive");
        assert_eq!(json["biz_id"], 12345);

        // 反序列化回来
        let deserialized: ApprovalWorkflowData = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.biz_id, data.biz_id);
        assert_eq!(deserialized.superior_id, data.superior_id);
    }
}
