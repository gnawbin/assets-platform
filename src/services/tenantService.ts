/**
 * 租户 API 服务
 *
 * 封装所有与租户相关的 Tauri 命令调用。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

export interface Tenant {
    id: number;
    tenant_name: string;
    parent_id: number | null;
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
