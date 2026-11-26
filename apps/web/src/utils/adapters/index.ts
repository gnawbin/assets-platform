/**
 * API 适配器统一导出（**HTTP 单通道**）
 *
 * 前端已从「Tauri IPC + HTTP 双通道」切换为纯 HTTP 单通道：
 * 桌面版与 Web 版的业务请求全部走 `fetch`（`httpAdapter`），
 * 不再存在 `invoke()` 调用点，也不再支持运行时切换适配器。
 *
 * 保留 `getAdapterType()` 是为了兼容既有页面的能力判断
 * （如 API Key / OAuth 客户端管理页只在 HTTP 通道可用），恒返回 `'http'`。
 */

export type { IApiAdapter, ApiConfig } from './types';
export {
    httpAdapter,
    extractServerMessage,
    refreshAccessToken,
    clearAuthSession,
    revokeRefreshTokenOnServer,
    COMMAND_ROUTE_MAP,
    UNEXPOSED_BACKEND_COMMANDS,
} from './httpAdapter';

/** 适配器类型（历史类型别名，只剩一种取值） */
export type AdapterType = 'http';

/** 兼容旧调用点：恒返回 `'http'`（前端已无 Tauri 通道） */
export function getAdapterType(): AdapterType {
    return 'http';
}

/** 兼容旧调用点：返回唯一的 HTTP 适配器 */
export { httpAdapter as getAdapter } from './httpAdapter';


