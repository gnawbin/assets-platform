//! PostgreSQL 数据库连接和初始化模块
//!
//! 提供 PostgreSQL 数据库的连接池管理、连接测试和数据初始化功能
//! 支持读写分离：主库（写）+ 多个从库（读，加权轮询负载均衡）

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

/// 默认 PostgreSQL 数据库名称（始终存在，用于管理操作）
const DEFAULT_POSTGRES_DB: &str = "postgres";

/// 从库副本配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaConfig {
    /// 从库主机地址
    pub host: String,
    /// 从库端口
    pub port: u16,
    /// 负载权重（值越大，分配的请求越多）
    pub weight: u32,
    /// 连接池最大连接数
    pub max_connections: u32,
    /// 连接池最小连接数
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,
}

fn default_min_connections() -> u32 {
    1
}

impl Default for ReplicaConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            weight: 1,
            max_connections: 10,
            min_connections: 1,
        }
    }
}

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
    /// 从库副本列表（可选，不配置则读写都走主库）
    #[serde(default)]
    pub read_replicas: Vec<ReplicaConfig>,
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
            read_replicas: Vec::new(),
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

        // 从环境变量读取从库配置（格式：READ_HOSTS="host1:port1:weight1,host2:port2:weight2"）
        let read_replicas = Self::parse_read_replicas_from_env();

        Ok(Self {
            host,
            port,
            database,
            username,
            password,
            read_replicas,
            ..Default::default()
        })
    }

    /// 从环境变量解析从库配置
    fn parse_read_replicas_from_env() -> Vec<ReplicaConfig> {
        let read_hosts = match std::env::var("PG_READ_HOSTS") {
            Ok(v) if !v.is_empty() => v,
            _ => return Vec::new(),
        };

        read_hosts
            .split(',')
            .filter_map(|part| {
                let part = part.trim();
                if part.is_empty() {
                    return None;
                }
                let segments: Vec<&str> = part.split(':').collect();
                let host = segments.first()?.to_string();
                let port = segments
                    .get(1)
                    .and_then(|s| s.parse::<u16>().ok())
                    .unwrap_or(5432);
                let weight = segments
                    .get(2)
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(1);
                Some(ReplicaConfig {
                    host,
                    port,
                    weight,
                    max_connections: 10,
                    min_connections: 1,
                })
            })
            .collect()
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

    /// 构建指定主机的连接选项
    pub fn connect_options_for_host(&self, host: &str, port: u16) -> Result<PgConnectOptions> {
        let conn_str = format!(
            "postgres://{}:{}@{}:{}/{}",
            self.username, self.password, host, port, self.database
        );
        PgConnectOptions::from_str(&conn_str)
            .map_err(|e| anyhow!("Failed to parse connection string for {}: {}", host, e))
    }
}

/// 创建连接池的辅助函数
async fn create_pool(
    options: PgConnectOptions,
    max_connections: u32,
    min_connections: u32,
    connect_timeout: u64,
    idle_timeout: u64,
) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .min_connections(min_connections)
        .acquire_timeout(Duration::from_secs(connect_timeout))
        .idle_timeout(Duration::from_secs(idle_timeout))
        .connect_with(options)
        .await
        .map_err(|e| anyhow!("Failed to create PostgreSQL pool: {}", e))?;

    // 测试连接
    sqlx::query("SELECT 1")
        .fetch_one(&pool)
        .await
        .map_err(|e| anyhow!("PostgreSQL connection test failed: {}", e))?;

    Ok(pool)
}

/// 读写分离连接池管理器
///
/// 管理一个主库（写）和多个从库（读，加权轮询负载均衡）
pub struct PgPoolManager {
    /// 主库连接池（写操作）
    write_pool: PgPool,
    /// 从库连接池列表（读操作）
    read_pools: Vec<PgPool>,
    /// 每个从库对应的权重
    read_weights: Vec<u32>,
    /// 总权重
    total_weight: u32,
    /// 轮询计数器
    read_counter: AtomicUsize,
}

impl PgPoolManager {
    /// 创建新的读写分离连接池管理器
    pub async fn new(config: &PostgresConfig) -> Result<Self> {
        // 1. 创建主库连接池
        let write_options = config.connect_options()?;
        let write_pool = create_pool(
            write_options,
            config.max_connections,
            config.min_connections,
            config.connect_timeout,
            config.idle_timeout,
        )
        .await?;

        // 2. 创建从库连接池
        let mut read_pools = Vec::new();
        let mut read_weights = Vec::new();
        let mut total_weight: u32 = 0;

        for replica in &config.read_replicas {
            let read_options = config.connect_options_for_host(&replica.host, replica.port)?;
            match create_pool(
                read_options,
                replica.max_connections,
                replica.min_connections,
                config.connect_timeout,
                config.idle_timeout,
            )
            .await
            {
                Ok(pool) => {
                    tracing::info!(
                        "已连接到从库: {}:{}, 权重: {}",
                        replica.host,
                        replica.port,
                        replica.weight
                    );
                    read_pools.push(pool);
                    read_weights.push(replica.weight);
                    total_weight += replica.weight;
                }
                Err(e) => {
                    tracing::warn!(
                        "连接从库 {}:{} 失败: {}，将跳过此从库",
                        replica.host,
                        replica.port,
                        e
                    );
                }
            }
        }

        if read_pools.is_empty() && !config.read_replicas.is_empty() {
            tracing::warn!("所有从库连接失败，读操作将回退到主库");
        }

        Ok(Self {
            write_pool,
            read_pools,
            read_weights,
            total_weight,
            read_counter: AtomicUsize::new(0),
        })
    }

    /// 获取写连接池（用于 INSERT / UPDATE / DELETE）
    pub fn write(&self) -> &PgPool {
        &self.write_pool
    }

    /// 获取读连接池（用于 SELECT）
    ///
    /// 如果有从库，使用加权轮询算法选择从库；
    /// 如果没有从库，回退到主库。
    pub fn read(&self) -> &PgPool {
        if self.read_pools.is_empty() {
            return &self.write_pool;
        }

        let count = self.read_counter.fetch_add(1, Ordering::Relaxed);
        let total = self.total_weight as usize;
        let mut pos = count % total;

        for (i, &w) in self.read_weights.iter().enumerate() {
            let weight = w as usize;
            if pos < weight {
                return &self.read_pools[i];
            }
            pos -= weight;
        }

        // 兜底：返回第一个从库
        &self.read_pools[0]
    }

    /// 关闭所有连接池
    pub async fn close(self) {
        self.write_pool.close().await;
        for pool in self.read_pools {
            pool.close().await;
        }
    }
}

/// 全局读写分离连接池管理器
static PG_POOL_MANAGER: OnceLock<RwLock<Option<PgPoolManager>>> = OnceLock::new();

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
        read_replicas: Vec::new(),
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

/// 初始化全局读写分离连接池
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

    // 2. 创建读写分离连接池管理器
    let manager = PgPoolManager::new(&config).await?;

    let replica_count = config.read_replicas.len();
    if replica_count > 0 {
        tracing::info!(
            "读写分离已启用: 1个主库 + {}个从库（加权轮询）",
            replica_count
        );
    } else {
        tracing::info!("未配置从库，读写均使用主库");
    }

    PG_POOL_MANAGER.get_or_init(|| RwLock::new(Some(manager)));
    Ok(())
}

/// 从环境变量初始化全局 PostgreSQL 连接池
#[allow(dead_code)]
pub async fn init_postgres_pool_from_env() -> Result<()> {
    let config = PostgresConfig::from_env()?;
    init_postgres_pool(config).await
}

/// 获取写连接池（用于 INSERT / UPDATE / DELETE）
#[allow(dead_code)]
pub fn get_write_pool() -> Result<PgPool> {
    let guard = PG_POOL_MANAGER
        .get()
        .ok_or_else(|| anyhow!("PostgreSQL pool manager not initialized"))?
        .read()
        .map_err(|_| anyhow!("Failed to acquire read lock on PostgreSQL pool manager"))?;

    match guard.as_ref() {
        Some(manager) => Ok(manager.write().clone()),
        None => Err(anyhow!("PostgreSQL pool manager is None")),
    }
}

/// 获取读连接池（用于 SELECT）
///
/// 如果有从库，使用加权轮询算法选择从库；
/// 如果没有从库，回退到主库。
#[allow(dead_code)]
pub fn get_read_pool() -> Result<PgPool> {
    let guard = PG_POOL_MANAGER
        .get()
        .ok_or_else(|| anyhow!("PostgreSQL pool manager not initialized"))?
        .read()
        .map_err(|_| anyhow!("Failed to acquire read lock on PostgreSQL pool manager"))?;

    match guard.as_ref() {
        Some(manager) => Ok(manager.read().clone()),
        None => Err(anyhow!("PostgreSQL pool manager is None")),
    }
}

/// 获取全局 PostgreSQL 连接池（兼容旧接口，内部调用 get_write_pool）
#[allow(dead_code)]
pub fn get_postgres_pool() -> Result<PgPool> {
    get_write_pool()
}

/// 检查全局 PostgreSQL 连接池是否已初始化
#[allow(dead_code)]
pub fn is_postgres_pool_initialized() -> bool {
    PG_POOL_MANAGER.get().is_some()
}

/// 关闭全局 PostgreSQL 连接池
#[allow(dead_code)]
pub async fn close_postgres_pool() {
    if let Some(lock) = PG_POOL_MANAGER.get() {
        let manager_opt = lock.write().unwrap().take();
        if let Some(manager) = manager_opt {
            manager.close().await;
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

    // 获取写连接池（建表操作需要写权限）
    let pool = get_write_pool()?;

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
        assert!(config.read_replicas.is_empty());
    }

    #[test]
    fn test_postgres_config_connection_string() {
        let config = PostgresConfig::default();
        let conn_str = config.connection_string();
        assert!(conn_str.contains("postgres://postgres:postgres@localhost:5432/assets_platform"));
    }

    #[test]
    fn test_parse_read_replicas_from_env() {
        // 模拟环境变量
        std::env::set_var("PG_READ_HOSTS", "192.168.1.101:5432:3,192.168.1.102:5432:1");
        let replicas = PostgresConfig::parse_read_replicas_from_env();
        assert_eq!(replicas.len(), 2);
        assert_eq!(replicas[0].host, "192.168.1.101");
        assert_eq!(replicas[0].port, 5432);
        assert_eq!(replicas[0].weight, 3);
        assert_eq!(replicas[1].host, "192.168.1.102");
        assert_eq!(replicas[1].weight, 1);
        std::env::remove_var("PG_READ_HOSTS");
    }

    #[test]
    fn test_parse_read_replicas_empty() {
        std::env::remove_var("PG_READ_HOSTS");
        let replicas = PostgresConfig::parse_read_replicas_from_env();
        assert!(replicas.is_empty());
    }
}
