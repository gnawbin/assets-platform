'use client';

import React, { useCallback, useEffect, useRef, useState } from 'react';
import {
    ActionIcon,
    Alert,
    Badge,
    Group,
    Paper,
    Select,
    Stack,
    Text,
    Tooltip,
} from '@mantine/core';
import { IconAlertTriangle, IconCopy, IconPlugOff, IconRefresh, IconTerminal2 } from '@tabler/icons-react';
import { notifications } from '@mantine/notifications';
import '@xterm/xterm/css/xterm.css';

import {
    useContainerTerminal,
    type ContainerTerminalStatus,
} from '@/hooks/useContainerTerminal';
import type { OpsTerminalTarget } from '@/services/opsService';
import type { ContainerWorkTarget } from './containerTarget';

const SHELL_OPTIONS = [
    { value: '/bin/sh', label: '/bin/sh' },
    { value: '/bin/bash', label: '/bin/bash' },
    { value: '/bin/ash', label: '/bin/ash' },
    { value: '/busybox/sh', label: '/busybox/sh' },
];

const STATUS_META: Record<ContainerTerminalStatus, { label: string; color: string }> = {
    idle: { label: '未连接', color: 'gray' },
    connecting: { label: '连接中', color: 'blue' },
    active: { label: '已连接', color: 'teal' },
    disconnected: { label: '已断开', color: 'yellow' },
    failed: { label: '连接失败', color: 'red' },
};

export interface ContainerTerminalPanelProps {
    target: ContainerWorkTarget;
}

export default function ContainerTerminalPanel({ target }: ContainerTerminalPanelProps) {
    const containerRef = useRef<HTMLDivElement>(null);
    const { status, error, exitCode, connect, disconnect, getSelection } =
        useContainerTerminal(containerRef);
    const [shell, setShell] = useState('/bin/sh');
    const connectedKey = useRef<string | null>(null);

    const terminalTarget: OpsTerminalTarget | null =
        target.provider === 'docker'
            ? { kind: 'docker', container: target.dockerContainer ?? '' }
            : {
                  kind: 'k8s',
                  namespace: target.namespace ?? '',
                  pod: target.pod ?? '',
                  container: target.containerName,
              };

    const start = useCallback(
        async (force: boolean) => {
            if (!terminalTarget) return;
            const key = `${target.uid}|${shell}|${target.containerName ?? ''}`;
            if (!force && connectedKey.current === key) return;
            connectedKey.current = key;
            await connect(target.connectionId, terminalTarget, shell);
        },
        [connect, shell, target.connectionId, target.containerName, target.uid, terminalTarget],
    );

    useEffect(() => {
        void start(false);
        // 目标 / shell 变化时重建会话；start 引用由这些依赖驱动
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [target.uid, shell, target.containerName]);

    useEffect(
        () => () => {
            connectedKey.current = null;
            void disconnect();
        },
        [disconnect],
    );

    const copySelection = async () => {
        const text = getSelection();
        if (!text) {
            notifications.show({ color: 'gray', title: '没有选中内容', message: '请先框选终端文本' });
            return;
        }
        try {
            await navigator.clipboard.writeText(text);
            notifications.show({ color: 'teal', title: '已复制', message: `${text.length} 个字符` });
        } catch {
            notifications.show({ color: 'red', title: '复制失败', message: '浏览器拒绝了剪贴板权限' });
        }
    };

    const meta = STATUS_META[status];
    const notRunnable =
        (target.provider === 'docker' && !target.dockerContainer) ||
        (target.provider === 'k8s' && (!target.namespace || !target.pod));

    if (notRunnable) {
        return (
            <Alert color="red" icon={<IconAlertTriangle size={16} />}>
                资源标识无法解析为容器 / Pod，无法打开终端。
            </Alert>
        );
    }

    return (
        <Paper withBorder p="sm" radius="md">
            <Stack gap="xs" style={{ flex: 1, minHeight: 0 }}>
                <Group justify="space-between" wrap="nowrap">
                    <Group gap="xs" wrap="nowrap">
                        <IconTerminal2 size={18} />
                        <Text fw={600} size="sm" lineClamp={1}>
                            {target.displayName}
                            {target.containerName ? ` / ${target.containerName}` : ''}
                        </Text>
                    </Group>
                    <Group gap="xs" wrap="nowrap">
                        <Select
                            aria-label="shell"
                            data={SHELL_OPTIONS}
                            value={shell}
                            onChange={(value) => setShell(value ?? '/bin/sh')}
                            size="xs"
                            w={130}
                            allowDeselect={false}
                        />
                        {exitCode !== null && (
                            <Badge color="gray" variant="light">退出码 {exitCode}</Badge>
                        )}
                        <Badge color={meta.color} variant="light">{meta.label}</Badge>
                        <Tooltip label="复制选中内容">
                            <ActionIcon variant="light" aria-label="复制选中内容" onClick={() => void copySelection()}>
                                <IconCopy size={16} />
                            </ActionIcon>
                        </Tooltip>
                        <Tooltip label="重新连接">
                            <ActionIcon
                                variant="light"
                                aria-label="重新连接"
                                disabled={status === 'connecting'}
                                onClick={() => void start(true)}
                            >
                                <IconRefresh size={16} />
                            </ActionIcon>
                        </Tooltip>
                        <Tooltip label="断开">
                            <ActionIcon
                                variant="light"
                                color="red"
                                aria-label="断开"
                                disabled={status !== 'active' && status !== 'connecting'}
                                onClick={() => void disconnect()}
                            >
                                <IconPlugOff size={16} />
                            </ActionIcon>
                        </Tooltip>
                    </Group>
                </Group>

                {error && (
                    <Alert color="red" icon={<IconAlertTriangle size={16} />}>
                        {error}
                    </Alert>
                )}
                {!error && exitCode === 127 && (
                    <Alert color="yellow" icon={<IconAlertTriangle size={16} />}>
                        该镜像不含所选 shell（scratch/distroless），请改用日志 / 性能，或使用 debug sidecar。
                    </Alert>
                )}

                <div
                    ref={containerRef}
                    data-testid="container-terminal"
                    style={{
                        flex: 1,
                        minHeight: 360,
                        padding: 6,
                        borderRadius: 6,
                        overflow: 'hidden',
                        background: '#0b0f14',
                    }}
                />
            </Stack>
        </Paper>
    );
}

