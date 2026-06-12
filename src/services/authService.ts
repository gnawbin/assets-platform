import { invoke } from '@tauri-apps/api/core';
import type { LoginResult } from '@/store/authStore';

/**
 * 用户登录
 */
export async function login(username: string, password: string): Promise<LoginResult> {
    return await invoke<LoginResult>('login', { username, password });
}
