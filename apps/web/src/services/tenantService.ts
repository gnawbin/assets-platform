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

/** 新增租户（后端 DTO：tenant_name / is_leaf / enable 必填，snake_case） */
export function insertTenant(params: {
    tenantName: string;
    parentId: string | null;
    isLeaf: boolean;
    schemaName: string | null;
    enable: boolean;
}) {
    return api.post<Tenant>('insert_tenant', {
        tenant_name: params.tenantName,
        parent_id: params.parentId,
        is_leaf: params.isLeaf,
        schema_name: params.schemaName,
        enable: params.enable,
    });
}

/** 更新租户（id 进路径，其余字段 snake_case） */
export function updateTenant(params: {
    id: string;
    tenantName: string;
    enable: boolean;
}) {
    return api.put<Tenant>('update_tenant', {
        id: params.id,
        tenant_name: params.tenantName,
        enable: params.enable,
    });
}

/** 删除租户（禁用租户） */
export function deleteTenant(id: string) {
    return api.delete<Tenant>('delete_tenant', { id });
}

/** 获取用户可访问的租户列表（路径占位符为 {id}） */
export function getUserTenants(userId: number | string) {
    return api.get<Tenant[]>('get_user_tenants', { id: String(userId) });
}

/** 为用户分配租户（覆盖式；操作人取自登录态 token） */
export function assignUserTenants(userId: number | string, tenantIds: string[]) {
    return api.post<void>('assign_user_tenants', {
        user_id: String(userId),
        tenant_ids: tenantIds.map(String),
    });
}
