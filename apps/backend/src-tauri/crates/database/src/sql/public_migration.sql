-- ==============================
-- public schema 迁移脚本（补充缺失列）
-- 用于已有旧表的数据库，新部署的库通过 public_tables.sql 直接建表
-- ==============================

-- sys_tenant 表增加树状结构字段
ALTER TABLE public.sys_tenant
ADD COLUMN IF NOT EXISTS parent_id BIGINT REFERENCES public.sys_tenant (id);

ALTER TABLE public.sys_tenant
ADD COLUMN IF NOT EXISTS is_leaf BOOLEAN NOT NULL DEFAULT false;

ALTER TABLE public.sys_tenant
ADD COLUMN IF NOT EXISTS updated_at TIMESTAMP
WITH
    TIME ZONE;

ALTER TABLE public.sys_tenant ALTER COLUMN schema_name DROP NOT NULL;

COMMENT ON COLUMN public.sys_tenant.parent_id IS '父组织ID';

COMMENT ON COLUMN public.sys_tenant.is_leaf IS '是否末级节点（末级才有 schema）';

COMMENT ON COLUMN public.sys_tenant.schema_name IS '对应 PostgreSQL schema 名，如 factory_a（仅末级节点需要）';

-- sys_user 表补充缺失列

-- 将 tenant_id 改为可空（超级管理员不属于任何机构）
ALTER TABLE public.sys_user ALTER COLUMN tenant_id DROP NOT NULL;

-- 新增 public.sys_role 表（角色从租户 schema 移到 public）
CREATE TABLE IF NOT EXISTS public.sys_role (
    id BIGINT PRIMARY KEY,
    role_key VARCHAR(100) NOT NULL,
    role_name VARCHAR(100) NOT NULL,
    description TEXT,
    is_super_admin BOOLEAN NOT NULL DEFAULT false,
    tenant_id BIGINT REFERENCES public.sys_tenant (id),
    created_by BIGINT,
    created_at TIMESTAMP
    WITH
        TIME ZONE,
        updated_by BIGINT,
        updated_at TIMESTAMP
    WITH
        TIME ZONE,
        deleted SMALLINT DEFAULT 0
);

COMMENT ON TABLE public.sys_role IS '全局角色表';

COMMENT ON COLUMN public.sys_role.role_key IS '角色标识';

COMMENT ON COLUMN public.sys_role.role_name IS '角色名称';

COMMENT ON COLUMN public.sys_role.is_super_admin IS '是否超级管理员角色';

COMMENT ON COLUMN public.sys_role.tenant_id IS '所属组织ID（超级管理员角色为空）';

-- 新增 public.sys_user_role 表（用户角色关联从租户 schema 移到 public）
CREATE TABLE IF NOT EXISTS public.sys_user_role (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    role_id BIGINT NOT NULL,
    created_by BIGINT,
    created_at TIMESTAMP
    WITH
        TIME ZONE,
        updated_by BIGINT,
        updated_at TIMESTAMP
    WITH
        TIME ZONE,
        deleted SMALLINT DEFAULT 0
);

COMMENT ON TABLE public.sys_user_role IS '用户角色关联表';

-- 新增 public.sys_role_menu 表（角色菜单关联从租户 schema 移到 public）
CREATE TABLE IF NOT EXISTS public.sys_role_menu (
    id BIGINT PRIMARY KEY,
    role_id BIGINT NOT NULL,
    menu_id BIGINT NOT NULL,
    created_by BIGINT,
    created_at TIMESTAMP
    WITH
        TIME ZONE,
        updated_by BIGINT,
        updated_at TIMESTAMP
    WITH
        TIME ZONE,
        deleted SMALLINT DEFAULT 0
);

COMMENT ON TABLE public.sys_role_menu IS '角色菜单关联表';

ALTER TABLE public.sys_user
ADD COLUMN IF NOT EXISTS email VARCHAR(255);

ALTER TABLE public.sys_user
ADD COLUMN IF NOT EXISTS phone VARCHAR(50);

ALTER TABLE public.sys_user
ADD COLUMN IF NOT EXISTS department_id BIGINT;

ALTER TABLE public.sys_user
ADD COLUMN IF NOT EXISTS is_super_admin BOOLEAN NOT NULL DEFAULT false;

ALTER TABLE public.sys_user
ADD COLUMN IF NOT EXISTS status SMALLINT NOT NULL DEFAULT 1;

ALTER TABLE public.sys_user
ADD COLUMN IF NOT EXISTS nickname VARCHAR(255);

ALTER TABLE public.sys_user
ADD COLUMN IF NOT EXISTS avatar VARCHAR(500);

ALTER TABLE public.sys_user
ADD COLUMN IF NOT EXISTS person_id VARCHAR(50);

ALTER TABLE public.sys_user
ADD COLUMN IF NOT EXISTS person_code VARCHAR(50);

ALTER TABLE public.sys_user
ADD COLUMN IF NOT EXISTS super_user_id BIGINT;

ALTER TABLE public.sys_user
ADD COLUMN IF NOT EXISTS created_by BIGINT;

ALTER TABLE public.sys_user
ADD COLUMN IF NOT EXISTS updated_by BIGINT;

ALTER TABLE public.sys_user
ADD COLUMN IF NOT EXISTS updated_at TIMESTAMP
WITH
    TIME ZONE;

ALTER TABLE public.sys_user
ADD COLUMN IF NOT EXISTS deleted SMALLINT DEFAULT 0;

-- 补充注释
COMMENT ON COLUMN public.sys_user.email IS '邮箱';

COMMENT ON COLUMN public.sys_user.phone IS '电话';

COMMENT ON COLUMN public.sys_user.department_id IS '部门ID';

COMMENT ON COLUMN public.sys_user.status IS '状态：1=正常，0=禁用';

COMMENT ON COLUMN public.sys_user.nickname IS '昵称';

COMMENT ON COLUMN public.sys_user.avatar IS '头像';

COMMENT ON COLUMN public.sys_user.person_id IS '身份证号';

COMMENT ON COLUMN public.sys_user.person_code IS '工号';

COMMENT ON COLUMN public.sys_user.super_user_id IS '上级用户ID';

COMMENT ON COLUMN public.sys_user.deleted IS '删除标志：0=未删除，1=已删除';

-- ==============================
-- 刷新令牌表（JWT refresh token 白名单 / 轮换链 / 重放检测）
-- 说明：老库启动时通过 migrate_public_tables 补齐；IF NOT EXISTS 保证幂等
-- ==============================
CREATE TABLE IF NOT EXISTS public.sys_user_refresh_token (
    jti BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    family_id BIGINT NOT NULL,
    issued_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP WITH TIME ZONE NOT NULL,
    revoked_at TIMESTAMP WITH TIME ZONE,
    replaced_by BIGINT
);

COMMENT ON TABLE public.sys_user_refresh_token IS '刷新令牌白名单（每次刷新轮换，支持重放检测）';

COMMENT ON COLUMN public.sys_user_refresh_token.jti IS '令牌唯一ID（雪花ID，写入 JWT 的 jti）';

COMMENT ON COLUMN public.sys_user_refresh_token.family_id IS '轮换族ID（检测到重放时整族撤销）';

COMMENT ON COLUMN public.sys_user_refresh_token.revoked_at IS '撤销时间（非空表示已失效）';

COMMENT ON COLUMN public.sys_user_refresh_token.replaced_by IS '轮换后的新 jti';

CREATE INDEX IF NOT EXISTS idx_public_sys_user_refresh_token_user ON public.sys_user_refresh_token (user_id);

CREATE INDEX IF NOT EXISTS idx_public_sys_user_refresh_token_family ON public.sys_user_refresh_token (family_id);

-- ==============================
-- API Key（外部系统接入凭证，设计文档 §9.1）
-- 说明：
--   * key_hash = SHA-256(完整 key) 的十六进制，单向哈希，不可逆
--   * key_prefix 为明文定位段，用于列表展示、日志定位与用户自查
--   * 主键由应用层 Snowflake 生成（utils::snowflake::next_id），不用 BIGSERIAL
--   * 全部 IF NOT EXISTS，保证老库每次启动幂等执行
-- ==============================
CREATE TABLE IF NOT EXISTS public.sys_api_key (
    id BIGINT PRIMARY KEY,
    tenant_id BIGINT,
    name VARCHAR(100) NOT NULL,
    description VARCHAR(500),
    key_prefix VARCHAR(32) NOT NULL,
    key_hash CHAR(64) NOT NULL,
    scopes TEXT[] NOT NULL DEFAULT '{}',
    rate_limit_per_min INT NOT NULL DEFAULT 600,
    expires_at TIMESTAMP WITH TIME ZONE,
    last_used_at TIMESTAMP WITH TIME ZONE,
    revoked_at TIMESTAMP WITH TIME ZONE,
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT now(),
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT now(),
    enable BOOLEAN NOT NULL DEFAULT TRUE,
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE public.sys_api_key IS '外部系统 API Key（只存哈希，明文仅创建/轮换响应出现一次）';

COMMENT ON COLUMN public.sys_api_key.tenant_id IS '所属租户（空表示 public 全局 key）';

COMMENT ON COLUMN public.sys_api_key.key_prefix IS '定位段（明文，用于展示与日志定位，不参与校验）';

COMMENT ON COLUMN public.sys_api_key.key_hash IS '完整 key 的 SHA-256 十六进制（等值查找，禁止可逆加密）';

COMMENT ON COLUMN public.sys_api_key.scopes IS '权限点集合（必须 ⊆ 创建者当前权限，禁止 system:* 与 upload:*）';

COMMENT ON COLUMN public.sys_api_key.rate_limit_per_min IS '每分钟请求数上限（默认 600）';

COMMENT ON COLUMN public.sys_api_key.revoked_at IS '吊销时间（非空即失效，行保留用于审计）';

CREATE UNIQUE INDEX IF NOT EXISTS uk_sys_api_key_hash ON public.sys_api_key (key_hash);

CREATE INDEX IF NOT EXISTS idx_sys_api_key_tenant ON public.sys_api_key (tenant_id, enable, deleted);

CREATE INDEX IF NOT EXISTS idx_sys_api_key_prefix ON public.sys_api_key (key_prefix);

-- ==============================
-- 权限点补录（Phase 1，设计文档 §6.2 / §6.3）
-- 说明：
--   * 只填 perms / http_method / http_path，**不填 command_name**
--     （T-6：command_name 是 Tauri 命令 → 权限的映射，新接口没有 Tauri 命令）
--   * 全部为 menu_type = 3（按钮），hidden_button = true（不出现在侧边栏）
--   * 同一 perms 覆盖多条路由时只登记一条代表性记录，
--     其余路由由代码侧 api::authz::ROUTE_PERMS 兜底（设计文档 §6.3 手段 2）
--   * id 段 200–299 专用于 HTTP 鉴权权限点，避免与既有 1–165 冲突
-- ==============================

-- API Key 管理（system:apikey:*）
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
    200,
    'API Key 列表',
    5,
    NULL,
    NULL,
    NULL,
    1,
    true,
    'system:apikey:list',
    3,
    true,
    NULL,
    'GET',
    '/api/api-keys',
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
            id = 200
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
    201,
    '创建 API Key',
    5,
    NULL,
    NULL,
    NULL,
    2,
    true,
    'system:apikey:add',
    3,
    true,
    NULL,
    'POST',
    '/api/api-keys',
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
            id = 201
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
    202,
    '轮换 API Key',
    5,
    NULL,
    NULL,
    NULL,
    3,
    true,
    'system:apikey:edit',
    3,
    true,
    NULL,
    'POST',
    '/api/api-keys/{id}/rotate',
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
            id = 202
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
    203,
    '吊销 API Key',
    5,
    NULL,
    NULL,
    NULL,
    4,
    true,
    'system:apikey:delete',
    3,
    true,
    NULL,
    'DELETE',
    '/api/api-keys/{id}',
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
            id = 203
    );

-- OAuth 客户端管理（system:oauthclient:*，Phase 2 才挂路由，这里先落权限点）
INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 205, 'OAuth 客户端列表', 5, NULL, NULL, NULL, 5, true, 'system:oauthclient:list', 3, true, NULL, NULL, NULL, 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 205);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 206, '新增 OAuth 客户端', 5, NULL, NULL, NULL, 6, true, 'system:oauthclient:add', 3, true, NULL, NULL, NULL, 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 206);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 207, '编辑 OAuth 客户端', 5, NULL, NULL, NULL, 7, true, 'system:oauthclient:edit', 3, true, NULL, NULL, NULL, 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 207);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 208, '删除 OAuth 客户端', 5, NULL, NULL, NULL, 8, true, 'system:oauthclient:delete', 3, true, NULL, NULL, NULL, 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 208);

-- API Key 管理页面（侧边栏可见，perms 与列表权限点一致）
INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 48, 'API Key 管理', 5, '/settings/api-keys', '/settings/api-keys/page', 'IconKey', 9, true, 'system:apikey:list', 2, false, NULL, NULL, NULL, 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 48);

-- Agent Skill（/api/skills*，改造前挂在 auth 之外，Phase 1 一并纳入鉴权）
INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 210, '技能列表', 5, NULL, NULL, NULL, 10, true, 'system:skill:list', 3, true, NULL, 'GET', '/api/skills', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 210);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 211, '执行技能', 5, NULL, NULL, NULL, 11, true, 'system:skill:execute', 3, true, NULL, 'POST', '/api/skills/execute', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 211);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 212, '注册技能', 5, NULL, NULL, NULL, 12, true, 'system:skill:add', 3, true, NULL, 'POST', '/api/skills/register', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 212);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 213, '注销技能', 5, NULL, NULL, NULL, 13, true, 'system:skill:delete', 3, true, NULL, 'DELETE', '/api/skills/{skill_id}', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 213);

-- 注册审核（/api/auth/registrations*，改造前无权限点）
INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 214, '注册申请列表', 5, NULL, NULL, NULL, 14, true, 'system:registration:list', 3, true, NULL, 'GET', '/api/auth/registrations', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 214);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 215, '审核注册申请', 5, NULL, NULL, NULL, 15, true, 'system:registration:approve', 3, true, NULL, 'POST', '/api/auth/registrations/{id}/approve', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 215);

-- 大文件上传（/api/upload/*，改造前匿名可访问）
INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 220, '上传文件', 5, NULL, NULL, NULL, 16, true, 'upload:file:add', 3, true, NULL, 'POST', '/api/upload/init', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 220);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 221, '上传进度', 5, NULL, NULL, NULL, 17, true, 'upload:file:list', 3, true, NULL, 'GET', '/api/upload/{id}/progress', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 221);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 222, '中止上传', 5, NULL, NULL, NULL, 18, true, 'upload:file:delete', 3, true, NULL, 'DELETE', '/api/upload/{id}', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 222);

-- 智能问答（/api/chat/stream，改造前匿名可访问）
INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 230, '智能问答', 52, NULL, NULL, NULL, 1, true, 'chat:completion:add', 3, true, NULL, 'POST', '/api/chat/stream', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 230);

-- 用户管理写操作（/api/users*）
INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 240, '新增用户', 43, NULL, NULL, NULL, 1, true, 'system:user:add', 3, true, NULL, 'POST', '/api/users', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 240);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 241, '编辑用户', 43, NULL, NULL, NULL, 2, true, 'system:user:edit', 3, true, NULL, 'PUT', '/api/users/{id}', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 241);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 242, '删除用户', 43, NULL, NULL, NULL, 3, true, 'system:user:delete', 3, true, NULL, 'DELETE', '/api/users/{id}', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 242);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 243, '重置密码', 43, NULL, NULL, NULL, 4, true, 'system:user:reset', 3, true, NULL, 'POST', '/api/users/{id}/reset-password', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 243);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 244, '分配用户角色', 43, NULL, NULL, NULL, 5, true, 'system:user:assign', 3, true, NULL, 'POST', '/api/users/{id}/roles', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 244);


-- 角色管理写操作（/api/roles*）
INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 245, '新增角色', 41, NULL, NULL, NULL, 1, true, 'system:role:add', 3, true, NULL, 'POST', '/api/roles', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 245);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 246, '删除角色', 41, NULL, NULL, NULL, 2, true, 'system:role:delete', 3, true, NULL, 'DELETE', '/api/roles/{id}', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 246);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 247, '分配角色权限', 41, NULL, NULL, NULL, 3, true, 'system:role:assign', 3, true, NULL, 'POST', '/api/roles/{id}/menus', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 247);

-- 部门管理写操作（/api/departments*）
INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 250, '新增部门', 42, NULL, NULL, NULL, 1, true, 'system:department:add', 3, true, NULL, 'POST', '/api/departments', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 250);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 251, '编辑部门', 42, NULL, NULL, NULL, 2, true, 'system:department:edit', 3, true, NULL, 'PUT', '/api/departments/{id}', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 251);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 252, '删除部门', 42, NULL, NULL, NULL, 3, true, 'system:department:delete', 3, true, NULL, 'DELETE', '/api/departments/{id}', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 252);

-- 租户分配 / 切换（租户切换是超管跨租户能力，独立权限点 + 审计）
INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 253, '分配用户租户', 46, NULL, NULL, NULL, 4, true, 'system:tenant:assign', 3, true, NULL, 'POST', '/api/tenants/assign', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 253);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 254, '切换租户', 46, NULL, NULL, NULL, 5, true, 'system:tenant:switch', 3, true, NULL, 'POST', '/api/tenants/switch', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 254);

-- 知识库写操作（/api/knowledge*）
INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 260, '新增知识', 50, NULL, NULL, NULL, 1, true, 'knowledge:add', 3, true, NULL, 'POST', '/api/knowledge', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 260);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 261, '编辑知识', 50, NULL, NULL, NULL, 2, true, 'knowledge:edit', 3, true, NULL, 'PUT', '/api/knowledge/{id}', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 261);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 262, '删除知识', 50, NULL, NULL, NULL, 3, true, 'knowledge:delete', 3, true, NULL, 'DELETE', '/api/knowledge/{id}', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 262);



-- 流程列表 / 编辑（补齐原先只有 add / approve / delete 的缺口）
INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 270, '领用申请列表', 20, NULL, NULL, NULL, 1, true, 'process:receive:list', 3, true, NULL, 'GET', '/api/process/receive', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 270);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 271, '编辑领用申请', 20, NULL, NULL, NULL, 2, true, 'process:receive:edit', 3, true, NULL, 'PUT', '/api/process/receive/{id}', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 271);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 272, '编辑归还申请', 21, NULL, NULL, NULL, 2, true, 'process:return:edit', 3, true, NULL, 'PUT', '/api/process/return/{id}', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 272);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 273, '编辑调拨申请', 22, NULL, NULL, NULL, 2, true, 'process:transfer:edit', 3, true, NULL, 'PUT', '/api/process/transfer/{id}', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 273);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 274, '维修申请列表', 23, NULL, NULL, NULL, 1, true, 'process:repair:list', 3, true, NULL, 'GET', '/api/process/repair', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 274);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 275, '编辑维修申请', 23, NULL, NULL, NULL, 2, true, 'process:repair:edit', 3, true, NULL, 'PUT', '/api/process/repair/{id}', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 275);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 276, '编辑报废申请', 24, NULL, NULL, NULL, 2, true, 'process:scrap:edit', 3, true, NULL, 'PUT', '/api/process/scrap/{id}', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 276);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 277, '采购申请列表', 25, NULL, NULL, NULL, 1, true, 'process:purchase:list', 3, true, NULL, 'GET', '/api/process/purchase', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 277);

INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 278, '编辑采购申请', 25, NULL, NULL, NULL, 2, true, 'process:purchase:edit', 3, true, NULL, 'PUT', '/api/process/purchase/{id}', 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 278);

-- ============================================================================
-- OAuth 2.1 授权服务器（Phase 2，设计文档 §9.2）
--   只新增表与索引，不改既有表结构（§16 T-5 加法式 DDL）；
--   客户端管理权限点（system:oauthclient:*，id 205-208）已在 Phase 1 落库，
--   这里补上 http_method / http_path，供鉴权自检与 DB 优先映射比对。
-- ============================================================================
CREATE TABLE IF NOT EXISTS public.sys_oauth_client (
    id BIGINT PRIMARY KEY,
    client_id VARCHAR(64) NOT NULL,
    client_name VARCHAR(100) NOT NULL,
    client_secret_hash CHAR(64),
    client_type SMALLINT NOT NULL DEFAULT 1,
    redirect_uris TEXT[] NOT NULL DEFAULT '{}',
    allowed_scopes TEXT[] NOT NULL DEFAULT '{}',
    grant_types TEXT[] NOT NULL DEFAULT '{authorization_code,refresh_token}',
    access_token_ttl INT NOT NULL DEFAULT 900,
    refresh_token_ttl INT NOT NULL DEFAULT 2592000,
    tenant_id BIGINT,
    enable BOOLEAN NOT NULL DEFAULT TRUE,
    deleted SMALLINT NOT NULL DEFAULT 0,
    created_by BIGINT,
    created_at TIMESTAMPTZ DEFAULT now(),
    updated_by BIGINT,
    updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS uk_sys_oauth_client_id ON public.sys_oauth_client (client_id);

CREATE INDEX IF NOT EXISTS idx_sys_oauth_client_tenant ON public.sys_oauth_client (tenant_id, enable, deleted);

CREATE TABLE IF NOT EXISTS public.sys_oauth_code (
    id BIGINT PRIMARY KEY,
    code_hash CHAR(64) NOT NULL,
    client_id VARCHAR(64) NOT NULL,
    user_id BIGINT NOT NULL,
    tenant_id BIGINT,
    redirect_uri TEXT NOT NULL,
    scopes TEXT[] NOT NULL DEFAULT '{}',
    code_challenge VARCHAR(128) NOT NULL,
    code_challenge_method VARCHAR(10) NOT NULL DEFAULT 'S256',
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS uk_sys_oauth_code_hash ON public.sys_oauth_code (code_hash);

CREATE INDEX IF NOT EXISTS idx_sys_oauth_code_expires ON public.sys_oauth_code (expires_at);

CREATE TABLE IF NOT EXISTS public.sys_oauth_token (
    id BIGINT PRIMARY KEY,
    token_hash CHAR(64) NOT NULL,
    token_type SMALLINT NOT NULL,
    client_id VARCHAR(64) NOT NULL,
    user_id BIGINT,
    tenant_id BIGINT,
    scopes TEXT[] NOT NULL DEFAULT '{}',
    family_id UUID NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    replaced_by VARCHAR(64),
    created_at TIMESTAMPTZ DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS uk_sys_oauth_token_hash ON public.sys_oauth_token (token_hash);

CREATE INDEX IF NOT EXISTS idx_sys_oauth_token_family ON public.sys_oauth_token (family_id);

CREATE INDEX IF NOT EXISTS idx_sys_oauth_token_user ON public.sys_oauth_token (user_id, revoked_at);

-- 浏览器会话（方案 B，设计文档 §7.3 / §9.2）：
--   `/oauth/authorize` 的浏览器重定向流程需要「已登录的用户会话」，
--   这里只增加一张表（§16 T-5 加法式 DDL），会话令牌只存 SHA-256。
CREATE TABLE IF NOT EXISTS public.sys_user_session (
    id BIGINT PRIMARY KEY,
    session_hash CHAR(64) NOT NULL,
    user_id BIGINT NOT NULL,
    tenant_id BIGINT,
    csrf_hash CHAR(64) NOT NULL,
    ip INET,
    user_agent VARCHAR(512),
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    last_seen_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT now()
);

COMMENT ON TABLE public.sys_user_session IS '浏览器会话（OAuth 方案 B：httpOnly Cookie 的 authorize 流程，只存哈希）';

COMMENT ON COLUMN public.sys_user_session.session_hash IS '会话令牌的 SHA-256 十六进制（Cookie 里的明文不落库）';

COMMENT ON COLUMN public.sys_user_session.csrf_hash IS '同意页 CSRF 令牌的 SHA-256（双提交 Cookie 校验）';

COMMENT ON COLUMN public.sys_user_session.revoked_at IS '撤销时间（登出 / 改密 / 封禁，非空即失效）';

CREATE UNIQUE INDEX IF NOT EXISTS uk_sys_user_session_hash ON public.sys_user_session (session_hash);

CREATE INDEX IF NOT EXISTS idx_sys_user_session_user ON public.sys_user_session (user_id, revoked_at);

CREATE INDEX IF NOT EXISTS idx_sys_user_session_expires ON public.sys_user_session (expires_at);

-- 客户端管理权限点补登路由（与 ROUTE_PERMS 保持一致，DB 优先）
UPDATE public.sys_menu SET http_method = 'GET', http_path = '/api/oauth-clients' WHERE id = 205 AND http_path IS NULL;

UPDATE public.sys_menu SET http_method = 'POST', http_path = '/api/oauth-clients' WHERE id = 206 AND http_path IS NULL;

UPDATE public.sys_menu SET http_method = 'PUT', http_path = '/api/oauth-clients/{id}' WHERE id = 207 AND http_path IS NULL;

UPDATE public.sys_menu SET http_method = 'DELETE', http_path = '/api/oauth-clients/{id}' WHERE id = 208 AND http_path IS NULL;

-- OAuth 客户端管理页面（侧边栏可见，perms 与列表权限点一致）
INSERT INTO public.sys_menu (id, menu_name, parent_id, path, component, icon, order_num, visible, perms, menu_type, hidden_button, command_name, http_method, http_path, created_by, created_at, updated_by, updated_at, deleted)
SELECT 49, 'OAuth 客户端', 5, '/settings/oauth-clients', '/settings/oauth-clients/page', 'IconApps', 10, true, 'system:oauthclient:list', 2, false, NULL, NULL, NULL, 1, NOW(), NULL, NULL, 0
WHERE NOT EXISTS (SELECT 1 FROM public.sys_menu WHERE id = 49);

