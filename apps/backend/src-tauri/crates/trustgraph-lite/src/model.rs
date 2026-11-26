//! 图数据模型。
//!
//! `NodeId` 沿用 Snowflake `i64` 主键，序列化为字符串以规避前端 JS 精度丢失；
//! `EdgeId` 是内核本地自增 id，业务唯一键始终是 `(from, relation, to)`。
//!
//! TODO(Phase 2): 落地 `Node` / `NodeKind` / `Edge` / `EdgeId` / `Triple` /
//! `RelationKind`，见设计文档 §5。
