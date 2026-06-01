use crate::database;
use crate::database::models::AssetCategory;
use crate::utils::snowflake::next_id;

/// 获取所有资产类别列表
pub async fn get_categories() -> Result<Vec<AssetCategory>, String> {
    let pool = database::get_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let categories = sqlx::query_as::<_, AssetCategory>(
        "SELECT id, category_name, asset_type, parent_id, sort, description, created_by, created_at, updated_by, updated_at,deleted FROM asset_category ORDER BY sort ASC"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("查询资产类别失败: {}", e))?;

    Ok(categories)
}

//获取资产类别最高级别的列表
pub async fn get_super_categories() -> Result<Vec<AssetCategory>, String> {
    let pool = database::get_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let categories = sqlx::query_as::<_, AssetCategory>(
        "SELECT id, category_name, asset_type, parent_id, sort, description, created_by, created_at, updated_by, updated_at,deleted  FROM asset_category where parent_id=0 ORDER BY sort ASC"
    )
    .fetch_all(&pool)
    .await

    .map_err(|e| format!("查询资产类别失败: {}", e))?;

    Ok(categories)
}
//插入新资产类别
pub async fn insert_category(category: &AssetCategory) -> Result<AssetCategory, String> {
    let pool = database::get_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let category = sqlx::query_as::<_, AssetCategory>(
        r#"
        INSERT INTO asset_category (id,category_name, asset_type, parent_id, sort, description, created_by, updated_by,created_at,updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7,$8, NOW(), NOW())
        RETURNING id, category_name, asset_type, parent_id, sort, description, created_by, created_at, updated_by, updated_at
        "#
    )
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
    .map_err(|e| format!("插入资产类别失败: {}", e))?;

    Ok(category)
}
pub async fn update_category(category: &AssetCategory) -> Result<AssetCategory, String> {
    let pool = database::get_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let category = sqlx::query_as::<_, AssetCategory>(
        r#"
        UPDATE asset_category
        SET category_name = $2, asset_type = $3, parent_id = $4, sort = $5, description = $6, updated_by = $7, updated_at = NOW()
        WHERE id = $1
        RETURNING id, category_name, asset_type, parent_id, sort, description, created_by, created_at, updated_by, updated_at
        "#
    )
    .bind(category.id)
    .bind(&category.category_name)
    .bind(&category.asset_type)
    .bind(category.parent_id)
    .bind(category.sort)
    .bind(&category.description)
    .bind(&category.updated_by)
    .fetch_one(&pool)
    .await
    .map_err(|e| format!("更新资产类别失败: {}", e))?;

    Ok(category)
}
pub async fn delete_category(id: i64) -> Result<(), String> {
    let pool = database::get_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    sqlx::query(
        r#"
        UPDATE asset_category
        SET deleted = true, updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .execute(&pool)
    .await
    .map_err(|e| format!("删除资产类别失败: {}", e))?;

    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[tokio::test]
    async fn test_insert_category() {
        let category = AssetCategory {
            id: 0,
            category_name: "测试类别".to_string(),
            asset_type: "测试类型".to_string(),
            parent_id: 0,
            sort: 1i16,
            description: Some("这是一个测试类别".to_string()),
            created_by: Some(1i64),
            created_at: Some(Utc::now()),
            updated_by: Some(1i64),
            updated_at: Some(Utc::now()),
            deleted: Some(0i16),
        };
        let result = insert_category(&category).await;
        match result {
            Ok(inserted_category) => {
                println!("插入成功: {:?}", inserted_category);
                assert_eq!(inserted_category.category_name, category.category_name);
                assert_eq!(inserted_category.asset_type, category.asset_type);
                assert_eq!(inserted_category.parent_id, category.parent_id);
                assert_eq!(inserted_category.sort, category.sort);
                assert_eq!(inserted_category.description, category.description);
                assert_eq!(inserted_category.created_by, category.created_by);
                assert_eq!(inserted_category.updated_by, category.updated_by);
                assert_eq!(inserted_category.deleted, category.deleted);
            }
            Err(e) => {
                panic!("插入失败: {}", e);
            }
        }
    }
}
