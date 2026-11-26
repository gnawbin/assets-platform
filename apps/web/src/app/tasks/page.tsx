'use client';

/**
 * 任务中心（异步解析任务）
 *
 * 数据来源：`GET /api/tasks`（租户 `task_ticket`），5 秒自动刷新；
 * 入队入口在「知识资产」页的「解析」按钮（`POST /api/tasks/parse`）。
 *
 * 说明：`TASK_QUEUE_MODE=inline`（默认）时后端不可入队，页面会显示后端返回的
 * 「任务队列未开启」提示 —— 这是预期行为，不是页面故障。
 */

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import Layout from '@/components/Layout';
import {
    ActionIcon,
    Alert,
    Badge,
    Collapse,
    Group,
    Loader,
    Paper,
    Progress,
    Select,
    Switch,
    Table,
    Text,
    Title,
    Tooltip,
} from '@mantine/core';
import {
    IconAlertCircle,
    IconChevronDown,
    IconChevronRight,
    IconRefresh,
} from '@tabler/icons-react';
import {
    TASK_STAGE_LABEL,
    TASK_STATUS_COLOR,
    TASK_STATUS_LABEL,
    isTaskTerminal,
    listParseTasks,
    type TaskItem,
    type TaskStage,
    type TaskStatus,
} from '@/services/taskService';

/** 自动刷新间隔（毫秒） */
const REFRESH_INTERVAL_MS = 5000;

/** 状态筛选选项 */
const STATUS_OPTIONS = [
    { value: 'all', label: '全部状态' },
    { value: 'pending', label: '排队中' },
    { value: 'running', label: '执行中' },
    { value: 'completed', label: '已完成' },
    { value: 'failed', label: '失败' },
] as const;

/** 时间格式化（后端给 RFC3339，本地展示） */
function formatTime(value: string | null): string {
    if (!value) return '-';
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return '-';
    return date.toLocaleString();
}

/** 阶段中文名（兼容未知值） */
function stageLabel(stage: TaskStage | null): string {
    if (!stage) return '-';
    return TASK_STAGE_LABEL[stage] ?? stage;
}

export default function TasksPage() {
    const [tasks, setTasks] = useState<TaskItem[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [statusFilter, setStatusFilter] = useState<string>('all');
    const [autoRefresh, setAutoRefresh] = useState(true);
    const [expanded, setExpanded] = useState<Record<string, boolean>>({});

    const loadTasks = useCallback(async () => {
        try {
            setError(null);
            const rows = await listParseTasks(
                statusFilter === 'all' ? {} : { status: statusFilter as TaskStatus },
            );
            setTasks(rows);
        } catch (err) {
            setError(err instanceof Error ? err.message : '加载任务列表失败');
        } finally {
            setLoading(false);
        }
    }, [statusFilter]);

    // 首次 / 切换筛选时立即拉一次
    useEffect(() => {
        setLoading(true);
        loadTasks();
    }, [loadTasks]);

    // 自动刷新：仅在开启时轮询（整表刷新，简单可靠）
    useEffect(() => {
        if (!autoRefresh) return;
        const timer = window.setInterval(() => {
            void loadTasks();
        }, REFRESH_INTERVAL_MS);
        return () => window.clearInterval(timer);
    }, [autoRefresh, loadTasks]);

    const stats = useMemo(() => {
        const counters = { pending: 0, running: 0, completed: 0, failed: 0 };
        for (const task of tasks) {
            if (task.status in counters) {
                counters[task.status as TaskStatus] += 1;
            }
        }
        return counters;
    }, [tasks]);

    const toggleExpanded = (taskId: string) => {
        setExpanded((prev) => ({ ...prev, [taskId]: !prev[taskId] }));
    };

    return (
        <Layout>
            <Group justify="space-between" mb="md">
                <div>
                    <Title order={3}>任务中心</Title>
                    <Text size="xs" c="dimmed">
                        视频 / 音频 / 文档解析任务进度（数据来自各租户 task_ticket，
                        {REFRESH_INTERVAL_MS / 1000}s 自动刷新）
                    </Text>
                </div>
                <Group gap="sm">
                    <Switch
                        checked={autoRefresh}
                        onChange={(e) => setAutoRefresh(e.currentTarget.checked)}
                        label="自动刷新"
                        size="sm"
                    />
                    <Select
                        data={[...STATUS_OPTIONS]}
                        value={statusFilter}
                        onChange={(value) => setStatusFilter(value ?? 'all')}
                        allowDeselect={false}
                        w={140}
                        size="xs"
                    />
                    <Tooltip label="立即刷新">
                        <ActionIcon variant="light" onClick={() => void loadTasks()} loading={loading}>
                            <IconRefresh size={16} />
                        </ActionIcon>
                    </Tooltip>
                </Group>
            </Group>

            <Group gap="xs" mb="md">
                <Badge color="gray" variant="light">排队 {stats.pending}</Badge>
                <Badge color="blue" variant="light">执行中 {stats.running}</Badge>
                <Badge color="teal" variant="light">已完成 {stats.completed}</Badge>
                <Badge color="red" variant="light">失败 {stats.failed}</Badge>
            </Group>

            {error && (
                <Alert icon={<IconAlertCircle size={16} />} color="red" mb="md" title="加载失败">
                    {error}
                </Alert>
            )}

            <Paper withBorder>
                {loading && tasks.length === 0 ? (
                    <Group justify="center" py="xl">
                        <Loader size="sm" />
                    </Group>
                ) : tasks.length === 0 ? (
                    <Text c="dimmed" size="sm" ta="center" py="xl">
                        暂无任务。可在「知识库 → 知识资产」页对已绑定文件的资产点击「解析」发起任务。
                    </Text>
                ) : (
                    <Table striped highlightOnHover verticalSpacing="xs">
                        <Table.Thead>
                            <Table.Tr>
                                <Table.Th w={40} />
                                <Table.Th>任务</Table.Th>
                                <Table.Th>资产</Table.Th>
                                <Table.Th>队列</Table.Th>
                                <Table.Th w={90}>状态</Table.Th>
                                <Table.Th w={200}>进度</Table.Th>
                                <Table.Th w={80}>重试</Table.Th>
                                <Table.Th w={170}>创建时间</Table.Th>
                            </Table.Tr>
                        </Table.Thead>
                        <Table.Tbody>{renderRows()}</Table.Tbody>
                    </Table>
                )}
            </Paper>

            <Text size="xs" c="dimmed" mt="md">
                提示：任务执行由 TASK_QUEUE_MODE 控制；未开启时入队会返回「任务队列未开启」。
                失败任务的重试由 worker 按 max_attempts 自动进行。
            </Text>
        </Layout>
    );

    /** 渲染表体（抽成函数，避免 JSX 嵌套过深） */
    function renderRows() {
        return tasks.map((task) => {
            const terminal = isTaskTerminal(task.status);
            const isOpen = Boolean(expanded[task.taskId]);
            return (
                <React.Fragment key={task.taskId}>
                    <Table.Tr>
                        <Table.Td>
                            <ActionIcon
                                variant="subtle"
                                size="sm"
                                onClick={() => toggleExpanded(task.taskId)}
                                aria-label="展开详情"
                            >
                                {isOpen ? <IconChevronDown size={14} /> : <IconChevronRight size={14} />}
                            </ActionIcon>
                        </Table.Td>
                        <Table.Td>
                            <Text size="xs" ff="monospace">
                                {task.taskId.slice(0, 8)}
                            </Text>
                            <Text size="xs" c="dimmed">
                                {task.taskType}
                            </Text>
                        </Table.Td>
                        <Table.Td>
                            <Text size="xs" ff="monospace">
                                {task.assetId ?? '-'}
                            </Text>
                        </Table.Td>
                        <Table.Td>
                            <Text size="xs">{task.queue}</Text>
                        </Table.Td>
                        <Table.Td>
                            <Badge
                                color={TASK_STATUS_COLOR[task.status] ?? 'gray'}
                                variant="light"
                                size="sm"
                            >
                                {TASK_STATUS_LABEL[task.status] ?? task.status}
                            </Badge>
                        </Table.Td>
                        <Table.Td>
                            <Progress
                                value={task.progress}
                                size="sm"
                                color={
                                    task.status === 'failed'
                                        ? 'red'
                                        : task.status === 'completed'
                                          ? 'teal'
                                          : 'blue'
                                }
                                animated={!terminal && task.status === 'running'}
                            />
                            <Text size="xs" c="dimmed" mt={2}>
                                {stageLabel(task.stage)} · {task.progress}%
                            </Text>
                        </Table.Td>
                        <Table.Td>
                            <Text size="xs">{task.attempts}</Text>
                        </Table.Td>
                        <Table.Td>
                            <Text size="xs">{formatTime(task.createdAt)}</Text>
                        </Table.Td>
                    </Table.Tr>
                    <Table.Tr>
                        <Table.Td colSpan={8} p={0}>
                            <Collapse expanded={isOpen}>
                                <Group
                                    gap="lg"
                                    align="flex-start"
                                    p="sm"
                                    bg="var(--mantine-color-gray-0)"
                                >
                                    <div>
                                        <Text size="xs" c="dimmed">任务 ID</Text>
                                        <Text size="xs" ff="monospace">{task.taskId}</Text>
                                    </div>
                                    <div>
                                        <Text size="xs" c="dimmed">开始 / 结束</Text>
                                        <Text size="xs">
                                            {formatTime(task.startedAt)} ~ {formatTime(task.finishedAt)}
                                        </Text>
                                    </div>
                                    <div>
                                        <Text size="xs" c="dimmed">最近更新</Text>
                                        <Text size="xs">{formatTime(task.updatedAt)}</Text>
                                    </div>
                                    {task.error && (
                                        <div style={{ flex: 1, minWidth: 260 }}>
                                            <Text size="xs" c="dimmed">错误</Text>
                                            <Text
                                                size="xs"
                                                c="red"
                                                style={{ whiteSpace: 'pre-wrap' }}
                                            >
                                                {task.error}
                                            </Text>
                                        </div>
                                    )}
                                </Group>
                            </Collapse>
                        </Table.Td>
                    </Table.Tr>
                </React.Fragment>
            );
        });
    }
}
