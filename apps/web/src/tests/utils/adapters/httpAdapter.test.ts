/**
 * httpAdapter 映射表完整性 + URL 拼装测试
 *
 * 前端已切换为纯 HTTP 单通道（无 Tauri-only 例外），因此这里验证：
 * 1. `COMMAND_ROUTE_MAP` 覆盖了后端 `apps/backend/src-tauri/src/lib.rs`
 *    invoke_handler 中注册的全部 Tauri 命令（RAG 也在其中）；
 * 2. 映射表自身格式合法（路径以 /api/ 开头、方法合法、占位符格式正确）；
 * 3. 基址与映射路径拼接后**不出现** `/api/api/...`（历史 bug，见 D-6）；
 * 4. 后端错误消息能透出到前端错误对象（D-5）。
 *
 * 当后端新增/移除 Tauri 命令时，需同步更新本文件的 REGISTERED_COMMANDS 列表。
 */

import {
    COMMAND_ROUTE_MAP,
    UNEXPOSED_BACKEND_COMMANDS,
    extractServerMessage,
    httpAdapter,
    isTransportFailure,
} from '@/utils/adapters/httpAdapter';
import { API_PROBE_CACHE_KEY } from '@/utils/apiBase';

/**
 * 后端 lib.rs invoke_handler 注册的 Tauri 命令全集（共 123 个）
 *
 * 与 apps/backend/src-tauri/src/lib.rs 的 invoke_handler 保持一致，
 * 按模块分组，方便对照。
 */
const REGISTERED_COMMANDS: string[] = [
    // 资产（固定资产 + 无形资产）
    'get_hardware_assets',
    'insert_hardware_asset',
    'update_hardware_asset',
    'delete_hardware_asset',
    'get_intangible_assets',
    'insert_intangible_asset',
    'update_intangible_asset',
    'delete_intangible_asset',

    // 资产分类
    'get_categories',
    'get_categories_parents',
    'insert_category',
    'update_category',
    'delete_category',

    // 部门
    'get_departments',
    'insert_department',
    'update_department',
    'delete_department',

    // 租户
    'get_tenants',
    'insert_tenant',
    'update_tenant',
    'delete_tenant',
    'switch_tenant',
    'assign_user_tenants',
    'get_user_tenants',

    // 用户
    'login',
    'get_users',
    'insert_user',
    'update_user',
    'delete_user',
    'reset_password',
    'get_current_user',

    // 角色权限
    'get_roles',
    'insert_role',
    'delete_role',
    'get_user_role_ids',
    'assign_user_roles',
    'get_role_menu_ids',
    'assign_role_menus',
    'get_all_menus_tree',
    'get_user_menus',

    // 注册申请
    'register',
    'get_registrations',
    'approve_registration',
    'reject_registration',

    // 知识库（知识树节点 + 知识条目）
    'get_knowledge_tree',
    'insert_knowledge_node',
    'update_knowledge_node',
    'delete_knowledge_node',
    'move_knowledge_node',
    'get_knowledge_list',
    'get_knowledge_by_id',
    'insert_knowledge',
    'update_knowledge',
    'delete_knowledge',

    // 知识资产（OKF 新表）
    'get_knowledge_asset_by_tree_node',
    'get_knowledge_asset',
    'list_knowledge_assets',
    'create_knowledge_asset',
    'update_knowledge_asset',
    'delete_knowledge_asset',
    'attach_file_to_knowledge',

    // 流程管理（领用/归还/调拨/维修/报废/采购）
    'get_receives',
    'insert_receive',
    'update_receive',
    'delete_receive',
    'get_returns',
    'insert_return',
    'update_return',
    'delete_return',
    'get_transfers',
    'insert_transfer',
    'update_transfer',
    'delete_transfer',
    'get_repairs',
    'insert_repair',
    'update_repair',
    'delete_repair',
    'get_scraps',
    'insert_scrap',
    'update_scrap',
    'delete_scrap',
    'get_purchases',
    'insert_purchase',
    'update_purchase',
    'delete_purchase',

    // LLM 厂商/模型
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

    // Skill 管理
    'list_skills',
    'get_skill',
    'execute_skill',
    'register_custom_skill',
    'unregister_skill',
    'get_skill_count',

    // 编号规则
    'get_numbering_rules',
    'get_numbering_rule',
    'save_numbering_rule',
    'reset_numbering_sequence',

    // RAG
    'chunk_and_vectorize',
    'test_rag_retrieval',

    // 智能问答对话
    'create_conversation',
    'send_message',
    'get_conversations',
    'get_conversation_messages',
    'update_conversation_title',
    'delete_conversation',

    // 大文件上传
    'upload_init',
    'upload_start',
    'upload_report_chunk',
    'upload_complete',
    'upload_abort',
    'upload_get_progress',
    'upload_commit',
    'upload_get_version_history',
];

describe('httpAdapter 命令路由映射表', () => {
    it('覆盖后端注册的全部 Tauri 命令（映射表 或 明确未暴露清单）', () => {
        const missing = REGISTERED_COMMANDS.filter(
            (command) =>
                !Object.prototype.hasOwnProperty.call(COMMAND_ROUTE_MAP, command) &&
                !UNEXPOSED_BACKEND_COMMANDS.has(command),
        );

        expect(missing).toEqual([]);
    });

    it('未暴露清单保持最小且不与映射表重叠', () => {
        // 该集合只应包含「后端有 Tauri 命令、无 HTTP 路由、前端无调用点」的命令；
        // 一旦补了 HTTP 路由，请登记进 COMMAND_ROUTE_MAP 并从此集合移除。
        expect([...UNEXPOSED_BACKEND_COMMANDS].sort()).toEqual([
            'chunk_and_vectorize',
            'test_rag_retrieval',
        ]);

        const overlaps = Object.keys(COMMAND_ROUTE_MAP).filter((command) =>
            UNEXPOSED_BACKEND_COMMANDS.has(command),
        );
        expect(overlaps).toEqual([]);
    });

    it('映射表路径以 /api/ 开头且 HTTP 方法合法', () => {
        const LEGAL_METHODS = ['GET', 'POST', 'PUT', 'DELETE'];

        for (const [command, route] of Object.entries(COMMAND_ROUTE_MAP)) {
            // 路径必须以 /api/ 开头
            expect(route.path).toMatch(/^\/api\//);
            // HTTP 方法必须合法
            expect(LEGAL_METHODS).toContain(route.method);
            // 占位符必须为 {word} 格式
            for (const placeholder of route.path.match(/\{(\w+)\}/g) ?? []) {
                expect(placeholder.slice(1, -1)).toMatch(/^[a-zA-Z][a-zA-Z0-9_]*$/);
            }
        }
    });

    it('映射表与未暴露清单中的命令名均为小写下划线风格', () => {
        const NAME_PATTERN = /^[a-z][a-z0-9_]*$/;

        for (const command of Object.keys(COMMAND_ROUTE_MAP)) {
            expect(command).toMatch(NAME_PATTERN);
        }

        for (const command of UNEXPOSED_BACKEND_COMMANDS) {
            expect(command).toMatch(NAME_PATTERN);
        }
    });
});

// ======================== URL 拼装（D-6：不得出现 /api/api） ========================

describe('httpAdapter URL 拼装', () => {
    /** 构造成功的 ApiResponse<T> 响应 */
    function okResponse(data: unknown): Response {
        return {
            ok: true,
            status: 200,
            statusText: 'OK',
            json: jest.fn().mockResolvedValue({ code: 0, message: 'ok', data }),
            text: jest.fn().mockResolvedValue(''),
        } as unknown as Response;
    }

    /** 构造失败响应（后端 ApiError 形如 { code, message }） */
    function errorResponse(status: number, body: string): Response {
        return {
            ok: false,
            status,
            statusText: status === 401 ? 'Unauthorized' : 'Bad Request',
            json: jest.fn().mockResolvedValue({}),
            text: jest.fn().mockResolvedValue(body),
        } as unknown as Response;
    }

    afterEach(() => {
        delete (window as Window & { __API_BASE__?: unknown }).__API_BASE__;
    });

    it('基址含 /api 时只拼一次（不出现 /api/api）', async () => {
        (window as Window & { __API_BASE__?: unknown }).__API_BASE__ = 'http://127.0.0.1:3001';
        (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(okResponse([]));

        await httpAdapter.get('get_categories');

        const [url] = (global.fetch as jest.Mock).mock.calls[0];
        expect(url).toBe('http://127.0.0.1:3001/api/categories');
        expect(url).not.toContain('/api/api');
    });

    it('基址已带 /api 后缀时同样只拼一次', async () => {
        (window as Window & { __API_BASE__?: unknown }).__API_BASE__ = 'https://demo.example.com/api';
        (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(okResponse([]));

        await httpAdapter.get('get_categories');

        const [url] = (global.fetch as jest.Mock).mock.calls[0];
        expect(url).toBe('https://demo.example.com/api/categories');
    });

    it('映射表中的路径占位符应被参数替换', async () => {
        (window as Window & { __API_BASE__?: unknown }).__API_BASE__ = 'http://127.0.0.1:3001';
        (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(okResponse(null));

        await httpAdapter.delete('delete_category', { id: 7 });

        const [url] = (global.fetch as jest.Mock).mock.calls[0];
        expect(url).toBe('http://127.0.0.1:3001/api/categories/7');
    });

    it('后端 ApiError.message 应透出到错误对象（D-5）', async () => {
        (window as Window & { __API_BASE__?: unknown }).__API_BASE__ = 'http://127.0.0.1:3001';
        (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(
            errorResponse(
                403,
                JSON.stringify({
                    code: 'FORBIDDEN',
                    message: '缺少权限：asset:category:create',
                    scope: 'asset:category:create',
                }),
            ),
        );

        await expect(
            httpAdapter.post('insert_category', { category: {} }, { showError: false }),
        ).rejects.toThrow('缺少权限：asset:category:create');

        // 403 的 scope 挂到错误对象，页面可提示「缺少权限点」
        const error = await httpAdapter
            .post('insert_category', { category: {} }, { showError: false })
            .catch((err: Error & { scope?: string }) => err);
        expect((error as { scope?: string }).scope).toBe('asset:category:create');
    });
});

// ======================== 认证头 / 401 续期（C-1…C-3） ========================

describe('httpAdapter 认证与续期', () => {
    function okResponse(data: unknown): Response {
        return {
            ok: true,
            status: 200,
            statusText: 'OK',
            json: jest.fn().mockResolvedValue({ code: 0, message: 'ok', data }),
            text: jest.fn().mockResolvedValue(''),
        } as unknown as Response;
    }

    function errorResponse(status: number, body: string, statusText = 'Unauthorized'): Response {
        return {
            ok: false,
            status,
            statusText,
            json: jest.fn().mockResolvedValue({}),
            text: jest.fn().mockResolvedValue(body),
        } as unknown as Response;
    }

    /** 续期成功响应（`ApiResponse` 包裹在 `data`） */
    function refreshOk(token = 'token-new'): Response {
        return okResponse({ token, refresh_token: 'refresh-new' });
    }

    beforeEach(() => {
        (window as Window & { __API_BASE__?: unknown }).__API_BASE__ = 'http://127.0.0.1:3001';
        localStorage.clear();
    });

    afterEach(() => {
        delete (window as Window & { __API_BASE__?: unknown }).__API_BASE__;
        localStorage.clear();
    });

    it('已登录时附加 Authorization: Bearer（C-1）', async () => {
        localStorage.setItem('auth_token', 'token-abc');
        (global.fetch as jest.Mock) = jest.fn().mockResolvedValue(okResponse([]));

        await httpAdapter.get('get_categories');

        const [, init] = (global.fetch as jest.Mock).mock.calls[0];
        expect((init as RequestInit).headers).toMatchObject({
            Authorization: 'Bearer token-abc',
        });
    });

    it('401 → 续期一次 → 用新令牌重放（C-2）', async () => {
        localStorage.setItem('auth_token', 'token-old');
        localStorage.setItem('refresh_token', 'refresh-old');

        (global.fetch as jest.Mock) = jest
            .fn()
            // 首次：401
            .mockResolvedValueOnce(errorResponse(401, JSON.stringify({ code: 401, message: '访问令牌已过期' })))
            // 续期
            .mockResolvedValueOnce(refreshOk())
            // 重放：成功
            .mockResolvedValueOnce(okResponse([{ id: 1 }]));

        const result = await httpAdapter.get<Array<{ id: number }>>('get_categories', {}, { showError: false });

        expect(result).toEqual([{ id: 1 }]);
        const calls = (global.fetch as jest.Mock).mock.calls;
        expect(calls).toHaveLength(3);
        expect(calls[1][0]).toBe('http://127.0.0.1:3001/api/auth/refresh');
        expect((calls[2][1] as RequestInit).headers).toMatchObject({
            Authorization: 'Bearer token-new',
        });
        expect(localStorage.getItem('auth_token')).toBe('token-new');
    });

    it('续期失败 → 清会话并给出可操作文案（C-3）', async () => {
        localStorage.setItem('auth_token', 'token-old');
        localStorage.setItem('refresh_token', 'refresh-old');
        localStorage.setItem('auth_user', '{"id":1}');

        (global.fetch as jest.Mock) = jest
            .fn()
            .mockResolvedValueOnce(errorResponse(401, JSON.stringify({ code: 401, message: '访问令牌已过期' })))
            // 续期失败
            .mockResolvedValueOnce(errorResponse(401, JSON.stringify({ error: 'invalid_grant' }), 'Unauthorized'));

        await expect(
            httpAdapter.get('get_categories', {}, { showError: false }),
        ).rejects.toThrow('访问令牌已过期');

        expect(localStorage.getItem('auth_token')).toBeNull();
        expect(localStorage.getItem('refresh_token')).toBeNull();
        expect(localStorage.getItem('auth_user')).toBeNull();
    });

    it('认证端点 401 不触发续期（避免刷新递归）', async () => {
        localStorage.setItem('auth_token', 'token-old');
        localStorage.setItem('refresh_token', 'refresh-old');

        (global.fetch as jest.Mock) = jest
            .fn()
            .mockResolvedValueOnce(errorResponse(401, JSON.stringify({ code: 401, message: '凭证无效' })));

        await expect(
            httpAdapter.post('login', { username: 'a', password: 'b' }, { showError: false }),
        ).rejects.toThrow('凭证无效');

        // 只有登录请求本身，没有额外续期
        expect((global.fetch as jest.Mock).mock.calls).toHaveLength(1);
    });
});

// ======================== 错误消息提取 ========================

describe('extractServerMessage', () => {
    it('识别 ApiError 的 message 字段', () => {
        expect(extractServerMessage(JSON.stringify({ code: 'E', message: '上传会话已过期' })))
            .toBe('上传会话已过期');
    });

    it('识别 OAuth 错误字段 error_description / error', () => {
        expect(extractServerMessage(JSON.stringify({ error: 'invalid_grant', error_description: 'refresh token 已过期' })))
            .toBe('refresh token 已过期');
        expect(extractServerMessage(JSON.stringify({ error: 'invalid_client' })))
            .toBe('invalid_client');
    });

    it('纯文本短消息按原文返回', () => {
        expect(extractServerMessage('service unavailable')).toBe('service unavailable');
    });

    it('HTML 错误页（nginx 502）不返回，交由调用方回落到状态行', () => {
        expect(extractServerMessage('<html><body>502 Bad Gateway</body></html>')).toBeNull();
    });

    it('空响应体返回 null', () => {
        expect(extractServerMessage('')).toBeNull();
    });
});

// ======================== 传输层失败（后端未启动 / 端口不通） ========================

describe('httpAdapter 传输层失败', () => {
    const originalFetch = global.fetch;
    const origin = 'http://127.0.0.1:3001';

    beforeEach(() => {
        (window as Window & { __API_BASE__?: unknown }).__API_BASE__ = origin;
        window.sessionStorage.clear();
        localStorage.clear();
    });

    afterEach(() => {
        global.fetch = originalFetch;
        delete (window as Window & { __API_BASE__?: unknown }).__API_BASE__;
        window.sessionStorage.clear();
        localStorage.clear();
    });

    it('Failed to fetch → 抛出带地址与建议的错误，并标记 transport', async () => {
        global.fetch = jest
            .fn()
            .mockRejectedValue(new TypeError('Failed to fetch')) as unknown as typeof fetch;

        const error = await httpAdapter
            .get('get_categories', {}, { showError: false })
            .catch((err: unknown) => err);

        expect(isTransportFailure(error)).toBe(true);
        expect((error as { transport?: boolean }).transport).toBe(true);
        expect((error as Error).message).toContain(`无法连接服务器 ${origin}`);
        expect((error as Error).message).toContain('请确认后端已启动');
        // 原始文案不再直接暴露给用户
        expect((error as Error).message).not.toBe('Failed to fetch');
    });

    it('传输层失败时清掉探活成功缓存（避免下次整页加载被旧缓存放行）', async () => {
        window.sessionStorage.setItem(
            API_PROBE_CACHE_KEY,
            JSON.stringify({ origin, at: Date.now() }),
        );
        global.fetch = jest
            .fn()
            .mockRejectedValue(new TypeError('Failed to fetch')) as unknown as typeof fetch;

        await httpAdapter.get('get_categories', {}, { showError: false }).catch(() => undefined);

        expect(window.sessionStorage.getItem(API_PROBE_CACHE_KEY)).toBeNull();
    });

    it('后端业务错误（403）不会被误判为传输层失败，也不清探活缓存', async () => {
        const cacheValue = JSON.stringify({ origin, at: Date.now() });
        window.sessionStorage.setItem(API_PROBE_CACHE_KEY, cacheValue);
        global.fetch = jest.fn().mockResolvedValue({
            ok: false,
            status: 403,
            statusText: 'Forbidden',
            json: jest.fn().mockResolvedValue({}),
            text: jest
                .fn()
                .mockResolvedValue(JSON.stringify({ code: 'FORBIDDEN', message: '权限不足' })),
        }) as unknown as typeof fetch;

        const error = await httpAdapter
            .post('insert_category', { category: {} }, { showError: false })
            .catch((err: unknown) => err);

        expect((error as Error).message).toBe('权限不足');
        expect(isTransportFailure(error)).toBe(false);
        expect(window.sessionStorage.getItem(API_PROBE_CACHE_KEY)).toBe(cacheValue);
    });

    it('isTransportFailure 覆盖各浏览器的网络错误文案', () => {
        expect(isTransportFailure(new TypeError('Failed to fetch'))).toBe(true);
        expect(isTransportFailure(new TypeError('Load failed'))).toBe(true);
        expect(
            isTransportFailure(new TypeError('NetworkError when attempting to fetch resource.')),
        ).toBe(true);
        expect(isTransportFailure(new TypeError('fetch failed'))).toBe(true);
        // 业务 / 解析错误不算传输层失败
        expect(isTransportFailure(new Error('用户名或密码错误'))).toBe(false);
        expect(isTransportFailure(new SyntaxError('Unexpected token < in JSON'))).toBe(false);
        expect(isTransportFailure('Failed to fetch')).toBe(false);
    });
});
