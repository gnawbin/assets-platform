'use client';

import React, { useCallback, useEffect, useState } from 'react';
import Layout from '@/components/Layout';
import {
    Alert,
    Badge,
    Button,
    Card,
    Center,
    Group,
    Loader,
    Modal,
    SimpleGrid,
    Stack,
    Table,
    Text,
    Title,
    Tooltip,
} from '@mantine/core';
import {
    IconPencil,
    IconPlayerPlay,
    IconPlugConnected,
    IconPlus,
    IconRefresh,
    IconTrash,
} from '@tabler/icons-react';
import { notifications } from '@mantine/notifications';
import {
    collectOpsResources,
    createOpsConnection,
    deleteOpsConnection,
    getOpsConnections,
    getOpsProviders,
    testOpsConnection,
    testOpsConnectionDraft,
    updateOpsConnection,
    type OpsConnectionView,
    type OpsHealthView,
    type OpsProviderOption,
} from '@/services/opsService';
import ConnectionFormFields, {
    createEmptyForm,
    formToInput,
    viewToForm,
    type OpsFormState,
} from '@/components/Ops/ConnectionFormFields';

export default function OpsConnectionsPage() {
    const [connections, setConnections] = useState<OpsConnectionView[]>([]);
    const [providers, setProviders] = useState<OpsProviderOption[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [modalOpen, setModalOpen] = useState(false);
    const [editingId, setEditingId] = useState<string | null>(null);
    const [form, setForm] = useState<OpsFormState>(createEmptyForm());
    const [health, setHealth] = useState<OpsHealthView | null>(null);
    const [busy, setBusy] = useState<string | null>(null);

    const loadData = useCallback(async () => {
        try {
            setLoading(true);
            setError(null);
            const [list, options] = await Promise.all([getOpsConnections(), getOpsProviders()]);
            setConnections(list);
            setProviders(options);
        } catch (err) {
            setError(err instanceof Error ? err.message : '加载失败');
        } finally {
            setLoading(false);
        }
    }, []);

    useEffect(() => {
        loadData();
    }, [loadData]);

    const openCreate = () => {
        setEditingId(null);
        setForm(createEmptyForm());
        setHealth(null);
        setModalOpen(true);
    };

    const openEdit = (row: OpsConnectionView) => {
        setEditingId(row.id);
        setForm(viewToForm(row));
        setHealth(null);
        setModalOpen(true);
    };

    const handleSave = async () => {
        try {
            setBusy('save');
            const input = formToInput(form);
            if (!input.name) {
                notifications.show({ color: 'red', title: '请填写连接名称', message: '' });
                return;
            }
            if (editingId) {
                await updateOpsConnection(editingId, input);
            } else {
                await createOpsConnection(input);
            }
            notifications.show({ color: 'green', title: editingId ? '连接已更新' : '连接已创建', message: '' });
            setModalOpen(false);
            await loadData();
        } catch (err) {
            notifications.show({ color: 'red', title: '保存失败', message: err instanceof Error ? err.message : String(err) });
        } finally {
            setBusy(null);
        }
    };

    const handleDelete = async (row: OpsConnectionView) => {
        if (!window.confirm('确认删除连接「' + row.name + '」？关联的资源观测记录会保留。')) {
            return;
        }
        try {
            setBusy('delete-' + row.id);
            await deleteOpsConnection(row.id);
            notifications.show({ color: 'green', title: '连接已删除', message: '' });
            await loadData();
        } catch (err) {
            notifications.show({ color: 'red', title: '删除失败', message: err instanceof Error ? err.message : String(err) });
        } finally {
            setBusy(null);
        }
    };

    const handleTest = async (row: OpsConnectionView) => {
        try {
            setBusy('test-' + row.id);
            const result = await testOpsConnection(row.id);
            setHealth(result);
            notifications.show({
                color: result.ok ? 'green' : 'red',
                title: result.ok ? '连接正常' : '连接失败（' + result.failure_class + '）',
                message: result.summary,
            });
            await loadData();
        } catch (err) {
            notifications.show({ color: 'red', title: '探测失败', message: err instanceof Error ? err.message : String(err) });
        } finally {
            setBusy(null);
        }
    };

    const handleCollect = async (row: OpsConnectionView) => {
        try {
            setBusy('collect-' + row.id);
            const result = await collectOpsResources(row.id);
            notifications.show({
                color: 'green',
                title: '采集完成',
                message: '共 ' + result.total + ' 项（新增 ' + result.created + ' / 更新 ' + result.updated + ' / 下线 ' + result.gone + '），耗时 ' + result.elapsed_ms + ' ms',
            });
            await loadData();
        } catch (err) {
            notifications.show({ color: 'red', title: '采集失败', message: err instanceof Error ? err.message : String(err) });
        } finally {
            setBusy(null);
        }
    };

    const handleDraftTest = async () => {
        try {
            setBusy('draft-test');
            const result = await testOpsConnectionDraft(formToInput(form));
            setHealth(result);
        } catch (err) {
            notifications.show({ color: 'red', title: '探测失败', message: err instanceof Error ? err.message : String(err) });
        } finally {
            setBusy(null);
        }
    };


    const enabledCount = connections.filter((item) => item.enabled).length;
    const errorCount = connections.filter((item) => !!item.last_error).length;
    const collectedCount = connections.filter((item) => !!item.last_collect_at).length;

    return (
        <Layout>
            <Stack gap="lg">
                <Group justify="space-between">
                    <Title order={3}>运维采集 · 连接管理</Title>
                    <Group gap="xs">
                        <Button variant="default" leftSection={<IconRefresh size={16} />} onClick={loadData} loading={loading}>刷新</Button>
                        <Button leftSection={<IconPlus size={16} />} onClick={openCreate}>新增连接</Button>
                    </Group>
                </Group>

                {error && <Alert color="red" title="加载失败">{error}</Alert>}

                <SimpleGrid cols={{ base: 2, md: 4 }}>
                    <Card withBorder><Text size="xs" c="dimmed">连接总数</Text><Text fw={700} size="xl">{connections.length}</Text></Card>
                    <Card withBorder><Text size="xs" c="dimmed">启用中</Text><Text fw={700} size="xl">{enabledCount}</Text></Card>
                    <Card withBorder><Text size="xs" c="dimmed">已采集过</Text><Text fw={700} size="xl">{collectedCount}</Text></Card>
                    <Card withBorder><Text size="xs" c="dimmed">有错误</Text><Text fw={700} size="xl" c={errorCount ? "red" : undefined}>{errorCount}</Text></Card>
                </SimpleGrid>

                <Card withBorder p="sm">
                    {loading ? (
                        <Center py="xl"><Loader size="sm" /></Center>
                    ) : connections.length === 0 ? (
                        <Center py="xl"><Text c="dimmed">暂无连接，点击右上角「新增连接」开始</Text></Center>
                    ) : (
                        <Table.ScrollContainer minWidth={1100}>
                            <Table striped highlightOnHover withTableBorder>
                                <Table.Thead>
                                    <Table.Tr>
                                        <Table.Th>名称</Table.Th>
                                        <Table.Th>来源</Table.Th>
                                        <Table.Th>端点</Table.Th>
                                        <Table.Th>认证方式</Table.Th>
                                        <Table.Th>层级</Table.Th>
                                        <Table.Th>状态</Table.Th>
                                        <Table.Th>最近采集</Table.Th>
                                        <Table.Th>最近错误</Table.Th>
                                        <Table.Th>操作</Table.Th>
                                    </Table.Tr>
                                </Table.Thead>
                                <Table.Tbody>

                                    {connections.map((row) => (
                                        <Table.Tr key={row.id}>
                                            <Table.Td>
                                                <Text fw={500}>{row.name}</Text>
                                                {row.has_credential && <Badge size="xs" ml={6} variant="light">已存凭证</Badge>}
                                            </Table.Td>
                                            <Table.Td>{row.provider_label}</Table.Td>
                                            <Table.Td><Text size="xs">{row.endpoint}</Text></Table.Td>
                                            <Table.Td><Badge variant="light">{row.auth_type}</Badge></Table.Td>

                                            <Table.Td>
                                                <Badge variant="light" color={row.tier === "remote" ? "orange" : row.tier === "intranet" ? "blue" : "gray"}>{row.tier}</Badge>
                                            </Table.Td>
                                            <Table.Td>{row.enabled ? <Badge color="green">启用</Badge> : <Badge color="gray">停用</Badge>}</Table.Td>
                                            <Table.Td><Text size="xs">{row.last_collect_at ? new Date(row.last_collect_at).toLocaleString() : "—"}</Text></Table.Td>
                                            <Table.Td>
                                                {row.last_error ? (
                                                    <Tooltip label={row.last_error} multiline w={320}>
                                                        <Text size="xs" c="red" lineClamp={1}>{row.last_error}</Text>
                                                    </Tooltip>
                                                ) : (
                                                    <Text size="xs" c="dimmed">—</Text>
                                                )}
                                            </Table.Td>

                                            <Table.Td>
                                                <Group gap={4} wrap="nowrap">
                                                    <Button size="compact-xs" variant="light" leftSection={<IconPlugConnected size={14} />} loading={busy === "test-" + row.id} onClick={() => handleTest(row)}>测试</Button>
                                                    <Button size="compact-xs" variant="light" color="teal" leftSection={<IconPlayerPlay size={14} />} loading={busy === "collect-" + row.id} onClick={() => handleCollect(row)}>采集</Button>
                                                    <Button size="compact-xs" variant="subtle" leftSection={<IconPencil size={14} />} onClick={() => openEdit(row)}>编辑</Button>
                                                    <Button size="compact-xs" variant="subtle" color="red" leftSection={<IconTrash size={14} />} loading={busy === "delete-" + row.id} onClick={() => handleDelete(row)}>删除</Button>
                                                </Group>
                                            </Table.Td>
                                        </Table.Tr>
                                    ))}
                                </Table.Tbody>
                            </Table>
                        </Table.ScrollContainer>
                    )}
                </Card>

                <Modal opened={modalOpen} onClose={() => setModalOpen(false)} size="lg"
                    title={editingId ? "编辑连接" : "新增连接"}>
                    <Stack gap="md">
                        <ConnectionFormFields value={form} onChange={setForm} providerOptions={providers} editing={!!editingId} />
                        {health && (
                            <Alert color={health.ok ? "green" : "red"} title={health.summary}>
                                <Text size="xs">版本：{health.server_version ?? "—"} / API：{health.api_version ?? "—"} / 耗时：{health.latency_ms} ms</Text>
                                <Text size="xs">归类：{health.failure_class}（{health.failure_hint}）</Text>
                                {health.error && <Text size="xs" mt={4}>{health.error}</Text>}
                            </Alert>
                        )}
                        <Group justify="flex-end">
                            <Button variant="default" onClick={() => setModalOpen(false)}>取消</Button>
                            <Button variant="light" leftSection={<IconPlugConnected size={16} />} loading={busy === "draft-test"} onClick={handleDraftTest}>测试连接</Button>
                            <Button onClick={handleSave} loading={busy === "save"}>保存</Button>
                        </Group>
                    </Stack>
                </Modal>
            </Stack>
        </Layout>
    );
}

