//! 信任传播。
//!
//! 带衰减的迭代传播：`t' = (1 - α) · seed + α · 邻居加权均值`，收敛阈值与迭代
//! 上限由 `TrustConfig` 控制；遍历顺序固定，保证结果确定。
//!
//! TODO(Phase 3): 落地 `TrustConfig` / `TrustReport` 与传播实现，见设计文档 §6。
