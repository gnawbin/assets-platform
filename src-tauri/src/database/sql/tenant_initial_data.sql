-- ==============================
-- 租户初始数据（执行前替换 {schema} 为实际 schema 名）
-- ==============================

-- 1. 默认角色
INSERT INTO {schema}.sys_role (id, role_key, role_name, description, created_by, created_at, deleted)
SELECT 1, 'admin', '超级管理员', '超级管理员角色，拥有所有权限', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.sys_role WHERE id = 1);

-- 2. 默认部门
INSERT INTO {schema}.sys_department (id, department_name, parent_id, description, created_by, created_at, deleted)
SELECT 1, '总公司', NULL, '默认顶级部门', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.sys_department WHERE id = 1);

-- 3. 默认用户角色关联（admin 用户 → admin 角色）
INSERT INTO {schema}.sys_user_role (id, user_id, role_id, created_by, created_at, deleted)
SELECT 1, 1, 1, 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.sys_user_role WHERE id = 1);

-- 4. 默认角色菜单关联（admin 角色 → 所有菜单）
INSERT INTO {schema}.sys_role_menu (id, role_id, menu_id, created_by, created_at, deleted)
SELECT
    row_number() OVER (ORDER BY m.id) + 1000,
    1,
    m.id,
    1,
    NOW(),
    0
FROM public.sys_menu m
WHERE NOT EXISTS (
    SELECT 1 FROM {schema}.sys_role_menu rm
    WHERE rm.role_id = 1 AND rm.menu_id = m.id
);