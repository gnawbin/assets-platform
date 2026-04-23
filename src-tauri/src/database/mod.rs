
pub mod models;
pub mod dual_database;
pub mod public_init;
pub mod encryption;
pub mod secure_init;
pub mod postgres;
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions, Row};
use anyhow::Result;
use std::env;
use std::fs;
use std::str::FromStr;
use std::sync::{OnceLock, RwLock};
use serde::{Deserialize, Serialize};
use std::path::Path;
use sha2::Sha256;

pub use dual_database::{
    DualDatabaseManager,
    DatabaseStatus,
    SecureDbState,
    get_wallet_database_pool,
    PUBLIC_DB_PATH,
    SECURE_DB_PATH,
};

pub use public_init::init_public_database;
pub use secure_init::{
    init_secure_database,
    unlock_secure_database,
    lock_secure_database,
    change_secure_password,
    is_secure_database_initialized,
};




static PUBLIC_INIT_SQL_CONTENT: &str = include_str!("../../data/public_init.sql");

/// 全局数据库连接池
static DATABASE_POOL: OnceLock<RwLock<SqlitePool>> = OnceLock::new();

/// 数据库管理器
pub struct DatabaseManager {
    pool: SqlitePool,
}

impl DatabaseManager {
    pub async fn new(database_url: &str) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await?;
        sqlx::query("PRAGMA foreign_keys = ON").execute(&pool).await?;
        Ok(Self { pool })
    }

    pub fn get_pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn new_encrypted(database_url: &str, db_password: &str) -> Result<Self> {
        let pwd = db_password.to_string();
        let options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);

        let pool = sqlx::pool::PoolOptions::new()
            .max_connections(1)
            .after_connect(move |conn, _| {
                let password = pwd.clone();
                Box::pin(async move {
                    let sql = format!("PRAGMA key = '{}'", password.replace("'", "''"));
                    sqlx::query(&sql).execute(&mut *conn).await?;
                    Ok(())
                })
            })
            .connect_with(options)
            .await?;

        Ok(Self { pool })
    }
}

/// 初始化全局池
fn init_global_pool(pool: SqlitePool) {
    let _ = DATABASE_POOL.set(RwLock::new(pool));
}

/// 更新全局池
fn update_global_pool(pool: SqlitePool) {
    if let Some(lock) = DATABASE_POOL.get() {
        *lock.write().unwrap() = pool;
    } else {
        init_global_pool(pool);
    }
}

/// 获取数据库池
pub fn get_database_pool() -> SqlitePool {
    if DualDatabaseManager::public_pool_ready() {
        return DualDatabaseManager::public_pool();
    }
    DATABASE_POOL.get().unwrap().read().unwrap().clone()
}

/// 导出数据库结构
#[tauri::command]
pub async fn export_database_to_init_sql() -> Result<String, String> {
    let pool = DualDatabaseManager::public_pool();
    let mut sql = String::new();

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;

    for table in tables {
        let create: String = sqlx::query_scalar(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name = ?"
        )
        .bind(&table)
        .fetch_one(&pool)
        .await
        .map_err(|e| e.to_string())?;

        sql.push_str(&create);
        sql.push_str(";\n");
    }

    fs::write("data/public_init.sql", sql).map_err(|e| e.to_string())?;
    Ok("导出成功".into())
}

/// 恢复出厂设置
#[tauri::command]
pub async fn reload_database() -> Result<String, String> {
    DualDatabaseManager::force_disconnect_all().await;
    if let Some(lock) = DATABASE_POOL.get() {
        lock.read().unwrap().clone().close().await;
    }

    let paths = ["data/public.db", "data/secure.db"];
    for path in paths {
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(format!("{path}-wal"));
        let _ = fs::remove_file(format!("{path}-shm"));
    }

    init_public_database().await.map_err(|e| e.to_string())?;
    Ok("重置完成".into())
}

/// 检查数据库状态
#[tauri::command]
pub async fn is_wallet_db_ready() -> Result<bool, String> {
    let pool = get_database_pool();
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE name='wallets'"
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(count > 0)
}

/// 检查数据库结构
#[tauri::command]
pub async fn check_database_schema() -> Result<serde_json::Value, String> {
    let pool = get_database_pool();
    let chains: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE name='chains'"
    ).fetch_one(&pool).await.map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "chains_table_exists": chains > 0,
    }))
}
