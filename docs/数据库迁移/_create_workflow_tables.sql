-- ===================================================================
-- AI 工作流数据表
-- 基于 docs/知识库模块/AI工作流编排器设计方案.md 第4节
-- ===================================================================

-- 1. workflow 工作流模板表
CREATE TABLE IF NOT EXISTS {schema}.workflow (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    version VARCHAR(20) DEFAULT '1.0.0',
    definition JSONB NOT NULL,
    node_types TEXT[] DEFAULT '{}',
    status VARCHAR(20) DEFAULT 'draft',
    use_count INT DEFAULT 0,
    last_executed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.workflow IS 'AI 工作流模板定义';

COMMENT ON COLUMN {schema}.workflow.definition IS '完整工作流定义 JSON';

COMMENT ON COLUMN {schema}.workflow.node_types IS '节点类型数组';

COMMENT ON COLUMN {schema}.workflow.status IS 'draft/published/archived';

CREATE INDEX IF NOT EXISTS idx_wf_user ON {schema}.workflow(user_id, deleted);

CREATE INDEX IF NOT EXISTS idx_wf_status ON {schema}.workflow(status, deleted);

CREATE INDEX IF NOT EXISTS idx_wf_node_types ON {schema}.workflow USING GIN(node_types);

CREATE INDEX IF NOT EXISTS idx_wf_time ON {schema}.workflow(created_at DESC);

-- 2. workflow_execution 执行记录表
CREATE TABLE IF NOT EXISTS {schema}.workflow_execution (
    id BIGSERIAL PRIMARY KEY,
    workflow_id BIGINT NOT NULL REFERENCES {schema}.workflow(id) ON DELETE CASCADE,
    user_id BIGINT NOT NULL,
    trigger_type VARCHAR(30) DEFAULT 'manual',
    input_data JSONB,
    result_data JSONB,
    error_message TEXT,
    node_results JSONB,
    status VARCHAR(20) DEFAULT 'running',
    total_duration_ms INT,
    total_tokens INT,
    total_cost DECIMAL(12,6),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ,
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.workflow_execution IS 'AI 工作流执行记录';

COMMENT ON COLUMN {schema}.workflow_execution.node_results IS '每个节点的执行详情';

COMMENT ON COLUMN {schema}.workflow_execution.status IS 'running/success/failed/cancelled';

CREATE INDEX IF NOT EXISTS idx_wfe_workflow ON {schema}.workflow_execution(workflow_id, deleted);

CREATE INDEX IF NOT EXISTS idx_wfe_user ON {schema}.workflow_execution(user_id, deleted);

CREATE INDEX IF NOT EXISTS idx_wfe_status ON {schema}.workflow_execution(status, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_wfe_time ON {schema}.workflow_execution(created_at DESC);