use crate::database;
use crate::utils::snowflake::next_id;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tracing::{error, info, warn};

// ======================== 固定资产（JOIN 视图） ========================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareAssetView {
    // assets 主表字段
    #[serde(
        serialize_with = "crate::database::models::i64_to_string",
        deserialize_with = "crate::database::models::i64_from_string"
    )]
    pub id: i64,
    pub asset_no: String,
    pub asset_type: String,
    #[serde(
        serialize_with = "crate::database::models::i64_to_string",
        deserialize_with = "crate::database::models::i64_from_string"
    )]
    pub category_id: i64,
    pub asset_name: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    #[serde(
        serialize_with = "crate::database::models::opt_i64_to_string",
        deserialize_with = "crate::database::models::opt_i64_from_string"
    )]
    pub department_id: Option<i64>,
    #[serde(
        serialize_with = "crate::database::models::opt_i64_to_string",
        deserialize_with = "crate::database::models::opt_i64_from_string"
    )]
    pub user_id: Option<i64>,
    pub status: i16,
    pub purchase_date: Option<String>,
    pub purchase_price: Option<f64>,
    pub quantity: Option<i32>,
    pub used_quantity: Option<i32>,
    pub expire_date: Option<String>,
    pub description: Option<String>,
    #[serde(
        serialize_with = "crate::database::models::opt_i64_to_string",
        deserialize_with = "crate::database::models::opt_i64_from_string"
    )]
    pub created_by: Option<i64>,
    pub created_at: Option<String>,
    #[serde(
        serialize_with = "crate::database::models::opt_i64_to_string",
        deserialize_with = "crate::database::models::opt_i64_from_string"
    )]
    pub updated_by: Option<i64>,
    pub updated_at: Option<String>,
    pub deleted: Option<i16>,
    // hard_assets 扩展字段
    #[serde(
        serialize_with = "crate::database::models::opt_i64_to_string",
        deserialize_with = "crate::database::models::opt_i64_from_string"
    )]
    pub hard_id: Option<i64>,
    pub sn: Option<String>,
    pub mac_address: Option<String>,
    pub location: Option<String>,
    pub hardware_config: Option<String>,
    #[serde(
        serialize_with = "crate::database::models::opt_i64_to_string",
        deserialize_with = "crate::database::models::opt_i64_from_string"
    )]
    pub use_user_id: Option<i64>,
    pub use_start_date: Option<String>,
    pub maintenance_vendor: Option<String>,
    pub maintenance_type: Option<String>,
    pub maintenance_expire_date: Option<String>,
    pub fault_desc: Option<String>,
}

// ======================== 无形资产（JOIN 视图） ========================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntangibleAssetView {
    // assets 主表字段
    #[serde(
        serialize_with = "crate::database::models::i64_to_string",
        deserialize_with = "crate::database::models::i64_from_string"
    )]
    pub id: i64,
    pub asset_no: String,
    pub asset_type: String,
    #[serde(
        serialize_with = "crate::database::models::i64_to_string",
        deserialize_with = "crate::database::models::i64_from_string"
    )]
    pub category_id: i64,
    pub asset_name: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    #[serde(
        serialize_with = "crate::database::models::opt_i64_to_string",
        deserialize_with = "crate::database::models::opt_i64_from_string"
    )]
    pub department_id: Option<i64>,
    #[serde(
        serialize_with = "crate::database::models::opt_i64_to_string",
        deserialize_with = "crate::database::models::opt_i64_from_string"
    )]
    pub user_id: Option<i64>,
    pub status: i16,
    pub purchase_date: Option<String>,
    pub purchase_price: Option<f64>,
    pub quantity: Option<i32>,
    pub used_quantity: Option<i32>,
    pub expire_date: Option<String>,
    pub description: Option<String>,
    #[serde(
        serialize_with = "crate::database::models::opt_i64_to_string",
        deserialize_with = "crate::database::models::opt_i64_from_string"
    )]
    pub created_by: Option<i64>,
    pub created_at: Option<String>,
    #[serde(
        serialize_with = "crate::database::models::opt_i64_to_string",
        deserialize_with = "crate::database::models::opt_i64_from_string"
    )]
    pub updated_by: Option<i64>,
    pub updated_at: Option<String>,
    pub deleted: Option<i16>,
    // intangible_assets 扩展字段
    #[serde(
        serialize_with = "crate::database::models::opt_i64_to_string",
        deserialize_with = "crate::database::models::opt_i64_from_string"
    )]
    pub intangible_id: Option<i64>,
    pub intangible_type: Option<String>,
    pub register_no: Option<String>,
    pub register_owner: Option<String>,
    pub register_date: Option<String>,
    pub valid_start_date: Option<String>,
    pub valid_end_date: Option<String>,
    pub right_status: Option<String>,
    pub license_key: Option<String>,
    pub license_type: Option<String>,
    pub authorized_scope: Option<String>,
    pub assigned_user_ids: Option<String>,
    pub bind_type: Option<String>,
    pub bind_info: Option<String>,
    pub version: Option<String>,
    pub download_link: Option<String>,
    pub amortization_method: Option<String>,
    pub useful_life: Option<i32>,
    pub amortization_amount: Option<f64>,
    pub residual_rate: Option<f64>,
}

// ======================== 新增/修改请求体 ========================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareAssetInput {
    // assets 主表字段
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub category_id: i64,
    pub asset_name: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    #[serde(deserialize_with = "crate::database::models::opt_i64_from_string")]
    pub department_id: Option<i64>,
    #[serde(deserialize_with = "crate::database::models::opt_i64_from_string")]
    pub user_id: Option<i64>,
    pub status: Option<i16>,
    pub purchase_date: Option<String>,
    pub purchase_price: Option<f64>,
    pub quantity: Option<i32>,
    pub used_quantity: Option<i32>,
    pub expire_date: Option<String>,
    pub description: Option<String>,
    // hard_assets 扩展字段
    pub sn: Option<String>,
    pub mac_address: Option<String>,
    pub location: Option<String>,
    pub hardware_config: Option<String>,
    #[serde(deserialize_with = "crate::database::models::opt_i64_from_string")]
    pub use_user_id: Option<i64>,
    pub use_start_date: Option<String>,
    pub maintenance_vendor: Option<String>,
    pub maintenance_type: Option<String>,
    pub maintenance_expire_date: Option<String>,
    pub fault_desc: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntangibleAssetInput {
    // assets 主表字段
    #[serde(deserialize_with = "crate::database::models::i64_from_string")]
    pub category_id: i64,
    pub asset_name: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    #[serde(deserialize_with = "crate::database::models::opt_i64_from_string")]
    pub department_id: Option<i64>,
    #[serde(deserialize_with = "crate::database::models::opt_i64_from_string")]
    pub user_id: Option<i64>,
    pub status: Option<i16>,
    pub purchase_date: Option<String>,
    pub purchase_price: Option<f64>,
    pub quantity: Option<i32>,
    pub used_quantity: Option<i32>,
    pub expire_date: Option<String>,
    pub description: Option<String>,
    // intangible_assets 扩展字段
    pub intangible_type: Option<String>,
    pub register_no: Option<String>,
    pub register_owner: Option<String>,
    pub register_date: Option<String>,
    pub valid_start_date: Option<String>,
    pub valid_end_date: Option<String>,
    pub right_status: Option<String>,
    pub license_key: Option<String>,
    pub license_type: Option<String>,
    pub authorized_scope: Option<String>,
    pub assigned_user_ids: Option<String>,
    pub bind_type: Option<String>,
    pub bind_info: Option<String>,
    pub version: Option<String>,
    pub download_link: Option<String>,
    pub amortization_method: Option<String>,
    pub useful_life: Option<i32>,
    pub amortization_amount: Option<f64>,
    pub residual_rate: Option<f64>,
}

// ======================== 辅助函数：从行映射到 HardwareAssetView ========================

fn row_to_hardware_view(row: &sqlx::postgres::PgRow) -> HardwareAssetView {
    HardwareAssetView {
        id: row.get("id"),
        asset_no: row.get("asset_no"),
        asset_type: row.get("asset_type"),
        category_id: row.get("category_id"),
        asset_name: row.get("asset_name"),
        manufacturer: row.get("manufacturer"),
        model: row.get("model"),
        department_id: row.get("department_id"),
        user_id: row.get("user_id"),
        status: row.get("status"),
        purchase_date: row.get("purchase_date"),
        purchase_price: row.get("purchase_price"),
        quantity: row.get("quantity"),
        used_quantity: row.get("used_quantity"),
        expire_date: row.get("expire_date"),
        description: row.get("description"),
        created_by: row.get("created_by"),
        created_at: row.get("created_at"),
        updated_by: row.get("updated_by"),
        updated_at: row.get("updated_at"),
        deleted: row.get("deleted"),
        hard_id: row.get("hard_id"),
        sn: row.get("sn"),
        mac_address: row.get("mac_address"),
        location: row.get("location"),
        hardware_config: row.get("hardware_config"),
        use_user_id: row.get("use_user_id"),
        use_start_date: row.get("use_start_date"),
        maintenance_vendor: row.get("maintenance_vendor"),
        maintenance_type: row.get("maintenance_type"),
        maintenance_expire_date: row.get("maintenance_expire_date"),
        fault_desc: row.get("fault_desc"),
    }
}

fn row_to_intangible_view(row: &sqlx::postgres::PgRow) -> IntangibleAssetView {
    IntangibleAssetView {
        id: row.get("id"),
        asset_no: row.get("asset_no"),
        asset_type: row.get("asset_type"),
        category_id: row.get("category_id"),
        asset_name: row.get("asset_name"),
        manufacturer: row.get("manufacturer"),
        model: row.get("model"),
        department_id: row.get("department_id"),
        user_id: row.get("user_id"),
        status: row.get("status"),
        purchase_date: row.get("purchase_date"),
        purchase_price: row.get("purchase_price"),
        quantity: row.get("quantity"),
        used_quantity: row.get("used_quantity"),
        expire_date: row.get("expire_date"),
        description: row.get("description"),
        created_by: row.get("created_by"),
        created_at: row.get("created_at"),
        updated_by: row.get("updated_by"),
        updated_at: row.get("updated_at"),
        deleted: row.get("deleted"),
        intangible_id: row.get("intangible_id"),
        intangible_type: row.get("intangible_type"),
        register_no: row.get("register_no"),
        register_owner: row.get("register_owner"),
        register_date: row.get("register_date"),
        valid_start_date: row.get("valid_start_date"),
        valid_end_date: row.get("valid_end_date"),
        right_status: row.get("right_status"),
        license_key: row.get("license_key"),
        license_type: row.get("license_type"),
        authorized_scope: row.get("authorized_scope"),
        assigned_user_ids: row.get("assigned_user_ids"),
        bind_type: row.get("bind_type"),
        bind_info: row.get("bind_info"),
        version: row.get("version"),
        download_link: row.get("download_link"),
        amortization_method: row.get("amortization_method"),
        useful_life: row.get("useful_life"),
        amortization_amount: row.get("amortization_amount"),
        residual_rate: row.get("residual_rate"),
    }
}

// ======================== 固定资产 CRUD ========================

fn hardware_select_sql(prefix: &str) -> String {
    format!(
        r#"
SELECT
    a.id, a.asset_no, a.asset_type, a.category_id, a.asset_name,
    a.manufacturer, a.model, a.department_id, a.user_id, a.status,
    a.purchase_date::text as purchase_date, a.purchase_price, a.quantity, a.used_quantity,
    a.expire_date::text as expire_date, a.description,
    a.created_by, a.created_at::text as created_at, a.updated_by, a.updated_at::text as updated_at, a.deleted,
    h.id as hard_id, h.sn, h.mac_address, h.location, h.hardware_config,
    h.use_user_id, h.use_start_date::text as use_start_date, h.maintenance_vendor,
    h.maintenance_type, h.maintenance_expire_date::text as maintenance_expire_date, h.fault_desc
FROM {}assets a
LEFT JOIN {}hard_assets h ON h.asset_id = a.id AND (h.deleted IS NULL OR h.deleted = 0)
WHERE a.asset_type = 'fixed' AND (a.deleted IS NULL OR a.deleted = 0)
"#,
        prefix, prefix
    )
}

/// 获取所有固定资产列表（JOIN assets + hard_assets）
pub async fn get_hardware_assets() -> Result<Vec<HardwareAssetView>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    let sql = format!(
        "{} ORDER BY a.created_at DESC",
        hardware_select_sql(&prefix)
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询固定资产列表失败: {}", e);
            format!("查询固定资产失败: {}", e)
        })?;

    let result: Vec<HardwareAssetView> = rows.iter().map(|r| row_to_hardware_view(r)).collect();
    let count = result.len();
    info!("查询固定资产列表成功: 共 {} 条记录", count);
    Ok(result)
}

/// 新增固定资产
pub async fn insert_hardware_asset(
    input: HardwareAssetInput,
    current_user_id: i64,
) -> Result<HardwareAssetView, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let asset_id = next_id() as i64;
    let hard_id = next_id() as i64;
    let asset_no = format!("{}", next_id());
    let prefix = database::schema_prefix();

    info!(
        "新增固定资产: name={}, category_id={}",
        input.asset_name, input.category_id
    );

    // 插入 assets 主表
    let sql = format!(
        r#"
        INSERT INTO {}assets (id, asset_no, asset_type, category_id, asset_name, manufacturer, model,
            department_id, user_id, status, purchase_date, purchase_price, quantity, used_quantity,
            expire_date, description, created_by, created_at, updated_by, updated_at, deleted)
        VALUES ($1, $2, 'fixed', $3, $4, $5, $6, $7, $8, $9,
            $10::timestamp, $11, $12, $13, $14::timestamp, $15,
            $16, NOW(), $16, NOW(), 0)
        "#,
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(asset_id)
        .bind(&asset_no)
        .bind(input.category_id)
        .bind(&input.asset_name)
        .bind(&input.manufacturer)
        .bind(&input.model)
        .bind(input.department_id)
        .bind(input.user_id)
        .bind(input.status.unwrap_or(0))
        .bind(&input.purchase_date)
        .bind(input.purchase_price)
        .bind(input.quantity)
        .bind(input.used_quantity)
        .bind(&input.expire_date)
        .bind(&input.description)
        .bind(current_user_id) // updated_by
        .execute(&pool)
        .await
        .map_err(|e| format!("插入资产主表失败: {}", e))?;

    // 插入 hard_assets 扩展表
    let sql = format!(
        r#"
        INSERT INTO {}hard_assets (id, asset_id, sn, mac_address, location, hardware_config,
            use_user_id, use_start_date, maintenance_vendor, maintenance_type,
            maintenance_expire_date, fault_desc, created_by, created_at, updated_by, updated_at, deleted)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8::timestamp, $9, $10,
            $11::timestamp, $12, $13, NOW(), $13, NOW(), 0)
        "#,
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(hard_id)
        .bind(asset_id)
        .bind(&input.sn)
        .bind(&input.mac_address)
        .bind(&input.location)
        .bind(&input.hardware_config)
        .bind(input.use_user_id)
        .bind(&input.use_start_date)
        .bind(&input.maintenance_vendor)
        .bind(&input.maintenance_type)
        .bind(&input.maintenance_expire_date)
        .bind(&input.fault_desc)
        .bind(current_user_id)
        .execute(&pool)
        .await
        .map_err(|e| format!("插入硬件扩展表失败: {}", e))?;

    // 返回刚插入的数据
    let result = get_hardware_asset_by_id(asset_id).await;
    if let Ok(ref asset) = result {
        info!(
            "新增固定资产成功: id={}, name={}, asset_no={}",
            asset.id, asset.asset_name, asset.asset_no
        );
    }
    result
}

/// 根据ID查询单个固定资产
async fn get_hardware_asset_by_id(asset_id: i64) -> Result<HardwareAssetView, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    let sql = format!("{} AND a.id = $1", hardware_select_sql(&prefix));
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(asset_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| {
            error!("查询固定资产失败: id={}, error={}", asset_id, e);
            format!("查询固定资产失败: {}", e)
        })?
        .ok_or_else(|| {
            warn!("固定资产不存在: id={}", asset_id);
            "固定资产不存在".to_string()
        })?;

    Ok(row_to_hardware_view(&row))
}

/// 修改固定资产
pub async fn update_hardware_asset(
    id: i64,
    input: HardwareAssetInput,
    current_user_id: i64,
) -> Result<HardwareAssetView, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    info!("更新固定资产: id={}, name={}", id, input.asset_name);

    // 更新 assets 主表
    let sql = format!(
        r#"
        UPDATE {}assets SET
            category_id = $2, asset_name = $3, manufacturer = $4, model = $5,
            department_id = $6, user_id = $7, status = $8,
            purchase_date = $9::timestamp, purchase_price = $10,
            quantity = $11, used_quantity = $12,
            expire_date = $13::timestamp, description = $14,
            updated_by = $15, updated_at = NOW()
        WHERE id = $1 AND asset_type = 'fixed' AND (deleted IS NULL OR deleted = 0)
        "#,
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(input.category_id)
        .bind(&input.asset_name)
        .bind(&input.manufacturer)
        .bind(&input.model)
        .bind(input.department_id)
        .bind(input.user_id)
        .bind(input.status.unwrap_or(0))
        .bind(&input.purchase_date)
        .bind(input.purchase_price)
        .bind(input.quantity)
        .bind(input.used_quantity)
        .bind(&input.expire_date)
        .bind(&input.description)
        .bind(current_user_id)
        .execute(&pool)
        .await
        .map_err(|e| format!("更新资产主表失败: {}", e))?;

    // 检查 hard_assets 是否存在，存在则更新，不存在则插入
    let existing = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
        "SELECT id FROM {}hard_assets WHERE asset_id = $1 AND (deleted IS NULL OR deleted = 0)",
        prefix
    )))
    .bind(id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("查询硬件扩展表失败: {}", e))?;

    if let Some(hard_id) = existing {
        let sql = format!(
            r#"
            UPDATE {}hard_assets SET
                sn = $2, mac_address = $3, location = $4, hardware_config = $5,
                use_user_id = $6, use_start_date = $7::timestamp,
                maintenance_vendor = $8, maintenance_type = $9,
                maintenance_expire_date = $10::timestamp, fault_desc = $11,
                updated_by = $12, updated_at = NOW()
            WHERE id = $1
            "#,
            prefix
        );
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(hard_id)
            .bind(&input.sn)
            .bind(&input.mac_address)
            .bind(&input.location)
            .bind(&input.hardware_config)
            .bind(input.use_user_id)
            .bind(&input.use_start_date)
            .bind(&input.maintenance_vendor)
            .bind(&input.maintenance_type)
            .bind(&input.maintenance_expire_date)
            .bind(&input.fault_desc)
            .bind(current_user_id)
            .execute(&pool)
            .await
            .map_err(|e| format!("更新硬件扩展表失败: {}", e))?;
    } else {
        let new_hard_id = next_id() as i64;
        let sql = format!(
            r#"
            INSERT INTO {}hard_assets (id, asset_id, sn, mac_address, location, hardware_config,
                use_user_id, use_start_date, maintenance_vendor, maintenance_type,
                maintenance_expire_date, fault_desc, created_by, created_at, updated_by, updated_at, deleted)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8::timestamp, $9, $10,
                $11::timestamp, $12, $13, NOW(), $13, NOW(), 0)
            "#,
            prefix
        );
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(new_hard_id)
            .bind(id)
            .bind(&input.sn)
            .bind(&input.mac_address)
            .bind(&input.location)
            .bind(&input.hardware_config)
            .bind(input.use_user_id)
            .bind(&input.use_start_date)
            .bind(&input.maintenance_vendor)
            .bind(&input.maintenance_type)
            .bind(&input.maintenance_expire_date)
            .bind(&input.fault_desc)
            .bind(current_user_id)
            .execute(&pool)
            .await
            .map_err(|e| format!("插入硬件扩展表失败: {}", e))?;
    }

    let result = get_hardware_asset_by_id(id).await;
    if let Ok(ref asset) = result {
        info!("更新固定资产成功: id={}, name={}", id, asset.asset_name);
    }
    result
}

/// 删除固定资产（软删除）
pub async fn delete_hardware_asset(id: i64) -> Result<(), String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    info!("删除固定资产: id={}", id);

    let sql = format!(
        "UPDATE {}assets SET deleted = 1, updated_at = NOW() WHERE id = $1 AND asset_type = 'fixed'",
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("删除固定资产失败: id={}, error={}", id, e);
            format!("删除固定资产失败: {}", e)
        })?;

    info!("删除固定资产成功: id={}", id);
    Ok(())
}

// ======================== 无形资产 CRUD ========================

fn intangible_select_sql(prefix: &str) -> String {
    format!(
        r#"
SELECT
    a.id, a.asset_no, a.asset_type, a.category_id, a.asset_name,
    a.manufacturer, a.model, a.department_id, a.user_id, a.status,
    a.purchase_date::text as purchase_date, a.purchase_price, a.quantity, a.used_quantity,
    a.expire_date::text as expire_date, a.description,
    a.created_by, a.created_at::text as created_at, a.updated_by, a.updated_at::text as updated_at, a.deleted,
    i.id as intangible_id, i.intangible_type, i.register_no, i.register_owner,
    i.register_date::text as register_date, i.valid_start_date::text as valid_start_date,
    i.valid_end_date::text as valid_end_date, i.right_status,
    i.license_key, i.license_type, i.authorized_scope, i.assigned_user_ids,
    i.bind_type, i.bind_info, i.version, i.download_link,
    i.amortization_method, i.useful_life, i.amortization_amount, i.residual_rate
FROM {}assets a
LEFT JOIN {}intangible_assets i ON i.asset_id = a.id AND (i.deleted IS NULL OR i.deleted = 0)
WHERE a.asset_type = 'intangible' AND (a.deleted IS NULL OR a.deleted = 0)
"#,
        prefix, prefix
    )
}

/// 获取所有无形资产列表（JOIN assets + intangible_assets）
pub async fn get_intangible_assets() -> Result<Vec<IntangibleAssetView>, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    let sql = format!(
        "{} ORDER BY a.created_at DESC",
        intangible_select_sql(&prefix)
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            error!("查询无形资产列表失败: {}", e);
            format!("查询无形资产失败: {}", e)
        })?;

    let result: Vec<IntangibleAssetView> = rows.iter().map(|r| row_to_intangible_view(r)).collect();
    let count = result.len();
    info!("查询无形资产列表成功: 共 {} 条记录", count);
    Ok(result)
}

/// 新增无形资产
pub async fn insert_intangible_asset(
    input: IntangibleAssetInput,
    current_user_id: i64,
) -> Result<IntangibleAssetView, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let asset_id = next_id() as i64;
    let intangible_id = next_id() as i64;
    let asset_no = format!("{}", next_id());
    let prefix = database::schema_prefix();

    info!(
        "新增无形资产: name={}, category_id={}",
        input.asset_name, input.category_id
    );

    // 插入 assets 主表
    let sql = format!(
        r#"
        INSERT INTO {}assets (id, asset_no, asset_type, category_id, asset_name, manufacturer, model,
            department_id, user_id, status, purchase_date, purchase_price, quantity, used_quantity,
            expire_date, description, created_by, created_at, updated_by, updated_at, deleted)
        VALUES ($1, $2, 'intangible', $3, $4, $5, $6, $7, $8, $9,
            $10::timestamp, $11, $12, $13, $14::timestamp, $15,
            $16, NOW(), $16, NOW(), 0)
        "#,
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(asset_id)
        .bind(&asset_no)
        .bind(input.category_id)
        .bind(&input.asset_name)
        .bind(&input.manufacturer)
        .bind(&input.model)
        .bind(input.department_id)
        .bind(input.user_id)
        .bind(input.status.unwrap_or(0))
        .bind(&input.purchase_date)
        .bind(input.purchase_price)
        .bind(input.quantity)
        .bind(input.used_quantity)
        .bind(&input.expire_date)
        .bind(&input.description)
        .bind(current_user_id)
        .execute(&pool)
        .await
        .map_err(|e| format!("插入资产主表失败: {}", e))?;

    // 插入 intangible_assets 扩展表
    let sql = format!(
        r#"
        INSERT INTO {}intangible_assets (
            id, asset_id, intangible_type, register_no, register_owner, register_date,
            valid_start_date, valid_end_date, right_status,
            license_key, license_type, authorized_scope, assigned_user_ids,
            bind_type, bind_info, version, download_link,
            amortization_method, useful_life, amortization_amount, residual_rate,
            created_by, created_at, updated_by, updated_at, deleted)
        VALUES ($1, $2, $3, $4, $5, $6::timestamp,
            $7::timestamp, $8::timestamp, $9,
            $10, $11, $12, $13,
            $14, $15, $16, $17,
            $18, $19, $20, $21,
            $22, NOW(), $22, NOW(), 0)
        "#,
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(intangible_id)
        .bind(asset_id)
        .bind(&input.intangible_type)
        .bind(&input.register_no)
        .bind(&input.register_owner)
        .bind(&input.register_date)
        .bind(&input.valid_start_date)
        .bind(&input.valid_end_date)
        .bind(&input.right_status)
        .bind(&input.license_key)
        .bind(&input.license_type)
        .bind(&input.authorized_scope)
        .bind(&input.assigned_user_ids)
        .bind(&input.bind_type)
        .bind(&input.bind_info)
        .bind(&input.version)
        .bind(&input.download_link)
        .bind(&input.amortization_method)
        .bind(input.useful_life)
        .bind(input.amortization_amount)
        .bind(input.residual_rate)
        .bind(current_user_id)
        .execute(&pool)
        .await
        .map_err(|e| format!("插入无形资产扩展表失败: {}", e))?;

    let result = get_intangible_asset_by_id(asset_id).await;
    if let Ok(ref asset) = result {
        info!(
            "新增无形资产成功: id={}, name={}, asset_no={}",
            asset.id, asset.asset_name, asset.asset_no
        );
    }
    result
}

/// 根据ID查询单个无形资产
async fn get_intangible_asset_by_id(asset_id: i64) -> Result<IntangibleAssetView, String> {
    let pool = database::get_read_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    let sql = format!("{} AND a.id = $1", intangible_select_sql(&prefix));
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(asset_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| {
            error!("查询无形资产失败: id={}, error={}", asset_id, e);
            format!("查询无形资产失败: {}", e)
        })?
        .ok_or_else(|| {
            warn!("无形资产不存在: id={}", asset_id);
            "无形资产不存在".to_string()
        })?;

    Ok(row_to_intangible_view(&row))
}

/// 修改无形资产
pub async fn update_intangible_asset(
    id: i64,
    input: IntangibleAssetInput,
    current_user_id: i64,
) -> Result<IntangibleAssetView, String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    info!("更新无形资产: id={}, name={}", id, input.asset_name);

    // 更新 assets 主表
    let sql = format!(
        r#"
        UPDATE {}assets SET
            category_id = $2, asset_name = $3, manufacturer = $4, model = $5,
            department_id = $6, user_id = $7, status = $8,
            purchase_date = $9::timestamp, purchase_price = $10,
            quantity = $11, used_quantity = $12,
            expire_date = $13::timestamp, description = $14,
            updated_by = $15, updated_at = NOW()
        WHERE id = $1 AND asset_type = 'intangible' AND (deleted IS NULL OR deleted = 0)
        "#,
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(input.category_id)
        .bind(&input.asset_name)
        .bind(&input.manufacturer)
        .bind(&input.model)
        .bind(input.department_id)
        .bind(input.user_id)
        .bind(input.status.unwrap_or(0))
        .bind(&input.purchase_date)
        .bind(input.purchase_price)
        .bind(input.quantity)
        .bind(input.used_quantity)
        .bind(&input.expire_date)
        .bind(&input.description)
        .bind(current_user_id)
        .execute(&pool)
        .await
        .map_err(|e| format!("更新资产主表失败: {}", e))?;

    // 检查 intangible_assets 是否存在
    let existing = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
        "SELECT id FROM {}intangible_assets WHERE asset_id = $1 AND deleted = 0",
        prefix
    )))
    .bind(id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("查询无形资产扩展表失败: {}", e))?;

    if let Some(intangible_id) = existing {
        let sql = format!(
            r#"
            UPDATE {}intangible_assets SET
                intangible_type = $2, register_no = $3, register_owner = $4,
                register_date = $5::timestamp, valid_start_date = $6::timestamp,
                valid_end_date = $7::timestamp, right_status = $8,
                license_key = $9, license_type = $10, authorized_scope = $11,
                assigned_user_ids = $12, bind_type = $13, bind_info = $14,
                version = $15, download_link = $16,
                amortization_method = $17, useful_life = $18,
                amortization_amount = $19, residual_rate = $20,
                updated_by = $21, updated_at = NOW()
            WHERE id = $1
            "#,
            prefix
        );
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(intangible_id)
            .bind(&input.intangible_type)
            .bind(&input.register_no)
            .bind(&input.register_owner)
            .bind(&input.register_date)
            .bind(&input.valid_start_date)
            .bind(&input.valid_end_date)
            .bind(&input.right_status)
            .bind(&input.license_key)
            .bind(&input.license_type)
            .bind(&input.authorized_scope)
            .bind(&input.assigned_user_ids)
            .bind(&input.bind_type)
            .bind(&input.bind_info)
            .bind(&input.version)
            .bind(&input.download_link)
            .bind(&input.amortization_method)
            .bind(input.useful_life)
            .bind(input.amortization_amount)
            .bind(input.residual_rate)
            .bind(current_user_id)
            .execute(&pool)
            .await
            .map_err(|e| format!("更新无形资产扩展表失败: {}", e))?;
    } else {
        let new_intangible_id = next_id() as i64;
        let sql = format!(
            r#"
            INSERT INTO {}intangible_assets (
                id, asset_id, intangible_type, register_no, register_owner, register_date,
                valid_start_date, valid_end_date, right_status,
                license_key, license_type, authorized_scope, assigned_user_ids,
                bind_type, bind_info, version, download_link,
                amortization_method, useful_life, amortization_amount, residual_rate,
                created_by, created_at, updated_by, updated_at, deleted)
            VALUES ($1, $2, $3, $4, $5, $6::timestamp,
                $7::timestamp, $8::timestamp, $9,
                $10, $11, $12, $13,
                $14, $15, $16, $17,
                $18, $19, $20, $21,
                $22, NOW(), $22, NOW(), 0)
            "#,
            prefix
        );
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(new_intangible_id)
            .bind(id)
            .bind(&input.intangible_type)
            .bind(&input.register_no)
            .bind(&input.register_owner)
            .bind(&input.register_date)
            .bind(&input.valid_start_date)
            .bind(&input.valid_end_date)
            .bind(&input.right_status)
            .bind(&input.license_key)
            .bind(&input.license_type)
            .bind(&input.authorized_scope)
            .bind(&input.assigned_user_ids)
            .bind(&input.bind_type)
            .bind(&input.bind_info)
            .bind(&input.version)
            .bind(&input.download_link)
            .bind(&input.amortization_method)
            .bind(input.useful_life)
            .bind(input.amortization_amount)
            .bind(input.residual_rate)
            .bind(current_user_id)
            .execute(&pool)
            .await
            .map_err(|e| format!("插入无形资产扩展表失败: {}", e))?;
    }

    let result = get_intangible_asset_by_id(id).await;
    if let Ok(ref asset) = result {
        info!("更新无形资产成功: id={}, name={}", id, asset.asset_name);
    }
    result
}

/// 删除无形资产（软删除）
pub async fn delete_intangible_asset(id: i64) -> Result<(), String> {
    let pool = database::get_write_pool().map_err(|e| format!("获取数据库连接失败: {}", e))?;
    let prefix = database::schema_prefix();

    info!("删除无形资产: id={}", id);

    let sql = format!(
        "UPDATE {}assets SET deleted = 1, updated_at = NOW() WHERE id = $1 AND asset_type = 'intangible'",
        prefix
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|e| {
            error!("删除无形资产失败: id={}, error={}", id, e);
            format!("删除无形资产失败: {}", e)
        })?;

    info!("删除无形资产成功: id={}", id);
    Ok(())
}
