/**
 * OAuth 2.1 客户端管理服务（Phase 2）
 *
 * 对应后端 `crates/api/src/oauth_routes.rs` 的管理接口：
 * - `GET    /api/oauth-clients`       列表（不含密钥哈希）
 * - `POST   /api/oauth-clients`       创建（响应里的 `client_secret` 明文**只出现一次**）
 * - `PUT    /api/oauth-clients/{id}`  更新（`enable=false` 会同时撤销其全部令牌）
 * - `DELETE /api/oauth-clients/{id}`  删除（软删除 + 撤销令牌）
 *
 * @remarks
 * 这些接口只存在于 HTTP 通道，Tauri 桌面通道没有对应命令（设计文档 §16 T-9），
 * 页面在 `getAdapterType() !== 'http'` 时会给出提示。
 * 请求体字段使用后端 serde 的 snake_case 命名。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

/** 客户端类型：confidential（服务端应用，有密钥） */
export const OAUTH_CLIENT_TYPE_CONFIDENTIAL = 1;

/** 客户端类型：public（桌面 / SPA，无密钥，强制 PKCE） */
export const OAUTH_CLIENT_TYPE_PUBLIC = 2;

/** 列表项（不含密钥哈希） */
export interface OauthClientView {
    id: string;
    /** 公开的客户端标识（出现在 authorize / token 请求里） */
    client_id: string;
    client_name: string;
    /** 1 = confidential，2 = public */
    client_type: number;
    redirect_uris: string[];
    allowed_scopes: string[];
    grant_types: string[];
    /** 访问令牌有效期（秒） */
    access_token_ttl: number;
    /** 刷新令牌有效期（秒） */
    refresh_token_ttl: number;
    enable: boolean;
    created_at: string | null;
}

/** 创建结果（`client_secret` 只返回这一次，public client 为 null） */
export interface CreatedOauthClient {
    id: string;
    client_id: string;
    client_secret: string | null;
    client_name: string;
    client_type: number;
    redirect_uris: string[];
    allowed_scopes: string[];
    grant_types: string[];
    access_token_ttl: number;
    refresh_token_ttl: number;
}

/** 创建参数 */
export interface CreateOauthClientParams {
    client_name: string;
    /** 缺省 2（public，强制 PKCE） */
    client_type?: number;
    /** 至少 1 条；https，或 http + 回环地址（127.0.0.1 / ::1） */
    redirect_uris: string[];
    /** 必须 ⊆ 创建者权限，且不能包含 system:* / upload:* */
    allowed_scopes: string[];
    grant_types?: string[];
    access_token_ttl?: number;
    refresh_token_ttl?: number;
}

/** 更新参数（省略字段表示保持不变） */
export interface UpdateOauthClientParams {
    client_name?: string;
    redirect_uris?: string[];
    allowed_scopes?: string[];
    grant_types?: string[];
    access_token_ttl?: number;
    refresh_token_ttl?: number;
    /** false = 禁用（同时撤销该客户端全部令牌） */
    enable?: boolean;
}

/** 授权范围模板（降低配置错误） */
export interface OauthScopeTemplate {
    key: string;
    label: string;
    description: string;
    scopes: string[];
}

/** 预置 scope 模板（与 API Key 页保持一致的常用组合） */
export const OAUTH_SCOPE_TEMPLATES: OauthScopeTemplate[] = [
    {
        key: 'readonly',
        label: '只读集成',
        description: '第三方拉取资产台账',
        scopes: [
            'asset:hardware:list',
            'asset:software:list',
            'asset:category:list',
            'knowledge:list',
        ],
    },
    {
        key: 'workflow',
        label: '工单写入',
        description: '代表用户提交领用 / 归还 / 维修单据',
        scopes: [
            'asset:hardware:list',
            'asset:software:list',
            'process:receive:add',
            'process:return:add',
            'process:repair:add',
        ],
    },
    {
        key: 'chat',
        label: '智能问答',
        description: '调用 SSE 流式问答',
        scopes: ['chat:completion:add', 'knowledge:list'],
    },
];

// ======================== 服务方法 ========================

/** 获取 OAuth 客户端列表（超管看全部，其余只看本租户） */
export function getOauthClients() {
    return api.get<OauthClientView[]>('get_oauth_clients');
}

/** 创建 OAuth 客户端（返回一次性客户端密钥） */
export function createOauthClient(params: CreateOauthClientParams) {
    return api.post<CreatedOauthClient>('create_oauth_client', { ...params });
}

/** 更新 OAuth 客户端 */
export function updateOauthClient(id: string, params: UpdateOauthClientParams) {
    return api.put<OauthClientView>('update_oauth_client', { id, ...params });
}

/** 删除 OAuth 客户端（同时撤销其全部令牌） */
export function deleteOauthClient(id: string) {
    return api.delete<void>('delete_oauth_client', { id });
}
