/** Department model */
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