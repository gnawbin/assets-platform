/**
 * authStore 回归测试
 *
 * 覆盖两处真实线上故障：
 *
 * 1. `init()` 曾把访问令牌当作 `get_current_user` 的查询参数下发
 *    → 后端 `auth_middleware` 的 `reject_token_in_query` 返回 400「令牌不得通过 URL 参数传递」
 *    → `init()` 的 catch 误判为「后端不可用」清空会话，表现为刷新页面必掉登录态；
 * 2. `/api/auth/login` 曾返回精简 DTO（只有 `user_id`，没有 `id`）
 *    → `ChatPage` 的 `user.id.toString()` 直接抛 TypeError 白屏。
 *
 * 断言基线：令牌只走 `Authorization` 头；登录结果统一归一化出 `id`。
 */

jest.mock('@/utils/api', () => ({
    api: { get: jest.fn(), post: jest.fn() },
}));

jest.mock('@/utils/adapters/httpAdapter', () => ({
    refreshAccessToken: jest.fn(),
    revokeRefreshTokenOnServer: jest.fn(),
}));

import { api } from '@/utils/api';
import { useAuthStore, type LoginResult, type UserInfo } from '@/store/authStore';

const mockGet = api.get as jest.Mock;
const mockPost = api.post as jest.Mock;

type WindowWithApiBase = Window & { __API_BASE__?: unknown };

/** 构造 payload 未过期的假 JWT（`init` 只在过期时才走刷新分支） */
function makeAccessToken(): string {
    const payload = btoa(JSON.stringify({ exp: Math.floor(Date.now() / 1000) + 3600 }));
    return `header.${payload}.signature`;
}

/** 后端 `/api/users/{id}` 返回的当前用户（`UserResponse` 同形） */
const FRESH_USER: UserInfo = {
    id: '9001',
    username: 'admin',
    real_name: '管理员',
    email: null,
    phone: null,
    department_id: null,
    is_super_admin: true,
    status: 1,
    nickname: null,
    avatar: null,
    tenant_id: '42',
};

/** 重置 zustand 单例状态，避免用例间串味 */
function resetStore() {
    useAuthStore.setState({
        user: null,
        token: null,
        isLoggedIn: false,
        isInitializing: true,
        availableTenants: [],
        selectedTenantId: null,
    });
}

describe('authStore', () => {
    beforeEach(() => {
        (window as WindowWithApiBase).__API_BASE__ = 'http://127.0.0.1:3001';
        resetStore();
    });

    afterEach(() => {
        delete (window as WindowWithApiBase).__API_BASE__;
    });

    it('init() 只按命令名请求当前用户，令牌不得进入 URL 查询参数', async () => {
        const token = makeAccessToken();
        localStorage.setItem('auth_token', token);
        // 模拟修复前登录留下的旧缓存（只有 user_id）
        localStorage.setItem('auth_user', JSON.stringify({ user_id: '9001', username: 'admin' }));
        mockGet.mockResolvedValue(FRESH_USER);

        await useAuthStore.getState().init();

        expect(mockGet).toHaveBeenCalledTimes(1);
        // 关键断言：不能有第二个参数。`{ token }` 会被 httpAdapter 拼成 `?token=...`，
        // 触发后端 400「令牌不得通过 URL 参数传递」。
        expect(mockGet.mock.calls[0]).toEqual(['get_current_user']);
        expect(useAuthStore.getState().user).toEqual(FRESH_USER);
        expect(useAuthStore.getState().isLoggedIn).toBe(true);
        expect(useAuthStore.getState().isInitializing).toBe(false);
        expect(JSON.parse(localStorage.getItem('auth_user') as string).id).toBe('9001');
    });

    it('init() 恢复登录态：保留租户列表并回写后端租户缓存', async () => {
        localStorage.setItem('auth_token', makeAccessToken());
        localStorage.setItem('auth_user', JSON.stringify(FRESH_USER));
        localStorage.setItem(
            'available_tenants',
            JSON.stringify([
                { id: '42', tenant_name: '租户B', schema_name: 'tenant_42', is_current: true },
            ]),
        );
        localStorage.setItem('selected_tenant_id', '42');
        mockGet.mockResolvedValue(FRESH_USER);
        mockPost.mockResolvedValue(undefined);

        await useAuthStore.getState().init();

        expect(useAuthStore.getState().availableTenants.map((t) => t.id)).toEqual(['42']);
        expect(useAuthStore.getState().selectedTenantId).toBe('42');
        expect(mockPost).toHaveBeenCalledWith('switch_tenant', { tenant_id: '42' });
    });

    it('init() 在取用户失败时清除会话（原有降级行为不变）', async () => {
        localStorage.setItem('auth_token', makeAccessToken());
        localStorage.setItem('auth_user', JSON.stringify(FRESH_USER));
        mockGet.mockRejectedValue(new Error('HTTP 500'));

        await useAuthStore.getState().init();

        expect(useAuthStore.getState().isLoggedIn).toBe(false);
        expect(useAuthStore.getState().isInitializing).toBe(false);
        expect(localStorage.getItem('auth_token')).toBeNull();
        expect(localStorage.getItem('auth_user')).toBeNull();
    });

    it('login() 持久化完整用户信息，并选中当前租户', () => {
        const result: LoginResult = {
            ...FRESH_USER,
            token: 'access-token',
            refresh_token: 'refresh-token',
            expires_in: 604800,
            available_tenants: [
                { id: '41', tenant_name: '租户A', schema_name: 'tenant_41', is_current: false },
                { id: '42', tenant_name: '租户B', schema_name: 'tenant_42', is_current: true },
            ],
        };

        useAuthStore.getState().login(result);

        const cached = JSON.parse(localStorage.getItem('auth_user') as string);
        expect(cached.id).toBe('9001');
        expect(cached.tenant_id).toBe('42');
        // 令牌与有效期不属于用户档案，不能写进 auth_user
        expect(cached.token).toBeUndefined();
        expect(cached.refresh_token).toBeUndefined();
        expect(cached.expires_in).toBeUndefined();
        expect(localStorage.getItem('auth_token')).toBe('access-token');
        expect(localStorage.getItem('refresh_token')).toBe('refresh-token');
        expect(useAuthStore.getState().user?.id).toBe('9001');
        expect(useAuthStore.getState().availableTenants).toHaveLength(2);
        expect(useAuthStore.getState().selectedTenantId).toBe('42');
    });

    it('login() 兼容旧后端：只有 user_id 时归一化出 id', () => {
        const legacy = {
            token: 'access-token',
            user_id: '9001',
            username: 'admin',
            real_name: '管理员',
            is_super_admin: false,
            available_tenants: [],
        };

        useAuthStore.getState().login(legacy as unknown as LoginResult);

        expect(useAuthStore.getState().user?.id).toBe('9001');
        expect(JSON.parse(localStorage.getItem('auth_user') as string).id).toBe('9001');
        // 归一化后不应残留旧字段
        expect(useAuthStore.getState().user).not.toHaveProperty('user_id');
    });
});
