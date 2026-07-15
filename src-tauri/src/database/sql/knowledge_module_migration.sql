-- ==============================
-- 知识库模块 新增表 DDL
-- ==============================
-- 适用版本：基于 Git commit abdfd18
-- 执行位置：
--   public 表执行在 public 命名空间
--   {schema} 表需替换为实际租户 schema 名
-- ==============================

-- ==============================
-- 一、public 全局表
-- ==============================

-- =====================
-- 1. sys_user_profile 用户个性化配置
-- =====================
CREATE TABLE IF NOT EXISTS public.sys_user_profile (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL UNIQUE,
    theme VARCHAR(20) DEFAULT 'light',
    language VARCHAR(20) DEFAULT 'zh-CN',
    upload_max_size BIGINT DEFAULT 104857600,
    editor_default_mode VARCHAR(20) DEFAULT 'wysiwyg',
    auto_summary BOOLEAN DEFAULT true,
    auto_vectorize BOOLEAN DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE public.sys_user_profile IS '用户个性化配置：界面、上传、编辑器偏好';

COMMENT ON COLUMN public.sys_user_profile.upload_max_size IS '上传文件大小上限（字节），默认100MB';

COMMENT ON COLUMN public.sys_user_profile.auto_summary IS '上传文件后是否自动生成摘要';

COMMENT ON COLUMN public.sys_user_profile.auto_vectorize IS '上传文件后是否自动向量化';

-- =====================
-- 6. sys_system_config 平台全局参数
-- =====================
CREATE TABLE IF NOT EXISTS public.sys_system_config (
    id BIGSERIAL PRIMARY KEY,
    config_key VARCHAR(100) NOT NULL UNIQUE,
    config_value TEXT,
    config_desc VARCHAR(255),
    config_type VARCHAR(20) NOT NULL DEFAULT 'string',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON
TABLE public.sys_system_config IS '平台全局参数：RustFS地址/向量维度/文件白名单等';

-- =====================
-- 7. sys_file_type_parse 文件解析规则
-- =====================
CREATE TABLE IF NOT EXISTS public.sys_file_type_parse (
    id BIGSERIAL PRIMARY KEY,
    suffix VARCHAR(20) NOT NULL UNIQUE,
    parse_type VARCHAR(30) NOT NULL,
    enable_ocr BOOLEAN DEFAULT false,
    enable_asr BOOLEAN DEFAULT false,
    parse_order INT DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- =====================
-- 8. sys_scheduled_task 定时任务配置
-- =====================
CREATE TABLE IF NOT EXISTS public.sys_scheduled_task (
    id BIGSERIAL PRIMARY KEY,
    task_key VARCHAR(50) NOT NULL UNIQUE,
    task_name VARCHAR(100),
    cron_expr VARCHAR(50),
    task_handler VARCHAR(100),
    task_params JSONB,
    task_status SMALLINT NOT NULL DEFAULT 1,
    last_exec_at TIMESTAMPTZ,
    next_exec_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- =====================
-- 9. sys_upload_task 分片上传任务表
-- =====================
CREATE TABLE IF NOT EXISTS public.sys_upload_task (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    task_id VARCHAR(100) NOT NULL UNIQUE,
    file_name VARCHAR(512) NOT NULL,
    file_mime VARCHAR(100),
    total_size BIGINT,
    total_chunk INT,
    finished_chunk INT DEFAULT 0,
    file_md5 VARCHAR(64),
    status VARCHAR(20) NOT NULL,
    file_url VARCHAR(1024),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON
TABLE public.sys_upload_task IS '分片上传任务记录（替代旧 file_uploads 表）';

CREATE INDEX IF NOT EXISTS idx_upload_task_uid ON public.sys_upload_task (user_id, deleted);

CREATE INDEX IF NOT EXISTS idx_upload_task_taskid ON public.sys_upload_task (task_id);

-- =====================
-- 10. sys_oper_log 用户操作日志
-- =====================
CREATE TABLE IF NOT EXISTS public.sys_oper_log (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT,
    oper_module VARCHAR(50),
    oper_type VARCHAR(30),
    oper_content TEXT,
    target_id BIGINT,
    ip VARCHAR(100),
    client_info TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_oper_log_uid ON public.sys_oper_log (user_id);

CREATE INDEX IF NOT EXISTS idx_oper_log_time ON public.sys_oper_log (created_at);

-- =====================
-- 11. sys_error_log 系统异常日志
-- =====================
CREATE TABLE IF NOT EXISTS public.sys_error_log (
    id BIGSERIAL PRIMARY KEY,
    error_type VARCHAR(50),
    error_msg TEXT,
    stack TEXT,
    related_id BIGINT,
    user_id BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- =====================
-- 12. sys_tag 全局公共标签表
-- =====================
CREATE TABLE IF NOT EXISTS public.sys_tag (
    id BIGSERIAL PRIMARY KEY,
    tag_name VARCHAR(50) NOT NULL UNIQUE,
    tag_color VARCHAR(20),
    category VARCHAR(50),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

-- ==============================
-- 三、用户注册自动初始化触发器（public 全局）
-- ==============================

-- 新用户注册时自动创建 user_llm_setting
CREATE OR REPLACE FUNCTION public.init_user_llm_setting(schema_name TEXT)
RETURNS TRIGGER AS $$
DECLARE
    v_provider_id BIGINT;
    v_chat_model_id BIGINT;
    v_embed_model_id BIGINT;
    v_schema TEXT;
BEGIN
    v_schema := quote_ident(schema_name);

    EXECUTE format('SELECT id FROM %I.llm_provider WHERE enable = true AND deleted = 0 ORDER BY weight DESC LIMIT 1', v_schema)
        INTO v_provider_id;

    IF v_provider_id IS NOT NULL THEN
        EXECUTE format('SELECT id FROM %I.llm_model WHERE provider_id = $1 AND model_type = ''chat'' AND enable = true ORDER BY id LIMIT 1', v_schema)
            INTO v_chat_model_id
            USING v_provider_id;

        EXECUTE format('SELECT id FROM %I.llm_model WHERE model_type = ''embedding'' AND enable = true ORDER BY id LIMIT 1', v_schema)
            INTO v_embed_model_id;
    END IF;

    EXECUTE format('INSERT INTO %I.user_llm_setting (user_id, default_provider_id, default_chat_model_id, default_embed_model_id) VALUES ($1, $2, $3, $4) ON CONFLICT (user_id) DO NOTHING', v_schema)
        USING NEW.id, v_provider_id, v_chat_model_id, v_embed_model_id;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- 新用户注册时自动创建 sys_user_profile
CREATE OR REPLACE FUNCTION public.init_user_profile()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO public.sys_user_profile (user_id)
    VALUES (NEW.id)
    ON CONFLICT (user_id) DO NOTHING;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;