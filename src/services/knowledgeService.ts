/**
 * 知识库 API 服务
 *
 * 封装所有与知识树、知识条目相关的 Tauri 命令调用。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

/** 知识树节点（树形结构，用于前端渲染） */
export interface KnowledgeTreeNode {
    id: string;
    knowledge_id: string | null;
    parent_id: string | null;
    node_type: string; // folder / document / link
    title: string;
    icon: string | null;
    sort_order: number;
    is_expanded: boolean;
    children: KnowledgeTreeNode[];
}

/** 知识树节点（扁平结构，对应数据库记录） */
export interface KnowledgeTree {
    id: string;
    knowledge_id: string | null;
    parent_id: string | null;
    node_type: string;
    title: string;
    icon: string | null;
    sort_order: number;
    is_expanded: boolean;
    created_by: string | null;
    created_at: string | null;
    updated_by: string | null;
    updated_at: string | null;
    deleted: number;
}

/** 知识条目 */
export interface AssetKnowledge {
    id: string;
    asset_id: string | null;
    doc_source: string; // manual / asset / hardware / intangible / document
    knowledge_type: string; // basic / contract / hardware / intangible
    title: string;
    content: string;
    chunk_index: number;
    vector_data: number[] | null;
    permission_level: string; // public / internal / secret
    owner_type: string | null;
    owner_id: string | null;
    created_by: string | null;
    created_at: string | null;
    updated_by: string | null;
    updated_at: string | null;
    deleted: number;
}

// ======================== 知识树节点 API ========================

/** 获取完整知识树 */
export function getKnowledgeTree() {
    return api.get<KnowledgeTreeNode[]>('get_knowledge_tree');
}

/** 新增知识树节点 */
export function insertKnowledgeNode(params: {
    knowledge_id?: string;
    parent_id?: string;
    node_type: string;
    title: string;
    icon?: string;
    sort_order?: number;
    created_by?: string;
}) {
    return api.post<KnowledgeTree>('insert_knowledge_node', params);
}

/** 更新知识树节点 */
export function updateKnowledgeNode(params: {
    id: string;
    title?: string;
    icon?: string;
    sort_order?: number;
    is_expanded?: boolean;
    updated_by?: string;
}) {
    return api.put<KnowledgeTree>('update_knowledge_node', params);
}

/** 删除知识树节点 */
export function deleteKnowledgeNode(id: string) {
    return api.delete<null>('delete_knowledge_node', { id });
}

/** 移动知识树节点 */
export function moveKnowledgeNode(params: {
    id: string;
    new_parent_id?: string;
}) {
    return api.put<KnowledgeTree>('move_knowledge_node', params);
}

// ======================== 知识条目 API ========================

/** 获取知识条目列表 */
export function getKnowledgeList(params?: {
    knowledge_id?: string;
    keyword?: string;
}) {
    return api.get<AssetKnowledge[]>('get_knowledge_list', params ?? {});
}

/** 获取单条知识条目 */
export function getKnowledgeById(id: string) {
    return api.get<AssetKnowledge>('get_knowledge_by_id', { id });
}

/** 新增知识条目 */
export function insertKnowledge(params: {
    knowledge_id?: string;
    asset_id?: string;
    doc_source?: string;
    knowledge_type?: string;
    title: string;
    content: string;
    permission_level?: string;
    created_by?: string;
}) {

    return api.post<AssetKnowledge>('insert_knowledge', params);
}

/** 更新知识条目 */
export function updateKnowledge(params: {
    id: string;
    title?: string;
    content?: string;
    knowledge_type?: string;
    permission_level?: string;
    updated_by?: string;
}) {
    return api.put<AssetKnowledge>('update_knowledge', params);
}

/** 删除知识条目 */
export function deleteKnowledge(id: string) {
    return api.delete<null>('delete_knowledge', { id });
}
