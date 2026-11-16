'use client';

/**
 * 服务端地址配置表单
 *
 * 两个使用场景，共用同一份逻辑，避免两处行为漂移：
 * - `mode="gate"`：`ServerConfigGate` 首屏阻断时使用（探活失败的兜底地址）
 * - `mode="page"`：`/settings/server` 诊断页使用
 *
 * 交互顺序刻意做成「先测后存」：
 * 1. 「测试连接」→ `GET {origin}/healthz`（免鉴权，未登录也能用）；
 * 2. 「保存并重载」→ `setApiBase()`（规范化 + 清本地会话）→ 重载页面。
 *
 * 安全口径：非回环 `http://` 会明文传令牌，`setApiBase` 默认拒绝，
 * 本表单捕获 `InsecureTransportError` 后要求用户**二次确认**才继续。
 */

import React, { useState } from 'react';
import {
    Alert,
    Badge,
    Button,
    Code,
    Group,
    Stack,
    Text,
    TextInput,
} from '@mantine/core';
import {
    IconAlertTriangle,
    IconCheck,
    IconDeviceFloppy,
    IconPlugConnected,
    IconRestore,
} from '@tabler/icons-react';
import {
    InsecureTransportError,
    clearApiBase,
    describeApiBase,
    isInsecureRemote,
    normalizeApiBase,
    probeServer,
    setApiBase,
    type ProbeResult,
} from '@/utils/apiBase';

export interface ServerConnectionFormProps {
    /** `gate` 用于首屏阻断，`page` 用于诊断页（仅影响按钮文案） */
    mode?: 'gate' | 'page';
    /** 保存成功后的回调；不传则整页重载（清空内存中的旧基址与登录态） */
    onSaved?: (base: string) => void;
}

/** 探活失败分类 → 可操作提示 */
function describeProbeError(result: ProbeResult): string {
    switch (result.errorKind) {
        case 'timeout':
            return '连接超时：请检查 IP、端口是否正确，以及防火墙 / 安全组是否放行';
        case 'network':
            return '网络不可达：可能是地址或端口错误、DNS 解析失败、证书无效，或服务端未放行 CORS';
        case 'http-4xx':
            return `服务器返回 ${result.status}：路径可能配置错误（应填后端根地址）`;
        case 'http-5xx':
            return `服务器返回 ${result.status}：服务已响应但内部异常，可查看 /healthz 的 db 字段`;
        default:
            return '连接失败';
    }
}

/** 重新加载页面，让所有模块用新基址重新初始化 */
function reloadPage(): void {
    if (typeof window !== 'undefined') {
        window.location.reload();
    }
}

export function ServerConnectionForm({ mode = 'page', onSaved }: ServerConnectionFormProps) {
    const current = describeApiBase();
    const [value, setValue] = useState(current.origin ?? '');
    const [testing, setTesting] = useState(false);
    const [result, setResult] = useState<ProbeResult | null>(null);
    const [error, setError] = useState<string | null>(null);
    /** 已确认「知道是明文 http 但还要保存」的地址 */
    const [pendingInsecure, setPendingInsecure] = useState<string | null>(null);

    const normalized = normalizeApiBase(value);
    const insecure = isInsecureRemote(normalized);
    const saveLabel = mode === 'gate' ? '保存并进入系统' : '保存并重载';

    /** 保存成功后统一收口：回调优先，否则整页重载 */
    const finish = (base: string) => {
        setError(null);
        setPendingInsecure(null);
        if (onSaved) {
            onSaved(base);
            return;
        }
        reloadPage();
    };

    const applySave = (base: string, allowInsecure: boolean) => {
        try {
            const saved = setApiBase(value, { allowInsecure });
            finish(saved || base);
        } catch (err) {
            if (err instanceof InsecureTransportError) {
                setPendingInsecure(normalized);
                return;
            }
            setError(err instanceof Error ? err.message : '保存失败');
        }
    };

    const handleTest = async () => {
        setError(null);
        setResult(null);
        if (!normalized) {
            setError('请先填写服务器地址，例如 http://192.168.1.10:3001');
            return;
        }
        setTesting(true);
        try {
            const origin = normalized.replace(/\/api\/?$/i, '');
            setResult(await probeServer(origin));
        } finally {
            setTesting(false);
        }
    };

    const handleSave = () => {
        if (!normalized) {
            setError('请先填写服务器地址，例如 http://192.168.1.10:3001');
            return;
        }
        applySave(normalized, !insecure);
    };

    const handleReset = () => {
        clearApiBase();
        setValue('');
        setResult(null);
        setError(null);
        setPendingInsecure(null);
        if (onSaved) {
            onSaved('');
            return;
        }
        reloadPage();
    };

    return (
        <Stack gap="md">
            <TextInput
                label="服务器地址"
                description="填后端根地址即可（自动补 /api），例如 http://192.168.1.10:3001"
                placeholder="http://192.168.1.10:3001"
                value={value}
                onChange={(event) => {
                    setValue(event.currentTarget.value);
                    setResult(null);
                    setError(null);
                    setPendingInsecure(null);
                }}
                data-testid="server-address-input"
            />

            {normalized && (
                <Text size="xs" c="dimmed">
                    解析结果：<Code>{normalized}</Code>
                </Text>
            )}

            <Group gap="xs">
                <Button
                    variant="light"
                    leftSection={<IconPlugConnected size={16} />}
                    loading={testing}
                    onClick={handleTest}
                >
                    测试连接
                </Button>
                <Button leftSection={<IconDeviceFloppy size={16} />} onClick={handleSave}>
                    {saveLabel}
                </Button>
                <Button
                    variant="subtle"
                    color="gray"
                    leftSection={<IconRestore size={16} />}
                    onClick={handleReset}
                >
                    恢复默认
                </Button>
            </Group>

            {result?.ok && (
                <Alert color="teal" variant="light" icon={<IconCheck size={16} />}>
                    <Group gap="xs" wrap="wrap">
                        <Text size="sm">连接成功</Text>
                        <Badge size="sm" variant="light">
                            {result.latencyMs} ms
                        </Badge>
                        {result.body?.db && (
                            <Badge
                                size="sm"
                                variant="light"
                                color={result.body.db === 'up' ? 'teal' : 'red'}
                            >
                                db: {result.body.db}
                            </Badge>
                        )}
                        {result.body?.service && (
                            <Badge size="sm" variant="light" color="gray">
                                {result.body.service}
                            </Badge>
                        )}
                    </Group>
                </Alert>
            )}

            {result && !result.ok && (
                <Alert color="red" variant="light" icon={<IconAlertTriangle size={16} />}>
                    {describeProbeError(result)}
                </Alert>
            )}

            {insecure && !pendingInsecure && (
                <Alert color="yellow" variant="light" icon={<IconAlertTriangle size={16} />}>
                    该地址使用 http://，令牌与业务数据会明文传输。生产环境请改用 https://；
                    仅在内网等可信网络中继续。
                </Alert>
            )}

            {pendingInsecure && (
                <Alert
                    color="orange"
                    variant="light"
                    icon={<IconAlertTriangle size={16} />}
                    title="确认使用明文 HTTP？"
                >
                    <Stack gap="sm">
                        <Text size="sm">
                            <Code>{pendingInsecure}</Code> 不是本机地址，使用 http:// 会让登录令牌
                            在网络中明文传输。确认这是可信内网后可以继续保存。
                        </Text>
                        <Group gap="xs">
                            <Button
                                color="orange"
                                size="xs"
                                onClick={() => applySave(pendingInsecure, true)}
                            >
                                仍要保存
                            </Button>
                            <Button
                                variant="subtle"
                                color="gray"
                                size="xs"
                                onClick={() => setPendingInsecure(null)}
                            >
                                取消
                            </Button>
                        </Group>
                    </Stack>
                </Alert>
            )}

            {error && (
                <Alert color="red" variant="light" icon={<IconAlertTriangle size={16} />}>
                    {error}
                </Alert>
            )}
        </Stack>
    );
}

export default ServerConnectionForm;
