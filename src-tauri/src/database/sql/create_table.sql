-- ==============================
-- 资产管理系统 数据库建表脚本
-- 严格对应 Rust 结构体
-- ==============================

-- 1. 资产分类表
CREATE TABLE IF NOT EXISTS asset_category (
    id bigserial PRIMARY KEY,
    category_name varchar(255) NOT NULL,
    asset_type varchar(100) NOT NULL,
    parent_id int8 NOT NULL,
    sort int2 NOT NULL,
    description text NULL,
    created_by int8 NULL,
    created_at timestamptztz NULL,
    updated_by int8 NULL,
    updated_at timestamptztz NULL,
    deleted int2 NULL
);
COMMENT ON TABLE asset_category IS '资产分类表';
COMMENT ON COLUMN asset_category.category_name IS '分类名称';
COMMENT ON COLUMN asset_category.asset_type IS '资产类型';
COMMENT ON COLUMN asset_category.parent_id IS '父分类ID';
COMMENT ON COLUMN asset_category.sort IS '排序号';


-- ==============================================
-- 资产主表（所有资产统一入口）
-- ==============================================
CREATE TABLE IF NOT EXISTS assets (
    id bigserial PRIMARY KEY,
    asset_no varchar(100) NOT NULL,
    asset_type varchar(50) NOT NULL,
    category_id int8 NOT NULL,
    asset_name varchar(255) NOT NULL,
    manufacturer varchar(255),
    model varchar(255),
    department_id int8  NULL,
    user_id int8 NULL,
    status int2 NOT NULL DEFAULT 0,
    purchase_date timestamptz,
    purchase_price numeric(12,2) DEFAULT 0.00,
    quantity int4 NOT NULL DEFAULT 1,
    used_quantity int4 NOT NULL DEFAULT 0,
    expire_date timestamptz,
    description text,

    created_by int8,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_by int8,
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted int2 NOT NULL DEFAULT 0,

    CONSTRAINT uk_asset_no UNIQUE (asset_no)
);

COMMENT ON TABLE assets IS '资产主表';
COMMENT ON COLUMN assets.id IS '主键ID';
COMMENT ON COLUMN assets.asset_no IS '资产编号';
COMMENT ON COLUMN assets.asset_type IS '资产类型：fixed=有形硬件 / intangible=无形资产';
COMMENT ON COLUMN assets.category_id IS '资产分类ID';
COMMENT ON COLUMN assets.asset_name IS '资产名称';
COMMENT ON COLUMN assets.manufacturer IS '制造商/厂商';
COMMENT ON COLUMN assets.model IS '型号';
COMMENT ON COLUMN assets.department_id IS '使用部门ID';
COMMENT ON COLUMN assets.user_id IS '使用人ID';
COMMENT ON COLUMN assets.status IS '状态：0=正常 1=借用 2=维修 3=报废 4=过期';
COMMENT ON COLUMN assets.purchase_date IS '购买日期';
COMMENT ON COLUMN assets.purchase_price IS '购买金额';
COMMENT ON COLUMN assets.quantity IS '总数量';
COMMENT ON COLUMN assets.used_quantity IS '已使用数量';
COMMENT ON COLUMN assets.expire_date IS '到期日';
COMMENT ON COLUMN assets.description IS '备注说明';
COMMENT ON COLUMN assets.created_by IS '创建人ID';
COMMENT ON COLUMN assets.created_at IS '创建时间';
COMMENT ON COLUMN assets.updated_by IS '更新人ID';
COMMENT ON COLUMN assets.updated_at IS '更新时间';
COMMENT ON COLUMN assets.deleted IS '删除标记：0=未删除 1=已删除';


-- ==============================================
-- 硬件资产扩展表
-- ==============================================
CREATE TABLE IF NOT EXISTS hard_assets (
    id bigserial PRIMARY KEY,
    asset_id int8 NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    sn varchar(100),
    mac_address varchar(100),
    location varchar(255),
    hardware_config text,
    use_user_id int8,
    use_start_date timestamptz,
    maintenance_vendor varchar(255),
    maintenance_type varchar(100),
    maintenance_expire_date timestamptz,
    fault_desc text,

    created_by int8,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_by int8,
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted int2 NOT NULL DEFAULT 0
);

COMMENT ON TABLE hard_assets IS '硬件资产表';
COMMENT ON COLUMN hard_assets.id IS '主键ID';
COMMENT ON COLUMN hard_assets.asset_id IS '关联资产主表ID';
COMMENT ON COLUMN hard_assets.sn IS '序列号SN';
COMMENT ON COLUMN hard_assets.mac_address IS 'MAC地址';
COMMENT ON COLUMN hard_assets.location IS '存放位置';
COMMENT ON COLUMN hard_assets.hardware_config IS '硬件配置';
COMMENT ON COLUMN hard_assets.use_user_id IS '使用人ID';
COMMENT ON COLUMN hard_assets.use_start_date IS '使用开始日期';
COMMENT ON COLUMN hard_assets.maintenance_vendor IS '维保厂商';
COMMENT ON COLUMN hard_assets.maintenance_type IS '维保类型';
COMMENT ON COLUMN hard_assets.maintenance_expire_date IS '维保到期日';
COMMENT ON COLUMN hard_assets.fault_desc IS '故障描述';
COMMENT ON COLUMN hard_assets.created_by IS '创建人ID';
COMMENT ON COLUMN hard_assets.created_at IS '创建时间';
COMMENT ON COLUMN hard_assets.updated_by IS '更新人ID';
COMMENT ON COLUMN hard_assets.updated_at IS '更新时间';
COMMENT ON COLUMN hard_assets.deleted IS '删除标记：0=未删除 1=已删除';

CREATE INDEX idx_hard_asset ON hard_assets(asset_id);

-- ==============================================
-- 无形资产扩展表
-- ==============================================
CREATE TABLE IF NOT EXISTS intangible_assets (
    id bigserial PRIMARY KEY,
    asset_id int8 NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    intangible_type varchar(50) NOT NULL,
    register_no varchar(100),
    register_owner varchar(255),
    register_date timestamptz,
    valid_start_date timestamptz,
    valid_end_date timestamptz,
    right_status varchar(100),

    license_key varchar(255),
    license_type varchar(100),
    authorized_scope varchar(255),
    assigned_user_ids text,
    bind_type varchar(100),
    bind_info text,
    version varchar(100),
    download_link varchar(255),

    amortization_method varchar(50) DEFAULT 'straight_line',
    useful_life int4,
    amortization_amount numeric(12,2) DEFAULT 0.00,
    residual_rate numeric(5,2) DEFAULT 0.05,

    created_by int8,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_by int8,
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted int2 NOT NULL DEFAULT 0
);

COMMENT ON TABLE intangible_assets IS '无形资产表';
COMMENT ON COLUMN intangible_assets.id IS '主键ID';
COMMENT ON COLUMN intangible_assets.asset_id IS '关联资产主表ID';
COMMENT ON COLUMN intangible_assets.intangible_type IS '无形资产类型：software/patent/trademark/copyright/franchise';
COMMENT ON COLUMN intangible_assets.register_no IS '注册号/专利号/商标号';
COMMENT ON COLUMN intangible_assets.register_owner IS '权利人';
COMMENT ON COLUMN intangible_assets.register_date IS '申请/注册日期';
COMMENT ON COLUMN intangible_assets.valid_start_date IS '生效开始日期';
COMMENT ON COLUMN intangible_assets.valid_end_date IS '有效截止日期';
COMMENT ON COLUMN intangible_assets.right_status IS '权利状态';

COMMENT ON COLUMN intangible_assets.license_key IS '许可证密钥';
COMMENT ON COLUMN intangible_assets.license_type IS '许可证类型：permanent/subscription/device/user';
COMMENT ON COLUMN intangible_assets.authorized_scope IS '授权范围';
COMMENT ON COLUMN intangible_assets.assigned_user_ids IS '授权用户ID集合';
COMMENT ON COLUMN intangible_assets.bind_type IS '绑定类型：设备/用户/IP';
COMMENT ON COLUMN intangible_assets.bind_info IS '绑定信息';
COMMENT ON COLUMN intangible_assets.version IS '版本号';
COMMENT ON COLUMN intangible_assets.download_link IS '下载地址';

COMMENT ON COLUMN intangible_assets.amortization_method IS '摊销方法：straight_line=直线摊销法';
COMMENT ON COLUMN intangible_assets.useful_life IS '使用寿命（年）';
COMMENT ON COLUMN intangible_assets.amortization_amount IS '月摊销额';
COMMENT ON COLUMN intangible_assets.residual_rate IS '残值率';

COMMENT ON COLUMN intangible_assets.created_by IS '创建人ID';
COMMENT ON COLUMN intangible_assets.created_at IS '创建时间';
COMMENT ON COLUMN intangible_assets.updated_by IS '更新人ID';
COMMENT ON COLUMN intangible_assets.updated_at IS '更新时间';
COMMENT ON COLUMN intangible_assets.deleted IS '删除标记：0=未删除 1=已删除';

CREATE INDEX idx_intangible_asset ON intangible_assets(asset_id);

-- ==============================================
-- 资产合同 / 文书 / 附件表
-- ==============================================
CREATE TABLE IF NOT EXISTS asset_documents (
    id bigserial PRIMARY KEY,
    asset_id int8 NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    doc_type varchar(50) NOT NULL,
    doc_name varchar(255) NOT NULL,
    doc_no varchar(100),
    party_a varchar(255),
    party_b varchar(255),
    sign_date timestamptz,
    effective_date timestamptz,
    expire_date timestamptz,

    file_path text,
    file_name varchar(255),
    file_size int8,
    remark text,

    created_by int8,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_by int8,
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted int2 NOT NULL DEFAULT 0
);

COMMENT ON TABLE asset_documents IS '资产文书合同表';
COMMENT ON COLUMN asset_documents.id IS '主键ID';
COMMENT ON COLUMN asset_documents.asset_id IS '关联资产主表ID';
COMMENT ON COLUMN asset_documents.doc_type IS '文档类型：contract/agreement/authorization/certificate/record';
COMMENT ON COLUMN asset_documents.doc_name IS '文档名称';
COMMENT ON COLUMN asset_documents.doc_no IS '合同编号/证书编号';
COMMENT ON COLUMN asset_documents.party_a IS '甲方';
COMMENT ON COLUMN asset_documents.party_b IS '乙方';
COMMENT ON COLUMN asset_documents.sign_date IS '签订日期';
COMMENT ON COLUMN asset_documents.effective_date IS '生效日期';
COMMENT ON COLUMN asset_documents.expire_date IS '到期日期';
COMMENT ON COLUMN asset_documents.file_path IS '文件存储路径';
COMMENT ON COLUMN asset_documents.file_name IS '文件原名';
COMMENT ON COLUMN asset_documents.file_size IS '文件大小（字节）';
COMMENT ON COLUMN asset_documents.remark IS '备注';
COMMENT ON COLUMN asset_documents.created_by IS '创建人ID';
COMMENT ON COLUMN asset_documents.created_at IS '创建时间';
COMMENT ON COLUMN asset_documents.updated_by IS '更新人ID';
COMMENT ON COLUMN asset_documents.updated_at IS '更新时间';
COMMENT ON COLUMN asset_documents.deleted IS '删除标记：0=未删除 1=已删除';

CREATE INDEX idx_document_asset ON asset_documents(asset_id);

-- ==============================================
-- 资产知识库表（RAG检索 + 大模型微调专用）
-- ==============================================
CREATE TABLE IF NOT EXISTS asset_knowledge (
    id bigserial PRIMARY KEY,
    asset_id int8 NOT NULL,                         -- 关联资产ID
    doc_source varchar(50) NOT NULL,                -- 数据来源：asset/hardware/intangible/document
    knowledge_type varchar(50) NOT NULL,            -- 知识类型：basic/contract/hardware/intangible
    title varchar(255) NOT NULL,                    -- 知识标题
    content text NOT NULL,                         -- 知识正文（用于向量化 + 微调）
    chunk_index int4 NOT NULL DEFAULT 0,            -- 文本分块序号
    vector_data vector(768),                        -- 向量数据（Embedding模型输出）

    -- 权限控制（对接OPA）
    permission_level varchar(50) NOT NULL DEFAULT 'internal',  -- 权限等级：public/internal/secret
    owner_type varchar(50),                        -- 归属类型：user/dept/role
    owner_id int8,                                 -- 归属人/部门/角色ID

    -- 基础字段
    created_by int8,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_by int8,
    updated_at timestamptz NOT NULL DEFAULT now(),
    deleted int2 NOT NULL DEFAULT 0
);

-- 注释
COMMENT ON TABLE asset_knowledge IS '资产知识库表（RAG检索 + 大模型微调专用）';
COMMENT ON COLUMN asset_knowledge.id IS '主键ID';
COMMENT ON COLUMN asset_knowledge.asset_id IS '关联资产主表ID';
COMMENT ON COLUMN asset_knowledge.doc_source IS '数据来源：asset=主表 / hardware=硬件 / intangible=无形资产 / document=合同文书';
COMMENT ON COLUMN asset_knowledge.knowledge_type IS '知识类型：basic=基础信息 / contract=合同 / hardware=硬件 / intangible=无形资产';
COMMENT ON COLUMN asset_knowledge.title IS '知识标题';
COMMENT ON COLUMN asset_knowledge.content IS '知识内容（用于向量化检索 + 模型微调）';
COMMENT ON COLUMN asset_knowledge.chunk_index IS '文本分块序号（大文本自动拆分用）';
COMMENT ON COLUMN asset_knowledge.vector_data IS '向量数据（Embedding向量化结果，768维）';
COMMENT ON COLUMN asset_knowledge.permission_level IS '权限等级：public=公开 / internal=内部 / secret=机密';
COMMENT ON COLUMN asset_knowledge.owner_type IS '归属类型：user=用户 / dept=部门 / role=角色';
COMMENT ON COLUMN asset_knowledge.owner_id IS '归属ID（用户ID/部门ID/角色ID）';
COMMENT ON COLUMN asset_knowledge.created_by IS '创建人ID';
COMMENT ON COLUMN asset_knowledge.created_at IS '创建时间';
COMMENT ON COLUMN asset_knowledge.updated_by IS '更新人ID';
COMMENT ON COLUMN asset_knowledge.updated_at IS '更新时间';
COMMENT ON COLUMN asset_knowledge.deleted IS '删除标记：0=未删除 1=已删除';

-- 索引
CREATE INDEX idx_knowledge_asset ON asset_knowledge(asset_id);
CREATE INDEX idx_knowledge_type ON asset_knowledge(knowledge_type);
CREATE INDEX idx_knowledge_permission ON asset_knowledge(permission_level);

-- 5. 系统用户表
CREATE TABLE IF NOT EXISTS sys_user (
    id bigserial PRIMARY KEY,
    username varchar(100) NOT NULL,
    passwd varchar(255) NOT NULL,
    domain varchar(100)  NULL,
    real_name varchar(100) NOT NULL,
    email varchar(100) NULL,
    phone varchar(50) NULL,
    department_id int8 NULL,
    status int2 NOT NULL,
    nickname varchar(100) NULL,
    avatar varchar(255) NULL,
    person_id varchar(50) NULL,
    person_code varchar(50) NULL,
    super_user_id int8 NULL,
    created_by int8 NULL,
    created_at timestamptz NULL,
    updated_by int8 NULL,
    updated_at timestamptz NULL,
    deleted int2 NULL
);
COMMENT ON TABLE sys_user IS '系统用户表';
COMMENT ON COLUMN sys_user.person_code IS '工号';

-- 6. 部门表
CREATE TABLE IF NOT EXISTS sys_department (
    id bigserial PRIMARY KEY,
    department_name varchar(255) NOT NULL,
    parent_id int8 NULL,
    description text NULL,
    created_by int8 NULL,
    created_at timestamptz NULL,
    updated_by int8 NULL,
    updated_at timestamptz NULL,
    deleted int2 NULL
);
COMMENT ON TABLE sys_department IS '部门表';

-- 7. 系统菜单&权限表
CREATE TABLE IF NOT EXISTS sys_menu (
    id bigserial PRIMARY KEY,
    menu_name varchar(255) NOT NULL,
    parent_id int8 NULL,
    path varchar(255) NULL,
    component varchar(255) NULL,
    icon varchar(255) NULL,
    order_num int2 NOT NULL,
    visible bool NOT NULL,
    perms varchar(255) NULL,
    menu_type int2 NOT NULL,
    hidden_button bool NOT NULL,
    created_by int8 NULL,
    created_at timestamptz NULL,
    updated_by int8 NULL,
    updated_at timestamptz NULL,
    deleted int2 NOT NULL
);
COMMENT ON TABLE sys_menu IS '系统菜单&权限表';
COMMENT ON COLUMN sys_menu.menu_type IS '1=目录 2=菜单 3=按钮';
COMMENT ON COLUMN sys_menu.perms IS '权限标识';
COMMENT ON COLUMN sys_menu.hidden_button IS '是否隐藏按钮';

-- 8. 角色表
CREATE TABLE IF NOT EXISTS sys_role (
    id bigserial PRIMARY KEY,
    role_key varchar(100) NOT NULL,
    role_name varchar(100) NOT NULL,
    description text NULL,
    created_by int8 NULL,
    created_at timestamptz NULL,
    updated_by int8 NULL,
    updated_at timestamptz NULL,
    deleted int2 NULL
);
COMMENT ON TABLE sys_role IS '角色表';

-- 9. 用户角色关联表
CREATE TABLE IF NOT EXISTS sys_user_role (
    id bigserial PRIMARY KEY,
    user_id int8 NOT NULL,
    role_id int8 NOT NULL,
    created_by int8 NULL,
    created_at timestamptz NULL,
    updated_by int8 NULL,
    updated_at timestamptz NULL,
    deleted int2 NULL
);
COMMENT ON TABLE sys_user_role IS '用户角色关联表';

-- 10. 角色菜单关联表
CREATE TABLE IF NOT EXISTS sys_role_menu (
    id bigserial PRIMARY KEY,
    role_id int8 NOT NULL,
    menu_id int8 NOT NULL,
    created_by int8 NULL,
    created_at timestamptz NULL,
    updated_by int8 NULL,
    updated_at timestamptz NULL,
    deleted int2 NULL
);
COMMENT ON TABLE sys_role_menu IS '角色菜单关联表';

-- 11. 资产领用申请表
CREATE TABLE IF NOT EXISTS asset_receive (
    id bigserial PRIMARY KEY,
    receive_no varchar(100) NOT NULL,
    asset_id int8 NOT NULL,
    user_id int8 NOT NULL,
    department_id int8 NOT NULL,
    receive_date timestamptz NOT NULL,
    reason text NOT NULL,
    status int2 NOT NULL,
    approve_by int8 NULL,
    approve_time timestamptz NULL,
    approve_remark text NULL,
    created_by int8 NULL,
    created_at timestamptz NULL,
    updated_by int8 NULL,
    updated_at timestamptz NULL,
    deleted int2 NOT NULL
);
COMMENT ON TABLE asset_receive IS '资产领用申请表';

-- 12. 资产归还确认表
CREATE TABLE IF NOT EXISTS asset_return (
    id bigserial PRIMARY KEY,
    return_no varchar(100) NOT NULL,
    receive_id int8 NOT NULL,
    asset_id int8 NOT NULL,
    user_id int8 NOT NULL,
    return_date timestamptz NOT NULL,
    asset_status int2 NOT NULL,
    remark text NULL,
    confirm_by int8 NOT NULL,
    confirm_time timestamptz NOT NULL,
    created_by int8 NULL,
    created_at timestamptz NULL,
    updated_by int8 NULL,
    updated_at timestamptz NULL,
    deleted int2 NOT NULL
);
COMMENT ON TABLE asset_return IS '资产归还确认表';

-- 13. 资产调拨表
CREATE TABLE IF NOT EXISTS asset_transfer (
    id bigserial PRIMARY KEY,
    transfer_no varchar(100) NOT NULL,
    asset_id int8 NOT NULL,
    out_dept_id int8 NOT NULL,
    in_dept_id int8 NOT NULL,
    out_user_id int8 NOT NULL,
    in_user_id int8 NOT NULL,
    transfer_date timestamptz NOT NULL,
    reason text NOT NULL,
    status int2 NOT NULL,
    approve_by int8 NULL,
    approve_time timestamptz NULL,
    created_by int8 NULL,
    created_at timestamptz NULL,
    updated_by int8 NULL,
    updated_at timestamptz NULL,
    deleted int2 NOT NULL
);
COMMENT ON TABLE asset_transfer IS '资产调拨表';

-- 14. 资产维修表
CREATE TABLE IF NOT EXISTS asset_repair (
    id bigserial PRIMARY KEY,
    repair_no varchar(100) NOT NULL,
    asset_id int8 NOT NULL,
    fault_desc text NOT NULL,
    repair_desc text NULL,
    repair_user_id int8 NULL,
    repair_dept_id int8 NULL,
    repair_file_url text NULL,
    repair_type int2 NOT NULL,
    vendor varchar(255) NULL,
    cost numeric(12,2) NULL,
    apply_date timestamptz NOT NULL,
    repair_date timestamptz NULL,
    finish_date timestamptz NULL,
    status int2 NOT NULL,
    created_by int8 NULL,
    created_at timestamptz NULL,
    updated_by int8 NULL,
    updated_at timestamptz NULL,
    deleted int2 NOT NULL
);
COMMENT ON TABLE asset_repair IS '资产维修表';

-- 15. 资产报废表
CREATE TABLE IF NOT EXISTS asset_scrap (
    id bigserial PRIMARY KEY,
    scrap_no varchar(100) NOT NULL,
    asset_id int8 NOT NULL,
    reason text NOT NULL,
    scrap_date timestamptz NOT NULL,
    status int2 NOT NULL,
    approve_by int8 NULL,
    approve_time timestamptz NULL,
    handle_user int8 NULL,
    created_by int8 NULL,
    created_at timestamptz NULL,
    updated_by int8 NULL,
    updated_at timestamptz NULL,
    deleted int2 NOT NULL
);
COMMENT ON TABLE asset_scrap IS '资产报废表';

-- 16. 资产采购申请表
CREATE TABLE IF NOT EXISTS asset_purchase (
    id bigserial PRIMARY KEY,
    purchase_no varchar(100) NOT NULL,
    asset_name varchar(255) NOT NULL,
    category_id int8 NOT NULL,
    model varchar(255) NULL,
    manufacturer varchar(255) NULL,
    quantity int4 NOT NULL,
    unit_price numeric(12,2) NULL,
    total_price numeric(12,2) NULL,
    apply_user int8 NOT NULL,
    dept_id int8 NOT NULL,
    reason text NOT NULL,
    status int2 NOT NULL,
    supplier varchar(255) NULL,
    purchase_date timestamptz NULL,
    arrive_date timestamptz NULL,
    created_by int8 NULL,
    created_at timestamptz NULL,
    updated_by int8 NULL,
    updated_at timestamptz NULL,
    deleted int2 NOT NULL
);
COMMENT ON TABLE asset_purchase IS '资产采购申请表';