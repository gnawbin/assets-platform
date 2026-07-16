/**
 * Tauri 适配器
 *
 * 通过 Tauri invoke() 调用 Rust 后端命令。
 * 桌面版默认使用此适配器。
 */

import { invoke } from '@tauri-apps/api/core';
import { notifyError } from '../notify';
import { logger } from '../logger';
import type { IApiAdapter, ApiConfig } from './types';

// ======================== 默认配置 ========================

const DEFAULT_CONFIG: Required<ApiConfig> = {
    showError: true,
    retryCount: 0,
    retryDelay: 1000,
    errorMessage: '操作失败，请稍后重试',
};

// ======================== 核心调用函数 ========================

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

// ======================== 导出适配器 ========================

export const tauriAdapter: IApiAdapter = {
    get: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => apiCall<T>(command, args, config),

    post: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => apiCall<T>(command, args, config),

    put: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => apiCall<T>(command, args, config),

    delete: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => apiCall<T>(command, args, config),
};
