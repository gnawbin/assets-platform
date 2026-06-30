//! OKF 知识资产 Tauri Command
//!
//! 全新 knowledge_asset 表命令，与现有 knowledge_commands.rs 完全独立。

use crate::database::models::KnowledgeAsset;
use crate::service;

/// 根据 tree_node_id 获取关联的知识资产
#[tauri::command]
pub async fn get_knowledge_asset_by_tree_node(
    tree_node_id: String,
) -> Result<KnowledgeAsset, String> {
    let id: i64 = tree_node_id
        .parse()
        .map_err(|e| format!("无效的节点ID: {}", e))?;
    service::knowledge_asset_service::get_knowledge_asset_by_tree_node(id).await
}

/// 获取单条知识资产
#[tauri::command]
pub async fn get_knowledge_asset(id: String) -> Result<KnowledgeAsset, String> {
    let item_id: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    service::knowledge_asset_service::get_knowledge_asset(item_id).await
}

/// 获取知识资产列表
#[tauri::command]
pub async fn list_knowledge_assets(
    okf_type: Option<String>,
    _tags: Option<Vec<String>>,
) -> Result<Vec<KnowledgeAsset>, String> {
    service::knowledge_asset_service::list_knowledge_assets(okf_type.as_deref(), None).await
}

/// 创建知识资产（同时创建 knowledge_tree 节点）
#[tauri::command]
pub async fn create_knowledge_asset(
    treeNodeId: String,
    title: String,
    okfType: String,
    content: Option<String>,
    summary: Option<String>,
    source: Option<String>,
    tags: Option<Vec<String>>,
    createdBy: Option<String>,
) -> Result<KnowledgeAsset, String> {
    let tree_node_id: i64 = treeNodeId
        .parse()
        .map_err(|e| format!("无效的节点ID: {}", e))?;
    let cb = match createdBy {
        Some(ref id) if !id.is_empty() => Some(
            id.parse::<i64>()
                .map_err(|e| format!("无效的创建人ID: {}", e))?,
        ),
        _ => None,
    };

    let asset = KnowledgeAsset {
        id: 0, // GENERATED ALWAYS AS IDENTITY
        tree_node_id,
        title,
        content,
        content_html: None,
        okf_type: okfType,
        summary,
        source,
        confidence: Some(1.0),
        status: "draft".to_string(),
        effective_at: None,
        expire_at: None,
        relation_ids: None,
        tags,
        file_url: None,
        file_name: None,
        file_size: None,
        file_mime: None,
        file_md5: None,
        editor_mode: "wysiwyg".to_string(),
        created_by: cb,
        created_at: None,
        updated_by: None,
        updated_at: None,
        deleted: 0,
    };

    service::knowledge_asset_service::create_knowledge_asset(&asset).await
}

/// 更新知识资产
#[tauri::command]
pub async fn update_knowledge_asset(
    id: String,
    title: Option<String>,
    content: Option<String>,
    okfType: Option<String>,
    summary: Option<String>,
    status: Option<String>,
    tags: Option<Vec<String>>,
    updatedBy: Option<String>,
) -> Result<KnowledgeAsset, String> {
    let asset_id: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    let ub = match updatedBy {
        Some(ref uid) if !uid.is_empty() => Some(
            uid.parse::<i64>()
                .map_err(|e| format!("无效的更新人ID: {}", e))?,
        ),
        _ => None,
    };

    service::knowledge_asset_service::update_knowledge_asset(
        asset_id,
        title.as_deref(),
        content.as_deref(),
        okfType.as_deref(),
        summary.as_deref(),
        status.as_deref(),
        tags,
        ub,
    )
    .await
}

/// 删除知识资产（软删除）
#[tauri::command]
pub async fn delete_knowledge_asset(id: String) -> Result<(), String> {
    let asset_id: i64 = id.parse().map_err(|e| format!("无效的ID: {}", e))?;
    service::knowledge_asset_service::delete_knowledge_asset(asset_id).await
}

/// 将文件绑定到知识资产
#[tauri::command]
pub async fn attach_file_to_knowledge(
    assetId: String,
    fileUrl: String,
    fileName: String,
    fileSize: i64,
    fileMime: String,
    fileMd5: String,
) -> Result<KnowledgeAsset, String> {
    let asset_id: i64 = assetId
        .parse()
        .map_err(|e| format!("无效的资产ID: {}", e))?;
    service::knowledge_asset_service::attach_file_to_asset(
        asset_id, &fileUrl, &fileName, fileSize, &fileMime, &fileMd5,
    )
    .await
}
