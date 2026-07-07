/**
 * 租户 API 服务
 *
 * 封装所有与租户相关的 Tauri 命令调用。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

export interface Tenant {
    id: string;
    tenant_name: string;
    parent_id: string | null;
    is_leaf: boolean;
    schema_name: string | null;
    enable: boolean;
    create_at: string | null;
    updated_at: string | null;
}

// ======================== 服务方法 ========================

/** 获取所有租户 */
export function getTenants() {
    return api.get<Tenant[]>('get_tenants');
}

/** 新增租户 */
export function insertTenant(params: {
    tenantName: string;
    parentId: string | null;
    isLeaf: boolean;
    schemaName: string | null;
    enable: boolean;
    createdBy: number | null;
}) {
    return api.post<Tenant>('insert_tenant', params);
}

/** 更新租户 */
export function updateTenant(params: {
    id: number;
    tenantName: string;
    enable: boolean;
}) {
    return api.put<Tenant>('update_tenant', params);
}

/** 删除租户（禁用租户） */
export function deleteTenant(id: number) {
    return api.delete<Tenant>('delete_tenant', { id });
}

/** 获取用户可访问的租户列表 */
export function getUserTenants(userId: number | string) {
    return api.get<Tenant[]>('get_user_tenants', { userId: String(userId) });
}

/** 为用户分配租户（覆盖式） */
export function assignUserTenants(userId: number | string, tenantIds: string[], currentUserId: number | string) {
    return api.post<void>('assign_user_tenants', {
        userId: String(userId),
        tenantIds: tenantIds.map(String),
        currentUserId: String(currentUserId),
    });
}
