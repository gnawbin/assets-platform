/**
 * 统一 API 调用层
 *
 * 封装底层 API 调用，提供统一的 get/post/put/delete 接口。
 * 底层通过适配器模式支持多种调用方式：
 * - Tauri invoke（桌面版）
 * - HTTP fetch（Web 版）
 *
 * 适配器在运行时动态选择，支持：
 * - 构建时通过环境变量 NEXT_PUBLIC_API_ADAPTER 设置默认值
 * - 运行时通过 setAdapter() 动态切换
 * - 自动检测 Tauri 环境
 *
 * 桌面版和 Web 版可以共存，共享同一套前端代码。
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

// ======================== 导出 API ========================

/**
 * 统一 API 对象
 *
 * 提供 get/post/put/delete 语义化方法，
 * 底层在运行时动态选择 Tauri invoke 或 HTTP fetch。
 *
 * 每次调用都从管理器获取当前适配器，支持运行时切换。
 */
export const api = {
    /**
     * GET 请求（查询类操作）
     */
    get: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => getAdapter().get<T>(command, args, config),

    /**
     * POST 请求（新增类操作）
     */
    post: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => getAdapter().post<T>(command, args, config),

    /**
     * PUT 请求（更新类操作）
     */
    put: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => getAdapter().put<T>(command, args, config),

    /**
     * DELETE 请求（删除类操作）
     */
    delete: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => getAdapter().delete<T>(command, args, config),
};

// 重新导出类型，方便外部使用
export type { ApiConfig };


