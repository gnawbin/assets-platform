/**
 * 菜单 API 服务
 *
 * 封装从 sys_menu 表获取动态菜单数据的 API 调用。
 * 用于 Sidebar 组件动态渲染左侧导航菜单。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

/** 侧边栏菜单项 */
export interface MenuItem {
    /** 菜单显示名称 */
    label: string;
    /** 路由路径（菜单/目录用） */
    path?: string;
    /** 图标名称（如 'IconDashboard', 'IconBooks'） */
    icon?: string;
    /** 子菜单 */
    children?: MenuItem[];
}

// ======================== 服务方法 ========================

/**
 * 获取当前用户的侧边栏菜单
 *
 * 根据当前登录用户的角色过滤菜单：
 * - 超级管理员：返回所有可见菜单
 * - 普通用户：只返回其角色已分配的菜单
 *
 * @param userId 当前登录用户ID（Tauri 模式需要传递，HTTP 模式由 JWT 自动识别）
 * @returns 树形菜单列表
 */
export function getUserMenus(userId?: string): Promise<MenuItem[]> {
    // 如果未传入 userId，尝试从 localStorage 获取用户信息作为兜底
    let effectiveUserId = userId;
    if (!effectiveUserId) {
        try {
            if (typeof window !== 'undefined') {
                const stored = localStorage.getItem('auth_user');
                if (stored) {
                    const user = JSON.parse(stored);
                    effectiveUserId = user.id?.toString();
                    console.log('[menuService] 从 localStorage 获取用户ID:', effectiveUserId);
                }
            }
        } catch {
            // 静默失败
        }
    }

    const args = effectiveUserId ? { userId: effectiveUserId } : undefined;
    console.log('[menuService] 调用 getUserMenus, args:', args);
    return api.get<MenuItem[]>('get_user_menus', args);
}


