-- ==============================
-- 运维采集模块（Docker / Swarm / Kubernetes）迁移
-- 幂等脚本：新租户建表后执行，已有租户启动时重复执行。
-- 注意：本模块**不修改** assets / asset_category 任何列，
--       采集资源与资产台账通过 asset_ops_link 关联（方案 C）。
-- ==============================

-- =====================
-- 1. ops_connection 采集连接配置
-- =====================
CREATE TABLE IF NOT EXISTS {schema}.ops_connection (
    id BIGINT PRIMARY KEY,
    connection_name VARCHAR(100) NOT NULL,
    provider VARCHAR(32) NOT NULL,
    endpoint VARCHAR(255) NOT NULL,
    auth_type VARCHAR(32) NOT NULL,
    auth_config_enc TEXT,
    credential_hint VARCHAR(255),
    namespace VARCHAR(128),
    timeout_secs INTEGER NOT NULL DEFAULT 15,
    docker_api_version VARCHAR(16),
    max_items INTEGER NOT NULL DEFAULT 20000,
    poll_interval_secs INTEGER NOT NULL DEFAULT 300,
    enabled SMALLINT NOT NULL DEFAULT 1,
    last_collect_at TIMESTAMPTZ,
    last_error TEXT,
    created_by BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_by BIGINT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.ops_connection IS '运维采集连接配置（凭证为 AES-256-GCM 密文，密钥来自 LLM_KEY_ENCRYPT_KEY）';
COMMENT ON COLUMN {schema}.ops_connection.provider IS '采集来源：docker / swarm / k8s';
COMMENT ON COLUMN {schema}.ops_connection.endpoint IS '端点：local:// / tcp://host:2375 / https://host:2376 / https://apiserver:6443';
COMMENT ON COLUMN {schema}.ops_connection.auth_type IS '认证方式稳定编码：local_socket/plain_tcp/mutual_tls/mutual_tls_inline/kubeconfig_file/kubeconfig_inline/direct';
COMMENT ON COLUMN {schema}.ops_connection.auth_config_enc IS '凭证密文（AuthKind 的 JSON 序列化后 AES-GCM 加密），无凭证的认证方式为 NULL';
COMMENT ON COLUMN {schema}.ops_connection.credential_hint IS '凭证提示（如 CA/证书/私钥已就绪），绝不含明文';
COMMENT ON COLUMN {schema}.ops_connection.max_items IS '单类资源采集上限，防止超大集群拖垮进程';
COMMENT ON COLUMN {schema}.ops_connection.poll_interval_secs IS '轮询间隔（秒），供后续调度器使用';

CREATE UNIQUE INDEX IF NOT EXISTS uk_ops_connection_name
    ON {schema}.ops_connection(connection_name) WHERE deleted = 0;

CREATE INDEX IF NOT EXISTS idx_ops_connection_provider
    ON {schema}.ops_connection(provider) WHERE deleted = 0;

-- =====================
-- 2. asset_ops_link 采集资源 ↔ 资产台账桥表
-- =====================
CREATE TABLE IF NOT EXISTS {schema}.asset_ops_link (
    id BIGINT PRIMARY KEY,
    connection_id BIGINT NOT NULL,
    provider VARCHAR(32) NOT NULL,
    resource_uid VARCHAR(255) NOT NULL,
    resource_kind VARCHAR(50) NOT NULL,
    display_name VARCHAR(255),
    asset_id BIGINT,
    status_snapshot VARCHAR(32),
    raw_state VARCHAR(64),
    parent_uid VARCHAR(255),
    attributes JSONB,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.asset_ops_link IS '采集资源与资产台账的关联（唯一键 connection_id + resource_uid；asset_id 为空表示尚未归档）';
COMMENT ON COLUMN {schema}.asset_ops_link.resource_uid IS '资源唯一字符串，对应 ops::ResourceRecord::uid()';
COMMENT ON COLUMN {schema}.asset_ops_link.resource_kind IS '归一化资源类型：container/image/pod/node/...';
COMMENT ON COLUMN {schema}.asset_ops_link.asset_id IS '关联的资产台账主键，未归档为 NULL（台账表结构不做任何变更）';
COMMENT ON COLUMN {schema}.asset_ops_link.status_snapshot IS '最近一次采集的归一化状态；完整采集后本次未出现的资源置 gone';
COMMENT ON COLUMN {schema}.asset_ops_link.attributes IS '采集到的关键属性（端口、镜像摘要、副本数等）';

CREATE UNIQUE INDEX IF NOT EXISTS uk_asset_ops_link_uid
    ON {schema}.asset_ops_link(connection_id, resource_uid);

CREATE INDEX IF NOT EXISTS idx_asset_ops_link_asset
    ON {schema}.asset_ops_link(asset_id);

CREATE INDEX IF NOT EXISTS idx_asset_ops_link_seen
    ON {schema}.asset_ops_link(last_seen_at DESC);