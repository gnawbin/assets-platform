/**
 * 统一 API 调用层
 *
 * 封装底层 API 调用，提供统一的 get/post/put/delete 接口。
 * 底层只有一条通道：**HTTP fetch**（`utils/adapters/httpAdapter.ts`）。
 *
 * @remarks
 * 前端已完成「Tauri IPC → HTTP」切换：桌面版与 Web 版共用同一套请求代码，
 * 服务端地址由 `utils/apiBase.ts` 解析。命令名 → 路由的映射见
 * `httpAdapter.ts` 的 `COMMAND_ROUTE_MAP`。
 *
 * 使用方式：
 * ```ts
 * import { api } from '@/utils/api';
 *
 * // 基础调用
 * const data = await api.get<Category[]>('get_categories');
 *
 * // 带参数
 * const result = await api.post('insert_category', { category: newCategory });
 *
 * // 自定义配置
 * const data = await api.get('get_categories', {}, { retryCount: 2, showError: false });
 * ```
 */

import { httpAdapter } from './adapters/httpAdapter';
import type { ApiConfig } from './adapters/types';

// ======================== 导出 API ========================

/**
 * 统一 API 对象
 *
 * 提供 get/post/put/delete 语义化方法，全部经 HTTP 适配器发出。
 */
export const api = {
    /**
     * GET 请求（查询类操作）
     */
    get: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => httpAdapter.get<T>(command, args, config),

    /**
     * POST 请求（新增类操作）
     */
    post: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => httpAdapter.post<T>(command, args, config),

    /**
     * PUT 请求（更新类操作）
     */
    put: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => httpAdapter.put<T>(command, args, config),

    /**
     * DELETE 请求（删除类操作）
     */
    delete: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => httpAdapter.delete<T>(command, args, config),
};

// 重新导出类型，方便外部使用
export type { ApiConfig };


