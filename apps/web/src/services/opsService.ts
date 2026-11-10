/**
 * 运维采集 API Service
 *
 * 封装 ops_connection / asset_ops_link 相关的 Tauri 命令调用。
 * 命令名与后端 commands::ops_commands 逐字一致；HTTP 模式下由 httpAdapter 映射到 /api/ops/**。
 */

import { api } from '@/utils/api';

// ======================== 类型定义 ========================

export type OpsProvider = 'docker' | 'swarm' | 'k8s';

/** 认证方式载荷（判别联合，tag 与后端 AuthKind::describe() 一致） */
export type OpsAuthPayload =
    | { type: 'local_socket' }
    | { type: 'plain_tcp' }
    | { type: 'mutual_tls'; ca_path: string; cert_path: string; key_path: string }
    | { type: 'mutual_tls_inline'; ca_pem: string; cert_pem: string; key_pem: string }
    | { type: 'kubeconfig_file'; path: string; context?: string | null }
    | { type: 'kubeconfig_inline'; yaml: string; context?: string | null }
    | {
          type: 'direct';
          token?: string | null;
          ca_pem?: string | null;
          client_cert_pem?: string | null;
          client_key_pem?: string | null;
          insecure_skip_tls_verify?: boolean;
      };

/** 新建 / 更新连接的入参；`auth` 传 null 表示沿用已存凭证（仅更新时有效） */
export interface OpsConnectionInput {
    name: string;
    provider: OpsProvider;
    endpoint: string;
    auth?: OpsAuthPayload | null;
    namespace?: string | null;
    timeout_secs?: number;
    docker_api_version?: string | null;
    max_items?: number;
    poll_interval_secs?: number;
    enabled?: boolean;
}

/** 连接视图（不含任何凭证明文） */
export interface OpsConnectionView {
    id: string;
    name: string;
    provider: string;
    provider_label: string;
    endpoint: string;
    auth_type: string;
    has_credential: boolean;
    credential_hint: string | null;
    namespace: string | null;
    timeout_secs: number;
    docker_api_version: string | null;
    max_items: number;
    poll_interval_secs: number;
    enabled: boolean;
    tier: string;
    risk_hints: string[];
    last_collect_at: string | null;
    last_error: string | null;
    created_at: string | null;
    updated_at: string | null;
}

/** 连通性探测结果 */
export interface OpsHealthView {
    ok: boolean;
    provider: string;
    provider_label: string;
    endpoint: string;
    tier: string;
    latency_ms: number;
    server_version: string | null;
    api_version: string | null;
    authenticated: boolean;
    authorized: boolean;
    warnings: string[];
    error: string | null;
    failure_class: string;
    failure_hint: string;
    summary: string;
}

export interface OpsKindCount {
    kind: string;
    count: number;
}

/** 一次手动采集的结果 */
export interface OpsCollectResult {
    connection_id: string;
    provider: string;
    endpoint: string;
    collected_at: string;
    elapsed_ms: number;
    total: number;
    short_lived: number;
    created: number;
    updated: number;
    gone: number;
    counts_by_kind: OpsKindCount[];
    partial_errors: string[];
}

/** 资源清单行（asset_no 来自台账 LEFT JOIN） */
export interface OpsResourceLinkView {
    id: string;
    connection_id: string;
    connection_name: string | null;
    provider: string;
    resource_uid: string;
    resource_kind: string;
    display_name: string | null;
    asset_id: string | null;
    asset_no: string | null;
    status_snapshot: string | null;
    raw_state: string | null;
    parent_uid: string | null;
    attributes: Record<string, unknown> | null;
    first_seen_at: string | null;
    last_seen_at: string | null;
}

export interface OpsAuthTypeOption {
    value: string;
    label: string;
    requires_credential: boolean;
    hint: string | null;
}

export interface OpsProviderOption {
    value: string;
    label: string;
    auth_types: OpsAuthTypeOption[];
}

// ======================== 服务方法 ========================

/** 来源 / 认证方式下拉数据 */
export function getOpsProviders(): Promise<OpsProviderOption[]> {
    return api.get<OpsProviderOption[]>('get_ops_providers');
}

/** 连接列表 */
export function getOpsConnections(): Promise<OpsConnectionView[]> {
    return api.get<OpsConnectionView[]>('get_ops_connections');
}

/** 单条连接 */
export function getOpsConnection(id: string): Promise<OpsConnectionView> {
    return api.get<OpsConnectionView>('get_ops_connection', { id });
}

/** 新建连接 */
export function createOpsConnection(input: OpsConnectionInput, currentUserId?: string) {
    return api.post<OpsConnectionView>('create_ops_connection', { input, currentUserId });
}

/** 更新连接（不传 auth 表示沿用已存凭证） */
export function updateOpsConnection(id: string, input: OpsConnectionInput, currentUserId?: string) {
    return api.put<OpsConnectionView>('update_ops_connection', { id, input, currentUserId });
}

/** 删除连接（软删） */
export function deleteOpsConnection(id: string): Promise<void> {
    return api.delete<void>('delete_ops_connection', { id });
}

/** 草稿探测（不落库） */
export function testOpsConnectionDraft(input: OpsConnectionInput): Promise<OpsHealthView> {
    return api.post<OpsHealthView>('test_ops_connection_draft', { input });
}

/** 已存连接探测 */
export function testOpsConnection(id: string): Promise<OpsHealthView> {
    return api.post<OpsHealthView>('test_ops_connection', { id });
}

/** 立即采集一次 */
export function collectOpsResources(id: string): Promise<OpsCollectResult> {
    return api.post<OpsCollectResult>('collect_ops_resources', { id });
}

/** 资源清单（可按连接过滤） */
export function getOpsLinks(connectionId?: string | null): Promise<OpsResourceLinkView[]> {
    return connectionId
        ? api.get<OpsResourceLinkView[]>('get_ops_links', { connectionId })
        : api.get<OpsResourceLinkView[]>('get_ops_links');
}

/** 把观测资源归档到已有资产 */
export function bindOpsLink(linkId: string, assetId: string, currentUserId?: string) {
    return api.post<OpsResourceLinkView>('bind_ops_link', { linkId, assetId, currentUserId });
}
