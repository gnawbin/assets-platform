//! 知识树 & 知识库 Service
//!
//! 提供知识树节点和知识条目的 CRUD 操作。
//! 知识树独立于资产管理，asset_id 为可选关联。

use crate::database;
use crate::database::models::{AssetKnowledge, KnowledgeTree, KnowledgeTreeNode};
use crate::utils::snowflake::next_id;
use tracing::{error, info};

// ======================== 知识树节点 ========================

/// 获取完整知识树（返回树形结构）
pub async fn get_knowledge_tree() -> Result<Vec<KnowledgeTreeNode>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    info!("获取知识树被调用");

    let sql = format!(
        "SELECT id, knowledge_id, parent_id, node_type, title, icon, sort_order, is_expanded, created_by, created_at, updated_by, updated_at, deleted FROM {}knowledge_tree WHERE (deleted IS NULL OR deleted = 0) ORDER BY sort_order ASC, id ASC",
        prefix
    );
    let all_nodes = sqlx::query_as::<_, KnowledgeTree>(&sql)
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询知识树失败: {}", e);
            format!("查询知识树失败: {}", e)
        })?;

    info!("查询到 {} 条知识树节点", all_nodes.len());

    // 构建树形结构
    let root_nodes: Vec<&KnowledgeTree> =
        all_nodes.iter().filter(|n| n.parent_id.is_none()).collect();

    let result = root_nodes
        .iter()
        .map(|root| build_tree_node(root, &all_nodes))
        .collect::<Vec<_>>();

    info!("知识树构建完成，返回 {} 个根节点", result.len());
    Ok(result)
}

fn build_tree_node(node: &KnowledgeTree, all_nodes: &[KnowledgeTree]) -> KnowledgeTreeNode {
    let children: Vec<KnowledgeTreeNode> = all_nodes
        .iter()
        .filter(|n| n.parent_id == Some(node.id))
        .map(|child| build_tree_node(child, all_nodes))
        .collect();

    KnowledgeTreeNode {
        id: node.id,
        knowledge_id: node.knowledge_id,
        parent_id: node.parent_id,
        node_type: node.node_type.clone(),
        title: node.title.clone(),
        icon: node.icon.clone(),
        sort_order: node.sort_order,
        is_expanded: node.is_expanded,
        children,
    }
}

/// 新增知识树节点
pub async fn insert_knowledge_node(
    knowledge_id: Option<i64>,
    parent_id: Option<i64>,
    node_type: &str,
    title: &str,
    icon: Option<&str>,
    sort_order: i32,
    created_by: Option<i64>,
) -> Result<KnowledgeTree, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    info!("新增知识树节点: title={}, type={}", title, node_type);

    let sql = format!(
        "INSERT INTO {}knowledge_tree (id, knowledge_id, parent_id, node_type, title, icon, sort_order, is_expanded, created_by, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW()) RETURNING id, knowledge_id, parent_id, node_type, title, icon, sort_order, is_expanded, created_by, created_at, updated_by, updated_at, deleted",
        prefix
    );
    let inserted = sqlx::query_as::<_, KnowledgeTree>(&sql)
        .bind(next_id() as i64)
        .bind(knowledge_id)
        .bind(parent_id)
        .bind(node_type)
        .bind(title)
        .bind(icon)
        .bind(sort_order)
        .bind(true) // is_expanded 默认展开
        .bind(created_by)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("新增知识树节点失败: title={}, error={}", title, e);
            format!("新增知识树节点失败: {}", e)
        })?;

    info!(
        "新增知识树节点成功: id={}, title={}",
        inserted.id, inserted.title
    );
    Ok(inserted)
}

/// 更新知识树节点
pub async fn update_knowledge_node(
    id: i64,
    title: Option<&str>,
    icon: Option<&str>,
    sort_order: Option<i32>,
    is_expanded: Option<bool>,
    updated_by: Option<i64>,
) -> Result<KnowledgeTree, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    info!("更新知识树节点: id={}", id);

    // 先查询现有节点
    let query_sql = format!(
        "SELECT id, knowledge_id, parent_id, node_type, title, icon, sort_order, is_expanded, created_by, created_at, updated_by, updated_at, deleted FROM {}knowledge_tree WHERE id = $1 AND (deleted IS NULL OR deleted = 0)",
        prefix
    );
    let existing = sqlx::query_as::<_, KnowledgeTree>(&query_sql)
        .bind(id)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("查询知识树节点失败: id={}, error={}", id, e);
            format!("查询知识树节点失败: {}", e)
        })?;

    let new_title = title.unwrap_or(&existing.title);
    let new_icon = icon.or(existing.icon.as_deref());
    let new_sort_order = sort_order.unwrap_or(existing.sort_order);
    let new_is_expanded = is_expanded.unwrap_or(existing.is_expanded);

    let update_sql = format!(
        "UPDATE {}knowledge_tree SET title = $1, icon = $2, sort_order = $3, is_expanded = $4, updated_by = $5, updated_at = NOW() WHERE id = $6 RETURNING id, knowledge_id, parent_id, node_type, title, icon, sort_order, is_expanded, created_by, created_at, updated_by, updated_at, deleted",
        prefix
    );
    let updated = sqlx::query_as::<_, KnowledgeTree>(&update_sql)
        .bind(new_title)
        .bind(new_icon)
        .bind(new_sort_order)
        .bind(new_is_expanded)
        .bind(updated_by)
        .bind(id)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("更新知识树节点失败: id={}, error={}", id, e);
            format!("更新知识树节点失败: {}", e)
        })?;

    info!("更新知识树节点成功: id={}", updated.id);
    Ok(updated)
}

/// 删除知识树节点（软删除）
pub async fn delete_knowledge_node(id: i64) -> Result<(), String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    info!("删除知识树节点: id={}", id);

    // 软删除节点及其所有子节点
    let sql = format!(
        "UPDATE {}knowledge_tree SET deleted = 1 WHERE id = $1 OR parent_id = $1",
        prefix
    );
    sqlx::query(&sql)
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("删除知识树节点失败: id={}, error={}", id, e);
            format!("删除知识树节点失败: {}", e)
        })?;

    info!("删除知识树节点成功: id={}", id);
    Ok(())
}

/// 移动知识树节点（修改 parent_id）
pub async fn move_knowledge_node(
    id: i64,
    new_parent_id: Option<i64>,
) -> Result<KnowledgeTree, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    info!(
        "移动知识树节点: id={}, new_parent_id={:?}",
        id, new_parent_id
    );

    let sql = format!(
        "UPDATE {}knowledge_tree SET parent_id = $1, updated_at = NOW() WHERE id = $2 AND (deleted IS NULL OR deleted = 0) RETURNING id, knowledge_id, parent_id, node_type, title, icon, sort_order, is_expanded, created_by, created_at, updated_by, updated_at, deleted",
        prefix
    );
    let updated = sqlx::query_as::<_, KnowledgeTree>(&sql)
        .bind(new_parent_id)
        .bind(id)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("移动知识树节点失败: id={}, error={}", id, e);
            format!("移动知识树节点失败: {}", e)
        })?;

    info!("移动知识树节点成功: id={}", updated.id);
    Ok(updated)
}

// ======================== 知识条目 ========================

/// 获取知识条目列表（支持按知识树节点筛选）
pub async fn get_knowledge_list(
    knowledge_id: Option<i64>,
    keyword: Option<String>,
) -> Result<Vec<AssetKnowledge>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    info!("获取知识条目列表被调用");

    let mut sql = format!(
        "SELECT id, asset_id, doc_source, knowledge_type, title, content, chunk_index, vector_data, permission_level, owner_type, owner_id, created_by, created_at, updated_by, updated_at, deleted FROM {}asset_knowledge WHERE (deleted IS NULL OR deleted = 0)",
        prefix
    );

    if let Some(kid) = knowledge_id {
        sql.push_str(&format!(" AND id = {}", kid));
    }

    if let Some(ref kw) = keyword {
        if !kw.is_empty() {
            sql.push_str(&format!(
                " AND (title ILIKE '%{}%' OR content ILIKE '%{}%')",
                kw.replace('\'', "''"),
                kw.replace('\'', "''")
            ));
        }
    }

    sql.push_str(" ORDER BY created_at DESC");

    let list = sqlx::query_as::<_, AssetKnowledge>(&sql)
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询知识条目列表失败: {}", e);
            format!("查询知识条目列表失败: {}", e)
        })?;

    info!("查询到 {} 条知识条目", list.len());
    Ok(list)
}

/// 获取单条知识条目
pub async fn get_knowledge_by_id(id: i64) -> Result<AssetKnowledge, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    let sql = format!(
        "SELECT id, asset_id, doc_source, knowledge_type, title, content, chunk_index, vector_data, permission_level, owner_type, owner_id, created_by, created_at, updated_by, updated_at, deleted FROM {}asset_knowledge WHERE id = $1 AND (deleted IS NULL OR deleted = 0)",
        prefix
    );
    let item = sqlx::query_as::<_, AssetKnowledge>(&sql)
        .bind(id)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("查询知识条目失败: id={}, error={}", id, e);
            format!("查询知识条目失败: {}", e)
        })?;

    Ok(item)
}

/// 新增知识条目
pub async fn insert_knowledge(
    asset_id: Option<i64>,
    doc_source: &str,
    knowledge_type: &str,
    title: &str,
    content: &str,
    permission_level: &str,
    created_by: Option<i64>,
) -> Result<AssetKnowledge, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    info!("新增知识条目: title={}", title);

    let sql = format!(
        "INSERT INTO {}asset_knowledge (id, asset_id, doc_source, knowledge_type, title, content, chunk_index, permission_level, created_by, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW()) RETURNING id, asset_id, doc_source, knowledge_type, title, content, chunk_index, vector_data, permission_level, owner_type, owner_id, created_by, created_at, updated_by, updated_at, deleted",
        prefix
    );
    let inserted = sqlx::query_as::<_, AssetKnowledge>(&sql)
        .bind(next_id() as i64)
        .bind(asset_id)
        .bind(doc_source)
        .bind(knowledge_type)
        .bind(title)
        .bind(content)
        .bind(0) // chunk_index
        .bind(permission_level)
        .bind(created_by)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("新增知识条目失败: title={}, error={}", title, e);
            format!("新增知识条目失败: {}", e)
        })?;

    info!(
        "新增知识条目成功: id={}, title={}",
        inserted.id, inserted.title
    );
    Ok(inserted)
}

/// 更新知识条目
pub async fn update_knowledge(
    id: i64,
    title: Option<&str>,
    content: Option<&str>,
    knowledge_type: Option<&str>,
    permission_level: Option<&str>,
    updated_by: Option<i64>,
) -> Result<AssetKnowledge, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    info!("更新知识条目: id={}", id);

    // 先查询现有条目
    let query_sql = format!(
        "SELECT id, asset_id, doc_source, knowledge_type, title, content, chunk_index, vector_data, permission_level, owner_type, owner_id, created_by, created_at, updated_by, updated_at, deleted FROM {}asset_knowledge WHERE id = $1 AND (deleted IS NULL OR deleted = 0)",
        prefix
    );
    let existing = sqlx::query_as::<_, AssetKnowledge>(&query_sql)
        .bind(id)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("查询知识条目失败: id={}, error={}", id, e);
            format!("查询知识条目失败: {}", e)
        })?;

    let new_title = title.unwrap_or(&existing.title);
    let new_content = content.unwrap_or(&existing.content);
    let new_knowledge_type = knowledge_type.unwrap_or(&existing.knowledge_type);
    let new_permission_level = permission_level.unwrap_or(&existing.permission_level);

    let update_sql = format!(
        "UPDATE {}asset_knowledge SET title = $1, content = $2, knowledge_type = $3, permission_level = $4, updated_by = $5, updated_at = NOW() WHERE id = $6 RETURNING id, asset_id, doc_source, knowledge_type, title, content, chunk_index, vector_data, permission_level, owner_type, owner_id, created_by, created_at, updated_by, updated_at, deleted",
        prefix
    );
    let updated = sqlx::query_as::<_, AssetKnowledge>(&update_sql)
        .bind(new_title)
        .bind(new_content)
        .bind(new_knowledge_type)
        .bind(new_permission_level)
        .bind(updated_by)
        .bind(id)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("更新知识条目失败: id={}, error={}", id, e);
            format!("更新知识条目失败: {}", e)
        })?;

    info!("更新知识条目成功: id={}", updated.id);
    Ok(updated)
}

/// 删除知识条目（软删除）
pub async fn delete_knowledge(id: i64) -> Result<(), String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    info!("删除知识条目: id={}", id);

    let sql = format!(
        "UPDATE {}asset_knowledge SET deleted = 1 WHERE id = $1",
        prefix
    );
    sqlx::query(&sql)
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("删除知识条目失败: id={}, error={}", id, e);
            format!("删除知识条目失败: {}", e)
        })?;

    info!("删除知识条目成功: id={}", id);
    Ok(())
}
