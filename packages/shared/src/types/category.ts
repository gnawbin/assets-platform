/** Asset category model */
export interface Category {
    id: string;
    name: string;
    parent_id: string | null;
    sort_order: number | null;
    status: number | null;
    remark: string | null;
    created_by: string | null;
    created_at: string | null;
    updated_by: string | null;
    updated_at: string | null;
    deleted: number | null;
    /** Children categories (for tree structure) */
    children?: Category[];
}