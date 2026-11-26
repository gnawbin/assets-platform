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

-- =====================
-- 3. ops_connection.exec_enabled（容器终端连接级开关，默认关闭）
-- =====================
ALTER TABLE {schema}.ops_connection
    ADD COLUMN IF NOT EXISTS exec_enabled SMALLINT NOT NULL DEFAULT 0;

COMMENT ON COLUMN {schema}.ops_connection.exec_enabled IS '是否允许容器终端（0=禁止 1=允许；默认 0，需同时满足环境开关与 ops:terminal:exec 权限点）';

-- =====================
-- 4. ops_terminal_session 容器终端会话审计（Docker + K8s 共用）
-- =====================
CREATE TABLE IF NOT EXISTS {schema}.ops_terminal_session (
    id             BIGINT PRIMARY KEY,
    provider       VARCHAR(16) NOT NULL,
    connection_id  BIGINT NOT NULL,
    target_kind    VARCHAR(16) NOT NULL,
    container_uid  VARCHAR(255),
    namespace      VARCHAR(255),
    pod_name       VARCHAR(255),
    container_name VARCHAR(255),
    shell          VARCHAR(64),
    user_id        BIGINT NOT NULL,
    state          VARCHAR(16) NOT NULL,
    client_ip      VARCHAR(64),
    started_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ended_at       TIMESTAMPTZ,
    bytes_in       BIGINT NOT NULL DEFAULT 0,
    bytes_out      BIGINT NOT NULL DEFAULT 0,
    exit_code      INT,
    error          TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE {schema}.ops_terminal_session IS '容器终端会话审计（谁 / 何时 / 哪个连接与容器 / shell / 字节数 / 退出码）；真正的 exec 会话只存在于持有它的进程内';
COMMENT ON COLUMN {schema}.ops_terminal_session.provider IS '来源：docker / k8s';
COMMENT ON COLUMN {schema}.ops_terminal_session.connection_id IS '软引用 ops_connection.id（不建外键，避免软删阻塞）';
COMMENT ON COLUMN {schema}.ops_terminal_session.target_kind IS '目标类型：docker_container / k8s_pod';
COMMENT ON COLUMN {schema}.ops_terminal_session.container_uid IS 'Docker 容器 Id（与 asset_ops_link.resource_uid 同口径）';
COMMENT ON COLUMN {schema}.ops_terminal_session.namespace IS 'K8s 命名空间';
COMMENT ON COLUMN {schema}.ops_terminal_session.pod_name IS 'K8s Pod 名';
COMMENT ON COLUMN {schema}.ops_terminal_session.container_name IS '容器名（Docker / K8s 都有；K8s 多容器必填）';
COMMENT ON COLUMN {schema}.ops_terminal_session.shell IS '实际使用的 shell（白名单命中项）';
COMMENT ON COLUMN {schema}.ops_terminal_session.state IS '会话状态：opening / active / closed / failed';
COMMENT ON COLUMN {schema}.ops_terminal_session.bytes_in IS '客户端 → 容器 的累计字节数';
COMMENT ON COLUMN {schema}.ops_terminal_session.bytes_out IS '容器 → 客户端 的累计字节数';
COMMENT ON COLUMN {schema}.ops_terminal_session.error IS '失败原因（不含终端输出）';

CREATE INDEX IF NOT EXISTS idx_ops_terminal_session_conn
    ON {schema}.ops_terminal_session(connection_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_ops_terminal_session_user
    ON {schema}.ops_terminal_session(user_id, started_at DESC);