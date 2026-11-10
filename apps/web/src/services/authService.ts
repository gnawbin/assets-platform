import { api } from '@/utils/api';
import type { LoginResult } from '@/store/authStore';
import { refreshAccessToken, revokeRefreshTokenOnServer } from '@/utils/adapters/httpAdapter';

/**
 * 用户登录
 */
export function login(username: string, password: string): Promise<LoginResult> {
    return api.post<LoginResult>('login', { username, password });
}

/**
 * 手动刷新访问令牌
 *
 * 一般无需调用：httpAdapter 在收到 401 时会自动用 refresh token 续期并重放请求。
 * 仅在需要提前续期（例如定时任务）时显式调用。
 *
 * @returns 新的访问令牌；无 refresh token 或刷新失败时返回 null
 */
export function refreshToken(): Promise<string | null> {
    return refreshAccessToken();
}

/**
 * 登出：撤销后端刷新令牌（幂等，失败不影响本地登出）
 */
export async function logout(): Promise<void> {
    const refreshToken =
        typeof window !== 'undefined' ? localStorage.getItem('refresh_token') : null;
    if (refreshToken) {
        await revokeRefreshTokenOnServer(refreshToken);
    }
}
