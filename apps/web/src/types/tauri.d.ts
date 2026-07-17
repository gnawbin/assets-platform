/**
 * Tauri 环境类型声明
 *
 * 为 Tauri v2 运行时环境检测提供类型定义。
 * Tauri v2 使用 window.__TAURI_INTERNALS__ 作为运行环境标记，
 * 同时兼容 v1 的 window.__TAURI__。
 */

interface Window {
    /** Tauri v1 全局 API 对象（v2 中需启用 app.withGlobalTauri 配置） */
    __TAURI__?: Record<string, unknown>;
    /** Tauri v2 内部运行环境标记（始终存在） */
    __TAURI_INTERNALS__?: Record<string, unknown>;
}
