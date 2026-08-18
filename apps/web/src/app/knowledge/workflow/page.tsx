'use client';

import React, { useEffect, useState, useCallback } from 'react';
import Layout from '@/components/Layout';
import {
    Title,
    Text,
    Stack,
    Group,
    Button,
    TextInput,
    Loader,
    Alert,
    Badge,
    Table,
    ActionIcon,
    Tooltip,
    Paper,
} from '@mantine/core';
import {
    IconAlertCircle,
    IconPlus,
    IconSearch,
    IconRefresh,
    IconPlayerPlay,
    IconEdit,
    IconTrash,
    IconHistory,
    IconHierarchy,
} from '@tabler/icons-react';
import { useRouter } from 'next/navigation';
import {
    listWorkflows,
    deleteWorkflow,
    executeWorkflow,
    type WorkflowMeta,
} from '@/services/workflowService';

const StatusBadge: React.FC<{ status: string }> = ({ status }) => {
    const config: Record<string, { color: string; label: string }> = {
        draft: { color: 'gray', label: '草稿' },
        published: { color: 'green', label: '已发布' },
        archived: { color: 'orange', label: '已归档' },
    };
    const { color, label } = config[status] || { color: 'gray', label: status };
    return <Badge variant="light" color={color}>{label}</Badge>;
};

export default function WorkflowListPage() {
    const router = useRouter();
    const [workflows, setWorkflows] = useState<WorkflowMeta[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [searchQuery, setSearchQuery] = useState('');

    const loadWorkflows = useCallback(async () => {
        try {
            setLoading(true);
            setError(null);
            const data = await listWorkflows();
            setWorkflows(data);
        } catch (err: unknown) {
            setError(err instanceof Error ? err.message : '加载工作流列表失败');
        } finally {
            setLoading(false);
        }
    }, []);

    useEffect(() => {
        loadWorkflows();
    }, [loadWorkflows]);

    const filtered = workflows.filter(
        (w) =>
            w.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
            (w.description || '').toLowerCase().includes(searchQuery.toLowerCase())
    );

    const handleDelete = async (id: string) => {
        if (!confirm('确定删除这个工作流？')) return;
        try {
            await deleteWorkflow(id);
            await loadWorkflows();
        } catch {
            alert('删除失败');
        }
    };

    const handleExecute = async (id: string) => {
        try {
            await executeWorkflow({ workflowId: id });
            router.push(`/knowledge/workflow/runs?id=${id}`);
        } catch {
            // ignore
        }
    };

    const getNodeTypesPreview = (types: string[]) => {
        return types.slice(0, 3).map((t) => {
            const label = t.startsWith('skill:') ? t.replace('skill:', '') : t;
            return label;
        });
    };

    return (
        <Layout>
            <Stack gap="lg">
                <Group justify="space-between">
                    <Group>
                        <IconHierarchy size={28} />
                        <div>
                            <Title order={2}>AI 工作流</Title>
                            <Text c="dimmed">
                                管理和编排 AI 工作流（共 {workflows.length} 个）
                            </Text>
                        </div>
                    </Group>
                    <Group>
                        <Button variant="light" leftSection={<IconRefresh size={16} />} onClick={loadWorkflows} loading={loading}>
                            刷新
                        </Button>
                        <Button leftSection={<IconPlus size={16} />} onClick={() => router.push('/knowledge/workflow/new')}>
                            新建工作流
                        </Button>
                    </Group>
                </Group>

                {error && (
                    <Alert icon={<IconAlertCircle size={16} />} title="错误" color="red">
                        {error}
                    </Alert>
                )}

                <TextInput
                    placeholder="搜索工作流名称或描述..."
                    leftSection={<IconSearch size={16} />}
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    style={{ maxWidth: 400 }}
                />

                {loading ? (
                    <Group justify="center" py="xl"><Loader /></Group>
                ) : filtered.length === 0 ? (
                    <Paper withBorder p="xl" ta="center">
                        <Text c="dimmed">
                            {searchQuery ? '未找到匹配的工作流' : '暂无工作流，点击"新建工作流"开始编排'}
                        </Text>
                    </Paper>
                ) : (
                    <Table striped highlightOnHover withTableBorder>
                        <Table.Thead>
                            <Table.Tr>
                                <Table.Th>名称</Table.Th>
                                <Table.Th>状态</Table.Th>
                                <Table.Th>节点类型</Table.Th>
                                <Table.Th>执行次数</Table.Th>
                                <Table.Th>最后执行</Table.Th>
                                <Table.Th>创建时间</Table.Th>
                                <Table.Th style={{ width: 140 }}>操作</Table.Th>
                            </Table.Tr>
                        </Table.Thead>
                        <Table.Tbody>
                            {filtered.map((wf) => (
                                <Table.Tr key={wf.id}>
                                    <Table.Td>
                                        <Text size="sm" fw={500}>{wf.name}</Text>
                                        {wf.description && (
                                            <Text size="xs" c="dimmed" lineClamp={1}>{wf.description}</Text>
                                        )}
                                    </Table.Td>
                                    <Table.Td><StatusBadge status={wf.status} /></Table.Td>
                                    <Table.Td>
                                        <Group gap={4}>
                                            {getNodeTypesPreview(wf.node_types).map((t) => (
                                                <Badge key={t} size="xs" variant="light" color="blue">{t}</Badge>
                                            ))}
                                            {wf.node_types.length > 3 && (
                                                <Badge size="xs" variant="light" color="gray">+{wf.node_types.length - 3}</Badge>
                                            )}
                                        </Group>
                                    </Table.Td>
                                    <Table.Td><Text size="sm">{wf.use_count}</Text></Table.Td>
                                    <Table.Td>
                                        <Text size="xs" c="dimmed">
                                            {wf.last_executed_at ? new Date(wf.last_executed_at).toLocaleString() : '-'}
                                        </Text>
                                    </Table.Td>
                                    <Table.Td>
                                        <Text size="xs" c="dimmed">{new Date(wf.created_at).toLocaleDateString()}</Text>
                                    </Table.Td>
                                    <Table.Td>
                                        <Group gap={4}>
                                            <Tooltip label="编辑">
                                                <ActionIcon variant="light" color="blue" size="sm" onClick={() => router.push(`/knowledge/workflow/editor?id=${wf.id}`)}>
                                                    <IconEdit size={14} />
                                                </ActionIcon>
                                            </Tooltip>
                                            <Tooltip label="执行">
                                                <ActionIcon variant="light" color="green" size="sm" onClick={() => handleExecute(wf.id)}>
                                                    <IconPlayerPlay size={14} />
                                                </ActionIcon>
                                            </Tooltip>
                                            <Tooltip label="执行历史">
                                                <ActionIcon variant="light" color="gray" size="sm" onClick={() => router.push(`/knowledge/workflow/runs?id=${wf.id}`)}>
                                                    <IconHistory size={14} />
                                                </ActionIcon>
                                            </Tooltip>
                                            <Tooltip label="删除">
                                                <ActionIcon variant="light" color="red" size="sm" onClick={() => handleDelete(wf.id)}>
                                                    <IconTrash size={14} />
                                                </ActionIcon>
                                            </Tooltip>
                                        </Group>
                                    </Table.Td>
                                </Table.Tr>
                            ))}
                        </Table.Tbody>
                    </Table>
                )}
            </Stack>
        </Layout>
    );
}