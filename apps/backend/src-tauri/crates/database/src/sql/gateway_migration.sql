-- ============================================================================
-- 网关多实例协调表（设计文档附录 D.2：PostgreSQL 单一依赖模式，不引入 Redis）
--
-- 四张表 + 五个索引全部 IF NOT EXISTS：全新安装与老库升级走同一条路径。
-- 注意：`execute_sql_content` 在单个事务内执行本文件，因此这里不能出现
-- `CREATE INDEX CONCURRENTLY`（事务内不允许）。
-- ============================================================================

-- ① worker id 租约：保证同一时刻只有一个实例使用某个 SNOWFLAKE_WORKER_ID
CREATE TABLE IF NOT EXISTS public.sys_gateway_instance (
    worker_id   SMALLINT PRIMARY KEY CHECK (worker_id BETWEEN 0 AND 31),
    instance_id TEXT NOT NULL,
    expires_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ② SSE 一次性票据：EventSource 不能带 Header，建流时用 query 传票
CREATE TABLE IF NOT EXISTS public.sys_gateway_sse_ticket (
    token      TEXT PRIMARY KEY,
    tenant_id  BIGINT,
    principal  JSONB NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_gw_sse_ticket_exp ON public.sys_gateway_sse_ticket (expires_at);

-- ③ SSE 事件日志：BIGSERIAL 全局单调，天然就是 Last-Event-ID 游标
CREATE TABLE IF NOT EXISTS public.sys_gateway_event (
    id         BIGSERIAL PRIMARY KEY,
    tenant_id  BIGINT NOT NULL,
    kind       VARCHAR(32) NOT NULL,
    payload    JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_gw_event_tenant_id ON public.sys_gateway_event (tenant_id, id);
CREATE INDEX IF NOT EXISTS idx_gw_event_created  ON public.sys_gateway_event (created_at);

-- ④ 共享限流计数：固定窗口，单语句 UPSERT 原子累加（跨实例总额准确）
CREATE TABLE IF NOT EXISTS public.sys_gateway_rate_limit (
    bucket_key   TEXT NOT NULL,
    window_start TIMESTAMPTZ NOT NULL,
    counter      INTEGER NOT NULL,
    PRIMARY KEY (bucket_key, window_start)
);

CREATE INDEX IF NOT EXISTS idx_gw_rate_limit_window ON public.sys_gateway_rate_limit (window_start);
