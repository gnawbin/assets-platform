//! Tauri Command 模块
//!
//! 按业务领域拆分 command 函数，每个模块对应一个业务领域。
//! 所有 command 函数仅做参数解析和委托，业务逻辑在 service 层实现。

pub mod asset_commands;
pub mod category_commands;
pub mod department_commands;
pub mod knowledge_asset_commands;
pub mod knowledge_commands;
pub mod process_commands;
pub mod register_commands;
pub mod role_commands;
pub mod skill_commands;
pub mod tenant_commands;
pub mod user_commands;
