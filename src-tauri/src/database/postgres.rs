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

/// 当前租户 schema 名称（初始化时设置）
static CURRENT_SCHEMA: OnceLock<RwLock<String>> = OnceLock::new();

/// 获取当前租户 schema 名称
pub fn get_current_schema() -> String {
    CURRENT_SCHEMA
        .get()
        .map(|lock| lock.read().unwrap().clone())
        .unwrap_or_else(|| "public".to_string())
}

/// 设置当前租户 schema 名称
pub fn set_current_schema(schema: &str) {
    let lock = CURRENT_SCHEMA.get_or_init(|| RwLock::new("public".to_string()));
    *lock.write().unwrap() = schema.to_string();
}

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

/// 执行 SQL 文件内容（通过 include_str! 编译时嵌入）
async fn execute_sql_content(pool: &PgPool, sql: &str, label: &str) -> Result<()> {
    // 按分号分割多条 SQL 语句，逐条执行
    for (i, statement) in sql.split(';').enumerate() {
        let trimmed = statement.trim();
        if trimmed.is_empty() {
            continue;
        }
        // 跳过纯注释行（以 -- 开头），但如果包含非注释内容则保留
        let mut lines = trimmed.lines();
        let first_content = lines.find(|line| {
            let l = line.trim();
            !l.is_empty() && !l.starts_with("--")
        });
        let sql_to_execute = match first_content {
            Some(_) => trimmed,
            None => continue,
        };
        sqlx::query(sql_to_execute)
            .execute(pool)
            .await
            .map_err(|e| {
                anyhow!(
                    "Failed to execute SQL statement {}#{}: {}\nSQL: {}",
                    label,
                    i,
                    e,
                    &sql_to_execute[..std::cmp::min(sql_to_execute.len(), 200)]
                )
            })?;
    }
    tracing::info!("SQL 文件 '{}' 执行成功", label);
    Ok(())
}

/// 执行带 {schema} 占位符的 SQL 文件内容
async fn execute_sql_content_with_schema(
    pool: &PgPool,
    sql: &str,
    schema: &str,
    label: &str,
) -> Result<()> {
    let replaced = sql.replace("{schema}", schema);
    execute_sql_content(pool, &replaced, label).await
}

/// 初始化 public schema 表结构
#[allow(dead_code)]
pub async fn init_public_tables(pool: &PgPool) -> Result<()> {
    let sql = include_str!("sql/public_tables.sql");
    execute_sql_content(pool, sql, "public_tables").await
}

/// 执行 public schema 迁移脚本（补充旧表缺失列）
#[allow(dead_code)]
pub async fn migrate_public_tables(pool: &PgPool) -> Result<()> {
    let sql = include_str!("sql/public_migration.sql");
    execute_sql_content(pool, sql, "public_migration").await
}

/// 初始化租户 schema 表结构
#[allow(dead_code)]
pub async fn init_tenant_tables(pool: &PgPool, schema: &str) -> Result<()> {
    let sql = include_str!("sql/tenant_tables.sql");
    execute_sql_content_with_schema(pool, sql, schema, "tenant_tables").await
}

/// 初始化 public schema 默认数据（租户、菜单等）
#[allow(dead_code)]
pub async fn init_public_default_data(pool: &PgPool) -> Result<()> {
    let sql = include_str!("sql/public_initial_data.sql");
    execute_sql_content(pool, sql, "public_initial_data").await
}

/// 初始化租户 schema 默认数据（角色、部门等）
#[allow(dead_code)]
pub async fn init_tenant_default_data(pool: &PgPool, schema: &str) -> Result<()> {
    let sql = include_str!("sql/tenant_initial_data.sql");
    execute_sql_content_with_schema(pool, sql, schema, "tenant_initial_data").await
}

/// 创建默认管理员账号（密码用 argon2 加密）
#[allow(dead_code)]
pub async fn init_default_admin(pool: &PgPool) -> Result<()> {
    use argon2::password_hash::SaltString;
    use argon2::{Argon2, PasswordHasher};

    // 检查是否已存在 admin 用户
    let exists: bool = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM public.sys_user WHERE username = 'admin')",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| anyhow!("检查 admin 用户是否存在时出错: {}", e))?;

    if exists {
        tracing::info!("admin 用户已存在，跳过创建");
        return Ok(());
    }

    // 从环境变量读取密码，默认 admin123
    let password =
        std::env::var("DEFAULT_ADMIN_PASSWORD").unwrap_or_else(|_| "admin123".to_string());

    // argon2 加密（使用系统随机数生成器）
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!("密码加密失败: {}", e))?
        .to_string();

    // 插入 admin 用户（超级管理员）
    sqlx::query(
        r#"
        INSERT INTO public.sys_user (id, username, passwd, real_name, is_super_admin, tenant_id, created_at)
        VALUES (1, 'admin', $1, '超级管理员', true, NULL, NOW())
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(&hash)
    .execute(pool)
    .await
    .map_err(|e| anyhow!("创建 admin 用户失败: {}", e))?;

    tracing::info!("默认管理员账号创建成功（用户名: admin）");
    Ok(())
}

/// 完整的 PostgreSQL 数据库初始化（多租户 schema 模式）
#[allow(dead_code)]
pub async fn init_postgres_database(config: PostgresConfig) -> Result<()> {
    // 1. 初始化连接池
    init_postgres_pool(config).await?;

    // 获取写连接池
    let pool = get_write_pool()?;

    // 2. 确保 public schema 存在
    tracing::info!("正在确保 public schema 存在...");
    sqlx::query("CREATE SCHEMA IF NOT EXISTS public")
        .execute(&pool)
        .await
        .map_err(|e| anyhow!("创建 public schema 失败: {}", e))?;

    // 3. 初始化 public schema 表结构
    tracing::info!("正在初始化 public schema 表结构...");
    init_public_tables(&pool).await?;

    // 4. 执行 public schema 迁移脚本（补充旧表缺失列）
    tracing::info!("正在执行 public schema 迁移脚本...");
    migrate_public_tables(&pool).await?;

    // 5. 初始化 public schema 默认数据（租户、菜单）
    tracing::info!("正在初始化 public schema 默认数据...");
    init_public_default_data(&pool).await?;

    // 6. 创建默认管理员账号
    tracing::info!("正在创建默认管理员账号...");
    init_default_admin(&pool).await?;

    // 7. 读取默认租户 schema 名称
    let schema: String = sqlx::query_scalar::<_, String>(
        "SELECT schema_name FROM public.sys_tenant WHERE id = 1 AND enable = true",
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| anyhow!("读取默认租户 schema 失败: {}", e))?
    .unwrap_or_else(|| "single".to_string());

    tracing::info!("默认租户 schema: {}", schema);

    // 8. 创建租户 schema（如果不存在）
    tracing::info!("正在创建租户 schema '{}'...", schema);
    sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {}", schema))
        .execute(&pool)
        .await
        .map_err(|e| anyhow!("创建 schema '{}' 失败: {}", schema, e))?;

    // 9. 初始化租户 schema 表结构
    tracing::info!("正在初始化租户 '{}' 表结构...", schema);
    init_tenant_tables(&pool, &schema).await?;

    // 10. 初始化租户 schema 默认数据
    tracing::info!("正在初始化租户 '{}' 默认数据...", schema);
    init_tenant_default_data(&pool, &schema).await?;

    // 11. 设置当前 schema（供 service 层查询使用）
    set_current_schema(&schema);
    tracing::info!("当前租户 schema 已设置为: {}", schema);

    tracing::info!("数据库初始化完成！");
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
