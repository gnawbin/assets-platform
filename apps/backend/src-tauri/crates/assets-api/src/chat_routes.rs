//! 智能问答 HTTP API 路由
//!
//! 提供 SSE 流式输出端点 POST /api/chat/stream
//! 保留现有 Tauri invoke 的非流式对话作为默认方案。

use std::sync::Arc;

use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use tokio::sync::mpsc;

use assets_service::conversation_service::{
    AttachmentParam, ConversationService,
};

/// SSE 流式对话请求体
#[derive(Debug, Deserialize)]
pub struct ChatStreamRequest {
    pub user_id: String,
    pub question: String,
    pub conv_id: Option<String>,
    /// 附件列表（图片走 dataUrl，视频/音频/文档走 S3 url）
    #[serde(default)]
    pub attachments: Vec<AttachmentParam>,
}

/// SSE 流式对话状态
pub struct ChatRouterState {
    pub llm_router: Arc<assets_service::llm_gateway_service::LLMRouter>,
}

/// SSE 流式对话端点
///
/// POST /api/chat/stream
/// body: { "user_id": "xxx", "question": "xxx", "conv_id": "xxx", "attachments": [...] }
///
/// 返回 SSE 事件流：
/// event: token → { "text": "..." }
/// event: done → { "convId": "...", "citedAssets": [...], "usage": {...} }
/// event: error → { "message": "..." }
pub async fn chat_stream(
    State(state): State<Arc<ChatRouterState>>,
    Json(params): Json<ChatStreamRequest>,
) -> impl IntoResponse {
    let user_id: i64 = match params.user_id.parse() {
        Ok(id) => id,
        Err(e) => {
            return Sse::new(futures::stream::once(async move {
                Ok::<_, axum::Error>(Event::default().event("error").data(
                    serde_json::json!({"message": format!("无效的用户ID: {}", e)}).to_string(),
                ))
            }))
            .into_response();
        }
    };

    let (sse_tx, sse_rx) = mpsc::channel::<String>(64);

    let llm_router = state.llm_router.clone();
    let question = params.question.clone();
    let conv_id = params.conv_id.clone();
    let attachments = params.attachments.clone();

    // 后台任务执行 RAG + LLM 调用
    tokio::spawn(async move {
        let _ = llm_router.refresh_providers().await;

        // 创建 bridge channel: service 发 String → 转为 SSE 事件格式
        let (string_tx, mut string_rx) = mpsc::channel::<String>(64);
        let bridge_sse_tx = sse_tx.clone();

        // bridge 任务：将 String token 封装为 SSE JSON data
        tokio::spawn(async move {
            while let Some(text) = string_rx.recv().await {
                let data = format!(
                    "event: token\ndata: {}\n\n",
                    serde_json::json!({"text": text}).to_string()
                );
                if bridge_sse_tx.send(data).await.is_err() {
                    break;
                }
            }
        });

        let run = if let Some(existing_conv_id) = conv_id {
            let conv_id_int: i64 = match existing_conv_id.parse() {
                Ok(id) => id,
                Err(e) => {
                    let _ = sse_tx
                        .send(format!(
                            "event: error\ndata: {}\n\n",
                            serde_json::json!({"message": format!("无效的会话ID: {}", e)})
                                .to_string()
                        ))
                        .await;
                    return;
                }
            };

            if attachments.is_empty() {
                ConversationService::continue_conversation_stream(
                    conv_id_int,
                    user_id,
                    &question,
                    &llm_router,
                    string_tx,
                )
                .await
            } else {
                ConversationService::continue_conversation_with_attachments_stream(
                    conv_id_int,
                    user_id,
                    &question,
                    &llm_router,
                    string_tx,
                    &attachments,
                )
                .await
            }
        } else {
            if attachments.is_empty() {
                ConversationService::create_conversation_and_answer_stream(
                    user_id,
                    &question,
                    None,
                    &llm_router,
                    string_tx,
                )
                .await
            } else {
                ConversationService::create_conversation_and_answer_with_attachments_stream(
                    user_id,
                    &question,
                    None,
                    &llm_router,
                    string_tx,
                    &attachments,
                )
                .await
            }
        };

        match run {
            Ok(resp) => {
                let _ = sse_tx
                    .send(format!(
                        "event: done\ndata: {}\n\n",
                        serde_json::json!({
                            "convId": resp.conv_id,
                            "citedAssets": resp.cited_assets,
                            "usage": resp.usage,
                        })
                        .to_string()
                    ))
                    .await;
            }
            Err(e) => {
                let _ = sse_tx
                    .send(format!(
                        "event: error\ndata: {}\n\n",
                        serde_json::json!({"message": e}).to_string()
                    ))
                    .await;
            }
        }
    });

    // 将 receiver 转换为 Stream，直接发送预格式化的 SSE 数据
    let stream = futures::stream::unfold(sse_rx, |mut rx| async {
        rx.recv()
            .await
            .map(|data| (Ok::<_, axum::Error>(Event::default().data(data)), rx))
    });

    Sse::new(stream).into_response()
}
