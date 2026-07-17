/**
 * LLM 厂商/模型 API Service
 */

import { api } from '@/utils/api';

export interface LlmProvider {
    id: string;
    provider_code: string;
    provider_name: string;
    base_url: string | null;
    api_key: string | null;
    weight: number | null;
    is_local: boolean;
    enable: boolean;
}

export interface LlmModel {
    id: string;
    provider_id: string;
    model_code: string;
    model_name: string;
    model_type: string;
    context_window: number | null;
    temperature_default: number | null;
    max_tokens_default: number | null;
    price_input: number | null;
    price_output: number | null;
    enable: boolean;
}

export interface UserLLmSetting {
    id: string;
    user_id: string;
    default_provider_id: string | null;
    default_chat_model_id: string | null;
    default_embed_model_id: string | null;
    custom_temp: number | null;
    custom_max_token: number | null;
}

/** 获取厂商列表 */
export function getLlmProviders(): Promise<LlmProvider[]> {
    return api.get('get_llm_providers');
}

/** 获取单个厂商 */
export function getLlmProvider(id: string): Promise<LlmProvider> {
    return api.get('get_llm_provider', { id });
}

/** 创建厂商 */
export function createLlmProvider(params: {
    providerCode: string;
    providerName: string;
    baseUrl?: string;
    apiKey?: string;
    weight?: number;
    isLocal?: boolean;
    createdBy?: string;
}): Promise<LlmProvider> {
    return api.post('create_llm_provider', params);
}

/** 更新厂商 */
export function updateLlmProvider(params: {
    id: string;
    providerCode?: string;
    providerName?: string;
    baseUrl?: string;
    apiKey?: string;
    weight?: number;
    enable?: boolean;
}): Promise<LlmProvider> {
    return api.put('update_llm_provider', params);
}

/** 删除厂商 */
export function deleteLlmProvider(id: string): Promise<void> {
    return api.delete('delete_llm_provider', { id });
}

/** 获取模型列表 */
export function getLlmModels(providerId?: string): Promise<LlmModel[]> {
    return api.get('get_llm_models', providerId ? { providerId } : undefined);
}

/** 获取用户模型偏好 */
export function getUserLLmSetting(userId: string): Promise<UserLLmSetting | null> {
    return api.get('get_user_llm_setting', { userId });
}

/** 保存用户模型偏好 */
export function saveUserLLmSetting(params: {
    userId: string;
    defaultProviderId?: string;
    defaultChatModelId?: string;
    defaultEmbedModelId?: string;
    customTemp?: number;
    customMaxToken?: number;
}): Promise<UserLLmSetting> {
    return api.post('save_user_llm_setting', params);
}