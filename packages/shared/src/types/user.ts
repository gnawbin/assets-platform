/** User information returned from backend */
export interface UserInfo {
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
    tenant_id: string | null;
    available_tenants?: TenantInfo[];
}

/** Tenant information */
export interface TenantInfo {
    id: string;
    tenant_name: string;
    schema_name: string | null;
    is_current: boolean;
}

/** Login result */
export interface LoginResult extends UserInfo {
    token: string;
    available_tenants: TenantInfo[];
}