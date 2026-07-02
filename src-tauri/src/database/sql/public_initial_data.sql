-- ==============================
-- public schema 初始数据
-- ==============================

-- 1. 默认租户
INSERT INTO
    public.sys_tenant (
        id,
        tenant_name,
        schema_name,
        enable
    )
SELECT 1, '默认厂区', 'single', true
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_tenant
        WHERE
            id = 1
    );

-- ==============================
-- 系统菜单种子数据
-- 对应 public.sys_menu 表结构
-- menu_type: 1=目录 2=菜单 3=按钮
-- ==============================

-- =====================
-- 1. 顶级目录/菜单
-- =====================
INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    1,
    '仪表盘',
    NULL,
    '/',
    '/Dashboard',
    'IconDashboard',
    1,
    true,
    NULL,
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 1
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    2,
    '资产台账',
    NULL,
    NULL,
    NULL,
    'IconBooks',
    2,
    true,
    NULL,
    1,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 2
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    3,
    '流程管理',
    NULL,
    NULL,
    NULL,
    'IconListCheck',
    3,
    true,
    NULL,
    1,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 3
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    4,
    '统计分析',
    NULL,
    NULL,
    NULL,
    'IconChartBar',
    4,
    true,
    NULL,
    1,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 4
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    5,
    '系统配置',
    NULL,
    NULL,
    NULL,
    'IconSettings',
    5,
    true,
    NULL,
    1,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 5
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    6,
    '知识库',
    NULL,
    NULL,
    NULL,
    'IconBrain',
    6,
    true,
    NULL,
    1,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 6
    );

-- =====================
-- 2. 资产台账 子菜单
-- =====================
INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    10,
    '资产分类',
    2,
    '/categories',
    '/categories/page',
    NULL,
    1,
    true,
    'asset:category:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 10
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    11,
    '硬资产',
    2,
    '/hardware',
    '/hardware/page',
    NULL,
    2,
    true,
    'asset:hardware:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 11
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    12,
    '软资产',
    2,
    '/software',
    '/software/page',
    NULL,
    3,
    true,
    'asset:software:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 12
    );

-- =====================
-- 3. 流程管理 子菜单
-- =====================
INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    20,
    '领用审批',
    3,
    '/process/approval',
    '/process/approval/page',
    NULL,
    1,
    true,
    'process:approval:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 20
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    21,
    '归还确认',
    3,
    '/process/return',
    '/process/return/page',
    NULL,
    2,
    true,
    'process:return:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 21
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    22,
    '调拨流程',
    3,
    '/process/transfer',
    '/process/transfer/page',
    NULL,
    3,
    true,
    'process:transfer:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 22
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    23,
    '维修流程',
    3,
    '/process/maintenance',
    '/process/maintenance/page',
    NULL,
    4,
    true,
    'process:maintenance:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 23
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    24,
    '报废流程',
    3,
    '/process/scrap',
    '/process/scrap/page',
    NULL,
    5,
    true,
    'process:scrap:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 24
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    25,
    '所有流程',
    3,
    '/process/all',
    '/process/all/page',
    NULL,
    6,
    true,
    'process:all:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 25
    );

-- =====================
-- 4. 统计分析 子菜单
-- =====================
INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    30,
    '资产统计',
    4,
    '/statistics/assets',
    '/statistics/assets/page',
    NULL,
    1,
    true,
    'statistics:assets:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 30
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    31,
    '部门分布',
    4,
    '/statistics/department',
    '/statistics/department/page',
    NULL,
    2,
    true,
    'statistics:department:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 31
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    32,
    '状态分析',
    4,
    '/statistics/status',
    '/statistics/status/page',
    NULL,
    3,
    true,
    'statistics:status:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 32
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    33,
    '维保统计',
    4,
    '/statistics/maintenance',
    '/statistics/maintenance/page',
    NULL,
    4,
    true,
    'statistics:maintenance:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 33
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    34,
    '授权统计',
    4,
    '/statistics/license',
    '/statistics/license/page',
    NULL,
    5,
    true,
    'statistics:license:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 34
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    35,
    '报表导出',
    4,
    '/statistics/export',
    '/statistics/export/page',
    NULL,
    6,
    true,
    'statistics:export:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 35
    );

-- =====================
-- 5. 知识库 子菜单
-- =====================
INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    50,
    '知识库管理',
    6,
    '/knowledge',
    '/knowledge/page',
    NULL,
    1,
    true,
    'knowledge:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 50
    );

-- =====================
-- 6. 知识库 子菜单（续）
-- =====================

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    52,
    '智能问答',
    6,
    '/chat',
    '/chat/page',
    NULL,
    2,
    true,
    'knowledge:chat',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 52
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    54,
    'LLM厂商管理',
    6,
    '/settings/llm',
    '/settings/llm/page',
    NULL,
    4,
    true,
    'knowledge:llm:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 54
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    55,
    '模型偏好',
    6,
    '/settings/llm/preference',
    '/settings/llm/preference/page',
    NULL,
    5,
    true,
    'knowledge:llm:preference',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 55
    );

-- Skill 管理移到知识库下（原父级 AI 工作流已删除）
INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    60,
    'Skill 管理',
    6,
    '/skills',
    '/skills/page',
    NULL,
    3,
    true,
    'skill:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 60
    );

-- =====================
-- 7. 系统配置 子菜单
-- =====================

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    41,
    '权限管理',
    5,
    '/settings/permissions',
    '/settings/permissions/page',
    NULL,
    2,
    true,
    'system:permission:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 41
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    42,
    '部门管理',
    5,
    '/settings/departments',
    '/settings/departments/page',
    NULL,
    2,
    true,
    'system:department:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 42
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    43,
    '用户管理',
    5,
    '/settings/users',
    '/settings/users/page',
    NULL,
    3,
    true,
    'system:user:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 43
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    45,
    '系统日志',
    5,
    '/settings/logs',
    '/settings/logs/page',
    NULL,
    4,
    true,
    'system:log:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 45
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    46,
    '租户管理',
    5,
    '/settings/tenants',
    '/settings/tenants/page',
    NULL,
    5,
    true,
    'system:tenant:list',
    2,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 46
    );

-- =====================
-- 6. 按钮级权限
-- =====================

-- 6.1 资产分类按钮
INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    100,
    '新增分类',
    10,
    NULL,
    NULL,
    NULL,
    1,
    true,
    'asset:category:add',
    3,
    false,
    'insert_category',
    'POST',
    '/api/categories',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 100
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    101,
    '编辑分类',
    10,
    NULL,
    NULL,
    NULL,
    2,
    true,
    'asset:category:edit',
    3,
    false,
    'update_category',
    'PUT',
    '/api/categories/{id}',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 101
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    102,
    '删除分类',
    10,
    NULL,
    NULL,
    NULL,
    3,
    true,
    'asset:category:delete',
    3,
    false,
    'delete_category',
    'DELETE',
    '/api/categories/{id}',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 102
    );

-- 6.2 硬资产按钮
INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    110,
    '新增硬资产',
    11,
    NULL,
    NULL,
    NULL,
    1,
    true,
    'asset:hardware:add',
    3,
    false,
    'insert_hardware_asset',
    'POST',
    '/api/assets/hardware',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 110
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    111,
    '编辑硬资产',
    11,
    NULL,
    NULL,
    NULL,
    2,
    true,
    'asset:hardware:edit',
    3,
    false,
    'update_hardware_asset',
    'PUT',
    '/api/assets/hardware/{id}',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 111
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    112,
    '删除硬资产',
    11,
    NULL,
    NULL,
    NULL,
    3,
    true,
    'asset:hardware:delete',
    3,
    false,
    'delete_hardware_asset',
    'DELETE',
    '/api/assets/hardware/{id}',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 112
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    113,
    '导出硬资产',
    11,
    NULL,
    NULL,
    NULL,
    4,
    true,
    'asset:hardware:export',
    3,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 113
    );

-- 6.3 软资产按钮
INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    120,
    '新增软资产',
    12,
    NULL,
    NULL,
    NULL,
    1,
    true,
    'asset:software:add',
    3,
    false,
    'insert_intangible_asset',
    'POST',
    '/api/assets/intangible',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 120
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    121,
    '编辑软资产',
    12,
    NULL,
    NULL,
    NULL,
    2,
    true,
    'asset:software:edit',
    3,
    false,
    'update_intangible_asset',
    'PUT',
    '/api/assets/intangible/{id}',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 121
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    122,
    '删除软资产',
    12,
    NULL,
    NULL,
    NULL,
    3,
    true,
    'asset:software:delete',
    3,
    false,
    'delete_intangible_asset',
    'DELETE',
    '/api/assets/intangible/{id}',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 122
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    123,
    '导出软资产',
    12,
    NULL,
    NULL,
    NULL,
    4,
    true,
    'asset:software:export',
    3,
    false,
    NULL,
    NULL,
    NULL,
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 123
    );

-- 6.4 流程管理按钮
INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    130,
    '新增领用',
    20,
    NULL,
    NULL,
    NULL,
    1,
    true,
    'process:receive:add',
    3,
    false,
    'insert_receive',
    'POST',
    '/api/process/receive',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 130
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    131,
    '审批领用',
    20,
    NULL,
    NULL,
    NULL,
    2,
    true,
    'process:receive:approve',
    3,
    false,
    'approve_receive',
    'PUT',
    '/api/process/receive/{id}/approve',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 131
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    132,
    '删除领用',
    20,
    NULL,
    NULL,
    NULL,
    3,
    true,
    'process:receive:delete',
    3,
    false,
    'delete_receive',
    'DELETE',
    '/api/process/receive/{id}',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 132
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    133,
    '新增归还',
    21,
    NULL,
    NULL,
    NULL,
    1,
    true,
    'process:return:add',
    3,
    false,
    'insert_return',
    'POST',
    '/api/process/return',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 133
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    134,
    '确认归还',
    21,
    NULL,
    NULL,
    NULL,
    2,
    true,
    'process:return:confirm',
    3,
    false,
    'confirm_return',
    'PUT',
    '/api/process/return/{id}/confirm',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 134
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    135,
    '删除归还',
    21,
    NULL,
    NULL,
    NULL,
    3,
    true,
    'process:return:delete',
    3,
    false,
    'delete_return',
    'DELETE',
    '/api/process/return/{id}',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 135
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    136,
    '新增调拨',
    22,
    NULL,
    NULL,
    NULL,
    1,
    true,
    'process:transfer:add',
    3,
    false,
    'insert_transfer',
    'POST',
    '/api/process/transfer',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 136
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    137,
    '审批调拨',
    22,
    NULL,
    NULL,
    NULL,
    2,
    true,
    'process:transfer:approve',
    3,
    false,
    'approve_transfer',
    'PUT',
    '/api/process/transfer/{id}/approve',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 137
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    138,
    '删除调拨',
    22,
    NULL,
    NULL,
    NULL,
    3,
    true,
    'process:transfer:delete',
    3,
    false,
    'delete_transfer',
    'DELETE',
    '/api/process/transfer/{id}',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 138
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    139,
    '新增维修',
    23,
    NULL,
    NULL,
    NULL,
    1,
    true,
    'process:repair:add',
    3,
    false,
    'insert_repair',
    'POST',
    '/api/process/repair',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 139
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    140,
    '维修完成',
    23,
    NULL,
    NULL,
    NULL,
    2,
    true,
    'process:repair:complete',
    3,
    false,
    'complete_repair',
    'PUT',
    '/api/process/repair/{id}/complete',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 140
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    141,
    '删除维修',
    23,
    NULL,
    NULL,
    NULL,
    3,
    true,
    'process:repair:delete',
    3,
    false,
    'delete_repair',
    'DELETE',
    '/api/process/repair/{id}',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 141
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    142,
    '新增报废',
    24,
    NULL,
    NULL,
    NULL,
    1,
    true,
    'process:scrap:add',
    3,
    false,
    'insert_scrap',
    'POST',
    '/api/process/scrap',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 142
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    143,
    '审批报废',
    24,
    NULL,
    NULL,
    NULL,
    2,
    true,
    'process:scrap:approve',
    3,
    false,
    'approve_scrap',
    'PUT',
    '/api/process/scrap/{id}/approve',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 143
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    144,
    '删除报废',
    24,
    NULL,
    NULL,
    NULL,
    3,
    true,
    'process:scrap:delete',
    3,
    false,
    'delete_scrap',
    'DELETE',
    '/api/process/scrap/{id}',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 144
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    145,
    '新增采购',
    25,
    NULL,
    NULL,
    NULL,
    1,
    true,
    'process:purchase:add',
    3,
    false,
    'insert_purchase',
    'POST',
    '/api/process/purchase',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 145
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    146,
    '审批采购',
    25,
    NULL,
    NULL,
    NULL,
    2,
    true,
    'process:purchase:approve',
    3,
    false,
    'approve_purchase',
    'PUT',
    '/api/process/purchase/{id}/approve',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 146
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    147,
    '删除采购',
    25,
    NULL,
    NULL,
    NULL,
    3,
    true,
    'process:purchase:delete',
    3,
    false,
    'delete_purchase',
    'DELETE',
    '/api/process/purchase/{id}',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 147
    );

-- 6.5 系统配置按钮

-- 租户管理按钮
INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    148,
    '新增租户',
    46,
    NULL,
    NULL,
    NULL,
    1,
    true,
    'system:tenant:add',
    3,
    false,
    'insert_tenant',
    'POST',
    '/api/tenants',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 148
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    149,
    '编辑租户',
    46,
    NULL,
    NULL,
    NULL,
    2,
    true,
    'system:tenant:edit',
    3,
    false,
    'update_tenant',
    'PUT',
    '/api/tenants/{id}',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 149
    );

INSERT INTO
    public.sys_menu (
        id,
        menu_name,
        parent_id,
        path,
        component,
        icon,
        order_num,
        visible,
        perms,
        menu_type,
        hidden_button,
        command_name,
        http_method,
        http_path,
        created_by,
        created_at,
        updated_by,
        updated_at,
        deleted
    )
SELECT
    150,
    '禁用租户',
    46,
    NULL,
    NULL,
    NULL,
    3,
    true,
    'system:tenant:delete',
    3,
    false,
    'delete_tenant',
    'DELETE',
    '/api/tenants/{id}',
    1,
    NOW(),
    NULL,
    NULL,
    0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_menu
        WHERE
            id = 150
    );

-- ==============================
-- 7. 默认角色
-- ==============================
INSERT INTO
    public.sys_role (
        id,
        role_key,
        role_name,
        description,
        is_super_admin,
        tenant_id,
        created_by,
        created_at,
        deleted
    )
SELECT 1, 'super_admin', '超级管理员', '超级管理员角色，拥有所有权限', true, NULL, 1, NOW(), 0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_role
        WHERE
            id = 1
    );

-- ==============================
-- 8. 默认角色菜单关联（super_admin 角色 → 所有菜单）
-- ==============================
INSERT INTO
    public.sys_role_menu (
        id,
        role_id,
        menu_id,
        created_by,
        created_at,
        deleted
    )
SELECT row_number() OVER (
        ORDER BY m.id
    ) + 1000, 1, m.id, 1, NOW(), 0
FROM public.sys_menu m
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_role_menu rm
        WHERE
            rm.role_id = 1
            AND rm.menu_id = m.id
    );

-- ==============================
-- 9. 默认用户角色关联（admin 用户 → super_admin 角色）
-- ==============================
INSERT INTO
    public.sys_user_role (
        id,
        user_id,
        role_id,
        created_by,
        created_at,
        deleted
    )
SELECT 1, 1, 1, 1, NOW(), 0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_user_role
        WHERE
            user_id = 1
            AND role_id = 1
    );