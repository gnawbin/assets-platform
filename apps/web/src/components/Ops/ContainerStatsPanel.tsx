'use client';

import React from 'react';
import {
    ActionIcon,
    Alert,
    Badge,
    Card,
    Group,
    Loader,
    Progress,
    SimpleGrid,
    Stack,
    Table,
    Text,
    Tooltip,
} from '@mantine/core';
import { IconAlertTriangle, IconRefresh } from '@tabler/icons-react';

import { useContainerStats, type ContainerStatsPoint } from '@/hooks/useContainerStats';
import type { ContainerWorkTarget } from './containerTarget';

function formatBytes(value: number): string {
    if (!Number.isFinite(value) || value <= 0) return '0 B';
    const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
    let size = value;
    let index = 0;
    while (size >= 1024 && index < units.length - 1) {
        size /= 1024;
        index += 1;
    }
    return `${size.toFixed(size >= 100 || index === 0 ? 0 : 2)} ${units[index]}`;
}

function formatNanoCores(value: number): string {
    const millicores = value / 1_000_000;
    if (millicores >= 1000) return `${(millicores / 1000).toFixed(2)} core`;
    return `${millicores.toFixed(0)} m`;
}

/** 极简 Sparkline（不引入额外图表依赖） */
function Sparkline({ points, color }: { points: ContainerStatsPoint[]; color: string }) {
    const values = points.map((point) => (point.cpu === null ? 0 : point.cpu));
    if (values.length < 2) {
        return <Text size="xs" c="dimmed">采样中…</Text>;
    }
    const max = Math.max(...values, 1);
    const width = 240;
    const height = 48;
    const path = values
        .map((value, index) => {
            const x = (index / (values.length - 1)) * width;
            const y = height - (value / max) * (height - 4) - 2;
            return `${index === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`;
        })
        .join(' ');
    return (
        <svg width={width} height={height} role="img" aria-label="CPU 趋势">
            <path d={path} fill="none" stroke={color} strokeWidth={1.5} />
        </svg>
    );
}

export interface ContainerStatsPanelProps {
    target: ContainerWorkTarget;
}

export default function ContainerStatsPanel({ target }: ContainerStatsPanelProps) {
    const { dockerStats, podMetrics, points, loading, error, refresh } = useContainerStats({
        provider: target.provider,
        connectionId: target.connectionId,
        container: target.dockerContainer,
        namespace: target.namespace,
        pod: target.pod,
        enabled: true,
    });

    return (
        <Stack gap="sm">
            <Group justify="space-between">
                <Group gap="xs">
                    <Badge color={target.provider === 'docker' ? 'blue' : 'violet'} variant="light">
                        {target.provider === 'docker' ? 'Docker 实时' : 'metrics-server（约 15s 窗口）'}
                    </Badge>
                    {loading && <Loader size="xs" />}
                </Group>
                <Tooltip label="立即刷新">
                    <ActionIcon variant="light" aria-label="刷新性能" onClick={() => void refresh()}>
                        <IconRefresh size={16} />
                    </ActionIcon>
                </Tooltip>
            </Group>

            {error && (
                <Alert color="red" icon={<IconAlertTriangle size={16} />}>
                    {error}
                </Alert>
            )}

            {target.provider === 'docker' && dockerStats && (
                <>
                    <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
                        <Card withBorder padding="sm">
                            <Text size="xs" c="dimmed">CPU</Text>
                            <Text fw={700} size="lg">
                                {dockerStats.cpu_percent === null
                                    ? '采样中'
                                    : `${dockerStats.cpu_percent.toFixed(2)}%`}
                            </Text>
                            <Sparkline points={points} color="#4dabf7" />
                        </Card>
                        <Card withBorder padding="sm">
                            <Text size="xs" c="dimmed">内存（工作集）</Text>
                            <Text fw={700} size="lg">
                                {formatBytes(dockerStats.mem_used_bytes)}
                                <Text span size="xs" c="dimmed"> / {formatBytes(dockerStats.mem_limit_bytes)}</Text>
                            </Text>
                            <Progress
                                value={Math.min(100, dockerStats.mem_percent)}
                                mt="xs"
                                color={dockerStats.mem_percent > 85 ? 'red' : 'teal'}
                            />
                            <Text size="xs" c="dimmed" mt={4}>
                                {dockerStats.mem_percent.toFixed(2)}%
                            </Text>
                        </Card>
                        <Card withBorder padding="sm">
                            <Text size="xs" c="dimmed">网络</Text>
                            <Text size="sm">接收：{formatBytes(dockerStats.net_rx_bytes)}</Text>
                            <Text size="sm">发送：{formatBytes(dockerStats.net_tx_bytes)}</Text>
                        </Card>
                        <Card withBorder padding="sm">
                            <Text size="xs" c="dimmed">块设备 / 进程</Text>
                            <Text size="sm">读：{dockerStats.blkio_read_bytes === null ? '—' : formatBytes(dockerStats.blkio_read_bytes)}</Text>
                            <Text size="sm">写：{dockerStats.blkio_write_bytes === null ? '—' : formatBytes(dockerStats.blkio_write_bytes)}</Text>
                            <Text size="sm">PIDs：{dockerStats.pids ?? '—'}</Text>
                        </Card>
                    </SimpleGrid>
                    <Text size="xs" c="dimmed">
                        采样时间：{new Date(dockerStats.read_at_ms).toLocaleString()}
                    </Text>
                </>
            )}

            {target.provider === 'k8s' && podMetrics && (
                <>
                    <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
                        <Card withBorder padding="sm">
                            <Text size="xs" c="dimmed">CPU（Pod 合计）</Text>
                            <Text fw={700} size="lg">{formatNanoCores(podMetrics.cpu_nanocores)}</Text>
                        </Card>
                        <Card withBorder padding="sm">
                            <Text size="xs" c="dimmed">内存（Pod 合计）</Text>
                            <Text fw={700} size="lg">{formatBytes(podMetrics.memory_bytes)}</Text>
                        </Card>
                    </SimpleGrid>
                    <Table.ScrollContainer minWidth={420}>
                        <Table striped withTableBorder>
                            <Table.Thead>
                                <Table.Tr>
                                    <Table.Th>容器</Table.Th>
                                    <Table.Th>CPU</Table.Th>
                                    <Table.Th>内存</Table.Th>
                                </Table.Tr>
                            </Table.Thead>
                            <Table.Tbody>
                                {podMetrics.container_metrics.map((item) => (
                                    <Table.Tr key={item.name}>
                                        <Table.Td>{item.name}</Table.Td>
                                        <Table.Td>{formatNanoCores(item.cpu_nanocores)}</Table.Td>
                                        <Table.Td>{formatBytes(item.memory_bytes)}</Table.Td>
                                    </Table.Tr>
                                ))}
                            </Table.Tbody>
                        </Table>
                    </Table.ScrollContainer>
                    <Text size="xs" c="dimmed">
                        窗口：{podMetrics.window ?? '15s'} · 采样时间：{podMetrics.timestamp ?? '—'}
                    </Text>
                </>
            )}

            {!loading && !error && !dockerStats && !podMetrics && (
                <Text size="sm" c="dimmed">暂无性能数据（容器可能已停止或指标尚未就绪）</Text>
            )}
        </Stack>
    );
}

