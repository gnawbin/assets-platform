-- ==============================
-- 租户初始数据（执行前替换 {schema} 为实际 schema 名）
-- ==============================

-- 1. 默认角色（写入 public.sys_role）
INSERT INTO
    public.sys_role (
        id,
        role_key,
        role_name,
        description,
        is_super_admin,
        tenant_id,
        created_by,
        created_at,
        deleted
    )
SELECT 1, 'admin', '超级管理员', '超级管理员角色，拥有所有权限', true, NULL, 1, NOW(), 0
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_role
        WHERE
            id = 1
    );

-- 3. 默认角色菜单关联（admin 角色 → 所有菜单，写入 public.sys_role_menu）
-- 注意：用户角色关联（sys_user_role）由 insert_tenant 函数动态创建，不在 SQL 中硬编码
INSERT INTO
    public.sys_role_menu (
        id,
        role_id,
        menu_id,
        created_by,
        created_at,
        deleted
    )
SELECT row_number() OVER (
        ORDER BY m.id
    ) + 1000, 1, m.id, 1, NOW(), 0
FROM public.sys_menu m
WHERE
    NOT EXISTS (
        SELECT 1
        FROM public.sys_role_menu rm
        WHERE
            rm.role_id = 1
            AND rm.menu_id = m.id
    );

-- ==============================
-- 5. 资产分类种子数据
-- ==============================
-- 固定资产一级分类（parent_id = 0）
INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 100, '计算机设备', 'fixed', 0, 1, '计算机及相关设备', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 100);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 110, '网络设备', 'fixed', 0, 2, '网络通信相关设备', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 110);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 120, '外设设备', 'fixed', 0, 3, '计算机外围设备', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 120);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 130, '移动设备', 'fixed', 0, 4, '移动通信设备', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 130);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 140, '机房硬件', 'fixed', 0, 5, '数据中心机房设备', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 140);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 150, '办公设备', 'fixed', 0, 6, '日常办公设备', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 150);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 199, '其他固定资产', 'fixed', 0, 99, '其他未分类的固定资产', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 199);

-- 固定资产二级分类 - 计算机设备（parent_id = 100）
INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 101, '服务器', 'fixed', 100, 1, '机架式服务器、塔式服务器、刀片服务器等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 101);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 102, '台式计算机', 'fixed', 100, 2, '办公台式机、工作站等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 102);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 103, '笔记本电脑', 'fixed', 100, 3, '办公笔记本、移动工作站等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 103);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 104, '平板电脑', 'fixed', 100, 4, 'iPad、安卓平板等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 104);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 105, '显示器', 'fixed', 100, 5, '办公显示器、专业显示器等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 105);

-- 固定资产二级分类 - 网络设备（parent_id = 110）
INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 111, '交换机', 'fixed', 110, 1, '接入层交换机、汇聚层交换机、核心交换机等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 111);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 112, '路由器', 'fixed', 110, 2, '企业路由器、边缘路由器等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 112);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 113, '防火墙', 'fixed', 110, 3, '硬件防火墙、UTM 设备等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 113);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 114, '无线AP', 'fixed', 110, 4, '企业级无线接入点', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 114);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 115, '负载均衡', 'fixed', 110, 5, '硬件负载均衡设备', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 115);

-- 固定资产二级分类 - 外设设备（parent_id = 120）
INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 121, '打印机', 'fixed', 120, 1, '激光打印机、喷墨打印机、针式打印机等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 121);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 122, '扫描仪', 'fixed', 120, 2, '文档扫描仪、条码扫描仪等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 122);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 123, '投影仪', 'fixed', 120, 3, '办公投影仪、会议投影仪等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 123);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 124, '复印机', 'fixed', 120, 4, '数码复合机等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 124);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 125, '多功能一体机', 'fixed', 120, 5, '打印/复印/扫描一体机', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 125);

-- 固定资产二级分类 - 移动设备（parent_id = 130）
INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 131, '手机', 'fixed', 130, 1, '工作手机', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 131);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 132, '对讲机', 'fixed', 130, 2, '无线对讲设备', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 132);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 133, '移动热点', 'fixed', 130, 3, '4G/5G 移动路由器', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 133);

-- 固定资产二级分类 - 机房硬件（parent_id = 140）
INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 141, '存储设备', 'fixed', 140, 1, '磁盘阵列、NAS、SAN 存储等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 141);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 142, 'UPS 电源', 'fixed', 140, 2, '不间断电源设备', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 142);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 143, '精密空调', 'fixed', 140, 3, '机房专用空调', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 143);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 144, '机柜', 'fixed', 140, 4, '标准服务器机柜、网络机柜等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 144);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 145, '配线架', 'fixed', 140, 5, '光纤配线架、网络配线架等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 145);

-- 固定资产二级分类 - 办公设备（parent_id = 150）
INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 151, '电话机', 'fixed', 150, 1, '办公座机', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 151);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 152, '考勤机', 'fixed', 150, 2, '指纹/人脸考勤设备', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 152);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 153, '门禁设备', 'fixed', 150, 3, '门禁控制器、读卡器等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 153);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 154, '监控设备', 'fixed', 150, 4, '摄像头、NVR 等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 154);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 155, '会议设备', 'fixed', 150, 5, '会议话筒、摄像头、音响等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 155);

-- ==============================
-- 无形资产分类
-- ==============================
-- 无形资产一级分类（parent_id = 0）
INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 200, '软件授权', 'intangible', 0, 1, '软件许可证及授权', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 200);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 210, '知识产权', 'intangible', 0, 2, '企业知识产权资产', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 210);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 220, '特许经营权', 'intangible', 0, 3, '特许经营及许可资质', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 220);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 299, '其他无形资产', 'intangible', 0, 99, '其他未分类的无形资产', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 299);

-- 无形资产二级分类 - 软件授权（parent_id = 200）
INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 201, '操作系统', 'intangible', 200, 1, 'Windows、Linux、macOS 等操作系统授权', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 201);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 202, '办公软件', 'intangible', 200, 2, 'Office、WPS、Adobe 等办公套件授权', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 202);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 203, '数据库软件', 'intangible', 200, 3, 'Oracle、SQL Server、MySQL、PostgreSQL 等数据库授权', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 203);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 204, '开发工具', 'intangible', 200, 4, 'IDE、编译器、版本管理工具等开发软件授权', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 204);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 205, '设计软件', 'intangible', 200, 5, 'AutoCAD、SolidWorks、Photoshop 等设计软件授权', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 205);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 206, '安全软件', 'intangible', 200, 6, '杀毒软件、终端安全、防火墙软件等安全软件授权', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 206);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 207, '虚拟化软件', 'intangible', 200, 7, 'VMware、Hyper-V、KVM 等虚拟化平台授权', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 207);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 208, '中间件', 'intangible', 200, 8, 'WebLogic、Tomcat、Nginx 等中间件授权', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 208);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 209, '云服务', 'intangible', 200, 9, 'SaaS、PaaS 订阅服务', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 209);

-- 无形资产二级分类 - 知识产权（parent_id = 210）
INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 211, '专利', 'intangible', 210, 1, '发明专利、实用新型专利、外观设计专利', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 211);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 212, '商标', 'intangible', 210, 2, '注册商标', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 212);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 213, '著作权', 'intangible', 210, 3, '软件著作权、作品著作权', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 213);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 214, '域名', 'intangible', 210, 4, '企业域名', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 214);

-- 无形资产二级分类 - 特许经营权（parent_id = 220）
INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 221, '经营许可', 'intangible', 220, 1, '行业经营许可证', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 221);

INSERT INTO {schema}.asset_category (id, category_name, asset_type, parent_id, sort, description, created_by, created_at, deleted)
SELECT 222, '资质认证', 'intangible', 220, 2, 'ISO 认证、高新技术企业认证等', 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.asset_category WHERE id = 222);

-- ==============================
-- 单据编号规则默认配置
-- ==============================
INSERT INTO {schema}.doc_numbering_rule (id, biz_type, biz_name, prefix, date_format, serial_length, separator, reset_mode, sample_output, is_active, created_by, created_at, deleted)
SELECT 1, 'asset', '资产编号', 'ZC', 'yyyyMMdd', 4, '-', 'never', 'ZC-20260715-0001', true, 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.doc_numbering_rule WHERE biz_type = 'asset');

INSERT INTO {schema}.doc_numbering_rule (id, biz_type, biz_name, prefix, date_format, serial_length, separator, reset_mode, sample_output, is_active, created_by, created_at, deleted)
SELECT 2, 'receive', '领用单号', 'LY', 'yyyyMMdd', 4, '-', 'yearly', 'LY-20260715-0001', true, 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.doc_numbering_rule WHERE biz_type = 'receive');

INSERT INTO {schema}.doc_numbering_rule (id, biz_type, biz_name, prefix, date_format, serial_length, separator, reset_mode, sample_output, is_active, created_by, created_at, deleted)
SELECT 3, 'return', '归还单号', 'GH', 'yyyyMMdd', 4, '-', 'yearly', 'GH-20260715-0001', true, 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.doc_numbering_rule WHERE biz_type = 'return');

INSERT INTO {schema}.doc_numbering_rule (id, biz_type, biz_name, prefix, date_format, serial_length, separator, reset_mode, sample_output, is_active, created_by, created_at, deleted)
SELECT 4, 'transfer', '调拨单号', 'DB', 'yyyyMMdd', 4, '-', 'yearly', 'DB-20260715-0001', true, 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.doc_numbering_rule WHERE biz_type = 'transfer');

INSERT INTO {schema}.doc_numbering_rule (id, biz_type, biz_name, prefix, date_format, serial_length, separator, reset_mode, sample_output, is_active, created_by, created_at, deleted)
SELECT 5, 'repair', '维修单号', 'WX', 'yyyyMMdd', 4, '-', 'yearly', 'WX-20260715-0001', true, 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.doc_numbering_rule WHERE biz_type = 'repair');

INSERT INTO {schema}.doc_numbering_rule (id, biz_type, biz_name, prefix, date_format, serial_length, separator, reset_mode, sample_output, is_active, created_by, created_at, deleted)
SELECT 6, 'scrap', '报废单号', 'BF', 'yyyyMMdd', 4, '-', 'yearly', 'BF-20260715-0001', true, 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.doc_numbering_rule WHERE biz_type = 'scrap');

INSERT INTO {schema}.doc_numbering_rule (id, biz_type, biz_name, prefix, date_format, serial_length, separator, reset_mode, sample_output, is_active, created_by, created_at, deleted)
SELECT 7, 'purchase', '采购单号', 'CG', 'yyyyMMdd', 4, '-', 'yearly', 'CG-20260715-0001', true, 1, NOW(), 0
WHERE NOT EXISTS (SELECT 1 FROM {schema}.doc_numbering_rule WHERE biz_type = 'purchase');