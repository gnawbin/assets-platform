'use client';

import { create } from 'zustand';
import { api } from '@/utils/api';

export interface UserInfo {
  id: number;
  username: string;
  real_name: string;
  email: string | null;
  phone: string | null;
  department_id: number | null;
  is_super_admin: boolean;
  status: number;
  nickname: string | null;
  avatar: string | null;
  tenant_id: number | null;
}

export interface LoginResult extends UserInfo {
  token: string;
}

interface AuthState {
  user: UserInfo | null;
  token: string | null;
  isLoggedIn: boolean;
  login: (result: LoginResult) => void;
  logout: () => void;
  init: () => Promise<void>;
}

/**
 * 解析 JWT token 的 payload 部分，检查是否过期
 */
function isTokenExpired(token: string): boolean {
  try {
    const parts = token.split('.');
    if (parts.length !== 3) return true;
    const payload = JSON.parse(atob(parts[1]));
    if (payload.exp) {
      const now = Math.floor(Date.now() / 1000);
      return now >= payload.exp;
    }
    return false;
  } catch {
    return true;
  }
}

export const useAuthStore = create<AuthState>((set) => ({
  user: null,
  token: null,
  isLoggedIn: false,

  login: (result: LoginResult) => {
    const { token, ...user } = result;
    localStorage.setItem('auth_token', token);
    localStorage.setItem('auth_user', JSON.stringify(user));
    set({ user, token, isLoggedIn: true });
  },

  logout: () => {
    localStorage.removeItem('auth_token');
    localStorage.removeItem('auth_user');
    set({ user: null, token: null, isLoggedIn: false });
  },

  init: async () => {
    const storedToken = localStorage.getItem('auth_token');
    const storedUser = localStorage.getItem('auth_user');

    if (storedToken && storedUser) {
      // 验证 token 是否过期
      if (isTokenExpired(storedToken)) {
        // token 已过期，清除登录状态
        localStorage.removeItem('auth_token');
        localStorage.removeItem('auth_user');
        set({ user: null, token: null, isLoggedIn: false });
        return;
      }

      try {
        // 优先从后端获取最新用户信息
        const freshUser = await api.get<UserInfo>('get_current_user', { token: storedToken });
        // 更新 localStorage 中的缓存
        localStorage.setItem('auth_user', JSON.stringify(freshUser));
        set({ user: freshUser, token: storedToken, isLoggedIn: true });
        return;
      } catch {
        // 后端请求失败，降级使用 localStorage 的缓存
        try {
          const user = JSON.parse(storedUser) as UserInfo;
          set({ user, token: storedToken, isLoggedIn: true });
        } catch {
          localStorage.removeItem('auth_token');
          localStorage.removeItem('auth_user');
        }
      }
    }
  },
}));
