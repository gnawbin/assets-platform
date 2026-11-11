//! 遍历与路径查询。
//!
//! 提供邻接查询、k-hop 子图、无权最短路（BFS）与加权最短路（Dijkstra）；
//! 深度上限 `MAX_DEPTH`，避免大图子图爆炸。
//!
//! TODO(Phase 3): 落地 `Direction` / `Subgraph` 与各遍历入口，见设计文档 §7。
