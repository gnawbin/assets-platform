/**
 * 流程管理 API 服务
 *
 * 封装所有与流程管理相关的 Tauri 命令调用。
 * 包括：领用、归还、调拨、维修、报废、采购。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

export interface AssetReceive {
    id: number;
    receive_no: string;
    asset_id: string;
    user_id: string;
    department_id: string;
    receive_date: string;
    reason: string;
    status: number;
    approve_by: number | null;
    approve_time: string | null;
    approve_remark: string | null;
    created_by: number | null;
    created_at: string | null;
    updated_by: number | null;
    updated_at: string | null;
}

export interface AssetReceiveInput {
    [key: string]: unknown;
    asset_id: string;
    user_id: string;
    department_id: string;
    receive_date: string;
    reason: string;
    status?: number;
}

export interface AssetReturn {
    id: number;
    return_no: string;
    receive_id: number;
    asset_id: number;
    user_id: number;
    return_date: string;
    asset_status: number;
    remark: string | null;
    confirm_by: number;
    confirm_time: string;
    created_by: number | null;
    created_at: string | null;
    updated_by: number | null;
    updated_at: string | null;
}

export interface AssetReturnInput {
    [key: string]: unknown;
    receive_id: number;
    asset_id: number;
    user_id: number;
    return_date: string;
    asset_status?: number;
    remark?: string;
    confirm_by: number;
    confirm_time: string;
}

export interface AssetTransfer {
    id: number;
    transfer_no: string;
    asset_id: number;
    out_dept_id: number;
    in_dept_id: number;
    out_user_id: number;
    in_user_id: number;
    transfer_date: string;
    reason: string;
    status: number;
    approve_by: number | null;
    approve_time: string | null;
    created_by: number | null;
    created_at: string | null;
    updated_by: number | null;
    updated_at: string | null;
}

export interface AssetTransferInput {
    [key: string]: unknown;
    asset_id: number;
    out_dept_id: number;
    in_dept_id: number;
    out_user_id: number;
    in_user_id: number;
    transfer_date: string;
    reason: string;
    status?: number;
}

export interface AssetRepair {
    id: number;
    repair_no: string;
    asset_id: number;
    fault_desc: string;
    repair_desc: string | null;
    repair_user_id: number | null;
    repair_dept_id: number | null;
    repair_file_url: string | null;
    repair_type: number;
    vendor: string | null;
    cost: number | null;
    apply_date: string;
    repair_date: string | null;
    finish_date: string | null;
    status: number;
    created_by: number | null;
    created_at: string | null;
    updated_by: number | null;
    updated_at: string | null;
}

export interface AssetRepairInput {
    [key: string]: unknown;
    asset_id: number;
    fault_desc: string;
    repair_desc?: string;
    repair_user_id?: number;
    repair_dept_id?: number;
    repair_file_url?: string;
    repair_type?: number;
    vendor?: string;
    cost?: number;
    apply_date: string;
    repair_date?: string;
    finish_date?: string;
    status?: number;
}

export interface AssetScrap {
    id: number;
    scrap_no: string;
    asset_id: number;
    reason: string;
    scrap_date: string;
    status: number;
    approve_by: number | null;
    approve_time: string | null;
    handle_user: number | null;
    created_by: number | null;
    created_at: string | null;
    updated_by: number | null;
    updated_at: string | null;
}

export interface AssetScrapInput {
    [key: string]: unknown;
    asset_id: number;
    reason: string;
    scrap_date: string;
    status?: number;
    handle_user?: number;
}

export interface AssetPurchase {
    id: number;
    purchase_no: string;
    asset_name: string;
    category_id: number;
    model: string | null;
    manufacturer: string | null;
    quantity: number;
    unit_price: number | null;
    total_price: number | null;
    apply_user: number;
    dept_id: number;
    reason: string;
    status: number;
    supplier: string | null;
    purchase_date: string | null;
    arrive_date: string | null;
    created_by: number | null;
    created_at: string | null;
    updated_by: number | null;
    updated_at: string | null;
}

export interface AssetPurchaseInput {
    [key: string]: unknown;
    asset_name: string;
    category_id: number;
    model?: string;
    manufacturer?: string;
    quantity: number;
    unit_price?: number;
    total_price?: number;
    apply_user: number;
    dept_id: number;
    reason: string;
    status?: number;
    supplier?: string;
    purchase_date?: string;
    arrive_date?: string;
}

// ======================== 领用管理 ========================

/** 获取所有领用记录 */
export function getReceives() {
    return api.get<AssetReceive[]>('get_receives');
}

/** 新增领用记录 */
export function insertReceive(params: AssetReceiveInput) {
    return api.post<AssetReceive>('insert_receive', params);
}

/** 更新领用记录 */
export function updateReceive(id: number, params: AssetReceiveInput) {
    return api.put<AssetReceive>('update_receive', { id, input: params });
}

/** 删除领用记录 */
export function deleteReceive(id: number) {
    return api.delete<null>('delete_receive', { id });
}

// ======================== 归还管理 ========================

/** 获取所有归还记录 */
export function getReturns() {
    return api.get<AssetReturn[]>('get_returns');
}

/** 新增归还记录 */
export function insertReturn(params: AssetReturnInput) {
    return api.post<AssetReturn>('insert_return', params);
}

/** 更新归还记录 */
export function updateReturn(id: number, params: AssetReturnInput) {
    return api.put<AssetReturn>('update_return', { id, input: params });
}

/** 删除归还记录 */
export function deleteReturn(id: number) {
    return api.delete<null>('delete_return', { id });
}

// ======================== 调拨管理 ========================

/** 获取所有调拨记录 */
export function getTransfers() {
    return api.get<AssetTransfer[]>('get_transfers');
}

/** 新增调拨记录 */
export function insertTransfer(params: AssetTransferInput) {
    return api.post<AssetTransfer>('insert_transfer', params);
}

/** 更新调拨记录 */
export function updateTransfer(id: number, params: AssetTransferInput) {
    return api.put<AssetTransfer>('update_transfer', { id, input: params });
}

/** 删除调拨记录 */
export function deleteTransfer(id: number) {
    return api.delete<null>('delete_transfer', { id });
}

// ======================== 维修管理 ========================

/** 获取所有维修记录 */
export function getRepairs() {
    return api.get<AssetRepair[]>('get_repairs');
}

/** 新增维修记录 */
export function insertRepair(params: AssetRepairInput) {
    return api.post<AssetRepair>('insert_repair', params);
}

/** 更新维修记录 */
export function updateRepair(id: number, params: AssetRepairInput) {
    return api.put<AssetRepair>('update_repair', { id, input: params });
}

/** 删除维修记录 */
export function deleteRepair(id: number) {
    return api.delete<null>('delete_repair', { id });
}

// ======================== 报废管理 ========================

/** 获取所有报废记录 */
export function getScraps() {
    return api.get<AssetScrap[]>('get_scraps');
}

/** 新增报废记录 */
export function insertScrap(params: AssetScrapInput) {
    return api.post<AssetScrap>('insert_scrap', params);
}

/** 更新报废记录 */
export function updateScrap(id: number, params: AssetScrapInput) {
    return api.put<AssetScrap>('update_scrap', { id, input: params });
}

/** 删除报废记录 */
export function deleteScrap(id: number) {
    return api.delete<null>('delete_scrap', { id });
}

// ======================== 采购管理 ========================

/** 获取所有采购记录 */
export function getPurchases() {
    return api.get<AssetPurchase[]>('get_purchases');
}

/** 新增采购记录 */
export function insertPurchase(params: AssetPurchaseInput) {
    return api.post<AssetPurchase>('insert_purchase', params);
}

/** 更新采购记录 */
export function updatePurchase(id: number, params: AssetPurchaseInput) {
    return api.put<AssetPurchase>('update_purchase', { id, input: params });
}

/** 删除采购记录 */
export function deletePurchase(id: number) {
    return api.delete<null>('delete_purchase', { id });
}
