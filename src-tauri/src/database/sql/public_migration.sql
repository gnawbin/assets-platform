-- ==============================
-- public schema 迁移脚本（补充缺失列）
-- 用于已有旧表的数据库，新部署的库通过 public_tables.sql 直接建表
-- ==============================

-- sys_user 表补充缺失列
ALTER TABLE public.sys_user
ADD COLUMN IF NOT EXISTS email VARCHAR(255);

ALTER TABLE public.sys_user
ADD COLUMN IF NOT EXISTS phone VARCHAR(50);

ALTER TABLE public.sys_user
ADD COLUMN IF NOT EXISTS department_id BIGINT;

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