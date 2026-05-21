//! PostgreSQL 数据库连接和初始化模块
//! 
//! 提供 PostgreSQL 数据库的连接池管理、连接测试和数据初始化功能

use anyhow::{Result, anyhow};
use sqlx::{PgPool, postgres::{PgConnectOptions, PgPoolOptions}};
use std::str::FromStr;
use std::sync::{OnceLock, RwLock};
use std::time::Duration;
use serde::{Deserialize, Serialize};

/// PostgreSQL 数据库配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresConfig {
    /// 数据库主机地址
    pub host: String,
    /// 数据库端口
    pub port: u16,
    /// 数据库名称
    pub database: String,
    /// 用户名
    pub username: String,
    /// 密码
    pub password: String,
    /// 连接池最大连接数
    pub max_connections: u32,
    /// 连接池最小连接数
    pub min_connections: u32,
    /// 连接超时时间（秒）
    pub connect_timeout: u64,
    /// 空闲连接超时时间（秒）
    pub idle_timeout: u64,
}

impl Default for PostgresConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            database: "assets_platform".to_string(),
            username: "postgres".to_string(),
            password: "postgres".to_string(),
            max_connections: 10,
            min_connections: 2,
            connect_timeout: 30,
            idle_timeout: 300,
        }
    }
}

impl PostgresConfig {
    /// 从环境变量创建配置
    pub fn from_env() -> Result<Self> {
        let host = std::env::var("PG_HOST").unwrap_or_else(|_| "localhost".to_string());
        let port = std::env::var("PG_PORT")
            .unwrap_or_else(|_| "5432".to_string())
            .parse::<u16>()
            .map_err(|e| anyhow!("Invalid PG_PORT: {}", e))?;
        let database = std::env::var("PG_DATABASE").unwrap_or_else(|_| "assets_platform".to_string());
        let username = std::env::var("PG_USERNAME").unwrap_or_else(|_| "postgres".to_string());
        let password = std::env::var("PG_PASSWORD").unwrap_or_else(|_| "postgres".to_string());
        
        Ok(Self {
            host,
            port,
            database,
            username,
            password,
            ..Default::default()
        })
    }
    
    /// 构建连接字符串
    pub fn connection_string(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}",
            self.username, self.password, self.host, self.port, self.database
        )
    }
    
    /// 构建连接选项
    pub fn connect_options(&self) -> Result<PgConnectOptions> {
        let conn_str = self.connection_string();
        PgConnectOptions::from_str(&conn_str)
            .map_err(|e| anyhow!("Failed to parse connection string: {}", e))
    }
}

/// PostgreSQL 连接池管理器
#[allow(dead_code)]
pub struct PostgresManager {
    config: PostgresConfig,
    pool: Option<PgPool>,
}

#[allow(dead_code)]
impl PostgresManager {
    /// 创建新的 PostgreSQL 管理器
    pub fn new(config: PostgresConfig) -> Self {
        Self {
            config,
            pool: None,
        }
    }
    
    /// 从环境变量创建 PostgreSQL 管理器
    pub fn from_env() -> Result<Self> {
        let config = PostgresConfig::from_env()?;
        Ok(Self::new(config))
    }
    
    /// 连接到 PostgreSQL 数据库
    pub async fn connect(&mut self) -> Result<()> {
        let options = self.config.connect_options()?;
        
        let pool = PgPoolOptions::new()
            .max_connections(self.config.max_connections)
            .min_connections(self.config.min_connections)
            .acquire_timeout(Duration::from_secs(self.config.connect_timeout))
            .idle_timeout(Duration::from_secs(self.config.idle_timeout))
            .connect_with(options)
            .await
            .map_err(|e| anyhow!("Failed to connect to PostgreSQL: {}", e))?;
        
        // 测试连接
        sqlx::query("SELECT 1")
            .fetch_one(&pool)
            .await
            .map_err(|e| anyhow!("PostgreSQL connection test failed: {}", e))?;
        
        self.pool = Some(pool);
        Ok(())
    }
    
    /// 断开数据库连接
    pub async fn disconnect(&mut self) {
        if let Some(pool) = self.pool.take() {
            pool.close().await;
        }
    }
    
    /// 获取数据库连接池
    pub fn pool(&self) -> Result<&PgPool> {
        self.pool.as_ref().ok_or_else(|| anyhow!("PostgreSQL not connected"))
    }
    
    /// 检查是否已连接
    pub fn is_connected(&self) -> bool {
        self.pool.is_some()
    }
    
    /// 获取数据库配置
    pub fn config(&self) -> &PostgresConfig {
        &self.config
    }
}

/// 全局 PostgreSQL 连接池
static POSTGRES_POOL: OnceLock<RwLock<Option<PgPool>>> = OnceLock::new();

/// 初始化全局 PostgreSQL 连接池
#[allow(dead_code)]
pub async fn init_postgres_pool(config: PostgresConfig) -> Result<()> {
    let options = config.connect_options()?;
    
    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
     
       .acquire_timeout(Duration::from_secs(config.connect_timeout))
        .idle_timeout(Duration::from_secs(config.idle_timeout))
        .connect_with(options)
        .await
        .map_err(|e| anyhow!("Failed to connect to PostgreSQL: {}", e))?;
    
    // 测试连接
    sqlx::query("SELECT 1")
        .fetch_one(&pool)
        .await
        .map_err(|e| anyhow!("PostgreSQL connection test failed: {}", e))?;
    
    POSTGRES_POOL.get_or_init(|| RwLock::new(Some(pool)));
    Ok(())
}

/// 从环境变量初始化全局 PostgreSQL 连接池
#[allow(dead_code)]
pub async fn init_postgres_pool_from_env() -> Result<()> {
    let config = PostgresConfig::from_env()?;
    init_postgres_pool(config).await
}

/// 获取全局 PostgreSQL 连接池
#[allow(dead_code)]
pub fn get_postgres_pool() -> Result<PgPool> {
    let guard = POSTGRES_POOL
        .get()
        .ok_or_else(|| anyhow!("PostgreSQL pool not initialized"))?
        .read()
        .map_err(|_| anyhow!("Failed to acquire read lock on PostgreSQL pool"))?;
    
    guard.as_ref().cloned().ok_or_else(|| anyhow!("PostgreSQL pool is None"))
}

/// 检查全局 PostgreSQL 连接池是否已初始化
#[allow(dead_code)]
pub fn is_postgres_pool_initialized() -> bool {
    POSTGRES_POOL.get().is_some()
}

/// 关闭全局 PostgreSQL 连接池
#[allow(dead_code)]
pub async fn close_postgres_pool() {
    if let Some(lock) = POSTGRES_POOL.get() {
        let pool_opt = lock.write().unwrap().take();
        if let Some(pool) = pool_opt {
            pool.close().await;
        }
    }
}

/// 测试 PostgreSQL 连接
#[allow(dead_code)]
pub async fn test_postgres_connection(config: &PostgresConfig) -> Result<()> {
    let options = config.connect_options()?;
    
    let pool = PgPoolOptions::new()
        .max_connections(1)

       .acquire_timeout(Duration::from_secs(config.connect_timeout))
        .connect_with(options)
        .await
        .map_err(|e| anyhow!("Failed to connect to PostgreSQL: {}", e))?;
    
    // 测试连接
    sqlx::query("SELECT 1")
        .fetch_one(&pool)
        .await
        .map_err(|e| anyhow!("PostgreSQL connection test failed: {}", e))?;
    
    pool.close().await;
    Ok(())
}

/// 初始化 PostgreSQL 数据库表结构
#[allow(dead_code)]
pub async fn init_postgres_tables(pool: &PgPool) -> Result<()> {
    // 创建系统配置表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS system_config (
            id BIGSERIAL PRIMARY KEY,
            config_key VARCHAR(255) NOT NULL UNIQUE,
            config_value TEXT NOT NULL,
            remark TEXT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
        )
        "#
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create system_config table: {}", e))?;
    
    // 创建数据库配置表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS db_config (
            id BIGSERIAL PRIMARY KEY,
            host VARCHAR(255) NOT NULL,
            port INTEGER NOT NULL,
            db_name VARCHAR(255) NOT NULL,
            username VARCHAR(255) NOT NULL,
            password VARCHAR(255) NOT NULL,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
        )
        "#
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create db_config table: {}", e))?;
    
    // 创建资产分类表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS asset_category (
            id BIGSERIAL PRIMARY KEY,
            category_name VARCHAR(255) NOT NULL,
            asset_type VARCHAR(50) NOT NULL,
            parent_id BIGINT DEFAULT 0,
            sort SMALLINT DEFAULT 0,
            description TEXT,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
        )
        "#
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create asset_category table: {}", e))?;
    
    // 创建资产表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS assets (
            id BIGSERIAL PRIMARY KEY,
            asset_no VARCHAR(100) NOT NULL UNIQUE,
            asset_type VARCHAR(50) NOT NULL,
            category_id BIGINT NOT NULL,
            asset_name VARCHAR(255) NOT NULL,
            manufacturer VARCHAR(255),
            model VARCHAR(255),
            serial_no VARCHAR(255),
            purchase_date DATE,
            purchase_price DECIMAL(15, 2),
            status VARCHAR(50) DEFAULT 'active',
            location VARCHAR(255),
            responsible_person VARCHAR(255),
            description TEXT,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (category_id) REFERENCES asset_category(id)
        )
        "#
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create assets table: {}", e))?;
    
    // 创建用户表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id BIGSERIAL PRIMARY KEY,
            username VARCHAR(100) NOT NULL UNIQUE,
            email VARCHAR(255) NOT NULL UNIQUE,
            password_hash VARCHAR(255) NOT NULL,
            full_name VARCHAR(255),
            role VARCHAR(50) DEFAULT 'user',
            is_active BOOLEAN DEFAULT true,
            last_login TIMESTAMP WITH TIME ZONE,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
        )
        "#
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create users table: {}", e))?;
    
    // 创建索引
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_assets_asset_no ON assets(asset_no)")
        .execute(pool)
        .await
        .map_err(|e| anyhow!("Failed to create index on assets: {}", e))?;
    
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_assets_category_id ON assets(category_id)")
        .execute(pool)
        .await
        .map_err(|e| anyhow!("Failed to create index on assets: {}", e))?;
    
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_asset_category_parent_id ON asset_category(parent_id)")
        .execute(pool)
        .await
        .map_err(|e| anyhow!("Failed to create index on asset_category: {}", e))?;
    
    Ok(())
}

/// 插入初始数据
#[allow(dead_code)]
pub async fn insert_initial_data(pool: &PgPool) -> Result<()> {
    // 插入默认系统配置
    sqlx::query(
        r#"
        INSERT INTO system_config (config_key, config_value, remark)
        VALUES 
            ('system_name', '资产管理系统', '系统名称'),
            ('system_version', '1.0.0', '系统版本'),
            ('default_language', 'zh-CN', '默认语言'),
            ('pagination_size', '20', '分页大小')
        ON CONFLICT (config_key) DO NOTHING
        "#
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to insert system config: {}", e))?;
    
    // 插入默认资产分类
    sqlx::query(
        r#"
        INSERT INTO asset_category (category_name, asset_type, parent_id, sort, description)
        VALUES 
            ('IT设备', 'hardware', 0, 1, '信息技术设备'),
            ('服务器', 'hardware', 1, 1, '服务器设备'),
            ('网络设备', 'hardware', 1, 2, '网络设备'),
            ('存储设备', 'hardware', 1, 3, '存储设备'),
            ('办公设备', 'hardware', 0, 2, '办公设备'),
            ('软件资产', 'software', 0, 3, '软件许可证和订阅'),
            ('操作系统', 'software', 6, 1, '操作系统软件'),
            ('办公软件', 'software', 6, 2, '办公软件'),
            ('开发工具', 'software', 6, 3, '开发工具软件')
        ON CONFLICT (id) DO NOTHING
        "#
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to insert asset categories: {}", e))?;
    
    // 插入默认用户（密码为 admin123 的哈希值）
    sqlx::query(
        r#"
        INSERT INTO users (username, email, password_hash, full_name, role)
        VALUES 
            ('admin', 'admin@example.com', '$2b$12$EixZaYVK1fsbw1ZfbX3OXePaWxn96p36WQoeG6Lruj3vjPGga31lW', '系统管理员', 'admin')
        ON CONFLICT (username) DO NOTHING
        "#
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to insert default user: {}", e))?;
    
    Ok(())
}

/// 完整的 PostgreSQL 数据库初始化
#[allow(dead_code)]
pub async fn init_postgres_database(config: PostgresConfig) -> Result<()> {
    // 初始化连接池
    init_postgres_pool(config).await?;
    
    // 获取连接池
    let pool = get_postgres_pool()?;
    
    // 初始化表结构
    init_postgres_tables(&pool).await?;
    
    // 插入初始数据
    insert_initial_data(&pool).await?;
    
    Ok(())
}

/// 从环境变量初始化 PostgreSQL 数据库
#[allow(dead_code)]
pub async fn init_postgres_database_from_env() -> Result<()> {
    let config = PostgresConfig::from_env()?;
    init_postgres_database(config).await
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_postgres_config_default() {
        let config = PostgresConfig::default();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 5432);
        assert_eq!(config.database, "assets_platform");
        assert_eq!(config.username, "postgres");
        assert_eq!(config.password, "postgres");
    }
    
    #[test]
    fn test_postgres_config_connection_string() {
        let config = PostgresConfig::default();
        let conn_str = config.connection_string();
        assert!(conn_str.contains("postgres://postgres:postgres@localhost:5432/assets_platform"));
    }
    
    #[tokio::test]
    async fn test_postgres_manager() {
        // 注意：这个测试需要实际的 PostgreSQL 数据库才能运行
        // 在实际环境中，应该使用测试数据库或模拟连接
        let config = PostgresConfig::default();
        let mut manager = PostgresManager::new(config);
        
        // 测试连接（在实际环境中取消注释）
        // let result = manager.connect().await;
        // assert!(result.is_ok() || result.is_err()); // 连接可能成功或失败，取决于数据库是否可用
        
        // 测试配置获取
        assert_eq!(manager.config().host, "localhost");
    }
}