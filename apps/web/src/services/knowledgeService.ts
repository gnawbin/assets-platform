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
    node_type: string; // folder / document / link / raw_file / wiki_node / skill
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

/** 新增知识树节点（后端 DTO：node_type / title 必填，其余可选，snake_case） */
export function insertKnowledgeNode(params: {
    knowledgeId?: string;
    parentId?: string;
    nodeType: string;
    title: string;
    icon?: string;
    sortOrder?: number;
}) {
    return api.post<KnowledgeTree>('insert_knowledge_node', {
        knowledge_id: params.knowledgeId,
        parent_id: params.parentId,
        node_type: params.nodeType,
        title: params.title,
        icon: params.icon,
        sort_order: params.sortOrder,
    });
}

/** 更新知识树节点（id 进路径，其余字段 snake_case） */
export function updateKnowledgeNode(params: {
    id: string;
    title?: string;
    icon?: string;
    sortOrder?: number;
    isExpanded?: boolean;
}) {
    return api.put<KnowledgeTree>('update_knowledge_node', {
        id: params.id,
        title: params.title,
        icon: params.icon,
        sort_order: params.sortOrder,
        is_expanded: params.isExpanded,
    });
}

/** 删除知识树节点 */
export function deleteKnowledgeNode(id: string) {
    return api.delete<null>('delete_knowledge_node', { id });
}

/** 移动知识树节点（后端字段为 new_parent_id） */
export function moveKnowledgeNode(params: {
    id: string;
    newParentId?: string;
}) {
    return api.put<KnowledgeTree>('move_knowledge_node', {
        id: params.id,
        new_parent_id: params.newParentId,
    });
}

// ======================== 知识条目 API ========================

/** 获取知识条目列表（后端 query 参数为 knowledge_id / keyword） */
export function getKnowledgeList(params?: {
    knowledgeId?: string;
    keyword?: string;
}) {
    return api.get<AssetKnowledge[]>('get_knowledge_list', {
        knowledge_id: params?.knowledgeId,
        keyword: params?.keyword,
    });
}

/** 获取单条知识条目 */
export function getKnowledgeById(id: string) {
    return api.get<AssetKnowledge>('get_knowledge_by_id', { id });
}

/** 新增知识条目（后端 DTO 无 knowledge_id，树节点关联在节点侧维护） */
export function insertKnowledge(params: {
    assetId?: string;
    docSource?: string;
    knowledgeType?: string;
    title: string;
    content: string;
    permissionLevel?: string;
}) {
    return api.post<AssetKnowledge>('insert_knowledge', {
        asset_id: params.assetId,
        doc_source: params.docSource,
        knowledge_type: params.knowledgeType,
        title: params.title,
        content: params.content,
        permission_level: params.permissionLevel,
    });
}

/** 更新知识条目（id 进路径，其余字段 snake_case） */
export function updateKnowledge(params: {
    id: string;
    title?: string;
    content?: string;
    knowledgeType?: string;
    permissionLevel?: string;
}) {
    return api.put<AssetKnowledge>('update_knowledge', {
        id: params.id,
        title: params.title,
        content: params.content,
        knowledge_type: params.knowledgeType,
        permission_level: params.permissionLevel,
    });
}

/** 删除知识条目 */
export function deleteKnowledge(id: string) {
    return api.delete<null>('delete_knowledge', { id });
}