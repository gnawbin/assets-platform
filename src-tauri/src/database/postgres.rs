//! PostgreSQL 数据库连接和初始化模块
//!
//! 提供 PostgreSQL 数据库的连接池管理、连接测试和数据初始化功能

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::str::FromStr;
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

/// 默认 PostgreSQL 数据库名称（始终存在，用于管理操作）
const DEFAULT_POSTGRES_DB: &str = "postgres";

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
        let database =
            std::env::var("PG_DATABASE").unwrap_or_else(|_| "assets_platform".to_string());
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
        Self { config, pool: None }
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
        self.pool
            .as_ref()
            .ok_or_else(|| anyhow!("PostgreSQL not connected"))
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

/// 确保目标数据库存在，若不存在则自动创建
///
/// 该函数会先连接到默认的 `postgres` 数据库，检查目标数据库是否存在，
/// 如果不存在则执行 `CREATE DATABASE` 创建它。
/// 需要 PostgreSQL 用户拥有 `CREATEDB` 权限。
#[allow(dead_code)]
pub async fn ensure_database_exists(config: &PostgresConfig) -> Result<()> {
    // 使用默认的 postgres 数据库进行管理操作
    let admin_config = PostgresConfig {
        database: DEFAULT_POSTGRES_DB.to_string(),
        max_connections: 2,
        min_connections: 1,
        ..config.clone()
    };

    let admin_url = admin_config.connection_string();
    let admin_options = PgConnectOptions::from_str(&admin_url)
        .map_err(|e| anyhow!("无法解析管理连接字符串: {}", e))?;

    // 创建临时连接池连接到默认 postgres 数据库
    let admin_pool = PgPoolOptions::new()
        .max_connections(2)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(config.connect_timeout))
        .connect_with(admin_options)
        .await
        .map_err(|e| {
            anyhow!(
                "无法连接到 PostgreSQL 服务器（默认 postgres 数据库）: {}",
                e
            )
        })?;

    // 检查目标数据库是否存在
    let db_exists: bool = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)",
    )
    .bind(&config.database)
    .fetch_one(&admin_pool)
    .await
    .map_err(|e| anyhow!("检查数据库是否存在时出错: {}", e))?;

    if !db_exists {
        tracing::info!("数据库 '{}' 不存在，正在自动创建...", config.database);

        // CREATE DATABASE 不能使用参数化查询，需要对数据库名进行安全处理
        let create_sql = format!(
            "CREATE DATABASE \"{}\"",
            config
                .database
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect::<String>()
        );

        sqlx::query(&create_sql)
            .execute(&admin_pool)
            .await
            .map_err(|e| {
                anyhow!(
                    "自动创建数据库 '{}' 失败: {}。\n\
                     请手动创建数据库：\n\
                     1. 打开 PostgreSQL 命令行或 pgAdmin\n\
                     2. 执行: CREATE DATABASE \"{}\";\n\
                     3. 然后重新启动应用",
                    config.database,
                    e,
                    config.database
                )
            })?;

        tracing::info!("数据库 '{}' 创建成功！", config.database);
    } else {
        tracing::info!("数据库 '{}' 已存在，跳过创建。", config.database);
    }

    // 关闭管理连接池
    admin_pool.close().await;
    Ok(())
}

/// 初始化全局 PostgreSQL 连接池
#[allow(dead_code)]
pub async fn init_postgres_pool(config: PostgresConfig) -> Result<()> {
    // 1. 确保目标数据库存在（自动创建）
    match ensure_database_exists(&config).await {
        Ok(()) => {}
        Err(e) => {
            tracing::warn!("自动创建数据库失败: {}", e);
            // 继续尝试连接，让连接池的错误提供更详细的信息
        }
    }

    // 2. 连接到目标数据库
    let options = config.connect_options()?;

    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(Duration::from_secs(config.connect_timeout))
        .idle_timeout(Duration::from_secs(config.idle_timeout))
        .connect_with(options)
        .await
        .map_err(|e| anyhow!("连接到 PostgreSQL 失败: {}", e))?;

    // 3. 测试连接
    sqlx::query("SELECT 1")
        .fetch_one(&pool)
        .await
        .map_err(|e| anyhow!("PostgreSQL 连接测试失败: {}", e))?;

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

    guard
        .as_ref()
        .cloned()
        .ok_or_else(|| anyhow!("PostgreSQL pool is None"))
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
///
/// 注意：此函数创建的表结构与 models.rs 中的实体定义保持一致。
/// 所有表均支持软删除（deleted 字段），使用 Snowflake ID 作为主键。
#[allow(dead_code)]
pub async fn init_postgres_tables(pool: &PgPool) -> Result<()> {
    // ======================== 资产分类表 ========================
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS asset_category (
            id BIGINT PRIMARY KEY,
            category_name VARCHAR(255) NOT NULL,
            asset_type VARCHAR(50) NOT NULL,
            parent_id BIGINT,
            sort SMALLINT DEFAULT 0,
            description TEXT,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create asset_category table: {}", e))?;

    // ======================== 资产主表 ========================
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS assets (
            id BIGINT PRIMARY KEY,
            asset_no VARCHAR(100) NOT NULL UNIQUE,
            asset_type VARCHAR(50) NOT NULL,
            category_id BIGINT NOT NULL,
            asset_name VARCHAR(255) NOT NULL,
            manufacturer VARCHAR(255),
            model VARCHAR(255),
            department_id BIGINT,
            user_id BIGINT,
            status SMALLINT DEFAULT 0,
            purchase_date TIMESTAMP,
            purchase_price DECIMAL(15, 2),
            quantity INT DEFAULT 1,
            used_quantity INT DEFAULT 0,
            expire_date TIMESTAMP,
            description TEXT,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0,
            FOREIGN KEY (category_id) REFERENCES asset_category(id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create assets table: {}", e))?;

    // ======================== 固定资产扩展表 ========================
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS hard_assets (
            id BIGINT PRIMARY KEY,
            asset_id BIGINT NOT NULL UNIQUE,
            sn VARCHAR(255),
            mac_address VARCHAR(255),
            location VARCHAR(255),
            maintenance_vendor VARCHAR(255),
            maintenance_type VARCHAR(50),
            maintenance_expire_date TIMESTAMP,
            hardware_config TEXT,
            use_user_id BIGINT,
            use_start_date TIMESTAMP,
            fault_desc TEXT,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0,
            FOREIGN KEY (asset_id) REFERENCES assets(id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create hard_assets table: {}", e))?;

    // ======================== 无形资产扩展表 ========================
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS intangible_assets (
            id BIGINT PRIMARY KEY,
            asset_id BIGINT NOT NULL UNIQUE,
            intangible_type VARCHAR(50) NOT NULL,
            register_no VARCHAR(255),
            register_owner VARCHAR(255),
            register_date TIMESTAMP,
            valid_start_date TIMESTAMP,
            valid_end_date TIMESTAMP,
            right_status VARCHAR(50),
            license_key VARCHAR(255),
            license_type VARCHAR(50),
            authorized_scope TEXT,
            assigned_user_ids TEXT,
            bind_type VARCHAR(50),
            bind_info TEXT,
            version VARCHAR(100),
            download_link TEXT,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0,
            FOREIGN KEY (asset_id) REFERENCES assets(id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create intangible_assets table: {}", e))?;

    // ======================== 用户表 ========================
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sys_user (
            id BIGINT PRIMARY KEY,
            username VARCHAR(100) NOT NULL UNIQUE,
            passwd VARCHAR(255) NOT NULL,
            domain VARCHAR(100),
            real_name VARCHAR(255) NOT NULL,
            email VARCHAR(255),
            phone VARCHAR(50),
            department_id BIGINT,
            status SMALLINT DEFAULT 1,
            nickname VARCHAR(255),
            avatar TEXT,
            person_id VARCHAR(50),
            person_code VARCHAR(50),
            super_user_id BIGINT,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create sys_user table: {}", e))?;

    // ======================== 部门表 ========================
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS department (
            id BIGINT PRIMARY KEY,
            department_name VARCHAR(255) NOT NULL,
            parent_id BIGINT,
            description TEXT,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create department table: {}", e))?;

    // ======================== 角色表 ========================
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sys_role (
            id BIGINT PRIMARY KEY,
            role_key VARCHAR(100) NOT NULL UNIQUE,
            role_name VARCHAR(255) NOT NULL,
            description TEXT,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create sys_role table: {}", e))?;

    // ======================== 菜单表 ========================
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sys_menu (
            id BIGINT PRIMARY KEY,
            menu_name VARCHAR(255) NOT NULL,
            parent_id BIGINT,
            path VARCHAR(255),
            component VARCHAR(255),
            icon VARCHAR(100),
            order_num SMALLINT DEFAULT 0,
            visible BOOLEAN DEFAULT true,
            perms VARCHAR(255),
            menu_type SMALLINT DEFAULT 1,
            hidden_button BOOLEAN DEFAULT false,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create sys_menu table: {}", e))?;

    // ======================== 用户角色关联表 ========================
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sys_user_role (
            id BIGINT PRIMARY KEY,
            user_id BIGINT NOT NULL,
            role_id BIGINT NOT NULL,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0,
            FOREIGN KEY (user_id) REFERENCES sys_user(id),
            FOREIGN KEY (role_id) REFERENCES sys_role(id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create sys_user_role table: {}", e))?;

    // ======================== 角色菜单关联表 ========================
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sys_role_menu (
            id BIGINT PRIMARY KEY,
            role_id BIGINT NOT NULL,
            menu_id BIGINT NOT NULL,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0,
            FOREIGN KEY (role_id) REFERENCES sys_role(id),
            FOREIGN KEY (menu_id) REFERENCES sys_menu(id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create sys_role_menu table: {}", e))?;

    // ======================== 资产文档表 ========================
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS asset_documents (
            id BIGINT PRIMARY KEY,
            asset_id BIGINT NOT NULL,
            doc_type VARCHAR(50) NOT NULL,
            doc_name VARCHAR(255) NOT NULL,
            doc_no VARCHAR(100) NOT NULL,
            party_a VARCHAR(255) NOT NULL,
            party_b VARCHAR(255) NOT NULL,
            sign_date TIMESTAMP,
            effective_date TIMESTAMP,
            expire_date TIMESTAMP,
            file_path TEXT NOT NULL,
            file_name VARCHAR(255) NOT NULL,
            file_size BIGINT NOT NULL,
            remark TEXT,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0,
            FOREIGN KEY (asset_id) REFERENCES assets(id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create asset_documents table: {}", e))?;

    // ======================== 资产知识表 ========================
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS asset_knowledge (
            id BIGINT PRIMARY KEY,
            asset_id BIGINT NOT NULL,
            doc_source VARCHAR(50) NOT NULL,
            knowledge_type VARCHAR(50) NOT NULL,
            title VARCHAR(255) NOT NULL,
            content TEXT NOT NULL,
            chunk_index INT DEFAULT 0,
            vector_data REAL[],
            permission_level VARCHAR(50) DEFAULT 'public',
            owner_type VARCHAR(50),
            owner_id BIGINT,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0,
            FOREIGN KEY (asset_id) REFERENCES assets(id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create asset_knowledge table: {}", e))?;

    // ======================== 流程相关表 ========================

    // 资产领用申请表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS asset_receive (
            id BIGINT PRIMARY KEY,
            receive_no VARCHAR(100) NOT NULL UNIQUE,
            asset_id BIGINT NOT NULL,
            user_id BIGINT NOT NULL,
            department_id BIGINT NOT NULL,
            receive_date TIMESTAMP WITH TIME ZONE NOT NULL,
            reason TEXT NOT NULL,
            status SMALLINT DEFAULT 0,
            approve_by BIGINT,
            approve_time TIMESTAMP WITH TIME ZONE,
            approve_remark TEXT,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0,
            FOREIGN KEY (asset_id) REFERENCES assets(id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create asset_receive table: {}", e))?;

    // 资产归还确认表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS asset_return (
            id BIGINT PRIMARY KEY,
            return_no VARCHAR(100) NOT NULL UNIQUE,
            receive_id BIGINT NOT NULL,
            asset_id BIGINT NOT NULL,
            user_id BIGINT NOT NULL,
            return_date TIMESTAMP WITH TIME ZONE NOT NULL,
            asset_status SMALLINT DEFAULT 0,
            remark TEXT,
            confirm_by BIGINT NOT NULL,
            confirm_time TIMESTAMP WITH TIME ZONE NOT NULL,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0,
            FOREIGN KEY (asset_id) REFERENCES assets(id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create asset_return table: {}", e))?;

    // 资产调拨表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS asset_transfer (
            id BIGINT PRIMARY KEY,
            transfer_no VARCHAR(100) NOT NULL UNIQUE,
            asset_id BIGINT NOT NULL,
            out_dept_id BIGINT NOT NULL,
            in_dept_id BIGINT NOT NULL,
            out_user_id BIGINT NOT NULL,
            in_user_id BIGINT NOT NULL,
            transfer_date TIMESTAMP WITH TIME ZONE NOT NULL,
            reason TEXT NOT NULL,
            status SMALLINT DEFAULT 0,
            approve_by BIGINT,
            approve_time TIMESTAMP WITH TIME ZONE,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0,
            FOREIGN KEY (asset_id) REFERENCES assets(id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create asset_transfer table: {}", e))?;

    // 资产维修表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS asset_repair (
            id BIGINT PRIMARY KEY,
            repair_no VARCHAR(100) NOT NULL UNIQUE,
            asset_id BIGINT NOT NULL,
            fault_desc TEXT NOT NULL,
            repair_desc TEXT,
            repair_user_id BIGINT,
            repair_dept_id BIGINT,
            repair_file_url TEXT,
            repair_type SMALLINT DEFAULT 0,
            vendor VARCHAR(255),
            cost DECIMAL(15, 2),
            apply_date TIMESTAMP WITH TIME ZONE NOT NULL,
            repair_date TIMESTAMP WITH TIME ZONE,
            finish_date TIMESTAMP WITH TIME ZONE,
            status SMALLINT DEFAULT 0,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0,
            FOREIGN KEY (asset_id) REFERENCES assets(id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create asset_repair table: {}", e))?;

    // 资产报废表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS asset_scrap (
            id BIGINT PRIMARY KEY,
            scrap_no VARCHAR(100) NOT NULL UNIQUE,
            asset_id BIGINT NOT NULL,
            reason TEXT NOT NULL,
            scrap_date TIMESTAMP WITH TIME ZONE NOT NULL,
            status SMALLINT DEFAULT 0,
            approve_by BIGINT,
            approve_time TIMESTAMP WITH TIME ZONE,
            handle_user BIGINT,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0,
            FOREIGN KEY (asset_id) REFERENCES assets(id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create asset_scrap table: {}", e))?;

    // 资产采购申请表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS asset_purchase (
            id BIGINT PRIMARY KEY,
            purchase_no VARCHAR(100) NOT NULL UNIQUE,
            asset_name VARCHAR(255) NOT NULL,
            category_id BIGINT NOT NULL,
            model VARCHAR(255),
            manufacturer VARCHAR(255),
            quantity INT NOT NULL,
            unit_price DECIMAL(15, 2),
            total_price DECIMAL(15, 2),
            apply_user BIGINT NOT NULL,
            dept_id BIGINT NOT NULL,
            reason TEXT NOT NULL,
            status SMALLINT DEFAULT 0,
            supplier VARCHAR(255),
            purchase_date TIMESTAMP WITH TIME ZONE,
            arrive_date TIMESTAMP WITH TIME ZONE,
            created_by BIGINT,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_by BIGINT,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            deleted SMALLINT DEFAULT 0,
            FOREIGN KEY (category_id) REFERENCES asset_category(id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create asset_purchase table: {}", e))?;

    // 通用审批记录表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS asset_approval (
            id BIGSERIAL PRIMARY KEY,
            biz_type SMALLINT NOT NULL,
            biz_id BIGINT NOT NULL,
            step SMALLINT NOT NULL,
            approver_id BIGINT NOT NULL,
            approve_status SMALLINT DEFAULT 0,
            remark TEXT,
            approve_time TIMESTAMP WITH TIME ZONE,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create asset_approval table: {}", e))?;

    // ======================== 创建索引 ========================
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_assets_asset_no ON assets(asset_no)")
        .execute(pool)
        .await
        .map_err(|e| anyhow!("Failed to create index on assets: {}", e))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_assets_category_id ON assets(category_id)")
        .execute(pool)
        .await
        .map_err(|e| anyhow!("Failed to create index on assets: {}", e))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_asset_category_parent_id ON asset_category(parent_id)",
    )
    .execute(pool)
    .await
    .map_err(|e| anyhow!("Failed to create index on asset_category: {}", e))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_sys_user_username ON sys_user(username)")
        .execute(pool)
        .await
        .map_err(|e| anyhow!("Failed to create index on sys_user: {}", e))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_sys_role_role_key ON sys_role(role_key)")
        .execute(pool)
        .await
        .map_err(|e| anyhow!("Failed to create index on sys_role: {}", e))?;

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
}
