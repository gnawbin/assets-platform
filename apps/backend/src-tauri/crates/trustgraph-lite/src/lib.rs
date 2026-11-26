//! trustgraph-lite：轻量级信任 / 知识图谱内核。
//!
//! 本 crate 只负责**图结构与算法**，不触碰任何 IO：
//!
//! - [`model`]：节点、边、三元组与关系类型；
//! - [`graph`]：邻接表存储与节点的增删查；
//! - [`traverse`]：k-hop、最短路径、诱导子图；
//! - [`trust`]：带衰减的迭代信任传播；
//! - [`snapshot`]：与前端对齐的 `{nodes, edges}` 快照；
//! - [`error`]：错误类型与 `GraphResult`。
//!
//! 设计文档：`docs/知识库模块/trustgraph-lite轻量级信任图谱内核设计方案.md`。
//!
//! # 依赖方向
//!
//! `service` / `api` → `trustgraph-lite`。本 crate 不得反向依赖 `database` /
//! `service` / `api` / `vector`，也不引入 `sqlx`、`reqwest` 等驱动。
//!
//! ```bash
//! cargo test -p trustgraph-lite
//! ```
//!
//! # 当前状态
//!
//! Phase 1 骨架：模块边界已确定，具体实现见 Phase 2 / Phase 3。

pub mod error;
pub mod graph;
pub mod model;
pub mod snapshot;
pub mod traverse;
pub mod trust;
