/**
 * 固定资产 API 服务
 *
 * 封装所有与固定资产相关的 Tauri 命令调用。
 * 统一通过 api 层处理错误、日志和重试。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

export interface HardwareAssetView {
    id: number;
    asset_no: string;
    asset_type: string;
    category_id: number;
    asset_name: string;
    manufacturer: string | null;
    model: string | null;
    department_id: number | null;
    user_id: number | null;
    status: number;
    purchase_date: string | null;
    purchase_price: number | null;
    quantity: number | null;
    used_quantity: number | null;
    expire_date: string | null;
    description: string | null;
    created_by: number | null;
    created_at: string | null;
    updated_by: number | null;
    updated_at: string | null;
    deleted: number | null;
    // hard_assets 扩展字段
    hard_id: number | null;
    sn: string | null;
    mac_address: string | null;
    location: string | null;
    hardware_config: string | null;
    use_user_id: number | null;
    use_start_date: string | null;
    maintenance_vendor: string | null;
    maintenance_type: string | null;
    maintenance_expire_date: string | null;
    fault_desc: string | null;
}

export interface HardwareAssetInput {
    category_id: number;
    asset_name: string;
    manufacturer: string | null;
    model: string | null;
    department_id: number | null;
    user_id: number | null;
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
    use_user_id: number | null;
    use_start_date: string | null;
    maintenance_vendor: string | null;
    maintenance_type: string | null;
    maintenance_expire_date: string | null;
    fault_desc: string | null;
}

// ======================== 服务方法 ========================

/** 获取所有固定资产 */
export function getHardwareAssets() {
    return api.get<HardwareAssetView[]>('get_hardware_assets');
}

/** 新增固定资产 */
export function insertHardwareAsset(input: HardwareAssetInput) {
    return api.post<string>('insert_hardware_asset', { input });
}

/** 更新固定资产 */
export function updateHardwareAsset(id: number, input: HardwareAssetInput) {
    return api.put<string>('update_hardware_asset', { id, input });
}

/** 删除固定资产（软删除） */
export function deleteHardwareAsset(id: number) {
    return api.delete<string>('delete_hardware_asset', { id });
}
