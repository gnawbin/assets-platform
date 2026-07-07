/**
 * 用户 API 服务
 *
 * 封装所有与用户相关的 Tauri 命令调用。
 * 统一通过 api 层处理错误、日志和重试。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

export interface User {
    id: string;
    username: string;
    real_name: string;
    email: string | null;
    phone: string | null;
    department_id: string | null;
    is_super_admin: boolean;
    status: number;
    nickname: string | null;
    avatar: string | null;
    person_id: string | null;
    person_code: string | null;
    super_user_id: string | null;
    tenant_id: string | null;
    tenant_name: string | null;
    created_by: string | null;
    created_at: string | null;
    updated_by: string | null;
    updated_at: string | null;
}

// ======================== 服务方法 ========================

/** 获取用户列表 */
export function getUsers(tenantId?: string | null, keyword?: string) {
    const args: Record<string, unknown> = {};
    if (tenantId !== undefined && tenantId !== null) {
        args.tenantId = tenantId;
    }
    if (keyword) {
        args.keyword = keyword;
    }
    return api.get<User[]>('get_users', Object.keys(args).length > 0 ? args : undefined);
}


/** 新增用户 */
export function insertUser(params: {
    username: string;
    password: string;
    realName: string;
    email: string | null;
    phone: string | null;
    departmentId: number | null;
    status: number;
    nickname: string | null;
    personId: null;
    personCode: string | null;
    superUserId: null;
    tenantId: string | null;
}) {
    return api.post<string>('insert_user', params);
}

/** 更新用户 */
export function updateUser(params: {
    id: string;
    username: string;
    realName: string;
    email: string | null;
    phone: string | null;
    departmentId: number | null;
    status: number;
    nickname: string | null;
    personId: null;
    personCode: string | null;
    superUserId: null;
}) {
    return api.put<string>('update_user', params);
}

/** 删除用户（软删除） */
export function deleteUser(id: string, currentUserId: string, isSuperAdmin: boolean) {
    return api.delete<string>('delete_user', { id, currentUserId, isSuperAdmin });
}

/** 重置密码 */
export function resetPassword(id: string, newPassword: string) {
    return api.post<string>('reset_password', { id, newPassword });
}
