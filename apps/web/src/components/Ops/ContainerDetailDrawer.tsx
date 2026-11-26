'use client';

import React, { useEffect, useState } from 'react';
import { Alert, Badge, Drawer, Group, Stack, Tabs, Text } from '@mantine/core';
import { IconAlertTriangle } from '@tabler/icons-react';

import ContainerLogsPanel from './ContainerLogsPanel';
import ContainerStatsPanel from './ContainerStatsPanel';
import ContainerTerminalPanel from './ContainerTerminalPanel';
import type { ContainerWorkTarget } from './containerTarget';

export type ContainerDetailTab = 'logs' | 'stats' | 'terminal';

export interface ContainerDetailDrawerProps {
    opened: boolean;
    target: ContainerWorkTarget | null;
    initialTab: ContainerDetailTab;
    onClose: () => void;
    onTabChange?: (tab: ContainerDetailTab) => void;
}

export default function ContainerDetailDrawer({
    opened,
    target,
    initialTab,
    onClose,
    onTabChange,
}: ContainerDetailDrawerProps) {
    const [tab, setTab] = useState<ContainerDetailTab>(initialTab);

    // 目标变化时同步外部 Tab（深链进入 / 切换资源）
    useEffect(() => {
        setTab(initialTab);
    }, [target?.uid, initialTab]);

    const handleTabChange = (value: string | null) => {
        const next = (value ?? 'logs') as ContainerDetailTab;
        setTab(next);
        onTabChange?.(next);
    };

    return (
        <Drawer
            opened={opened && !!target}
            onClose={onClose}
            position="right"
            size="80%"
            title={
                target ? (
                    <Group gap="xs">
                        <Text fw={600}>{target.displayName}</Text>
                        <Badge variant="light">{target.provider === 'docker' ? 'Docker 容器' : 'K8s Pod'}</Badge>
                        {target.connectionName && (
                            <Text size="xs" c="dimmed">连接：{target.connectionName}</Text>
                        )}
                    </Group>
                ) : null
            }
        >
            {!target ? (
                <Alert color="yellow" icon={<IconAlertTriangle size={16} />}>
                    资源标识无法解析，请返回列表重新选择。
                </Alert>
            ) : (
                <Tabs value={tab} onChange={handleTabChange} keepMounted={false}>
                    <Tabs.List>
                        <Tabs.Tab value="logs">日志</Tabs.Tab>
                        <Tabs.Tab value="stats">性能</Tabs.Tab>
                        <Tabs.Tab value="terminal">终端</Tabs.Tab>
                    </Tabs.List>

                    <Tabs.Panel value="logs" pt="sm">
                        <ContainerLogsPanel key={`logs-${target.uid}`} target={target} />
                    </Tabs.Panel>
                    <Tabs.Panel value="stats" pt="sm">
                        <ContainerStatsPanel key={`stats-${target.uid}`} target={target} />
                    </Tabs.Panel>
                    <Tabs.Panel value="terminal" pt="sm">
                        {target.execEnabled ? (
                            <ContainerTerminalPanel key={`terminal-${target.uid}`} target={target} />
                        ) : (
                            <Stack gap="xs">
                                <Alert color="yellow" icon={<IconAlertTriangle size={16} />}>
                                    该连接未允许容器终端：请在「连接管理 → 编辑」中勾选「允许容器终端」，
                                    并确认服务端已设置 `OPS_CONTAINER_EXEC_ENABLED=true`。
                                </Alert>
                                <Text size="xs" c="dimmed">
                                    日志与性能不受影响，可继续使用；scratch / distroless 镜像即使打开开关也无法进入 shell。
                                </Text>
                            </Stack>
                        )}
                    </Tabs.Panel>
                </Tabs>
            )}
        </Drawer>
    );
}
