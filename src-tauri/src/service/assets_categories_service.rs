use crate::database;
use crate::database::models::AssetCategory;
use crate::utils::snowflake::next_id;
use tracing::{error, info};

/// 获取所有资产类别列表
pub async fn get_categories() -> Result<Vec<AssetCategory>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();
    let sql = format!(
        "SELECT id, category_name, asset_type, parent_id, sort, description, created_by, created_at, updated_by, updated_at,deleted FROM {}asset_category where deleted=0 ORDER BY sort ASC",
        prefix
    );
    let categories = sqlx::query_as::<_, AssetCategory>(&sql)
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询资产类别列表失败: {}", e);
            format!("查询资产类别失败: {}", e)
        })?;

    let count = categories.len();
    info!("查询资产类别列表成功: 共 {} 条记录", count);
    Ok(categories)
}

//获取资产类别最高级别的列表
pub async fn get_super_categories() -> Result<Vec<AssetCategory>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();
    let sql = format!(
        "SELECT id, category_name, asset_type, parent_id, sort, description, created_by, created_at, updated_by, updated_at,deleted FROM {}asset_category WHERE parent_id IS NULL AND deleted=0 ORDER BY sort ASC",
        prefix
    );
    let categories = sqlx::query_as::<_, AssetCategory>(&sql)
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询顶级资产类别失败: {}", e);
            format!("查询资产类别失败: {}", e)
        })?;

    let count = categories.len();
    info!("查询顶级资产类别成功: 共 {} 条记录", count);
    Ok(categories)
}
//插入新资产类别
pub async fn insert_category(category: &AssetCategory) -> Result<AssetCategory, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!(
        "新增资产类别: name={}, type={}",
        category.category_name, category.asset_type
    );

    let prefix = database::schema_prefix();
    let sql = format!(
        r#"INSERT INTO {}asset_category (id,category_name, asset_type, parent_id, sort, description, created_by, updated_by,created_at,updated_at,deleted)
        VALUES ($1, $2, $3, $4, $5, $6, $7,$8, NOW(), NOW(),0)
        RETURNING id, category_name, asset_type, parent_id, sort, description, created_by, created_at, updated_by, updated_at, deleted"#,
        prefix
    );
    let category = sqlx::query_as::<_, AssetCategory>(&sql)
        .bind((next_id()) as i64)
        .bind(&category.category_name)
        .bind(&category.asset_type)
        .bind(category.parent_id)
        .bind(category.sort)
        .bind(&category.description)
        .bind(&category.created_by)
        .bind(&category.updated_by)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!(
                "新增资产类别失败: name={}, error={}",
                category.category_name, e
            );
            format!("插入资产类别失败: {}", e)
        })?;

    info!(
        "新增资产类别成功: id={}, name={}",
        category.id, category.category_name
    );
    Ok(category)
}
pub async fn update_category(category: &AssetCategory) -> Result<AssetCategory, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!(
        "更新资产类别: id={}, name={}",
        category.id, category.category_name
    );

    let prefix = database::schema_prefix();
    let sql = format!(
        r#"UPDATE {}asset_category
        SET category_name = $2, asset_type = $3, parent_id = $4, sort = $5, description = $6, updated_by = $7, updated_at = NOW(),deleted=0
        WHERE id = $1
        RETURNING id, category_name, asset_type, parent_id, sort, description, created_by, created_at, updated_by, updated_at,deleted"#,
        prefix
    );
    let category = sqlx::query_as::<_, AssetCategory>(&sql)
        .bind(category.id)
        .bind(&category.category_name)
        .bind(&category.asset_type)
        .bind(category.parent_id)
        .bind(category.sort)
        .bind(&category.description)
        .bind(&category.updated_by)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("更新资产类别失败: id={}, error={}", category.id, e);
            format!("更新资产类别失败: {}", e)
        })?;

    info!(
        "更新资产类别成功: id={}, name={}",
        category.id, category.category_name
    );
    Ok(category)
}
pub async fn delete_category(id: i64) -> Result<(), String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("删除资产类别: id={}", id);

    let prefix = database::schema_prefix();
    let sql = format!(
        "UPDATE {}asset_category SET deleted = 1, updated_at = NOW() WHERE id = $1",
        prefix
    );
    sqlx::query(&sql)
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("删除资产类别失败: id={}, error={}", id, e);
            format!("删除资产类别失败: {}", e)
        })?;
    info!("删除资产类别成功: id={}", id);
    Ok(())
}
