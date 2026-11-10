//! 知识树 & 知识库 HTTP API 路由
//!
//! 提供 RESTful 风格的 HTTP 接口

use axum::{extract::Path, extract::Query, Json};
use serde::Deserialize;
use tracing::info;

use database::models::{AssetKnowledge, KnowledgeTree, KnowledgeTreeNode};
use service;

use super::response::{ApiError, ApiResponse};

// ======================== 请求 DTO ========================

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct InsertNodeRequest {
    /// 关联知识条目 ID（folder 可为空）
    pub knowledge_id: Option<String>,
    /// 父节点 ID（顶级节点为空）
    pub parent_id: Option<String>,
    /// 节点类型：folder / document / link
    pub node_type: String,
    /// 节点显示名称
    pub title: String,
    pub icon: Option<String>,
    pub sort_order: Option<i32>,
    pub created_by: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateNodeRequest {
    pub title: Option<String>,
    pub icon: Option<String>,
    pub sort_order: Option<i32>,
    pub is_expanded: Option<bool>,
    pub updated_by: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct MoveNodeRequest {
    /// 新的父节点 ID（null 表示移到顶级）
    pub new_parent_id: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct InsertKnowledgeRequest {
    /// 关联资产 ID（可选）
    pub asset_id: Option<String>,
    /// 来源：manual / asset / hardware / intangible / document
    pub doc_source: Option<String>,
    /// 知识类型：basic 等
    pub knowledge_type: Option<String>,
    /// 标题
    pub title: String,
    /// 正文内容
    pub content: String,
    /// 权限级别：internal 等
    pub permission_level: Option<String>,
    pub created_by: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateKnowledgeRequest {
    pub title: Option<String>,
    pub content: Option<String>,
    pub knowledge_type: Option<String>,
    pub permission_level: Option<String>,
    pub updated_by: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct KnowledgeListQuery {
    /// 知识树节点 ID（过滤该节点下的条目）
    pub knowledge_id: Option<String>,
    /// 标题 / 内容关键词
    pub keyword: Option<String>,
}

// ======================== 知识树节点路由 ========================

/// GET /api/knowledge/tree - 获取完整知识树
#[utoipa::path(
    get,
    path = "/api/knowledge/tree",
    tag = "知识库",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "知识树（含 children 递归）", body = ApiResponse<Vec<database::models::KnowledgeTreeNode>>),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn get_tree() -> Result<Json<ApiResponse<Vec<KnowledgeTreeNode>>>, ApiError> {
    info!("[HTTP] GET /api/knowledge/tree");
    match service::knowledge_service::get_knowledge_tree().await {
        Ok(tree) => Ok(Json(ApiResponse::success(tree))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// POST /api/knowledge/node - 新增知识树节点
#[utoipa::path(
    post,
    path = "/api/knowledge/node",
    tag = "知识库",
    security(("bearer_auth" = [])),
    request_body = InsertNodeRequest,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<database::models::KnowledgeTree>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn insert_node(
    Json(req): Json<InsertNodeRequest>,
) -> Result<Json<ApiResponse<KnowledgeTree>>, ApiError> {
    info!("[HTTP] POST /api/knowledge/node: title={}", req.title);

    let kid = parse_opt_i64(req.knowledge_id).map_err(|e| ApiError::bad_request(&e))?;
    let pid = parse_opt_i64(req.parent_id).map_err(|e| ApiError::bad_request(&e))?;
    let cb = parse_opt_i64(req.created_by).map_err(|e| ApiError::bad_request(&e))?;

    match service::knowledge_service::insert_knowledge_node(
        kid,
        pid,
        &req.node_type,
        &req.title,
        req.icon.as_deref(),
        req.sort_order.unwrap_or(0),
        Some(cb.unwrap_or(1)),
    )
    .await
    {
        Ok(node) => Ok(Json(ApiResponse::success(node))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// PUT /api/knowledge/node/{id} - 更新知识树节点
#[utoipa::path(
    put,
    path = "/api/knowledge/node/{id}",
    tag = "知识库",
    security(("bearer_auth" = [])),
    request_body = UpdateNodeRequest,
    responses(
        (status = 200, description = "更新成功", body = ApiResponse<database::models::KnowledgeTree>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn update_node(
    Path(id): Path<String>,
    Json(req): Json<UpdateNodeRequest>,
) -> Result<Json<ApiResponse<KnowledgeTree>>, ApiError> {
    info!("[HTTP] PUT /api/knowledge/node/{}", id);

    let node_id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的节点ID"))?;
    let ub = parse_opt_i64(req.updated_by).map_err(|e| ApiError::bad_request(&e))?;

    match service::knowledge_service::update_knowledge_node(
        node_id,
        req.title.as_deref(),
        req.icon.as_deref(),
        req.sort_order,
        req.is_expanded,
        ub,
    )
    .await
    {
        Ok(node) => Ok(Json(ApiResponse::success(node))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// DELETE /api/knowledge/node/{id} - 删除知识树节点
#[utoipa::path(
    delete,
    path = "/api/knowledge/node/{id}",
    tag = "知识库",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "删除成功（`data` 为 null）"),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn delete_node(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    info!("[HTTP] DELETE /api/knowledge/node/{}", id);

    let node_id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的节点ID"))?;

    match service::knowledge_service::delete_knowledge_node(node_id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// PUT /api/knowledge/node/{id}/move - 移动知识树节点
#[utoipa::path(
    put,
    path = "/api/knowledge/node/{id}/move",
    tag = "知识库",
    security(("bearer_auth" = [])),
    request_body = MoveNodeRequest,
    responses(
        (status = 200, description = "移动成功", body = ApiResponse<database::models::KnowledgeTree>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn move_node(
    Path(id): Path<String>,
    Json(req): Json<MoveNodeRequest>,
) -> Result<Json<ApiResponse<KnowledgeTree>>, ApiError> {
    info!("[HTTP] PUT /api/knowledge/node/{}/move", id);

    let node_id: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的节点ID"))?;
    let npid = parse_opt_i64(req.new_parent_id).map_err(|e| ApiError::bad_request(&e))?;

    match service::knowledge_service::move_knowledge_node(node_id, npid).await {
        Ok(node) => Ok(Json(ApiResponse::success(node))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

// ======================== 知识条目路由 ========================

/// GET /api/knowledge/list - 获取知识条目列表
#[utoipa::path(
    get,
    path = "/api/knowledge/list",
    tag = "知识库",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "知识条目列表", body = ApiResponse<Vec<database::models::AssetKnowledge>>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn get_list(
    Query(query): Query<KnowledgeListQuery>,
) -> Result<Json<ApiResponse<Vec<AssetKnowledge>>>, ApiError> {
    info!("[HTTP] GET /api/knowledge/list");

    let kid = parse_opt_i64(query.knowledge_id).map_err(|e| ApiError::bad_request(&e))?;

    match service::knowledge_service::get_knowledge_list(kid, query.keyword).await {
        Ok(list) => Ok(Json(ApiResponse::success(list))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// GET /api/knowledge/{id} - 获取单条知识条目
#[utoipa::path(
    get,
    path = "/api/knowledge/{id}",
    tag = "知识库",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "知识条目", body = ApiResponse<database::models::AssetKnowledge>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn get_by_id(
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<AssetKnowledge>>, ApiError> {
    info!("[HTTP] GET /api/knowledge/{}", id);

    let kid: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的知识条目ID"))?;

    match service::knowledge_service::get_knowledge_by_id(kid).await {
        Ok(item) => Ok(Json(ApiResponse::success(item))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// POST /api/knowledge - 新增知识条目
#[utoipa::path(
    post,
    path = "/api/knowledge",
    tag = "知识库",
    security(("bearer_auth" = [])),
    request_body = InsertKnowledgeRequest,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<database::models::AssetKnowledge>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn insert_knowledge(
    Json(req): Json<InsertKnowledgeRequest>,
) -> Result<Json<ApiResponse<AssetKnowledge>>, ApiError> {
    info!("[HTTP] POST /api/knowledge: title={}", req.title);

    let aid = parse_opt_i64(req.asset_id).map_err(|e| ApiError::bad_request(&e))?;
    let cb = parse_opt_i64(req.created_by).map_err(|e| ApiError::bad_request(&e))?;

    match service::knowledge_service::insert_knowledge(
        aid,
        req.doc_source.as_deref().unwrap_or("manual"),
        req.knowledge_type.as_deref().unwrap_or("basic"),
        &req.title,
        &req.content,
        req.permission_level.as_deref().unwrap_or("internal"),
        Some(cb.unwrap_or(1)),
    )
    .await
    {
        Ok(item) => Ok(Json(ApiResponse::success(item))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// PUT /api/knowledge/{id} - 更新知识条目
#[utoipa::path(
    put,
    path = "/api/knowledge/{id}",
    tag = "知识库",
    security(("bearer_auth" = [])),
    request_body = UpdateKnowledgeRequest,
    responses(
        (status = 200, description = "更新成功", body = ApiResponse<database::models::AssetKnowledge>),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn update_knowledge(
    Path(id): Path<String>,
    Json(req): Json<UpdateKnowledgeRequest>,
) -> Result<Json<ApiResponse<AssetKnowledge>>, ApiError> {
    info!("[HTTP] PUT /api/knowledge/{}", id);

    let kid: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的知识条目ID"))?;
    let ub = parse_opt_i64(req.updated_by).map_err(|e| ApiError::bad_request(&e))?;

    match service::knowledge_service::update_knowledge(
        kid,
        req.title.as_deref(),
        req.content.as_deref(),
        req.knowledge_type.as_deref(),
        req.permission_level.as_deref(),
        ub,
    )
    .await
    {
        Ok(item) => Ok(Json(ApiResponse::success(item))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

/// DELETE /api/knowledge/{id} - 删除知识条目
#[utoipa::path(
    delete,
    path = "/api/knowledge/{id}",
    tag = "知识库",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "删除成功（`data` 为 null）"),
        (status = 400, description = "参数错误", body = ApiError),
        (status = 401, description = "未认证", body = ApiError),
        (status = 500, description = "服务端错误", body = ApiError),
    )
)]
pub async fn delete_knowledge(Path(id): Path<String>) -> Result<Json<ApiResponse<()>>, ApiError> {
    info!("[HTTP] DELETE /api/knowledge/{}", id);

    let kid: i64 = id
        .parse()
        .map_err(|_| ApiError::bad_request("无效的知识条目ID"))?;

    match service::knowledge_service::delete_knowledge(kid).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => Err(ApiError::internal_error(e)),
    }
}

// ======================== 辅助函数 ========================

fn parse_opt_i64(val: Option<String>) -> Result<Option<i64>, String> {
    match val {
        Some(ref v) if !v.is_empty() => v
            .parse::<i64>()
            .map(Some)
            .map_err(|_| format!("无效的ID: {}", v)),
        _ => Ok(None),
    }
}
