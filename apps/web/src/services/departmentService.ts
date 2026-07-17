/**
 * 部门 API 服务
 *
 * 封装所有与部门相关的 Tauri 命令调用。
 * 统一通过 api 层处理错误、日志和重试。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

export interface Department {
    id: string;
    department_name: string;
    parent_id: string | null;
    description: string | null;
    created_by: string | null;
    created_at: string | null;
    updated_by: string | null;
    updated_at: string | null;
    deleted: number | null;
    tenant_id: string;
}

// ======================== 服务方法 ========================

/** 获取所有部门（可按租户过滤） */
export function getDepartments(tenantId?: string | null) {
    const args: Record<string, unknown> = {};
    if (tenantId) {
        args.tenantId = tenantId;
    }
    return api.get<Department[]>('get_departments', args);
}

/** 新增部门 */
export function insertDepartment(params: {
    departmentName: string;
    parentId: string | null;
    description: string | null;
    tenantId: string | null;
}) {
    return api.post<string>('insert_department', {
        departmentName: params.departmentName,
        parentId: params.parentId,
        description: params.description,
        tenantId: params.tenantId,
    });
}

/** 更新部门 */
export function updateDepartment(params: {
    id: string;
    departmentName: string;
    parentId: string | null;
    description: string | null;
}) {
    return api.put<string>('update_department', params);
}

/** 删除部门（软删除） */
export function deleteDepartment(id: string) {
    return api.delete<string>('delete_department', { id });
}
