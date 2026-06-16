pub mod models;
pub mod postgres;

use anyhow::Result;
use sqlx::PgPool;

/// 初始化数据库（PostgreSQL）
///
/// 1. 从环境变量加载配置
/// 2. 自动创建数据库（如不存在）
/// 3. 初始化读写分离连接池
/// 4. 创建表结构
pub async fn init_database() -> Result<()> {
    let config = postgres::PostgresConfig::from_env()?;
    postgres::init_postgres_pool(config).await?;
    let pool = postgres::get_write_pool()?;
    postgres::init_postgres_tables(&pool).await?;
    Ok(())
}

/// 获取写连接池（用于 INSERT / UPDATE / DELETE，兼容旧接口）
pub fn get_pool() -> Result<PgPool> {
    postgres::get_write_pool()
}

/// 获取写连接池（用于 INSERT / UPDATE / DELETE）
pub fn get_write_pool() -> Result<PgPool> {
    postgres::get_write_pool()
}

/// 获取读连接池（用于 SELECT）
///
/// 如果有从库，使用加权轮询算法选择从库；
/// 如果没有从库，回退到主库。
pub fn get_read_pool() -> Result<PgPool> {
    postgres::get_read_pool()
}

/// 关闭所有数据库连接
pub async fn close_all_databases() {
    postgres::close_postgres_pool().await;
}
