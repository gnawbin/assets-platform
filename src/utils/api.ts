/**
 * 统一 API 调用层
 *
 * 封装 Tauri invoke() 调用，提供：
 * - 统一错误处理（自动通知 + 日志）
 * - 请求重试机制
 * - 请求日志记录
 * - 泛型支持
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

import { invoke } from '@tauri-apps/api/core';
import { notifyError } from './notify';
import { logger } from './logger';

// ======================== 类型定义 ========================

/** API 调用配置 */
export interface ApiConfig {
    /** 是否自动显示错误通知（默认 true） */
    showError?: boolean;
    /** 失败重试次数（默认 0） */
    retryCount?: number;
    /** 重试间隔毫秒数（默认 1000） */
    retryDelay?: number;
    /** 自定义错误消息 */
    errorMessage?: string;
}

// ======================== 默认配置 ========================

const DEFAULT_CONFIG: Required<ApiConfig> = {
    showError: true,
    retryCount: 0,
    retryDelay: 1000,
    errorMessage: '操作失败，请稍后重试',
};

// ======================== 核心调用函数 ========================

/**
 * 统一 API 调用
 *
 * @param command Tauri 命令名称
 * @param args 命令参数
 * @param config 调用配置
 * @returns 泛型 T 类型的响应数据
 */
async function apiCall<T>(
    command: string,
    args?: Record<string, unknown>,
    config?: ApiConfig,
): Promise<T> {
    const mergedConfig = { ...DEFAULT_CONFIG, ...config };
    const { showError, retryCount, retryDelay, errorMessage } = mergedConfig;

    for (let attempt = 0; attempt <= retryCount; attempt++) {
        try {
            logger.debug(`[API] 调用: ${command}`, {
                args,
                attempt: attempt + 1,
                maxRetries: retryCount + 1,
            });

            const result = await invoke<T>(command, args);

            logger.debug(`[API] 成功: ${command}`, { result });
            return result;
        } catch (error) {
            const errorStr =
                typeof error === 'string'
                    ? error
                    : error instanceof Error
                        ? error.message
                        : String(error);

            logger.error(
                `[API] 失败: ${command}`,
                error instanceof Error ? error : new Error(errorStr),
                {
                    args,
                    attempt: attempt + 1,
                },
            );

            // 如果还有重试次数，等待后继续
            if (attempt < retryCount) {
                logger.warn(`[API] 重试: ${command}`, {
                    attempt: attempt + 1,
                    nextAttempt: attempt + 2,
                    delay: retryDelay,
                });
                await new Promise((resolve) => setTimeout(resolve, retryDelay));
                continue;
            }

            // 最后一次失败，显示错误通知
            if (showError) {
                notifyError('操作失败', errorStr || errorMessage);
            }

            throw error;
        }
    }

    // TypeScript 无法识别循环中的 throw，这里做兜底
    throw new Error(`API 调用失败: ${command}`);
}

// ======================== 导出 API ========================

/**
 * 统一 API 对象
 *
 * 提供 get/post/put/delete 语义化方法，
 * 底层都调用相同的 apiCall 核心函数。
 */
export const api = {
    /**
     * GET 请求（查询类操作）
     */
    get: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => apiCall<T>(command, args, config),

    /**
     * POST 请求（新增类操作）
     */
    post: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => apiCall<T>(command, args, config),

    /**
     * PUT 请求（更新类操作）
     */
    put: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => apiCall<T>(command, args, config),

    /**
     * DELETE 请求（删除类操作）
     */
    delete: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => apiCall<T>(command, args, config),
};
