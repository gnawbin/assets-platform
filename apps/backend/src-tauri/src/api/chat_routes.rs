//! 智能问答 HTTP API 路由
//!
//! 提供 SSE 流式输出端点 /api/chat/stream
//! 保留现有 Tauri invoke 的非流式对话作为默认方案。

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use futures::stream::Stream;
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::service::conversation_service::ConversationService;

/// SSE 查询参数
#[derive(Debug, Deserialize)]
pub struct ChatStreamQuery {
    pub user_id: String,
    pub question: String,
    pub conv_id: Option<String>,
}

/// SSE 流式对话状态
pub struct ChatRouterState {
    pub llm_router: Arc<crate::service::llm_gateway_service::LLMRouter>,
}

/// SSE 流式对话端点
///
/// GET /api/chat/stream?user_id=xxx&question=xxx&conv_id=xxx
///
/// 返回 SSE 事件流：
/// event: token → { "text": "..." }
/// event: done → { "convId": "...", "citedAssets": [...], "usage": {...} }
/// event: error → { "message": "..." }
pub async fn chat_stream(
    State(state): State<Arc<ChatRouterState>>,
    Query(params): Query<ChatStreamQuery>,
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

    let (sse_tx, mut sse_rx) = mpsc::channel::<String>(64);

    let llm_router = state.llm_router.clone();
    let question = params.question.clone();
    let conv_id = params.conv_id.clone();

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

        if let Some(existing_conv_id) = conv_id {
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

            match ConversationService::continue_conversation_stream(
                conv_id_int,
                user_id,
                &question,
                &llm_router,
                string_tx,
            )
            .await
            {
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
        } else {
            match ConversationService::create_conversation_and_answer_stream(
                user_id,
                &question,
                None,
                &llm_router,
                string_tx,
            )
            .await
            {
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
