/**
 * 对话系统 API Service
 *
 * 智能问答的多轮对话管理。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

/** 聊天附件（多模态输入） */
export interface ChatAttachment {
    /** 附件类型：image / video / audio / document */
    type: 'image' | 'video' | 'audio' | 'document';
    /** 文件名 */
    name: string;
    /** 图片的 base64 data URL（type=image 时） */
    dataUrl?: string;
    /** S3 文件 URL（video/audio/document 时） */
    url?: string;
    /** MIME 类型 */
    mime?: string;
}

export interface ConversationResponse {
    convId: string;
    answer: string;
    citedAssets: AssetInfo[];
    usage: TokenUsage;
}

export interface ConversationListResponse {
    items: ConversationSummary[];
    total: string;
    page: number;
    pageSize: number;
}

export interface ConversationSummary {
    id: string;
    title: string;
    bindKnowledgeTreeId: string | null;
    created_at: string;
    updated_at: string;
}

export interface MessageResponse {
    id: string;
    role: 'user' | 'assistant';
    content: string;
    citedAssets?: AssetInfo[];
    referenceText?: string;
    metadata?: {
        model?: string;
        durationMs?: number;
        /** 多模态附件（历史消息回看渲染） */
        attachments?: ChatAttachment[];
    };
    createdAt: string;
}

export interface AssetInfo {
    id: string;
    title: string;
    okfType: string;
}

export interface TokenUsage {
    inputTokens: number;
    outputTokens: number;
    totalTokens: number;
    cost: number;
}

// ======================== 后端载荷（snake_case → 视图模型） ========================

interface RawAssetInfo {
    id: string;
    title: string;
    okf_type: string;
}

interface RawConversationResponse {
    conv_id: string;
    answer: string;
    cited_assets: RawAssetInfo[];
    usage: {
        input_tokens: number;
        output_tokens: number;
        total_tokens: number;
        cost: number;
    };
}

interface RawConversationSummary {
    id: string;
    title: string | null;
    bind_knowledge_tree_id: string | null;
    created_at: string | null;
    updated_at: string | null;
}

interface RawConversationListResponse {
    items: RawConversationSummary[];
    total: string;
    page: number;
    pageSize: number;
}

interface RawMessage {
    id: string;
    conv_id: string;
    role: string;
    content: string;
    reference_text: string | null;
    metadata: MessageResponse['metadata'] | null;
    created_at: string | null;
}

function toAssetInfo(raw: RawAssetInfo): AssetInfo {
    return { id: raw.id, title: raw.title, okfType: raw.okf_type };
}

function toConversationResponse(raw: RawConversationResponse): ConversationResponse {
    return {
        convId: raw.conv_id,
        answer: raw.answer,
        citedAssets: (raw.cited_assets ?? []).map(toAssetInfo),
        usage: {
            inputTokens: raw.usage?.input_tokens ?? 0,
            outputTokens: raw.usage?.output_tokens ?? 0,
            totalTokens: raw.usage?.total_tokens ?? 0,
            cost: raw.usage?.cost ?? 0,
        },
    };
}

function toMessageResponse(raw: RawMessage): MessageResponse {
    return {
        id: raw.id,
        role: raw.role as 'user' | 'assistant',
        content: raw.content,
        referenceText: raw.reference_text ?? undefined,
        metadata: raw.metadata ?? undefined,
        createdAt: raw.created_at ?? '',
    };
}

// ======================== API 方法 ========================

/**
 * 创建新会话
 *
 * 请求体字段与后端 `CreateConversationRequest` 一致（`modelName` 而非 `modelId`）。
 */
export async function createConversation(params: {
    userId: string;
    question: string;
    bindTreeNodeId?: string;
    providerId?: string;
    /** 模型编码（对应后端 `model_name`） */
    modelName?: string;
    /** 多模态附件列表 */
    attachments?: ChatAttachment[];
}): Promise<ConversationResponse> {
    const raw = await api.post<RawConversationResponse>('create_conversation', params);
    return toConversationResponse(raw);
}

/** 继续会话（`convId` 走路径，请求体不含该字段） */
export async function sendMessage(params: {
    convId: string;
    userId: string;
    question: string;
    providerId?: string;
    /** 模型编码（对应后端 `model_name`） */
    modelName?: string;
    /** 多模态附件列表 */
    attachments?: ChatAttachment[];
}): Promise<ConversationResponse> {
    const raw = await api.post<RawConversationResponse>('send_message', params);
    return toConversationResponse(raw);
}

/** 获取会话列表（后端 query 参数为 userId / page / pageSize） */
export async function getConversations(params: {
    userId: string;
    page?: number;
    pageSize?: number;
}): Promise<ConversationListResponse> {
    const raw = await api.get<RawConversationListResponse>('get_conversations', params);
    return {
        items: (raw.items ?? []).map((item) => ({
            id: item.id,
            title: item.title ?? '',
            bindKnowledgeTreeId: item.bind_knowledge_tree_id ?? null,
            created_at: item.created_at ?? '',
            updated_at: item.updated_at ?? '',
        })),
        total: raw.total,
        page: raw.page,
        pageSize: raw.pageSize,
    };
}

/** 获取会话消息（`convId` 走路径，page/pageSize 走 query） */
export async function getConversationMessages(params: {
    convId: string;
    page?: number;
    pageSize?: number;
}): Promise<MessageResponse[]> {
    const raw = await api.get<RawMessage[]>('get_conversation_messages', params);
    return (raw ?? []).map(toMessageResponse);
}

/** 更新会话标题（`convId` 走路径，请求体仅 `title`） */
export function updateConversationTitle(params: {
    convId: string;
    title: string;
}): Promise<void> {
    return api.put('update_conversation_title', params);
}

/** 删除会话（`convId` 走路径） */
export function deleteConversation(convId: string): Promise<void> {
    return api.delete('delete_conversation', { convId });
}