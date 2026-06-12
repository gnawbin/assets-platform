/**
 * 无形资产 API 服务
 *
 * 封装所有与无形资产相关的 Tauri 命令调用。
 * 统一通过 api 层处理错误、日志和重试。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

export interface IntangibleAssetView {
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
    // intangible_assets 扩展字段
    intangible_id: number | null;
    intangible_type: string | null;
    register_no: string | null;
    register_owner: string | null;
    register_date: string | null;
    valid_start_date: string | null;
    valid_end_date: string | null;
    right_status: string | null;
    license_key: string | null;
    license_type: string | null;
    authorized_scope: string | null;
    assigned_user_ids: string | null;
    bind_type: string | null;
    bind_info: string | null;
    version: string | null;
    download_link: string | null;
    amortization_method: string | null;
    useful_life: number | null;
    amortization_amount: number | null;
    residual_rate: number | null;
}

export interface IntangibleAssetInput {
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
    intangible_type: string | null;
    register_no: string | null;
    register_owner: string | null;
    register_date: string | null;
    valid_start_date: string | null;
    valid_end_date: string | null;
    right_status: string | null;
    license_key: string | null;
    license_type: string | null;
    authorized_scope: string | null;
    assigned_user_ids: string | null;
    bind_type: string | null;
    bind_info: string | null;
    version: string | null;
    download_link: string | null;
    amortization_method: string | null;
    useful_life: number | null;
    amortization_amount: number | null;
    residual_rate: number | null;
}

// ======================== 服务方法 ========================

/** 获取所有无形资产 */
export function getIntangibleAssets() {
    return api.get<IntangibleAssetView[]>('get_intangible_assets');
}

/** 新增无形资产 */
export function insertIntangibleAsset(input: IntangibleAssetInput) {
    return api.post<string>('insert_intangible_asset', { input });
}

/** 更新无形资产 */
export function updateIntangibleAsset(id: number, input: IntangibleAssetInput) {
    return api.put<string>('update_intangible_asset', { id, input });
}

/** 删除无形资产（软删除） */
export function deleteIntangibleAsset(id: number) {
    return api.delete<string>('delete_intangible_asset', { id });
}
