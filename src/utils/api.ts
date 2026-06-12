/**
 * 统一 API 调用层
 *
 * 封装底层 API 调用，提供统一的 get/post/put/delete 接口。
 * 底层通过适配器模式支持多种调用方式：
 * - Tauri invoke（桌面版）
 * - HTTP fetch（未来 Web 版）
 *
 * 适配器选择方式（通过环境变量）：
 * - NEXT_PUBLIC_API_ADAPTER=tauri（默认，桌面版）
 * - NEXT_PUBLIC_API_ADAPTER=http（未来 Web 版）
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

import { getAdapter } from './adapters';
import type { ApiConfig } from './adapters/types';

// ======================== 获取适配器 ========================

const adapter = getAdapter();

// ======================== 导出 API ========================

/**
 * 统一 API 对象
 *
 * 提供 get/post/put/delete 语义化方法，
 * 底层根据环境变量自动选择 Tauri invoke 或 HTTP fetch。
 */
export const api = {
    /**
     * GET 请求（查询类操作）
     */
    get: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => adapter.get<T>(command, args, config),

    /**
     * POST 请求（新增类操作）
     */
    post: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => adapter.post<T>(command, args, config),

    /**
     * PUT 请求（更新类操作）
     */
    put: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => adapter.put<T>(command, args, config),

    /**
     * DELETE 请求（删除类操作）
     */
    delete: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => adapter.delete<T>(command, args, config),
};

// 重新导出类型，方便外部使用
export type { ApiConfig };
