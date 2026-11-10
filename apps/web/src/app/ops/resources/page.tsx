'use client';

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import Layout from '@/components/Layout';
import AssetSelect from '@/components/AssetSelect';
import {
    Alert,
    Badge,
    Button,
    Card,
    Center,
    Group,
    Loader,
    Modal,
    Select,
    SimpleGrid,
    Stack,
    Switch,
    Table,
    Text,
    TextInput,
    Title,
    Tooltip,
} from '@mantine/core';
import { IconArchive, IconRefresh } from '@tabler/icons-react';
import { notifications } from '@mantine/notifications';
import {
    bindOpsLink,
    getOpsConnections,
    getOpsLinks,
    type OpsConnectionView,
    type OpsResourceLinkView,
} from '@/services/opsService';

const STATUS_LABEL: Record<string, string> = {
    running: '运行中',
    under_repair: '维修中',
    pending_change: '待变更',
    pending_offline: '待下线',
    offline: '已下线',
    scrapped: '已报废',
    alarm_muted: '告警屏蔽',
    frozen: '冻结',
    unknown: '未知',
    gone: '本次未发现',
};

export default function OpsResourcesPage() {
    const [links, setLinks] = useState<OpsResourceLinkView[]>([]);
    const [connections, setConnections] = useState<OpsConnectionView[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [filterConnection, setFilterConnection] = useState<string | null>(null);
    const [filterKind, setFilterKind] = useState<string | null>(null);
    const [onlyUnbound, setOnlyUnbound] = useState(false);
    const [keyword, setKeyword] = useState('');
    const [bindTarget, setBindTarget] = useState<OpsResourceLinkView | null>(null);
    const [bindAssetId, setBindAssetId] = useState<string | null>(null);
    const [binding, setBinding] = useState(false);

    const loadData = useCallback(async () => {
        try {
            setLoading(true);
            setError(null);
            const [rows, conns] = await Promise.all([getOpsLinks(filterConnection), getOpsConnections()]);
            setLinks(rows);
            setConnections(conns);
        } catch (err) {
            setError(err instanceof Error ? err.message : '加载失败');
        } finally {
            setLoading(false);
        }
    }, [filterConnection]);

    useEffect(() => {
        loadData();
    }, [loadData]);

    const kindOptions = useMemo(() => {
        const kinds = Array.from(new Set(links.map((item) => item.resource_kind))).sort();
        return kinds.map((kind) => ({ value: kind, label: kind }));
    }, [links]);

    const filtered = useMemo(() => {
        const text = keyword.trim().toLowerCase();
        return links.filter((item) => {
            if (filterKind && item.resource_kind !== filterKind) return false;
            if (onlyUnbound && item.asset_id) return false;
            if (!text) return true;
            return (item.display_name ?? '').toLowerCase().includes(text) || item.resource_uid.toLowerCase().includes(text);
        });
    }, [links, filterKind, onlyUnbound, keyword]);

    const handleBind = async () => {
        if (!bindTarget || !bindAssetId) {
            notifications.show({ color: 'red', title: '请先选择要绑定的资产', message: '' });
            return;
        }
        try {
            setBinding(true);
            await bindOpsLink(bindTarget.id, bindAssetId);
            notifications.show({ color: 'green', title: '已归档到资产', message: '' });
            setBindTarget(null);
            setBindAssetId(null);
            await loadData();
        } catch (err) {
            notifications.show({ color: 'red', title: '绑定失败', message: err instanceof Error ? err.message : String(err) });
        } finally {
            setBinding(false);
        }
    };

    const unboundCount = links.filter((item) => !item.asset_id).length;
    const shortLivedCount = links.filter((item) => ['container', 'pod', 'swarm_task', 'job'].includes(item.resource_kind)).length;

    return (
        <Layout>
            <Stack gap="lg">
                <Group justify="space-between">
                    <Title order={3}>运维采集 · 资源清单</Title>
                    <Button variant="default" leftSection={<IconRefresh size={16} />} onClick={loadData} loading={loading}>刷新</Button>
                </Group>

                {error && <Alert color="red" title="加载失败">{error}</Alert>}

                <Group gap="sm" align="flex-end">
                    <Select label="连接" placeholder="全部连接" clearable value={filterConnection} onChange={setFilterConnection}
                        data={connections.map((item) => ({ value: item.id, label: item.name }))} w={220} />
                    <Select label="资源类型" placeholder="全部类型" clearable value={filterKind} onChange={setFilterKind} data={kindOptions} w={180} />
                    <TextInput label="搜索" placeholder="名称或 uid" value={keyword} onChange={(event) => setKeyword(event.currentTarget.value)} w={220} />
                    <Switch label="只看未归档" checked={onlyUnbound} onChange={(event) => setOnlyUnbound(event.currentTarget.checked)} mb={6} />
                </Group>

                <SimpleGrid cols={{ base: 2, md: 3 }}>
                    <Card withBorder><Text size="xs" c="dimmed">观测资源</Text><Text fw={700} size="xl">{links.length}</Text></Card>
                    <Card withBorder><Text size="xs" c="dimmed">短时动态资产</Text><Text fw={700} size="xl">{shortLivedCount}</Text></Card>
                    <Card withBorder><Text size="xs" c="dimmed">未归档</Text><Text fw={700} size="xl" c={unboundCount ? "orange" : undefined}>{unboundCount}</Text></Card>
                </SimpleGrid>

                <Card withBorder p="sm">
                    {loading ? (
                        <Center py="xl"><Loader size="sm" /></Center>
                    ) : filtered.length === 0 ? (
                        <Center py="xl"><Text c="dimmed">暂无资源，请先在「连接管理」里执行一次采集</Text></Center>
                    ) : (
                        <Table.ScrollContainer minWidth={1200}>
                            <Table striped highlightOnHover withTableBorder>
                                <Table.Thead>
                                    <Table.Tr>
                                        <Table.Th>资源</Table.Th>
                                        <Table.Th>类型</Table.Th>
                                        <Table.Th>状态</Table.Th>
                                        <Table.Th>原始状态</Table.Th>
                                        <Table.Th>父资源</Table.Th>
                                        <Table.Th>最近发现</Table.Th>
                                        <Table.Th>连接</Table.Th>
                                        <Table.Th>归档</Table.Th>
                                    </Table.Tr>
                                </Table.Thead>
                                <Table.Tbody>

                                    {filtered.map((row) => (
                                        <Table.Tr key={row.id}>
                                            <Table.Td>
                                                <Text fw={500}>{row.display_name ?? row.resource_uid}</Text>
                                                <Tooltip label={row.resource_uid} multiline w={420}>
                                                    <Text size="xs" c="dimmed" lineClamp={1}>{row.resource_uid}</Text>
                                                </Tooltip>
                                            </Table.Td>
                                            <Table.Td><Badge variant="light">{row.resource_kind}</Badge></Table.Td>
                                            <Table.Td>
                                                <Badge color={row.status_snapshot === "running" ? "green" : row.status_snapshot === "gone" ? "gray" : "orange"}>
                                                    {STATUS_LABEL[row.status_snapshot ?? "unknown"] ?? row.status_snapshot}
                                                </Badge>
                                            </Table.Td>

                                            <Table.Td><Text size="xs">{row.raw_state ?? "—"}</Text></Table.Td>
                                            <Table.Td>
                                                {row.parent_uid ? (
                                                    <Tooltip label={row.parent_uid} multiline w={420}>
                                                        <Text size="xs" c="dimmed" lineClamp={1}>{row.parent_uid}</Text>
                                                    </Tooltip>
                                                ) : (
                                                    <Text size="xs" c="dimmed">—</Text>
                                                )}
                                            </Table.Td>
                                            <Table.Td><Text size="xs">{row.last_seen_at ? new Date(row.last_seen_at).toLocaleString() : "—"}</Text></Table.Td>
                                            <Table.Td><Text size="xs">{row.connection_name ?? row.connection_id}</Text></Table.Td>
                                            <Table.Td>
                                                {row.asset_id ? (
                                                    <Badge color="green" variant="light">{row.asset_no ?? row.asset_id}</Badge>
                                                ) : (
                                                    <Button size="compact-xs" variant="light" leftSection={<IconArchive size={14} />} onClick={() => { setBindTarget(row); setBindAssetId(null); }}>绑定资产</Button>
                                                )}
                                            </Table.Td>
                                        </Table.Tr>
                                    ))}
                                </Table.Tbody>
                            </Table>
                        </Table.ScrollContainer>
                    )}
                </Card>

                <Modal opened={!!bindTarget} onClose={() => setBindTarget(null)} title="绑定到资产台账" size="xl">
                    <Stack gap="md">
                        <Text size="sm" c="dimmed">资源：{bindTarget?.display_name ?? bindTarget?.resource_uid}</Text>
                        <AssetSelect mode="single" label="选择资产" value={bindAssetId} onChange={setBindAssetId} />
                        <Group justify="flex-end">
                            <Button variant="default" onClick={() => setBindTarget(null)}>取消</Button>
                            <Button onClick={handleBind} loading={binding}>确认绑定</Button>
                        </Group>
                    </Stack>
                </Modal>
            </Stack>
        </Layout>
    );
}

