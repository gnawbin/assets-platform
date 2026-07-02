//! PostgreSQL 持久化提供者
//!
//! 封装 wfe-postgres 的 PostgresPersistenceProvider 适配我们的数据库连接池。
//! wfe-postgres 使用 wfc schema 管理其工作流表，
//! 我们通过 from_pool() 复用现有的 PgPool。

use wfe_postgres::PostgresPersistenceProvider;

use crate::database;

/// 创建 PostgreSQL 持久化提供者实例
///
/// 复用应用现有的数据库连接池（database::get_write_pool()）。
/// 使用 wfe-postgres 的内置 wfc schema 存储工作流实例和执行指针。
pub fn create_persistence_provider() -> PostgresPersistenceProvider {
    let pool = database::get_write_pool().expect("无法获取数据库写连接池");
    PostgresPersistenceProvider::from_pool(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试 create_persistence_provider 的函数签名（实际需要数据库连接）
    /// 真正的集成测试在 tests/integration_test.rs 中
    #[test]
    fn test_create_persistence_provider_type() {
        // 验证函数签名正确
        let _fn_ptr = create_persistence_provider;
        assert!(true);
    }
}
