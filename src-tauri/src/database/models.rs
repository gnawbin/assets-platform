use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DbConfig {
    pub id: i64,
    pub host: String,
    pub port: i32,
    pub db_name: String,
    pub username: String,
    pub password: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SystemConfig {
    pub id: i64,
    pub config_key: String,
    pub config_value: String,
    pub remark: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetCategory {
    pub id: i64,
    pub category_name: String,
    pub asset_type: String,
    pub parent_id: i64,
    pub sort: i16,
    pub description: Option<String>,
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_by: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
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
    pub created_at: Option<DateTime<Utc>>,
    pub updated_by: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i8,
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
    pub maintenance_expire_date: Option<DateTime<Utc>>, //维保到期日期，用于维保到期提醒，优先级高于主表expire_date
    pub hardware_config: Option<String>, //硬件配置详情，如CPU、内存、硬盘、显卡等，JSON格式存储（如{"cpu":"i7-13700H","memory":"16GB"}）
    pub use_user_id: Option<i64>, //外键，关联用户表（user）id，记录当前使用人，状态为“在用/借用”时必填
    pub use_start_date: Option<DateTime<Utc>>, //使用开始日期，状态变更为“在用/借用”时自动记录
    pub fault_desc: Option<String>, //故障描述，状态为“维修”时填写，记录故障详情
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_by: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted: i8,
}
