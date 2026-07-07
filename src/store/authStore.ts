'use client';

import { create } from 'zustand';
import { api } from '@/utils/api';

export interface TenantInfo {
  id: string;
  tenant_name: string;
  schema_name: string | null;
  is_current: boolean;
}

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
  tenant_id: string | null;
  available_tenants?: TenantInfo[];
}

export interface LoginResult extends UserInfo {
  token: string;
  available_tenants: TenantInfo[];
}

interface AuthState {
  user: UserInfo | null;
  token: string | null;
  isLoggedIn: boolean;
  isInitializing: boolean;
  availableTenants: TenantInfo[];
  selectedTenantId: string | null;
  login: (result: LoginResult) => void;
  logout: () => void;
  switchTenant: (tenantId: string) => void;
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

export const useAuthStore = create<AuthState>((set, get) => ({
  user: null,
  token: null,
  isLoggedIn: false,
  isInitializing: true,
  availableTenants: [],
  selectedTenantId: null,

  login: (result: LoginResult) => {
    const { token, available_tenants, ...user } = result;
    localStorage.setItem('auth_token', token);
    localStorage.setItem('auth_user', JSON.stringify(user));
    localStorage.setItem('available_tenants', JSON.stringify(available_tenants || []));

    // 自动选中用户的当前租户（第一个或 tenant_id 对应的）
    let selectedTenantId: string | null = null;
    if (available_tenants && available_tenants.length > 0) {
      // 优先使用用户的 tenant_id 匹配（两者都是 string，安全比较）
      if (user.tenant_id) {
        const match = available_tenants.find(t => t.id === user.tenant_id);
        if (match) {
          selectedTenantId = match.id;
        }
      }
      // 如果没有匹配，选第一个
      if (!selectedTenantId) {
        selectedTenantId = available_tenants[0].id;
      }
    }
    if (selectedTenantId) {
      localStorage.setItem('selected_tenant_id', selectedTenantId);
    }

    set({
      user,
      token,
      isLoggedIn: true,
      availableTenants: available_tenants || [],
      selectedTenantId,
    });
  },

  logout: () => {
    localStorage.removeItem('auth_token');
    localStorage.removeItem('auth_user');
    localStorage.removeItem('available_tenants');
    localStorage.removeItem('selected_tenant_id');
    set({
      user: null,
      token: null,
      isLoggedIn: false,
      availableTenants: [],
      selectedTenantId: null,
    });
  },

  switchTenant: (tenantId: string) => {
    localStorage.setItem('selected_tenant_id', tenantId);
    set({ selectedTenantId: tenantId });
  },

  init: async () => {
    set({ isInitializing: true });

    const storedToken = localStorage.getItem('auth_token');
    const storedUser = localStorage.getItem('auth_user');
    const storedTenants = localStorage.getItem('available_tenants');
    const storedSelectedTenant = localStorage.getItem('selected_tenant_id');

    if (storedToken && storedUser) {
      // 验证 token 是否过期
      if (isTokenExpired(storedToken)) {
        // token 已过期，清除登录状态
        localStorage.removeItem('auth_token');
        localStorage.removeItem('auth_user');
        localStorage.removeItem('available_tenants');
        localStorage.removeItem('selected_tenant_id');
        set({
          user: null,
          token: null,
          isLoggedIn: false,
          isInitializing: false,
          availableTenants: [],
          selectedTenantId: null,
        });
        return;
      }

      try {
        // 优先从后端获取最新用户信息
        const freshUser = await api.get<UserInfo>('get_current_user', { token: storedToken });
        // 更新 localStorage 中的缓存
        localStorage.setItem('auth_user', JSON.stringify(freshUser));

        // 解析 localStorage 中的 available_tenants，直接保留 id 为 string（不转 Number）
        // 后端将 i64 序列化为字符串，JSON.parse 后 id 就是 string，无需转换
        const parsedTenants: TenantInfo[] = storedTenants
          ? JSON.parse(storedTenants)
          : [];

        set({
          user: freshUser,
          token: storedToken,
          isLoggedIn: true,
          isInitializing: false,
          availableTenants: parsedTenants,
          selectedTenantId: storedSelectedTenant ?? null,
        });

        // 恢复租户后，同步通知后端更新 USER_TENANT_CACHE（刷新页面后后端缓存丢失）
        if (storedSelectedTenant) {
          try {
            await api.post('switch_tenant', {
              userId: String(freshUser.id ?? ''),
              tenantId: storedSelectedTenant,
            });
          } catch {
            // 切换租户失败不影响登录状态，静默处理
          }
        }

        return;
      } catch {
        // 后端服务不可用（未启动 / 故障 / 重启），清除缓存，直接走登录页
        localStorage.removeItem('auth_token');
        localStorage.removeItem('auth_user');
        localStorage.removeItem('available_tenants');
        localStorage.removeItem('selected_tenant_id');
        set({
          user: null,
          token: null,
          isLoggedIn: false,
          isInitializing: false,
          availableTenants: [],
          selectedTenantId: null,
        });
      }
    }

    set({ isInitializing: false });
  },
}));