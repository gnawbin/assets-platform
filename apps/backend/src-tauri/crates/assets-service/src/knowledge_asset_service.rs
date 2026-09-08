//! OKF 知识资产 Service
//!
//! 操作全新 knowledge_asset 表，与旧 knowledge_service.rs 完全独立。
//! 不与 asset_knowledge、knowledge_tree 等旧表产生冲突。

use assets_database;
use assets_database::models::KnowledgeAsset;
use tracing::{error, info};

/// 根据 tree_node_id 获取关联的知识资产（未关联时返回 None）
pub async fn get_knowledge_asset_by_tree_node(
    tree_node_id: i64,
) -> Result<Option<KnowledgeAsset>, String> {
    let pool = assets_database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();

    let sql = format!(
        "SELECT id, tree_node_id, title, content, content_html, okf_type, summary, source, confidence, status, effective_at, expire_at, relation_ids, tags, file_url, file_name, file_size, file_mime, file_md5, editor_mode, created_by, created_at, updated_by, updated_at, deleted FROM {}knowledge_asset WHERE tree_node_id = $1 AND (deleted IS NULL OR deleted = 0)",
        prefix
    );
    let item = sqlx::query_as::<_, KnowledgeAsset>(sqlx::AssertSqlSafe(sql))
        .bind(tree_node_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| {
            error!(
                "查询知识资产失败: tree_node_id={}, error={}",
                tree_node_id, e
            );
            format!("查询知识资产失败: {}", e)
        })?;

    Ok(item)
}

/// 获取单条知识资产
pub async fn get_knowledge_asset(id: i64) -> Result<KnowledgeAsset, String> {
    let pool = assets_database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();

    let sql = format!(
        "SELECT id, tree_node_id, title, content, content_html, okf_type, summary, source, confidence, status, effective_at, expire_at, relation_ids, tags, file_url, file_name, file_size, file_mime, file_md5, editor_mode, created_by, created_at, updated_by, updated_at, deleted FROM {}knowledge_asset WHERE id = $1 AND (deleted IS NULL OR deleted = 0)",
        prefix
    );
    let item = sqlx::query_as::<_, KnowledgeAsset>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("查询知识资产失败: id={}, error={}", id, e);
            format!("查询知识资产失败: {}", e)
        })?;

    Ok(item)
}

/// 获取知识资产列表（按 okf_type 和 tags 过滤）
pub async fn list_knowledge_assets(
    okf_type: Option<&str>,
    tags: Option<Vec<String>>,
) -> Result<Vec<KnowledgeAsset>, String> {
    let pool = assets_database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();

    let mut sql = format!(
        "SELECT id, tree_node_id, title, content, content_html, okf_type, summary, source, confidence, status, effective_at, expire_at, relation_ids, tags, file_url, file_name, file_size, file_mime, file_md5, editor_mode, created_by, created_at, updated_by, updated_at, deleted FROM {}knowledge_asset WHERE (deleted IS NULL OR deleted = 0)",
        prefix
    );

    if let Some(ot) = okf_type {
        sql.push_str(&format!(" AND okf_type = '{}'", ot.replace('\'', "''")));
    }

    sql.push_str(" ORDER BY created_at DESC");

    let list = sqlx::query_as::<_, KnowledgeAsset>(sqlx::AssertSqlSafe(sql))
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询知识资产列表失败: {}", e);
            format!("查询知识资产列表失败: {}", e)
        })?;

    Ok(list)
}

/// 创建知识资产
pub async fn create_knowledge_asset(asset: &KnowledgeAsset) -> Result<KnowledgeAsset, String> {
    let pool = assets_database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();

    info!("新增知识资产: title={}", asset.title);

    let sql = format!(
        "INSERT INTO {}knowledge_asset (tree_node_id, title, content, content_html, okf_type, summary, source, confidence, status, effective_at, expire_at, relation_ids, tags, file_url, file_name, file_size, file_mime, file_md5, editor_mode, created_by, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, NOW()) RETURNING id, tree_node_id, title, content, content_html, okf_type, summary, source, confidence, status, effective_at, expire_at, relation_ids, tags, file_url, file_name, file_size, file_mime, file_md5, editor_mode, created_by, created_at, updated_by, updated_at, deleted",
        prefix
    );
    let inserted = sqlx::query_as::<_, KnowledgeAsset>(sqlx::AssertSqlSafe(sql))
        .bind(asset.tree_node_id)
        .bind(&asset.title)
        .bind(&asset.content)
        .bind(&asset.content_html)
        .bind(&asset.okf_type)
        .bind(&asset.summary)
        .bind(&asset.source)
        .bind(asset.confidence)
        .bind(&asset.status)
        .bind(asset.effective_at)
        .bind(asset.expire_at)
        .bind(&asset.relation_ids)
        .bind(&asset.tags)
        .bind(&asset.file_url)
        .bind(&asset.file_name)
        .bind(asset.file_size)
        .bind(&asset.file_mime)
        .bind(&asset.file_md5)
        .bind(&asset.editor_mode)
        .bind(asset.created_by)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("新增知识资产失败: title={}, error={}", asset.title, e);
            format!("新增知识资产失败: {}", e)
        })?;

    info!("新增知识资产成功: id={}", inserted.id);
    Ok(inserted)
}

/// 更新知识资产
pub async fn update_knowledge_asset(
    id: i64,
    title: Option<&str>,
    content: Option<&str>,
    okf_type: Option<&str>,
    summary: Option<&str>,
    status: Option<&str>,
    tags: Option<Vec<String>>,
    updated_by: Option<i64>,
) -> Result<KnowledgeAsset, String> {
    let pool = assets_database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();

    info!("更新知识资产: id={}", id);

    // 先查询现有记录
    let query_sql = format!(
        "SELECT id, tree_node_id, title, content, content_html, okf_type, summary, source, confidence, status, effective_at, expire_at, relation_ids, tags, file_url, file_name, file_size, file_mime, file_md5, editor_mode, created_by, created_at, updated_by, updated_at, deleted FROM {}knowledge_asset WHERE id = $1 AND (deleted IS NULL OR deleted = 0)",
        prefix
    );
    let existing = sqlx::query_as::<_, KnowledgeAsset>(sqlx::AssertSqlSafe(query_sql))
        .bind(id)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("查询知识资产失败: id={}, error={}", id, e);
            format!("查询知识资产失败: {}", e)
        })?;

    let new_title = title.unwrap_or(&existing.title);
    let new_content = content.unwrap_or(existing.content.as_deref().unwrap_or(""));
    let new_okf_type = okf_type.unwrap_or(&existing.okf_type);
    let new_summary = summary.unwrap_or(existing.summary.as_deref().unwrap_or(""));
    let new_status = status.unwrap_or(&existing.status);
    let new_tags = tags.unwrap_or(existing.tags.unwrap_or_default());

    let update_sql = format!(
        "UPDATE {}knowledge_asset SET title = $1, content = $2, okf_type = $3, summary = $4, status = $5, tags = $6, updated_by = $7, updated_at = NOW() WHERE id = $8 RETURNING id, tree_node_id, title, content, content_html, okf_type, summary, source, confidence, status, effective_at, expire_at, relation_ids, tags, file_url, file_name, file_size, file_mime, file_md5, editor_mode, created_by, created_at, updated_by, updated_at, deleted",
        prefix
    );
    let updated = sqlx::query_as::<_, KnowledgeAsset>(sqlx::AssertSqlSafe(update_sql))
        .bind(new_title)
        .bind(new_content)
        .bind(new_okf_type)
        .bind(new_summary)
        .bind(new_status)
        .bind(&new_tags)
        .bind(updated_by)
        .bind(id)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("更新知识资产失败: id={}, error={}", id, e);
            format!("更新知识资产失败: {}", e)
        })?;

    info!("更新知识资产成功: id={}", updated.id);
    Ok(updated)
}

/// 删除知识资产（软删除）
pub async fn delete_knowledge_asset(id: i64) -> Result<(), String> {
    let pool = assets_database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();

    info!("删除知识资产: id={}", id);

    let sql = format!(
        "UPDATE {}knowledge_asset SET deleted = 1 WHERE id = $1",
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("删除知识资产失败: id={}, error={}", id, e);
            format!("删除知识资产失败: {}", e)
        })?;

    info!("删除知识资产成功: id={}", id);
    Ok(())
}

/// 将文件信息绑定到知识资产（上传完成后调用）
pub async fn attach_file_to_asset(
    asset_id: i64,
    file_url: &str,
    file_name: &str,
    file_size: i64,
    file_mime: &str,
    file_md5: &str,
) -> Result<KnowledgeAsset, String> {
    let pool = assets_database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = assets_database::schema_prefix();

    info!(
        "绑定文件到知识资产: asset_id={}, file={}",
        asset_id, file_name
    );

    let sql = format!(
        "UPDATE {}knowledge_asset SET file_url = $1, file_name = $2, file_size = $3, file_mime = $4, file_md5 = $5, updated_at = NOW() WHERE id = $6 AND (deleted IS NULL OR deleted = 0) RETURNING id, tree_node_id, title, content, content_html, okf_type, summary, source, confidence, status, effective_at, expire_at, relation_ids, tags, file_url, file_name, file_size, file_mime, file_md5, editor_mode, created_by, created_at, updated_by, updated_at, deleted",
        prefix
    );
    let updated = sqlx::query_as::<_, KnowledgeAsset>(sqlx::AssertSqlSafe(sql))
        .bind(file_url)
        .bind(file_name)
        .bind(file_size)
        .bind(file_mime)
        .bind(file_md5)
        .bind(asset_id)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("绑定文件到知识资产失败: asset_id={}, error={}", asset_id, e);
            format!("绑定文件到知识资产失败: {}", e)
        })?;

    info!("绑定文件到知识资产成功: asset_id={}", asset_id);
    Ok(updated)
}
