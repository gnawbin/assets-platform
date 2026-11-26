-- ==============================
-- 异步任务队列（apalis）业务进度表
-- 幂等脚本：新租户建表后执行，存量租户启动时重复执行。
--
-- 设计要点（docs/apalis异步任务队列开发文档.md §7 / §14.3）：
--   1. `apalis.jobs` 是全局基础设施表（跨租户），不放进租户 schema；
--      `task_ticket` 是**业务进度表**，按租户隔离，前端只查它。
--   2. id 由生产者生成（UUID），同时作为 `apalis` 的任务 id，二者一一对应。
--   3. 本表只存业务关心的字段：进度、阶段、错误摘要；不存文件内容 / 凭证 / 绝对路径。
-- ==============================

CREATE TABLE IF NOT EXISTS {schema}.task_ticket (
    id VARCHAR(64) PRIMARY KEY,
    task_type VARCHAR(32) NOT NULL,
    queue VARCHAR(64) NOT NULL,
    asset_id BIGINT,
    tenant_schema VARCHAR(64) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'pending',
    stage VARCHAR(32),
    progress SMALLINT NOT NULL DEFAULT 0,
    attempts INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    trace_id VARCHAR(64),
    dedupe_key VARCHAR(160),
    created_by BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE {schema}.task_ticket IS '异步任务进度票据：前端轮询 / SSE 进度都读它，与 apalis 任务 id 一一对应';
COMMENT ON COLUMN {schema}.task_ticket.id IS '任务 id（UUID，由生产者生成，同时作为 apalis 任务 id）';
COMMENT ON COLUMN {schema}.task_ticket.task_type IS '任务类型：parse / vectorize';
COMMENT ON COLUMN {schema}.task_ticket.queue IS 'apalis 队列名：parse / parse-video / vectorize';
COMMENT ON COLUMN {schema}.task_ticket.asset_id IS '软引用 knowledge_asset.id（不建外键，避免软删阻塞）';
COMMENT ON COLUMN {schema}.task_ticket.tenant_schema IS '冗余的租户 schema 名，便于跨表排查（与所在 schema 一致）';
COMMENT ON COLUMN {schema}.task_ticket.status IS '状态：pending / running / completed / failed';
COMMENT ON COLUMN {schema}.task_ticket.stage IS '阶段：queued / downloading / parsing / chunking / done（仅展示用）';
COMMENT ON COLUMN {schema}.task_ticket.progress IS '进度百分比 0~100（粗粒度，按阶段推进）';
COMMENT ON COLUMN {schema}.task_ticket.attempts IS '已尝试次数（worker 侧自增，便于判断是否在重试）';
COMMENT ON COLUMN {schema}.task_ticket.error IS '最近一次失败原因（截断到 4KB，不含凭证 / 绝对路径）';
COMMENT ON COLUMN {schema}.task_ticket.dedupe_key IS '幂等键（与 apalis 任务的 idempotency_key 相同）';

CREATE INDEX IF NOT EXISTS idx_task_ticket_status
    ON {schema}.task_ticket(status, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_task_ticket_asset
    ON {schema}.task_ticket(asset_id);

CREATE INDEX IF NOT EXISTS idx_task_ticket_dedupe
    ON {schema}.task_ticket(dedupe_key) WHERE dedupe_key IS NOT NULL;
