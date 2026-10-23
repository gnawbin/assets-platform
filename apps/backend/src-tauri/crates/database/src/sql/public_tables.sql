-- ==============================
-- public schema 公共表
-- ==============================

-- 1. 组织结构配置表（树状结构）
CREATE TABLE IF NOT EXISTS public.sys_tenant (
    id BIGINT PRIMARY KEY,
    tenant_name VARCHAR(100) NOT NULL,
    parent_id BIGINT REFERENCES public.sys_tenant (id),
    is_leaf BOOLEAN NOT NULL DEFAULT false,
    schema_name VARCHAR(50) UNIQUE,
    enable BOOLEAN DEFAULT true,
    create_at TIMESTAMP
    WITH
        TIME ZONE DEFAULT CURRENT_TIMESTAMP,
        updated_at TIMESTAMP
    WITH
        TIME ZONE
);

COMMENT ON TABLE public.sys_tenant IS '组织结构配置表（树状结构）';

COMMENT ON COLUMN public.sys_tenant.tenant_name IS '组织结构名称';

COMMENT ON COLUMN public.sys_tenant.parent_id IS '父组织ID';

COMMENT ON COLUMN public.sys_tenant.is_leaf IS '是否末级节点（末级才有 schema）';

COMMENT ON COLUMN public.sys_tenant.schema_name IS '对应 PostgreSQL schema 名，如 factory_a（仅末级节点需要）';

COMMENT ON COLUMN public.sys_tenant.enable IS '是否启用';

-- 2. 全局登录用户表
CREATE TABLE IF NOT EXISTS public.sys_user (
    id BIGINT PRIMARY KEY,
    username VARCHAR(100) NOT NULL UNIQUE,
    passwd VARCHAR(255) NOT NULL,
    domain VARCHAR(100),
    real_name VARCHAR(255) NOT NULL,
    email VARCHAR(255),
    phone VARCHAR(50),
    department_id BIGINT,
    is_super_admin BOOLEAN NOT NULL DEFAULT false,
    status SMALLINT NOT NULL DEFAULT 1,
    nickname VARCHAR(255),
    avatar VARCHAR(500),
    person_id VARCHAR(50),
    person_code VARCHAR(50),
    super_user_id BIGINT,
    tenant_id BIGINT REFERENCES public.sys_tenant (id),
    created_by BIGINT,
    created_at TIMESTAMP
    WITH
        TIME ZONE DEFAULT CURRENT_TIMESTAMP,
        updated_by BIGINT,
        updated_at TIMESTAMP
    WITH
        TIME ZONE,
        deleted SMALLINT DEFAULT 0
);

COMMENT ON TABLE public.sys_user IS '全局登录用户表';

COMMENT ON COLUMN public.sys_user.tenant_id IS '所属组织ID';

COMMENT ON COLUMN public.sys_user.email IS '邮箱';

COMMENT ON COLUMN public.sys_user.phone IS '电话';

COMMENT ON COLUMN public.sys_user.department_id IS '部门ID';

COMMENT ON COLUMN public.sys_user.is_super_admin IS '是否超级管理员';

COMMENT ON COLUMN public.sys_user.status IS '状态：1=正常，0=禁用';

COMMENT ON COLUMN public.sys_user.nickname IS '昵称';

COMMENT ON COLUMN public.sys_user.avatar IS '头像';

COMMENT ON COLUMN public.sys_user.person_id IS '身份证号';

COMMENT ON COLUMN public.sys_user.person_code IS '工号';

COMMENT ON COLUMN public.sys_user.super_user_id IS '上级用户ID';

COMMENT ON COLUMN public.sys_user.deleted IS '删除标志：0=未删除，1=已删除';

-- 3. 全局菜单表（结构不变，仅加 public. 前缀）
CREATE TABLE IF NOT EXISTS public.sys_menu (
    id BIGINT PRIMARY KEY,
    menu_name VARCHAR(255) NOT NULL,
    parent_id BIGINT,
    path VARCHAR(255),
    component VARCHAR(255),
    icon VARCHAR(255),
    order_num SMALLINT NOT NULL,
    visible BOOLEAN NOT NULL,
    perms VARCHAR(255),
    menu_type SMALLINT NOT NULL,
    hidden_button BOOLEAN NOT NULL,
    command_name VARCHAR(255),
    http_method VARCHAR(10),
    http_path VARCHAR(255),
    created_by BIGINT,
    created_at TIMESTAMP
    WITH
        TIME ZONE,
        updated_by BIGINT,
        updated_at TIMESTAMP
    WITH
        TIME ZONE,
        deleted SMALLINT NOT NULL
);

COMMENT ON
TABLE public.sys_menu IS '系统菜单&权限表（同时存储 Tauri 命令名 → HTTP 路由映射）';

-- 4. 全局角色表
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

-- 5. 用户角色关联表
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

-- 6. 角色菜单关联表
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

-- 7. 全局部门表（多组织共享，通过 tenant_id 区分）
CREATE TABLE IF NOT EXISTS public.sys_department (
    id BIGINT PRIMARY KEY,
    tenant_id BIGINT NOT NULL REFERENCES public.sys_tenant (id),
    department_name VARCHAR(255) NOT NULL,
    parent_id BIGINT,
    description TEXT,
    created_by BIGINT,
    created_at TIMESTAMP
    WITH
        TIME ZONE,
        updated_by BIGINT,
        updated_at TIMESTAMP
    WITH
        TIME ZONE,
        deleted SMALLINT
);

COMMENT ON TABLE public.sys_department IS '全局部门表（多组织共享）';

COMMENT ON COLUMN public.sys_department.tenant_id IS '所属组织ID';

COMMENT ON COLUMN public.sys_department.department_name IS '部门名称';

COMMENT ON COLUMN public.sys_department.parent_id IS '父部门ID';

COMMENT ON COLUMN public.sys_department.description IS '部门描述';

-- 5. 用户注册申请表
CREATE TABLE IF NOT EXISTS public.sys_user_register (
    id BIGINT PRIMARY KEY,
    username VARCHAR(100) NOT NULL UNIQUE,
    passwd VARCHAR(255) NOT NULL,
    real_name VARCHAR(255) NOT NULL,
    email VARCHAR(255),
    phone VARCHAR(50),
    department_name VARCHAR(255),
    company_name VARCHAR(255),
    reason TEXT,
    status SMALLINT NOT NULL DEFAULT 0,
    approve_by BIGINT,
    approve_time TIMESTAMP
    WITH
        TIME ZONE,
        approve_remark TEXT,
        created_at TIMESTAMP
    WITH
        TIME ZONE DEFAULT CURRENT_TIMESTAMP,
        updated_at TIMESTAMP
    WITH
        TIME ZONE
);

COMMENT ON TABLE public.sys_user_register IS '用户注册申请表';

COMMENT ON COLUMN public.sys_user_register.status IS '状态：0=待审核、1=已通过、2=已驳回';

COMMENT ON COLUMN public.sys_user_register.approve_by IS '审核人ID';

COMMENT ON COLUMN public.sys_user_register.approve_time IS '审核时间';

COMMENT ON COLUMN public.sys_user_register.approve_remark IS '审核备注';

-- 8. 用户-组织关联表（多对多）
CREATE TABLE IF NOT EXISTS public.sys_user_tenant (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES public.sys_user (id) ON DELETE CASCADE,
    tenant_id BIGINT NOT NULL REFERENCES public.sys_tenant (id) ON DELETE CASCADE,
    created_by BIGINT,
    created_at TIMESTAMP
    WITH
        TIME ZONE DEFAULT CURRENT_TIMESTAMP,
        UNIQUE (user_id, tenant_id)
);

COMMENT ON TABLE public.sys_user_tenant IS '用户-组织关联表（多对多）';

COMMENT ON COLUMN public.sys_user_tenant.user_id IS '用户ID';

COMMENT ON COLUMN public.sys_user_tenant.tenant_id IS '组织ID';

-- 索引
CREATE INDEX IF NOT EXISTS idx_public_sys_user_username ON public.sys_user (username);

CREATE INDEX IF NOT EXISTS idx_public_sys_user_tenant_id ON public.sys_user (tenant_id);

CREATE INDEX IF NOT EXISTS idx_public_sys_menu_parent_id ON public.sys_menu (parent_id);

CREATE INDEX IF NOT EXISTS idx_public_sys_user_register_status ON public.sys_user_register (status);

CREATE INDEX IF NOT EXISTS idx_public_sys_user_tenant_user_id ON public.sys_user_tenant (user_id);

CREATE INDEX IF NOT EXISTS idx_public_sys_user_tenant_tenant_id ON public.sys_user_tenant (tenant_id);