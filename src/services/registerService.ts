// 注册申请相关 API
import { api } from '@/utils/api';

export interface RegisterRequest {
    username: string;
    password: string;
    real_name: string;
    email?: string;
    phone?: string;
    department_name?: string;
    company_name?: string;
    reason?: string;
}

export interface RegisterResponse {
    id: number;
    username: string;
    real_name: string;
    email?: string;
    phone?: string;
    department_name?: string;
    company_name?: string;
    reason?: string;
    status: number; // 0=待审核, 1=已通过, 2=已驳回
    approve_by?: number;
    approve_time?: string;
    approve_remark?: string;
    created_at?: string;
}

// 用户注册申请
export function register(data: RegisterRequest) {
    return api.post<RegisterResponse>('register', data as unknown as Record<string, unknown>);
}

// 获取注册申请列表
export function getRegistrations(status?: number) {
    return api.get<RegisterResponse[]>('get_registrations', { status });
}

// 审核通过注册申请
export function approveRegistration(id: number, approve_by: number, tenant_id: number, approve_remark?: string) {
    return api.post<RegisterResponse>('approve_registration', { id: String(id), approve_by, tenant_id, approve_remark });
}

// 驳回注册申请
export function rejectRegistration(id: number, approve_by: number, approve_remark?: string) {
    return api.post<RegisterResponse>('reject_registration', { id: String(id), approve_by, approve_remark });
}
