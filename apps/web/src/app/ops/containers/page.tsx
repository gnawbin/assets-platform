'use client';

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import {
    Alert,
    Badge,
    Button,
    Card,
    Center,
    Group,
    Loader,
    Select,
    Stack,
    Table,
    Text,
    TextInput,
    Title,
    Tooltip,
} from '@mantine/core';
import { IconAlertTriangle, IconRefresh, IconSearch } from '@tabler/icons-react';

import Layout from '@/components/Layout';
import ContainerDetailDrawer, {
    type ContainerDetailTab,
} from '@/components/Ops/ContainerDetailDrawer';
import { workTargetFromLink, type ContainerWorkTarget } from '@/components/Ops/containerTarget';
import {
    getOpsConnections,
    getOpsLinks,
    type OpsConnectionView,
    type OpsResourceLinkView,
} from '@/services/opsService';

const STATUS_LABEL: Record<string, string> = {
    running: '运行中',
    unknown: '未知',
    offline: '已停止',
    gone: '本次未发现',
    frozen: '冻结',
};

function readQuery(): { connectionId: string | null; uid: string | null; tab: ContainerDetailTab } {
    if (typeof window === 'undefined') {
        return { connectionId: null, uid: null, tab: 'logs' };
    }
    const params = new URLSearchParams(window.location.search);
    const tab = params.get('tab');
    return {
        connectionId: params.get('connectionId'),
        uid: params.get('uid'),
        tab: tab === 'stats' || tab === 'terminal' ? tab : 'logs',
    };
}

export default function OpsContainersPage() {
    const router = useRouter();
    const [links, setLinks] = useState<OpsResourceLinkView[]>([]);
    const [connections, setConnections] = useState<OpsConnectionView[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [filterConnection, setFilterConnection] = useState<string | null>(null);
    const [filterProvider, setFilterProvider] = useState<string | null>(null);
    const [filterKind, setFilterKind] = useState<string | null>(null);
    const [keyword, setKeyword] = useState('');
    const [selectedUid, setSelectedUid] = useState<string | null>(null);
    const [initialTab, setInitialTab] = useState<ContainerDetailTab>('logs');
    const [invalidUid, setInvalidUid] = useState(false);

    // 首次进入：读取深链参数
    useEffect(() => {
        const query = readQuery();
        setSelectedUid(query.uid);
        setInitialTab(query.tab);
        setFilterConnection(query.connectionId);
    }, []);

    const loadData = useCallback(async () => {
        try {
            setLoading(true);
            setError(null);
            const [rows, conns] = await Promise.all([getOpsLinks(), getOpsConnections()]);
            setLinks(rows.filter((row) => row.resource_kind === 'container' || row.resource_kind === 'pod'));
            setConnections(conns);
        } catch (err) {
            setError(err instanceof Error ? err.message : '加载容器列表失败');
        } finally {
            setLoading(false);
        }
    }, []);

    useEffect(() => {
        void loadData();
    }, [loadData]);

    const targets = useMemo(
        () =>
            links
                .map((link) => workTargetFromLink(link, connections))
                .filter((item): item is ContainerWorkTarget => item !== null),
        [links, connections],
    );

    const selectedTarget = useMemo(
        () => targets.find((item) => item.uid === selectedUid) ?? null,
        [targets, selectedUid],
    );

    // uid 存在但解析不到：顶部提示，不打开抽屉
    useEffect(() => {
        if (!selectedUid) {
            setInvalidUid(false);
            return;
        }
        if (!loading && !selectedTarget) {
            setInvalidUid(true);
        }
    }, [loading, selectedTarget, selectedUid]);

    const filtered = useMemo(() => {
        const text = keyword.trim().toLowerCase();
        return links.filter((link) => {
            if (filterConnection && link.connection_id !== filterConnection) return false;
            if (filterProvider && link.provider !== filterProvider) return false;
            if (filterKind && link.resource_kind !== filterKind) return false;
            if (!text) return true;
            return (
                (link.display_name ?? '').toLowerCase().includes(text) ||
                link.resource_uid.toLowerCase().includes(text)
            );
        });
    }, [filterConnection, filterKind, filterProvider, keyword, links]);

    const syncUrl = (uid: string | null, tab: ContainerDetailTab) => {
        const params = new URLSearchParams();
        if (filterConnection) params.set('connectionId', filterConnection);
        if (uid) {
            params.set('uid', uid);
            params.set('tab', tab);
        }
        const query = params.toString();
        router.replace(query ? `/ops/containers?${query}` : '/ops/containers');
    };

    const openDrawer = (uid: string, tab: ContainerDetailTab) => {
        setSelectedUid(uid);
        setInitialTab(tab);
        setInvalidUid(false);
        syncUrl(uid, tab);
    };

    const closeDrawer = () => {
        setSelectedUid(null);
        setInvalidUid(false);
        syncUrl(null, initialTab);
    };

    const kindOptions = useMemo(
        () =>
            Array.from(new Set(links.map((item) => item.resource_kind))).map((kind) => ({
                value: kind,
                label: kind === 'container' ? 'Docker 容器' : 'K8s Pod',
            })),
        [links],
    );

    return (
        <Layout>
            <Stack gap="md">
                <Group justify="space-between">
                    <div>
                        <Title order={3}>容器运维</Title>
                        <Text size="sm" c="dimmed">
                            按需查看容器 / Pod 的日志、性能，或在允许的连接上打开交互终端
                        </Text>
                    </div>
                    <Button
                        variant="light"
                        leftSection={<IconRefresh size={16} />}
                        onClick={() => void loadData()}
                        loading={loading}
                    >
                        刷新
                    </Button>
                </Group>

                {invalidUid && (
                    <Alert color="yellow" icon={<IconAlertTriangle size={16} />}>
                        资源标识无法解析：请从列表重新选择容器 / Pod。
                    </Alert>
                )}
                {error && (
                    <Alert color="red" icon={<IconAlertTriangle size={16} />}>
                        {error}
                    </Alert>
                )}

                <Card withBorder>
                    <Group align="flex-end" wrap="wrap">
                        <Select
                            label="连接"
                            placeholder="全部连接"
                            data={connections.map((item) => ({ value: item.id, label: item.name }))}
                            value={filterConnection}
                            onChange={setFilterConnection}
                            clearable
                            w={220}
                        />
                        <Select
                            label="来源"
                            placeholder="全部来源"
                            data={[
                                { value: 'docker', label: 'Docker' },
                                { value: 'k8s', label: 'Kubernetes' },
                            ]}
                            value={filterProvider}
                            onChange={setFilterProvider}
                            clearable
                            w={160}
                        />
                        <Select
                            label="类型"
                            placeholder="全部类型"
                            data={kindOptions}
                            value={filterKind}
                            onChange={setFilterKind}
                            clearable
                            w={160}
                        />
                        <TextInput
                            label="关键字"
                            placeholder="名称 / uid"
                            leftSection={<IconSearch size={14} />}
                            value={keyword}
                            onChange={(event) => setKeyword(event.currentTarget.value)}
                            w={240}
                        />
                    </Group>
                </Card>

                <Card withBorder>
                    {loading ? (
                        <Center py="xl">
                            <Loader size="sm" />
                        </Center>
                    ) : filtered.length === 0 ? (
                        <Center py="xl">
                            <Text c="dimmed">暂无可用容器 / Pod，请先在「连接管理」中采集一次</Text>
                        </Center>
                    ) : (
                        <Table.ScrollContainer minWidth={1000}>
                            <Table striped highlightOnHover withTableBorder>
                                <Table.Thead>
                                    <Table.Tr>
                                        <Table.Th>资源</Table.Th>
                                        <Table.Th>来源</Table.Th>
                                        <Table.Th>类型</Table.Th>
                                        <Table.Th>状态</Table.Th>
                                        <Table.Th>详情</Table.Th>
                                        <Table.Th>连接</Table.Th>
                                        <Table.Th>最近发现</Table.Th>
                                        <Table.Th>操作</Table.Th>
                                    </Table.Tr>
                                </Table.Thead>
                                <Table.Tbody>

                                    {filtered.map((link) => {
                                        const connection = connections.find((item) => item.id === link.connection_id);
                                        const attributes = link.attributes ?? {};
                                        const imageId = typeof attributes.image_id === 'string'
                                            ? attributes.image_id.replace(/^sha256:/, '').slice(0, 12)
                                            : null;
                                        const nodeName = typeof attributes.node_name === 'string'
                                            ? attributes.node_name
                                            : null;
                                        const restartCount = typeof attributes.restart_count === 'number'
                                            ? attributes.restart_count
                                            : null;
                                        const containerCount = typeof attributes.container_count === 'number'
                                            ? attributes.container_count
                                            : null;
                                        return (
                                            <Table.Tr key={link.id}>
                                                <Table.Td>
                                                    <Text fw={500}>{link.display_name ?? link.resource_uid}</Text>
                                                    <Tooltip label={link.resource_uid} multiline w={420}>
                                                        <Text size="xs" c="dimmed" lineClamp={1}>{link.resource_uid}</Text>
                                                    </Tooltip>
                                                </Table.Td>
                                                <Table.Td><Badge variant="light">{link.provider}</Badge></Table.Td>
                                                <Table.Td>
                                                    <Badge color={link.resource_kind === 'container' ? 'blue' : 'violet'} variant="light">
                                                        {link.resource_kind}
                                                    </Badge>
                                                </Table.Td>
                                                <Table.Td>
                                                    <Badge color={link.status_snapshot === 'running' ? 'green' : 'gray'}>
                                                        {STATUS_LABEL[link.status_snapshot ?? 'unknown'] ?? link.status_snapshot ?? '未知'}
                                                    </Badge>
                                                </Table.Td>
                                                <Table.Td>
                                                    {link.provider === 'docker' ? (
                                                        <Text size="xs" c="dimmed">镜像 {imageId ?? '—'}</Text>
                                                    ) : (
                                                        <Text size="xs" c="dimmed">
                                                            节点 {nodeName ?? '—'} · 重启 {restartCount ?? '—'} · 容器 {containerCount ?? '—'}
                                                        </Text>
                                                    )}
                                                </Table.Td>
                                                <Table.Td>
                                                    <Text size="xs">{connection?.name ?? link.connection_name ?? link.connection_id}</Text>
                                                </Table.Td>
                                                <Table.Td>
                                                    <Text size="xs">
                                                        {link.last_seen_at ? new Date(link.last_seen_at).toLocaleString() : '—'}
                                                    </Text>
                                                </Table.Td>
                                                <Table.Td>
                                                    <Group gap={4} wrap="nowrap">
                                                        <Button size="compact-xs" variant="light" onClick={() => openDrawer(link.resource_uid, 'logs')}>
                                                            日志
                                                        </Button>
                                                        <Button size="compact-xs" variant="light" onClick={() => openDrawer(link.resource_uid, 'stats')}>
                                                            性能
                                                        </Button>
                                                        {connection?.exec_enabled && (
                                                            <Button
                                                                size="compact-xs"
                                                                variant="light"
                                                                color="teal"
                                                                onClick={() => openDrawer(link.resource_uid, 'terminal')}
                                                            >
                                                                终端
                                                            </Button>
                                                        )}
                                                    </Group>
                                                </Table.Td>
                                            </Table.Tr>
                                        );
                                    })}

                                </Table.Tbody>
                            </Table>
                        </Table.ScrollContainer>
                    )}
                </Card>

                <ContainerDetailDrawer
                    opened={!!selectedUid && !!selectedTarget}
                    target={selectedTarget}
                    initialTab={initialTab}
                    onClose={closeDrawer}
                    onTabChange={(tab) => {
                        setInitialTab(tab);
                        if (selectedUid) syncUrl(selectedUid, tab);
                    }}
                />
            </Stack>
        </Layout>
    );
}

