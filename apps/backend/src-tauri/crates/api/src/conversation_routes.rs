//! 智能问答会话 HTTP API 路由
//!
//! 对应原 Tauri 命令：`create_conversation` / `send_message` / `get_conversations` /
//! `get_conversation_messages` / `update_conversation_title` / `delete_conversation`。
//!
//! # 请求形状约定（全 HTTP 化统一原则）
//!
//! 前端 `httpAdapter.extractParams()` 只把**路径占位符**对应的参数从请求体里剥掉，
//! 其余参数原样作为 JSON body（GET / DELETE 则作为 query string）发送。
//!
//! | 命令 | 路由 | 路径参数 | body / query |
//! | --- | --- | --- | --- |
//! | `get_conversations(userId, page, pageSize)` | `GET /api/conversations` | — | `?userId&page&pageSize` |
//! | `create_conversation(userId, question, …)` | `POST /api/conversations` | — | `{userId, question, bindTreeNodeId?, providerId?, modelName?, attachments?}` |
//! | `get_conversation_messages(convId, page, pageSize)` | `GET /api/conversations/{convId}/messages` | `convId` | `?page&pageSize` |
//! | `send_message(convId, userId, question, …)` | `POST /api/conversations/{convId}/messages` | `convId` | `{userId, question, providerId?, modelName?, attachments?}` |
//! | `update_conversation_title(convId, title)` | `PUT /api/conversations/{convId}/title` | `convId` | `{title}` |
//! | `delete_conversation(convId)` | `DELETE /api/conversations/{convId}` | `convId` | — |
//!
//! # 归属校验（与桌面命令的关键差异）
//!
//! 桌面命令信任前端传入的 `userId`；HTTP 形态一律以**令牌 `sub`** 为准，
//! body 里的 `userId` 仅用于比对告警，绝不采信（否则任何人都能读他人会话）。
//! 所有按 `convId` 操作的端点都会经过 `ConversationService::ensure_conversation_owned`
//! 或等价的服务端过滤，越权时返回 404（而非 403），避免暴露他人会话是否存在。

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::{Extension, Json};
use serde::Deserialize;
use tracing::{info, warn};

use service::conversation_service::{AttachmentParam, ConversationResponse, ConversationService};
use service::llm_gateway_service::LLMRouter;

use super::auth::UserContext;
use super::response::{ApiError, ApiResponse};

/// 会话路由状态：与 SSE 对话共享同一个 LLM Router
pub struct ConversationRouterState {
    pub llm_router: Arc<LLMRouter>,
}

// ======================== 请求 DTO ========================

/// 创建会话请求（与 `create_conversation` 命令参数同名）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateConversationRequest {
    /// 用户 ID —— 客户端可伪造，服务端以令牌 `sub` 为准
    #[serde(rename = "userId", default)]
    pub user_id: Option<String>,
    /// 用户问题
    pub question: String,
    /// 绑定的知识库目录节点 ID（非法值报 400）
    #[serde(rename = "bindTreeNodeId", default)]
    pub bind_tree_node_id: Option<String>,
    /// 指定 LLM 厂商 ID（非法值静默忽略，与桌面命令一致）
    #[serde(rename = "providerId", default)]
    pub provider_id: Option<String>,
    /// 指定模型名
    #[serde(rename = "modelName", default)]
    pub model_name: Option<String>,
    /// 多模态附件
    #[serde(default)]
    pub attachments: Option<Vec<AttachmentParam>>,
}

/// 续聊请求（与 `send_message` 命令参数同名；`convId` 走路径）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SendMessageRequest {
    /// 用户 ID —— 客户端可伪造，服务端以令牌 `sub` 为准
    #[serde(rename = "userId", default)]
    pub user_id: Option<String>,
    /// 用户问题
    pub question: String,
    /// 指定 LLM 厂商 ID（非法值静默忽略）
    #[serde(rename = "providerId", default)]
    pub provider_id: Option<String>,
    /// 指定模型名
    #[serde(rename = "modelName", default)]
    pub model_name: Option<String>,
    /// 多模态附件
    #[serde(default)]
    pub attachments: Option<Vec<AttachmentParam>>,
}

/// 会话列表查询参数（GET，前端以 query string 发送）
#[derive(Debug, Deserialize)]
pub struct ListConversationsQuery {
    /// 用户 ID —— 仅比对告警，服务端以令牌 `sub` 为准
    #[serde(rename = "userId", default)]
    pub user_id: Option<String>,
    /// 页码（从 1 开始，缺省 1）
    #[serde(default)]
    pub page: Option<i32>,
    /// 每页条数（缺省 20）
    #[serde(rename = "pageSize", default)]
    pub page_size: Option<i32>,
}

/// 消息列表查询参数（GET）
#[derive(Debug, Deserialize)]
pub struct ListMessagesQuery {
    /// 页码（从 1 开始，缺省 1）
    #[serde(default)]
    pub page: Option<i32>,
    /// 每页条数（缺省 100，与命令一致）
    #[serde(rename = "pageSize", default)]
    pub page_size: Option<i32>,
}

/// 更新标题请求：`{ title }`（`convId` 走路径）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateTitleRequest {
    /// 新标题
    pub title: String,
}


// ======================== 参数与错误处理 ========================

/// 服务层错误 → HTTP 语义
///
/// 归属校验失败（`CONVERSATION_NOT_FOUND`）统一映射为 **404**：既表示不存在，
/// 也表示不属于当前用户，调用方无法据此判断他人会话是否存在。
fn map_service_error(message: String) -> ApiError {
    if message.contains("未找到") {
        ApiError::not_found(message)
    } else if message.starts_with("无效的") {
        ApiError::bad_request(message)
    } else {
        ApiError::internal_error(message)
    }
}

/// 解析路径里的会话 ID（失败 → 400）
fn parse_conv_id(raw: &str) -> Result<i64, ApiError> {
    raw.trim()
        .parse::<i64>()
        .map_err(|e| ApiError::bad_request(format!("无效的会话ID: {}", e)))
}

/// 解析可选的字符串 ID：空串视为未指定，格式非法静默忽略（与桌面命令一致）
fn parse_opt_id(raw: Option<&str>) -> Option<i64> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
}

/// 解析必须合法的可选 ID（`bindTreeNodeId`：桌面命令对非法值报错）
fn parse_opt_id_strict(raw: Option<&str>) -> Result<Option<i64>, ApiError> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s
            .parse::<i64>()
            .map(Some)
            .map_err(|e| ApiError::bad_request(format!("无效的节点ID: {}", e))),
        None => Ok(None),
    }
}

/// 分页参数归一化：缺省值与 Tauri 命令一致，并夹到 `>= 1` 避免负数 OFFSET
fn normalize_page(page: Option<i32>, page_size: Option<i32>, default_page_size: i32) -> (i32, i32) {
    (page.unwrap_or(1).max(1), page_size.unwrap_or(default_page_size).max(1))
}

/// 以令牌 `sub` 为准解析调用者身份
///
/// 受保护路由上 `auth_middleware` 必然注入过 `UserContext`；缺失说明路由挂在认证
/// 中间件之外，此时返回 401（而不是把请求当匿名放行）。body 里的 `userId` 只用于
/// 比对告警：与令牌不一致时**不报错**，而是按令牌继续并留痕，避免误伤旧前端。
fn resolve_user_id(
    token_user_id: Option<i64>,
    body_user_id: Option<&str>,
) -> Result<i64, ApiError> {
    let token_user_id = token_user_id.ok_or_else(|| ApiError::unauthorized("未认证"))?;

    if let Some(raw) = body_user_id {
        if let Ok(body_id) = raw.trim().parse::<i64>() {
            if body_id != token_user_id {
                warn!(
                    "[HTTP] 请求体 userId={} 与令牌 sub={} 不一致，以令牌为准（防冒用身份）",
                    body_id, token_user_id
                );
            }
        }
    }

    Ok(token_user_id)
}

/// 从请求上下文取令牌身份（`None` 表示中间件未注入）
fn token_user_id(user_ctx: &Option<Extension<UserContext>>) -> Option<i64> {
    user_ctx.as_ref().map(|Extension(ctx)| ctx.user_id)
}

// ======================== 路由 handler ========================

/// GET /api/conversations - 当前用户的会话列表
///
/// 只返回令牌 `sub` 自己的会话；query 里的 `userId` 不参与过滤。
#[utoipa::path(
    get,
    path = "/api/conversations",
    tag = "智能问答",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "会话列表（`data` = `{items, total, page, pageSize}`，与命令返回同形）"),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn list_conversations(
    user_ctx: Option<Extension<UserContext>>,
    Query(q): Query<ListConversationsQuery>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    let user_id = resolve_user_id(token_user_id(&user_ctx), q.user_id.as_deref())?;
    let (page, page_size) = normalize_page(q.page, q.page_size, 20);
    info!(
        "[HTTP] GET /api/conversations user_id={} page={} page_size={}",
        user_id, page, page_size
    );

    let (list, total) = ConversationService::get_conversations(user_id, page, page_size)
        .await
        .map_err(map_service_error)?;

    // 形状与 Tauri 命令 get_conversations 一致（total 为字符串，避免 JS 精度丢失）
    Ok(Json(ApiResponse::success(serde_json::json!({
        "items": list,
        "total": total.to_string(),
        "page": page,
        "pageSize": page_size,
    }))))
}

/// POST /api/conversations - 新建会话并回答首个问题（同步返回完整回答）
///
/// 需要 SSE 流式输出请改用 `POST /api/chat/stream`。
#[utoipa::path(
    post,
    path = "/api/conversations",
    tag = "智能问答",
    security(("bearer_auth" = [])),
    request_body = CreateConversationRequest,
    responses(
        (status = 200, description = "会话 ID + 回答 + 引用资产（`data` 为 `ConversationResponse`）"),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn create_conversation(
    State(state): State<Arc<ConversationRouterState>>,
    user_ctx: Option<Extension<UserContext>>,
    Json(req): Json<CreateConversationRequest>,
) -> Result<Json<ApiResponse<ConversationResponse>>, ApiError> {
    let user_id = resolve_user_id(token_user_id(&user_ctx), req.user_id.as_deref())?;
    let bind_tree_node_id = parse_opt_id_strict(req.bind_tree_node_id.as_deref())?;
    let provider_id = parse_opt_id(req.provider_id.as_deref());
    let attachments = req.attachments.unwrap_or_default();
    info!(
        "[HTTP] POST /api/conversations user_id={} attachments={}",
        user_id,
        attachments.len()
    );

    let result = if attachments.is_empty() {
        ConversationService::create_conversation_and_answer(
            user_id,
            &req.question,
            bind_tree_node_id,
            &state.llm_router,
            provider_id,
            req.model_name,
        )
        .await
    } else {
        ConversationService::create_conversation_and_answer_with_attachments(
            user_id,
            &req.question,
            bind_tree_node_id,
            &state.llm_router,
            provider_id,
            req.model_name,
            &attachments,
        )
        .await
    };

    Ok(Json(ApiResponse::success(result.map_err(map_service_error)?)))
}

/// GET /api/conversations/{convId}/messages - 会话历史消息
///
/// 非本人会话返回 404（不区分「不存在」与「无权访问」）。
#[utoipa::path(
    get,
    path = "/api/conversations/{convId}/messages",
    tag = "智能问答",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "消息列表（`data` 为 `Message[]`）"),
        (status = 400, description = "会话 ID 非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "会话不存在或不属于当前用户", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn list_messages(
    Path(conv_id): Path<String>,
    user_ctx: Option<Extension<UserContext>>,
    Query(q): Query<ListMessagesQuery>,
) -> Result<Json<ApiResponse<Vec<database::models::Message>>>, ApiError> {
    let conv_id = parse_conv_id(&conv_id)?;
    let user_id = resolve_user_id(token_user_id(&user_ctx), None)?;
    let (page, page_size) = normalize_page(q.page, q.page_size, 100);
    info!(
        "[HTTP] GET /api/conversations/{}/messages user_id={} page={} page_size={}",
        conv_id, user_id, page, page_size
    );

    let messages =
        ConversationService::get_conversation_messages_for_user(conv_id, user_id, page, page_size)
            .await
            .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(messages)))
}

/// POST /api/conversations/{convId}/messages - 继续会话（同步返回完整回答）
///
/// 服务端先校验会话归属，再走 RAG + LLM；非本人会话返回 404。
#[utoipa::path(
    post,
    path = "/api/conversations/{convId}/messages",
    tag = "智能问答",
    security(("bearer_auth" = [])),
    request_body = SendMessageRequest,
    responses(
        (status = 200, description = "回答 + 引用资产（`data` 为 `ConversationResponse`）"),
        (status = 400, description = "会话 ID 或参数非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "会话不存在或不属于当前用户", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn send_message(
    State(state): State<Arc<ConversationRouterState>>,
    Path(conv_id): Path<String>,
    user_ctx: Option<Extension<UserContext>>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<ApiResponse<ConversationResponse>>, ApiError> {
    let conv_id = parse_conv_id(&conv_id)?;
    let user_id = resolve_user_id(token_user_id(&user_ctx), req.user_id.as_deref())?;
    let provider_id = parse_opt_id(req.provider_id.as_deref());
    let attachments = req.attachments.unwrap_or_default();
    info!(
        "[HTTP] POST /api/conversations/{}/messages user_id={} attachments={}",
        conv_id,
        user_id,
        attachments.len()
    );

    let result = if attachments.is_empty() {
        ConversationService::continue_conversation(
            conv_id,
            user_id,
            &req.question,
            &state.llm_router,
            provider_id,
            req.model_name,
        )
        .await
    } else {
        ConversationService::continue_conversation_with_attachments(
            conv_id,
            user_id,
            &req.question,
            &state.llm_router,
            provider_id,
            req.model_name,
            &attachments,
        )
        .await
    };

    Ok(Json(ApiResponse::success(result.map_err(map_service_error)?)))
}

/// PUT /api/conversations/{convId}/title - 更新会话标题
#[utoipa::path(
    put,
    path = "/api/conversations/{convId}/title",
    tag = "智能问答",
    security(("bearer_auth" = [])),
    request_body = UpdateTitleRequest,
    responses(
        (status = 200, description = "更新成功"),
        (status = 400, description = "会话 ID 非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "会话不存在或不属于当前用户", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn update_title(
    Path(conv_id): Path<String>,
    user_ctx: Option<Extension<UserContext>>,
    Json(req): Json<UpdateTitleRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let conv_id = parse_conv_id(&conv_id)?;
    let user_id = resolve_user_id(token_user_id(&user_ctx), None)?;
    info!(
        "[HTTP] PUT /api/conversations/{}/title user_id={}",
        conv_id, user_id
    );

    ConversationService::update_conversation_title_for_user(conv_id, user_id, &req.title)
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(())))
}

/// DELETE /api/conversations/{convId} - 删除会话（逻辑删除）
#[utoipa::path(
    delete,
    path = "/api/conversations/{convId}",
    tag = "智能问答",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "删除成功"),
        (status = 400, description = "会话 ID 非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "会话不存在或不属于当前用户", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn delete_conversation(
    Path(conv_id): Path<String>,
    user_ctx: Option<Extension<UserContext>>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let conv_id = parse_conv_id(&conv_id)?;
    let user_id = resolve_user_id(token_user_id(&user_ctx), None)?;
    info!(
        "[HTTP] DELETE /api/conversations/{} user_id={}",
        conv_id, user_id
    );

    ConversationService::delete_conversation_for_user(conv_id, user_id)
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Uri;
    use service::conversation_service::CONVERSATION_NOT_FOUND;

    /// 归属校验失败必须是 404（而不是 403 / 500），且不泄露会话是否存在
    #[test]
    fn not_found_maps_to_404() {
        let err = map_service_error(CONVERSATION_NOT_FOUND.to_string());
        assert_eq!(err.code, 404);
        assert_eq!(err.message, CONVERSATION_NOT_FOUND);
    }

    /// 其它服务层错误按 500 处理，不臆造 4xx
    #[test]
    fn other_errors_map_to_500() {
        let err = map_service_error("数据库连接失败: boom".to_string());
        assert_eq!(err.code, 500);
    }

    /// 路径里的会话 ID 非法 → 400
    #[test]
    fn invalid_conv_id_is_bad_request() {
        assert_eq!(parse_conv_id("123").expect("合法 ID 应解析成功"), 123);
        let err = parse_conv_id("abc").expect_err("非法 ID 应报错");
        assert_eq!(err.code, 400);
    }

    /// `providerId` 非法时静默忽略（与桌面命令 `.and_then(parse.ok())` 一致）
    #[test]
    fn provider_id_is_lenient() {
        assert_eq!(parse_opt_id(Some("12")), Some(12));
        assert_eq!(parse_opt_id(Some("abc")), None);
        assert_eq!(parse_opt_id(Some("  ")), None);
        assert_eq!(parse_opt_id(None), None);
    }

    /// `bindTreeNodeId` 非法时报 400（与桌面命令一致）
    #[test]
    fn bind_tree_node_id_is_strict() {
        assert_eq!(parse_opt_id_strict(Some("9")).expect("合法 ID"), Some(9));
        assert_eq!(parse_opt_id_strict(Some("")).expect("空串视为未指定"), None);
        let err = parse_opt_id_strict(Some("x")).expect_err("非法节点 ID 应报错");
        assert_eq!(err.code, 400);
        assert!(err.message.contains("无效的节点ID"));
    }

    /// 分页缺省值与 Tauri 命令保持一致（会话 20 / 消息 100），且夹到 >= 1
    #[test]
    fn page_defaults_match_tauri_command() {
        assert_eq!(normalize_page(None, None, 20), (1, 20));
        assert_eq!(normalize_page(None, None, 100), (1, 100));
        assert_eq!(normalize_page(Some(3), Some(50), 20), (3, 50));
        assert_eq!(normalize_page(Some(0), Some(-5), 20), (1, 1));
    }

    /// 令牌缺失 → 401（受保护路由不应把请求当匿名放行）
    #[test]
    fn missing_token_is_unauthorized() {
        let err = resolve_user_id(None, Some("7")).expect_err("无令牌必须拒绝");
        assert_eq!(err.code, 401);
    }

    /// body 里的 userId 不得覆盖令牌身份（防冒用他人会话）
    #[test]
    fn body_user_id_cannot_impersonate_token_user() {
        assert_eq!(resolve_user_id(Some(5), Some("5")).expect("一致时正常"), 5);
        // 不一致时以令牌为准，不报错（兼容旧前端）
        assert_eq!(
            resolve_user_id(Some(5), Some("6")).expect("不一致也按令牌继续"),
            5
        );
        // 缺失或非法 body userId 同样以令牌为准
        assert_eq!(resolve_user_id(Some(5), None).expect("缺省正常"), 5);
        assert_eq!(resolve_user_id(Some(5), Some("abc")).expect("非法也按令牌"), 5);
    }

    /// GET 参数使用 camelCase（前端 httpAdapter 以 query string 原样发送）
    #[test]
    fn query_params_are_camel_case() {
        let uri = Uri::from_static("/api/conversations?userId=42&page=2&pageSize=50");
        let Query(q) =
            Query::<ListConversationsQuery>::try_from_uri(&uri).expect("应能解析会话列表 query");
        assert_eq!(q.user_id.as_deref(), Some("42"));
        assert_eq!(q.page, Some(2));
        assert_eq!(q.page_size, Some(50));

        let uri = Uri::from_static("/api/conversations/1/messages?page=1&pageSize=100");
        let Query(q) =
            Query::<ListMessagesQuery>::try_from_uri(&uri).expect("应能解析消息列表 query");
        assert_eq!(q.page, Some(1));
        assert_eq!(q.page_size, Some(100));
    }

    /// 前端 `create_conversation` 的 body 必须能原样反序列化
    ///
    /// 前端会额外发送 `modelId`（命令只认 `modelName`），未知字段应被忽略而不是报错。
    #[test]
    fn create_body_matches_command_arguments() {
        let json = r#"{
            "userId": "7",
            "question": "资产盘点的流程是什么？",
            "bindTreeNodeId": "9",
            "providerId": "3",
            "modelId": "gpt-4o-mini",
            "attachments": [{"type": "image", "name": "a.png", "dataUrl": "data:image/png;base64,AA"}]
        }"#;
        let req: CreateConversationRequest = serde_json::from_str(json).expect("body 应可反序列化");
        assert_eq!(req.user_id.as_deref(), Some("7"));
        assert_eq!(req.bind_tree_node_id.as_deref(), Some("9"));
        assert_eq!(req.provider_id.as_deref(), Some("3"));
        assert_eq!(req.model_name, None);
        assert_eq!(req.attachments.expect("附件应解析").len(), 1);
    }

    /// 续聊 / 改标题 body：路径参数已由前端剥离，剩余字段直接反序列化
    #[test]
    fn send_message_body_matches_command_arguments() {
        let json = r#"{"userId":"7","question":"继续","providerId":"3"}"#;
        let req: SendMessageRequest = serde_json::from_str(json).expect("body 应可反序列化");
        assert_eq!(req.user_id.as_deref(), Some("7"));
        assert_eq!(req.question, "继续");
        assert!(req.attachments.is_none());

        let req: UpdateTitleRequest =
            serde_json::from_str(r#"{"title":"新标题"}"#).expect("标题 body 应可反序列化");
        assert_eq!(req.title, "新标题");
    }
}
