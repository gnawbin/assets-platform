/** Hardware asset view model (returned from backend) */
export interface HardwareAssetView {
    id: string;
    asset_no: string;
    asset_type: string;
    category_id: string;
    asset_name: string;
    manufacturer: string | null;
    model: string | null;
    department_id: string | null;
    user_id: string | null;
    status: number;
    purchase_date: string | null;
    purchase_price: number | null;
    quantity: number | null;
    used_quantity: number | null;
    expire_date: string | null;
    description: string | null;
    created_by: string | null;
    created_at: string | null;
    updated_by: string | null;
    updated_at: string | null;
    deleted: number | null;
    // hard_assets extension fields
    hard_id: string | null;
    sn: string | null;
    mac_address: string | null;
    location: string | null;
    hardware_config: string | null;
    use_user_id: string | null;
    use_start_date: string | null;
    maintenance_vendor: string | null;
    maintenance_type: string | null;
    maintenance_expire_date: string | null;
    fault_desc: string | null;
}

/** Hardware asset input (for create/update) */
export interface HardwareAssetInput {
    category_id: string;
    asset_name: string;
    manufacturer: string | null;
    model: string | null;
    department_id: string | null;
    user_id: string | null;
    status: number | null;
    purchase_date: string | null;
    purchase_price: number | null;
    quantity: number | null;
    used_quantity: number | null;
    expire_date: string | null;
    description: string | null;
    sn: string | null;
    mac_address: string | null;
    location: string | null;
    hardware_config: string | null;
    use_user_id: string | null;
    use_start_date: string | null;
    maintenance_vendor: string | null;
    maintenance_type: string | null;
    maintenance_expire_date: string | null;
    fault_desc: string | null;
}