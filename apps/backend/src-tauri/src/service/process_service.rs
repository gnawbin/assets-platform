//! 流程管理 Service
//!
//! 提供资产领用、归还、调拨、维修、报废、采购的 CRUD 操作。
//! 遵循分层架构，所有业务逻辑在此实现。

use crate::database;
use crate::database::models::{
    AssetPurchase, AssetReceive, AssetRepair, AssetReturn, AssetScrap, AssetTransfer,
};
use crate::utils::snowflake::next_id;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

// ======================== 领用管理 ========================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetReceiveInput {
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub asset_id: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub user_id: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub department_id: i64,
    pub receive_date: String,
    pub reason: String,
    pub status: Option<i8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetReceiveUpdateInput {
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub asset_id: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub user_id: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub department_id: i64,
    pub receive_date: String,
    pub reason: String,
    pub status: Option<i8>,
}

/// 获取所有领用记录
pub async fn get_receives() -> Result<Vec<AssetReceive>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    let prefix = database::schema_prefix();
    let sql = format!(
        "SELECT id, receive_no, asset_id, user_id, department_id, receive_date, reason, status, approve_by, approve_time, approve_remark, created_by, created_at, updated_by, updated_at, deleted FROM {}asset_receive WHERE deleted = 0 ORDER BY created_at DESC",
        prefix
    );
    let rows = sqlx::query_as::<_, AssetReceive>(sqlx::AssertSqlSafe(sql))
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询领用记录失败: {}", e);
            format!("查询领用记录失败: {}", e)
        })?;

    let count = rows.len();
    info!("查询领用记录成功: 共 {} 条", count);
    Ok(rows)
}

/// 新增领用记录
pub async fn insert_receive(input: AssetReceiveInput) -> Result<AssetReceive, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let id = next_id() as i64;
    let receive_no = format!("RECV-{}", id);

    info!(
        "新增领用记录: asset_id={}, user_id={}",
        input.asset_id, input.user_id
    );

    let prefix = database::schema_prefix();
    let sql = format!(
        r#"
        INSERT INTO {}asset_receive (id, receive_no, asset_id, user_id, department_id, receive_date, reason, status, created_by, created_at, updated_by, updated_at, deleted)
        VALUES ($1, $2, $3, $4, $5, $6::timestamptz, $7, $8, $9, NOW(), $9, NOW(), 0)
        RETURNING id, receive_no, asset_id, user_id, department_id, receive_date, reason, status, approve_by, approve_time, approve_remark, created_by, created_at, updated_by, updated_at, deleted
        "#,
        prefix
    );
    let row = sqlx::query_as::<_, AssetReceive>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(&receive_no)
        .bind(input.asset_id)
        .bind(input.user_id)
        .bind(input.department_id)
        .bind(&input.receive_date)
        .bind(&input.reason)
        .bind(input.status.unwrap_or(0))
        .bind(1i64)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("新增领用记录失败: {}", e);
            format!("新增领用记录失败: {}", e)
        })?;

    info!(
        "新增领用记录成功: id={}, receive_no={}",
        row.id, row.receive_no
    );
    Ok(row)
}

/// 更新领用记录
pub async fn update_receive(
    id: i64,
    input: AssetReceiveUpdateInput,
) -> Result<AssetReceive, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("更新领用记录: id={}", id);

    let prefix = database::schema_prefix();
    let sql = format!(
        r#"
        UPDATE {}asset_receive SET
            asset_id = $2, user_id = $3, department_id = $4,
            receive_date = $5::timestamptz, reason = $6, status = $7,
            updated_by = $8, updated_at = NOW()
        WHERE id = $1 AND deleted = 0
        RETURNING id, receive_no, asset_id, user_id, department_id, receive_date, reason, status, approve_by, approve_time, approve_remark, created_by, created_at, updated_by, updated_at, deleted
        "#,
        prefix
    );
    let row = sqlx::query_as::<_, AssetReceive>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(input.asset_id)
        .bind(input.user_id)
        .bind(input.department_id)
        .bind(&input.receive_date)
        .bind(&input.reason)
        .bind(input.status.unwrap_or(0))
        .bind(1i64)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("更新领用记录失败: id={}, error={}", id, e);
            format!("更新领用记录失败: {}", e)
        })?;

    info!("更新领用记录成功: id={}", id);
    Ok(row)
}

/// 删除领用记录（软删除）
pub async fn delete_receive(id: i64) -> Result<(), String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("删除领用记录: id={}", id);

    let prefix = database::schema_prefix();
    let sql = format!(
        "UPDATE {}asset_receive SET deleted = 1, updated_at = NOW() WHERE id = $1",
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("删除领用记录失败: id={}, error={}", id, e);
            format!("删除领用记录失败: {}", e)
        })?;

    info!("删除领用记录成功: id={}", id);
    Ok(())
}

// ======================== 归还管理 ========================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetReturnInput {
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub receive_id: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub asset_id: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub user_id: i64,
    pub return_date: String,
    pub asset_status: Option<i8>,
    pub remark: Option<String>,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub confirm_by: i64,
    pub confirm_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetReturnUpdateInput {
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub receive_id: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub asset_id: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub user_id: i64,
    pub return_date: String,
    pub asset_status: Option<i8>,
    pub remark: Option<String>,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub confirm_by: i64,
    pub confirm_time: String,
}

/// 获取所有归还记录
pub async fn get_returns() -> Result<Vec<AssetReturn>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    let prefix = database::schema_prefix();
    let sql = format!(
        "SELECT id, return_no, receive_id, asset_id, user_id, return_date, asset_status, remark, confirm_by, confirm_time, created_by, created_at, updated_by, updated_at, deleted FROM {}asset_return WHERE deleted = 0 ORDER BY created_at DESC",
        prefix
    );
    let rows = sqlx::query_as::<_, AssetReturn>(sqlx::AssertSqlSafe(sql))
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询归还记录失败: {}", e);
            format!("查询归还记录失败: {}", e)
        })?;

    let count = rows.len();
    info!("查询归还记录成功: 共 {} 条", count);
    Ok(rows)
}

/// 新增归还记录
pub async fn insert_return(input: AssetReturnInput) -> Result<AssetReturn, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let id = next_id() as i64;
    let return_no = format!("RET-{}", id);

    info!(
        "新增归还记录: asset_id={}, user_id={}",
        input.asset_id, input.user_id
    );

    let prefix = database::schema_prefix();
    let sql = format!(
        r#"
        INSERT INTO {}asset_return (id, return_no, receive_id, asset_id, user_id, return_date, asset_status, remark, confirm_by, confirm_time, created_by, created_at, updated_by, updated_at, deleted)
        VALUES ($1, $2, $3, $4, $5, $6::timestamptz, $7, $8, $9, $10::timestamptz, $11, NOW(), $11, NOW(), 0)
        RETURNING id, return_no, receive_id, asset_id, user_id, return_date, asset_status, remark, confirm_by, confirm_time, created_by, created_at, updated_by, updated_at, deleted
        "#,
        prefix
    );
    let row = sqlx::query_as::<_, AssetReturn>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(&return_no)
        .bind(input.receive_id)
        .bind(input.asset_id)
        .bind(input.user_id)
        .bind(&input.return_date)
        .bind(input.asset_status.unwrap_or(0))
        .bind(&input.remark)
        .bind(input.confirm_by)
        .bind(&input.confirm_time)
        .bind(1i64)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("新增归还记录失败: {}", e);
            format!("新增归还记录失败: {}", e)
        })?;

    info!(
        "新增归还记录成功: id={}, return_no={}",
        row.id, row.return_no
    );
    Ok(row)
}

/// 更新归还记录
pub async fn update_return(id: i64, input: AssetReturnUpdateInput) -> Result<AssetReturn, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("更新归还记录: id={}", id);

    let prefix = database::schema_prefix();
    let sql = format!(
        r#"
        UPDATE {}asset_return SET
            receive_id = $2, asset_id = $3, user_id = $4,
            return_date = $5::timestamptz, asset_status = $6, remark = $7,
            confirm_by = $8, confirm_time = $9::timestamptz,
            updated_by = $10, updated_at = NOW()
        WHERE id = $1 AND deleted = 0
        RETURNING id, return_no, receive_id, asset_id, user_id, return_date, asset_status, remark, confirm_by, confirm_time, created_by, created_at, updated_by, updated_at, deleted
        "#,
        prefix
    );
    let row = sqlx::query_as::<_, AssetReturn>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(input.receive_id)
        .bind(input.asset_id)
        .bind(input.user_id)
        .bind(&input.return_date)
        .bind(input.asset_status.unwrap_or(0))
        .bind(&input.remark)
        .bind(input.confirm_by)
        .bind(&input.confirm_time)
        .bind(1i64)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("更新归还记录失败: id={}, error={}", id, e);
            format!("更新归还记录失败: {}", e)
        })?;

    info!("更新归还记录成功: id={}", id);
    Ok(row)
}

/// 删除归还记录（软删除）
pub async fn delete_return(id: i64) -> Result<(), String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("删除归还记录: id={}", id);

    let prefix = database::schema_prefix();
    let sql = format!(
        "UPDATE {}asset_return SET deleted = 1, updated_at = NOW() WHERE id = $1",
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("删除归还记录失败: id={}, error={}", id, e);
            format!("删除归还记录失败: {}", e)
        })?;

    info!("删除归还记录成功: id={}", id);
    Ok(())
}

// ======================== 调拨管理 ========================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetTransferInput {
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub asset_id: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub out_dept_id: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub in_dept_id: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub out_user_id: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub in_user_id: i64,
    pub transfer_date: String,
    pub reason: String,
    pub status: Option<i8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetTransferUpdateInput {
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub asset_id: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub out_dept_id: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub in_dept_id: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub out_user_id: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub in_user_id: i64,
    pub transfer_date: String,
    pub reason: String,
    pub status: Option<i8>,
}

/// 获取所有调拨记录
pub async fn get_transfers() -> Result<Vec<AssetTransfer>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    let prefix = database::schema_prefix();
    let sql = format!(
        "SELECT id, transfer_no, asset_id, out_dept_id, in_dept_id, out_user_id, in_user_id, transfer_date, reason, status, approve_by, approve_time, created_by, created_at, updated_by, updated_at, deleted FROM {}asset_transfer WHERE deleted = 0 ORDER BY created_at DESC",
        prefix
    );
    let rows = sqlx::query_as::<_, AssetTransfer>(sqlx::AssertSqlSafe(sql))
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询调拨记录失败: {}", e);
            format!("查询调拨记录失败: {}", e)
        })?;

    let count = rows.len();
    info!("查询调拨记录成功: 共 {} 条", count);
    Ok(rows)
}

/// 新增调拨记录
pub async fn insert_transfer(input: AssetTransferInput) -> Result<AssetTransfer, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let id = next_id() as i64;
    let transfer_no = format!("TRSF-{}", id);

    info!("新增调拨记录: asset_id={}", input.asset_id);

    let prefix = database::schema_prefix();
    let sql = format!(
        r#"
        INSERT INTO {}asset_transfer (id, transfer_no, asset_id, out_dept_id, in_dept_id, out_user_id, in_user_id, transfer_date, reason, status, created_by, created_at, updated_by, updated_at, deleted)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8::timestamptz, $9, $10, $11, NOW(), $11, NOW(), 0)
        RETURNING id, transfer_no, asset_id, out_dept_id, in_dept_id, out_user_id, in_user_id, transfer_date, reason, status, approve_by, approve_time, created_by, created_at, updated_by, updated_at, deleted
        "#,
        prefix
    );
    let row = sqlx::query_as::<_, AssetTransfer>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(&transfer_no)
        .bind(input.asset_id)
        .bind(input.out_dept_id)
        .bind(input.in_dept_id)
        .bind(input.out_user_id)
        .bind(input.in_user_id)
        .bind(&input.transfer_date)
        .bind(&input.reason)
        .bind(input.status.unwrap_or(0))
        .bind(1i64)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("新增调拨记录失败: {}", e);
            format!("新增调拨记录失败: {}", e)
        })?;

    info!(
        "新增调拨记录成功: id={}, transfer_no={}",
        row.id, row.transfer_no
    );
    Ok(row)
}

/// 更新调拨记录
pub async fn update_transfer(
    id: i64,
    input: AssetTransferUpdateInput,
) -> Result<AssetTransfer, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("更新调拨记录: id={}", id);

    let prefix = database::schema_prefix();
    let sql = format!(
        r#"
        UPDATE {}asset_transfer SET
            asset_id = $2, out_dept_id = $3, in_dept_id = $4,
            out_user_id = $5, in_user_id = $6,
            transfer_date = $7::timestamptz, reason = $8, status = $9,
            updated_by = $10, updated_at = NOW()
        WHERE id = $1 AND deleted = 0
        RETURNING id, transfer_no, asset_id, out_dept_id, in_dept_id, out_user_id, in_user_id, transfer_date, reason, status, approve_by, approve_time, created_by, created_at, updated_by, updated_at, deleted
        "#,
        prefix
    );
    let row = sqlx::query_as::<_, AssetTransfer>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(input.asset_id)
        .bind(input.out_dept_id)
        .bind(input.in_dept_id)
        .bind(input.out_user_id)
        .bind(input.in_user_id)
        .bind(&input.transfer_date)
        .bind(&input.reason)
        .bind(input.status.unwrap_or(0))
        .bind(1i64)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("更新调拨记录失败: id={}, error={}", id, e);
            format!("更新调拨记录失败: {}", e)
        })?;

    info!("更新调拨记录成功: id={}", id);
    Ok(row)
}

/// 删除调拨记录（软删除）
pub async fn delete_transfer(id: i64) -> Result<(), String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("删除调拨记录: id={}", id);

    let prefix = database::schema_prefix();
    let sql = format!(
        "UPDATE {}asset_transfer SET deleted = 1, updated_at = NOW() WHERE id = $1",
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("删除调拨记录失败: id={}, error={}", id, e);
            format!("删除调拨记录失败: {}", e)
        })?;

    info!("删除调拨记录成功: id={}", id);
    Ok(())
}

// ======================== 维修管理 ========================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetRepairInput {
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub asset_id: i64,
    pub fault_desc: String,
    pub repair_desc: Option<String>,
    #[serde(deserialize_with = "crate::database::models::opt_i64_from_string")]
    pub repair_user_id: Option<i64>,
    #[serde(deserialize_with = "crate::database::models::opt_i64_from_string")]
    pub repair_dept_id: Option<i64>,
    pub repair_file_url: Option<String>,
    pub repair_type: Option<i8>,
    pub vendor: Option<String>,
    pub cost: Option<f64>,
    pub apply_date: String,
    pub repair_date: Option<String>,
    pub finish_date: Option<String>,
    pub status: Option<i8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetRepairUpdateInput {
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub asset_id: i64,
    pub fault_desc: String,
    pub repair_desc: Option<String>,
    #[serde(deserialize_with = "crate::database::models::opt_i64_from_string")]
    pub repair_user_id: Option<i64>,
    #[serde(deserialize_with = "crate::database::models::opt_i64_from_string")]
    pub repair_dept_id: Option<i64>,
    pub repair_file_url: Option<String>,
    pub repair_type: Option<i8>,
    pub vendor: Option<String>,
    pub cost: Option<f64>,
    pub apply_date: String,
    pub repair_date: Option<String>,
    pub finish_date: Option<String>,
    pub status: Option<i8>,
}

/// 获取所有维修记录
pub async fn get_repairs() -> Result<Vec<AssetRepair>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    let prefix = database::schema_prefix();
    let sql = format!(
        "SELECT id, repair_no, asset_id, fault_desc, repair_desc, repair_user_id, repair_dept_id, repair_file_url, repair_type, vendor, cost, apply_date, repair_date, finish_date, status, created_by, created_at, updated_by, updated_at, deleted FROM {}asset_repair WHERE deleted = 0 ORDER BY created_at DESC",
        prefix
    );
    let rows = sqlx::query_as::<_, AssetRepair>(sqlx::AssertSqlSafe(sql))
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询维修记录失败: {}", e);
            format!("查询维修记录失败: {}", e)
        })?;

    let count = rows.len();
    info!("查询维修记录成功: 共 {} 条", count);
    Ok(rows)
}

/// 新增维修记录
pub async fn insert_repair(input: AssetRepairInput) -> Result<AssetRepair, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let id = next_id() as i64;
    let repair_no = format!("REPR-{}", id);

    info!("新增维修记录: asset_id={}", input.asset_id);

    let prefix = database::schema_prefix();
    let sql = format!(
        r#"
        INSERT INTO {}asset_repair (id, repair_no, asset_id, fault_desc, repair_desc, repair_user_id, repair_dept_id, repair_file_url, repair_type, vendor, cost, apply_date, repair_date, finish_date, status, created_by, created_at, updated_by, updated_at, deleted)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12::timestamptz, $13::timestamptz, $14::timestamptz, $15, $16, NOW(), $16, NOW(), 0)
        RETURNING id, repair_no, asset_id, fault_desc, repair_desc, repair_user_id, repair_dept_id, repair_file_url, repair_type, vendor, cost, apply_date, repair_date, finish_date, status, created_by, created_at, updated_by, updated_at, deleted
        "#,
        prefix
    );
    let row = sqlx::query_as::<_, AssetRepair>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(&repair_no)
        .bind(input.asset_id)
        .bind(&input.fault_desc)
        .bind(&input.repair_desc)
        .bind(input.repair_user_id)
        .bind(input.repair_dept_id)
        .bind(&input.repair_file_url)
        .bind(input.repair_type.unwrap_or(0))
        .bind(&input.vendor)
        .bind(input.cost)
        .bind(&input.apply_date)
        .bind(&input.repair_date)
        .bind(&input.finish_date)
        .bind(input.status.unwrap_or(0))
        .bind(1i64)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("新增维修记录失败: {}", e);
            format!("新增维修记录失败: {}", e)
        })?;

    info!(
        "新增维修记录成功: id={}, repair_no={}",
        row.id, row.repair_no
    );
    Ok(row)
}

/// 更新维修记录
pub async fn update_repair(id: i64, input: AssetRepairUpdateInput) -> Result<AssetRepair, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("更新维修记录: id={}", id);

    let prefix = database::schema_prefix();
    let sql = format!(
        r#"
        UPDATE {}asset_repair SET
            asset_id = $2, fault_desc = $3, repair_desc = $4,
            repair_user_id = $5, repair_dept_id = $6, repair_file_url = $7,
            repair_type = $8, vendor = $9, cost = $10,
            apply_date = $11::timestamptz, repair_date = $12::timestamptz,
            finish_date = $13::timestamptz, status = $14,
            updated_by = $15, updated_at = NOW()
        WHERE id = $1 AND deleted = 0
        RETURNING id, repair_no, asset_id, fault_desc, repair_desc, repair_user_id, repair_dept_id, repair_file_url, repair_type, vendor, cost, apply_date, repair_date, finish_date, status, created_by, created_at, updated_by, updated_at, deleted
        "#,
        prefix
    );
    let row = sqlx::query_as::<_, AssetRepair>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(input.asset_id)
        .bind(&input.fault_desc)
        .bind(&input.repair_desc)
        .bind(input.repair_user_id)
        .bind(input.repair_dept_id)
        .bind(&input.repair_file_url)
        .bind(input.repair_type.unwrap_or(0))
        .bind(&input.vendor)
        .bind(input.cost)
        .bind(&input.apply_date)
        .bind(&input.repair_date)
        .bind(&input.finish_date)
        .bind(input.status.unwrap_or(0))
        .bind(1i64)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("更新维修记录失败: id={}, error={}", id, e);
            format!("更新维修记录失败: {}", e)
        })?;

    info!("更新维修记录成功: id={}", id);
    Ok(row)
}

/// 删除维修记录（软删除）
pub async fn delete_repair(id: i64) -> Result<(), String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("删除维修记录: id={}", id);

    let prefix = database::schema_prefix();
    let sql = format!(
        "UPDATE {}asset_repair SET deleted = 1, updated_at = NOW() WHERE id = $1",
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("删除维修记录失败: id={}, error={}", id, e);
            format!("删除维修记录失败: {}", e)
        })?;

    info!("删除维修记录成功: id={}", id);
    Ok(())
}

// ======================== 报废管理 ========================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetScrapInput {
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub asset_id: i64,
    pub reason: String,
    pub scrap_date: String,
    pub status: Option<i8>,
    #[serde(deserialize_with = "crate::database::models::opt_i64_from_string")]
    pub handle_user: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetScrapUpdateInput {
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub asset_id: i64,
    pub reason: String,
    pub scrap_date: String,
    pub status: Option<i8>,
    #[serde(deserialize_with = "crate::database::models::opt_i64_from_string")]
    pub handle_user: Option<i64>,
}

/// 获取所有报废记录
pub async fn get_scraps() -> Result<Vec<AssetScrap>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    let prefix = database::schema_prefix();
    let sql = format!(
        "SELECT id, scrap_no, asset_id, reason, scrap_date, status, approve_by, approve_time, handle_user, created_by, created_at, updated_by, updated_at, deleted FROM {}asset_scrap WHERE deleted = 0 ORDER BY created_at DESC",
        prefix
    );
    let rows = sqlx::query_as::<_, AssetScrap>(sqlx::AssertSqlSafe(sql))
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询报废记录失败: {}", e);
            format!("查询报废记录失败: {}", e)
        })?;

    let count = rows.len();
    info!("查询报废记录成功: 共 {} 条", count);
    Ok(rows)
}

/// 新增报废记录
pub async fn insert_scrap(input: AssetScrapInput) -> Result<AssetScrap, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let id = next_id() as i64;
    let scrap_no = format!("SCRP-{}", id);

    info!("新增报废记录: asset_id={}", input.asset_id);

    let prefix = database::schema_prefix();
    let sql = format!(
        r#"
        INSERT INTO {}asset_scrap (id, scrap_no, asset_id, reason, scrap_date, status, handle_user, created_by, created_at, updated_by, updated_at, deleted)
        VALUES ($1, $2, $3, $4, $5::timestamptz, $6, $7, $8, NOW(), $8, NOW(), 0)
        RETURNING id, scrap_no, asset_id, reason, scrap_date, status, approve_by, approve_time, handle_user, created_by, created_at, updated_by, updated_at, deleted
        "#,
        prefix
    );
    let row = sqlx::query_as::<_, AssetScrap>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(&scrap_no)
        .bind(input.asset_id)
        .bind(&input.reason)
        .bind(&input.scrap_date)
        .bind(input.status.unwrap_or(0))
        .bind(input.handle_user)
        .bind(1i64)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("新增报废记录失败: {}", e);
            format!("新增报废记录失败: {}", e)
        })?;

    info!("新增报废记录成功: id={}, scrap_no={}", row.id, row.scrap_no);
    Ok(row)
}

/// 更新报废记录
pub async fn update_scrap(id: i64, input: AssetScrapUpdateInput) -> Result<AssetScrap, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("更新报废记录: id={}", id);

    let prefix = database::schema_prefix();
    let sql = format!(
        r#"
        UPDATE {}asset_scrap SET
            asset_id = $2, reason = $3, scrap_date = $4::timestamptz,
            status = $5, handle_user = $6,
            updated_by = $7, updated_at = NOW()
        WHERE id = $1 AND deleted = 0
        RETURNING id, scrap_no, asset_id, reason, scrap_date, status, approve_by, approve_time, handle_user, created_by, created_at, updated_by, updated_at, deleted
        "#,
        prefix
    );
    let row = sqlx::query_as::<_, AssetScrap>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(input.asset_id)
        .bind(&input.reason)
        .bind(&input.scrap_date)
        .bind(input.status.unwrap_or(0))
        .bind(input.handle_user)
        .bind(1i64)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("更新报废记录失败: id={}, error={}", id, e);
            format!("更新报废记录失败: {}", e)
        })?;

    info!("更新报废记录成功: id={}", id);
    Ok(row)
}

/// 删除报废记录（软删除）
pub async fn delete_scrap(id: i64) -> Result<(), String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("删除报废记录: id={}", id);

    let prefix = database::schema_prefix();
    let sql = format!(
        "UPDATE {}asset_scrap SET deleted = 1, updated_at = NOW() WHERE id = $1",
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("删除报废记录失败: id={}, error={}", id, e);
            format!("删除报废记录失败: {}", e)
        })?;

    info!("删除报废记录成功: id={}", id);
    Ok(())
}

// ======================== 采购管理 ========================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetPurchaseInput {
    pub asset_name: String,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub category_id: i64,
    pub model: Option<String>,
    pub manufacturer: Option<String>,
    pub quantity: i32,
    pub unit_price: Option<f64>,
    pub total_price: Option<f64>,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub apply_user: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub dept_id: i64,
    pub reason: String,
    pub status: Option<i8>,
    pub supplier: Option<String>,
    pub purchase_date: Option<String>,
    pub arrive_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetPurchaseUpdateInput {
    pub asset_name: String,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub category_id: i64,
    pub model: Option<String>,
    pub manufacturer: Option<String>,
    pub quantity: i32,
    pub unit_price: Option<f64>,
    pub total_price: Option<f64>,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub apply_user: i64,
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub dept_id: i64,
    pub reason: String,
    pub status: Option<i8>,
    pub supplier: Option<String>,
    pub purchase_date: Option<String>,
    pub arrive_date: Option<String>,
}

/// 获取所有采购记录
pub async fn get_purchases() -> Result<Vec<AssetPurchase>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    let prefix = database::schema_prefix();
    let sql = format!(
        "SELECT id, purchase_no, asset_name, category_id, model, manufacturer, quantity, unit_price, total_price, apply_user, dept_id, reason, status, supplier, purchase_date, arrive_date, created_by, created_at, updated_by, updated_at, deleted FROM {}asset_purchase WHERE deleted = 0 ORDER BY created_at DESC",
        prefix
    );
    let rows = sqlx::query_as::<_, AssetPurchase>(sqlx::AssertSqlSafe(sql))
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询采购记录失败: {}", e);
            format!("查询采购记录失败: {}", e)
        })?;

    let count = rows.len();
    info!("查询采购记录成功: 共 {} 条", count);
    Ok(rows)
}

/// 新增采购记录
pub async fn insert_purchase(input: AssetPurchaseInput) -> Result<AssetPurchase, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let id = next_id() as i64;
    let purchase_no = format!("PUR-{}", id);

    info!("新增采购记录: asset_name={}", input.asset_name);

    let prefix = database::schema_prefix();
    let sql = format!(
        r#"
        INSERT INTO {}asset_purchase (id, purchase_no, asset_name, category_id, model, manufacturer, quantity, unit_price, total_price, apply_user, dept_id, reason, status, supplier, purchase_date, arrive_date, created_by, created_at, updated_by, updated_at, deleted)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15::timestamptz, $16::timestamptz, $17, NOW(), $17, NOW(), 0)
        RETURNING id, purchase_no, asset_name, category_id, model, manufacturer, quantity, unit_price, total_price, apply_user, dept_id, reason, status, supplier, purchase_date, arrive_date, created_by, created_at, updated_by, updated_at, deleted
        "#,
        prefix
    );
    let row = sqlx::query_as::<_, AssetPurchase>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(&purchase_no)
        .bind(&input.asset_name)
        .bind(input.category_id)
        .bind(&input.model)
        .bind(&input.manufacturer)
        .bind(input.quantity)
        .bind(input.unit_price)
        .bind(input.total_price)
        .bind(input.apply_user)
        .bind(input.dept_id)
        .bind(&input.reason)
        .bind(input.status.unwrap_or(0))
        .bind(&input.supplier)
        .bind(&input.purchase_date)
        .bind(&input.arrive_date)
        .bind(1i64)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("新增采购记录失败: {}", e);
            format!("新增采购记录失败: {}", e)
        })?;

    info!(
        "新增采购记录成功: id={}, purchase_no={}",
        row.id, row.purchase_no
    );
    Ok(row)
}

/// 更新采购记录
pub async fn update_purchase(
    id: i64,
    input: AssetPurchaseUpdateInput,
) -> Result<AssetPurchase, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("更新采购记录: id={}", id);

    let prefix = database::schema_prefix();
    let sql = format!(
        r#"
        UPDATE {}asset_purchase SET
            asset_name = $2, category_id = $3, model = $4, manufacturer = $5,
            quantity = $6, unit_price = $7, total_price = $8,
            apply_user = $9, dept_id = $10, reason = $11, status = $12,
            supplier = $13, purchase_date = $14::timestamptz,
            arrive_date = $15::timestamptz,
            updated_by = $16, updated_at = NOW()
        WHERE id = $1 AND deleted = 0
        RETURNING id, purchase_no, asset_name, category_id, model, manufacturer, quantity, unit_price, total_price, apply_user, dept_id, reason, status, supplier, purchase_date, arrive_date, created_by, created_at, updated_by, updated_at, deleted
        "#,
        prefix
    );
    let row = sqlx::query_as::<_, AssetPurchase>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(&input.asset_name)
        .bind(input.category_id)
        .bind(&input.model)
        .bind(&input.manufacturer)
        .bind(input.quantity)
        .bind(input.unit_price)
        .bind(input.total_price)
        .bind(input.apply_user)
        .bind(input.dept_id)
        .bind(&input.reason)
        .bind(input.status.unwrap_or(0))
        .bind(&input.supplier)
        .bind(&input.purchase_date)
        .bind(&input.arrive_date)
        .bind(1i64)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            error!("更新采购记录失败: id={}, error={}", id, e);
            format!("更新采购记录失败: {}", e)
        })?;

    info!("更新采购记录成功: id={}", id);
    Ok(row)
}

/// 删除采购记录（软删除）
pub async fn delete_purchase(id: i64) -> Result<(), String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;

    info!("删除采购记录: id={}", id);

    let prefix = database::schema_prefix();
    let sql = format!(
        "UPDATE {}asset_purchase SET deleted = 1, updated_at = NOW() WHERE id = $1",
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("删除采购记录失败: id={}, error={}", id, e);
            format!("删除采购记录失败: {}", e)
        })?;

    info!("删除采购记录成功: id={}", id);
    Ok(())
}
