import { api } from '@/utils/api';
import type { LoginResult } from '@/store/authStore';

/**
 * 用户登录
 */
export function login(username: string, password: string): Promise<LoginResult> {
    return api.post<LoginResult>('login', { username, password });
}
