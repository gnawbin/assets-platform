//! 自定义审批步骤（StepBody 实现）
//!
//! 基于 wfe-core 的 StepBody trait 实现三种审批步骤：
//! - ApprovalStep: 通用审批步骤，等待人工审批事件（通过/驳回）
//! - AutoStep: 系统自动执行的步骤
//! - NotifyStep: 发送通知的步骤

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use wfe_core::models::ExecutionResult;
use wfe_core::traits::step::{StepBody, StepExecutionContext};

/// 审批步骤配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalStepConfig {
    /// 审批角色标识
    /// - "applicant" — 发起人
    /// - "applicant_superior" — 发起人上级
    /// - "dept_head" — 部门负责人
    /// - "asset_manager" — 设备管理员
    /// - "finance" — 财务
    /// - "admin" — 管理员
    /// - "system" — 系统自动
    pub role: String,
    /// 审批人ID（可选，不指定则按角色动态查找）
    pub approver_id: Option<i64>,
    /// 审批标题
    pub title: String,
    /// 超时时间（秒），0 表示不超时
    pub timeout_seconds: u64,
}

/// 审批事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalEvent {
    pub workflow_id: String,
    pub step_id: usize,
    pub action: String, // "approve" | "reject"
    pub comment: String,
    pub user_id: i64,
}

/// 通用审批步骤（等待人工审批事件）
///
/// 当步骤第一次执行时，返回 `wait_for_event` 等待审批事件。
/// 当审批事件发布后重新执行时，检查事件数据并决定推进或驳回。
#[derive(Default)]
pub struct ApprovalStep {
    /// 步骤配置（通过 step_config 传入）
    pub config: Option<ApprovalStepConfig>,
}

#[async_trait]
impl StepBody for ApprovalStep {
    async fn run(&mut self, ctx: &StepExecutionContext<'_>) -> wfe_core::Result<ExecutionResult> {
        let config: ApprovalStepConfig = ctx
            .step
            .step_config
            .as_ref()
            .and_then(|c| serde_json::from_value(c.clone()).ok())
            .unwrap_or(ApprovalStepConfig {
                role: "unknown".into(),
                approver_id: None,
                title: "审批".into(),
                timeout_seconds: 0,
            });

        // 检查是否已被审批事件唤醒
        if ctx.execution_pointer.event_published {
            // 事件已到达，记录审批结果
            let event_data = ctx
                .execution_pointer
                .event_data
                .as_ref()
                .ok_or_else(|| wfe_core::WfeError::StepExecution("缺少审批事件数据".into()))?;

            let action = event_data["action"].as_str().unwrap_or("reject");

            if action == "reject" {
                // 驳回：工作流结束
                return Err(wfe_core::WfeError::StepExecution("审批被驳回".into()));
            }

            // 通过：继续下一步
            tracing::info!(
                "审批通过: workflow={}, step={}, config={:?}",
                ctx.workflow.id,
                ctx.step.id,
                config,
            );
            return Ok(ExecutionResult::next());
        }

        // 未审批：等待审批事件
        let event_key = format!("{}-approval-{}", ctx.workflow.id, ctx.step.id);

        tracing::info!(
            "等待审批: workflow={}, step={}, event_key={}",
            ctx.workflow.id,
            ctx.step.id,
            event_key,
        );

        Ok(ExecutionResult::wait_for_event(
            "approval.event",
            &event_key,
            chrono::Utc::now(),
        ))
    }
}

/// 系统自动步骤（无需人工审批）
///
/// 自动执行并立即推进到下一步。
#[derive(Default)]
pub struct AutoStep;

#[async_trait]
impl StepBody for AutoStep {
    async fn run(&mut self, _ctx: &StepExecutionContext<'_>) -> wfe_core::Result<ExecutionResult> {
        // 自动执行的逻辑，比如更新资产状态等
        tracing::info!("AutoStep 自动执行完成");
        Ok(ExecutionResult::next())
    }
}

/// 通知步骤（发送通知给指定人）
#[derive(Default)]
pub struct NotifyStep;

#[async_trait]
impl StepBody for NotifyStep {
    async fn run(&mut self, ctx: &StepExecutionContext<'_>) -> wfe_core::Result<ExecutionResult> {
        // 从 workflow.data 中解析通知信息
        let data = &ctx.workflow.data;
        let notify_user_id = data["notify_user_id"].as_i64().unwrap_or(0);

        // TODO: 调用通知服务（站内信/邮件/钉钉）
        tracing::info!(
            "发送通知: workflow={}, user_id={}",
            ctx.workflow.id,
            notify_user_id
        );

        Ok(ExecutionResult::next())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试 ApprovalStepConfig 的 JSON 序列化和反序列化
    #[test]
    fn test_approval_step_config_serde() {
        let config = ApprovalStepConfig {
            role: "applicant_superior".into(),
            approver_id: Some(1001),
            title: "设备领用 - 上级审批".into(),
            timeout_seconds: 604800,
        };

        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["role"], "applicant_superior");
        assert_eq!(json["approver_id"], 1001);
        assert_eq!(json["title"], "设备领用 - 上级审批");
        assert_eq!(json["timeout_seconds"], 604800);

        // 反序列化回来
        let deserialized: ApprovalStepConfig = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.role, config.role);
        assert_eq!(deserialized.approver_id, config.approver_id);
    }

    /// 测试 ApprovalEvent 的序列化
    #[test]
    fn test_approval_event_serde() {
        let event = ApprovalEvent {
            workflow_id: "wf-recv-123".into(),
            step_id: 1,
            action: "approve".into(),
            comment: "同意".into(),
            user_id: 1001,
        };

        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["action"], "approve");
        assert_eq!(json["user_id"], 1001);
    }

    /// 测试 ApprovalStep 的默认值
    #[test]
    fn test_approval_step_default() {
        let step = ApprovalStep::default();
        assert!(step.config.is_none());
    }

    /// 测试 AutoStep 的默认值
    #[test]
    fn test_auto_step_default() {
        let step = AutoStep::default();
        let _ = step; // 确保可以创建
    }

    /// 测试 NotifyStep 的默认值
    #[test]
    fn test_notify_step_default() {
        let step = NotifyStep::default();
        let _ = step; // 确保可以创建
    }
}
