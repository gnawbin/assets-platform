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
    -- string / int / float / bool / json
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
    suffix VARCHAR(20) NOT NULL UNIQUE, -- pdf / docx / xlsx / mp3 / mp4
    parse_type VARCHAR(30) NOT NULL, -- text / image / audio / video
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
    task_status SMALLINT NOT NULL DEFAULT 1, -- 0=停用 1=启用
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
    status VARCHAR(20) NOT NULL, -- uploading / completed / failed / cancelled
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
    oper_module VARCHAR(50), -- knowledge / upload / config
    oper_type VARCHAR(30), -- create / update / delete / upload / parse
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
    error_type VARCHAR(50), -- parse_error / vector_error / llm_error / asr_error
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
-- 二、{schema} 租户级表
-- 执行前将 {schema} 替换为实际 schema 名
-- ==============================

-- =====================
-- 2. llm_provider 大模型服务商配置（多租户）
-- =====================
CREATE TABLE IF NOT EXISTS {schema}.llm_provider (
    id BIGSERIAL PRIMARY KEY,
    provider_code VARCHAR(50) NOT NULL UNIQUE,
    -- openai / claude / qwen / volcengine / tencent / ollama
    provider_name VARCHAR(100) NOT NULL,
    base_url VARCHAR(1024),
    api_key TEXT, -- AES-256-GCM 加密存储
    secret_key TEXT, -- AES-256-GCM 加密存储
    extra_config JSONB, -- {region, project_id, endpoint_id}
    weight INT NOT NULL DEFAULT 10, -- 负载均衡权重
    is_local BOOLEAN NOT NULL DEFAULT false,
    enable BOOLEAN NOT NULL DEFAULT true,
    created_by BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.llm_provider IS '大模型服务商配置（多租户）';

COMMENT ON COLUMN {schema}.llm_provider.api_key IS 'AES-256-GCM 加密存储，前端永不返回明文';

COMMENT ON COLUMN {schema}.llm_provider.weight IS '负载均衡权重，越高优先被选择';

CREATE INDEX IF NOT EXISTS idx_llm_provider_code ON {schema}.llm_provider (provider_code, deleted);

CREATE INDEX IF NOT EXISTS idx_llm_provider_enable ON {schema}.llm_provider (enable, deleted);

-- =====================
-- 3. llm_model 模型明细表（多租户）
-- =====================
CREATE TABLE IF NOT EXISTS {schema}.llm_model (
    id BIGSERIAL PRIMARY KEY,
    provider_id BIGINT NOT NULL REFERENCES {schema}.llm_provider (id) ON DELETE CASCADE,
    model_code VARCHAR(100) NOT NULL,
    model_name VARCHAR(100) NOT NULL,
    model_type VARCHAR(30) NOT NULL, -- chat / embedding / asr / tts
    context_window INT,
    temperature_default FLOAT DEFAULT 0.7,
    max_tokens_default INT DEFAULT 2048,
    price_input NUMERIC(10, 6) DEFAULT 0,
    price_output NUMERIC(10, 6) DEFAULT 0,
    enable BOOLEAN NOT NULL DEFAULT true,
    created_by BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0,
    UNIQUE (provider_id, model_code)
);

COMMENT ON TABLE {schema}.llm_model IS '模型明细表（多租户）';

COMMENT ON COLUMN {schema}.llm_model.model_type IS 'chat=对话 embedding=向量 asr=语音识别 tts=语音合成';

COMMENT ON COLUMN {schema}.llm_model.price_input IS '输入价格（每1K tokens，单位：元）';

COMMENT ON COLUMN {schema}.llm_model.price_output IS '输出价格（每1K tokens，单位：元）';

CREATE INDEX IF NOT EXISTS idx_llm_model_provider ON {schema}.llm_model (provider_id, deleted);

CREATE INDEX IF NOT EXISTS idx_llm_model_type ON {schema}.llm_model (model_type, enable, deleted);

-- =====================
-- 4. user_llm_setting 用户模型偏好（多租户）
-- =====================
CREATE TABLE IF NOT EXISTS {schema}.user_llm_setting (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL UNIQUE,
    default_provider_id BIGINT REFERENCES {schema}.llm_provider (id),
    default_chat_model_id BIGINT REFERENCES {schema}.llm_model (id),
    default_embed_model_id BIGINT REFERENCES {schema}.llm_model (id),
    custom_temp FLOAT,
    custom_max_token INT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.user_llm_setting IS '用户模型偏好配置（多租户）';

COMMENT ON COLUMN {schema}.user_llm_setting.custom_temp IS '用户自定义温度，覆盖模型默认值';

COMMENT ON COLUMN {schema}.user_llm_setting.custom_max_token IS '用户自定义最大输出Token';

CREATE INDEX IF NOT EXISTS idx_user_llm_uid ON {schema}.user_llm_setting (user_id, deleted);

-- =====================
-- 5. llm_call_record LLM调用用量日志（多租户）
-- =====================
CREATE TABLE IF NOT EXISTS {schema}.llm_call_record (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT,
    conv_id BIGINT,
    provider_id BIGINT NOT NULL,
    model_id BIGINT NOT NULL,
    call_type VARCHAR(30) NOT NULL, -- chat / embedding / asr / tts
    input_tokens INT NOT NULL DEFAULT 0,
    output_tokens INT NOT NULL DEFAULT 0,
    total_cost NUMERIC(10, 6) DEFAULT 0,
    duration_ms INT DEFAULT 0,
    status VARCHAR(20) NOT NULL, -- success / fail
    error_msg TEXT,
    request_id VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE {schema}.llm_call_record IS 'LLM 调用全链路日志（多租户）';

COMMENT ON COLUMN {schema}.llm_call_record.total_cost IS '费用=price_input*(输入tokens/1000) + price_output*(输出tokens/1000)';

COMMENT ON COLUMN {schema}.llm_call_record.duration_ms IS '调用耗时，用于性能监控';

CREATE INDEX IF NOT EXISTS idx_llm_call_user ON {schema}.llm_call_record (user_id);

CREATE INDEX IF NOT EXISTS idx_llm_call_conv ON {schema}.llm_call_record (conv_id);

CREATE INDEX IF NOT EXISTS idx_llm_call_time ON {schema}.llm_call_record (created_at);

CREATE INDEX IF NOT EXISTS idx_llm_call_status ON {schema}.llm_call_record (status);

-- =====================
-- 13. document_chunk 向量分片检索表
-- =====================
CREATE TABLE IF NOT EXISTS {schema}.document_chunk (
    id BIGSERIAL PRIMARY KEY,
    asset_id BIGINT NOT NULL REFERENCES {schema}.knowledge_asset(id) ON DELETE CASCADE,
    chunk_index INT NOT NULL,
    chunk_text TEXT NOT NULL,
    token_count INT,
    embedding vector(1536),              -- pgvector 向量
    title VARCHAR(512),                  -- 来源资产标题（冗余）
    okf_type VARCHAR(30),                -- 来源OKF类型（冗余）
    tags TEXT[],                         -- 来源标签（冗余）
    tree_node_id BIGINT,                 -- 来源目录ID（冗余）
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.document_chunk IS 'RAG向量分片检索表';

COMMENT ON COLUMN {schema}.document_chunk.embedding IS '1536维pgvector向量，HNSW索引加速';

COMMENT ON COLUMN {schema}.document_chunk.title IS '来源资产标题（冗余，避免每次关联查询）';

COMMENT ON COLUMN {schema}.document_chunk.tree_node_id IS '来源目录ID（冗余，用于限定目录检索）';

CREATE INDEX IF NOT EXISTS idx_chunk_asset ON {schema}.document_chunk(asset_id, deleted);

CREATE INDEX IF NOT EXISTS idx_chunk_tree ON {schema}.document_chunk(tree_node_id, deleted);

-- pgvector HNSW 索引（需要先安装 pgvector 扩展）
-- CREATE INDEX IF NOT EXISTS idx_chunk_embedding ON {schema}.document_chunk USING hnsw (embedding vector_cosine_ops);

-- =====================
-- 14. conversation 对话会话表
-- =====================
CREATE TABLE IF NOT EXISTS {schema}.conversation (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    title VARCHAR(255),
    bind_knowledge_tree_id BIGINT REFERENCES {schema}.knowledge_tree(id),
        -- NULL = 全局检索，非NULL = 限定到该目录及子目录
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.conversation IS '多轮对话会话';

COMMENT ON COLUMN {schema}.conversation.title IS '首次提问截取前30字，用户可重命名';

COMMENT ON COLUMN {schema}.conversation.bind_knowledge_tree_id IS '绑定知识树目录ID，NULL=全部知识库，非NULL=仅检索该目录';

CREATE INDEX IF NOT EXISTS idx_conv_user ON {schema}.conversation(user_id, deleted);

CREATE INDEX IF NOT EXISTS idx_conv_tree ON {schema}.conversation(bind_knowledge_tree_id, deleted);

CREATE INDEX IF NOT EXISTS idx_conv_time ON {schema}.conversation(created_at DESC);

-- =====================
-- 15. message 会话消息表
-- =====================
CREATE TABLE IF NOT EXISTS {schema}.message (
    id BIGSERIAL PRIMARY KEY,
    conv_id BIGINT NOT NULL REFERENCES {schema}.conversation(id) ON DELETE CASCADE,
    role VARCHAR(20) NOT NULL,
        -- user / assistant / system
    content TEXT NOT NULL,
    audio_url VARCHAR(1024),             -- 语音消息 RustFS 地址
    reference_asset_ids BIGINT[],        -- 引用的 knowledge_asset.id 数组
    reference_text VARCHAR(2048),        -- 引用原文快照
    metadata JSONB,                      -- {model, provider, temperature, duration_ms}
    input_tokens INT DEFAULT 0,
    output_tokens INT DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.message IS '会话消息记录';

COMMENT ON COLUMN {schema}.message.role IS '消息角色：user=用户 assistant=AI system=系统提示词';

COMMENT ON COLUMN {schema}.message.reference_asset_ids IS '本次回答引用的 knowledge_asset.id 数组，前端可点击跳转';

COMMENT ON COLUMN {schema}.message.reference_text IS '引用原文快照，返回相关文档片段的原文';

COMMENT ON COLUMN {schema}.message.input_tokens IS '本次请求消耗的输入Token数';

COMMENT ON COLUMN {schema}.message.output_tokens IS '本次回复消耗的输出Token数';

CREATE INDEX IF NOT EXISTS idx_msg_conv ON {schema}.message(conv_id, deleted);

CREATE INDEX IF NOT EXISTS idx_msg_conv_time ON {schema}.message(conv_id, created_at ASC);

-- =====================
-- 16. memory 用户长期记忆表
-- =====================
CREATE TABLE IF NOT EXISTS {schema}.memory (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    content TEXT NOT NULL,
    category VARCHAR(50),
    importance FLOAT DEFAULT 0.5,
    source_conv_id BIGINT REFERENCES {schema}.conversation(id),
    next_review_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.memory IS '用户长期记忆，遗忘曲线间隔重复复习';

COMMENT ON COLUMN {schema}.memory.next_review_at IS '下次复习时间，遗忘曲线调度';

CREATE INDEX IF NOT EXISTS idx_memory_user_review ON {schema}.memory(user_id, next_review_at, deleted);

-- =====================
-- 17. skill_execution 技能执行日志表
-- =====================
CREATE TABLE IF NOT EXISTS {schema}.skill_execution (
    id BIGSERIAL PRIMARY KEY,
    asset_id BIGINT REFERENCES {schema}.knowledge_asset(id),
    trigger_type VARCHAR(30),
        -- chat_voice / chat_text / manual
    input_params JSONB,
    output_result JSONB,
    status VARCHAR(20) NOT NULL,
        -- running / success / fail
    error_msg TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE {schema}.skill_execution IS 'Skill规则/流程执行日志';

CREATE INDEX IF NOT EXISTS idx_skill_asset ON {schema}.skill_execution(asset_id);

CREATE INDEX IF NOT EXISTS idx_skill_status ON {schema}.skill_execution(status);

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
    
    -- 找第一个启用的 chat provider（在对应租户 schema 中查询）
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

-- ==============================
-- 四、知识库种子数据（{schema} 租户级）
-- ==============================

-- 内置 LLM 厂商种子（每个租户 schema 独立）
INSERT INTO
    {schema}.llm_provider (
        provider_code,
        provider_name,
        base_url,
        weight,
        is_local
    )
SELECT 'openai', 'OpenAI', 'https://api.openai.com', 10, false
WHERE
    NOT EXISTS (
        SELECT 1
        FROM {schema}.llm_provider
        WHERE
            provider_code = 'openai'
    );

INSERT INTO
    {schema}.llm_provider (
        provider_code,
        provider_name,
        base_url,
        weight,
        is_local
    )
SELECT 'claude', 'Anthropic Claude', 'https://api.anthropic.com', 8, false
WHERE
    NOT EXISTS (
        SELECT 1
        FROM {schema}.llm_provider
        WHERE
            provider_code = 'claude'
    );

INSERT INTO
    {schema}.llm_provider (
        provider_code,
        provider_name,
        base_url,
        weight,
        is_local
    )
SELECT 'qwen', '通义千问', 'https://dashscope.aliyuncs.com', 5, false
WHERE
    NOT EXISTS (
        SELECT 1
        FROM {schema}.llm_provider
        WHERE
            provider_code = 'qwen'
    );

INSERT INTO
    {schema}.llm_provider (
        provider_code,
        provider_name,
        base_url,
        weight,
        is_local
    )
SELECT 'volcengine', '火山引擎', 'https://ark.cn-beijing.volces.com', 3, false
WHERE
    NOT EXISTS (
        SELECT 1
        FROM {schema}.llm_provider
        WHERE
            provider_code = 'volcengine'
    );

INSERT INTO
    {schema}.llm_provider (
        provider_code,
        provider_name,
        base_url,
        weight,
        is_local
    )
SELECT 'tencent', '腾讯混元', 'https://api.hunyuan.cloud.tencent.com', 3, false
WHERE
    NOT EXISTS (
        SELECT 1
        FROM {schema}.llm_provider
        WHERE
            provider_code = 'tencent'
    );

INSERT INTO
    {schema}.llm_provider (
        provider_code,
        provider_name,
        base_url,
        weight,
        is_local
    )
SELECT 'ollama', 'Ollama 本地', 'http://localhost:11434', 3, true
WHERE
    NOT EXISTS (
        SELECT 1
        FROM {schema}.llm_provider
        WHERE
            provider_code = 'ollama'
    );

-- OpenAI 内置模型（每个租户 schema 独立）
INSERT INTO
    {schema}.llm_model (
        provider_id,
        model_code,
        model_name,
        model_type,
        context_window,
        price_input,
        price_output
    )
SELECT p.id, 'gpt-4o', 'GPT-4o', 'chat', 128000, 0.0025, 0.01
FROM {schema}.llm_provider p
WHERE
    p.provider_code = 'openai'
    AND NOT EXISTS (
        SELECT 1
        FROM {schema}.llm_model
        WHERE
            provider_id = p.id
            AND model_code = 'gpt-4o'
    );

INSERT INTO
    {schema}.llm_model (
        provider_id,
        model_code,
        model_name,
        model_type,
        context_window,
        price_input,
        price_output
    )
SELECT p.id, 'gpt-4o-mini', 'GPT-4o Mini', 'chat', 128000, 0.00015, 0.0006
FROM {schema}.llm_provider p
WHERE
    p.provider_code = 'openai'
    AND NOT EXISTS (
        SELECT 1
        FROM {schema}.llm_model
        WHERE
            provider_id = p.id
            AND model_code = 'gpt-4o-mini'
    );

INSERT INTO
    {schema}.llm_model (
        provider_id,
        model_code,
        model_name,
        model_type,
        price_input,
        price_output
    )
SELECT p.id, 'text-embedding-3-small', 'Text Embedding 3 Small', 'embedding', 0.00002, 0
FROM {schema}.llm_provider p
WHERE
    p.provider_code = 'openai'
    AND NOT EXISTS (
        SELECT 1
        FROM {schema}.llm_model
        WHERE
            provider_id = p.id
            AND model_code = 'text-embedding-3-small'
    );