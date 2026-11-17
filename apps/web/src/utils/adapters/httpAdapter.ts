/**
 * HTTP 适配器（**前端唯一适配器**）
 *
 * 通过 fetch() 调用 HTTP API 服务。
 * 维护命令名到 HTTP 路径的映射表，使 service 层无需改动即可切换通道。
 *
 * @remarks
 * 前端已从「Tauri IPC + HTTP 双通道」切换为**纯 HTTP 单通道**：
 * 桌面版与 Web 版共用本适配器，不再有 `invoke()` 调用点。
 * 基址由 `utils/apiBase.ts` 统一解析（禁止在此硬编码 `localhost:3001`）。
 *
 * 映射表覆盖后端 lib.rs invoke_handler 中注册的全部 Tauri 命令（154 条），
 * 每条命令都必须有对应的 HTTP 路由；`tests/utils/adapters/httpAdapter.test.ts`
 * 会在缺项时失败。新增后端命令时请同步补路由 + 本表 + `authz::ROUTE_PERMS`。
 *
 * 路径占位符 {xxx} 与前端 service 层实际传入的参数字段名保持一致
 * （如 {id} / {roleId} / {skillId} / {userId} / {uploadId}）。
 *
 * 映射表后续可迁移到数据库 sys_menu 表中动态加载。
 */

import { notifyError } from '../notify';
import { logger } from '../logger';
import { clearProbeCache, getApiOrigin } from '../apiBase';
import type { IApiAdapter, ApiConfig } from './types';

// ======================== 默认配置 ========================

const DEFAULT_CONFIG: Required<ApiConfig> = {
    showError: true,
    retryCount: 0,
    retryDelay: 1000,
    errorMessage: '操作失败，请稍后重试',
};

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
    // 刷新 / 登出：访问令牌可能已过期，属于公开路由（靠 refresh token 自身校验）
    refresh_token: { method: 'POST', path: '/api/auth/refresh' },
    logout: { method: 'POST', path: '/api/auth/logout' },

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

    // ======================== 运维采集（Docker / Swarm / K8s，只读） ========================
    get_ops_providers: { method: 'GET', path: '/api/ops/providers' },
    get_ops_connections: { method: 'GET', path: '/api/ops/connections' },
    get_ops_connection: { method: 'GET', path: '/api/ops/connections/{id}' },
    create_ops_connection: { method: 'POST', path: '/api/ops/connections' },
    update_ops_connection: { method: 'PUT', path: '/api/ops/connections/{id}' },
    delete_ops_connection: { method: 'DELETE', path: '/api/ops/connections/{id}' },
    test_ops_connection_draft: { method: 'POST', path: '/api/ops/connections/test' },
    test_ops_connection: { method: 'POST', path: '/api/ops/connections/{id}/test' },
    collect_ops_resources: { method: 'POST', path: '/api/ops/connections/{id}/collect' },
    get_ops_links: { method: 'GET', path: '/api/ops/links' },
    bind_ops_link: { method: 'POST', path: '/api/ops/links/{linkId}/bind' },

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

    // ======================== API Key 管理（仅 HTTP 通道） ========================
    get_api_keys: { method: 'GET', path: '/api/api-keys' },
    create_api_key: { method: 'POST', path: '/api/api-keys' },
    rotate_api_key: { method: 'POST', path: '/api/api-keys/{id}/rotate' },
    revoke_api_key: { method: 'DELETE', path: '/api/api-keys/{id}' },

    // ======================== OAuth 客户端管理（仅 HTTP 通道，Phase 2） ========================
    get_oauth_clients: { method: 'GET', path: '/api/oauth-clients' },
    create_oauth_client: { method: 'POST', path: '/api/oauth-clients' },
    update_oauth_client: { method: 'PUT', path: '/api/oauth-clients/{id}' },
    delete_oauth_client: { method: 'DELETE', path: '/api/oauth-clients/{id}' },

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

    // ======================== 知识库 - OKF 知识资产（新表 knowledge_asset） ========================
    // 路径占位符沿用命令参数名（{treeNodeId} / {id} / {assetId}），
    // 其余参数按命令参数形状原样作为 body / query 发送
    list_knowledge_assets: { method: 'GET', path: '/api/knowledge/asset' },
    get_knowledge_asset_by_tree_node: { method: 'GET', path: '/api/knowledge/asset/by-tree-node/{treeNodeId}' },
    get_knowledge_asset: { method: 'GET', path: '/api/knowledge/asset/{id}' },
    create_knowledge_asset: { method: 'POST', path: '/api/knowledge/asset' },
    update_knowledge_asset: { method: 'PUT', path: '/api/knowledge/asset/{id}' },
    delete_knowledge_asset: { method: 'DELETE', path: '/api/knowledge/asset/{id}' },
    // assetId 走路径，文件信息留在 body
    attach_file_to_knowledge: { method: 'POST', path: '/api/knowledge/asset/{assetId}/attach' },

    // ======================== LLM 厂商 / 模型 / 用户偏好 ========================
    // 厂商响应里的 api_key 一律是脱敏占位符 ******；更新时回传该值表示「不修改」
    get_llm_providers: { method: 'GET', path: '/api/llm/providers' },
    get_llm_provider: { method: 'GET', path: '/api/llm/providers/{id}' },
    create_llm_provider: { method: 'POST', path: '/api/llm/providers' },
    update_llm_provider: { method: 'PUT', path: '/api/llm/providers/{id}' },
    delete_llm_provider: { method: 'DELETE', path: '/api/llm/providers/{id}' },
    // providerId 走路径（占位符与命令参数同名）
    fetch_llm_models: { method: 'POST', path: '/api/llm/providers/{providerId}/fetch-models' },
    get_llm_models: { method: 'GET', path: '/api/llm/models' },
    create_llm_model: { method: 'POST', path: '/api/llm/models' },
    update_llm_model: { method: 'PUT', path: '/api/llm/models/{id}' },
    delete_llm_model: { method: 'DELETE', path: '/api/llm/models/{id}' },
    // 用户偏好：userId 仅作兼容（服务端以令牌为准），GET 走 query、POST 走 body
    get_user_llm_setting: { method: 'GET', path: '/api/llm/user-setting' },
    save_user_llm_setting: { method: 'POST', path: '/api/llm/user-setting' },

    // ======================== 单据编号规则 ========================
    get_numbering_rules: { method: 'GET', path: '/api/numbering/rules' },
    get_numbering_rule: { method: 'GET', path: '/api/numbering/rules/{bizType}' },
    // body 形状与命令参数一致：{ id?, input }
    save_numbering_rule: { method: 'POST', path: '/api/numbering/rules' },
    // bizType 走路径，resetKey 留在 body
    reset_numbering_sequence: { method: 'POST', path: '/api/numbering/rules/{bizType}/reset' },

    // ======================== 智能问答 - 会话管理 ========================
    // 归属校验在服务端完成（以令牌 sub 为准），body/query 里的 userId 仅作兼容告警
    get_conversations: { method: 'GET', path: '/api/conversations' },
    create_conversation: { method: 'POST', path: '/api/conversations' },
    // convId 走路径，page/pageSize 作为 query string
    get_conversation_messages: { method: 'GET', path: '/api/conversations/{convId}/messages' },
    send_message: { method: 'POST', path: '/api/conversations/{convId}/messages' },
    update_conversation_title: { method: 'PUT', path: '/api/conversations/{convId}/title' },
    delete_conversation: { method: 'DELETE', path: '/api/conversations/{convId}' },

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
    // 版本历史：fileGroupId 走路径
    upload_get_version_history: { method: 'GET', path: '/api/upload/versions/{fileGroupId}' },
};

// ======================== 未暴露到 HTTP 的后端命令 ========================

/**
 * 后端仍在 `invoke_handler` 注册、但**没有 HTTP 路由**的命令。
 *
 * 前端已切换为纯 HTTP 单通道，这些命令在桌面版/Web 版都**无法调用**；
 * 之所以保留在此，是为了让映射表完整性测试能给出精确的「缺项」信号，
 * 而不是把缺项混进真正的漏配里。
 *
 * 当前仅剩 RAG 两条：前端无任何调用点（知识库入库走 `uploadService` +
 * 后端向量化任务），后端也尚未提供 `crates/api` 路由。
 * 处理方式二选一：补 HTTP 路由（再登记进 `COMMAND_ROUTE_MAP`），
 * 或删除这两个 Tauri 命令与本集合条目。
 */
export const UNEXPOSED_BACKEND_COMMANDS: ReadonlySet<string> = new Set<string>([
    'chunk_and_vectorize',
    'test_rag_retrieval',
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
 *
 * @returns 完整 URL；服务端地址未配置时返回 `null`（由调用方给出可操作错误）
 *
 * @remarks
 * 这里拼接的是 `getApiOrigin()`（**不含** `/api`），
 * 而映射表里的 path 自带 `/api/...`，因此不会出现 `/api/api/...`。
 */
function buildUrl(path: string, pathArgs: Record<string, string>): string | null {
    const origin = getApiOrigin();
    if (!origin) {
        return null;
    }

    let resolvedPath = path;
    for (const [key, value] of Object.entries(pathArgs)) {
        resolvedPath = resolvedPath.replace(`{${key}}`, value);
    }
    return `${origin}${resolvedPath}`;
}

/** 截断长文本（保留可读前缀，避免通知里塞整页 HTML） */
function truncate(text: string, max: number): string {
    return text.length <= max ? text : `${text.slice(0, max)}…`;
}

/**
 * 从后端错误响应体中提取可读消息
 *
 * 后端 `ApiError` 统一返回 `{ code, message }`（鉴权失败可能附带 `scope`），
 * OAuth 端点返回 `{ error, error_description }`。把业务消息直接抛给调用方，
 * 页面才能显示「权限不足：缺少 xxx」而不是裸的 `HTTP 403 Forbidden`。
 *
 * 非 JSON（如 nginx 的 502 HTML 页）返回 `null`，由调用方回落到状态行。
 */
export function extractServerMessage(rawBody: string): string | null {
    const body = (rawBody || '').trim();
    if (!body) {
        return null;
    }

    try {
        const parsed: unknown = JSON.parse(body);
        if (parsed && typeof parsed === 'object') {
            const record = parsed as Record<string, unknown>;
            for (const key of ['message', 'error_description', 'error', 'msg', 'detail']) {
                const value = record[key];
                if (typeof value === 'string' && value.trim()) {
                    return value.trim();
                }
            }
        }
    } catch {
        // 非 JSON：按纯文本处理（限长且排除 HTML）
        if (body.length <= 300 && !body.startsWith('<')) {
            return body;
        }
    }

    return null;
}

/**
 * 构造带业务消息的错误对象
 *
 * 除 `message` 外，403 响应通常还带 `scope`（缺少的权限点），
 * 挂到 `error.scope` 上，方便页面提示「缺少权限点：asset:category:create」。
 */
export interface ApiErrorWithScope extends Error {
    /** 缺少的权限点 / 授权范围（后端 `ApiError.scope`） */
    scope?: string;
}

/** 由错误响应体构造错误对象（非 JSON 时只带 message） */
function buildServerError(message: string, rawBody: string): ApiErrorWithScope {
    const error: ApiErrorWithScope = new Error(message);
    try {
        const parsed: unknown = JSON.parse(rawBody);
        if (parsed && typeof parsed === 'object') {
            const scope = (parsed as Record<string, unknown>).scope;
            if (typeof scope === 'string' && scope.trim()) {
                error.scope = scope.trim();
            }
        }
    } catch {
        // 非 JSON：没有附加字段
    }
    return error;
}

/**
 * 传输层失败（请求没拿到任何 HTTP 响应）的错误对象
 *
 * 与后端业务错误（[`ApiErrorWithScope`]，一定带 HTTP 状态码）区分开：
 * 页面据此提示「后端没启动 / 地址不对」，而不是「用户名或密码错误」。
 */
export interface TransportError extends Error {
    /** 固定为 `true`，供调用方判定（无需再靠 message 文本匹配） */
    readonly transport: true;
    /** 实际请求的地址（含路径，便于用户核对端口） */
    readonly url: string;
}

/**
 * 浏览器 fetch 传输层失败的 message 特征
 *
 * 同一种失败在不同内核下文案不同：
 * - Chrome / Edge：`Failed to fetch`
 * - Safari：`Load failed`
 * - Firefox：`NetworkError when attempting to fetch resource.`
 * - Node / undici：`fetch failed`
 */
const TRANSPORT_FAILURE_PATTERN =
    /failed to fetch|load failed|networkerror|network request failed|fetch failed/i;

/**
 * 判定异常是否属于「请求没送达」（DNS 失败 / 端口未监听 / 证书无效 / CORS 被拒）
 *
 * 只认 message 特征，**不**用 `instanceof TypeError` 兜底：
 * 业务代码里的 `JSON.stringify` 循环引用等同样会抛 `TypeError`，
 * 误判成网络故障反而会掩盖真实 bug。
 */
export function isTransportFailure(error: unknown): boolean {
    if (!(error instanceof Error)) {
        return false;
    }
    if ((error as Partial<TransportError>).transport === true) {
        return true;
    }
    return TRANSPORT_FAILURE_PATTERN.test(error.message);
}

/**
 * 构造可操作的「连不上服务器」错误
 *
 * 裸的 `TypeError: Failed to fetch` 既不含地址也不给下一步动作，
 * 这里补上当前生效的源地址与两种最常见的处理方式（启动后端 / 改地址）。
 */
function buildTransportError(url: string): TransportError {
    const target = getApiOrigin() || url;
    const error = new Error(
        `无法连接服务器 ${target}：请求未送达（后端未启动、端口不通，或证书 / 跨域被拒）。`
            + '请确认后端已启动，或到「系统配置 → 服务端连接」修改地址。',
    ) as TransportError;
    return Object.assign(error, { transport: true as const, url });
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

// ======================== 令牌自动续期 ========================

/** 认证端点本身不参与 401 自动续期，避免刷新递归 */
const AUTH_ENDPOINTS: ReadonlySet<string> = new Set<string>([
    '/api/auth/login',
    '/api/auth/register',
    '/api/auth/refresh',
    '/api/auth/logout',
]);

/** 并发去重：同一时刻只发起一次刷新请求，其余请求复用它 */
let refreshInFlight: Promise<string | null> | null = null;

/** 保存新的令牌对，并通知 authStore 同步内存中的 token */
function persistRefreshedTokens(token: string, refreshToken?: string | null): void {
    try {
        localStorage.setItem('auth_token', token);
        if (refreshToken) {
            localStorage.setItem('refresh_token', refreshToken);
        }
    } catch {
        // localStorage 不可用时忽略（请求仍能继续）
    }

    if (typeof window !== 'undefined') {
        window.dispatchEvent(
            new CustomEvent('auth:tokens-refreshed', { detail: { token } }),
        );
    }
}

async function doRefreshAccessToken(): Promise<string | null> {
    let refreshToken: string | null = null;
    try {
        refreshToken =
            typeof window !== 'undefined' ? localStorage.getItem('refresh_token') : null;
    } catch {
        return null;
    }
    if (!refreshToken) {
        return null;
    }

    try {
        const refreshUrl = buildUrl('/api/auth/refresh', {});
        if (!refreshUrl) {
            logger.warn('[HTTP] 服务端地址未配置，跳过令牌续期');
            return null;
        }
        const response = await fetch(refreshUrl, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ refresh_token: refreshToken }),
        });
        if (!response.ok) {
            logger.warn(`[HTTP] 刷新令牌失败: HTTP ${response.status}`);
            return null;
        }

        const payload = await response.json();
        const data = payload && typeof payload === 'object' ? payload.data : undefined;
        if (!data?.token) {
            return null;
        }

        persistRefreshedTokens(data.token as string, (data.refresh_token as string) || null);
        logger.info('[HTTP] 访问令牌已自动续期');
        return data.token as string;
    } catch (error) {
        logger.warn('[HTTP] 刷新令牌请求异常', { error });
        return null;
    }
}

/**
 * 用 refresh token 换取新的访问令牌（并发安全）
 *
 * @returns 新的访问令牌；刷新失败 / 无 refresh token 时返回 null
 */
export async function refreshAccessToken(): Promise<string | null> {
    if (refreshInFlight) {
        return refreshInFlight;
    }

    const task = doRefreshAccessToken();
    refreshInFlight = task;
    try {
        return await task;
    } finally {
        refreshInFlight = null;
    }
}

/** 清除本地登录态并跳转登录页（刷新失败后的兜底） */
export function clearAuthSession(): void {
    try {
        localStorage.removeItem('auth_token');
        localStorage.removeItem('refresh_token');
        localStorage.removeItem('auth_user');
        localStorage.removeItem('available_tenants');
        localStorage.removeItem('selected_tenant_id');
    } catch {
        // 忽略
    }

    if (typeof window !== 'undefined' && window.location.pathname !== '/login') {
        window.location.assign('/login');
    }
}

/**
 * 撤销后端刷新令牌（主动登出；失败不阻断本地登出流程）
 */
export async function revokeRefreshTokenOnServer(refreshToken: string): Promise<void> {
    try {
        const logoutUrl = buildUrl('/api/auth/logout', {});
        if (!logoutUrl) {
            return;
        }
        await fetch(logoutUrl, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ refresh_token: refreshToken }),
            keepalive: true,
        });
    } catch (error) {
        logger.warn('[HTTP] 撤销刷新令牌失败（不影响本地登出）', { error });
    }
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
        const errMsg = `未找到命令 "${command}" 的 HTTP 路由映射`
            + `（请在 httpAdapter.ts 的 COMMAND_ROUTE_MAP 中登记）`;
        logger.error(`[HTTP] ${errMsg}`);
        if (showError) {
            notifyError('操作失败', errMsg);
        }
        throw new Error(errMsg);
    }

    const { method, path: pathTemplate } = route;

    // 提取路径参数和请求体参数
    const { pathArgs, bodyArgs } = extractParams(args, pathTemplate);

    // 构建最终 URL：服务端地址未配置时给出可操作提示，**不发请求**
    const url = buildUrl(pathTemplate, pathArgs);
    if (!url) {
        const errMsg = '未配置服务器地址：请到「系统配置 → 服务端连接」填写后端地址'
            + '（或在构建时设置 NEXT_PUBLIC_API_BASE_URL）';
        logger.error(`[HTTP] ${errMsg}`);
        if (showError) {
            notifyError('无法连接服务器', errMsg);
        }
        throw new Error(errMsg);
    }

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

            let response = await fetch(finalUrl, fetchOptions);
            let sessionCleared = false;

            // 401 → 用 refresh token 静默续期并重放一次（认证端点本身不自救，避免刷新递归）
            if (
                response.status === 401 &&
                !AUTH_ENDPOINTS.has(pathTemplate) &&
                getAuthToken()
            ) {
                const renewedToken = await refreshAccessToken();
                if (renewedToken) {
                    logger.warn(
                        `[HTTP] 访问令牌已过期，已使用刷新令牌重试: ${method} ${finalUrl}`,
                    );
                    fetchOptions.headers = {
                        ...fetchOptions.headers,
                        Authorization: `Bearer ${renewedToken}`,
                    };
                    response = await fetch(finalUrl, fetchOptions);

                    // 续期成功但仍是 401：令牌/账号确实无效（账号被禁用、令牌被撤销），
                    // 不能停在「有 token 但一直失败」的状态，必须清会话回登录页。
                    if (response.status === 401) {
                        logger.warn(
                            `[HTTP] 续期后仍返回 401，清除登录态: ${method} ${finalUrl}`,
                        );
                        clearAuthSession();
                        sessionCleared = true;
                    }
                } else {
                    logger.warn('[HTTP] 刷新令牌失败，清除登录态并跳转登录页');
                    clearAuthSession();
                    sessionCleared = true;
                }
            }

            if (!response.ok) {
                const rawBody = await response.text().catch(() => '');
                const serverMessage = extractServerMessage(rawBody);
                const statusLine = `HTTP ${response.status} ${response.statusText}`.trim();

                // 401 且已清会话：给出可操作提示，避免只显示裸状态码
                const message = serverMessage
                    || (response.status === 401 && sessionCleared
                        ? `${statusLine} - 登录状态已失效，请重新登录`
                        : `${statusLine}${rawBody ? ` - ${truncate(rawBody, 300)}` : ''}`);

                throw buildServerError(message, rawBody);
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

            // 传输层失败（后端没启动 / 端口不通 / 证书或跨域被拒）：没有 HTTP 响应，
            // 与后端返回的业务错误不是一回事，必须给出可操作提示。
            const transportFailure = isTransportFailure(error);
            if (transportFailure) {
                // 「上次探活成功」的缓存会掩盖后端已挂的事实（ServerConfigGate 直接放行），
                // 清掉它，下一次整页加载会重新探活、及时拦住用户去配置地址。
                clearProbeCache();
            }

            logger.error(
                `[HTTP] 失败: ${method} ${url}`,
                error instanceof Error ? error : new Error(errorStr),
                {
                    args,
                    attempt: attempt + 1,
                    transportFailure,
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

            // 最后一次失败：传输层失败替换成带地址与建议的错误，其余原样抛出
            if (transportFailure) {
                const failure = buildTransportError(url);
                if (showError) {
                    notifyError('无法连接服务器', failure.message);
                }
                throw failure;
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
