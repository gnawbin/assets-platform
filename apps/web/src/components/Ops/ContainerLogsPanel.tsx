'use client';

import React, { useCallback, useEffect, useState } from 'react';
import {
    Alert,
    Badge,
    Button,
    Group,
    Loader,
    Select,
    Stack,
    Switch,
    Text,
} from '@mantine/core';
import { IconAlertTriangle, IconCopy, IconRefresh } from '@tabler/icons-react';
import { notifications } from '@mantine/notifications';

import {
    getOpsContainerLogs,
    getOpsPodLogs,
    type OpsContainerLogsView,
    type OpsPodLogsView,
} from '@/services/opsService';
import type { ContainerWorkTarget } from './containerTarget';

const LINE_OPTIONS = ['200', '500', '1000', '2000'];

export interface ContainerLogsPanelProps {
    target: ContainerWorkTarget;
}

export default function ContainerLogsPanel({ target }: ContainerLogsPanelProps) {
    const [rows, setRows] = useState('200');
    const [timestamps, setTimestamps] = useState(false);
    const [previous, setPrevious] = useState(false);
    const [containerName, setContainerName] = useState<string | null>(target.containerName);
    const [dockerLogs, setDockerLogs] = useState<OpsContainerLogsView | null>(null);
    const [podLogs, setPodLogs] = useState<OpsPodLogsView | null>(null);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    // 目标切换时重置容器选择
    useEffect(() => {
        setContainerName(target.containerName ?? target.containers[0] ?? null);
        setPrevious(false);
        setDockerLogs(null);
        setPodLogs(null);
    }, [target.uid, target.containerName, target.containers]);

    const load = useCallback(async () => {
        setLoading(true);
        setError(null);
        try {
            if (target.provider === 'docker') {
                const view = await getOpsContainerLogs(
                    target.connectionId,
                    target.dockerContainer ?? '',
                    Number(rows),
                    timestamps,
                );
                setDockerLogs(view);
                setPodLogs(null);
            } else {
                const view = await getOpsPodLogs(
                    target.connectionId,
                    target.namespace ?? '',
                    target.pod ?? '',
                    containerName,
                    previous,
                    Number(rows),
                    timestamps,
                );
                setPodLogs(view);
                setDockerLogs(null);
            }
        } catch (err) {
            setError(err instanceof Error ? err.message : '加载日志失败');
        } finally {
            setLoading(false);
        }
    }, [containerName, previous, rows, target, timestamps]);

    useEffect(() => {
        void load();
    }, [load]);

    const truncated = dockerLogs?.truncated ?? podLogs?.truncated ?? false;
    const lines =
        target.provider === 'docker'
            ? (dockerLogs?.lines ?? [])
            : (podLogs?.lines ?? []).map((message) => ({ stream: 'console', message }));

    const copyAll = async () => {
        const text = lines.map((line) => line.message).join('\n');
        if (!text) {
            notifications.show({ color: 'gray', title: '暂无日志', message: '请先刷新' });
            return;
        }
        try {
            await navigator.clipboard.writeText(text);
            notifications.show({ color: 'teal', title: '已复制', message: `${lines.length} 行` });
        } catch {
            notifications.show({ color: 'red', title: '复制失败', message: '浏览器拒绝了剪贴板权限' });
        }
    };

    return (
        <Stack gap="xs">
            <Group justify="space-between" wrap="wrap">
                <Group gap="xs">
                    <Select
                        aria-label="行数"
                        data={LINE_OPTIONS}
                        value={rows}
                        onChange={(value) => setRows(value ?? '200')}
                        w={110}
                    />
                    <Switch
                        label="时间戳"
                        checked={timestamps}
                        onChange={(event) => setTimestamps(event.currentTarget.checked)}
                    />
                    {target.provider === 'k8s' && target.containers.length > 1 && (
                        <Select
                            aria-label="容器"
                            placeholder="选择容器"
                            data={target.containers}
                            value={containerName}
                            onChange={setContainerName}
                            w={200}
                        />
                    )}
                    {target.provider === 'k8s' && (
                        <Switch
                            label="上次运行日志"
                            checked={previous}
                            onChange={(event) => setPrevious(event.currentTarget.checked)}
                        />
                    )}
                </Group>
                <Group gap="xs">
                    <Button
                        size="xs"
                        variant="light"
                        leftSection={<IconRefresh size={14} />}
                        onClick={() => void load()}
                        loading={loading}
                    >
                        刷新
                    </Button>
                    <Button
                        size="xs"
                        variant="default"
                        leftSection={<IconCopy size={14} />}
                        onClick={() => void copyAll()}
                    >
                        复制
                    </Button>
                </Group>
            </Group>

            {truncated && (
                <Alert color="yellow" icon={<IconAlertTriangle size={16} />}>
                    日志已截断（超过字节上限），可减少行数或改用 `docker logs` / `kubectl logs`。
                </Alert>
            )}
            {error && (
                <Alert color="red" icon={<IconAlertTriangle size={16} />}>
                    {error}
                </Alert>
            )}
            {loading && !lines.length && (
                <Group justify="center" py="md">
                    <Loader size="sm" />
                </Group>
            )}

            <div
                style={{
                    maxHeight: 520,
                    minHeight: 240,
                    overflow: 'auto',
                    background: '#0b0f14',
                    borderRadius: 8,
                    padding: 12,
                    fontFamily: 'Menlo, Consolas, "Courier New", monospace',
                    fontSize: 12,
                    lineHeight: 1.55,
                }}
            >
                {lines.length === 0 && !loading ? (
                    <Text c="dimmed" size="xs">暂无日志</Text>
                ) : (
                    lines.map((line, index) => (
                        <div
                            key={`${index}-${line.message.slice(0, 24)}`}
                            style={{
                                whiteSpace: 'pre-wrap',
                                wordBreak: 'break-all',
                                color: line.stream === 'stderr' ? '#ff8f8f' : '#d8dee9',
                            }}
                        >
                            {line.message}
                        </div>
                    ))
                )}
            </div>
            {target.provider === 'k8s' && podLogs?.container && (
                <Badge variant="light" color="gray">
                    容器：{podLogs.container}
                </Badge>
            )}
        </Stack>
    );
}

