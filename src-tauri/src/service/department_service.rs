use crate::database;
use crate::database::models::Department;
use crate::utils::snowflake::next_id;
use tracing::{error, info, warn};

/// 获取所有部门列表
pub async fn get_departments() -> Result<Vec<Department>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let departments = sqlx::query_as::<_, Department>(
        "SELECT id, department_name, parent_id, description, created_by, created_at, updated_by, updated_at, deleted FROM sys_department WHERE deleted IS NULL OR deleted = 0 ORDER BY id ASC"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        error!("查询部门列表失败: {}", e);
        format!("查询部门列表失败: {}", e)
    })?;

    let count = departments.len();
    info!("查询部门列表成功: 共 {} 条记录", count);
    Ok(departments)
}

/// 新增部门
pub async fn insert_department(
    department_name: &str,
    parent_id: Option<i64>,
    description: Option<&str>,
    created_by: Option<i64>,
) -> Result<Department, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!(
        "新增部门: name={}, parent_id={:?}",
        department_name, parent_id
    );

    let department = sqlx::query_as::<_, Department>(
        r#"
        INSERT INTO sys_department (id, department_name, parent_id, description, created_by, created_at, updated_by, updated_at, deleted)
        VALUES ($1, $2, $3, $4, $5, NOW(), $5, NOW(), 0)
        RETURNING id, department_name, parent_id, description, created_by, created_at, updated_by, updated_at, deleted
        "#
    )
    .bind(next_id() as i64)
    .bind(department_name)
    .bind(parent_id)
    .bind(description)
    .bind(created_by)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        error!("新增部门失败: name={}, error={}", department_name, e);
        format!("新增部门失败: {}", e)
    })?;

    info!(
        "新增部门成功: id={}, name={}",
        department.id, department.department_name
    );
    Ok(department)
}

/// 更新部门信息
pub async fn update_department(
    id: i64,
    department_name: &str,
    parent_id: Option<i64>,
    description: Option<&str>,
    updated_by: Option<i64>,
) -> Result<Department, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("更新部门: id={}, name={}", id, department_name);

    let department = sqlx::query_as::<_, Department>(
        r#"
        UPDATE sys_department
        SET department_name = $2, parent_id = $3, description = $4, updated_by = $5, updated_at = NOW()
        WHERE id = $1 AND (deleted IS NULL OR deleted = 0)
        RETURNING id, department_name, parent_id, description, created_by, created_at, updated_by, updated_at, deleted
        "#
    )
    .bind(id)
    .bind(department_name)
    .bind(parent_id)
    .bind(description)
    .bind(updated_by)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        error!("更新部门失败: id={}, error={}", id, e);
        format!("更新部门失败: {}", e)
    })?;

    info!("更新部门成功: id={}, name={}", id, department_name);
    Ok(department)
}

/// 删除部门（软删除）
pub async fn delete_department(id: i64) -> Result<(), String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("删除部门: id={}", id);

    // 先检查是否有子部门
    let children = sqlx::query_as::<_, Department>(
        "SELECT id, department_name, parent_id, description, created_by, created_at, updated_by, updated_at, deleted FROM sys_department WHERE parent_id = $1 AND (deleted IS NULL OR deleted = 0)"
    )
    .bind(id)
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        error!("查询子部门失败: parent_id={}, error={}", id, e);
        format!("查询子部门失败: {}", e)
    })?;

    if !children.is_empty() {
        warn!(
            "删除部门失败，存在子部门: id={}, 子部门数={}",
            id,
            children.len()
        );
        return Err("该部门下存在子部门，请先删除子部门".to_string());
    }

    sqlx::query("UPDATE sys_department SET deleted = 1, updated_at = NOW() WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("删除部门失败: id={}, error={}", id, e);
            format!("删除部门失败: {}", e)
        })?;

    info!("删除部门成功: id={}", id);
    Ok(())
}
