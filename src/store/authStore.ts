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

interface AuthState {
  user: UserInfo | null;
  isLoggedIn: boolean;
  login: (user: UserInfo) => void;
  logout: () => void;
  init: () => void;
}

export const useAuthStore = create<AuthState>((set) => ({
  user: null,
  isLoggedIn: false,

  login: (user: UserInfo) => {
    localStorage.setItem('auth_user', JSON.stringify(user));
    set({ user, isLoggedIn: true });
  },

  logout: () => {
    localStorage.removeItem('auth_user');
    set({ user: null, isLoggedIn: false });
  },

  init: () => {
    const stored = localStorage.getItem('auth_user');
    if (stored) {
      try {
        const user = JSON.parse(stored) as UserInfo;
        set({ user, isLoggedIn: true });
      } catch {
        localStorage.removeItem('auth_user');
      }
    }
  },
}));
