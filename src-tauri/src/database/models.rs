use std::str;

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetCategory {
    pub id: i64,
    pub category_name: String,
    pub asset_type: String,
    pub parent_id: i64,
    pub sort: i16,
    pub description: Option<String>,
    pub created_by: Option<i64>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_by: Option<i64>,
    pub updated_at: Option<NaiveDateTime>,
    pub deleted: Option<i16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Assets {
    pub id: i64, //主键，唯一标识一条资产记录
    pub asset_no: String,
    pub asset_type: String,
    pub category_id: i64,
    pub asset_name: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub department_id: i64,
    pub status: i8,
    pub purchase_date: Option<DateTime<Utc>>,
    pub purchase_price: Option<f64>,
    pub quantity: i32,      //资产数量，硬资产默认1，软资产记录授权总数量
    pub used_quantity: i32, //已使用数量，软资产记录已分配授权数
    pub expire_date: Option<DateTime<Utc>>,
    pub description: String,
    pub created_by: Option<i64>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_by: Option<i64>,
    pub updated_at: Option<NaiveDateTime>,
    pub deleted: Option<i16>,
}
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct HardAssets {
    pub id: i64,                                        //主键，唯一标识一条硬资产扩展记录
    pub asset_id: i64, //外键，关联资产主表（assets）id，一对一关联，软删除主表时同步软删除该记录
    pub sn: Option<String>, //硬资产唯一序列号，如电脑、网络设备的SN码，不可重复
    pub mac_address: Option<String>, //硬资产唯一序列号，如电脑、网络设备的SN码，不可重复
    pub location: Option<String>, //资产存放位置，如XX办公楼302室、机房A区01柜
    pub maintenance_vendor: Option<String>, //维保厂商，如联想售后、华为维保，关联维保管理
    pub maintenance_type: Option<String>, //维保方式，取值：上门维保、寄修、远程维保
    pub maintenance_expire_date: Option<NaiveDateTime>, //维保到期日期，用于维保到期提醒，优先级高于主表expire_date
    pub hardware_config: Option<String>, //硬件配置详情，如CPU、内存、硬盘、显卡等，JSON格式存储（如{"cpu":"i7-13700H","memory":"16GB"}）
    pub use_user_id: Option<i64>, //外键，关联用户表（user）id，记录当前使用人，状态为“在用/借用”时必填
    pub use_start_date: Option<NaiveDateTime>, //使用开始日期，状态变更为“在用/借用”时自动记录
    pub fault_desc: Option<String>, //故障描述，状态为“维修”时填写，记录故障详情
    pub created_by: Option<i64>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_by: Option<i64>,
    pub updated_at: Option<NaiveDateTime>,
    pub deleted: Option<i16>,
}
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SoftAssets {
    pub id: i64,                               //主键，唯一标识一条软资产扩展记录
    pub asset_id: i64, //外键，关联资产主表（assets）id，一对一关联，软删除主表时同步软删除该记录
    pub license_key: Option<String>, //软件授权码，记录软件授权信息，如Office 365的25
    pub license_type: Option<String>, //授权类型，取值：单机授权、企业授权、订阅授权等
    pub license_period: Option<String>, //授权期限，记录授权有效期，如1年、永久等
    pub authorized_scope: Option<String>, //授权范围，记录授权使用范围，如个人使用、部门使用、全公司使用等
    pub assigned_user_ids: Option<String>, //已分配用户ID列表，记录已分配授权的用户，JSON格式存储（如[1,2,3]）
    pub bind_type: Option<String>,         //绑定类型，记录授权绑定方式，如设备绑定、用户绑定等
    pub bind_info: Option<String>, //绑定信息，记录授权绑定的具体信息，如绑定设备的SN码或绑定用户的ID
    pub renew_record: Option<String>, //续费记录，记录授权续费历史，JSON格式存储（如[{"renew_date":"2024-01-01","expire_date":"2025-01-01","renew_cost":100}]）
    pub renew_reminder: Option<NaiveDateTime>, //续费提醒日期，记录授权到期前的续费提醒时间
    pub version: Option<String>, //软件版本，记录软件的具体版本信息，如Windows 10 Pro、Office 365等
    pub download_link: Option<String>, //软件下载链接，记录软件的官方下载地址或内部下载地址
    pub authorize_contract: Option<String>, //授权合同信息，记录与软件供应商签订的授权合同详情，如合同编号、签订日期、合同附件链接等
    pub created_by: Option<i64>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_by: Option<i64>,
    pub updated_at: Option<NaiveDateTime>,
    pub deleted: Option<i16>,
}
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SysUser {
    pub id: i64,                           //主键，唯一标识一条用户记录
    pub username: String,                  //用户名，登录系统使用，唯一
    pub passwd: String,                    //密码，登录系统使用，存储加密后的密码
    pub domain: String,                    //域，登录系统使用，记录用户所属域，如本地账户、公司域等
    pub real_name: String,                 //真实姓名，记录用户的真实姓名
    pub email: Option<String>,             //邮箱，记录用户的电子邮件地址
    pub phone: Option<String>,             //电话，记录用户的联系电话号码
    pub department_id: Option<i64>,        //部门ID，外键，关联部门表
    pub status: i8,                        //状态，记录用户的当前状态，如1=正常、0=禁用
    pub nickname: Option<String>,          //昵称，记录用户的昵称或别名
    pub avatar: Option<String>,            //头像，记录用户的头像URL或存储路径
    pub person_id: Option<String>,         //身份证号，记录用户的身份证号码
    pub person_code: Option<String>,       //工号，记录用户的工号或员工编号
    pub super_user_id: Option<i64>, //上级用户ID，外键，关联自身id，记录用户的直接上级领导，顶级用户super_user_id为null
    pub created_by: Option<i64>,    //创建人，记录创建该用户的管理员
    pub created_at: Option<NaiveDateTime>, //创建时间，记录用户的创建时间
    pub updated_by: Option<i64>,    //更新人，记录最后一次修改该用户
    pub updated_at: Option<NaiveDateTime>, //更新时间，记录用户的最后修改时间
    pub deleted: Option<i16>,       //删除标志，记录用户是否被删除，0=未删除，1=已删除，软删除使用
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Department {
    pub id: i64,                           //主键，唯一标识一条部门记录
    pub department_name: String,           //部门名称，记录部门的名称
    pub parent_id: Option<i64>, //父部门ID，外键，关联自身id，实现部门层级关系，顶级部门parent_id为null
    pub description: Option<String>, //描述，记录部门的详细描述信息
    pub created_by: Option<i64>, //创建人，记录创建该部门的管理员
    pub created_at: Option<NaiveDateTime>, //创建时间，记录部门的创建时间
    pub updated_by: Option<i64>, //更新人，记录最后一次修改该部门的管理员
    pub updated_at: Option<NaiveDateTime>, //更新时间，记录部门的最后修改时间
    pub deleted: Option<i16>,   //删除标志，记录部门是否被删除，0=未删除，1=已删除，软删除使用
}

/// 系统菜单 & 权限按钮实体
/// 对应表：sys_menu
/// 菜单类型：1=目录 2=菜单 3=按钮
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SysMenu {
    /// 主键ID
    pub id: i64,

    /// 菜单/按钮名称
    pub menu_name: String,

    /// 父菜单ID（顶级为 null）
    pub parent_id: Option<i64>,

    /// 路由路径（菜单用）
    pub path: Option<String>,

    /// 前端组件路径（菜单用）
    pub component: Option<String>,

    /// 图标
    pub icon: Option<String>,

    /// 排序号（越小越靠前）
    pub order_num: i16,

    /// 是否可见（true=显示，false=隐藏）
    pub visible: bool,

    /// 权限标识（按钮用）
    /// 例如：sys:user:add, sys:user:edit, sys:user:delete
    pub perms: Option<String>,

    /// 菜单类型
    /// 1=目录, 2=菜单, 3=按钮
    pub menu_type: i16,

    /// 是否隐藏按钮（true=隐藏，false=显示）
    /// 专门控制【按钮是否在页面显示】
    pub hidden_button: bool,
    // ============================================================================
    /// 创建人ID
    pub created_by: Option<i64>,

    /// 创建时间
    pub created_at: Option<NaiveDateTime>,

    /// 更新人ID
    pub updated_by: Option<i64>,

    /// 更新时间
    pub updated_at: Option<NaiveDateTime>,

    /// 软删除标志（0=未删除，1=已删除）
    pub deleted: i16,
}
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Role {
    pub id: i64,                           //主键，唯一标识一条角色记录
    pub role_key: String,                  //角色标识，登录系统使用，唯一，如admin、user等
    pub role_name: String,                 //角色名称，记录角色的名称
    pub description: Option<String>,       //描述，记录角色的详细描述信息
    pub created_by: Option<i64>,           //创建人，记录创建该角色的管理员
    pub created_at: Option<NaiveDateTime>, //创建时间，记录角色的创建时间
    pub updated_by: Option<i64>,           //更新人，记录最后一次修改该角色的管理员
    pub updated_at: Option<NaiveDateTime>, //更新时间，记录角色的最后修改时间
    pub deleted: Option<i16>, //删除标志，记录角色是否被删除，0=未删除，1=已删除，软删除使用
}
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UserRole {
    pub id: i64,                           //主键，唯一标识一条用户角色关联记录
    pub user_id: i64,                      //用户ID，外键，关联用户表（sys_user）id
    pub role_id: i64,                      //角色ID，外键，关联角色表（role）id
    pub created_by: Option<i64>,           //创建人，记录创建该关联的管理员
    pub created_at: Option<NaiveDateTime>, //创建时间，记录该关联的创建时间
    pub updated_by: Option<i64>,           //更新人，记录最后一次修改该关联的管理员
    pub updated_at: Option<NaiveDateTime>, //更新时间，记录该关联的最后修改时间
    pub deleted: Option<bool>,             //主键，唯一标识一条角色菜单关联记录
}

// ======================== 【1】资产领用申请表 ========================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetReceive {
    pub id: i64,
    pub receive_no: String,          // 领用单号（唯一）
    pub asset_id: i64,               // 资产ID
    pub user_id: i64,                // 领用人ID
    pub department_id: i64,          // 领用部门
    pub receive_date: DateTime<Utc>, // 领用日期
    pub reason: String,              // 领用原因
    pub status: i8,                  // 状态：0=待审批 1=已同意 2=已驳回 3=已领用 4=已归还
    pub approve_by: Option<i64>,     // 审批人
    pub approve_time: Option<NaiveDateTime>,
    pub approve_remark: Option<String>,
    pub created_by: Option<i64>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_by: Option<i64>,
    pub updated_at: Option<NaiveDateTime>,
    pub deleted: i16,
}

// ======================== 【2】资产归还确认表 ========================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetReturn {
    pub id: i64,
    pub return_no: String, // 归还单号
    pub receive_id: i64,   // 关联领用单ID
    pub asset_id: i64,
    pub user_id: i64,               // 归还人
    pub return_date: DateTime<Utc>, // 归还日期
    pub asset_status: i8,           // 归还时资产状态：0=正常 1=损坏 2=故障
    pub remark: Option<String>,     // 归还备注
    pub confirm_by: i64,            // 确认人
    pub confirm_time: DateTime<Utc>,
    pub created_by: Option<i64>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_by: Option<i64>,
    pub updated_at: Option<NaiveDateTime>,
    pub deleted: i16,
}

// ======================== 【3】资产调拨表 ========================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetTransfer {
    pub id: i64,
    pub transfer_no: String, // 调拨单号
    pub asset_id: i64,
    pub out_dept_id: i64, // 调出部门
    pub in_dept_id: i64,  // 调入部门
    pub out_user_id: i64, // 调出人
    pub in_user_id: i64,  // 调入人
    pub transfer_date: DateTime<Utc>,
    pub reason: String, // 调拨原因
    pub status: i8,     // 0=待审批 1=已调拨 2=已驳回
    pub approve_by: Option<i64>,
    pub approve_time: Option<NaiveDateTime>,
    pub created_by: Option<i64>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_by: Option<i64>,
    pub updated_at: Option<NaiveDateTime>,
    pub deleted: i16,
}

// ======================== 【4】资产维修表 ========================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetRepair {
    pub id: i64,
    pub repair_no: String, // 维修单号
    pub asset_id: i64,
    pub fault_desc: String,                 // 故障描述
    pub repair_desc: Option<String>,        // 维修描述
    pub repair_user_id: Option<i64>,        // 维修人ID
    pub repair_dept_id: Option<i64>,        // 维修部门ID
    pub repair_file_url: Option<String>, // 维修相关附件URL，记录维修过程中产生的相关文件，如维修报告、照片等
    pub repair_type: i8,                 // 0=送修 1=上门 2=远程
    pub vendor: Option<String>,          // 维修商
    pub cost: Option<f64>,               // 维修费用
    pub apply_date: DateTime<Utc>,       // 申请日期
    pub repair_date: Option<NaiveDateTime>, // 维修日期
    pub finish_date: Option<NaiveDateTime>, // 完成日期
    pub status: i8,                      // 0=待维修 1=维修中 2=已完成 3=无法维修
    pub created_by: Option<i64>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_by: Option<i64>,
    pub updated_at: Option<NaiveDateTime>,
    pub deleted: i16,
}

// ======================== 【5】资产报废表 ========================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetScrap {
    pub id: i64,
    pub scrap_no: String, // 报废单号
    pub asset_id: i64,
    pub reason: String, // 报废原因
    pub scrap_date: DateTime<Utc>,
    pub status: i8, // 0=待审批 1=已批准 2=已驳回 3=已报废
    pub approve_by: Option<i64>,
    pub approve_time: Option<NaiveDateTime>,
    pub handle_user: Option<i64>, // 处理人
    pub created_by: Option<i64>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_by: Option<i64>,
    pub updated_at: Option<NaiveDateTime>,
    pub deleted: i16,
}

// ======================== 【6】资产采购申请表 ========================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetPurchase {
    pub id: i64,
    pub purchase_no: String, // 采购单号
    pub asset_name: String,  // 采购资产名称
    pub category_id: i64,    // 分类
    pub model: Option<String>,
    pub manufacturer: Option<String>,
    pub quantity: i32,                        // 采购数量
    pub unit_price: Option<f64>,              // 单价
    pub total_price: Option<f64>,             // 总价
    pub apply_user: i64,                      // 申请人
    pub dept_id: i64,                         // 申请部门
    pub reason: String,                       // 采购原因
    pub status: i8,                           // 0=待审批 1=采购中 2=已完成 3=已驳回
    pub supplier: Option<String>,             // 供应商
    pub purchase_date: Option<NaiveDateTime>, //购买时间
    pub arrive_date: Option<NaiveDateTime>,   //预计到货时间
    pub created_by: Option<i64>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_by: Option<i64>,
    pub updated_at: Option<NaiveDateTime>,
    pub deleted: i16,
}

// ======================== 【7】通用审批记录表（所有流程共用） ========================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetApproval {
    pub id: i64,
    pub biz_type: i8,       // 业务类型：1=领用 2=归还 3=调拨 4=维修 5=报废 6=采购
    pub biz_id: i64,        // 业务ID
    pub step: i16,          // 审批步骤
    pub approver_id: i64,   // 审批人ID
    pub approve_status: i8, // 0=待审 1=同意 2=驳回
    pub remark: Option<String>,
    pub approve_time: Option<NaiveDateTime>,
    pub created_at: Option<NaiveDateTime>,
}
