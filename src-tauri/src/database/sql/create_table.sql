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
    created_at timestamp NULL,
    updated_by int8 NULL,
    updated_at timestamp NULL,
    deleted int2 NULL
);
COMMENT ON TABLE asset_category IS '资产分类表';
COMMENT ON COLUMN asset_category.category_name IS '分类名称';
COMMENT ON COLUMN asset_category.asset_type IS '资产类型';
COMMENT ON COLUMN asset_category.parent_id IS '父分类ID';
COMMENT ON COLUMN asset_category.sort IS '排序号';

-- 2. 资产主表
CREATE TABLE IF NOT EXISTS assets (
    id bigserial PRIMARY KEY,
    asset_no varchar(100) NOT NULL,
    asset_type varchar(50) NOT NULL,
    category_id int8 NOT NULL,
    asset_name varchar(255) NOT NULL,
    manufacturer varchar(255) NULL,
    model varchar(255) NULL,
    department_id int8 NOT NULL,
    status int2 NOT NULL,
    purchase_date timestamp NULL,
    purchase_price numeric(12,2) NULL,
    quantity int4 NOT NULL,
    used_quantity int4 NOT NULL,
    expire_date timestamp NULL,
    description text NOT NULL,
    created_by int8 NULL,
    created_at timestamp NULL,
    updated_by int8 NULL,
    updated_at timestamp NULL,
    deleted int2 NULL
);
COMMENT ON TABLE assets IS '资产主表';
COMMENT ON COLUMN assets.asset_no IS '资产编号';
COMMENT ON COLUMN assets.quantity IS '资产数量，硬资产默认1，软资产记录授权总数量';
COMMENT ON COLUMN assets.used_quantity IS '已使用数量，软资产记录已分配授权数';

-- 3. 硬件资产扩展表
CREATE TABLE IF NOT EXISTS hard_assets (
    id bigserial PRIMARY KEY,
    asset_id int8 NOT NULL,
    sn varchar(100) NULL,
    mac_address varchar(100) NULL,
    location varchar(255) NULL,
    maintenance_vendor varchar(255) NULL,
    maintenance_type varchar(100) NULL,
    maintenance_expire_date timestamp NULL,
    hardware_config text NULL,
    use_user_id int8 NULL,
    use_start_date timestamp NULL,
    fault_desc text NULL,
    created_by int8 NULL,
    created_at timestamp NULL,
    updated_by int8 NULL,
    updated_at timestamp NULL,
    deleted int2 NULL
);
COMMENT ON TABLE hard_assets IS '硬件资产扩展表';
COMMENT ON COLUMN hard_assets.asset_id IS '关联资产主表ID';
COMMENT ON COLUMN hard_assets.sn IS '硬件序列号';
COMMENT ON COLUMN hard_assets.hardware_config IS '硬件配置JSON';

-- 4. 软件资产扩展表
CREATE TABLE IF NOT EXISTS soft_assets (
    id bigserial PRIMARY KEY,
    asset_id int8 NOT NULL,
    license_key varchar(255) NULL,
    license_type varchar(100) NULL,
    license_period varchar(100) NULL,
    authorized_scope varchar(255) NULL,
    assigned_user_ids text NULL,
    bind_type varchar(100) NULL,
    bind_info text NULL,
    renew_record text NULL,
    renew_reminder timestamp NULL,
    version varchar(100) NULL,
    download_link varchar(255) NULL,
    authorize_contract text NULL,
    created_by int8 NULL,
    created_at timestamp NULL,
    updated_by int8 NULL,
    updated_at timestamp NULL,
    deleted int2 NULL
);
COMMENT ON TABLE soft_assets IS '软件资产扩展表';
COMMENT ON COLUMN soft_assets.assigned_user_ids IS '已分配用户ID列表 JSON';

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
    created_at timestamp NULL,
    updated_by int8 NULL,
    updated_at timestamp NULL,
    deleted int2 NULL
);
COMMENT ON TABLE sys_user IS '系统用户表';
COMMENT ON COLUMN sys_user.person_code IS '工号';

-- 6. 部门表
CREATE TABLE IF NOT EXISTS department (
    id bigserial PRIMARY KEY,
    department_name varchar(255) NOT NULL,
    parent_id int8 NULL,
    description text NULL,
    created_by int8 NULL,
    created_at timestamp NULL,
    updated_by int8 NULL,
    updated_at timestamp NULL,
    deleted int2 NULL
);
COMMENT ON TABLE department IS '部门表';

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
    created_at timestamp NULL,
    updated_by int8 NULL,
    updated_at timestamp NULL,
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
    created_at timestamp NULL,
    updated_by int8 NULL,
    updated_at timestamp NULL,
    deleted int2 NULL
);
COMMENT ON TABLE sys_role IS '角色表';

-- 9. 用户角色关联表
CREATE TABLE IF NOT EXISTS sys_user_role (
    id bigserial PRIMARY KEY,
    user_id int8 NOT NULL,
    role_id int8 NOT NULL,
    created_by int8 NULL,
    created_at timestamp NULL,
    updated_by int8 NULL,
    updated_at timestamp NULL,
    deleted int2 NULL
);
COMMENT ON TABLE sys_user_role IS '用户角色关联表';

-- 10. 角色菜单关联表
CREATE TABLE IF NOT EXISTS sys_role_menu (
    id bigserial PRIMARY KEY,
    role_id int8 NOT NULL,
    menu_id int8 NOT NULL,
    created_by int8 NULL,
    created_at timestamp NULL,
    updated_by int8 NULL,
    updated_at timestamp NULL,
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
    receive_date timestamp NOT NULL,
    reason text NOT NULL,
    status int2 NOT NULL,
    approve_by int8 NULL,
    approve_time timestamp NULL,
    approve_remark text NULL,
    created_by int8 NULL,
    created_at timestamp NULL,
    updated_by int8 NULL,
    updated_at timestamp NULL,
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
    return_date timestamp NOT NULL,
    asset_status int2 NOT NULL,
    remark text NULL,
    confirm_by int8 NOT NULL,
    confirm_time timestamp NOT NULL,
    created_by int8 NULL,
    created_at timestamp NULL,
    updated_by int8 NULL,
    updated_at timestamp NULL,
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
    transfer_date timestamp NOT NULL,
    reason text NOT NULL,
    status int2 NOT NULL,
    approve_by int8 NULL,
    approve_time timestamp NULL,
    created_by int8 NULL,
    created_at timestamp NULL,
    updated_by int8 NULL,
    updated_at timestamp NULL,
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
    apply_date timestamp NOT NULL,
    repair_date timestamp NULL,
    finish_date timestamp NULL,
    status int2 NOT NULL,
    created_by int8 NULL,
    created_at timestamp NULL,
    updated_by int8 NULL,
    updated_at timestamp NULL,
    deleted int2 NOT NULL
);
COMMENT ON TABLE asset_repair IS '资产维修表';

-- 15. 资产报废表
CREATE TABLE IF NOT EXISTS asset_scrap (
    id bigserial PRIMARY KEY,
    scrap_no varchar(100) NOT NULL,
    asset_id int8 NOT NULL,
    reason text NOT NULL,
    scrap_date timestamp NOT NULL,
    status int2 NOT NULL,
    approve_by int8 NULL,
    approve_time timestamp NULL,
    handle_user int8 NULL,
    created_by int8 NULL,
    created_at timestamp NULL,
    updated_by int8 NULL,
    updated_at timestamp NULL,
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
    purchase_date timestamp NULL,
    arrive_date timestamp NULL,
    created_by int8 NULL,
    created_at timestamp NULL,
    updated_by int8 NULL,
    updated_at timestamp NULL,
    deleted int2 NOT NULL
);
COMMENT ON TABLE asset_purchase IS '资产采购申请表';