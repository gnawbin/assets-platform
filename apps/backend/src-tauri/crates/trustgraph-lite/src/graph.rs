//! 邻接表存储。
//!
//! 节点表与出入边表均使用 `BTreeMap`，保证遍历顺序确定；容量上限见设计文档 §9
//! （`MAX_NODES` / `MAX_EDGES` / `MAX_FANOUT`）。
//!
//! TODO(Phase 2): 落地 `TrustGraph` 与其增删查方法，见设计文档 §7。
