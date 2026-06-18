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

COMMENT ON COLUMN public.sys_tenant.parent_id IS '父租户ID';

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

COMMENT ON COLUMN public.sys_role.tenant_id IS '所属租户ID（超级管理员角色为空）';

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