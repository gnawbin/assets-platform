-- ==============================
-- 运维终端（SSH）模块迁移
-- 幂等脚本：新租户建表后执行，存量租户启动时重复执行。
-- 注意：本模块**不修改** ops_connection / assets / asset_category 任何列，
--       SSH 主机与采集连接是两套生命周期（方案 D1）。
-- ==============================

-- =====================
-- 1. ops_ssh_target SSH 主机配置
-- =====================
CREATE TABLE IF NOT EXISTS {schema}.ops_ssh_target (
    id BIGINT PRIMARY KEY,
    target_name VARCHAR(100) NOT NULL,
    host VARCHAR(255) NOT NULL,
    port INTEGER NOT NULL DEFAULT 22,
    username VARCHAR(128) NOT NULL,
    auth_type VARCHAR(32) NOT NULL,
    auth_config_enc TEXT,
    credential_hint VARCHAR(128),
    host_key_fingerprint VARCHAR(128),
    strict_host_key_check SMALLINT NOT NULL DEFAULT 1,
    asset_id BIGINT,
    description TEXT,
    connect_timeout_secs INTEGER NOT NULL DEFAULT 10,
    enabled SMALLINT NOT NULL DEFAULT 1,
    last_connected_at TIMESTAMPTZ,
    last_error TEXT,
    created_by BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_by BIGINT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.ops_ssh_target IS 'SSH 主机配置（凭证为 AES-256-GCM 密文，密钥来自 LLM_KEY_ENCRYPT_KEY）';
COMMENT ON COLUMN {schema}.ops_ssh_target.target_name IS '主机别名，同租户内唯一（deleted = 0）';
COMMENT ON COLUMN {schema}.ops_ssh_target.host IS '主机名或 IP，不含 scheme / 端口';
COMMENT ON COLUMN {schema}.ops_ssh_target.auth_type IS '认证方式稳定编码：password / private_key';
COMMENT ON COLUMN {schema}.ops_ssh_target.auth_config_enc IS '凭证密文（{"password"} 或 {"private_key_pem","passphrase"} 的 AES-GCM 密文），只进不出';
COMMENT ON COLUMN {schema}.ops_ssh_target.credential_hint IS '凭证提示（如「密码（已配置）」），绝不含明文';
COMMENT ON COLUMN {schema}.ops_ssh_target.host_key_fingerprint IS 'TOFU 固定的主机指纹，形如 SHA256:xxxx；为空表示只扫描不校验';
COMMENT ON COLUMN {schema}.ops_ssh_target.strict_host_key_check IS '1 = 必须与 host_key_fingerprint 逐字匹配；0 = 允许首连记录';
COMMENT ON COLUMN {schema}.ops_ssh_target.asset_id IS '软引用资产台账，未绑定为 NULL（不建外键）';
COMMENT ON COLUMN {schema}.ops_ssh_target.connect_timeout_secs IS '连接 / 握手超时（秒），服务层再夹到 3..=60';

CREATE UNIQUE INDEX IF NOT EXISTS uk_ops_ssh_target_name
    ON {schema}.ops_ssh_target(target_name) WHERE deleted = 0;

CREATE INDEX IF NOT EXISTS idx_ops_ssh_target_asset
    ON {schema}.ops_ssh_target(asset_id) WHERE deleted = 0;

-- =====================
-- 2. ops_ssh_session 会话审计
-- =====================
CREATE TABLE IF NOT EXISTS {schema}.ops_ssh_session (
    id BIGINT PRIMARY KEY,
    target_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    state VARCHAR(16) NOT NULL,
    client_ip VARCHAR(64),
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ended_at TIMESTAMPTZ,
    bytes_in BIGINT NOT NULL DEFAULT 0,
    bytes_out BIGINT NOT NULL DEFAULT 0,
    exit_code INTEGER,
    error TEXT
);

COMMENT ON TABLE {schema}.ops_ssh_session IS 'SSH 会话审计（谁 / 何时 / 哪台 / 字节数 / 退出码）；真正的连接只存在于持有它的进程内';
COMMENT ON COLUMN {schema}.ops_ssh_session.target_id IS '软引用 ops_ssh_target.id（不建外键，避免软删阻塞）';
COMMENT ON COLUMN {schema}.ops_ssh_session.state IS '会话状态：opening / active / closed / failed';
COMMENT ON COLUMN {schema}.ops_ssh_session.bytes_in IS '客户端 → 目标主机 的累计字节数';
COMMENT ON COLUMN {schema}.ops_ssh_session.bytes_out IS '目标主机 → 客户端 的累计字节数';
COMMENT ON COLUMN {schema}.ops_ssh_session.error IS '失败原因（主机不可达 / 认证失败 / 指纹不匹配等），不含凭证';

CREATE INDEX IF NOT EXISTS idx_ops_ssh_session_user
    ON {schema}.ops_ssh_session(user_id, started_at DESC);

CREATE INDEX IF NOT EXISTS idx_ops_ssh_session_target
    ON {schema}.ops_ssh_session(target_id, started_at DESC);
