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
 * 从 sys_menu 表查询可见的目录和菜单（menu_type=1 或 2），
 * 按 order_num 排序，构建树形结构返回。
 *
 * @returns 树形菜单列表
 */
export function getUserMenus(): Promise<MenuItem[]> {
    return api.get<MenuItem[]>('get_user_menus');
}
