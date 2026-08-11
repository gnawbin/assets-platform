'use client';

import React, { useState, useEffect } from 'react';
import {
    Timeline,
    Text,
    Group,
    Badge,
    Paper,
    Stack,
    Code,
    Loader,
    Alert,
    Collapse,
    Anchor,
} from '@mantine/core';
import { IconCheck, IconX, IconClock, IconPlayerPlay, IconAlertCircle } from '@tabler/icons-react';
import { listExecutions, getExecution, type WorkflowExecution, type NodeExecutionResult } from '@/services/workflowService';
import dayjs from 'dayjs';

// ======================== 工具函数 ========================

/** 安全序列化 unknown 类型为字符串，避免 TypeScript ReactNode 兼容错误 */
const safeStringify = (val: unknown): string => {
    if (val === undefined || val === null) return '';
    if (typeof val === 'string') return val;
    try {
        const result = JSON.stringify(val, null, 2);
        return result ?? '';
    } catch {
        return '';
    }
};

// ======================== 执行状态图标 ========================

const ExecutionStatusIcon: React.FC<{ status: string }> = ({ status }) => {
    switch (status) {
        case 'success':
            return <IconCheck size={16} color="white" />;
        case 'failed':
            return <IconX size={16} color="white" />;
        case 'running':
            return <IconPlayerPlay size={16} color="white" />;
        default:
            return <IconClock size={16} color="white" />;
    }
};

const ExecutionStatusColor: Record<string, string> = {
    success: 'green',
    failed: 'red',
    running: 'blue',
    cancelled: 'gray',
};

// ======================== 节点详情 ========================

interface NodeDetailProps {
    result: NodeExecutionResult;
    defaultOpen?: boolean;
}

const NodeDetail: React.FC<NodeDetailProps> = ({ result, defaultOpen = false }) => {
    const [opened, setOpened] = useState(defaultOpen);
    const nodeColor =
        result.status === 'success' ? 'green' :
            result.status === 'failed' ? 'red' : 'gray';

    return (
        <Paper withBorder p="sm" radius="sm">
            <Group justify="space-between" onClick={() => setOpened((o) => !o)} style={{ cursor: 'pointer' }}>
                <Group gap="xs">
                    <Badge size="sm" color={nodeColor} variant="light">
                        {result.status}
                    </Badge>
                    <Text size="sm" fw={500}>
                        {result.label || result.node_id}
                    </Text>
                </Group>
                <Group gap="xs">
                    {result.duration_ms !== undefined && (
                        <Text size="xs" c="dimmed">
                            {result.duration_ms >= 1000
                                ? `${(result.duration_ms / 1000).toFixed(1)}s`
                                : `${result.duration_ms}ms`}
                        </Text>
                    )}
                </Group>
            </Group>

            <Collapse expanded={opened}>
                <Stack gap="xs" mt="sm">
                    {Boolean(result.input) && (
                        <div>
                            <Text size="xs" fw={500} c="dimmed">输入</Text>
                            <Code block style={{ fontSize: 11, whiteSpace: 'pre-wrap' }}>
                                {safeStringify(result.input)}
                            </Code>
                        </div>
                    )}
                    {Boolean(result.output) && (
                        <div>
                            <Text size="xs" fw={500} c="dimmed">输出</Text>
                            <Code block style={{ fontSize: 11, whiteSpace: 'pre-wrap' }}>
                                {safeStringify(result.output)}
                            </Code>
                        </div>
                    )}
                    {result.error && (
                        <Alert icon={<IconAlertCircle size={14} />} color="red" variant="light" p="xs">
                            <Text size="xs">{result.error}</Text>
                        </Alert>
                    )}
                </Stack>
            </Collapse>
        </Paper>
    );
};

// ======================== 执行时间线 ========================

interface ExecutionTimelineProps {
    executionId: string;
}

export const ExecutionDetailTimeline: React.FC<ExecutionTimelineProps> = ({ executionId }) => {
    const [execution, setExecution] = useState<WorkflowExecution | null>(null);
    const [loading, setLoading] = useState(true);

    useEffect(() => {
        (async () => {
            try {
                const data = await getExecution(executionId);
                setExecution(data);
            } catch {
                // ignore
            } finally {
                setLoading(false);
            }
        })();
    }, [executionId]);

    if (loading) {
        return <Group justify="center" py="xl"><Loader /></Group>;
    }

    if (!execution) {
        return <Text c="dimmed" ta="center" py="xl">未找到执行记录</Text>;
    }

    return (
        <Stack gap="md">
            <Group gap="xs">
                <Badge size="lg" color={ExecutionStatusColor[execution.status]} variant="filled">
                    {execution.status === 'success' ? '成功' :
                        execution.status === 'failed' ? '失败' :
                            execution.status === 'running' ? '运行中' : '已取消'}
                </Badge>
                {execution.total_duration_ms && (
                    <Text size="sm" c="dimmed">
                        总耗时: {execution.total_duration_ms >= 1000
                            ? `${(execution.total_duration_ms / 1000).toFixed(1)}s`
                            : `${execution.total_duration_ms}ms`}
                    </Text>
                )}
                {execution.total_tokens && (
                    <Text size="sm" c="dimmed">
                        Token: {execution.total_tokens.toLocaleString()}
                    </Text>
                )}
            </Group>

            {execution.error_message && (
                <Alert icon={<IconAlertCircle size={14} />} color="red" title="执行错误">
                    {execution.error_message}
                </Alert>
            )}

            <Text size="sm" c="dimmed">
                执行时间: {dayjs(execution.created_at).format('YYYY-MM-DD HH:mm:ss')}
                {execution.finished_at && ` → ${dayjs(execution.finished_at).format('HH:mm:ss')}`}
            </Text>

            <Timeline active={execution.node_results?.length ?? 0} bulletSize={24}>
                {(execution.node_results || []).map((nr, idx) => (
                    <Timeline.Item
                        key={nr.node_id}
                        bullet={<ExecutionStatusIcon status={nr.status} />}
                        color={ExecutionStatusColor[nr.status]}
                        title={nr.label || nr.node_id}
                    >
                        <NodeDetail result={nr} defaultOpen={nr.status === 'failed'} />
                    </Timeline.Item>
                ))}
            </Timeline>
        </Stack>
    );
};

// ======================== 执行历史列表 ========================

interface ExecutionHistoryListProps {
    workflowId?: string;
    onSelectExecution?: (executionId: string) => void;
}

const ExecutionTimeline: React.FC<ExecutionHistoryListProps> = ({ workflowId, onSelectExecution }) => {
    const [executions, setExecutions] = useState<WorkflowExecution[]>([]);
    const [loading, setLoading] = useState(true);

    useEffect(() => {
        (async () => {
            try {
                const data = await listExecutions(workflowId);
                setExecutions(data);
            } catch {
                // ignore
            } finally {
                setLoading(false);
            }
        })();
    }, [workflowId]);

    if (loading) {
        return <Group justify="center" py="xl"><Loader /></Group>;
    }

    if (executions.length === 0) {
        return <Text c="dimmed" ta="center" py="xl">暂无执行历史</Text>;
    }

    return (
        <Stack gap="sm">
            {executions.map((exec) => (
                <Paper
                    key={exec.id}
                    withBorder
                    p="sm"
                    radius="sm"
                    style={{ cursor: onSelectExecution ? 'pointer' : 'default' }}
                    onClick={() => onSelectExecution?.(exec.id)}
                >
                    <Group justify="space-between">
                        <Group gap="xs">
                            <Badge
                                size="sm"
                                color={ExecutionStatusColor[exec.status]}
                                variant="filled"
                                leftSection={<ExecutionStatusIcon status={exec.status} />}
                            >
                                {exec.status === 'success' ? '成功' :
                                    exec.status === 'failed' ? '失败' :
                                        exec.status === 'running' ? '运行中' : '已取消'}
                            </Badge>
                            <Text size="sm" fw={500}>
                                {exec.workflow_name || `执行 #${exec.id}`}
                            </Text>
                        </Group>
                        <Group gap="md">
                            {exec.total_duration_ms && (
                                <Text size="xs" c="dimmed">
                                    {exec.total_duration_ms >= 1000
                                        ? `${(exec.total_duration_ms / 1000).toFixed(1)}s`
                                        : `${exec.total_duration_ms}ms`}
                                </Text>
                            )}
                            <Text size="xs" c="dimmed">
                                {dayjs(exec.created_at).format('MM-DD HH:mm')}
                            </Text>
                        </Group>
                    </Group>
                    {exec.error_message && (
                        <Text size="xs" c="red" mt={4}>
                            {exec.error_message}
                        </Text>
                    )}
                </Paper>
            ))}
        </Stack>
    );
};

export default ExecutionTimeline;