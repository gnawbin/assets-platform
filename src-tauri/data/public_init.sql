CREATE TABLE casbin_rule (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    ptype VARCHAR(18) NOT NULL,
    v0 VARCHAR(125) NOT NULL,
    v1 VARCHAR(125) NOT NULL,
    v2 VARCHAR(125) NOT NULL,
    v3 VARCHAR(125) NOT NULL,
    v4 VARCHAR(125) NOT NULL,
    v5 VARCHAR(125) NOT NULL,
    UNIQUE (ptype, v0, v1, v2, v3, v4, v5)
);

-- sys_menu
CREATE TABLE sys_menu (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    menu_type TEXT NOT NULL,
    menu_name TEXT NOT NULL,
    icon_type INTEGER NULL,
    icon TEXT NULL,
    route_name TEXT NOT NULL,
    route_path TEXT NOT NULL,
    component TEXT NOT NULL,
    path_param TEXT NULL,
    status TEXT NOT NULL,
    active_menu TEXT NULL,
    hide_in_menu BOOLEAN NULL,
    pid TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    i18n_key TEXT NULL,
    keep_alive BOOLEAN NULL,
    constant BOOLEAN NOT NULL,
    href TEXT NULL,
    multi_tab BOOLEAN NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP NOT NULL,
    created_by TEXT NOT NULL,
    updated_at TIMESTAMP NULL,
    updated_by TEXT NULL,
    UNIQUE (route_name)
);

-- asset_categories
CREATE TABLE IF NOT EXISTS asset_categories (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    category_name TEXT NOT NULL,
    asset_type TEXT NOT NULL,
    parent_id INTEGER DEFAULT 0 NOT NULL,
    sort INTEGER DEFAULT 0 NOT NULL,
    description TEXT DEFAULT '' NOT NULL,
    created_by INTEGER DEFAULT 0 NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_by INTEGER DEFAULT 0 NOT NULL,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO asset_categories (category_name, asset_type, parent_id, sort, description) VALUES ('服务器', '硬件资产', 0, 1, '包括机架式服务器、塔式服务器等');
INSERT INTO asset_categories (category_name, asset_type, parent_id, sort, description) VALUES ('网络设备', '硬件资产', 0, 2, '包括交换机、路由器、防火墙等');
INSERT INTO asset_categories (category_name, asset_type, parent_id, sort, description) VALUES ('办公设备', '硬件资产', 0, 3, '包括台式机、笔记本、打印机等');
INSERT INTO asset_categories (category_name, asset_type, parent_id, sort, description) VALUES ('存储设备', '硬件资产', 0, 4, '包括磁盘阵列、NAS、SAN等');
INSERT INTO asset_categories (category_name, asset_type, parent_id, sort, description) VALUES ('操作系统', '软件资产', 0, 5, '包括 Windows、Linux、macOS 等');
INSERT INTO asset_categories (category_name, asset_type, parent_id, sort, description) VALUES ('办公软件', '软件资产', 0, 6, '包括 Office、WPS、Adobe 等');
INSERT INTO asset_categories (category_name, asset_type, parent_id, sort, description) VALUES ('数据库软件', '软件资产', 0, 7, '包括 MySQL、PostgreSQL、Oracle 等');
INSERT INTO asset_categories (category_name, asset_type, parent_id, sort, description) VALUES ('安全软件', '软件资产', 0, 8, '包括杀毒软件、防火墙软件等');
