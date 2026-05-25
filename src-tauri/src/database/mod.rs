pub mod models;
pub mod postgres;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use std::env;
use std::sync::{OnceLock, RwLock};

use sqlx::PgPool;

/// 数据库配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// PostgreSQL 连接字符串
    pub postgres_url: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            postgres_url: "postgres://postgres:postgres@localhost:5432/assets_platform".to_string(),
        }
    }
}

impl DatabaseConfig {
    /// 从环境变量加载数据库配置
    pub fn from_env() -> Self {
        let postgres_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
            format!(
                "postgres://{}:{}@{}:{}/{}",
                env::var("PG_USERNAME").unwrap_or_else(|_| "postgres".to_string()),
                env::var("PG_PASSWORD").unwrap_or_else(|_| "postgres".to_string()),
                env::var("PG_HOST").unwrap_or_else(|_| "localhost".to_string()),
                env::var("PG_PORT").unwrap_or_else(|_| "5432".to_string()),
                env::var("PG_DATABASE").unwrap_or_else(|_| "assets_platform".to_string()),
            )
        });

        Self { postgres_url }
    }
}

/// 数据库管理器
#[allow(dead_code)]
pub struct DatabaseManager {
    /// 数据库连接池（PostgreSQL）
    pool: Option<PgPool>,
    /// 数据库配置
    config: DatabaseConfig,
}

#[allow(dead_code)]
impl DatabaseManager {
    /// 创建新的数据库管理器
    pub fn new(config: DatabaseConfig) -> Self {
        Self { pool: None, config }
    }

    /// 初始化数据库连接
    pub async fn init(&mut self) -> Result<()> {
        let config = postgres::PostgresConfig::from_env()?;
        postgres::init_postgres_pool(config).await?;
        let pool = postgres::get_postgres_pool()?;
        self.pool = Some(pool);
        Ok(())
    }

    /// 获取数据库连接池
    pub fn pool(&self) -> Result<&PgPool> {
        self.pool
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("数据库未初始化"))
    }

    /// 检查数据库是否已连接
    pub fn is_connected(&self) -> bool {
        self.pool.is_some()
    }

    /// 关闭所有数据库连接
    pub async fn close(&mut self) {
        if let Some(pool) = self.pool.take() {
            pool.close().await;
        }
    }
}

/// 全局数据库管理器实例
static DB_MANAGER: OnceLock<RwLock<Option<DatabaseManager>>> = OnceLock::new();

/// 获取全局数据库管理器
fn get_db_manager() -> &'static RwLock<Option<DatabaseManager>> {
    DB_MANAGER.get_or_init(|| RwLock::new(None))
}

/// 初始化数据库（PostgreSQL）
pub async fn init_database() -> Result<()> {
    let config = DatabaseConfig::from_env();
    let mut manager = DatabaseManager::new(config);
    manager.init().await?;

    let lock = get_db_manager();
    let mut guard = lock
        .write()
        .map_err(|_| anyhow::anyhow!("获取数据库管理器写锁失败"))?;
    *guard = Some(manager);
    Ok(())
}

/// 获取数据库连接池（便捷函数）
pub fn get_pool() -> Result<PgPool> {
    let lock = get_db_manager();
    let guard = lock
        .read()
        .map_err(|_| anyhow::anyhow!("获取数据库管理器读锁失败"))?;

    match guard.as_ref() {
        Some(manager) => manager.pool().cloned(),
        None => Err(anyhow::anyhow!("数据库管理器未初始化")),
    }
}

/// 关闭所有数据库连接
pub async fn close_all_databases() {
    let lock = get_db_manager();
    let mut guard = lock.write().unwrap();
    if let Some(ref mut manager) = *guard {
        manager.close().await;
    }
    *guard = None;
}
