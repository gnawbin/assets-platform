//! 对话系统 Tauri Command

use std::sync::Arc;

use crate::service::conversation_service::{
    AttachmentParam, ConversationResponse, ConversationService,
};
use crate::service::llm_gateway_service::LLMRouter;

/// 创建新会话并发送第一条消息
#[tauri::command]
pub async fn create_conversation(
    userId: String,
    question: String,
    attachments: Option<Vec<AttachmentParam>>,
    bindTreeNodeId: Option<String>,
    providerId: Option<String>,
    modelName: Option<String>,
    router: tauri::State<'_, Arc<LLMRouter>>,
) -> Result<ConversationResponse, String> {
    let user_id: i64 = userId.parse().map_err(|e| format!("无效的用户ID: {}", e))?;
    let bind_id = match bindTreeNodeId {
        Some(ref id) if !id.is_empty() => Some(
            id.parse::<i64>()
                .map_err(|e| format!("无效的节点ID: {}", e))?,
        ),
        _ => None,
    };
    let pid = providerId.and_then(|id| id.parse::<i64>().ok());
    let atts = attachments.unwrap_or_default();

    if atts.is_empty() {
        ConversationService::create_conversation_and_answer(
            user_id,
            &question,
            bind_id,
            router.inner(),
            pid,
            modelName,
        )
        .await
    } else {
        ConversationService::create_conversation_and_answer_with_attachments(
            user_id,
            &question,
            bind_id,
            router.inner(),
            pid,
            modelName,
            &atts,
        )
        .await
    }
}

/// 继续已有会话
#[tauri::command]
pub async fn send_message(
    convId: String,
    userId: String,
    question: String,
    attachments: Option<Vec<AttachmentParam>>,
    providerId: Option<String>,
    modelName: Option<String>,
    router: tauri::State<'_, Arc<LLMRouter>>,
) -> Result<ConversationResponse, String> {
    let conv_id: i64 = convId.parse().map_err(|e| format!("无效的会话ID: {}", e))?;
    let user_id: i64 = userId.parse().map_err(|e| format!("无效的用户ID: {}", e))?;
    let pid = providerId.and_then(|id| id.parse::<i64>().ok());
    let atts = attachments.unwrap_or_default();

    if atts.is_empty() {
        ConversationService::continue_conversation(
            conv_id,
            user_id,
            &question,
            router.inner(),
            pid,
            modelName,
        )
        .await
    } else {
        ConversationService::continue_conversation_with_attachments(
            conv_id,
            user_id,
            &question,
            router.inner(),
            pid,
            modelName,
            &atts,
        )
        .await
    }
}

/// 获取会话列表
#[tauri::command]
pub async fn get_conversations(
    userId: String,
    page: Option<i32>,
    pageSize: Option<i32>,
) -> Result<serde_json::Value, String> {
    let user_id: i64 = userId.parse().map_err(|e| format!("无效的用户ID: {}", e))?;
    let p = page.unwrap_or(1);
    let ps = pageSize.unwrap_or(20);

    let (list, total) = ConversationService::get_conversations(user_id, p, ps).await?;

    Ok(serde_json::json!({
        "items": list,
        "total": total.to_string(),
        "page": p,
        "pageSize": ps,
    }))
}

/// 获取会话历史消息
#[tauri::command]
pub async fn get_conversation_messages(
    convId: String,
    page: Option<i32>,
    pageSize: Option<i32>,
) -> Result<Vec<crate::database::models::Message>, String> {
    let conv_id: i64 = convId.parse().map_err(|e| format!("无效的会话ID: {}", e))?;
    let p = page.unwrap_or(1);
    let ps = pageSize.unwrap_or(100);

    ConversationService::get_conversation_messages(conv_id, p, ps).await
}

/// 更新会话标题
#[tauri::command]
pub async fn update_conversation_title(convId: String, title: String) -> Result<(), String> {
    let conv_id: i64 = convId.parse().map_err(|e| format!("无效的会话ID: {}", e))?;
    ConversationService::update_conversation_title(conv_id, &title).await
}

/// 删除会话
#[tauri::command]
pub async fn delete_conversation(convId: String) -> Result<(), String> {
    let conv_id: i64 = convId.parse().map_err(|e| format!("无效的会话ID: {}", e))?;
    ConversationService::delete_conversation(conv_id).await
}
