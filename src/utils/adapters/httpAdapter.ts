/**
 * HTTP 适配器
 *
 * 通过 fetch() 调用 HTTP API 服务。
 * 未来前后端分离时使用此适配器替代 Tauri invoke。
 *
 * @remarks
 * 当前为骨架实现，待后端 HTTP API 服务就绪后启用。
 * 切换方式：设置环境变量 NEXT_PUBLIC_API_ADAPTER=http
 */

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

/** API 基础 URL，从环境变量读取 */
const BASE_URL = process.env.NEXT_PUBLIC_API_BASE_URL || 'http://localhost:3001/api';

// ======================== 核心调用函数 ========================

async function httpCall<T>(
    method: string,
    path: string,
    data?: Record<string, unknown>,
    config?: ApiConfig,
): Promise<T> {
    const mergedConfig = { ...DEFAULT_CONFIG, ...config };
    const { showError, retryCount, retryDelay, errorMessage } = mergedConfig;

    const url = `${BASE_URL}${path.startsWith('/') ? path : `/${path}`}`;

    for (let attempt = 0; attempt <= retryCount; attempt++) {
        try {
            const fetchOptions: RequestInit = {
                method,
                headers: {
                    'Content-Type': 'application/json',
                },
            };

            // 添加请求体（GET/DELETE 不发送 body）
            if (data && method !== 'GET' && method !== 'DELETE') {
                fetchOptions.body = JSON.stringify(data);
            }

            // 添加查询参数（GET/DELETE）
            let finalUrl = url;
            if (data && (method === 'GET' || method === 'DELETE')) {
                const params = new URLSearchParams();
                for (const [key, value] of Object.entries(data)) {
                    if (value !== undefined && value !== null) {
                        params.append(key, String(value));
                    }
                }
                const queryString = params.toString();
                if (queryString) {
                    finalUrl = `${url}?${queryString}`;
                }
            }

            // 添加认证 token
            const token = getAuthToken();
            if (token) {
                fetchOptions.headers = {
                    ...fetchOptions.headers,
                    Authorization: `Bearer ${token}`,
                };
            }

            logger.debug(`[HTTP] ${method} ${path}`, {
                data,
                attempt: attempt + 1,
                maxRetries: retryCount + 1,
            });

            const response = await fetch(finalUrl, fetchOptions);

            if (!response.ok) {
                const errorBody = await response.text().catch(() => '');
                throw new Error(
                    `HTTP ${response.status}: ${response.statusText}${errorBody ? ` - ${errorBody}` : ''}`,
                );
            }

            const result = await response.json();

            logger.debug(`[HTTP] 成功: ${method} ${path}`, { result });
            return result as T;
        } catch (error) {
            const errorStr =
                typeof error === 'string'
                    ? error
                    : error instanceof Error
                        ? error.message
                        : String(error);

            logger.error(
                `[HTTP] 失败: ${method} ${path}`,
                error instanceof Error ? error : new Error(errorStr),
                {
                    data,
                    attempt: attempt + 1,
                },
            );

            // 如果还有重试次数，等待后继续
            if (attempt < retryCount) {
                logger.warn(`[HTTP] 重试: ${method} ${path}`, {
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

    throw new Error(`HTTP 调用失败: ${method} ${path}`);
}

/**
 * 从 localStorage 获取认证 token
 */
function getAuthToken(): string | null {
    try {
        if (typeof window !== 'undefined') {
            const stored = localStorage.getItem('auth_token');
            return stored || null;
        }
    } catch {
        // 静默失败
    }
    return null;
}

// ======================== 导出适配器 ========================

export const httpAdapter: IApiAdapter = {
    get: <T>(
        path: string,
        params?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => httpCall<T>('GET', path, params, config),

    post: <T>(
        path: string,
        data?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => httpCall<T>('POST', path, data, config),

    put: <T>(
        path: string,
        data?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => httpCall<T>('PUT', path, data, config),

    delete: <T>(
        path: string,
        params?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => httpCall<T>('DELETE', path, params, config),
};
