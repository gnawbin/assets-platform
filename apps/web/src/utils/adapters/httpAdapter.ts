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
 * 映射表覆盖后端 lib.rs invoke_handler 中注册的全部 Tauri 命令：
 * - 后端 HTTP API（crates/assets-api）已实现的路由 → 写入 COMMAND_ROUTE_MAP
 * - 后端尚未提供 HTTP 端点（LLM 厂商/知识资产 OKF/对话/编号规则/RAG 等）
 *   → 登记在 TAURI_ONLY_COMMANDS，HTTP 模式下会抛出明确错误
 *
 * 路径占位符 {xxx} 与前端 service 层实际传入的参数字段名保持一致
 * （如 {id} / {roleId} / {skillId} / {userId} / {uploadId}）。
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
 * 路径中的 {xxx} 占位符会在运行时被参数中同名 key 的值替换
 * （例如 {id} 取 args.id、{roleId} 取 args.roleId），
 * 占位符 key 与前端 service 层实际传参保持一致。
 *
 * 后续可迁移到数据库 sys_menu 表中动态加载。
 */
interface RouteMapping {
    method: string;
    path: string;
}

export const COMMAND_ROUTE_MAP: Record<string, RouteMapping> = {
    // ======================== 认证（公开路由） ========================
    login: { method: 'POST', path: '/api/auth/login' },
    register: { method: 'POST', path: '/api/auth/register' },

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
    get_current_user: { method: 'GET', path: '/api/users/me' },
    get_users: { method: 'GET', path: '/api/users' },
    insert_user: { method: 'POST', path: '/api/users' },
    update_user: { method: 'PUT', path: '/api/users/{id}' },
    delete_user: { method: 'DELETE', path: '/api/users/{id}' },
    reset_password: { method: 'POST', path: '/api/users/{id}/reset-password' },
    get_user_role_ids: { method: 'GET', path: '/api/users/{id}/roles' },
    assign_user_roles: { method: 'POST', path: '/api/users/{id}/roles' },

    // ======================== 角色权限 ========================
    get_roles: { method: 'GET', path: '/api/roles' },
    insert_role: { method: 'POST', path: '/api/roles' },
    delete_role: { method: 'DELETE', path: '/api/roles/{roleId}' },
    get_role_menu_ids: { method: 'GET', path: '/api/roles/{roleId}/menus' },
    assign_role_menus: { method: 'POST', path: '/api/roles/{roleId}/menus' },
    get_all_menus_tree: { method: 'GET', path: '/api/menus/tree' },
    get_user_menus: { method: 'GET', path: '/api/menus/user' },

    // ======================== 租户 ========================
    get_tenants: { method: 'GET', path: '/api/tenants' },
    insert_tenant: { method: 'POST', path: '/api/tenants' },
    update_tenant: { method: 'PUT', path: '/api/tenants/{id}' },
    delete_tenant: { method: 'DELETE', path: '/api/tenants/{id}' },
    switch_tenant: { method: 'POST', path: '/api/tenants/switch' },
    assign_user_tenants: { method: 'POST', path: '/api/tenants/assign' },
    get_user_tenants: { method: 'GET', path: '/api/users/{userId}/tenants' },

    // ======================== 注册申请 ========================
    get_registrations: { method: 'GET', path: '/api/auth/registrations' },
    approve_registration: { method: 'POST', path: '/api/auth/registrations/{id}/approve' },
    reject_registration: { method: 'POST', path: '/api/auth/registrations/{id}/reject' },

    // ======================== 知识库 - 知识树节点 ========================
    get_knowledge_tree: { method: 'GET', path: '/api/knowledge/tree' },
    insert_knowledge_node: { method: 'POST', path: '/api/knowledge/node' },
    update_knowledge_node: { method: 'PUT', path: '/api/knowledge/node/{id}' },
    delete_knowledge_node: { method: 'DELETE', path: '/api/knowledge/node/{id}' },
    move_knowledge_node: { method: 'PUT', path: '/api/knowledge/node/{id}/move' },

    // ======================== 知识库 - 知识条目 ========================
    get_knowledge_list: { method: 'GET', path: '/api/knowledge/list' },
    get_knowledge_by_id: { method: 'GET', path: '/api/knowledge/{id}' },
    insert_knowledge: { method: 'POST', path: '/api/knowledge' },
    update_knowledge: { method: 'PUT', path: '/api/knowledge/{id}' },
    delete_knowledge: { method: 'DELETE', path: '/api/knowledge/{id}' },

    // ======================== 流程管理 - 领用 ========================
    get_receives: { method: 'GET', path: '/api/process/receive' },
    insert_receive: { method: 'POST', path: '/api/process/receive' },
    update_receive: { method: 'PUT', path: '/api/process/receive/{id}' },
    delete_receive: { method: 'DELETE', path: '/api/process/receive/{id}' },

    // ======================== 流程管理 - 归还 ========================
    get_returns: { method: 'GET', path: '/api/process/return' },
    insert_return: { method: 'POST', path: '/api/process/return' },
    update_return: { method: 'PUT', path: '/api/process/return/{id}' },
    delete_return: { method: 'DELETE', path: '/api/process/return/{id}' },

    // ======================== 流程管理 - 调拨 ========================
    get_transfers: { method: 'GET', path: '/api/process/transfer' },
    insert_transfer: { method: 'POST', path: '/api/process/transfer' },
    update_transfer: { method: 'PUT', path: '/api/process/transfer/{id}' },
    delete_transfer: { method: 'DELETE', path: '/api/process/transfer/{id}' },

    // ======================== 流程管理 - 维修 ========================
    get_repairs: { method: 'GET', path: '/api/process/repair' },
    insert_repair: { method: 'POST', path: '/api/process/repair' },
    update_repair: { method: 'PUT', path: '/api/process/repair/{id}' },
    delete_repair: { method: 'DELETE', path: '/api/process/repair/{id}' },

    // ======================== 流程管理 - 报废 ========================
    get_scraps: { method: 'GET', path: '/api/process/scrap' },
    insert_scrap: { method: 'POST', path: '/api/process/scrap' },
    update_scrap: { method: 'PUT', path: '/api/process/scrap/{id}' },
    delete_scrap: { method: 'DELETE', path: '/api/process/scrap/{id}' },

    // ======================== 流程管理 - 采购 ========================
    get_purchases: { method: 'GET', path: '/api/process/purchase' },
    insert_purchase: { method: 'POST', path: '/api/process/purchase' },
    update_purchase: { method: 'PUT', path: '/api/process/purchase/{id}' },
    delete_purchase: { method: 'DELETE', path: '/api/process/purchase/{id}' },

    // ======================== Zen Engine - Skill 管理 ========================
    list_skills: { method: 'GET', path: '/api/skills' },
    get_skill: { method: 'GET', path: '/api/skills/{skillId}' },
    execute_skill: { method: 'POST', path: '/api/skills/execute' },
    register_custom_skill: { method: 'POST', path: '/api/skills/register' },
    unregister_skill: { method: 'DELETE', path: '/api/skills/{skillId}' },
    get_skill_count: { method: 'GET', path: '/api/skills/count' },

    // ======================== 大文件上传（两步提交） ========================
    // 注：上传流程由 uploadService 的 UploadClient 直接调用，不经过 api 层，
    // 以下映射仅作为路由契约登记。
    upload_init: { method: 'POST', path: '/api/upload/init' },
    upload_start: { method: 'POST', path: '/api/upload/{uploadId}/start' },
    upload_commit: { method: 'POST', path: '/api/upload/{uploadId}/commit' },
    upload_report_chunk: { method: 'POST', path: '/api/upload/{uploadId}/chunk' },
    upload_get_progress: { method: 'GET', path: '/api/upload/{uploadId}/progress' },
    upload_complete: { method: 'POST', path: '/api/upload/{uploadId}/complete' },
    upload_abort: { method: 'DELETE', path: '/api/upload/{uploadId}' },
};

// ======================== 仅 Tauri 模式支持的命令 ========================

/**
 * 后端 HTTP API（crates/assets-api）尚未提供 HTTP 端点的命令。
 *
 * 这些命令仅能通过 Tauri invoke() 调用。若在 HTTP 适配器下被调用，
 * httpCall() 会抛出明确错误，避免使用不存在的虚拟路由导致 404 混淆。
 *
 * 后续为以下模块补充 HTTP 路由后，应同步从本集合移除并加入 COMMAND_ROUTE_MAP：
 * - llm_provider：LLM 厂商 / 模型 / 用户偏好
 * - knowledge_asset：OKF 知识资产（/api/knowledge/asset/... 路由尚未实现）
 * - conversation：智能问答会话（当前仅有 /api/chat/stream SSE 流式接口）
 * - numbering：单据编号规则
 * - rag：RAG 检索
 * - upload_get_version_history：上传版本历史
 */
export const TAURI_ONLY_COMMANDS: ReadonlySet<string> = new Set<string>([
    // ======================== LLM 厂商/模型 ========================
    'get_llm_providers',
    'get_llm_provider',
    'create_llm_provider',
    'update_llm_provider',
    'delete_llm_provider',
    'get_llm_models',
    'create_llm_model',
    'update_llm_model',
    'delete_llm_model',
    'fetch_llm_models',
    'get_user_llm_setting',
    'save_user_llm_setting',

    // ======================== 知识资产（OKF 新表） ========================
    'get_knowledge_asset_by_tree_node',
    'get_knowledge_asset',
    'list_knowledge_assets',
    'create_knowledge_asset',
    'update_knowledge_asset',
    'delete_knowledge_asset',
    'attach_file_to_knowledge',

    // ======================== 智能问答对话 ========================
    'create_conversation',
    'send_message',
    'get_conversations',
    'get_conversation_messages',
    'update_conversation_title',
    'delete_conversation',

    // ======================== 编号规则 ========================
    'get_numbering_rules',
    'get_numbering_rule',
    'save_numbering_rule',
    'reset_numbering_sequence',

    // ======================== RAG ========================
    'chunk_and_vectorize',
    'test_rag_retrieval',

    // ======================== 上传版本历史 ========================
    'upload_get_version_history',
]);

// ======================== 工具函数 ========================

/**
 * 根据命令名查找 HTTP 路由映射
 */
function findRoute(command: string): RouteMapping | undefined {
    return COMMAND_ROUTE_MAP[command];
}

/**
 * 从参数中提取路径参数（如 id / roleId / skillId / userId / uploadId），
 * 剩余的作为请求体或查询参数
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
        const isTauriOnly = TAURI_ONLY_COMMANDS.has(command);
        const errMsg = isTauriOnly
            ? `命令 "${command}" 暂无 HTTP 映射（仅支持 Tauri 模式）。`
            + `当前使用 HTTP 适配器，请通过 setAdapter('tauri') 切换到 Tauri 模式，`
            + `或等待后端为该命令补充 HTTP 端点后在 httpAdapter.ts 中登记路由。`
            : `未找到命令 "${command}" 的 HTTP 路由映射`;
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
