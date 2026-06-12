/**
 * HTTP 适配器
 *
 * 通过 fetch() 调用 HTTP API 服务。
 * 维护命令名到 HTTP 路径的映射表，使 service 层无需改动即可切换适配器。
 *
 * @remarks
 * 切换方式：设置环境变量 NEXT_PUBLIC_API_ADAPTER=http
 * 或运行时调用 setAdapter('http')
 *
 * 映射表后续可迁移到数据库 sys_menu 表中动态加载。
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

// ======================== 命令名 → HTTP 路径映射表 ========================

/**
 * 命令名到 HTTP 路由的映射
 *
 * key: Tauri 命令名（前端 service 层使用的名称）
 * value: { method: HTTP 方法, path: HTTP 路径模板 }
 *
 * 路径中的 {id} 占位符会在运行时被参数中的 id 值替换。
 * 后续可迁移到数据库 sys_menu 表中动态加载。
 */
interface RouteMapping {
    method: string;
    path: string;
}

const COMMAND_ROUTE_MAP: Record<string, RouteMapping> = {
    // ======================== 认证 ========================
    login: { method: 'POST', path: '/api/auth/login' },

    // ======================== 资产分类 ========================
    get_categories: { method: 'GET', path: '/api/categories' },
    get_categories_parents: { method: 'GET', path: '/api/categories/parents' },
    insert_category: { method: 'POST', path: '/api/categories' },
    update_category: { method: 'PUT', path: '/api/categories/{id}' },
    delete_category: { method: 'DELETE', path: '/api/categories/{id}' },

    // ======================== 固定资产 ========================
    get_hardware_assets: { method: 'GET', path: '/api/assets/hardware' },
    insert_hardware_asset: { method: 'POST', path: '/api/assets/hardware' },
    update_hardware_asset: { method: 'PUT', path: '/api/assets/hardware/{id}' },
    delete_hardware_asset: { method: 'DELETE', path: '/api/assets/hardware/{id}' },

    // ======================== 无形资产 ========================
    get_intangible_assets: { method: 'GET', path: '/api/assets/intangible' },
    insert_intangible_asset: { method: 'POST', path: '/api/assets/intangible' },
    update_intangible_asset: { method: 'PUT', path: '/api/assets/intangible/{id}' },
    delete_intangible_asset: { method: 'DELETE', path: '/api/assets/intangible/{id}' },

    // ======================== 部门 ========================
    get_departments: { method: 'GET', path: '/api/departments' },
    insert_department: { method: 'POST', path: '/api/departments' },
    update_department: { method: 'PUT', path: '/api/departments/{id}' },
    delete_department: { method: 'DELETE', path: '/api/departments/{id}' },

    // ======================== 用户 ========================
    get_users: { method: 'GET', path: '/api/users' },
    insert_user: { method: 'POST', path: '/api/users' },
    update_user: { method: 'PUT', path: '/api/users/{id}' },
    delete_user: { method: 'DELETE', path: '/api/users/{id}' },
    reset_password: { method: 'POST', path: '/api/users/{id}/reset-password' },

    // ======================== 角色 ========================
    get_roles: { method: 'GET', path: '/api/roles' },
    insert_role: { method: 'POST', path: '/api/roles' },
    delete_role: { method: 'DELETE', path: '/api/roles/{id}' },
    get_all_menus_tree: { method: 'GET', path: '/api/menus/tree' },
    get_role_menu_ids: { method: 'GET', path: '/api/roles/{id}/menus' },
    assign_role_menus: { method: 'POST', path: '/api/roles/{id}/menus' },
};

// ======================== 工具函数 ========================

/**
 * 根据命令名查找 HTTP 路由映射
 */
function findRoute(command: string): RouteMapping | undefined {
    return COMMAND_ROUTE_MAP[command];
}

/**
 * 替换路径中的 {id} 占位符
 * 从参数中提取 id 字段，替换路径模板中的 {id}
 */
function resolvePath(path: string, args?: Record<string, unknown>): string {
    if (!args || !path.includes('{id}')) {
        return path;
    }

    const idValue = args['id'];
    if (idValue !== undefined && idValue !== null) {
        return path.replace('{id}', String(idValue));
    }

    return path;
}

/**
 * 从参数中提取路径参数（如 id），剩余的作为请求体或查询参数
 */
function extractParams(
    args: Record<string, unknown> | undefined,
    path: string,
): {
    pathArgs: Record<string, string>;
    bodyArgs: Record<string, unknown>;
} {
    const pathArgs: Record<string, string> = {};
    const bodyArgs: Record<string, unknown> = { ...args };

    // 提取路径中的占位符对应的参数
    const placeholders = path.match(/\{(\w+)\}/g);
    if (placeholders) {
        for (const placeholder of placeholders) {
            const key = placeholder.slice(1, -1); // 去掉 { }
            if (bodyArgs[key] !== undefined) {
                pathArgs[key] = String(bodyArgs[key]);
                delete bodyArgs[key];
            }
        }
    }

    return { pathArgs, bodyArgs };
}

/**
 * 构建最终 URL，替换路径参数
 */
function buildUrl(path: string, pathArgs: Record<string, string>): string {
    let resolvedPath = path;
    for (const [key, value] of Object.entries(pathArgs)) {
        resolvedPath = resolvedPath.replace(`{${key}}`, value);
    }
    return `${BASE_URL}${resolvedPath}`;
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

// ======================== 核心调用函数 ========================

async function httpCall<T>(
    command: string,
    args?: Record<string, unknown>,
    config?: ApiConfig,
): Promise<T> {
    const mergedConfig = { ...DEFAULT_CONFIG, ...config };
    const { showError, retryCount, retryDelay, errorMessage } = mergedConfig;

    // 查找路由映射
    const route = findRoute(command);
    if (!route) {
        const errMsg = `未找到命令 "${command}" 的 HTTP 路由映射`;
        logger.error(`[HTTP] ${errMsg}`);
        if (showError) {
            notifyError('操作失败', errMsg);
        }
        throw new Error(errMsg);
    }

    const { method, path: pathTemplate } = route;

    // 提取路径参数和请求体参数
    const { pathArgs, bodyArgs } = extractParams(args, pathTemplate);

    // 构建最终 URL
    const url = buildUrl(pathTemplate, pathArgs);

    for (let attempt = 0; attempt <= retryCount; attempt++) {
        try {
            const fetchOptions: RequestInit = {
                method,
                headers: {
                    'Content-Type': 'application/json',
                },
            };

            // 添加请求体（GET/DELETE 不发送 body）
            const hasBody = method !== 'GET' && method !== 'DELETE';
            if (hasBody && Object.keys(bodyArgs).length > 0) {
                fetchOptions.body = JSON.stringify(bodyArgs);
            }

            // 添加查询参数（GET/DELETE）
            let finalUrl = url;
            if (!hasBody && Object.keys(bodyArgs).length > 0) {
                const params = new URLSearchParams();
                for (const [key, value] of Object.entries(bodyArgs)) {
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

            logger.debug(`[HTTP] ${method} ${finalUrl}`, {
                body: hasBody ? bodyArgs : undefined,
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

            logger.debug(`[HTTP] 成功: ${method} ${finalUrl}`, { result });

            // HTTP API 返回格式为 ApiResponse<T>，提取 data 字段
            // 兼容两种返回格式：{ code, message, data } 或直接数据
            if (result && typeof result === 'object' && 'data' in result) {
                return result.data as T;
            }

            return result as T;
        } catch (error) {
            const errorStr =
                typeof error === 'string'
                    ? error
                    : error instanceof Error
                        ? error.message
                        : String(error);

            logger.error(
                `[HTTP] 失败: ${method} ${url}`,
                error instanceof Error ? error : new Error(errorStr),
                {
                    args,
                    attempt: attempt + 1,
                },
            );

            // 如果还有重试次数，等待后继续
            if (attempt < retryCount) {
                logger.warn(`[HTTP] 重试: ${method} ${url}`, {
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

    throw new Error(`HTTP 调用失败: ${method} ${url}`);
}

// ======================== 导出适配器 ========================

export const httpAdapter: IApiAdapter = {
    get: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => httpCall<T>(command, args, config),

    post: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => httpCall<T>(command, args, config),

    put: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => httpCall<T>(command, args, config),

    delete: <T>(
        command: string,
        args?: Record<string, unknown>,
        config?: ApiConfig,
    ): Promise<T> => httpCall<T>(command, args, config),
};
