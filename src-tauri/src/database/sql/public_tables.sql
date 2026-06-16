-- ==============================
-- public schema 公共表
-- ==============================

-- 1. 租户配置表
CREATE TABLE IF NOT EXISTS public.sys_tenant (
    id BIGINT PRIMARY KEY,
    tenant_name VARCHAR(100) NOT NULL,
    schema_name VARCHAR(50) UNIQUE NOT NULL,
    enable BOOLEAN DEFAULT true,
    create_at TIMESTAMP
    WITH
        TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

COMMENT ON TABLE public.sys_tenant IS '租户配置表';

COMMENT ON COLUMN public.sys_tenant.tenant_name IS '租户名称';

COMMENT ON COLUMN public.sys_tenant.schema_name IS '对应 PostgreSQL schema 名，如 factory_a';

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
    status SMALLINT NOT NULL DEFAULT 1,
    nickname VARCHAR(255),
    avatar VARCHAR(500),
    person_id VARCHAR(50),
    person_code VARCHAR(50),
    super_user_id BIGINT,
    tenant_id BIGINT NOT NULL REFERENCES public.sys_tenant (id),
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

COMMENT ON COLUMN public.sys_user.tenant_id IS '所属租户ID';

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

-- 索引
CREATE INDEX IF NOT EXISTS idx_public_sys_user_username ON public.sys_user (username);

CREATE INDEX IF NOT EXISTS idx_public_sys_user_tenant_id ON public.sys_user (tenant_id);

CREATE INDEX IF NOT EXISTS idx_public_sys_menu_parent_id ON public.sys_menu (parent_id);