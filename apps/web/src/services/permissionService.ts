/**
 * 权限 API 服务
 *
 * 封装所有与角色、权限相关的 Tauri 命令调用。
 * 统一通过 api 层处理错误、日志和重试。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

export interface Role {
    id: string;
    role_key: string;
    role_name: string;
    description: string | null;
    is_super_admin: boolean;
    tenant_id: string | null;
    created_by: string | null;
    created_at: string | null;
    updated_by: string | null;
    updated_at: string | null;
    deleted: number | null;
}

export interface MantineTree {
    value: string;
    label: string;
    children: MantineTree[] | null;
    checked?: boolean;
}

// ======================== 服务方法 ========================

/** 获取所有角色（支持按租户筛选和关键词搜索，后端 query 参数为 tenant_id / keyword） */
export function getRoles(tenantId?: string, keyword?: string) {
    return api.get<Role[]>('get_roles', { tenant_id: tenantId, keyword });
}

/** 新增角色（后端 DTO：role_name / role_key 必填，snake_case） */
export function insertRole(params: {
    role_key: string;
    role_name: string;
    description?: string | null;
    tenant_id: string;
}) {
    return api.post<Role>('insert_role', {
        role_key: params.role_key,
        role_name: params.role_name,
        description: params.description || null,
        tenant_id: params.tenant_id,
    });
}

/** 删除角色（路径占位符为 {id}） */
export function deleteRole(roleId: string) {
    return api.delete<null>('delete_role', { id: roleId });
}

/** 获取所有菜单树 */
export function getAllMenusTree() {
    return api.get<MantineTree[]>('get_all_menus_tree');
}

/** 获取角色已分配的菜单 ID 列表（后端返回字符串数组，避免雪花 ID 精度丢失） */
export function getRoleMenuIds(roleId: string) {
    return api.get<string[]>('get_role_menu_ids', { id: roleId });
}

/** 分配角色菜单权限 */
export function assignRoleMenus(roleId: string, menuIds: string[]) {
    return api.post<null>('assign_role_menus', { id: roleId, menu_ids: menuIds });
}


/** 获取用户已分配的角色 ID 列表（后端返回字符串数组，避免雪花 ID 精度丢失） */
export function getUserRoleIds(userId: string) {
    return api.get<string[]>('get_user_role_ids', { id: userId });
}

/** 为用户分配角色 */
export function assignUserRoles(userId: string, roleIds: string[]) {
    return api.post<null>('assign_user_roles', { id: userId, role_ids: roleIds });
}