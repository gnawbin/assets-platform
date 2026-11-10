/**
 * API Key 管理服务（外部系统接入凭证）
 *
 * 对应后端 `crates/api/src/api_key_routes.rs`：
 * - `GET    /api/api-keys`              列表（不含明文与哈希）
 * - `POST   /api/api-keys`              创建（响应里的 `api_key` 明文**只出现一次**）
 * - `POST   /api/api-keys/{id}/rotate`  轮换（旧 key 24 小时后过期）
 * - `DELETE /api/api-keys/{id}`         吊销（立即生效）
 *
 * @remarks
 * 这些接口只存在于 HTTP 通道（`crates/api`），Tauri 桌面通道没有对应命令，
 * 因此页面在 `getAdapterType() !== 'http'` 时会给出提示（见 settings/api-keys 页）。
 * 请求体字段使用后端 serde 的 snake_case 命名。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

/** 列表项（不含明文与哈希） */
export interface ApiKeyView {
    id: string;
    name: string;
    description: string | null;
    /** 定位段（明文，用于展示与日志定位） */
    key_prefix: string;
    scopes: string[];
    rate_limit_per_min: number;
    expires_at: string | null;
    last_used_at: string | null;
    revoked_at: string | null;
    enable: boolean;
    created_at: string | null;
}

/** 创建 / 轮换结果（`api_key` 只返回这一次） */
export interface CreatedApiKey {
    id: string;
    api_key: string;
    key_prefix: string;
    scopes: string[];
    expires_at: string | null;
}

/** 创建参数 */
export interface CreateApiKeyParams {
    name: string;
    description?: string | null;
    /** 至少 1 个、最多 50 个，必须 ⊆ 创建者权限，禁止 `system:*` */
    scopes: string[];
    /** 有效天数（默认 90，最长 365） */
    expires_in_days?: number;
    /** 每分钟请求上限（默认 600） */
    rate_limit_per_min?: number;
}

/** scope 模板（降低配置错误，设计文档 §6.6） */
export interface ScopeTemplate {
    key: string;
    label: string;
    description: string;
    scopes: string[];
}

/** 预置 scope 模板 */
export const API_KEY_SCOPE_TEMPLATES: ScopeTemplate[] = [
    {
        key: 'readonly',
        label: '只读集成',
        description: 'ERP 拉取资产台账',
        scopes: [
            'asset:hardware:list',
            'asset:software:list',
            'asset:category:list',
            'statistics:assets:list',
        ],
    },
    {
        key: 'workflow',
        label: '工单写入',
        description: 'OA 提交领用 / 归还 / 维修单据',
        scopes: [
            'asset:hardware:list',
            'asset:software:list',
            'process:receive:add',
            'process:return:add',
            'process:repair:add',
        ],
    },
    {
        key: 'ops',
        label: '采集上报',
        description: '运维平台回写资源',
        scopes: ['ops:connection:collect', 'ops:link:bind', 'ops:resource:list'],
    },
];

// ======================== 服务方法 ========================

/** 获取 API Key 列表（超管看全部，其余只看本租户） */
export function getApiKeys() {
    return api.get<ApiKeyView[]>('get_api_keys');
}

/** 创建 API Key（返回一次性明文） */
export function createApiKey(params: CreateApiKeyParams) {
    return api.post<CreatedApiKey>('create_api_key', { ...params });
}

/** 轮换 API Key（旧 key 24 小时后过期） */
export function rotateApiKey(id: string, expiresInDays?: number) {
    return api.post<CreatedApiKey>('rotate_api_key', {
        id,
        expires_in_days: expiresInDays,
    });
}

/** 吊销 API Key（立即生效，行保留用于审计） */
export function revokeApiKey(id: string) {
    return api.delete<void>('revoke_api_key', { id });
}
