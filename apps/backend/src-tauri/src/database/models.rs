use std::str;

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};
use sqlx::FromRow;
use std::fmt;

// 把 i64 序列化为字符串的辅助函数（防止前端 JS 精度丢失）
pub fn i64_to_string<S>(value: &i64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}

// 把字符串反序列化为 i64 的辅助函数
pub fn i64_from_string<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    struct I64Visitor;

    impl<'de> Visitor<'de> for I64Visitor {
        type Value = i64;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or number representing an i64")
        }

        fn visit_i64<E: de::Error>(self, value: i64) -> Result<i64, E> {
            Ok(value)
        }

        fn visit_u64<E: de::Error>(self, value: u64) -> Result<i64, E> {
            Ok(value as i64)
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<i64, E> {
            value.parse::<i64>().map_err(de::Error::custom)
        }
    }

    deserializer.deserialize_any(I64Visitor)
}

// 处理 Option<i64> 的辅助函数
pub fn opt_i64_to_string<S>(value: &Option<i64>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(v) => serializer.serialize_str(&v.to_string()),
        None => serializer.serialize_none(),
    }
}

// 把 Vec<string> 反序列化为 Vec<i64> 的辅助函数
pub fn vec_i64_from_string<'de, D>(deserializer: D) -> Result<Vec<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    struct VecI64Visitor;

    impl<'de> Visitor<'de> for VecI64Visitor {
        type Value = Vec<i64>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an array of strings or numbers")
        }

        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<i64>, A::Error> {
            let mut vec = Vec::new();
            while let Some(value) = seq.next_element::<serde_json::Value>()? {
                match value {
                    serde_json::Value::String(s) => {
                        let n = s.parse::<i64>().map_err(de::Error::custom)?;
                        vec.push(n);
                    }
                    serde_json::Value::Number(n) => {
                        let n = n
                            .as_i64()
                            .ok_or_else(|| de::Error::custom("expected i64 number"))?;
                        vec.push(n);
                    }
                    _ => return Err(de::Error::custom("expected string or number")),
                }
            }
            Ok(vec)
        }
    }

    deserializer.deserialize_any(VecI64Visitor)
}

// 把 Option<string> 反序列化为 Option<i64> 的辅助函数
pub fn opt_i64_from_string<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptI64Visitor;

    impl<'de> Visitor<'de> for OptI64Visitor {
        type Value = Option<i64>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string, number, or null representing an optional i64")
        }

        fn visit_none<E: de::Error>(self) -> Result<Option<i64>, E> {
            Ok(None)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Option<i64>, E> {
            Ok(None)
        }

        fn visit_i64<E: de::Error>(self, value: i64) -> Result<Option<i64>, E> {
            Ok(Some(value))
        }

        fn visit_u64<E: de::Error>(self, value: u64) -> Result<Option<i64>, E> {
            Ok(Some(value as i64))
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Option<i64>, E> {
            if value.is_empty() {
                Ok(None)
            } else {
                value.parse::<i64>().map(Some).map_err(de::Error::custom)
            }
        }
    }

    deserializer.deserialize_any(OptI64Visitor)
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetCategory {
    #[serde(serialize_with = "i64_to_string", deserialize_with = "i64_from_string")]
    pub id: i64,
    pub category_name: String,
    pub asset_type: String,
    #[serde(default)]
    #[serde(
        serialize_with = "opt_i64_to_string",
        deserialize_with = "opt_i64_from_string"
    )]
    pub parent_id: Option<i64>,
    pub sort: i16,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    #[serde(
        serialize_with = "opt_i64_to_string",
        deserialize_with = "opt_i64_from_string"
    )]
    pub created_by: Option<i64>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    #[serde(
        serialize_with = "opt_i64_to_string",
        deserialize_with = "opt_i64_from_string"
    )]
    pub updated_by: Option<i64>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub deleted: Option<i16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct HardAssets {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64, //主键，唯一标识一条硬资产扩展记录
    #[serde(serialize_with = "i64_to_string")]
    pub asset_id: i64, //外键，关联资产主表（assets）id，一对一关联，软删除主表时同步软删除该记录
    pub sn: Option<String>, //硬资产唯一序列号，如电脑、网络设备的SN码，不可重复
    pub mac_address: Option<String>, //硬资产唯一序列号，如电脑、网络设备的SN码，不可重复
    pub location: Option<String>, //资产存放位置，如XX办公楼302室、机房A区01柜
    pub maintenance_vendor: Option<String>, //维保厂商，如联想售后、华为维保，关联维保管理
    pub maintenance_type: Option<String>, //维保方式，取值：上门维保、寄修、远程维保
    pub maintenance_expire_date: Option<NaiveDateTime>, //维保到期日期，用于维保到期提醒，优先级高于主表expire_date
    pub hardware_config: Option<String>, //硬件配置详情，如CPU、内存、硬盘、显卡等，JSON格式存储（如{"cpu":"i7-13700H","memory":"16GB"}）
    pub use_start_date: Option<NaiveDateTime>, //使用开始日期，状态变更为“在用/借用”时自动记录
    pub fault_desc: Option<String>,      //故障描述，状态为“维修”时填写，记录故障详情
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: Option<i16>,
}
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct IntangibleAssets {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64, //主键，唯一标识一条软资产扩展记录
    #[serde(serialize_with = "i64_to_string")]
    pub asset_id: i64, //外键，关联资产主表（assets）id，一对一关联，软删除主表时同步软删除该记录
    pub intangible_type: String, //无形资产类型：software/patent/trademark/copyright/franchise
    pub register_no: Option<String>, //注册号，软件著作权登记号、专利号、商标注册号等，无形资产特有
    pub register_owner: Option<String>, //权利人，无形资产特有，记录软件著作权的著作权人、专利的专利权人等
    pub register_date: Option<NaiveDateTime>, //注册日期，无形资产特有
    pub valid_start_date: Option<NaiveDateTime>, //生效开始日期，无形资产特有
    pub valid_end_date: Option<NaiveDateTime>, //有效截止日期，无形资产特有
    pub right_status: Option<String>, //权利状态，无形资产特有，记录软件著作权的权利状态、专利的专利权状态等
    pub license_key: Option<String>,  //许可证密钥，软件资产特有，记录软件授权的许可证密钥
    pub license_type: Option<String>, //许可证类型，软件资产特有，取值：permanent/subscription/device/user
    pub authorized_scope: Option<String>, //授权范围，软件资产特有，记录软件授权的范围，如授权给哪个部门、哪个用户等
    pub bind_type: Option<String>, //绑定类型，软件资产特有，取值：设备/用户/IP，记录软件授权的绑定方式
    pub bind_info: Option<String>, //绑定信息，软件资产特有，记录软件授权的绑定详情，如绑定的设备ID、用户ID或IP地址等
    pub version: Option<String>,   //版本号，软件资产特有，记录软件的版本信息
    pub download_link: Option<String>, //下载地址，软件资产特有，记录软件下载链接或存储路径
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: Option<i16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SysUser {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64, //主键，唯一标识一条用户记录
    pub username: String,       //用户名，登录系统使用，唯一
    pub passwd: String,         //密码，登录系统使用，存储加密后的密码
    pub domain: Option<String>, //域，登录系统使用，记录用户所属域，如本地账户、公司域等
    pub real_name: String,      //真实姓名，记录用户的真实姓名
    pub email: Option<String>,  //邮箱，记录用户的电子邮件地址
    pub phone: Option<String>,  //电话，记录用户的联系电话号码
    #[serde(serialize_with = "opt_i64_to_string")]
    pub department_id: Option<i64>, //部门ID，外键，关联部门表
    pub is_super_admin: bool,   //是否超级管理员，超级管理员可以管理所有租户
    pub status: i16,            //状态，记录用户的当前状态，如1=正常、0=禁用
    pub nickname: Option<String>, //昵称，记录用户的昵称或别名
    pub avatar: Option<String>, //头像
    pub person_id: Option<String>, //身份证号，记录用户的身份证号码
    pub person_code: Option<String>, //工号，记录用户的工号或员工编号
    #[serde(serialize_with = "opt_i64_to_string")]
    pub super_user_id: Option<i64>, //上级用户ID，外键，关联自身id，记录用户的直接上级领导，顶级用户super_user_id为null
    #[serde(serialize_with = "opt_i64_to_string")]
    pub tenant_id: Option<i64>, //所属租户ID，外键，关联sys_tenant表
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>, //创建人，记录创建该用户的管理员
    pub created_at: Option<DateTime<Utc>>, //创建时间，记录用户的创建时间
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>, //更新人，记录最后一次修改该用户
    pub updated_at: Option<DateTime<Utc>>, //更新时间，记录用户的最后修改时间
    pub deleted: Option<i16>, //删除标志，记录用户是否被删除，0=未删除，1=已删除，软删除使用
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Department {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64, //主键,唯一标识一条部门记录
    #[serde(serialize_with = "i64_to_string")]
    pub tenant_id: i64, //所属租户ID，外键，关联sys_tenant表
    pub department_name: String, //部门名称,记录部门的名称
    #[serde(serialize_with = "opt_i64_to_string")]
    pub parent_id: Option<i64>, //父部门ID，外键，关联自身id，实现部门层级关系，顶级部门parent_id为null
    pub description: Option<String>, //描述，记录部门的详细描述信息
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>, //创建人，记录创建该部门的管理员
    pub created_at: Option<DateTime<Utc>>, //创建时间，记录部门的创建时间
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>, //更新人，记录最后一次修改该部门的管理员
    pub updated_at: Option<DateTime<Utc>>, //更新时间，记录部门的最后修改时间
    pub deleted: Option<i16>,        //删除标志，记录部门是否被删除，0=未删除，1=已删除，软删除使用
}

/// 租户配置实体
/// 对应表：public.sys_tenant
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SysTenant {
    #[serde(serialize_with = "i64_to_string", deserialize_with = "i64_from_string")]
    pub id: i64,
    pub tenant_name: String,
    #[serde(
        serialize_with = "opt_i64_to_string",
        deserialize_with = "opt_i64_from_string"
    )]
    pub parent_id: Option<i64>,
    pub is_leaf: bool,
    pub schema_name: Option<String>,
    pub enable: bool,
    pub create_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// 系统菜单 & 权限按钮实体
/// 对应表：sys_menu
/// 菜单类型：1=目录 2=菜单 3=按钮
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SysMenu {
    /// 主键ID
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,

    /// 菜单/按钮名称
    pub menu_name: String,

    /// 父菜单ID（顶级为 null）
    #[serde(serialize_with = "opt_i64_to_string")]
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
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>,

    /// 创建时间
    pub created_at: Option<DateTime<Utc>>,

    /// 更新人ID
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>,

    /// 更新时间
    pub updated_at: Option<DateTime<Utc>>,

    /// 软删除标志（0=未删除，1=已删除）
    pub deleted: i16,
}
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Role {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64, //主键，唯一标识一条角色记录
    pub role_key: String,  //角色标识，登录系统使用，唯一，如admin、user等
    pub role_name: String, //角色名称，记录角色的名称
    pub description: Option<String>, //描述，记录角色的详细描述信息
    pub is_super_admin: bool, //是否超级管理员角色
    #[serde(
        serialize_with = "opt_i64_to_string",
        deserialize_with = "opt_i64_from_string"
    )]
    pub tenant_id: Option<i64>, //所属租户ID（超级管理员角色为空）
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>, //创建人，记录创建该角色的管理员
    pub created_at: Option<DateTime<Utc>>, //创建时间，记录角色的创建时间
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>, //更新人，记录最后一次修改该角色的管理员
    pub updated_at: Option<DateTime<Utc>>, //更新时间，记录角色的最后修改时间
    pub deleted: Option<i16>, //删除标志，记录角色是否被删除，0=未删除，1=已删除，软删除使用
}
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UserRole {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64, //主键，唯一标识一条用户角色关联记录
    #[serde(serialize_with = "i64_to_string")]
    pub user_id: i64, //用户ID，外键，关联用户表（sys_user）id
    #[serde(serialize_with = "i64_to_string")]
    pub role_id: i64, //角色ID，外键，关联角色表（role）id
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>, //创建人，记录创建该关联的管理员
    pub created_at: Option<DateTime<Utc>>, //创建时间，记录该关联的创建时间
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>, //更新人，记录最后一次修改该关联的管理员
    pub updated_at: Option<DateTime<Utc>>, //更新时间，记录该关联的最后修改时间
    pub deleted: Option<i16>,              //删除标志
}

/// 角色菜单关联表
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct RoleMenu {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    #[serde(serialize_with = "i64_to_string")]
    pub role_id: i64,
    #[serde(serialize_with = "i64_to_string")]
    pub menu_id: i64,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: Option<i16>,
}
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetDocuments {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    #[serde(serialize_with = "i64_to_string")]
    pub asset_id: i64, //外键，关联资产主表（assets）id，一对多关联，软删除主表时同步软删除该记录
    pub doc_type: String,                      //文档类型，如合同、发票、保修单等
    pub doc_name: String,                      //文档名称，记录文档的名称或标题
    pub doc_no: String,                        //文档编号
    pub party_a: String,                       //甲方
    pub party_b: String,                       //乙方
    pub sign_date: Option<NaiveDateTime>,      //签订日期
    pub effective_date: Option<NaiveDateTime>, //生效日期
    pub expire_date: Option<NaiveDateTime>,    //到期日期
    pub file_path: String,                     //文件存储路径
    pub file_name: String,                     //文件原名
    #[serde(serialize_with = "i64_to_string")]
    pub file_size: i64, //文件大小（字节）
    pub remark: Option<String>,                //备注
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>, //创建人，记录创建该文档的管理员
    pub created_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>, //更新人，记录最后一次修改该文档的管理员
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: Option<i16>, //删除标志，记录文档是否被删除，0=未删除，1=已删除，软删除使用
}
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetKnowledge {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    #[serde(
        serialize_with = "opt_i64_to_string",
        deserialize_with = "opt_i64_from_string"
    )]
    pub asset_id: Option<i64>, //关联资产ID（可选，知识条目可不关联资产）
    pub doc_source: String, //数据来源：manual/asset/hardware/intangible/document
    pub knowledge_type: String, //知识类型：basic/contract/hardware/intangible
    pub title: String,      //知识标题
    pub content: String,    //知识正文（用于向量化 + 微调）
    pub chunk_index: i32,   //文本分块序号
    pub vector_data: Option<Vec<f32>>, //向量数据（Embedding模型输出）

    // 权限控制（对接OPA）
    pub permission_level: String,   //权限等级：public/internal/secret
    pub owner_type: Option<String>, //归属类型：user/dept/role
    #[serde(serialize_with = "opt_i64_to_string")]
    pub owner_id: Option<i64>, //归属人/部门/角色ID

    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}

/// 知识树节点（AIFlowy 风格树形导航）
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct KnowledgeTree {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    #[serde(
        serialize_with = "opt_i64_to_string",
        deserialize_with = "opt_i64_from_string"
    )]
    pub knowledge_id: Option<i64>, // 关联知识条目ID（folder 类型可为空）
    #[serde(
        serialize_with = "opt_i64_to_string",
        deserialize_with = "opt_i64_from_string"
    )]
    pub parent_id: Option<i64>, // 父节点ID（顶级节点为空）
    pub node_type: String,    // 节点类型：folder/document/link
    pub title: String,        // 节点显示名称
    pub icon: Option<String>, // 自定义图标
    pub sort_order: i32,      // 同级排序号
    pub is_expanded: bool,    // 是否展开
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}

/// 知识树节点（带子节点，用于前端渲染）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeTreeNode {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    #[serde(
        serialize_with = "opt_i64_to_string",
        deserialize_with = "opt_i64_from_string"
    )]
    pub knowledge_id: Option<i64>,
    #[serde(
        serialize_with = "opt_i64_to_string",
        deserialize_with = "opt_i64_from_string"
    )]
    pub parent_id: Option<i64>,
    pub node_type: String,
    pub title: String,
    pub icon: Option<String>,
    pub sort_order: i32,
    pub is_expanded: bool,
    pub children: Vec<KnowledgeTreeNode>,
}

// ======================== 【1】资产领用申请表 ========================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetReceive {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    pub receive_no: String, // 领用单号（唯一）
    #[serde(serialize_with = "i64_to_string")]
    pub asset_id: i64, // 资产ID
    #[serde(serialize_with = "i64_to_string")]
    pub user_id: i64, // 领用人ID
    #[serde(serialize_with = "i64_to_string")]
    pub department_id: i64, // 领用部门
    pub receive_date: DateTime<Utc>, // 领用日期
    pub reason: String,     // 领用原因
    pub status: i8,         // 状态：0=待审批 1=已同意 2=已驳回 3=已领用 4=已归还
    #[serde(serialize_with = "opt_i64_to_string")]
    pub approve_by: Option<i64>, // 审批人
    pub approve_time: Option<DateTime<Utc>>,
    pub approve_remark: Option<String>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}

// ======================== 【2】资产归还确认表 ========================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetReturn {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    pub return_no: String, // 归还单号
    #[serde(serialize_with = "i64_to_string")]
    pub receive_id: i64, // 关联领用单ID
    #[serde(serialize_with = "i64_to_string")]
    pub asset_id: i64,
    #[serde(serialize_with = "i64_to_string")]
    pub user_id: i64, // 归还人
    pub return_date: DateTime<Utc>, // 归还日期
    pub asset_status: i8,           // 归还时资产状态：0=正常 1=损坏 2=故障
    pub remark: Option<String>,     // 归还备注
    #[serde(serialize_with = "i64_to_string")]
    pub confirm_by: i64, // 确认人
    pub confirm_time: DateTime<Utc>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}

// ======================== 【3】资产调拨表 ========================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetTransfer {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    pub transfer_no: String, // 调拨单号
    #[serde(serialize_with = "i64_to_string")]
    pub asset_id: i64,
    #[serde(serialize_with = "i64_to_string")]
    pub out_dept_id: i64, // 调出部门
    #[serde(serialize_with = "i64_to_string")]
    pub in_dept_id: i64, // 调入部门
    #[serde(serialize_with = "i64_to_string")]
    pub out_user_id: i64, // 调出人
    #[serde(serialize_with = "i64_to_string")]
    pub in_user_id: i64, // 调入人
    pub transfer_date: DateTime<Utc>,
    pub reason: String, // 调拨原因
    pub status: i8,     // 0=待审批 1=已调拨 2=已驳回
    #[serde(serialize_with = "opt_i64_to_string")]
    pub approve_by: Option<i64>,
    pub approve_time: Option<DateTime<Utc>>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}

// ======================== 【4】资产维修表 ========================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetRepair {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    pub repair_no: String, // 维修单号
    #[serde(serialize_with = "i64_to_string")]
    pub asset_id: i64,
    pub fault_desc: String,          // 故障描述
    pub repair_desc: Option<String>, // 维修描述
    #[serde(serialize_with = "opt_i64_to_string")]
    pub repair_user_id: Option<i64>, // 维修人ID
    #[serde(serialize_with = "opt_i64_to_string")]
    pub repair_dept_id: Option<i64>, // 维修部门ID
    pub repair_file_url: Option<String>, // 维修相关附件URL，记录维修过程中产生的相关文件，如维修报告、照片等
    pub repair_type: i8,                 // 0=送修 1=上门 2=远程
    pub vendor: Option<String>,          // 维修商
    pub cost: Option<f64>,               // 维修费用
    pub apply_date: DateTime<Utc>,       // 申请日期
    pub repair_date: Option<DateTime<Utc>>, // 维修日期
    pub finish_date: Option<DateTime<Utc>>, // 完成日期
    pub status: i8,                      // 0=待维修 1=维修中 2=已完成 3=无法维修
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Assets {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64, //主键ID
    pub asset_no: String,   //资产编号
    pub asset_type: String, //资产类型，取值：hardware（固定资产）、intangible（无形资产）
    #[serde(serialize_with = "i64_to_string")]
    pub category_id: i64, //分类ID，外键，关联资产分类表（asset_category）id
    pub asset_name: String, //资产名称
    pub manufacturer: Option<String>, //制造商
    pub model: Option<String>, //型号
    pub department_ids: Option<Vec<i64>>, //使用部门ID集合
    pub user_ids: Option<Vec<i64>>, //使用人ID集合
    pub status: i16,        //状态：0=正常 1=借用 2=维修 3=报废 4=过期
    pub purchase_date: Option<NaiveDateTime>, //购买日期 ；
    pub purchase_price: Option<f64>, //购买价格
    pub quantity: Option<i32>, //总数量，默认为1，针对批量采购的资产记录实际数量
    pub used_quantity: Option<i32>, //已使用数量，针对批量采购的资产记录已领用的数量，默认为0
    pub expire_date: Option<NaiveDateTime>, //过期日期，用于记录资产的过期时间，系统可根据该字段自动识别过期资产并进行提醒
    pub description: Option<String>,        //描述，记录资产的详细描述信息
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>, //创建人，记录创建该资产的管理员
    pub created_at: Option<DateTime<Utc>>,  //创建时间，记录资产的创建时间
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>, //更新人，记录最后一次修改该资产的管理员
    pub updated_at: Option<DateTime<Utc>>,  //更新时间，记录资产的最后修改时间
    pub deleted: Option<i16>,               //删除标志
}

// ======================== 【5】资产报废表 ========================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetScrap {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    pub scrap_no: String, // 报废单号
    #[serde(serialize_with = "i64_to_string")]
    pub asset_id: i64,
    pub reason: String, // 报废原因
    pub scrap_date: DateTime<Utc>,
    pub status: i8, // 0=待审批 1=已批准 2=已驳回 3=已报废
    #[serde(serialize_with = "opt_i64_to_string")]
    pub approve_by: Option<i64>,
    pub approve_time: Option<DateTime<Utc>>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub handle_user: Option<i64>, // 处理人
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}

// ======================== 【6】资产采购申请表 ========================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetPurchase {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    pub purchase_no: String, // 采购单号
    pub asset_name: String,  // 采购资产名称
    #[serde(serialize_with = "i64_to_string")]
    pub category_id: i64, // 分类
    pub model: Option<String>,
    pub manufacturer: Option<String>,
    pub quantity: i32,            // 采购数量
    pub unit_price: Option<f64>,  // 单价
    pub total_price: Option<f64>, // 总价
    #[serde(serialize_with = "i64_to_string")]
    pub apply_user: i64, // 申请人
    #[serde(serialize_with = "i64_to_string")]
    pub dept_id: i64, // 申请部门
    pub reason: String,           // 采购原因
    pub status: i8,               // 0=待审批 1=采购中 2=已完成 3=已驳回
    pub supplier: Option<String>, // 供应商
    pub purchase_date: Option<DateTime<Utc>>, //购买时间
    pub arrive_date: Option<DateTime<Utc>>, //预计到货时间
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}

// ======================== 【7】通用审批记录表（所有流程共用） ========================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetApproval {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    pub biz_type: i8, // 业务类型：1=领用 2=归还 3=调拨 4=维修 5=报废 6=采购
    #[serde(serialize_with = "i64_to_string")]
    pub biz_id: i64, // 业务ID
    pub step: i16,    // 审批步骤
    #[serde(serialize_with = "i64_to_string")]
    pub approver_id: i64, // 审批人ID
    pub approve_status: i8, // 0=待审 1=同意 2=驳回
    pub remark: Option<String>,
    pub approve_time: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MantineTree {
    pub value: String,                      // 必须：id 转字符串
    pub label: String,                      // 必须：显示名称
    pub children: Option<Vec<MantineTree>>, // 子节点
    pub checked: Option<bool>,              // 可选：是否选中（权限分配时使用）
}

/// 用户注册申请实体
/// 对应表：public.sys_user_register
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SysUserRegister {
    #[serde(serialize_with = "i64_to_string", deserialize_with = "i64_from_string")]
    pub id: i64,
    pub username: String,
    pub passwd: String,
    pub real_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub department_name: Option<String>,
    pub company_name: Option<String>,
    pub reason: Option<String>,
    pub status: i16,
    #[serde(
        serialize_with = "opt_i64_to_string",
        deserialize_with = "opt_i64_from_string"
    )]
    pub approve_by: Option<i64>,
    pub approve_time: Option<DateTime<Utc>>,
    pub approve_remark: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// 用户-租户关联表（多对多）
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SysUserTenant {
    #[serde(serialize_with = "i64_to_string", deserialize_with = "i64_from_string")]
    pub id: i64,
    #[serde(serialize_with = "i64_to_string", deserialize_with = "i64_from_string")]
    pub user_id: i64,
    #[serde(serialize_with = "i64_to_string", deserialize_with = "i64_from_string")]
    pub tenant_id: i64,
    #[serde(
        serialize_with = "opt_i64_to_string",
        deserialize_with = "opt_i64_from_string"
    )]
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
}

/// 租户信息（登录响应中使用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantInfo {
    #[serde(serialize_with = "i64_to_string", deserialize_with = "i64_from_string")]
    pub id: i64,
    pub tenant_name: String,
    pub schema_name: Option<String>,
    pub is_current: bool,
}

/// 侧边栏菜单项（用于前端动态渲染）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidebarMenuItem {
    pub label: String,                          // 菜单名称
    pub path: Option<String>,                   // 路由路径
    pub icon: Option<String>,                   // 图标名称
    pub children: Option<Vec<SidebarMenuItem>>, // 子菜单
}

// ======================== OKF 知识资产 ========================

/// OKF 知识资产（全新 knowledge_asset 表，不与旧 asset_knowledge 冲突）
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct KnowledgeAsset {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    #[serde(serialize_with = "i64_to_string")]
    pub tree_node_id: i64,
    pub title: String,
    pub content: Option<String>,
    pub content_html: Option<String>,
    pub okf_type: String,
    pub summary: Option<String>,
    pub source: Option<String>,
    pub confidence: Option<f64>,
    pub status: String,
    pub effective_at: Option<DateTime<Utc>>,
    pub expire_at: Option<DateTime<Utc>>,
    pub relation_ids: Option<Vec<i64>>,
    pub tags: Option<Vec<String>>,
    pub file_url: Option<String>,
    pub file_name: Option<String>,
    pub file_size: Option<i64>,
    pub file_mime: Option<String>,
    pub file_md5: Option<String>,
    pub editor_mode: String,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub updated_by: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}

// ======================== 知识库模块新增 Model ========================

/// 文档向量分片（RAG 核心表）
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DocumentChunk {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    #[serde(serialize_with = "i64_to_string")]
    pub asset_id: i64,
    pub chunk_index: i32,
    pub chunk_text: String,
    pub token_count: Option<i32>,
    pub embedding: Option<Vec<f32>>,
    pub title: Option<String>,
    pub okf_type: Option<String>,
    pub tags: Option<Vec<String>>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub tree_node_id: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}

/// 对话会话
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Conversation {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    #[serde(serialize_with = "i64_to_string")]
    pub user_id: i64,
    pub title: Option<String>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub bind_knowledge_tree_id: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}

/// 会话消息
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Message {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    #[serde(serialize_with = "i64_to_string")]
    pub conv_id: i64,
    pub role: String,
    pub content: String,
    pub audio_url: Option<String>,
    pub reference_asset_ids: Option<Vec<i64>>,
    pub reference_text: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub input_tokens: Option<i32>,
    pub output_tokens: Option<i32>,
    pub created_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}

/// 用户长期记忆
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Memory {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    #[serde(serialize_with = "i64_to_string")]
    pub user_id: i64,
    pub content: String,
    pub category: Option<String>,
    pub importance: Option<f64>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub source_conv_id: Option<i64>,
    pub next_review_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}

/// 技能执行日志
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SkillExecution {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub asset_id: Option<i64>,
    pub trigger_type: Option<String>,
    pub input_params: Option<serde_json::Value>,
    pub output_result: Option<serde_json::Value>,
    pub status: String,
    pub error_msg: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
}

/// LLM 厂商配置
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct LlmProvider {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    pub provider_code: String,
    pub provider_name: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub secret_key: Option<String>,
    pub extra_config: Option<serde_json::Value>,
    pub weight: Option<i32>,
    pub is_local: bool,
    pub enable: bool,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}

/// LLM 模型明细
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct LlmModel {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    #[serde(serialize_with = "i64_to_string")]
    pub provider_id: i64,
    pub model_code: String,
    pub model_name: String,
    pub model_type: String,
    pub context_window: Option<i32>,
    pub temperature_default: Option<f64>,
    pub max_tokens_default: Option<i32>,
    pub price_input: Option<rust_decimal::Decimal>,
    pub price_output: Option<rust_decimal::Decimal>,
    pub enable: bool,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}

/// 用户模型偏好配置
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UserLLmSetting {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    #[serde(serialize_with = "i64_to_string")]
    pub user_id: i64,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub default_provider_id: Option<i64>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub default_chat_model_id: Option<i64>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub default_embed_model_id: Option<i64>,
    pub custom_temp: Option<f64>,
    pub custom_max_token: Option<i32>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}

/// LLM 调用记录
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct LLmCallRecord {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub user_id: Option<i64>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub conv_id: Option<i64>,
    #[serde(serialize_with = "i64_to_string")]
    pub provider_id: i64,
    #[serde(serialize_with = "i64_to_string")]
    pub model_id: i64,
    pub call_type: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub total_cost: Option<rust_decimal::Decimal>,
    pub duration_ms: Option<i32>,
    pub status: String,
    pub error_msg: Option<String>,
    pub request_id: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
}

// ======================== 请求/响应结构体 ========================

/// Token 用量
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub total_tokens: i32,
    pub cost: f64,
}

/// LLM Chat 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    /// 多模态内容片段（如图片），存在时优先于 content 使用；向后兼容纯文本
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_parts: Option<Vec<ContentPart>>,
}

/// 多模态内容片段（OpenAI 兼容格式）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentPart {
    Text {
        r#type: String,
        text: String,
    },
    Image {
        r#type: String,
        image_url: ImageUrl,
    },
}

/// 图片 URL（OpenAI 兼容 image_url 格式，支持 data URL base64）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
}

/// LLM Chat 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMChatRequest {
    pub messages: Vec<ChatMessage>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<i32>,
    pub stream: Option<bool>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub user_id: Option<i64>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub conv_id: Option<i64>,
}

/// LLM Chat 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMChatResponse {
    pub content: String,
    pub model: String,
    pub usage: TokenUsage,
    #[serde(serialize_with = "i64_to_string")]
    pub provider_id: i64,
    #[serde(serialize_with = "i64_to_string")]
    pub model_id: i64,
    pub request_id: String,
}

/// LLM Embedding 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMEmbeddingRequest {
    pub input: Vec<String>,
    pub model: Option<String>,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub user_id: Option<i64>,
}

/// LLM Embedding 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMEmbeddingResponse {
    pub embeddings: Vec<Vec<f32>>,
    pub model: String,
    pub usage: TokenUsage,
    #[serde(serialize_with = "i64_to_string")]
    pub provider_id: i64,
    #[serde(serialize_with = "i64_to_string")]
    pub model_id: i64,
}

/// RAG 检索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkResult {
    #[serde(serialize_with = "i64_to_string")]
    pub chunk_id: i64,
    pub chunk_text: String,
    pub chunk_index: i32,
    pub title: String,
    pub okf_type: String,
    #[serde(serialize_with = "i64_to_string")]
    pub asset_id: i64,
    pub similarity: f64,
    pub token_count: i32,
}

/// RAG 检索参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrieveParams {
    pub question: String,
    #[serde(serialize_with = "opt_i64_to_string")]
    pub bind_tree_node_id: Option<i64>,
    pub top_k: i32,
    pub max_tokens: i32,
    pub okf_type_filter: Option<String>,
    pub min_similarity: f64,
}

/// 单据编号规则配置
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DocNumberingRule {
    #[serde(serialize_with = "i64_to_string", deserialize_with = "i64_from_string")]
    pub id: i64,
    pub biz_type: String,
    pub biz_name: String,
    pub prefix: Option<String>,
    pub date_format: Option<String>,
    pub date_position: Option<String>,
    pub serial_length: i32,
    pub separator: Option<String>,
    pub reset_mode: Option<String>,
    pub sample_output: Option<String>,
    pub is_active: bool,
    #[serde(
        serialize_with = "opt_i64_to_string",
        deserialize_with = "opt_i64_from_string"
    )]
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    #[serde(
        serialize_with = "opt_i64_to_string",
        deserialize_with = "opt_i64_from_string"
    )]
    pub updated_by: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i16,
}

/// 单据编号流水号计数
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DocNumberingSequence {
    #[serde(serialize_with = "i64_to_string", deserialize_with = "i64_from_string")]
    pub id: i64,
    pub biz_type: String,
    pub reset_key: String,
    pub current_seq: i32,
    pub updated_at: Option<DateTime<Utc>>,
}

/// 记忆条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    #[serde(serialize_with = "i64_to_string")]
    pub id: i64,
    pub content: String,
    pub category: Option<String>,
    pub importance: Option<f64>,
}
