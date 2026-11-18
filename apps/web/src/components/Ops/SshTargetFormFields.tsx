'use client';

import React, { useState } from 'react';
import {
    Alert,
    Button,
    Divider,
    Group,
    NumberInput,
    PasswordInput,
    SegmentedControl,
    Stack,
    Switch,
    Text,
    Textarea,
    TextInput,
} from '@mantine/core';
import { IconAlertTriangle, IconScan } from '@tabler/icons-react';
import { modals } from '@mantine/modals';
import { notifications } from '@mantine/notifications';
import AssetSelect from '@/components/AssetSelect';
import {
    probeDraftSshTarget,
    type SshAuthType,
    type SshTargetInput,
    type SshTargetView,
} from '@/services/sshService';

export interface SshFormState {
    name: string;
    host: string;
    port: number;
    username: string;
    authType: SshAuthType;
    /** 编辑时沿用已存凭证（不提交 password / private_key_pem） */
    keepCredential: boolean;
    password: string;
    privateKeyPem: string;
    passphrase: string;
    strictHostKeyCheck: boolean;
    hostKeyFingerprint: string;
    assetId: string;
    description: string;
    connectTimeoutSecs: number;
    enabled: boolean;
}

export function createEmptyForm(): SshFormState {
    return {
        name: '',
        host: '',
        port: 22,
        username: 'root',
        authType: 'password',
        keepCredential: false,
        password: '',
        privateKeyPem: '',
        passphrase: '',
        strictHostKeyCheck: true,
        hostKeyFingerprint: '',
        assetId: '',
        description: '',
        connectTimeoutSecs: 15,
        enabled: true,
    };
}

/** 主机视图 → 表单（凭证不回显，编辑时默认沿用） */
export function viewToForm(view: SshTargetView): SshFormState {
    return {
        ...createEmptyForm(),
        name: view.name,
        host: view.host,
        port: view.port,
        username: view.username,
        authType: (view.auth_type === 'private_key' ? 'private_key' : 'password') as SshAuthType,
        keepCredential: view.has_credential,
        strictHostKeyCheck: view.strict_host_key_check,
        hostKeyFingerprint: view.host_key_fingerprint ?? '',
        assetId: view.asset_id ?? '',
        description: view.description ?? '',
        connectTimeoutSecs: view.connect_timeout_secs,
        enabled: view.enabled,
    };
}

/** 表单 → 接口入参（沿用凭证时省略凭证字段，后端保留原值） */
export function formToInput(form: SshFormState): SshTargetInput {
    const input: SshTargetInput = {
        name: form.name.trim(),
        host: form.host.trim(),
        port: form.port,
        username: form.username.trim(),
        auth_type: form.authType,
        strict_host_key_check: form.strictHostKeyCheck,
        host_key_fingerprint: form.hostKeyFingerprint.trim() || null,
        asset_id: form.assetId || null,
        description: form.description.trim() || null,
        connect_timeout_secs: form.connectTimeoutSecs,
        enabled: form.enabled,
    };
    if (!form.keepCredential) {
        if (form.authType === 'password') {
            input.password = form.password;
            input.private_key_pem = null;
            input.passphrase = null;
        } else {
            input.private_key_pem = form.privateKeyPem;
            input.passphrase = form.passphrase.trim() || null;
            input.password = null;
        }
    }
    return input;
}

interface Props {
    value: SshFormState;
    onChange: (next: SshFormState) => void;
    /** 编辑态：展示「沿用已存凭证」开关 */
    editing: boolean;
}

export default function SshTargetFormFields({ value, onChange, editing }: Props) {
    const [scanning, setScanning] = useState(false);

    const set = (patch: Partial<SshFormState>) => onChange({ ...value, ...patch });

    const canScan = value.host.trim().length > 0 && value.username.trim().length > 0;

    /** 用当前表单（未保存）的凭证扫描主机指纹 */
    const scanFingerprint = async () => {
        if (!canScan) return;
        setScanning(true);
        try {
            const result = await probeDraftSshTarget({
                host: value.host.trim(),
                port: value.port,
                username: value.username.trim(),
                auth_type: value.authType,
                password: value.keepCredential ? null : value.password,
                private_key_pem: value.keepCredential ? null : value.privateKeyPem,
                passphrase: value.keepCredential ? null : value.passphrase.trim() || null,
                connect_timeout_secs: value.connectTimeoutSecs,
            });
            const previous = value.hostKeyFingerprint.trim();
            notifications.show({
                color: result.matched === false ? 'orange' : 'teal',
                title: '指纹扫描完成',
                message: `算法 ${result.key_type}，耗时 ${result.latency_ms}ms`,
            });
            if (previous && previous !== result.fingerprint) {
                // 指纹变化必须人工确认，避免静默接受中间人
                modals.openConfirmModal({
                    title: '主机指纹已变化',
                    children: (
                        <Text size="sm">
                            已记录：{previous}
                            <br />
                            本次扫描：{result.fingerprint}
                            <br />
                            确认替换？指纹变化可能来自重装系统，也可能是中间人攻击。
                        </Text>
                    ),
                    labels: { confirm: '替换指纹', cancel: '保留原值' },
                    confirmProps: { color: 'red' },
                    onConfirm: () => set({ hostKeyFingerprint: result.fingerprint }),
                });
            } else {
                set({ hostKeyFingerprint: result.fingerprint });
            }
        } catch (err) {
            notifications.show({
                color: 'red',
                title: '指纹扫描失败',
                message: err instanceof Error ? err.message : '未知错误',
            });
        } finally {
            setScanning(false);
        }
    };

    return (
        <Stack gap="sm">
            <TextInput
                label="名称"
                required
                value={value.name}
                placeholder="如 生产 Web 节点"
                onChange={(event) => set({ name: event.currentTarget.value })}
            />
            <Group grow align="flex-end">
                <TextInput
                    label="主机地址"
                    required
                    value={value.host}
                    placeholder="10.0.0.12 或 host.example.com"
                    onChange={(event) => set({ host: event.currentTarget.value })}
                />
                <NumberInput
                    label="端口"
                    min={1}
                    max={65535}
                    value={value.port}
                    onChange={(next) => set({ port: Number(next) || 22 })}
                />
            </Group>
            <TextInput
                label="登录用户"
                required
                value={value.username}
                placeholder="root"
                onChange={(event) => set({ username: event.currentTarget.value })}
            />

            <Divider label="认证方式" labelPosition="left" />
            <SegmentedControl
                fullWidth
                value={value.authType}
                data={[
                    { value: 'password', label: '密码' },
                    { value: 'private_key', label: '私钥' },
                ]}
                onChange={(next) => set({ authType: next as SshAuthType })}
            />
            {editing && value.keepCredential && (
                <Switch
                    label="沿用已保存的凭证（关闭后可重新填写）"
                    checked={value.keepCredential}
                    onChange={(event) => set({ keepCredential: event.currentTarget.checked })}
                />
            )}
            {(!editing || !value.keepCredential) && value.authType === 'password' && (
                <PasswordInput
                    label="密码"
                    required
                    value={value.password}
                    onChange={(event) => set({ password: event.currentTarget.value })}
                />
            )}
            {(!editing || !value.keepCredential) && value.authType === 'private_key' && (
                <>
                    <Textarea
                        label="私钥 PEM"
                        required
                        minRows={4}
                        autosize={false}
                        className="ops-pem"
                        placeholder="-----BEGIN OPENSSH PRIVATE KEY-----"
                        value={value.privateKeyPem}
                        onChange={(event) => set({ privateKeyPem: event.currentTarget.value })}
                    />
                    <PasswordInput
                        label="私钥口令（无口令留空）"
                        value={value.passphrase}
                        onChange={(event) => set({ passphrase: event.currentTarget.value })}
                    />
                </>
            )}

            <Divider label="主机指纹" labelPosition="left" />
            <Switch
                label="严格校验主机指纹（拒绝未记录指纹的主机）"
                checked={value.strictHostKeyCheck}
                onChange={(event) => set({ strictHostKeyCheck: event.currentTarget.checked })}
            />
            <Group align="flex-end" grow>
                <TextInput
                    label="主机指纹（SHA256）"
                    value={value.hostKeyFingerprint}
                    placeholder="SHA256:xxxxxxxx"
                    onChange={(event) => set({ hostKeyFingerprint: event.currentTarget.value })}
                />
                <Button
                    variant="light"
                    leftSection={<IconScan size={16} />}
                    loading={scanning}
                    disabled={!canScan}
                    onClick={scanFingerprint}
                >
                    扫描指纹
                </Button>
            </Group>
            {value.strictHostKeyCheck && !value.hostKeyFingerprint.trim() && (
                <Alert color="orange" icon={<IconAlertTriangle size={16} />}>
                    严格模式下指纹为空将拒绝连接，请先扫描或手动填写。
                </Alert>
            )}

            <Divider label="关联与备注" labelPosition="left" />
            <AssetSelect
                mode="single"
                label="绑定资产（可选）"
                value={value.assetId || null}
                onChange={(id) => set({ assetId: id ?? '' })}
            />
            <Textarea
                label="描述"
                minRows={2}
                autosize
                value={value.description}
                onChange={(event) => set({ description: event.currentTarget.value })}
            />
            <NumberInput
                label="连接超时（秒）"
                min={5}
                max={120}
                value={value.connectTimeoutSecs}
                onChange={(next) => set({ connectTimeoutSecs: Number(next) || 15 })}
            />
            <Switch
                label="启用该主机"
                checked={value.enabled}
                onChange={(event) => set({ enabled: event.currentTarget.checked })}
            />
        </Stack>
    );
}
