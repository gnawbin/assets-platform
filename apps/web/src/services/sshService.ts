/**
 * 运维终端（SSH）API Service
 *
 * 服务方法名即 httpAdapter 的命令名（`COMMAND_ROUTE_MAP` 已登记），
 * HTTP 模式下映射到 `/api/ops/ssh/**`；WS 通道走 `buildSshWsUrl`。
 */

import { api } from '@/utils/api';
import { getApiOrigin } from '@/utils/apiBase';

// ======================== 类型定义 ========================

export type SshAuthType = 'password' | 'private_key';

/** 新建 / 编辑主机的入参（凭证字段缺省 = 沿用已存凭证） */
export interface SshTargetInput {
    name: string;
    host: string;
    port?: number | null;
    username: string;
    auth_type?: SshAuthType | null;
    /** 密码（仅 auth_type=password） */
    password?: string | null;
    /** 私钥 PEM（仅 auth_type=private_key） */
    private_key_pem?: string | null;
    /** 私钥口令（可选） */
    passphrase?: string | null;
    strict_host_key_check?: boolean | null;
    host_key_fingerprint?: string | null;
    asset_id?: string | null;
    description?: string | null;
    connect_timeout_secs?: number | null;
    enabled?: boolean | null;
}

/** 表单未保存时的指纹探测入参（凭证不落库） */
export interface SshProbeInput {
    host: string;
    port?: number | null;
    username: string;
    auth_type?: SshAuthType | null;
    password?: string | null;
    private_key_pem?: string | null;
    passphrase?: string | null;
    connect_timeout_secs?: number | null;
}

/** 主机视图（后端保证不含任何凭证） */
export interface SshTargetView {
    id: string;
    name: string;
    host: string;
    port: number;
    username: string;
    auth_type: SshAuthType | string;
    has_credential: boolean;
    credential_hint: string | null;
    host_key_fingerprint: string | null;
    strict_host_key_check: boolean;
    asset_id: string | null;
    description: string | null;
    connect_timeout_secs: number;
    enabled: boolean;
    last_connected_at: string | null;
    last_error: string | null;
    created_at: string | null;
    updated_at: string | null;
}

/** 指纹探测结果 */
export interface SshProbeView {
    fingerprint: string;
    key_type: string;
    /** 与已记录指纹是否一致；null 表示此前没有记录 */
    matched: boolean | null;
    latency_ms: number;
}

/** 会话视图（审计口径） */
export interface SshSessionView {
    id: string;
    target_id: string;
    target_name: string;
    user_id: string;
    state: string;
    client_ip: string | null;
    started_at: string | null;
    ended_at: string | null;
    bytes_in: number;
    bytes_out: number;
    exit_code: number | null;
    error: string | null;
}

/** 开会话响应（票据用于随后的 WS 升级） */
export interface SshSessionTicket {
    session_id: string;
    ticket: string;
    expires_in: number;
}

// ======================== 服务方法 ========================

/** 主机列表 */
export function getSshTargets(): Promise<SshTargetView[]> {
    return api.get<SshTargetView[]>('get_ssh_targets');
}

/** 单台主机 */
export function getSshTarget(id: string): Promise<SshTargetView> {
    return api.get<SshTargetView>('get_ssh_target', { id });
}

/** 新建主机 */
export function createSshTarget(input: SshTargetInput) {
    return api.post<SshTargetView>('create_ssh_target', { ...input });
}

/** 编辑主机（不传密码 / 私钥表示沿用已存凭证） */
export function updateSshTarget(id: string, input: SshTargetInput) {
    return api.put<SshTargetView>('update_ssh_target', { ...input, id });
}

/** 删除主机（软删；存在活动会话时后端返回 400） */
export function deleteSshTarget(id: string): Promise<void> {
    return api.delete<void>('delete_ssh_target', { id });
}

/** 用已保存凭证扫描主机指纹（不认证、不落库） */
export function probeSshTarget(id: string): Promise<SshProbeView> {
    return api.post<SshProbeView>('probe_ssh_target', { id });
}

/** 表单未保存时扫描指纹 */
export function probeDraftSshTarget(input: SshProbeInput): Promise<SshProbeView> {
    return api.post<SshProbeView>('probe_draft_ssh_target', { ...input });
}

/** 活跃会话列表 */
export function getSshSessions(): Promise<SshSessionView[]> {
    return api.get<SshSessionView[]>('get_ssh_sessions');
}

/** 打开会话，返回 `sessionId + ticket` */
export function openSshSession(targetId: string, cols: number, rows: number) {
    return api.post<SshSessionTicket>('open_ssh_session', { target_id: targetId, cols, rows });
}

/** 关闭会话（幂等） */
export function closeSshSession(sessionId: string): Promise<void> {
    return api.delete<void>('close_ssh_session', { sessionId });
}

/**
 * 拼装 WS 地址（票据走查询串，绝不把 JWT 放 URL）
 *
 * `getApiOrigin()` 在未配置服务端时返回 `null`，此时回落到当前页面 origin。
 */
export function buildSshWsUrl(sessionId: string, ticket: string): string {
    const origin = getApiOrigin() ?? window.location.origin;
    const wsOrigin = origin.replace(/^http/, 'ws');
    return `${wsOrigin}/api/ops/ssh/sessions/${encodeURIComponent(sessionId)}/ws?ticket=${encodeURIComponent(ticket)}`;
}
