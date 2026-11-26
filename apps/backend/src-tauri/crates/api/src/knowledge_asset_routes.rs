//! OKF 知识资产 HTTP API 路由
//!
//! 对应原 Tauri 命令：`get_knowledge_asset_by_tree_node` / `get_knowledge_asset` /
//! `list_knowledge_assets` / `create_knowledge_asset` / `update_knowledge_asset` /
//! `delete_knowledge_asset` / `attach_file_to_knowledge`。
//!
//! # 请求形状约定（全 HTTP 化统一原则）
//!
//! 前端 `httpAdapter.extractParams()` 只把**路径占位符**对应的参数从请求体里剥掉，
//! 其余参数原样作为 JSON body（GET / DELETE 则作为 query string）发送。
//! 因此路径占位符必须复用命令参数名：
//!
//! | 命令 | 路由 | 路径参数 | body / query |
//! | --- | --- | --- | --- |
//! | `list_knowledge_assets(okfType?)` | `GET /api/knowledge/asset` | — | `?okfType` |
//! | `get_knowledge_asset_by_tree_node(treeNodeId)` | `GET /api/knowledge/asset/by-tree-node/{treeNodeId}` | `treeNodeId` | — |
//! | `get_knowledge_asset(id)` | `GET /api/knowledge/asset/{id}` | `id` | — |
//! | `create_knowledge_asset(treeNodeId, title, okfType, …)` | `POST /api/knowledge/asset` | — | `{treeNodeId, title, okfType, content?, summary?, source?, tags?, createdBy?}` |
//! | `update_knowledge_asset(id, …)` | `PUT /api/knowledge/asset/{id}` | `id` | `{title?, content?, okfType?, summary?, status?, tags?, updatedBy?}` |
//! | `delete_knowledge_asset(id)` | `DELETE /api/knowledge/asset/{id}` | `id` | — |
//! | `attach_file_to_knowledge(assetId, fileUrl, …)` | `POST /api/knowledge/asset/{assetId}/attach` | `assetId` | `{fileUrl, fileName, fileSize, fileMime, fileMd5}` |
//!
//! # 与桌面命令的差异
//!
//! 桌面命令信任前端传入的 `createdBy` / `updatedBy`；HTTP 形态一律以**令牌 `sub`** 为准，
//! body 里的同名字段仅用于比对告警（不采信），避免伪造审计人。
//!
//! 路由前缀 `/api/knowledge/asset` 与既有 `/api/knowledge/{id}`、`/api/knowledge/tree`、
//! `/api/knowledge/list` 共存：axum（matchit）静态段优先于参数段，不会互相遮蔽
//! （`knowledge_asset_router_builds` 单测锁定了这一点）。

use axum::extract::{Path, Query};
use axum::{Extension, Json};
use serde::Deserialize;
use tracing::{info, warn};

use database::models::KnowledgeAsset;
use service::knowledge_asset_service::{self, KNOWLEDGE_ASSET_NOT_FOUND};

use super::auth::UserContext;
use super::response::{ApiError, ApiResponse};

// ======================== 请求 DTO ========================

/// 创建知识资产请求（与 `create_knowledge_asset` 命令参数同名）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateAssetRequest {
    /// 关联的知识树节点 ID（字符串形式的 i64）
    #[serde(rename = "treeNodeId")]
    pub tree_node_id: String,
    /// 标题
    pub title: String,
    /// OKF 类型：raw_source / concept / fact / rule / param / process / case
    #[serde(rename = "okfType")]
    pub okf_type: String,
    /// Markdown 正文
    #[serde(default)]
    pub content: Option<String>,
    /// 摘要
    #[serde(default)]
    pub summary: Option<String>,
    /// 来源
    #[serde(default)]
    pub source: Option<String>,
    /// 标签
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// 创建人 —— 客户端可伪造，服务端以令牌 `sub` 为准
    #[serde(rename = "createdBy", default)]
    pub created_by: Option<String>,
}

/// 更新知识资产请求（`id` 走路径，其余字段与命令参数同名）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateAssetRequest {
    /// 标题（缺省表示不改）
    #[serde(default)]
    pub title: Option<String>,
    /// Markdown 正文（缺省表示不改）
    #[serde(default)]
    pub content: Option<String>,
    /// OKF 类型（缺省表示不改）
    #[serde(rename = "okfType", default)]
    pub okf_type: Option<String>,
    /// 摘要（缺省表示不改）
    #[serde(default)]
    pub summary: Option<String>,
    /// 状态：draft / valid / outdated / banned
    #[serde(default)]
    pub status: Option<String>,
    /// 标签（缺省表示不改）
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// 更新人 —— 客户端可伪造，服务端以令牌 `sub` 为准
    #[serde(rename = "updatedBy", default)]
    pub updated_by: Option<String>,
}

/// 绑定文件请求（`assetId` 走路径）
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AttachFileRequest {
    /// 对象存储直链
    #[serde(rename = "fileUrl")]
    pub file_url: String,
    /// 原始文件名
    #[serde(rename = "fileName")]
    pub file_name: String,
    /// 文件大小（字节）
    #[serde(rename = "fileSize")]
    pub file_size: i64,
    /// MIME 类型
    #[serde(rename = "fileMime")]
    pub file_mime: String,
    /// 文件 MD5
    #[serde(rename = "fileMd5")]
    pub file_md5: String,
}

/// 列表查询参数（GET，前端以 query string 发送）
#[derive(Debug, Deserialize)]
pub struct ListAssetsQuery {
    /// OKF 类型过滤（缺省返回全部）
    #[serde(rename = "okfType", default)]
    pub okf_type: Option<String>,
}



// ======================== 参数与错误处理 ========================

/// 服务层错误 → HTTP 语义
///
/// `KNOWLEDGE_ASSET_NOT_FOUND` 映射为 **404**（记录不存在或已软删除）；
/// 解析类错误（`无效的…`）映射为 **400**；其余按 500 处理。
fn map_service_error(message: String) -> ApiError {
    if message.contains("未找到") {
        ApiError::not_found(message)
    } else if message.starts_with("无效的") {
        ApiError::bad_request(message)
    } else {
        ApiError::internal_error(message)
    }
}

/// 解析字符串形式的 i64 ID（失败 → 400，文案与桌面命令一致）
fn parse_id(raw: &str, label: &str) -> Result<i64, ApiError> {
    raw.trim()
        .parse::<i64>()
        .map_err(|e| ApiError::bad_request(format!("无效的{}: {}", label, e)))
}

/// 解析可选的字符串 ID：空串 / 非法值视为未指定
///
/// 与桌面命令 `createdBy` / `updatedBy` 的 `.parse().ok()` 行为一致。
fn parse_opt_id(raw: Option<&str>) -> Option<i64> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
}

/// 以令牌 `sub` 为准解析审计主体（创建人 / 更新人）
///
/// 受保护路由上 `auth_middleware` 必然注入过 `UserContext`；缺失说明路由挂在认证
/// 中间件之外，此时返回 401。body 里的 `createdBy` / `updatedBy` 只用于比对告警：
/// 与令牌不一致时**不报错**，而是按令牌继续并留痕，避免误伤旧前端。
fn resolve_actor_id(
    token_user_id: Option<i64>,
    body_user_id: Option<&str>,
) -> Result<i64, ApiError> {
    let token_user_id = token_user_id.ok_or_else(|| ApiError::unauthorized("未认证"))?;

    if let Some(raw) = body_user_id {
        if let Some(body_id) = parse_opt_id(Some(raw)) {
            if body_id != token_user_id {
                warn!(
                    "[HTTP] 请求体操作人={} 与令牌 sub={} 不一致，以令牌为准（防冒用审计人）",
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

/// GET /api/knowledge/asset - 知识资产列表（可按 OKF 类型过滤）
#[utoipa::path(
    get,
    path = "/api/knowledge/asset",
    tag = "知识资产",
    security(("bearer_auth" = [])),
    params(("okfType" = Option<String>, Query, description = "OKF 类型过滤（raw_source / concept / fact / rule / param / process / case）")),
    responses(
        (status = 200, description = "知识资产数组（与命令返回同形）"),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn list_assets(
    user_ctx: Option<Extension<UserContext>>,
    Query(q): Query<ListAssetsQuery>,
) -> Result<Json<ApiResponse<Vec<KnowledgeAsset>>>, ApiError> {
    let user_id = resolve_actor_id(token_user_id(&user_ctx), None)?;
    let okf_type = q
        .okf_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    info!(
        "[HTTP] GET /api/knowledge/asset user_id={} okf_type={:?}",
        user_id, okf_type
    );

    let list = knowledge_asset_service::list_knowledge_assets(okf_type, None)
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(list)))
}

/// GET /api/knowledge/asset/by-tree-node/{treeNodeId} - 按知识树节点取关联资产
///
/// 未关联时返回 `data: null`（与命令的 `Option<KnowledgeAsset>` 一致）。
#[utoipa::path(
    get,
    path = "/api/knowledge/asset/by-tree-node/{treeNodeId}",
    tag = "知识资产",
    security(("bearer_auth" = [])),
    params(("treeNodeId" = String, Path, description = "知识树节点 ID")),
    responses(
        (status = 200, description = "知识资产；未关联时为 null"),
        (status = 400, description = "节点 ID 非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn get_by_tree_node(
    Path(treeNodeId): Path<String>,
    user_ctx: Option<Extension<UserContext>>,
) -> Result<Json<ApiResponse<Option<KnowledgeAsset>>>, ApiError> {
    let user_id = resolve_actor_id(token_user_id(&user_ctx), None)?;
    let tree_node_id = parse_id(&treeNodeId, "节点ID")?;
    info!(
        "[HTTP] GET /api/knowledge/asset/by-tree-node/{} user_id={}",
        tree_node_id, user_id
    );

    let item = knowledge_asset_service::get_knowledge_asset_by_tree_node(tree_node_id)
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(item)))
}

/// GET /api/knowledge/asset/{id} - 单条知识资产
#[utoipa::path(
    get,
    path = "/api/knowledge/asset/{id}",
    tag = "知识资产",
    security(("bearer_auth" = [])),
    params(("id" = String, Path, description = "知识资产 ID")),
    responses(
        (status = 200, description = "知识资产"),
        (status = 400, description = "ID 非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "知识资产不存在或已删除", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn get_asset(
    Path(id): Path<String>,
    user_ctx: Option<Extension<UserContext>>,
) -> Result<Json<ApiResponse<KnowledgeAsset>>, ApiError> {
    let user_id = resolve_actor_id(token_user_id(&user_ctx), None)?;
    let asset_id = parse_id(&id, "ID")?;
    info!("[HTTP] GET /api/knowledge/asset/{} user_id={}", asset_id, user_id);

    let item = knowledge_asset_service::get_knowledge_asset(asset_id)
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(item)))
}

/// POST /api/knowledge/asset - 创建知识资产
///
/// `created_by` 取令牌 `sub`；body 里的 `createdBy` 仅比对告警。
#[utoipa::path(
    post,
    path = "/api/knowledge/asset",
    tag = "知识资产",
    security(("bearer_auth" = [])),
    request_body = CreateAssetRequest,
    responses(
        (status = 200, description = "创建成功，返回完整知识资产"),
        (status = 400, description = "参数错误（如 treeNodeId 非法）", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn create_asset(
    user_ctx: Option<Extension<UserContext>>,
    Json(req): Json<CreateAssetRequest>,
) -> Result<Json<ApiResponse<KnowledgeAsset>>, ApiError> {
    let user_id = resolve_actor_id(token_user_id(&user_ctx), req.created_by.as_deref())?;
    let tree_node_id = parse_id(&req.tree_node_id, "节点ID")?;
    info!(
        "[HTTP] POST /api/knowledge/asset user_id={} tree_node_id={} okf_type={}",
        user_id, tree_node_id, req.okf_type
    );

    let item = knowledge_asset_service::create_knowledge_asset_from_input(
        tree_node_id,
        req.title,
        req.okf_type,
        req.content,
        req.summary,
        req.source,
        req.tags,
        Some(user_id),
    )
    .await
    .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(item)))
}


/// PUT /api/knowledge/asset/{id} - 更新知识资产
///
/// `updated_by` 取令牌 `sub`；body 里的 `updatedBy` 仅比对告警。
#[utoipa::path(
    put,
    path = "/api/knowledge/asset/{id}",
    tag = "知识资产",
    security(("bearer_auth" = [])),
    params(("id" = String, Path, description = "知识资产 ID")),
    request_body = UpdateAssetRequest,
    responses(
        (status = 200, description = "更新成功，返回完整知识资产"),
        (status = 400, description = "ID 非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "知识资产不存在或已删除", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn update_asset(
    Path(id): Path<String>,
    user_ctx: Option<Extension<UserContext>>,
    Json(req): Json<UpdateAssetRequest>,
) -> Result<Json<ApiResponse<KnowledgeAsset>>, ApiError> {
    let user_id = resolve_actor_id(token_user_id(&user_ctx), req.updated_by.as_deref())?;
    let asset_id = parse_id(&id, "ID")?;
    info!("[HTTP] PUT /api/knowledge/asset/{} user_id={}", asset_id, user_id);

    let item = knowledge_asset_service::update_knowledge_asset(
        asset_id,
        req.title.as_deref(),
        req.content.as_deref(),
        req.okf_type.as_deref(),
        req.summary.as_deref(),
        req.status.as_deref(),
        req.tags,
        Some(user_id),
    )
    .await
    .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(item)))
}

/// DELETE /api/knowledge/asset/{id} - 删除知识资产（软删除）
///
/// 记录不存在或已删除时返回 404（重复删除同样 404）。
#[utoipa::path(
    delete,
    path = "/api/knowledge/asset/{id}",
    tag = "知识资产",
    security(("bearer_auth" = [])),
    params(("id" = String, Path, description = "知识资产 ID")),
    responses(
        (status = 200, description = "删除成功"),
        (status = 400, description = "ID 非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "知识资产不存在或已删除", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn delete_asset(
    Path(id): Path<String>,
    user_ctx: Option<Extension<UserContext>>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let user_id = resolve_actor_id(token_user_id(&user_ctx), None)?;
    let asset_id = parse_id(&id, "ID")?;
    info!(
        "[HTTP] DELETE /api/knowledge/asset/{} user_id={}",
        asset_id, user_id
    );

    knowledge_asset_service::delete_knowledge_asset(asset_id)
        .await
        .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(())))
}


/// POST /api/knowledge/asset/{assetId}/attach - 绑定已上传文件到知识资产
#[utoipa::path(
    post,
    path = "/api/knowledge/asset/{assetId}/attach",
    tag = "知识资产",
    security(("bearer_auth" = [])),
    params(("assetId" = String, Path, description = "知识资产 ID")),
    request_body = AttachFileRequest,
    responses(
        (status = 200, description = "绑定成功，返回完整知识资产"),
        (status = 400, description = "资产 ID 非法", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 404, description = "知识资产不存在或已删除", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn attach_file(
    Path(assetId): Path<String>,
    user_ctx: Option<Extension<UserContext>>,
    Json(req): Json<AttachFileRequest>,
) -> Result<Json<ApiResponse<KnowledgeAsset>>, ApiError> {
    let user_id = resolve_actor_id(token_user_id(&user_ctx), None)?;
    let asset_id = parse_id(&assetId, "资产ID")?;
    info!(
        "[HTTP] POST /api/knowledge/asset/{}/attach user_id={} file={}",
        asset_id, user_id, req.file_name
    );

    let item = knowledge_asset_service::attach_file_to_asset(
        asset_id,
        &req.file_url,
        &req.file_name,
        req.file_size,
        &req.file_mime,
        &req.file_md5,
    )
    .await
    .map_err(map_service_error)?;

    Ok(Json(ApiResponse::success(item)))
}


#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Uri;
    use axum::routing::{delete, get, post, put};
    use axum::Router;

    /// 404 / 500 / 400 映射：文案含「未找到」才是 404，避免把内部错误当成“不存在”
    #[test]
    fn not_found_maps_to_404() {
        let err = map_service_error(KNOWLEDGE_ASSET_NOT_FOUND.to_string());
        assert_eq!(err.code, 404);
        assert_eq!(err.message, KNOWLEDGE_ASSET_NOT_FOUND);

        assert_eq!(map_service_error("查询知识资产失败: boom".to_string()).code, 500);
        assert_eq!(map_service_error("无效的节点ID: x".to_string()).code, 400);
    }

    /// 三类 ID 的 400 文案必须与桌面命令一致
    #[test]
    fn invalid_ids_are_bad_request() {
        assert_eq!(parse_id("12", "ID").expect("合法 ID"), 12);
        assert_eq!(parse_id(" 12 ", "ID").expect("两侧空白应被裁剪"), 12);

        let err = parse_id("abc", "节点ID").expect_err("非法节点 ID");
        assert_eq!(err.code, 400);
        assert!(err.message.starts_with("无效的节点ID:"));

        let err = parse_id("abc", "资产ID").expect_err("非法资产 ID");
        assert!(err.message.starts_with("无效的资产ID:"));
    }

    /// 令牌缺失 → 401；body 操作人不得覆盖令牌身份
    #[test]
    fn actor_id_comes_from_token_only() {
        assert_eq!(
            resolve_actor_id(None, Some("7")).expect_err("无令牌必须拒绝").code,
            401
        );
        assert_eq!(resolve_actor_id(Some(5), None).expect("缺省正常"), 5);
        assert_eq!(resolve_actor_id(Some(5), Some("5")).expect("一致正常"), 5);
        // 不一致 / 非法 / 空串 → 仍以令牌为准，不报错（兼容旧前端）
        assert_eq!(resolve_actor_id(Some(5), Some("6")).expect("不一致按令牌"), 5);
        assert_eq!(resolve_actor_id(Some(5), Some("abc")).expect("非法按令牌"), 5);
        assert_eq!(resolve_actor_id(Some(5), Some("")).expect("空串按令牌"), 5);
    }

    /// GET 参数使用 camelCase（httpAdapter 以 query string 原样发送）
    #[test]
    fn list_query_is_camel_case() {
        let uri = Uri::from_static("/api/knowledge/asset?okfType=concept");
        let Query(q) = Query::<ListAssetsQuery>::try_from_uri(&uri).expect("应能解析 query");
        assert_eq!(q.okf_type.as_deref(), Some("concept"));

        let uri = Uri::from_static("/api/knowledge/asset");
        let Query(q) = Query::<ListAssetsQuery>::try_from_uri(&uri).expect("缺省应可解析");
        assert!(q.okf_type.is_none());
    }

    /// 前端 `createKnowledgeAsset` 的 body 必须能原样反序列化（未知字段忽略）
    #[test]
    fn create_body_matches_command_arguments() {
        let json = r##"{
            "treeNodeId": "9",
            "title": "扳手使用说明",
            "okfType": "concept",
            "content": "# 正文",
            "tags": ["扳手", "工具"],
            "createdBy": "7",
            "unknownField": 1
        }"##;
        let req: CreateAssetRequest = serde_json::from_str(json).expect("body 应可反序列化");
        assert_eq!(req.tree_node_id, "9");
        assert_eq!(req.title, "扳手使用说明");
        assert_eq!(req.okf_type, "concept");
        assert_eq!(req.content.as_deref(), Some("# 正文"));
        assert_eq!(req.tags.as_ref().map(|t| t.len()), Some(2));
        assert_eq!(req.created_by.as_deref(), Some("7"));
        assert!(req.summary.is_none());
        assert!(req.source.is_none());
    }

    /// 更新 body 允许只传部分字段（前端按需发送）
    #[test]
    fn update_body_is_partial() {
        let req: UpdateAssetRequest =
            serde_json::from_str(r#"{"title":"新标题","status":"valid"}"#)
                .expect("部分字段应可反序列化");
        assert_eq!(req.title.as_deref(), Some("新标题"));
        assert_eq!(req.status.as_deref(), Some("valid"));
        assert!(req.content.is_none());
        assert!(req.okf_type.is_none());
        assert!(req.tags.is_none());
    }

    /// 绑定文件 body 与命令参数同名（fileSize 为数字，assetId 已被路径剥离）
    #[test]
    fn attach_body_matches_command_arguments() {
        let json = r#"{
            "assetId": "1",
            "fileUrl": "https://minio.local/k/a.pdf",
            "fileName": "a.pdf",
            "fileSize": 2048,
            "fileMime": "application/pdf",
            "fileMd5": "d41d8cd98f00b204e9800998ecf8427e"
        }"#;
        let req: AttachFileRequest = serde_json::from_str(json).expect("body 应可反序列化");
        assert_eq!(req.file_url, "https://minio.local/k/a.pdf");
        assert_eq!(req.file_name, "a.pdf");
        assert_eq!(req.file_size, 2048);
        assert_eq!(req.file_mime, "application/pdf");
        assert_eq!(req.file_md5, "d41d8cd98f00b204e9800998ecf8427e");
    }

    /// 7 条路由可同时注册：`/api/knowledge/asset` 静态段与既有 `/api/knowledge/{id}` 不冲突
    ///
    /// matchit 发现冲突会在构建 Router 时 panic，因此“能构建”即代表路由表合法。
    #[test]
    fn knowledge_asset_router_builds() {
        let router: Router = Router::new()
            .route("/api/knowledge/asset", get(list_assets).post(create_asset))
            .route(
                "/api/knowledge/asset/by-tree-node/{treeNodeId}",
                get(get_by_tree_node),
            )
            .route(
                "/api/knowledge/asset/{id}",
                get(get_asset).put(update_asset).delete(delete_asset),
            )
            .route("/api/knowledge/asset/{assetId}/attach", post(attach_file))
            // 既有路由：验证静态段（tree/list）与参数段（{id}）共存
            .route("/api/knowledge/tree", get(list_assets))
            .route("/api/knowledge/list", get(list_assets))
            .route("/api/knowledge/{id}", get(get_asset));

        let _ = router;
    }
}

