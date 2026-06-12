/**
 * 部门 API 服务
 *
 * 封装所有与部门相关的 Tauri 命令调用。
 * 统一通过 api 层处理错误、日志和重试。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

export interface Department {
    id: number;
    department_name: string;
    parent_id: number | null;
    description: string | null;
    created_by: number | null;
    created_at: string | null;
    updated_by: number | null;
    updated_at: string | null;
    deleted: number | null;
}

// ======================== 服务方法 ========================

/** 获取所有部门 */
export function getDepartments() {
    return api.get<Department[]>('get_departments');
}

/** 新增部门 */
export function insertDepartment(params: {
    departmentName: string;
    parentId: string | null;
    description: string | null;
    createdBy: number | null;
}) {
    return api.post<string>('insert_department', params);
}

/** 更新部门 */
export function updateDepartment(params: {
    id: number;
    departmentName: string;
    parentId: string | null;
    description: string | null;
    updatedBy: number | null;
}) {
    return api.put<string>('update_department', params);
}

/** 删除部门（软删除） */
export function deleteDepartment(id: number) {
    return api.delete<string>('delete_department', { id });
}
