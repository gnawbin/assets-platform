/**
 * 资产分类 API 服务
 *
 * 封装所有与资产分类相关的 Tauri 命令调用。
 * 统一通过 api 层处理错误、日志和重试。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

export interface Category {
    id: string;
    category_name: string;
    asset_type: string;
    parent_id: string;
    sort: number;
    description: string | null;
    created_by: string | null;
    created_at: string | null;
    updated_by: string | null;
    updated_at: string | null;
    deleted: number | null;
}

/** 新增分类参数 */
export interface InsertCategoryParams {
    categoryName: string;
    assetType: string;
    parentId: string;
    sort: number;
    description: string | null;
}

/** 更新分类参数 */
export interface UpdateCategoryParams {
    id: string;
    categoryName: string;
    assetType: string;
    parentId: string;
    sort: number;
    description: string | null;
}

// ======================== 服务方法 ========================

/** 获取所有分类 */
export function getCategories() {
    return api.get<Category[]>('get_categories');
}

/** 新增分类 */
export function insertCategory(params: InsertCategoryParams) {
    return api.post<string>('insert_category', {
        category: {
            id: "0",
            category_name: params.categoryName,
            asset_type: params.assetType,
            parent_id: params.parentId,
            sort: params.sort,
            description: params.description,
            deleted: null,
        },
    });
}

/** 更新分类 */
export function updateCategory(params: UpdateCategoryParams) {
    return api.put<string>('update_category', {
        category: {
            id: params.id,
            category_name: params.categoryName,
            asset_type: params.assetType,
            parent_id: params.parentId,
            sort: params.sort,
            description: params.description,
            deleted: null,
        },
    });
}

/** 删除分类（软删除） */
export function deleteCategory(id: string) {
    return api.delete<string>('delete_category', { id });
}
