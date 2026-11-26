use database;
use database::models::Department;
use utils::snowflake::next_id;
use tracing::{error, info, warn};

/// 获取所有部门列表（按租户过滤）
pub async fn get_departments(tenant_id: Option<i64>) -> Result<Vec<Department>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    let departments = if let Some(tid) = tenant_id {
        sqlx::query_as::<_, Department>(
            "SELECT id, tenant_id, department_name, parent_id, leader_id, description, created_by, created_at, updated_by, updated_at, deleted FROM public.sys_department WHERE tenant_id = $1 AND (deleted IS NULL OR deleted = 0) ORDER BY id ASC"
        )
        .bind(tid)
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询部门列表失败: {}", e);
            format!("查询部门列表失败: {}", e)
        })?
    } else {
        sqlx::query_as::<_, Department>(
            "SELECT id, tenant_id, department_name, parent_id, leader_id, description, created_by, created_at, updated_by, updated_at, deleted FROM public.sys_department WHERE (deleted IS NULL OR deleted = 0) ORDER BY id ASC"
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询部门列表失败: {}", e);
            format!("查询部门列表失败: {}", e)
        })?
    };

    let count = departments.len();
    info!("查询部门列表成功: 共 {} 条记录", count);
    Ok(departments)
}

/// 新增部门
pub async fn insert_department(
    department_name: &str,
    parent_id: Option<i64>,
    leader_id: Option<i64>,
    description: Option<&str>,
    created_by: Option<i64>,
    tenant_id: i64,
) -> Result<Department, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!(
        "新增部门: name={}, parent_id={:?}, tenant_id={}",
        department_name, parent_id, tenant_id
    );

    let department = sqlx::query_as::<_, Department>(
        r#"
        INSERT INTO public.sys_department (id, tenant_id, department_name, parent_id, leader_id, description, created_by, created_at, updated_by, updated_at, deleted)
        VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), $7, NOW(), 0)
        RETURNING id, tenant_id, department_name, parent_id, leader_id, description, created_by, created_at, updated_by, updated_at, deleted
        "#
    )
    .bind(next_id() as i64)
    .bind(tenant_id)
    .bind(department_name)
    .bind(parent_id.unwrap_or(0))
    .bind(leader_id)
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
    leader_id: Option<i64>,
    description: Option<&str>,
    updated_by: Option<i64>,
) -> Result<Department, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("更新部门: id={}, name={}", id, department_name);

    let department = sqlx::query_as::<_, Department>(
        r#"
        UPDATE public.sys_department
        SET department_name = $2, parent_id = $3, leader_id = $4, description = $5, updated_by = $6, updated_at = NOW()
        WHERE id = $1 AND (deleted IS NULL OR deleted = 0)
        RETURNING id, tenant_id, department_name, parent_id, leader_id, description, created_by, created_at, updated_by, updated_at, deleted
        "#
    )
    .bind(id)
    .bind(department_name)
    .bind(parent_id)
    .bind(leader_id)
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
        "SELECT id, tenant_id, department_name, parent_id, leader_id, description, created_by, created_at, updated_by, updated_at, deleted FROM public.sys_department WHERE parent_id = $1 AND (deleted IS NULL OR deleted = 0)"
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

    sqlx::query("UPDATE public.sys_department SET deleted = 1, updated_at = NOW() WHERE id = $1")
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

/// 审批人解析所需的部门成员。
#[derive(Debug, Clone)]
pub struct DepartmentMember {
    pub id: i64,
    pub name: String,
}

/// 查询部门负责人；负责人为空时向上级部门回退。
pub async fn get_department_leader(
    tenant_id: i64,
    department_id: i64,
) -> Result<Option<DepartmentMember>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {e}"))?;
    let mut current = Some(department_id);

    for _ in 0..20 {
        let Some(id) = current else {
            break;
        };
        let row: Option<(Option<i64>, Option<i64>)> = sqlx::query_as(
            "SELECT parent_id, leader_id FROM public.sys_department
             WHERE id = $1 AND tenant_id = $2 AND (deleted IS NULL OR deleted = 0)",
        )
        .bind(id)
        .bind(tenant_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| format!("查询部门负责人失败: {e}"))?;

        let Some((parent_id, leader_id)) = row else {
            break;
        };
        if let Some(leader_id) = leader_id {
            if let Some(name) = get_active_user_name(tenant_id, leader_id).await? {
                return Ok(Some(DepartmentMember {
                    id: leader_id,
                    name,
                }));
            }
        }
        current = parent_id.filter(|value| *value != 0);
    }
    Ok(None)
}

/// 查询部门成员（可选包含子部门），仅返回启用用户。
pub async fn get_department_members(
    tenant_id: i64,
    department_id: i64,
    include_children: bool,
) -> Result<Vec<DepartmentMember>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {e}"))?;
    let mut department_ids = vec![department_id];

    if include_children {
        let mut frontier = vec![department_id];
        for _ in 0..20 {
            if frontier.is_empty() {
                break;
            }
            let children: Vec<(i64,)> = sqlx::query_as(
                "SELECT id FROM public.sys_department
                 WHERE parent_id = ANY($1) AND tenant_id = $2
                   AND (deleted IS NULL OR deleted = 0)",
            )
            .bind(&frontier)
            .bind(tenant_id)
            .fetch_all(&pool)
            .await
            .map_err(|e| format!("查询子部门失败: {e}"))?;
            frontier = children.into_iter().map(|row| row.0).collect();
            department_ids.extend(frontier.iter().copied());
        }
    }

    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, COALESCE(NULLIF(real_name, ''), username)
         FROM public.sys_user
         WHERE department_id = ANY($1) AND tenant_id = $2
           AND status <> 0
           AND (deleted IS NULL OR deleted = 0)
         ORDER BY id ASC",
    )
    .bind(&department_ids)
    .bind(tenant_id)
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("查询部门成员失败: {e}"))?;

    Ok(rows
        .into_iter()
        .map(|(id, name)| DepartmentMember { id, name })
        .collect())
}

/// 查询部门名称，供执行错误提示使用。
pub async fn get_department_name(tenant_id: i64, department_id: i64) -> Option<String> {
    let pool = database::get_read_pool().ok()?;
    sqlx::query_scalar::<_, String>(
        "SELECT department_name FROM public.sys_department
         WHERE id = $1 AND tenant_id = $2 AND (deleted IS NULL OR deleted = 0)",
    )
    .bind(department_id)
    .bind(tenant_id)
    .fetch_optional(&pool)
    .await
    .ok()
    .flatten()
}

async fn get_active_user_name(tenant_id: i64, user_id: i64) -> Result<Option<String>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {e}"))?;
    sqlx::query_scalar::<_, String>(
        "SELECT COALESCE(NULLIF(real_name, ''), username)
         FROM public.sys_user
         WHERE id = $1 AND tenant_id = $2
           AND status <> 0
           AND (deleted IS NULL OR deleted = 0)",
    )
    .bind(user_id)
    .bind(tenant_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("查询用户失败: {e}"))
}
