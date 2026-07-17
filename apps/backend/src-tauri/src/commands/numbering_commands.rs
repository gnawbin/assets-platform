//! 编号规则管理 Command
//!
//! 对应 lib.rs 中的 get_numbering_rules / get_numbering_rule / save_numbering_rule / reset_numbering_sequence

use crate::service;
use crate::service::numbering_service::{NumberingRuleInput, NumberingRuleResponse};

/// 获取所有编号规则
#[tauri::command]
pub async fn get_numbering_rules() -> Result<Vec<NumberingRuleResponse>, String> {
    service::numbering_service::get_rules().await
}

/// 根据业务类型获取单条规则
#[tauri::command]
pub async fn get_numbering_rule(bizType: String) -> Result<NumberingRuleResponse, String> {
    service::numbering_service::get_rule(&bizType).await
}

/// 保存编号规则（新增或更新）
#[tauri::command]
pub async fn save_numbering_rule(
    id: Option<String>,
    input: NumberingRuleInput,
) -> Result<NumberingRuleResponse, String> {
    let rule_id: Option<i64> = match id {
        Some(s) if !s.is_empty() => Some(s.parse().map_err(|e| format!("无效的规则ID: {}", e))?),
        _ => None,
    };
    // current_user_id 暂传 None，后续可对接登录上下文
    service::numbering_service::save_rule(rule_id, input, None).await
}

/// 重置流水号
#[tauri::command]
pub async fn reset_numbering_sequence(bizType: String, resetKey: String) -> Result<(), String> {
    service::numbering_service::reset_sequence(&bizType, &resetKey).await
}
