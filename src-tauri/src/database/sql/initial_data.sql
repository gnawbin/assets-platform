-- ==============================
-- 系统菜单种子数据
-- 对应 sys_menu 表结构
-- menu_type: 1=目录 2=菜单 3=按钮
-- ==============================

-- 清空已有数据（谨慎使用）
-- TRUNCATE TABLE role_menu, sys_menu RESTART IDENTITY CASCADE;

-- =====================
-- 1. 顶级目录/菜单
-- =====================
INSERT INTO sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, created_by, created_at, updated_by, updated_at, deleted)
VALUES
(1,  '仪表盘',   NULL, '/',              '/Dashboard',        'IconDashboard',   1,  true,  NULL,                          2, false, 1, NOW(), NULL, NULL, 0),
(2,  '资产台账',  NULL, NULL,             NULL,                'IconBooks',       2,  true,  NULL,                          1, false, 1, NOW(), NULL, NULL, 0),
(3,  '流程管理',  NULL, NULL,             NULL,                'IconListCheck',   3,  true,  NULL,                          1, false, 1, NOW(), NULL, NULL, 0),
(4,  '统计分析',  NULL, NULL,             NULL,                'IconChartBar',    4,  true,  NULL,                          1, false, 1, NOW(), NULL, NULL, 0),
(5,  '系统配置',  NULL, NULL,             NULL,                'IconSettings',    5,  true,  NULL,                          1, false, 1, NOW(), NULL, NULL, 0);

-- =====================
-- 2. 资产台账 子菜单
-- =====================
INSERT INTO sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, created_by, created_at, updated_by, updated_at, deleted)
VALUES
(10, '资产分类', 2, '/categories',       '/categories/page',  NULL,              1,  true,  'asset:category:list',         2, false, 1, NOW(), NULL, NULL, 0),
(11, '硬资产',   2, '/hardware',         '/hardware/page',    NULL,              2,  true,  'asset:hardware:list',         2, false, 1, NOW(), NULL, NULL, 0),
(12, '软资产',   2, '/software',         '/software/page',    NULL,              3,  true,  'asset:software:list',         2, false, 1, NOW(), NULL, NULL, 0);

-- =====================
-- 3. 流程管理 子菜单
-- =====================
INSERT INTO sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, created_by, created_at, updated_by, updated_at, deleted)
VALUES
(20, '领用审批',  3, '/process/approval',    '/process/approval/page',    NULL,  1,  true,  'process:approval:list',    2, false, 1, NOW(), NULL, NULL, 0),
(21, '归还确认',  3, '/process/return',     '/process/return/page',      NULL,  2,  true,  'process:return:list',      2, false, 1, NOW(), NULL, NULL, 0),
(22, '调拨流程',  3, '/process/transfer',   '/process/transfer/page',    NULL,  3,  true,  'process:transfer:list',    2, false, 1, NOW(), NULL, NULL, 0),
(23, '维修流程',  3, '/process/maintenance','/process/maintenance/page', NULL,  4,  true,  'process:maintenance:list', 2, false, 1, NOW(), NULL, NULL, 0),
(24, '报废流程',  3, '/process/scrap',      '/process/scrap/page',       NULL,  5,  true,  'process:scrap:list',       2, false, 1, NOW(), NULL, NULL, 0),
(25, '所有流程',  3, '/process/all',        '/process/all/page',         NULL,  6,  true,  'process:all:list',         2, false, 1, NOW(), NULL, NULL, 0);

-- =====================
-- 4. 统计分析 子菜单
-- =====================
INSERT INTO sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, created_by, created_at, updated_by, updated_at, deleted)
VALUES
(30, '资产统计',   4, '/statistics/assets',      '/statistics/assets/page',      NULL,  1,  true,  'statistics:assets:list',      2, false, 1, NOW(), NULL, NULL, 0),
(31, '部门分布',   4, '/statistics/department',  '/statistics/department/page',  NULL,  2,  true,  'statistics:department:list',  2, false, 1, NOW(), NULL, NULL, 0),
(32, '状态分析',   4, '/statistics/status',      '/statistics/status/page',      NULL,  3,  true,  'statistics:status:list',      2, false, 1, NOW(), NULL, NULL, 0),
(33, '维保统计',   4, '/statistics/maintenance', '/statistics/maintenance/page', NULL,  4,  true,  'statistics:maintenance:list', 2, false, 1, NOW(), NULL, NULL, 0),
(34, '授权统计',   4, '/statistics/license',     '/statistics/license/page',     NULL,  5,  true,  'statistics:license:list',     2, false, 1, NOW(), NULL, NULL, 0),
(35, '报表导出',   4, '/statistics/export',      '/statistics/export/page',      NULL,  6,  true,  'statistics:export:list',      2, false, 1, NOW(), NULL, NULL, 0);

-- =====================
-- 5. 系统配置 子菜单
-- =====================
INSERT INTO sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, created_by, created_at, updated_by, updated_at, deleted)
VALUES
(40, '数据库配置',   5, '/settings/database',       '/settings/database/page',       NULL,  1,  true,  'system:database:config',   2, false, 1, NOW(), NULL, NULL, 0),
(41, '权限管理',     5, '/settings/permissions',    '/settings/permissions/page',    NULL,  2,  true,  'system:permission:list',   2, false, 1, NOW(), NULL, NULL, 0),
(42, '部门管理',     5, '/settings/departments',    '/settings/departments/page',    NULL,  3,  true,  'system:department:list',   2, false, 1, NOW(), NULL, NULL, 0),
(43, '用户管理',     5, '/settings/users',          '/settings/users/page',          NULL,  4,  true,  'system:user:list',         2, false, 1, NOW(), NULL, NULL, 0),
(44, '流程设计',     5, '/settings/process-design','/settings/process-design/page', NULL,  5,  true,  'system:process:design',    2, false, 1, NOW(), NULL, NULL, 0),
(45, '系统日志',     5, '/settings/logs',           '/settings/logs/page',           NULL,  6,  true,  'system:log:list',          2, false, 1, NOW(), NULL, NULL, 0);

-- =====================
-- 6. 按钮级权限
-- =====================

-- 6.1 资产分类按钮
INSERT INTO sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, created_by, created_at, updated_by, updated_at, deleted)
VALUES
(100, '新增分类', 10, NULL, NULL, NULL, 1, true, 'asset:category:add',    3, false, 1, NOW(), NULL, NULL, 0),
(101, '编辑分类', 10, NULL, NULL, NULL, 2, true, 'asset:category:edit',   3, false, 1, NOW(), NULL, NULL, 0),
(102, '删除分类', 10, NULL, NULL, NULL, 3, true, 'asset:category:delete', 3, false, 1, NOW(), NULL, NULL, 0);

-- 6.2 硬资产按钮
INSERT INTO sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, created_by, created_at, updated_by, updated_at, deleted)
VALUES
(110, '新增硬资产', 11, NULL, NULL, NULL, 1, true, 'asset:hardware:add',    3, false, 1, NOW(), NULL, NULL, 0),
(111, '编辑硬资产', 11, NULL, NULL, NULL, 2, true, 'asset:hardware:edit',   3, false, 1, NOW(), NULL, NULL, 0),
(112, '删除硬资产', 11, NULL, NULL, NULL, 3, true, 'asset:hardware:delete', 3, false, 1, NOW(), NULL, NULL, 0),
(113, '导出硬资产', 11, NULL, NULL, NULL, 4, true, 'asset:hardware:export', 3, false, 1, NOW(), NULL, NULL, 0);

-- 6.3 软资产按钮
INSERT INTO sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, created_by, created_at, updated_by, updated_at, deleted)
VALUES
(120, '新增软资产', 12, NULL, NULL, NULL, 1, true, 'asset:software:add',    3, false, 1, NOW(), NULL, NULL, 0),
(121, '编辑软资产', 12, NULL, NULL, NULL, 2, true, 'asset:software:edit',   3, false, 1, NOW(), NULL, NULL, 0),
(122, '删除软资产', 12, NULL, NULL, NULL, 3, true, 'asset:software:delete', 3, false, 1, NOW(), NULL, NULL, 0),
(123, '导出软资产', 12, NULL, NULL, NULL, 4, true, 'asset:software:export', 3, false, 1, NOW(), NULL, NULL, 0);

-- 6.4 流程管理按钮
INSERT INTO sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, created_by, created_at, updated_by, updated_at, deleted)
VALUES
(130, '审批通过', 20, NULL, NULL, NULL, 1, true, 'process:approval:approve', 3, false, 1, NOW(), NULL, NULL, 0),
(131, '审批驳回', 20, NULL, NULL, NULL, 2, true, 'process:approval:reject',  3, false, 1, NOW(), NULL, NULL, 0),
(132, '确认归还', 21, NULL, NULL, NULL, 1, true, 'process:return:confirm',   3, false, 1, NOW(), NULL, NULL, 0),
(133, '新增调拨', 22, NULL, NULL, NULL, 1, true, 'process:transfer:add',     3, false, 1, NOW(), NULL, NULL, 0),
(134, '新增维修', 23, NULL, NULL, NULL, 1, true, 'process:maintenance:add',  3, false, 1, NOW(), NULL, NULL, 0),
(135, '新增报废', 24, NULL, NULL, NULL, 1, true, 'process:scrap:add',        3, false, 1, NOW(), NULL, NULL, 0);

-- 6.5 系统配置按钮
INSERT INTO sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, created_by, created_at, updated_by, updated_at, deleted)
VALUES
(140, '新增角色',   41, NULL, NULL, NULL, 1, true, 'system:permission:add',    3, false, 1, NOW(), NULL, NULL, 0),
(141, '编辑角色',   41, NULL, NULL, NULL, 2, true, 'system:permission:edit',   3, false, 1, NOW(), NULL, NULL, 0),
(142, '删除角色',   41, NULL, NULL, NULL, 3, true, 'system:permission:delete', 3, false, 1, NOW(), NULL, NULL, 0),
(143, '新增部门',   42, NULL, NULL, NULL, 1, true, 'system:department:add',    3, false, 1, NOW(), NULL, NULL, 0),
(144, '编辑部门',   42, NULL, NULL, NULL, 2, true, 'system:department:edit',   3, false, 1, NOW(), NULL, NULL, 0),
(145, '删除部门',   42, NULL, NULL, NULL, 3, true, 'system:department:delete', 3, false, 1, NOW(), NULL, NULL, 0),
(146, '新增用户',   43, NULL, NULL, NULL, 1, true, 'system:user:add',          3, false, 1, NOW(), NULL, NULL, 0),
(147, '编辑用户',   43, NULL, NULL, NULL, 2, true, 'system:user:edit',         3, false, 1, NOW(), NULL, NULL, 0),
(148, '删除用户',   43, NULL, NULL, NULL, 3, true, 'system:user:delete',       3, false, 1, NOW(), NULL, NULL, 0),
(149, '重置密码',   43, NULL, NULL, NULL, 4, true, 'system:user:reset-pwd',    3, false, 1, NOW(), NULL, NULL, 0);

-- =====================
-- 更新序列
-- =====================
SELECT setval('sys_menu_id_seq', COALESCE((SELECT MAX(id) FROM sys_menu), 1));

INSERT INTO sys_role(id, role_key, role_name, description, created_by, created_at, updated_by, updated_at, deleted)
VALUES(1,'admin','管理员','系统管理员',1,NOW(),1,NOW(),0);

select setval('sys_role_id_seq', (select max(id) from sys_role));
INSERT INTO sys_role_menu(id, role_id, menu_id, created_by, created_at, updated_by, updated_at, deleted)
VALUES(1,1,1,1,NOW(),1,NOW(),0),
(2,1,2,1,NOW(),1,NOW(),0),
(3,1,3,1,NOW(),1,NOW(),0),
(4,1,4,1,NOW(),1,NOW(),0),
(5,1,5,1,NOW(),1,NOW(),0),
(6,1,10,1,NOW(),1,NOW(),0),
(7,1,11,1,NOW(),1,NOW(),0),
(8,1,12,1,NOW(),1,NOW(),0),
(9,1,20,1,NOW(),1,NOW(),0),
(10,1,21,1,NOW(),1,NOW(),0),
(11,1,22,1,NOW(),1,NOW(),0),
(12,1,23,1,NOW(),1,NOW(),0),
(13,1,24,1,NOW(),1,NOW(),0),
(14,1,25,1,NOW(),1,NOW(),0),
(15,1,30,1,NOW(),1,NOW(),0),
(16,1,31,1,NOW(),1,NOW(),0),
(17,1,32,1,NOW(),1,NOW(),0),
(18,1,33,1,NOW(),1,NOW(),0),
(19,1,34,1,NOW(),1,NOW(),0),
(20,1,35,1,NOW(),1,NOW(),0),
(21,1,40,1,NOW(),1,NOW(),0),
(22,1,41,1,NOW(),1,NOW(),0),
(23,1,42,1,NOW(),1,NOW(),0),
(24,1,43,1,NOW(),1,NOW(),0),
(25,1,44,1,NOW(),1,NOW(),0),
(26,1,45,1,NOW(),1,NOW(),0),
(27,1,100,1,NOW(),1,NOW(),0),
(28,1,101,1,NOW(),1,NOW(),0),
(29,1,102,1,NOW(),1,NOW(),0),
(30,1,110,1,NOW(),1,NOW(),0),
(31,1,111,1,NOW(),1,NOW(),0),
(32,1,112,1,NOW(),1,NOW(),0),
(33,1,113,1,NOW(),1,NOW(),0),
(34,1,120,1,NOW(),1,NOW(),0),
(35,1,121,1,NOW(),1,NOW(),0),
(36,1,122,1,NOW(),1,NOW(),0),
(37,1,123,1,NOW(),1,NOW(),0),
(38,1,130,1,NOW(),1,NOW(),0),
(39,1,131,1,NOW(),1,NOW(),0),
(40,1,132,1,NOW(),1,NOW(),0),
(41,1,133,1,NOW(),1,NOW(),0),
(42,1,134,1,NOW(),1,NOW(),0),
(43,1,135,1,NOW(),1,NOW(),0),
(44,1,140,1,NOW(),1,NOW(),0),
(45,1,141,1,NOW(),1,NOW(),0),
(46,1,142,1,NOW(),1,NOW(),0),
(47,1,143,1,NOW(),1,NOW(),0),
(48,1,144,1,NOW(),1,NOW(),0),
(49,1,145,1,NOW(),1, NOW(),0),
(50,1,146,1,NOW(),1,NOW(),0),
(51,1,147,1,NOW(),1,NOW(),0),
(52,1,148,1,NOW(),1,NOW(),0),
(53,1,149,1,NOW(),1,NOW(),0);  
select setval('sys_role_menu_id_seq', (select max(id) from sys_role_menu));

INSERT INTO SYS_USER(id,  username, passwd, domain, real_name, email, phone, department_id, status, nickname, avatar, person_id, person_code, super_user_id, created_by, created_at, updated_by, updated_at, deleted)
VALUES(1, 'admin', '$argon2id$v=19$m=19456,t=2,p=1$iBgqWh/LgwmCcfXByfgy/Q$s/j6aLm9NzqlouAPvYehwEMjIN7CfOjBS7kcQKjoee0', NULL, '系统管理员', 'admin@example.com', '13800138000', NULL, 1, '管理员', NULL, NULL, NULL, NULL, 1, NOW(), 1, NOW(), 0);

select setval('sys_user_id_seq',(select max(id) from sys_user)) 


