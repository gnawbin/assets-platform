//! 快照导出。
//!
//! 输出与前端对齐的 `{nodes, edges}`：节点 id 序列化为字符串，可选携带信任分，
//! 供 `knowledge_service::get_knowledge_graph` 直接透传。
//!
//! TODO(Phase 3): 落地 `GraphNode` / `GraphEdge` / `GraphSnapshot`，见设计文档 §7.1。
