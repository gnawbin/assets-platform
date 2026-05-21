use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use sqlx::FromRow;
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DbConfig{
   pub id: i64,
   pub host: String,
   pub port: i64,
    pub db_name: String,
  pub username: String,
  pub password: String,
  pub created_at: DateTime<Utc>,
  pub update_at: DateTime<Utc>,
}
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct  SystemConfig{
  pub id: i64,
  pub config_key: String,
  pub config_value: String,
  pub remark: String,
  pub created_at: DateTime<Utc>,
  pub update_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AssetCategory{
 pub id: i64,
 pub category_name: String,
 pub asset_type: String,
 pub parent_id: i64,
 pub sort: i8,
 pub description: String,
 pub created_by: i64,
 pub created_at:  DateTime<Utc>,
 pub update_by: i64,
 pub update_at: DateTime<Utc>,
}
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Assets{
  pub id: i64,
  pub asset_no: String,
  pub asset_type: String ,
  pub category_id: i64,
  pub asset_name: String,
  pub manufacturer: String,
  
}
