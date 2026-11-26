/**
 * 运维采集 API Service
 *
 * 封装 ops_connection / asset_ops_link 相关的 Tauri 命令调用。
 * 命令名与后端 commands::ops_commands 逐字一致；HTTP 模式下由 httpAdapter 映射到 /api/ops/**。
 */

import { api } from '@/utils/api';
import { getApiOrigin } from '@/utils/apiBase';

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
    /** 是否允许容器终端（默认 false） */
    exec_enabled?: boolean;
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
    /** 是否允许容器终端（默认关闭） */
    exec_enabled: boolean;
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

// ======================== 容器日志 / 性能 / 终端 ========================

export interface OpsContainerLogLine {
    stream: 'stdout' | 'stderr' | 'stdin' | 'console' | string;
    message: string;
}

export interface OpsContainerLogsView {
    container: string;
    lines: OpsContainerLogLine[];
    bytes: number;
    truncated: boolean;
}

export interface OpsContainerStatsView {
    container: string;
    read_at_ms: number;
    cpu_percent: number | null;
    mem_used_bytes: number;
    mem_limit_bytes: number;
    mem_percent: number;
    net_rx_bytes: number;
    net_tx_bytes: number;
    pids: number | null;
    blkio_read_bytes: number | null;
    blkio_write_bytes: number | null;
}

export interface OpsPodLogsView {
    namespace: string;
    pod: string;
    container: string | null;
    containers: string[];
    previous: boolean;
    lines: string[];
    bytes: number;
    truncated: boolean;
}

export interface OpsPodContainerMetrics {
    name: string;
    cpu_nanocores: number;
    memory_bytes: number;
}

export interface OpsPodMetricsView {
    namespace: string;
    pod: string;
    containers: string[];
    window: string | null;
    timestamp: string | null;
    container_metrics: OpsPodContainerMetrics[];
    cpu_nanocores: number;
    memory_bytes: number;
}

/** 终端目标（Docker 与 K8s 共用一套会话 / WS） */
export type OpsTerminalTarget =
    | { kind: 'docker'; container: string }
    | { kind: 'k8s'; namespace: string; pod: string; container?: string | null };

/** 开会话响应（票据用于随后的 WS 升级） */
export interface OpsTerminalSessionTicket {
    session_id: string;
    ticket: string;
    expires_in: number;
}

/** 从 resource_uid 解析出的目标 */
export type OpsResourceTarget =
    | { provider: 'docker'; endpoint: string; containerId: string }
    | { provider: 'k8s'; endpoint: string; namespace: string; pod: string };

/**
 * 解析 `resource_uid`（规则与后端 `ResourceKey::uid()` 完全一致）
 *
 * - `docker://<endpoint>/container/<containerId>`
 * - `k8s://<endpoint>/pod/<namespace>/<pod>`
 *
 * endpoint 已去 scheme（如 `local` / `10.0.0.5:2375`）。
 */
export function parseOpsResourceTarget(uid: string): OpsResourceTarget | null {
    const docker = /^docker:\/\/([^/]+)\/container\/(.+)$/.exec(uid);
    if (docker) {
        return { provider: 'docker', endpoint: docker[1], containerId: docker[2] };
    }
    const k8s = /^k8s:\/\/([^/]+)\/pod\/([^/]+)\/(.+)$/.exec(uid);
    if (k8s) {
        return { provider: 'k8s', endpoint: k8s[1], namespace: k8s[2], pod: k8s[3] };
    }
    return null;
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

/** 新建连接（后端 DTO 为扁平 snake_case，创建人取自登录态 token） */
export function createOpsConnection(input: OpsConnectionInput) {
    return api.post<OpsConnectionView>('create_ops_connection', { ...input });
}

/** 更新连接（id 进路径；不传 auth 表示沿用已存凭证） */
export function updateOpsConnection(id: string, input: OpsConnectionInput) {
    return api.put<OpsConnectionView>('update_ops_connection', { ...input, id });
}

/** 删除连接（软删） */
export function deleteOpsConnection(id: string): Promise<void> {
    return api.delete<void>('delete_ops_connection', { id });
}

/** 草稿探测（不落库；请求体即扁平输入） */
export function testOpsConnectionDraft(input: OpsConnectionInput): Promise<OpsHealthView> {
    return api.post<OpsHealthView>('test_ops_connection_draft', { ...input });
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

/** 把观测资源归档到已有资产（linkId 走路径，请求体仅 `asset_id`；操作人取自登录态 token） */
export function bindOpsLink(linkId: string, assetId: string) {
    return api.post<OpsResourceLinkView>('bind_ops_link', { linkId, asset_id: assetId });
}

// ======================== 容器日志 / 性能 ========================

/** Docker 容器日志 */
export function getOpsContainerLogs(
    connectionId: string,
    container: string,
    lines?: number,
    timestamps?: boolean,
): Promise<OpsContainerLogsView> {
    return api.get<OpsContainerLogsView>('get_ops_container_logs', {
        connectionId,
        container,
        lines,
        timestamps,
    });
}

/** Docker 容器性能 */
export function getOpsContainerStats(
    connectionId: string,
    container: string,
    twoSamples?: boolean,
): Promise<OpsContainerStatsView> {
    return api.get<OpsContainerStatsView>('get_ops_container_stats', {
        connectionId,
        container,
        twoSamples,
    });
}

/** K8s Pod 日志 */
export function getOpsPodLogs(
    connectionId: string,
    namespace: string,
    pod: string,
    container?: string | null,
    previous?: boolean,
    lines?: number,
    timestamps?: boolean,
): Promise<OpsPodLogsView> {
    return api.get<OpsPodLogsView>('get_ops_pod_logs', {
        connectionId,
        namespace,
        pod,
        container,
        previous,
        lines,
        timestamps,
    });
}

/** K8s Pod 指标 */
export function getOpsPodMetrics(
    connectionId: string,
    namespace: string,
    pod: string,
): Promise<OpsPodMetricsView> {
    return api.get<OpsPodMetricsView>('get_ops_pod_metrics', {
        connectionId,
        namespace,
        pod,
    });
}

// ======================== 容器终端 ========================

/** 打开终端会话，返回 `sessionId + ticket` */
export function openOpsTerminalSession(
    connectionId: string,
    target: OpsTerminalTarget,
    shell?: string | null,
    cols?: number,
    rows?: number,
): Promise<OpsTerminalSessionTicket> {
    return api.post<OpsTerminalSessionTicket>('open_ops_terminal_session', {
        connectionId,
        target,
        shell,
        cols,
        rows,
    });
}

/** 关闭会话（幂等） */
export function closeOpsTerminalSession(sessionId: string): Promise<void> {
    return api.delete<void>('close_ops_terminal_session', { sessionId });
}

/** 拼装 WS 地址（票据走查询串，绝不把 JWT 放 URL） */
export function buildOpsTerminalWsUrl(sessionId: string, ticket: string): string {
    const origin = getApiOrigin() ?? window.location.origin;
    const wsOrigin = origin.replace(/^http/, 'ws');
    return `${wsOrigin}/api/ops/terminal/sessions/${encodeURIComponent(sessionId)}/ws?ticket=${encodeURIComponent(ticket)}`;
}
