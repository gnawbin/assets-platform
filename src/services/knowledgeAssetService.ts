/**
 * OKF 知识资产 API 服务
 *
 * 操作全新的 knowledge_asset 表，与旧 knowledgeService.ts 完全独立。
 * 支持 OKF 知识类型、文件上传绑定、Markdown 内容管理。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

/** OKF 知识类型 */
export type OkfType = 'raw_source' | 'concept' | 'fact' | 'rule' | 'param' | 'process' | 'case';

/** 知识资产状态 */
export type KnowledgeStatus = 'draft' | 'valid' | 'outdated' | 'banned';

/** 编辑器模式 */
export type EditorMode = 'wysiwyg' | 'markdown' | 'raw';

/** OKF 知识资产 */
export interface KnowledgeAsset {
    id: string;
    tree_node_id: string;
    title: string;
    content: string | null;
    content_html: string | null;
    okf_type: OkfType;
    summary: string | null;
    source: string | null;
    confidence: number | null;
    status: KnowledgeStatus;
    effective_at: string | null;
    expire_at: string | null;
    relation_ids: string[] | null;
    tags: string[] | null;
    file_url: string | null;
    file_name: string | null;
    file_size: number | null;
    file_mime: string | null;
    file_md5: string | null;
    editor_mode: EditorMode;
    created_by: string | null;
    created_at: string | null;
    updated_by: string | null;
    updated_at: string | null;
}

// ======================== API 方法 ========================

/** 根据 tree_node_id 获取关联的知识资产 */
export function getKnowledgeAssetByTreeNode(treeNodeId: string) {
    return api.get<KnowledgeAsset>('get_knowledge_asset_by_tree_node', { treeNodeId });
}

/** 根据 id 获取单条知识资产 */
export function getKnowledgeAsset(id: string) {
    return api.get<KnowledgeAsset>('get_knowledge_asset', { id });
}

/** 获取知识资产列表 */
export function listKnowledgeAssets(okfType?: OkfType) {
    return api.get<KnowledgeAsset[]>('list_knowledge_assets', okfType ? { okfType } : undefined);
}

/** 创建知识资产 */
export function createKnowledgeAsset(params: {
    treeNodeId: string;
    title: string;
    okfType: OkfType;
    content?: string;
    summary?: string;
    source?: string;
    tags?: string[];
}) {
    return api.post<KnowledgeAsset>('create_knowledge_asset', params);
}

/** 更新知识资产 */
export function updateKnowledgeAsset(params: {
    id: string;
    title?: string;
    content?: string;
    okfType?: OkfType;
    summary?: string;
    source?: string;
    status?: KnowledgeStatus;
    tags?: string[];
}) {
    return api.put<KnowledgeAsset>('update_knowledge_asset', params);
}

/** 删除知识资产（软删除） */
export function deleteKnowledgeAsset(id: string) {
    return api.delete<null>('delete_knowledge_asset', { id });
}

/** 将文件绑定到知识资产（上传完成后调用） */
export function attachFileToKnowledge(params: {
    assetId: string;
    fileUrl: string;
    fileName: string;
    fileSize: number;
    fileMime: string;
    fileMd5: string;
}) {
    return api.post<KnowledgeAsset>('attach_file_to_knowledge', params);
}