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