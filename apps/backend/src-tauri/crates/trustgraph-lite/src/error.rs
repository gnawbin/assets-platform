//! 内核错误类型。
//!
//! 只覆盖图结构自身的非法操作（不存在、重复、超限、非法配置）；数据库与网络
//! 错误由调用方（`database` / `service`）负责转换，本 crate 不提供任何
//! `From<sqlx::Error>` 之类的桥接。
//!
//! TODO(Phase 2): 落地 `GraphError` / `GraphResult`，见设计文档 §8。
