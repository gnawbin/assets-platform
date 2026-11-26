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

/** 获取用户列表（后端 query 参数为 tenant_id / keyword） */
export function getUsers(tenantId?: string | null, keyword?: string) {
    const args: Record<string, unknown> = {};
    if (tenantId !== undefined && tenantId !== null) {
        args.tenant_id = tenantId;
    }
    if (keyword) {
        args.keyword = keyword;
    }
    return api.get<User[]>('get_users', Object.keys(args).length > 0 ? args : undefined);
}


/**
 * 新增用户
 *
 * 后端 `CreateUserRequest` 为扁平 snake_case（username / password / real_name 必填）；
 * 雪花 ID 一律按字符串传输，避免 `Number()` 造成的精度丢失。
 */
export function insertUser(params: {
    username: string;
    password: string;
    realName: string;
    email: string | null;
    phone: string | null;
    departmentId: string | null;
    status: number;
    nickname: string | null;
    personId: string | null;
    personCode: string | null;
    superUserId: string | null;
    tenantId: string | null;
}) {
    return api.post<User>('insert_user', {
        username: params.username,
        password: params.password,
        real_name: params.realName,
        email: params.email,
        phone: params.phone,
        department_id: params.departmentId,
        status: params.status,
        nickname: params.nickname,
        person_id: params.personId,
        person_code: params.personCode,
        super_user_id: params.superUserId,
        tenant_id: params.tenantId,
    });
}

/** 更新用户（id 进路径，其余字段 snake_case） */
export function updateUser(params: {
    id: string;
    username: string;
    realName: string;
    email: string | null;
    phone: string | null;
    departmentId: string | null;
    status: number;
    nickname: string | null;
    personId: string | null;
    personCode: string | null;
    superUserId: string | null;
}) {
    return api.put<User>('update_user', {
        id: params.id,
        username: params.username,
        real_name: params.realName,
        email: params.email,
        phone: params.phone,
        department_id: params.departmentId,
        status: params.status,
        nickname: params.nickname,
        person_id: params.personId,
        person_code: params.personCode,
        super_user_id: params.superUserId,
    });
}

/** 删除用户（软删除，操作人取自登录态 token） */
export function deleteUser(id: string) {
    return api.delete<null>('delete_user', { id });
}

/** 重置密码（id 进路径，body 为 new_password） */
export function resetPassword(id: string, newPassword: string) {
    return api.post<null>('reset_password', { id, new_password: newPassword });
}
