use crate::database;
use crate::database::models::{MantineTree, Role, RoleMenu, SidebarMenuItem, SysMenu};
use crate::utils::snowflake::next_id;
use tracing::{error, info};

/// 新增角色
pub async fn insert_role(role: &Role) -> Result<Role, String> {
    let pool = database::get_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("新增角色: name={}, key={}", role.role_name, role.role_key);

    let inserted = sqlx::query_as::<_, Role>(
        "INSERT INTO sys_role (id,role_key, role_name, description, created_by, created_at) VALUES ($1, $2, $3, $4,$5, NOW()) RETURNING id, role_key, role_name, description, created_by, created_at, updated_by, updated_at, deleted"
    )
    .bind(next_id() as i64)
    .bind(&role.role_key)
    .bind(&role.role_name)
    .bind(&role.description)
    .bind(role.created_by)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        error!("新增角色失败: name={}, error={}", role.role_name, e);
        format!("新增角色失败: {}", e)
    })?;

    info!(
        "新增角色成功: id={}, name={}",
        inserted.id, inserted.role_name
    );
    Ok(inserted)
}

/// 获取所有角色列表
pub async fn get_roles() -> Result<Vec<Role>, String> {
    let pool = database::get_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let roles = sqlx::query_as::<_, Role>(
        "SELECT id, role_key, role_name, description, created_by, created_at, updated_by, updated_at, deleted FROM sys_role WHERE deleted IS NULL OR deleted = 0 ORDER BY id ASC"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        error!("查询角色列表失败: {}", e);
        format!("查询角色列表失败: {}", e)
    })?;

    let count = roles.len();
    info!("查询角色列表成功: 共 {} 条记录", count);
    Ok(roles)
}

/// 获取指定角色已分配的菜单权限ID列表
pub async fn get_role_menu_ids(role_id: i64) -> Result<Vec<i64>, String> {
    let pool = database::get_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("查询角色菜单权限: role_id={}", role_id);

    let menus = sqlx::query_as::<_, RoleMenu>(
        "SELECT id, role_id, menu_id, created_by, created_at, updated_by, updated_at, deleted FROM sys_role_menu WHERE role_id = $1 AND (deleted IS NULL OR deleted = 0)"
    )
    .bind(role_id)
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        error!("查询角色权限失败: role_id={}, error={}", role_id, e);
        format!("查询角色权限失败: {}", e)
    })?;

    let menu_ids: Vec<i64> = menus.into_iter().map(|m| m.menu_id).collect();
    info!(
        "查询角色菜单权限成功: role_id={}, 菜单数={}",
        role_id,
        menu_ids.len()
    );
    Ok(menu_ids)
}

/// 为角色分配菜单权限（先删除旧关联，再插入新关联）
pub async fn assign_role_menus(role_id: i64, menu_ids: Vec<i64>) -> Result<(), String> {
    let pool = database::get_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!(
        "分配角色菜单权限: role_id={}, 菜单数={}",
        role_id,
        menu_ids.len()
    );

    // 开启事务
    let mut tx = pool.begin().await.map_err(|e| {
        error!("开启事务失败: role_id={}, error={}", role_id, e);
        format!("开启事务失败: {}", e)
    })?;

    // 1. 删除该角色所有旧的菜单关联
    sqlx::query("DELETE FROM sys_role_menu WHERE role_id = $1")
        .bind(role_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            error!("删除旧权限关联失败: role_id={}, error={}", role_id, e);
            format!("删除旧权限关联失败: {}", e)
        })?;

    // 2. 插入新的菜单关联
    for menu_id in &menu_ids {
        sqlx::query(
            "INSERT INTO sys_role_menu (id ,role_id, menu_id, created_by, created_at) VALUES ($1, $2, $3,$4, NOW())"
        )
        .bind(next_id() as i64)
        .bind(role_id)
        .bind(menu_id)
        .bind(1)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            error!("插入权限关联失败: role_id={}, menu_id={}, error={}", role_id, menu_id, e);
            format!("插入权限关联失败: {}", e)
        })?;
    }

    // 提交事务
    tx.commit().await.map_err(|e| {
        error!("提交事务失败: role_id={}, error={}", role_id, e);
        format!("提交事务失败: {}", e)
    })?;

    info!("分配角色菜单权限成功: role_id={}", role_id);
    Ok(())
}

/// 删除角色（软删除）
pub async fn delete_role(role_id: i64) -> Result<(), String> {
    let pool = database::get_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("删除角色: role_id={}", role_id);

    let mut tx = pool.begin().await.map_err(|e| {
        error!("开启事务失败: role_id={}, error={}", role_id, e);
        format!("开启事务失败: {}", e)
    })?;

    // 1. 软删除角色
    sqlx::query("UPDATE sys_role SET deleted = 1 WHERE id = $1")
        .bind(role_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            error!("删除角色失败: role_id={}, error={}", role_id, e);
            format!("删除角色失败: {}", e)
        })?;

    // 2. 删除角色菜单关联
    sqlx::query("DELETE FROM sys_role_menu WHERE role_id = $1")
        .bind(role_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            error!("删除角色菜单关联失败: role_id={}, error={}", role_id, e);
            format!("删除角色菜单关联失败: {}", e)
        })?;

    // 3. 删除用户角色关联
    sqlx::query("DELETE FROM sys_user_role WHERE role_id = $1")
        .bind(role_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            error!("删除用户角色关联失败: role_id={}, error={}", role_id, e);
            format!("删除用户角色关联失败: {}", e)
        })?;

    tx.commit().await.map_err(|e| {
        error!("提交事务失败: role_id={}, error={}", role_id, e);
        format!("提交事务失败: {}", e)
    })?;

    info!("删除角色成功: role_id={}", role_id);
    Ok(())
}

/// 获取所有菜单树（用于权限分配）
pub async fn get_all_menus_tree() -> Result<Vec<MantineTree>, String> {
    info!("获取所有菜单树被调用");
    let pool = database::get_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let all_menus = sqlx::query_as::<_, SysMenu>(
        "SELECT id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, created_by, created_at, updated_by, updated_at, deleted FROM sys_menu WHERE deleted = 0 ORDER BY order_num ASC"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        error!("查询菜单失败: {}", e);
        format!("查询菜单失败: {}", e)
    })?;

    info!("查询到 {} 条菜单记录", all_menus.len());

    // 构建树形结构
    // parent_id 为 None 或 0 均视为顶级菜单（兼容不同数据来源）
    let root_menus: Vec<&SysMenu> = all_menus
        .iter()
        .filter(|m| m.parent_id.is_none() || m.parent_id == Some(0))
        .collect();
    info!("顶级菜单 {} 条", root_menus.len());
    let result = root_menus
        .iter()
        .map(|root| build_menu_node(root, &all_menus))
        .collect::<Vec<_>>();
    info!("菜单树构建完成，返回 {} 个根节点", result.len());

    Ok(result)
}

fn build_menu_node(menu: &SysMenu, all_menus: &[SysMenu]) -> MantineTree {
    let children: Vec<MantineTree> = all_menus
        .iter()
        .filter(|m| m.parent_id == Some(menu.id))
        .map(|child| build_menu_node(child, all_menus))
        .collect();

    MantineTree {
        value: menu.id.to_string(),
        label: menu.menu_name.clone(),
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
        checked: Some(true),
    }
}

/// 获取用户已分配的角色 ID 列表
pub async fn get_user_role_ids(user_id: i64) -> Result<Vec<i64>, String> {
    let pool = database::get_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("查询用户角色关联: user_id={}", user_id);

    let roles = sqlx::query_as::<_, database::models::UserRole>(
        "SELECT id, user_id, role_id, created_by, created_at, updated_by, updated_at, deleted FROM sys_user_role WHERE user_id = $1 AND (deleted IS NULL OR deleted = 0)"
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        error!("查询用户角色失败: user_id={}, error={}", user_id, e);
        format!("查询用户角色失败: {}", e)
    })?;

    let role_ids: Vec<i64> = roles.into_iter().map(|r| r.role_id).collect();
    info!(
        "查询用户角色成功: user_id={}, 角色数={}",
        user_id,
        role_ids.len()
    );
    Ok(role_ids)
}

/// 为用户分配角色（先删除旧关联，再插入新关联）
pub async fn assign_user_roles(user_id: i64, role_ids: Vec<i64>) -> Result<(), String> {
    let pool = database::get_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!(
        "分配用户角色: user_id={}, 角色数={}",
        user_id,
        role_ids.len()
    );

    // 开启事务
    let mut tx = pool.begin().await.map_err(|e| {
        error!("开启事务失败: user_id={}, error={}", user_id, e);
        format!("开启事务失败: {}", e)
    })?;

    // 1. 删除该用户所有旧的角色关联
    sqlx::query("DELETE FROM sys_user_role WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            error!("删除用户旧角色关联失败: user_id={}, error={}", user_id, e);
            format!("删除用户旧角色关联失败: {}", e)
        })?;

    // 2. 插入新的角色关联
    for role_id in &role_ids {
        sqlx::query(
            "INSERT INTO sys_user_role (id, user_id, role_id, created_by, created_at) VALUES ($1, $2, $3, $4, NOW())"
        )
        .bind(next_id() as i64)
        .bind(user_id)
        .bind(role_id)
        .bind(1)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            error!("插入用户角色关联失败: user_id={}, role_id={}, error={}", user_id, role_id, e);
            format!("插入用户角色关联失败: {}", e)
        })?;
    }

    // 提交事务
    tx.commit().await.map_err(|e| {
        error!("提交事务失败: user_id={}, error={}", user_id, e);
        format!("提交事务失败: {}", e)
    })?;

    info!("分配用户角色成功: user_id={}", user_id);
    Ok(())
}

/// 获取侧边栏菜单（只返回目录和菜单，不返回按钮）
pub async fn get_user_menus() -> Result<Vec<SidebarMenuItem>, String> {
    info!("获取侧边栏菜单被调用");
    let pool = database::get_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let all_menus = sqlx::query_as::<_, SysMenu>(
        "SELECT id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, created_by, created_at, updated_by, updated_at, deleted FROM sys_menu WHERE deleted = 0 AND menu_type IN (1, 2) AND visible = true ORDER BY order_num ASC"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        error!("查询侧边栏菜单失败: {}", e);
        format!("查询侧边栏菜单失败: {}", e)
    })?;

    info!("查询到 {} 条侧边栏菜单记录", all_menus.len());

    // 构建树形结构
    let root_menus: Vec<&SysMenu> = all_menus
        .iter()
        .filter(|m| m.parent_id.is_none() || m.parent_id == Some(0))
        .collect();
    info!("顶级侧边栏菜单 {} 条", root_menus.len());

    let result = root_menus
        .iter()
        .map(|root| build_sidebar_node(root, &all_menus))
        .collect::<Vec<_>>();
    info!("侧边栏菜单树构建完成，返回 {} 个根节点", result.len());

    Ok(result)
}

fn build_sidebar_node(menu: &SysMenu, all_menus: &[SysMenu]) -> SidebarMenuItem {
    let children: Vec<SidebarMenuItem> = all_menus
        .iter()
        .filter(|m| m.parent_id == Some(menu.id))
        .map(|child| build_sidebar_node(child, all_menus))
        .collect();

    SidebarMenuItem {
        label: menu.menu_name.clone(),
        path: menu.path.clone(),
        icon: menu.icon.clone(),
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}
