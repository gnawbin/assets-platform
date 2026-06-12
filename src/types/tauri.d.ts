/**
 * Tauri 环境类型声明
 *
 * 为 window.__TAURI__ 提供类型定义，
 * 用于在运行时检测是否运行在 Tauri 环境中。
 */

interface Window {
    __TAURI__?: Record<string, unknown>;
}
