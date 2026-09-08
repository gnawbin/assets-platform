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

// ======================== API 方法 ========================

/** 创建新会话 */
export function createConversation(params: {
    userId: string;
    question: string;
    bindTreeNodeId?: string;
    providerId?: string;
    modelId?: string;
    /** 多模态附件列表 */
    attachments?: ChatAttachment[];
}): Promise<ConversationResponse> {
    return api.post('create_conversation', params);
}

/** 继续会话 */
export function sendMessage(params: {
    convId: string;
    userId: string;
    question: string;
    providerId?: string;
    modelId?: string;
    /** 多模态附件列表 */
    attachments?: ChatAttachment[];
}): Promise<ConversationResponse> {
    return api.post('send_message', params);
}

/** 获取会话列表 */
export function getConversations(params: {
    userId: string;
    page?: number;
    pageSize?: number;
}): Promise<ConversationListResponse> {
    return api.get('get_conversations', params);
}

/** 获取会话消息 */
export function getConversationMessages(params: {
    convId: string;
    page?: number;
    pageSize?: number;
}): Promise<MessageResponse[]> {
    return api.get('get_conversation_messages', params);
}

/** 更新会话标题 */
export function updateConversationTitle(params: {
    convId: string;
    title: string;
}): Promise<void> {
    return api.put('update_conversation_title', params);
}

/** 删除会话 */
export function deleteConversation(convId: string): Promise<void> {
    return api.delete('delete_conversation', { convId });
}