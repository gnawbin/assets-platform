pub mod models;
pub mod postgres;

use anyhow::Result;
use sqlx::PgPool;
use std::future::Future;
use std::sync::{OnceLock, RwLock};

/// 每个请求的 schema 上下文（tokio task local）
///
/// HTTP 模式下，auth_middleware 通过 with_schema() 设置此值。
/// Tauri 模式下不经过 auth_middleware，回退到 GLOBAL_SCHEMA。
tokio::task_local! {
    pub static CURRENT_SCHEMA: String;
}

/// 全局当前 schema（供 Tauri 模式使用）
///
/// Tauri 命令不经过 HTTP 中间件，无法通过 CURRENT_SCHEMA 获取 schema。
/// switch_tenant 命令会设置此全局变量，schema_prefix() 在
/// CURRENT_SCHEMA 未设置时回退到此处。
static GLOBAL_SCHEMA: OnceLock<RwLock<String>> = OnceLock::new();

/// 获取全局 schema 读写锁
fn get_global_schema() -> &'static RwLock<String> {
    GLOBAL_SCHEMA.get_or_init(|| RwLock::new("public".to_string()))
}

/// 设置全局当前 schema（Tauri 模式下由 switch_tenant 调用）
pub fn set_global_schema(schema: String) {
    if let Ok(mut guard) = get_global_schema().write() {
        *guard = schema;
    }
}

/// 在请求上下文中执行带 schema 的操作
///
/// 在 auth middleware 或 route handler 中调用此函数，为后续所有 `schema_prefix()`
/// 调用提供正确的 schema 值。
///
/// # 用法
/// ```ignore
/// with_schema(schema_name.to_string(), async {
/// let data = some_service::get_data().await?;
/// Ok(Json(data))
/// }).await
/// ```
pub async fn with_schema<F, T>(schema: String, f: F) -> T
where
    F: Future<Output = T>,
{
    CURRENT_SCHEMA.scope(schema, f).await
}

/// 获取 schema 前缀（例如 "tenant_b."）
///
/// - 优先级：CURRENT_SCHEMA（HTTP 模式）> GLOBAL_SCHEMA（Tauri 模式）> "public"
/// - 如果 schema 为 "public" 或 "public"，返回空字符串
/// - 否则返回 "{schema}." 格式的前缀
pub fn schema_prefix() -> String {
    let schema = CURRENT_SCHEMA
        .try_with(|s| s.clone())
        // HTTP 模式未设置时，回退到全局 schema（Tauri 模式使用）
        .unwrap_or_else(|_| {
            get_global_schema()
                .read()
                .map(|g| g.clone())
                .unwrap_or_else(|_| "public".to_string())
        });
    if schema == "public" || schema.is_empty() {
        String::new()
    } else {
        format!("{}.", schema)
    }
}

/// 初始化数据库（PostgreSQL）
///
/// 1. 从环境变量加载配置
/// 2. 自动创建数据库（如不存在）
/// 3. 初始化读写分离连接池
/// 4. 创建 public schema 表结构
/// 5. 初始化 public schema 默认数据（租户、菜单）
/// 6. 创建默认管理员账号
/// 7. 创建租户 schema 表结构
/// 8. 初始化租户 schema 默认数据
pub async fn init_database() -> Result<()> {
    let config = postgres::PostgresConfig::from_env()?;
    postgres::init_postgres_database(config).await
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
