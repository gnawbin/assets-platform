'use client';

import { create } from 'zustand';

export interface UserInfo {
  id: number;
  username: string;
  real_name: string;
  email: string | null;
  phone: string | null;
  department_id: number | null;
  status: number;
  nickname: string | null;
  avatar: string | null;
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
  init: () => void;
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

  init: () => {
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
        const user = JSON.parse(storedUser) as UserInfo;
        set({ user, token: storedToken, isLoggedIn: true });
      } catch {
        localStorage.removeItem('auth_token');
        localStorage.removeItem('auth_user');
      }
    }
  },
}));
