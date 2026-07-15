-- ==============================
-- 租户业务表（执行前替换 {schema} 为实际 schema 名）
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
    department_id BIGINT,
    user_id BIGINT,
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

COMMENT ON COLUMN {schema}.assets.department_id IS '使用部门ID';

COMMENT ON COLUMN {schema}.assets.user_id IS '使用人ID';

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
    use_user_id BIGINT,
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

COMMENT ON COLUMN {schema}.hard_assets.use_user_id IS '使用人ID';

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
    assigned_user_ids TEXT,
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

COMMENT ON COLUMN {schema}.intangible_assets.assigned_user_ids IS '授权用户ID集合';

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
    asset_id BIGINT,                              -- 改为可选，知识条目可不关联资产
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
    node_type VARCHAR(20) NOT NULL DEFAULT 'document',  -- folder / document / link
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
file_group_id VARCHAR(36) NOT NULL, -- UUID，同一文件的不同版本共用
version INTEGER NOT NULL DEFAULT 1, -- 版本号，从 1 开始递增
is_latest BOOLEAN NOT NULL DEFAULT true, -- 是否为当前最新版本
change_reason VARCHAR(500), -- 变更原因，如"更新合同条款"
file_md5 VARCHAR(64), -- 文件 MD5，用于判断是否真的变更

-- S3 分片上传字段
upload_id VARCHAR(255),                            -- S3 Multipart Upload ID（pending 状态时可为空）
    bucket VARCHAR(255),                               -- S3 存储桶
    object_key VARCHAR(1024),                          -- S3 对象键
    original_filename VARCHAR(512) NOT NULL,
    file_size BIGINT NOT NULL,
    mime_type VARCHAR(255),
    chunk_size INTEGER NOT NULL DEFAULT 5242880,
    total_chunks INTEGER NOT NULL,
    received_chunks INTEGER[] NOT NULL DEFAULT '{}',
    status VARCHAR(20) NOT NULL DEFAULT 'pending',     -- pending/uploading/completed/committed/cancelled/failed
    file_url VARCHAR(2048),
    etag VARCHAR(255),

-- 业务上下文（关联业务实体）


context_type VARCHAR(50),                          -- 业务类型：knowledge/asset/document
    context_id BIGINT,                                 -- 业务实体 ID
    commit_at TIMESTAMP WITH TIME ZONE,                -- 正式提交时间

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