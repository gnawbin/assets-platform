-- ==============================
-- 工作流引擎迁移（bpmninja + adk-graph）
-- 幂等脚本：新租户建表后执行，已有租户启动时重复执行。
-- ==============================

-- 1. workflow 表补充 BPMN 与引擎部署字段
ALTER TABLE {schema}.workflow ADD COLUMN IF NOT EXISTS bpmn_xml TEXT;

ALTER TABLE {schema}.workflow ADD COLUMN IF NOT EXISTS engine_definition_key UUID;

ALTER TABLE {schema}.workflow ADD COLUMN IF NOT EXISTS engine_version INT;

ALTER TABLE {schema}.workflow ADD COLUMN IF NOT EXISTS definition_hash TEXT;

COMMENT ON COLUMN {schema}.workflow.bpmn_xml IS '标准 BPMN 2.0 XML，编辑器与外部 BPMN 工具的无损交换格式';

COMMENT ON COLUMN {schema}.workflow.engine_definition_key IS '引擎内流程定义 key，与 definition_hash 一起用于复用部署版本';

COMMENT ON COLUMN {schema}.workflow.definition_hash IS 'BPMN + IR 的定义指纹，变化时才重新部署';

-- 2. workflow_execution 补充引擎实例与审批字段
ALTER TABLE {schema}.workflow_execution ADD COLUMN IF NOT EXISTS engine_instance_id UUID;

ALTER TABLE {schema}.workflow_execution ADD COLUMN IF NOT EXISTS engine_definition_key UUID;

ALTER TABLE {schema}.workflow_execution ADD COLUMN IF NOT EXISTS current_node_id VARCHAR(255);

ALTER TABLE {schema}.workflow_execution ADD COLUMN IF NOT EXISTS agent_thread_id TEXT;

ALTER TABLE {schema}.workflow_execution ADD COLUMN IF NOT EXISTS waiting_task_id UUID;

ALTER TABLE {schema}.workflow_execution ADD COLUMN IF NOT EXISTS interrupt_payload JSONB;

ALTER TABLE {schema}.workflow_execution ADD COLUMN IF NOT EXISTS bpmn_status TEXT;

ALTER TABLE {schema}.workflow_execution ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

COMMENT ON COLUMN {schema}.workflow_execution.bpmn_status IS '引擎原始状态，保留 waiting_user_task 等细节';

COMMENT ON COLUMN {schema}.workflow_execution.interrupt_payload IS 'Agent 中断负载，审批弹窗展示待确认的工具与参数';

CREATE INDEX IF NOT EXISTS idx_wfe_instance ON {schema}.workflow_execution(engine_instance_id);

CREATE INDEX IF NOT EXISTS idx_wfe_waiting_task ON {schema}.workflow_execution(waiting_task_id);

-- 3. bpmninja 持久化表（引擎对象统一存 JSONB，按索引列过滤）
CREATE TABLE IF NOT EXISTS {schema}.bpmn_engine_object (
    kind TEXT NOT NULL,
    id UUID NOT NULL,
    instance_id UUID,
    definition_key UUID,
    business_key TEXT,
    topic TEXT,
    worker_id TEXT,
    lock_expiration TIMESTAMPTZ,
    data JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (kind, id)
);

CREATE INDEX IF NOT EXISTS idx_bpmn_obj_instance ON {schema}.bpmn_engine_object(instance_id);

CREATE INDEX IF NOT EXISTS idx_bpmn_obj_topic ON {schema}.bpmn_engine_object(kind, topic);

CREATE INDEX IF NOT EXISTS idx_bpmn_obj_lock ON {schema}.bpmn_engine_object(lock_expiration);

CREATE TABLE IF NOT EXISTS {schema}.bpmn_engine_history (
    id UUID PRIMARY KEY,
    instance_id UUID NOT NULL,
    event_type TEXT,
    node_id TEXT,
    timestamp TIMESTAMPTZ NOT NULL,
    data JSONB NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_bpmn_history_instance ON {schema}.bpmn_engine_history(instance_id, timestamp);

CREATE TABLE IF NOT EXISTS {schema}.bpmn_engine_file (
    object_key TEXT PRIMARY KEY,
    data BYTEA NOT NULL
);

CREATE TABLE IF NOT EXISTS {schema}.bpmn_engine_completed (
    instance_id UUID PRIMARY KEY,
    data JSONB NOT NULL
);

-- 4. Agent checkpoint（adk-graph Checkpointer）与运行记录
CREATE TABLE IF NOT EXISTS {schema}.workflow_agent_checkpoint (
    thread_id TEXT NOT NULL,
    checkpoint_id TEXT NOT NULL,
    data JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (thread_id, checkpoint_id)
);

COMMENT ON TABLE {schema}.workflow_agent_checkpoint IS 'adk-graph Agent 线程快照，支持进程重启后恢复中断';

CREATE INDEX IF NOT EXISTS idx_agent_checkpoint_thread ON {schema}.workflow_agent_checkpoint(thread_id, created_at DESC);

CREATE TABLE IF NOT EXISTS {schema}.workflow_agent_run (
    id BIGSERIAL PRIMARY KEY,
    execution_id BIGINT NOT NULL,
    node_id TEXT NOT NULL,
    engine_task_id UUID,
    thread_id TEXT NOT NULL,
    checkpoint_id TEXT,
    status TEXT NOT NULL,
    config JSONB,
    state JSONB,
    interrupt JSONB,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ
);

COMMENT ON TABLE {schema}.workflow_agent_run IS 'Agent 单次运行记录，engine_task_id 唯一用于 worker 幂等';

CREATE UNIQUE INDEX IF NOT EXISTS uq_agent_run_task ON {schema}.workflow_agent_run(engine_task_id);

CREATE INDEX IF NOT EXISTS idx_agent_run_execution ON {schema}.workflow_agent_run(execution_id, started_at DESC);

CREATE INDEX IF NOT EXISTS idx_agent_run_thread ON {schema}.workflow_agent_run(thread_id);
