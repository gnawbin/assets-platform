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
    leader_id: string | null;
    description: string | null;
    created_by: string | null;
    created_at: string | null;
    updated_by: string | null;
    updated_at: string | null;
    deleted: number | null;
    tenant_id: string;
}

// ======================== 服务方法 ========================

/** 获取所有部门（可按租户过滤，后端 query 参数为 tenant_id） */
export function getDepartments(tenantId?: string | null) {
    return api.get<Department[]>('get_departments', {
        tenant_id: tenantId ?? undefined,
    });
}

/** 新增部门（后端 DTO：department_name / tenant_id 必填，snake_case） */
export function insertDepartment(params: {
    departmentName: string;
    parentId: string | null;
    leaderId: string | null;
    description: string | null;
    tenantId: string | null;
}) {
    return api.post<Department>('insert_department', {
        department_name: params.departmentName,
        parent_id: params.parentId,
        leader_id: params.leaderId,
        description: params.description,
        tenant_id: params.tenantId,
    });
}

/** 更新部门（id 进路径，其余字段 snake_case） */
export function updateDepartment(params: {
    id: string;
    departmentName: string;
    parentId: string | null;
    leaderId: string | null;
    description: string | null;
}) {
    return api.put<Department>('update_department', {
        id: params.id,
        department_name: params.departmentName,
        parent_id: params.parentId,
        leader_id: params.leaderId,
        description: params.description,
    });
}

/** 删除部门（软删除） */
export function deleteDepartment(id: string) {
    return api.delete<string>('delete_department', { id });
}
