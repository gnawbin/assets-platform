//! 知识树 & 知识库 Command
//!
//! 对应 lib.rs 中的知识树相关命令

use assets_database::models::{AssetKnowledge, KnowledgeTreeNode};
use assets_service;
use tracing::info;

// ======================== 知识树节点 ========================

/// 获取完整知识树
#[tauri::command]
pub async fn get_knowledge_tree() -> Result<Vec<KnowledgeTreeNode>, String> {
    info!("[DEBUG] get_knowledge_tree called");
    assets_service::knowledge_service::get_knowledge_tree().await
}

/// 新增知识树节点
#[tauri::command]
pub async fn insert_knowledge_node(
    knowledge_id: Option<String>,
    parent_id: Option<String>,
    node_type: String,
    title: String,
    icon: Option<String>,
    sort_order: Option<i32>,
    currentUserId: Option<String>,
) -> Result<assets_database::models::KnowledgeTree, String> {
    info!("[DEBUG] insert_knowledge_node called: title={}", title);

    let kid = match knowledge_id {
        Some(ref id) if !id.is_empty() => Some(
            id.parse::<i64>()
                .map_err(|e| format!("无效的知识条目ID: {}", e))?,
        ),
        _ => None,
    };
    let pid = match parent_id {
        Some(ref id) if !id.is_empty() => Some(
            id.parse::<i64>()
                .map_err(|e| format!("无效的父节点ID: {}", e))?,
        ),
        _ => None,
    };
    let user_id = currentUserId.and_then(|id| id.parse().ok()).unwrap_or(1i64);

    assets_service::knowledge_service::insert_knowledge_node(
        kid,
        pid,
        &node_type,
        &title,
        icon.as_deref(),
        sort_order.unwrap_or(0),
        Some(user_id),
    )
    .await
}

/// 更新知识树节点
#[tauri::command]
pub async fn update_knowledge_node(
    id: String,
    title: Option<String>,
    icon: Option<String>,
    sort_order: Option<i32>,
    is_expanded: Option<bool>,
    currentUserId: Option<String>,
) -> Result<assets_database::models::KnowledgeTree, String> {
    info!("[DEBUG] update_knowledge_node called: id={}", id);

    let node_id: i64 = id.parse().map_err(|e| format!("无效的节点ID: {}", e))?;
    let user_id = currentUserId
        .and_then(|uid| uid.parse().ok())
        .unwrap_or(1i64);

    assets_service::knowledge_service::update_knowledge_node(
        node_id,
        title.as_deref(),
        icon.as_deref(),
        sort_order,
        is_expanded,
        Some(user_id),
    )
    .await
}

/// 删除知识树节点
#[tauri::command]
pub async fn delete_knowledge_node(id: String) -> Result<(), String> {
    info!("[DEBUG] delete_knowledge_node called: id={}", id);

    let node_id: i64 = id.parse().map_err(|e| format!("无效的节点ID: {}", e))?;
    assets_service::knowledge_service::delete_knowledge_node(node_id).await
}

/// 移动知识树节点
#[tauri::command]
pub async fn move_knowledge_node(
    id: String,
    new_parent_id: Option<String>,
) -> Result<assets_database::models::KnowledgeTree, String> {
    info!("[DEBUG] move_knowledge_node called: id={}", id);

    let node_id: i64 = id.parse().map_err(|e| format!("无效的节点ID: {}", e))?;
    let npid = match new_parent_id {
        Some(ref pid) if !pid.is_empty() => Some(
            pid.parse::<i64>()
                .map_err(|e| format!("无效的父节点ID: {}", e))?,
        ),
        _ => None,
    };

    assets_service::knowledge_service::move_knowledge_node(node_id, npid).await
}

// ======================== 知识条目 ========================

/// 获取知识条目列表
#[tauri::command]
pub async fn get_knowledge_list(
    knowledge_id: Option<String>,
    keyword: Option<String>,
) -> Result<Vec<AssetKnowledge>, String> {
    info!("[DEBUG] get_knowledge_list called");

    let kid = match knowledge_id {
        Some(ref id) if !id.is_empty() => Some(
            id.parse::<i64>()
                .map_err(|e| format!("无效的知识条目ID: {}", e))?,
        ),
        _ => None,
    };

    assets_service::knowledge_service::get_knowledge_list(kid, keyword).await
}

/// 获取单条知识条目
#[tauri::command]
pub async fn get_knowledge_by_id(id: String) -> Result<AssetKnowledge, String> {
    info!("[DEBUG] get_knowledge_by_id called: id={}", id);

    let kid: i64 = id.parse().map_err(|e| format!("无效的知识条目ID: {}", e))?;
    assets_service::knowledge_service::get_knowledge_by_id(kid).await
}

/// 新增知识条目
#[tauri::command]
pub async fn insert_knowledge(
    asset_id: Option<String>,
    doc_source: Option<String>,
    knowledge_type: Option<String>,
    title: String,
    content: String,
    permission_level: Option<String>,
    currentUserId: Option<String>,
) -> Result<AssetKnowledge, String> {
    info!("[DEBUG] insert_knowledge called: title={}", title);

    let aid = match asset_id {
        Some(ref id) if !id.is_empty() => Some(
            id.parse::<i64>()
                .map_err(|e| format!("无效的资产ID: {}", e))?,
        ),
        _ => None,
    };
    let user_id = currentUserId.and_then(|id| id.parse().ok()).unwrap_or(1i64);

    assets_service::knowledge_service::insert_knowledge(
        aid,
        doc_source.as_deref().unwrap_or("manual"),
        knowledge_type.as_deref().unwrap_or("basic"),
        &title,
        &content,
        permission_level.as_deref().unwrap_or("internal"),
        Some(user_id),
    )
    .await
}

/// 更新知识条目
#[tauri::command]
pub async fn update_knowledge(
    id: String,
    title: Option<String>,
    content: Option<String>,
    knowledge_type: Option<String>,
    permission_level: Option<String>,
    currentUserId: Option<String>,
) -> Result<AssetKnowledge, String> {
    info!("[DEBUG] update_knowledge called: id={}", id);

    let kid: i64 = id.parse().map_err(|e| format!("无效的知识条目ID: {}", e))?;
    let user_id = currentUserId
        .and_then(|uid| uid.parse().ok())
        .unwrap_or(1i64);

    assets_service::knowledge_service::update_knowledge(
        kid,
        title.as_deref(),
        content.as_deref(),
        knowledge_type.as_deref(),
        permission_level.as_deref(),
        Some(user_id),
    )
    .await
}

/// 删除知识条目
#[tauri::command]
pub async fn delete_knowledge(id: String) -> Result<(), String> {
    info!("[DEBUG] delete_knowledge called: id={}", id);

    let kid: i64 = id.parse().map_err(|e| format!("无效的知识条目ID: {}", e))?;
    assets_service::knowledge_service::delete_knowledge(kid).await
}
