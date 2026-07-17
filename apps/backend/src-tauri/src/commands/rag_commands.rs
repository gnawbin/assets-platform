//! RAG 相关 Tauri Command

use crate::database::models::{ChunkResult, DocumentChunk};
use crate::service::rag_service::RAGRetriever;

/// 对知识资产执行分片 + 向量化
#[tauri::command]
pub async fn chunk_and_vectorize(
    assetId: String,
    title: String,
    content: String,
    okfType: String,
    tags: Vec<String>,
    treeNodeId: Option<String>,
) -> Result<Vec<DocumentChunk>, String> {
    let asset_id: i64 = assetId
        .parse()
        .map_err(|e| format!("无效的资产ID: {}", e))?;
    let tree_node_id = match treeNodeId {
        Some(ref id) if !id.is_empty() => Some(
            id.parse::<i64>()
                .map_err(|e| format!("无效的节点ID: {}", e))?,
        ),
        _ => None,
    };

    RAGRetriever::chunk_and_vectorize(asset_id, &content, &title, &okfType, &tags, tree_node_id)
        .await
}

/// 测试 RAG 检索
#[tauri::command]
pub async fn test_rag_retrieval(
    question: String,
    bindTreeNodeId: Option<String>,
    topK: Option<i32>,
) -> Result<Vec<ChunkResult>, String> {
    let bind_id = match bindTreeNodeId {
        Some(ref id) if !id.is_empty() => Some(
            id.parse::<i64>()
                .map_err(|e| format!("无效的节点ID: {}", e))?,
        ),
        _ => None,
    };

    let params = crate::database::models::RetrieveParams {
        question,
        bind_tree_node_id: bind_id,
        top_k: topK.unwrap_or(5),
        max_tokens: 2000,
        okf_type_filter: None,
        min_similarity: 0.0,
    };

    RAGRetriever::retrieve(&params).await
}
