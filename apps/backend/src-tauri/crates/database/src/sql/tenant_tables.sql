-- ==============================
-- 组织结构业务表（执行前替换 {schema} 为实际 schema 名）
-- ==============================

-- 1. 资产分类表
CREATE TABLE IF NOT EXISTS {schema}.asset_category (
    id BIGINT PRIMARY KEY,
    category_name VARCHAR(255) NOT NULL,
    asset_type VARCHAR(100) NOT NULL,
    parent_id BIGINT NOT NULL,
    sort SMALLINT NOT NULL,
    description TEXT,
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE,
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE,
    deleted SMALLINT
);

COMMENT ON TABLE {schema}.asset_category IS '资产分类表';

COMMENT ON COLUMN {schema}.asset_category.category_name IS '分类名称';

COMMENT ON COLUMN {schema}.asset_category.asset_type IS '资产类型';

COMMENT ON COLUMN {schema}.asset_category.parent_id IS '父分类ID';

COMMENT ON COLUMN {schema}.asset_category.sort IS '排序号';

-- 2. 资产主表
CREATE TABLE IF NOT EXISTS {schema}.assets (
    id BIGINT PRIMARY KEY,
    asset_no VARCHAR(100) NOT NULL,
    asset_type VARCHAR(50) NOT NULL,
    category_id BIGINT NOT NULL,
    asset_name VARCHAR(255) NOT NULL,
    manufacturer VARCHAR(255),
    model VARCHAR(255),
    department_ids BIGINT[],
    user_ids BIGINT[],
    status SMALLINT NOT NULL DEFAULT 0,
    purchase_date TIMESTAMP WITH TIME ZONE,
    purchase_price NUMERIC(12, 2) DEFAULT 0.00,
    quantity INTEGER NOT NULL DEFAULT 1,
    used_quantity INTEGER NOT NULL DEFAULT 0,
    expire_date TIMESTAMP WITH TIME ZONE,
    description TEXT,
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0,
    CONSTRAINT uk_asset_no UNIQUE (asset_no)
);

COMMENT ON TABLE {schema}.assets IS '资产主表';

COMMENT ON COLUMN {schema}.assets.id IS '主键ID';

COMMENT ON COLUMN {schema}.assets.asset_no IS '资产编号';

COMMENT ON COLUMN {schema}.assets.asset_type IS '资产类型：fixed=有形硬件 / intangible=无形资产';

COMMENT ON COLUMN {schema}.assets.category_id IS '资产分类ID';

COMMENT ON COLUMN {schema}.assets.asset_name IS '资产名称';

COMMENT ON COLUMN {schema}.assets.manufacturer IS '制造商/厂商';

COMMENT ON COLUMN {schema}.assets.model IS '型号';

COMMENT ON COLUMN {schema}.assets.department_ids IS '使用部门ID集合';

COMMENT ON COLUMN {schema}.assets.user_ids IS '使用人ID集合';

COMMENT ON COLUMN {schema}.assets.status IS '状态：0=正常 1=借用 2=维修 3=报废 4=过期';

COMMENT ON COLUMN {schema}.assets.purchase_date IS '购买日期';

COMMENT ON COLUMN {schema}.assets.purchase_price IS '购买金额';

COMMENT ON COLUMN {schema}.assets.quantity IS '总数量';

COMMENT ON COLUMN {schema}.assets.used_quantity IS '已使用数量';

COMMENT ON COLUMN {schema}.assets.expire_date IS '到期日';

COMMENT ON COLUMN {schema}.assets.description IS '备注说明';

COMMENT ON COLUMN {schema}.assets.created_by IS '创建人ID';

COMMENT ON COLUMN {schema}.assets.created_at IS '创建时间';

COMMENT ON COLUMN {schema}.assets.updated_by IS '更新人ID';

COMMENT ON COLUMN {schema}.assets.updated_at IS '更新时间';

COMMENT ON COLUMN {schema}.assets.deleted IS '删除标记：0=未删除 1=已删除';

-- 3. 硬件资产扩展表
CREATE TABLE IF NOT EXISTS {schema}.hard_assets (
    id BIGINT PRIMARY KEY,
    asset_id BIGINT NOT NULL REFERENCES {schema}.assets(id) ON DELETE CASCADE,
    sn VARCHAR(100),
    mac_address VARCHAR(100),
    location VARCHAR(255),
    hardware_config TEXT,
    use_start_date TIMESTAMP WITH TIME ZONE,
    maintenance_vendor VARCHAR(255),
    maintenance_type VARCHAR(100),
    maintenance_expire_date TIMESTAMP WITH TIME ZONE,
    fault_desc TEXT,
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.hard_assets IS '硬件资产表';

COMMENT ON COLUMN {schema}.hard_assets.id IS '主键ID';

COMMENT ON COLUMN {schema}.hard_assets.asset_id IS '关联资产主表ID';

COMMENT ON COLUMN {schema}.hard_assets.sn IS '序列号SN';

COMMENT ON COLUMN {schema}.hard_assets.mac_address IS 'MAC地址';

COMMENT ON COLUMN {schema}.hard_assets.location IS '存放位置';

COMMENT ON COLUMN {schema}.hard_assets.hardware_config IS '硬件配置';

COMMENT ON COLUMN {schema}.hard_assets.use_start_date IS '使用开始日期';

COMMENT ON COLUMN {schema}.hard_assets.maintenance_vendor IS '维保厂商';

COMMENT ON COLUMN {schema}.hard_assets.maintenance_type IS '维保类型';

COMMENT ON COLUMN {schema}.hard_assets.maintenance_expire_date IS '维保到期日';

COMMENT ON COLUMN {schema}.hard_assets.fault_desc IS '故障描述';

COMMENT ON COLUMN {schema}.hard_assets.created_by IS '创建人ID';

COMMENT ON COLUMN {schema}.hard_assets.created_at IS '创建时间';

COMMENT ON COLUMN {schema}.hard_assets.updated_by IS '更新人ID';

COMMENT ON COLUMN {schema}.hard_assets.updated_at IS '更新时间';

COMMENT ON COLUMN {schema}.hard_assets.deleted IS '删除标记：0=未删除 1=已删除';

CREATE INDEX IF NOT EXISTS idx_hard_asset ON {schema}.hard_assets (asset_id);

-- 4. 无形资产扩展表
CREATE TABLE IF NOT EXISTS {schema}.intangible_assets (
    id BIGINT PRIMARY KEY,
    asset_id BIGINT NOT NULL REFERENCES {schema}.assets(id) ON DELETE CASCADE,
    intangible_type VARCHAR(50) NOT NULL,
    register_no VARCHAR(100),
    register_owner VARCHAR(255),
    register_date TIMESTAMP WITH TIME ZONE,
    valid_start_date TIMESTAMP WITH TIME ZONE,
    valid_end_date TIMESTAMP WITH TIME ZONE,
    right_status VARCHAR(100),
    license_key VARCHAR(255),
    license_type VARCHAR(100),
    authorized_scope VARCHAR(255),
    bind_type VARCHAR(100),
    bind_info TEXT,
    version VARCHAR(100),
    download_link VARCHAR(255),
    amortization_method VARCHAR(50) DEFAULT 'straight_line',
    useful_life INTEGER,
    amortization_amount NUMERIC(12, 2) DEFAULT 0.00,
    residual_rate NUMERIC(5, 2) DEFAULT 0.05,
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.intangible_assets IS '无形资产表';

COMMENT ON COLUMN {schema}.intangible_assets.id IS '主键ID';

COMMENT ON COLUMN {schema}.intangible_assets.asset_id IS '关联资产主表ID';

COMMENT ON COLUMN {schema}.intangible_assets.intangible_type IS '无形资产类型：software/patent/trademark/copyright/franchise';

COMMENT ON COLUMN {schema}.intangible_assets.register_no IS '注册号/专利号/商标号';

COMMENT ON COLUMN {schema}.intangible_assets.register_owner IS '权利人';

COMMENT ON COLUMN {schema}.intangible_assets.register_date IS '申请/注册日期';

COMMENT ON COLUMN {schema}.intangible_assets.valid_start_date IS '生效开始日期';

COMMENT ON COLUMN {schema}.intangible_assets.valid_end_date IS '有效截止日期';

COMMENT ON COLUMN {schema}.intangible_assets.right_status IS '权利状态';

COMMENT ON COLUMN {schema}.intangible_assets.license_key IS '许可证密钥';

COMMENT ON COLUMN {schema}.intangible_assets.license_type IS '许可证类型：permanent/subscription/device/user';

COMMENT ON COLUMN {schema}.intangible_assets.authorized_scope IS '授权范围';

COMMENT ON COLUMN {schema}.intangible_assets.bind_type IS '绑定类型：设备/用户/IP';

COMMENT ON COLUMN {schema}.intangible_assets.bind_info IS '绑定信息';

COMMENT ON COLUMN {schema}.intangible_assets.version IS '版本号';

COMMENT ON COLUMN {schema}.intangible_assets.download_link IS '下载地址';

COMMENT ON COLUMN {schema}.intangible_assets.amortization_method IS '摊销方法：straight_line=直线摊销法';

COMMENT ON COLUMN {schema}.intangible_assets.useful_life IS '使用寿命（年）';

COMMENT ON COLUMN {schema}.intangible_assets.amortization_amount IS '月摊销额';

COMMENT ON COLUMN {schema}.intangible_assets.residual_rate IS '残值率';

COMMENT ON COLUMN {schema}.intangible_assets.created_by IS '创建人ID';

COMMENT ON COLUMN {schema}.intangible_assets.created_at IS '创建时间';

COMMENT ON COLUMN {schema}.intangible_assets.updated_by IS '更新人ID';

COMMENT ON COLUMN {schema}.intangible_assets.updated_at IS '更新时间';

COMMENT ON COLUMN {schema}.intangible_assets.deleted IS '删除标记：0=未删除 1=已删除';

CREATE INDEX IF NOT EXISTS idx_intangible_asset ON {schema}.intangible_assets (asset_id);

-- 5. 资产合同/文书/附件表
CREATE TABLE IF NOT EXISTS {schema}.asset_documents (
    id BIGINT PRIMARY KEY,
    asset_id BIGINT NOT NULL REFERENCES {schema}.assets(id) ON DELETE CASCADE,
    doc_type VARCHAR(50) NOT NULL,
    doc_name VARCHAR(255) NOT NULL,
    doc_no VARCHAR(100),
    party_a VARCHAR(255),
    party_b VARCHAR(255),
    sign_date TIMESTAMP WITH TIME ZONE,
    effective_date TIMESTAMP WITH TIME ZONE,
    expire_date TIMESTAMP WITH TIME ZONE,
    file_path TEXT,
    file_name VARCHAR(255),
    file_size BIGINT,
    remark TEXT,
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.asset_documents IS '资产文书合同表';

COMMENT ON COLUMN {schema}.asset_documents.id IS '主键ID';

COMMENT ON COLUMN {schema}.asset_documents.asset_id IS '关联资产主表ID';

COMMENT ON COLUMN {schema}.asset_documents.doc_type IS '文档类型：contract/agreement/authorization/certificate/record';

COMMENT ON COLUMN {schema}.asset_documents.doc_name IS '文档名称';

COMMENT ON COLUMN {schema}.asset_documents.doc_no IS '合同编号/证书编号';

COMMENT ON COLUMN {schema}.asset_documents.party_a IS '甲方';

COMMENT ON COLUMN {schema}.asset_documents.party_b IS '乙方';

COMMENT ON COLUMN {schema}.asset_documents.sign_date IS '签订日期';

COMMENT ON COLUMN {schema}.asset_documents.effective_date IS '生效日期';

COMMENT ON COLUMN {schema}.asset_documents.expire_date IS '到期日期';

COMMENT ON COLUMN {schema}.asset_documents.file_path IS '文件存储路径';

COMMENT ON COLUMN {schema}.asset_documents.file_name IS '文件原名';

COMMENT ON COLUMN {schema}.asset_documents.file_size IS '文件大小（字节）';

COMMENT ON COLUMN {schema}.asset_documents.remark IS '备注';

COMMENT ON COLUMN {schema}.asset_documents.created_by IS '创建人ID';

COMMENT ON COLUMN {schema}.asset_documents.created_at IS '创建时间';

COMMENT ON COLUMN {schema}.asset_documents.updated_by IS '更新人ID';

COMMENT ON COLUMN {schema}.asset_documents.updated_at IS '更新时间';

COMMENT ON COLUMN {schema}.asset_documents.deleted IS '删除标记：0=未删除 1=已删除';

CREATE INDEX IF NOT EXISTS idx_document_asset ON {schema}.asset_documents (asset_id);

-- 6. 资产知识库表
CREATE TABLE IF NOT EXISTS {schema}.asset_knowledge (
    id BIGINT PRIMARY KEY,
    asset_id BIGINT,
    doc_source VARCHAR(50) NOT NULL DEFAULT 'manual',
    knowledge_type VARCHAR(50) NOT NULL DEFAULT 'basic',
    title VARCHAR(255) NOT NULL,
    content TEXT NOT NULL,
    chunk_index INTEGER NOT NULL DEFAULT 0,
    vector_data BYTEA,
    permission_level VARCHAR(50) NOT NULL DEFAULT 'internal',
    owner_type VARCHAR(50),
    owner_id BIGINT,
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.asset_knowledge IS '资产知识库表（RAG检索 + 大模型微调专用）';

COMMENT ON COLUMN {schema}.asset_knowledge.id IS '主键ID';

COMMENT ON COLUMN {schema}.asset_knowledge.asset_id IS '关联资产主表ID（可选，知识条目可不关联资产）';

COMMENT ON COLUMN {schema}.asset_knowledge.doc_source IS '数据来源：manual=手动创建 / asset=主表 / hardware=硬件 / intangible=无形资产 / document=合同文书';

COMMENT ON COLUMN {schema}.asset_knowledge.knowledge_type IS '知识类型：basic=基础信息 / contract=合同 / hardware=硬件 / intangible=无形资产';

COMMENT ON COLUMN {schema}.asset_knowledge.title IS '知识标题';

COMMENT ON COLUMN {schema}.asset_knowledge.content IS '知识内容（用于向量化检索 + 模型微调）';

COMMENT ON COLUMN {schema}.asset_knowledge.chunk_index IS '文本分块序号（大文本自动拆分用）';

COMMENT ON COLUMN {schema}.asset_knowledge.vector_data IS '向量数据';

COMMENT ON COLUMN {schema}.asset_knowledge.permission_level IS '权限等级：public=公开 / internal=内部 / secret=机密';

COMMENT ON COLUMN {schema}.asset_knowledge.owner_type IS '归属类型：user=用户 / dept=部门 / role=角色';

COMMENT ON COLUMN {schema}.asset_knowledge.owner_id IS '归属ID（用户ID/部门ID/角色ID）';

COMMENT ON COLUMN {schema}.asset_knowledge.created_by IS '创建人ID';

COMMENT ON COLUMN {schema}.asset_knowledge.created_at IS '创建时间';

COMMENT ON COLUMN {schema}.asset_knowledge.updated_by IS '更新人ID';

COMMENT ON COLUMN {schema}.asset_knowledge.updated_at IS '更新时间';

COMMENT ON COLUMN {schema}.asset_knowledge.deleted IS '删除标记：0=未删除 1=已删除';

CREATE INDEX IF NOT EXISTS idx_knowledge_asset ON {schema}.asset_knowledge (asset_id);

CREATE INDEX IF NOT EXISTS idx_knowledge_type ON {schema}.asset_knowledge (knowledge_type);

CREATE INDEX IF NOT EXISTS idx_knowledge_permission ON {schema}.asset_knowledge (permission_level);

-- 7. 知识树节点表（AIFlowy 风格树形导航）
CREATE TABLE IF NOT EXISTS {schema}.knowledge_tree (
    id BIGINT PRIMARY KEY,
    knowledge_id BIGINT REFERENCES {schema}.asset_knowledge(id) ON DELETE CASCADE,
    parent_id BIGINT REFERENCES {schema}.knowledge_tree(id),
    node_type VARCHAR(20) NOT NULL DEFAULT 'document',
    title VARCHAR(255) NOT NULL,
    icon VARCHAR(50),
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_expanded BOOLEAN NOT NULL DEFAULT true,
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.knowledge_tree IS '知识树节点表（AIFlowy 风格树形导航）';

COMMENT ON COLUMN {schema}.knowledge_tree.knowledge_id IS '关联知识条目ID（folder 类型可为空）';

COMMENT ON COLUMN {schema}.knowledge_tree.parent_id IS '父节点ID（顶级节点为空）';

COMMENT ON COLUMN {schema}.knowledge_tree.node_type IS '节点类型：folder=文件夹 document=文档 link=链接';

COMMENT ON COLUMN {schema}.knowledge_tree.title IS '节点显示名称';

COMMENT ON COLUMN {schema}.knowledge_tree.icon IS '自定义图标';

COMMENT ON COLUMN {schema}.knowledge_tree.sort_order IS '同级排序号（越小越靠前）';

COMMENT ON COLUMN {schema}.knowledge_tree.is_expanded IS '是否展开（保存用户展开状态）';

CREATE INDEX IF NOT EXISTS idx_knowledge_tree_parent ON {schema}.knowledge_tree (parent_id);

CREATE INDEX IF NOT EXISTS idx_knowledge_tree_knowledge ON {schema}.knowledge_tree (knowledge_id);

-- 8. 资产领用申请表
CREATE TABLE IF NOT EXISTS {schema}.asset_receive (
    id BIGINT PRIMARY KEY,
    receive_no VARCHAR(100) NOT NULL,
    asset_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    department_id BIGINT NOT NULL,
    receive_date TIMESTAMP WITH TIME ZONE NOT NULL,
    reason TEXT NOT NULL,
    status SMALLINT NOT NULL,
    approve_by BIGINT,
    approve_time TIMESTAMP WITH TIME ZONE,
    approve_remark TEXT,
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE,
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE,
    deleted SMALLINT NOT NULL
);

COMMENT ON TABLE {schema}.asset_receive IS '资产领用申请表';

-- 12. 资产归还确认表
CREATE TABLE IF NOT EXISTS {schema}.asset_return (
    id BIGINT PRIMARY KEY,
    return_no VARCHAR(100) NOT NULL,
    receive_id BIGINT NOT NULL,
    asset_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    return_date TIMESTAMP WITH TIME ZONE NOT NULL,
    asset_status SMALLINT NOT NULL,
    remark TEXT,
    confirm_by BIGINT NOT NULL,
    confirm_time TIMESTAMP WITH TIME ZONE NOT NULL,
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE,
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE,
    deleted SMALLINT NOT NULL
);

COMMENT ON TABLE {schema}.asset_return IS '资产归还确认表';

-- 13. 资产调拨表
CREATE TABLE IF NOT EXISTS {schema}.asset_transfer (
    id BIGINT PRIMARY KEY,
    transfer_no VARCHAR(100) NOT NULL,
    asset_id BIGINT NOT NULL,
    out_dept_id BIGINT NOT NULL,
    in_dept_id BIGINT NOT NULL,
    out_user_id BIGINT NOT NULL,
    in_user_id BIGINT NOT NULL,
    transfer_date TIMESTAMP WITH TIME ZONE NOT NULL,
    reason TEXT NOT NULL,
    status SMALLINT NOT NULL,
    approve_by BIGINT,
    approve_time TIMESTAMP WITH TIME ZONE,
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE,
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE,
    deleted SMALLINT NOT NULL
);

COMMENT ON TABLE {schema}.asset_transfer IS '资产调拨表';

-- 14. 资产维修表
CREATE TABLE IF NOT EXISTS {schema}.asset_repair (
    id BIGINT PRIMARY KEY,
    repair_no VARCHAR(100) NOT NULL,
    asset_id BIGINT NOT NULL,
    fault_desc TEXT NOT NULL,
    repair_desc TEXT,
    repair_user_id BIGINT,
    repair_dept_id BIGINT,
    repair_file_url TEXT,
    repair_type SMALLINT NOT NULL,
    vendor VARCHAR(255),
    cost NUMERIC(12, 2),
    apply_date TIMESTAMP WITH TIME ZONE NOT NULL,
    repair_date TIMESTAMP WITH TIME ZONE,
    finish_date TIMESTAMP WITH TIME ZONE,
    status SMALLINT NOT NULL,
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE,
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE,
    deleted SMALLINT NOT NULL
);

COMMENT ON TABLE {schema}.asset_repair IS '资产维修表';

-- 15. 资产报废表
CREATE TABLE IF NOT EXISTS {schema}.asset_scrap (
    id BIGINT PRIMARY KEY,
    scrap_no VARCHAR(100) NOT NULL,
    asset_id BIGINT NOT NULL,
    reason TEXT NOT NULL,
    scrap_date TIMESTAMP WITH TIME ZONE NOT NULL,
    status SMALLINT NOT NULL,
    approve_by BIGINT,
    approve_time TIMESTAMP WITH TIME ZONE,
    handle_user BIGINT,
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE,
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE,
    deleted SMALLINT NOT NULL
);

COMMENT ON TABLE {schema}.asset_scrap IS '资产报废表';

-- 16. 资产采购申请表
CREATE TABLE IF NOT EXISTS {schema}.asset_purchase (
    id BIGINT PRIMARY KEY,
    purchase_no VARCHAR(100) NOT NULL,
    asset_name VARCHAR(255) NOT NULL,
    category_id BIGINT NOT NULL,
    model VARCHAR(255),
    manufacturer VARCHAR(255),
    quantity INTEGER NOT NULL,
    unit_price NUMERIC(12, 2),
    total_price NUMERIC(12, 2),
    apply_user BIGINT NOT NULL,
    dept_id BIGINT NOT NULL,
    reason TEXT NOT NULL,
    status SMALLINT NOT NULL,
    supplier VARCHAR(255),
    purchase_date TIMESTAMP WITH TIME ZONE,
    arrive_date TIMESTAMP WITH TIME ZONE,
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE,
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE,
    deleted SMALLINT NOT NULL
);

COMMENT ON TABLE {schema}.asset_purchase IS '资产采购申请表';

-- 17. 文件上传记录表（大文件分片上传，支持附件版本管理）
CREATE TABLE IF NOT EXISTS {schema}.file_uploads (
    id BIGINT PRIMARY KEY,

-- 版本管理字段
file_group_id VARCHAR(36) NOT NULL,
version INTEGER NOT NULL DEFAULT 1,
is_latest BOOLEAN NOT NULL DEFAULT true,
change_reason VARCHAR(500),
file_md5 VARCHAR(64),

-- S3 分片上传字段
upload_id VARCHAR(255),
    bucket VARCHAR(255),
    object_key VARCHAR(1024),
    original_filename VARCHAR(512) NOT NULL,
    file_size BIGINT NOT NULL,
    mime_type VARCHAR(255),
    chunk_size INTEGER NOT NULL DEFAULT 5242880,
    total_chunks INTEGER NOT NULL,
    received_chunks INTEGER[] NOT NULL DEFAULT '{}',
    received_etags TEXT[] NOT NULL DEFAULT '{}',
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    file_url VARCHAR(2048),
    etag VARCHAR(255),

-- 业务上下文


context_type VARCHAR(50),
    context_id BIGINT,
    commit_at TIMESTAMP WITH TIME ZONE,

    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0,

    CONSTRAINT uk_file_group_version UNIQUE (file_group_id, version)
);

COMMENT ON TABLE {schema}.file_uploads IS '大文件分片上传记录表（支持附件版本管理）';

COMMENT ON COLUMN {schema}.file_uploads.file_group_id IS '版本分组UUID，同一文件的不同版本共用此ID';

COMMENT ON COLUMN {schema}.file_uploads.version IS '版本号，从1开始递增';

COMMENT ON COLUMN {schema}.file_uploads.is_latest IS '是否为当前最新版本';

COMMENT ON COLUMN {schema}.file_uploads.change_reason IS '变更原因说明';

COMMENT ON COLUMN {schema}.file_uploads.file_md5 IS '文件 MD5 哈希，用于判断是否真的发生变更';

COMMENT ON COLUMN {schema}.file_uploads.upload_id IS 'S3 Multipart Upload ID（pending 状态时可为空）';

COMMENT ON COLUMN {schema}.file_uploads.bucket IS 'S3 存储桶';

COMMENT ON COLUMN {schema}.file_uploads.object_key IS 'S3 对象键';

COMMENT ON COLUMN {schema}.file_uploads.chunk_size IS '分片大小（字节），默认 5MB';

COMMENT ON COLUMN {schema}.file_uploads.total_chunks IS '总分片数';

COMMENT ON COLUMN {schema}.file_uploads.received_chunks IS '已接收的分片序号数组';

COMMENT ON COLUMN {schema}.file_uploads.received_etags IS '已接收的分片对应的真实 ETag 数组（与 received_chunks 一一对应）';

COMMENT ON COLUMN {schema}.file_uploads.status IS '状态：pending=待上传(占位) / uploading=上传中 / completed=已合并(待提交) / committed=已提交(已关联业务) / cancelled=已取消 / failed=失败';

COMMENT ON COLUMN {schema}.file_uploads.context_type IS '业务上下文类型：knowledge/asset/document';

COMMENT ON COLUMN {schema}.file_uploads.context_id IS '业务实体 ID';

COMMENT ON COLUMN {schema}.file_uploads.commit_at IS '正式提交到业务实体的时间';

CREATE INDEX IF NOT EXISTS idx_file_uploads_status ON {schema}.file_uploads (status);

CREATE INDEX IF NOT EXISTS idx_file_uploads_created_by ON {schema}.file_uploads (created_by);

CREATE INDEX IF NOT EXISTS idx_file_uploads_context ON {schema}.file_uploads (context_type, context_id);

CREATE INDEX IF NOT EXISTS idx_file_uploads_file_group ON {schema}.file_uploads (file_group_id);

-- 18. OKF 知识资产表（不与现有 asset_knowledge 冲突）
CREATE TABLE IF NOT EXISTS {schema}.knowledge_asset (
    id BIGINT PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
    tree_node_id BIGINT NOT NULL REFERENCES {schema}.knowledge_tree(id) ON DELETE CASCADE,
    title VARCHAR(512) NOT NULL,
    content TEXT,
    content_html TEXT,
    okf_type VARCHAR(30) NOT NULL DEFAULT 'raw_source',
    summary TEXT,
    source VARCHAR(512),
    confidence FLOAT DEFAULT 1.0,
    status VARCHAR(20) NOT NULL DEFAULT 'draft',
    effective_at TIMESTAMP WITH TIME ZONE,
    expire_at TIMESTAMP WITH TIME ZONE,
    relation_ids BIGINT[],
    tags TEXT[],
    file_url VARCHAR(1024),
    file_name VARCHAR(512),
    file_size BIGINT,
    file_mime VARCHAR(100),
    file_md5 VARCHAR(64),
    editor_mode VARCHAR(20) NOT NULL DEFAULT 'wysiwyg',
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.knowledge_asset IS 'OKF 标准化知识资产表';

COMMENT ON COLUMN {schema}.knowledge_asset.okf_type IS 'OKF知识类型：raw_source=原始素材 concept=概念 fact=事实 rule=规则 param=参数 process=流程 case=案例';

COMMENT ON COLUMN {schema}.knowledge_asset.status IS '状态：draft=草稿 valid=有效 outdated=过期 banned=禁用';

COMMENT ON COLUMN {schema}.knowledge_asset.tree_node_id IS '关联知识树节点ID';

COMMENT ON COLUMN {schema}.knowledge_asset.confidence IS '可信度 0.0~1.0';

COMMENT ON COLUMN {schema}.knowledge_asset.file_md5 IS '文件 MD5 防重复上传';

CREATE INDEX IF NOT EXISTS idx_knowledge_asset_tree_node ON {schema}.knowledge_asset (tree_node_id);

CREATE INDEX IF NOT EXISTS idx_knowledge_asset_okf_type ON {schema}.knowledge_asset (okf_type);

CREATE INDEX IF NOT EXISTS idx_knowledge_asset_status ON {schema}.knowledge_asset (status);

-- ==============================
-- 19. 单据编号规则配置表
-- ==============================
CREATE TABLE IF NOT EXISTS {schema}.doc_numbering_rule (
    id BIGINT PRIMARY KEY,
    biz_type VARCHAR(50) NOT NULL UNIQUE,
    biz_name VARCHAR(100) NOT NULL,
    prefix VARCHAR(50),
    date_format VARCHAR(20) DEFAULT 'yyyyMMdd',
    date_position VARCHAR(100) DEFAULT 'after_prefix',
    serial_length INT NOT NULL DEFAULT 4,
    separator VARCHAR(5) DEFAULT '-',
    reset_mode VARCHAR(20) DEFAULT 'yearly',
    sample_output VARCHAR(100),
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_by BIGINT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_by BIGINT,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.doc_numbering_rule IS '单据编号规则配置表';

COMMENT ON COLUMN {schema}.doc_numbering_rule.biz_type IS '业务类型：asset/receive/return/transfer/repair/scrap/purchase';

COMMENT ON COLUMN {schema}.doc_numbering_rule.biz_name IS '业务名称：资产编号/领用单号/归还单号/调拨单号/维修单号/报废单号/采购单号';

COMMENT ON COLUMN {schema}.doc_numbering_rule.prefix IS '前缀，如 ZC/LY/GH/DB/WX/BF/CG';

COMMENT ON COLUMN {schema}.doc_numbering_rule.date_format IS '日期格式：yyyyMMdd/yyMMdd/yyyyMM/yyyy/空（无日期）';

COMMENT ON COLUMN {schema}.doc_numbering_rule.date_position IS '日期位置：after_prefix（前缀后）/before_serial（流水号前）';

COMMENT ON COLUMN {schema}.doc_numbering_rule.serial_length IS '流水号位数，如4→0001';

COMMENT ON COLUMN {schema}.doc_numbering_rule.separator IS '分隔符，如"-"';

COMMENT ON COLUMN {schema}.doc_numbering_rule.reset_mode IS '重置模式：yearly（按年）/monthly（按月）/never（永不）';

COMMENT ON COLUMN {schema}.doc_numbering_rule.sample_output IS '示例输出，如"ZC-202607-0001"';

COMMENT ON COLUMN {schema}.doc_numbering_rule.is_active IS '是否启用';

-- ==============================
-- 20. 单据编号流水号计数表
-- ==============================
CREATE TABLE IF NOT EXISTS {schema}.doc_numbering_sequence (
    id BIGINT PRIMARY KEY,
    biz_type VARCHAR(50) NOT NULL,
    reset_key VARCHAR(50) NOT NULL DEFAULT '',
    current_seq INT NOT NULL DEFAULT 0,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    CONSTRAINT uk_numbering_seq UNIQUE (biz_type, reset_key)
);

COMMENT ON TABLE {schema}.doc_numbering_sequence IS '单据编号流水号计数表';

COMMENT ON COLUMN {schema}.doc_numbering_sequence.biz_type IS '业务类型';

COMMENT ON COLUMN {schema}.doc_numbering_sequence.reset_key IS '重置键：如"202607"（年月）或"2026"（年）';

COMMENT ON COLUMN {schema}.doc_numbering_sequence.current_seq IS '当前流水号值';

CREATE INDEX IF NOT EXISTS idx_numbering_seq_biz ON {schema}.doc_numbering_sequence (biz_type);

-- ==============================
-- 21. llm_provider 大模型服务商配置（多组织）
-- ==============================
CREATE TABLE IF NOT EXISTS {schema}.llm_provider (
    id BIGSERIAL PRIMARY KEY,
    provider_code VARCHAR(50) NOT NULL UNIQUE,
    provider_name VARCHAR(100) NOT NULL,
    base_url VARCHAR(1024),
    api_key TEXT,
    secret_key TEXT,
    extra_config JSONB,
    weight INT NOT NULL DEFAULT 10,
    is_local BOOLEAN NOT NULL DEFAULT false,
    enable BOOLEAN NOT NULL DEFAULT true,
    created_by BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.llm_provider IS '大模型服务商配置（多组织）';

COMMENT ON COLUMN {schema}.llm_provider.api_key IS 'AES-256-GCM 加密存储，前端永不返回明文';

COMMENT ON COLUMN {schema}.llm_provider.weight IS '负载均衡权重，越高优先被选择';

CREATE INDEX IF NOT EXISTS idx_llm_provider_code ON {schema}.llm_provider (provider_code, deleted);

CREATE INDEX IF NOT EXISTS idx_llm_provider_enable ON {schema}.llm_provider (enable, deleted);

-- ==============================
-- 22. llm_model 模型明细表（多组织）
-- ==============================
CREATE TABLE IF NOT EXISTS {schema}.llm_model (
    id BIGSERIAL PRIMARY KEY,
    provider_id BIGINT NOT NULL REFERENCES {schema}.llm_provider (id) ON DELETE CASCADE,
    model_code VARCHAR(100) NOT NULL,
    model_name VARCHAR(100) NOT NULL,
    model_type VARCHAR(30) NOT NULL,
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

COMMENT ON TABLE {schema}.llm_model IS '模型明细表（多组织）';

COMMENT ON COLUMN {schema}.llm_model.model_type IS 'chat=对话 embedding=向量 asr=语音识别 tts=语音合成';

COMMENT ON COLUMN {schema}.llm_model.price_input IS '输入价格（每1K tokens，单位：元）';

COMMENT ON COLUMN {schema}.llm_model.price_output IS '输出价格（每1K tokens，单位：元）';

CREATE INDEX IF NOT EXISTS idx_llm_model_provider ON {schema}.llm_model (provider_id, deleted);

CREATE INDEX IF NOT EXISTS idx_llm_model_type ON {schema}.llm_model (model_type, enable, deleted);

-- ==============================
-- 23. user_llm_setting 用户模型偏好（多组织）
-- ==============================
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

COMMENT ON TABLE {schema}.user_llm_setting IS '用户模型偏好配置（多组织）';

COMMENT ON COLUMN {schema}.user_llm_setting.custom_temp IS '用户自定义温度，覆盖模型默认值';

COMMENT ON COLUMN {schema}.user_llm_setting.custom_max_token IS '用户自定义最大输出Token';

CREATE INDEX IF NOT EXISTS idx_user_llm_uid ON {schema}.user_llm_setting (user_id, deleted);

-- ==============================
-- 24. llm_call_record LLM调用用量日志（多组织）
-- ==============================
CREATE TABLE IF NOT EXISTS {schema}.llm_call_record (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT,
    conv_id BIGINT,
    provider_id BIGINT NOT NULL,
    model_id BIGINT NOT NULL,
    call_type VARCHAR(30) NOT NULL,
    input_tokens INT NOT NULL DEFAULT 0,
    output_tokens INT NOT NULL DEFAULT 0,
    total_cost NUMERIC(10, 6) DEFAULT 0,
    duration_ms INT DEFAULT 0,
    status VARCHAR(20) NOT NULL,
    error_msg TEXT,
    request_id VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE {schema}.llm_call_record IS 'LLM 调用全链路日志（多组织）';

COMMENT ON COLUMN {schema}.llm_call_record.total_cost IS '费用=price_input*(输入tokens/1000) + price_output*(输出tokens/1000)';

COMMENT ON COLUMN {schema}.llm_call_record.duration_ms IS '调用耗时，用于性能监控';

CREATE INDEX IF NOT EXISTS idx_llm_call_user ON {schema}.llm_call_record (user_id);

CREATE INDEX IF NOT EXISTS idx_llm_call_conv ON {schema}.llm_call_record (conv_id);

CREATE INDEX IF NOT EXISTS idx_llm_call_time ON {schema}.llm_call_record (created_at);

CREATE INDEX IF NOT EXISTS idx_llm_call_status ON {schema}.llm_call_record (status);

-- ==============================
-- 25. document_chunk 向量分片检索表
-- ==============================
CREATE TABLE IF NOT EXISTS {schema}.document_chunk (
    id BIGSERIAL PRIMARY KEY,
    asset_id BIGINT NOT NULL REFERENCES {schema}.knowledge_asset(id) ON DELETE CASCADE,
    chunk_index INT NOT NULL,
    chunk_text TEXT NOT NULL,
    token_count INT,
    embedding vector(1536),
    title VARCHAR(512),
    okf_type VARCHAR(30),
    tags TEXT[],
    tree_node_id BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted SMALLINT NOT NULL DEFAULT 0
);

COMMENT ON TABLE {schema}.document_chunk IS 'RAG向量分片检索表';

COMMENT ON COLUMN {schema}.document_chunk.embedding IS '1536维pgvector向量，HNSW索引加速';

COMMENT ON COLUMN {schema}.document_chunk.title IS '来源资产标题（冗余，避免每次关联查询）';

COMMENT ON COLUMN {schema}.document_chunk.tree_node_id IS '来源目录ID（冗余，用于限定目录检索）';

CREATE INDEX IF NOT EXISTS idx_chunk_asset ON {schema}.document_chunk(asset_id, deleted);

CREATE INDEX IF NOT EXISTS idx_chunk_tree ON {schema}.document_chunk(tree_node_id, deleted);

-- ==============================
-- 26. conversation 对话会话表
-- ==============================
CREATE TABLE IF NOT EXISTS {schema}.conversation (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    title VARCHAR(255),
    bind_knowledge_tree_id BIGINT REFERENCES {schema}.knowledge_tree(id),
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

-- ==============================
-- 27. message 会话消息表
-- ==============================
CREATE TABLE IF NOT EXISTS {schema}.message (
    id BIGSERIAL PRIMARY KEY,
    conv_id BIGINT NOT NULL REFERENCES {schema}.conversation(id) ON DELETE CASCADE,
    role VARCHAR(20) NOT NULL,
    content TEXT NOT NULL,
    audio_url VARCHAR(1024),
    reference_asset_ids BIGINT[],
    reference_text VARCHAR(2048),
    metadata JSONB,
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

-- ==============================
-- 28. memory 用户长期记忆表
-- ==============================
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

-- ==============================
-- 29. skill_execution 技能执行日志表
-- ==============================
CREATE TABLE IF NOT EXISTS {schema}.skill_execution (
    id BIGSERIAL PRIMARY KEY,
    asset_id BIGINT REFERENCES {schema}.knowledge_asset(id),
    trigger_type VARCHAR(30),
    input_params JSONB,
    output_result JSONB,
    status VARCHAR(20) NOT NULL,
    error_msg TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE {schema}.skill_execution IS 'Skill规则/流程执行日志';

CREATE INDEX IF NOT EXISTS idx_skill_asset ON {schema}.skill_execution(asset_id);

CREATE INDEX IF NOT EXISTS idx_skill_status ON {schema}.skill_execution(status);

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