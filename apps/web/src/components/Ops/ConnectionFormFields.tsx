'use client';

import React from 'react';
import {
    Alert,
    Divider,
    NumberInput,
    PasswordInput,
    Select,
    Stack,
    Switch,
    Text,
    Textarea,
    TextInput,
} from '@mantine/core';
import { IconAlertTriangle, IconLock } from '@tabler/icons-react';
import type {
    OpsAuthPayload,
    OpsConnectionInput,
    OpsConnectionView,
    OpsProvider,
    OpsProviderOption,
} from '@/services/opsService';

/** 凭据/配置可从库中沿用的认证方式（编辑时默认勾选「沿用」） */
const KEEPABLE_AUTH = ['mutual_tls', 'mutual_tls_inline', 'kubeconfig_file', 'kubeconfig_inline', 'direct'];

export interface OpsFormState {
    name: string;
    provider: OpsProvider;
    endpoint: string;
    authType: string;
    keepCredential: boolean;
    caPath: string;
    certPath: string;
    keyPath: string;
    caPem: string;
    certPem: string;
    keyPem: string;
    kubeconfigPath: string;
    kubeconfigYaml: string;
    kubeContext: string;
    token: string;
    clientCertPem: string;
    clientKeyPem: string;
    insecure: boolean;
    namespace: string;
    timeoutSecs: number;
    maxItems: number;
    pollIntervalSecs: number;
    enabled: boolean;
}

export function createEmptyForm(provider: OpsProvider = 'docker'): OpsFormState {
    return {
        name: '',
        provider,
        endpoint: provider === 'k8s' ? '' : 'tcp://',
        authType: provider === 'k8s' ? 'kubeconfig_inline' : 'mutual_tls_inline',
        keepCredential: false,
        caPath: '',
        certPath: '',
        keyPath: '',
        caPem: '',
        certPem: '',
        keyPem: '',
        kubeconfigPath: '',
        kubeconfigYaml: '',
        kubeContext: '',
        token: '',
        clientCertPem: '',
        clientKeyPem: '',
        insecure: false,
        namespace: '',
        timeoutSecs: 15,
        maxItems: 20000,
        pollIntervalSecs: 300,
        enabled: true,
    };
}

/** 连接视图 → 表单（凭证不回显，编辑时默认沿用） */
export function viewToForm(view: OpsConnectionView): OpsFormState {
    const base = createEmptyForm(view.provider as OpsProvider);
    return {
        ...base,
        name: view.name,
        endpoint: view.endpoint,
        authType: view.auth_type,
        keepCredential: KEEPABLE_AUTH.includes(view.auth_type),
        namespace: view.namespace ?? '',
        timeoutSecs: view.timeout_secs,
        maxItems: view.max_items,
        pollIntervalSecs: view.poll_interval_secs,
        enabled: view.enabled,
    };
}

/** 表单 → 认证载荷 */
function buildAuth(form: OpsFormState): OpsAuthPayload {
    switch (form.authType) {
        case 'plain_tcp':
            return { type: 'plain_tcp' };
        case 'mutual_tls':
            return {
                type: 'mutual_tls',
                ca_path: form.caPath.trim(),
                cert_path: form.certPath.trim(),
                key_path: form.keyPath.trim(),
            };
        case 'mutual_tls_inline':
            return {
                type: 'mutual_tls_inline',
                ca_pem: form.caPem,
                cert_pem: form.certPem,
                key_pem: form.keyPem,
            };
        case 'kubeconfig_file':
            return { type: 'kubeconfig_file', path: form.kubeconfigPath.trim(), context: form.kubeContext.trim() || null };
        case 'kubeconfig_inline':
            return { type: 'kubeconfig_inline', yaml: form.kubeconfigYaml, context: form.kubeContext.trim() || null };
        case 'direct':
            return {
                type: 'direct',
                token: form.token.trim() || null,
                ca_pem: form.caPem.trim() || null,
                client_cert_pem: form.clientCertPem.trim() || null,
                client_key_pem: form.clientKeyPem.trim() || null,
                insecure_skip_tls_verify: form.insecure,
            };
        default:
            return { type: 'local_socket' };
    }
}

/** 表单 → service 入参（沿用凭证时 auth 传 null） */
export function formToInput(form: OpsFormState): OpsConnectionInput {
    return {
        name: form.name.trim(),
        provider: form.provider,
        endpoint: form.endpoint.trim(),
        auth: form.keepCredential ? null : buildAuth(form),
        namespace: form.namespace.trim() || null,
        timeout_secs: form.timeoutSecs,
        max_items: form.maxItems,
        poll_interval_secs: form.pollIntervalSecs,
        enabled: form.enabled,
    };
}

interface Props {
    value: OpsFormState;
    onChange: (next: OpsFormState) => void;
    providerOptions: OpsProviderOption[];
    editing: boolean;
}

export default function ConnectionFormFields({ value, onChange, providerOptions, editing }: Props) {
    const set = (patch: Partial<OpsFormState>) => onChange({ ...value, ...patch });
    const provider = providerOptions.find((item) => item.value === value.provider);
    const authOptions = (provider?.auth_types ?? []).map((item) => ({ value: item.value, label: item.label }));
    const authHint = provider?.auth_types.find((item) => item.value === value.authType)?.hint ?? null;
    const isLocalSocket = value.authType === 'local_socket';
    const showCredential = !value.keepCredential;


    const handleProviderChange = (next: string | null) => {
        const code = (next ?? 'docker') as OpsProvider;
        const empty = createEmptyForm(code);
        set({ provider: code, authType: empty.authType, endpoint: empty.endpoint, keepCredential: false });
    };

    return (
        <Stack gap="sm">
            <TextInput label="连接名称" required value={value.name} placeholder="如 生产 Docker / 机房 A K8s"
                onChange={(event) => set({ name: event.currentTarget.value })} />
            <Select label="采集来源" required allowDeselect={false} value={value.provider}
                data={providerOptions.map((item) => ({ value: item.value, label: item.label }))}
                onChange={handleProviderChange} />
            <TextInput label="端点" disabled={isLocalSocket} value={value.endpoint}
                placeholder={isLocalSocket ? "本机 socket（无需填写）" : "https://10.0.0.5:2376"}
                description="可省略 scheme，按认证方式自动补 tcp:// 或 https://"
                onChange={(event) => set({ endpoint: event.currentTarget.value })} />
            <Select label="认证方式" required allowDeselect={false} value={value.authType}
                data={authOptions}
                onChange={(next) => set({ authType: next ?? value.authType, keepCredential: false })} />
            {authHint && <Text size="xs" c="dimmed">{authHint}</Text>}

            <Divider label="认证与凭证" labelPosition="left" />
            {editing && KEEPABLE_AUTH.includes(value.authType) && (
                <Switch label="沿用已保存的凭证 / 配置（不重新录入）" checked={value.keepCredential}
                    onChange={(event) => set({ keepCredential: event.currentTarget.checked })} />
            )}

            {value.authType === "plain_tcp" && (
                <Alert color="yellow" icon={<IconAlertTriangle size={16} />} title="明文 TCP">
                    未加密传输，凭证与数据可能被旁路嗅探，仅限隔离内网使用。
                </Alert>
            )}

            {showCredential && value.authType === "mutual_tls" && (
                <>
                    <TextInput label="CA 证书路径" value={value.caPath} placeholder="/etc/docker/ca.pem"
                        onChange={(event) => set({ caPath: event.currentTarget.value })} />
                    <TextInput label="客户端证书路径" value={value.certPath} placeholder="/etc/docker/cert.pem"
                        onChange={(event) => set({ certPath: event.currentTarget.value })} />
                    <TextInput label="客户端私钥路径" value={value.keyPath} placeholder="/etc/docker/key.pem"
                        onChange={(event) => set({ keyPath: event.currentTarget.value })} />
                </>
            )}

            {showCredential && value.authType === "mutual_tls_inline" && (
                <>
                    <Alert color="blue" icon={<IconLock size={16} />} title="证书加密保存">
                        PEM 原文将以 AES-256-GCM 加密落库，任何接口都不会回传。
                    </Alert>
                    <Textarea label="CA 证书 PEM" minRows={4} autosize={false} value={value.caPem}
                        placeholder="-----BEGIN CERTIFICATE-----" className="ops-pem"
                        onChange={(event) => set({ caPem: event.currentTarget.value })} />
                    <Textarea label="客户端证书 PEM" minRows={4} autosize={false} value={value.certPem}
                        placeholder="-----BEGIN CERTIFICATE-----" className="ops-pem"
                        onChange={(event) => set({ certPem: event.currentTarget.value })} />
                    <Textarea label="客户端私钥 PEM" minRows={4} autosize={false} value={value.keyPem}
                        placeholder="-----BEGIN PRIVATE KEY-----" className="ops-pem"
                        onChange={(event) => set({ keyPem: event.currentTarget.value })} />
                </>
            )}

            {showCredential && value.authType === "kubeconfig_file" && (
                <>
                    <TextInput label="kubeconfig 路径" value={value.kubeconfigPath} placeholder="/root/.kube/config"
                        onChange={(event) => set({ kubeconfigPath: event.currentTarget.value })} />
                    <TextInput label="context（可选）" value={value.kubeContext} placeholder="留空使用 current-context"
                        onChange={(event) => set({ kubeContext: event.currentTarget.value })} />
                </>
            )}

            {showCredential && value.authType === "kubeconfig_inline" && (
                <>
                    <Textarea label="kubeconfig YAML" minRows={6} autosize={false} value={value.kubeconfigYaml}
                        placeholder="apiVersion: v1" className="ops-pem"
                        onChange={(event) => set({ kubeconfigYaml: event.currentTarget.value })} />
                    <TextInput label="context（可选）" value={value.kubeContext} placeholder="留空使用 current-context"
                        onChange={(event) => set({ kubeContext: event.currentTarget.value })} />
                </>
            )}

            {showCredential && value.authType === "direct" && (
                <>
                    <PasswordInput label="SA Token（与客户端证书二选一或并存）" value={value.token}
                        onChange={(event) => set({ token: event.currentTarget.value })} />
                    <Textarea label="CA 证书 PEM（可留空）" minRows={3} autosize={false} value={value.caPem}
                        className="ops-pem" onChange={(event) => set({ caPem: event.currentTarget.value })} />
                    <Textarea label="客户端证书 PEM（可留空）" minRows={3} autosize={false} value={value.clientCertPem}
                        className="ops-pem" onChange={(event) => set({ clientCertPem: event.currentTarget.value })} />
                    <Textarea label="客户端私钥 PEM（可留空）" minRows={3} autosize={false} value={value.clientKeyPem}
                        className="ops-pem" onChange={(event) => set({ clientKeyPem: event.currentTarget.value })} />
                    <Switch label="跳过 TLS 证书校验（自签集群临时使用，存在中间人风险）" checked={value.insecure}
                        onChange={(event) => set({ insecure: event.currentTarget.checked })} />
                </>
            )}

            <Divider label="采集参数" labelPosition="left" />
            {value.provider === "k8s" && (
                <TextInput label="命名空间（可选）" value={value.namespace} placeholder="留空采集全部命名空间"
                    onChange={(event) => set({ namespace: event.currentTarget.value })} />
            )}
            <NumberInput label="连接超时（秒）" min={1} max={600} value={value.timeoutSecs}
                onChange={(next) => set({ timeoutSecs: Number(next) || 15 })} />
            <NumberInput label="单类资源上限" min={1} max={200000} value={value.maxItems}
                onChange={(next) => set({ maxItems: Number(next) || 20000 })} />
            <NumberInput label="轮询间隔（秒，调度器启用后生效）" min={30} max={86400} value={value.pollIntervalSecs}
                onChange={(next) => set({ pollIntervalSecs: Number(next) || 300 })} />
            <Switch label="启用该连接" checked={value.enabled}
                onChange={(event) => set({ enabled: event.currentTarget.checked })} />
        </Stack>
    );
}

