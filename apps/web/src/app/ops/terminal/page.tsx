'use client';

import React, { useCallback, useEffect, useRef, useState } from 'react';
import {
    ActionIcon,
    Alert,
    Badge,
    Button,
    Center,
    Grid,
    Group,
    Loader,
    Modal,
    Stack,
    Table,
    Text,
    Title,
    Tooltip,
} from '@mantine/core';
import {
    IconAlertCircle,
    IconPencil,
    IconPlayerPlay,
    IconPlugOff,
    IconPlus,
    IconRefresh,
    IconScan,
    IconTrash,
} from '@tabler/icons-react';
import { modals } from '@mantine/modals';
import { notifications } from '@mantine/notifications';
import Layout from '@/components/Layout';
import SshTerminalPanel from '@/components/Ops/SshTerminalPanel';
import SshTargetFormFields, {
    createEmptyForm,
    formToInput,
    viewToForm,
    type SshFormState,
} from '@/components/Ops/SshTargetFormFields';
import {
    closeSshSession,
    createSshTarget,
    deleteSshTarget,
    getSshSessions,
    getSshTargets,
    probeSshTarget,
    updateSshTarget,
    type SshSessionView,
    type SshTargetView,
} from '@/services/sshService';

function describeError(err: unknown): string {
    return err instanceof Error ? err.message : '请求失败';
}

export default function OpsTerminalPage() {
    const [targets, setTargets] = useState<SshTargetView[]>([]);
    const [sessions, setSessions] = useState<SshSessionView[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [selected, setSelected] = useState<SshTargetView | null>(null);
    const [modalOpen, setModalOpen] = useState(false);
    const [editingId, setEditingId] = useState<string | null>(null);
    const [form, setForm] = useState<SshFormState>(createEmptyForm());
    const [saving, setSaving] = useState(false);
    const [busyId, setBusyId] = useState<string | null>(null);

    const loadTargets = useCallback(async () => {
        const list = await getSshTargets();
        setTargets(list);
        return list;
    }, []);

    const loadSessions = useCallback(async () => {
        try {
            setSessions(await getSshSessions());
        } catch {
            // 会话列表失败不阻塞主机管理
        }
    }, []);

    const loadData = useCallback(async () => {
        try {
            setLoading(true);
            setError(null);
            await Promise.all([loadTargets(), loadSessions()]);
        } catch (err) {
            setError(describeError(err));
        } finally {
            setLoading(false);
        }
    }, [loadSessions, loadTargets]);

    useEffect(() => {
        void loadData();
    }, [loadData]);

    // 支持 /ops/terminal?target=<id> 直接进终端（只处理一次）
    const deepLinkHandled = useRef(false);
    useEffect(() => {
        if (deepLinkHandled.current || loading || targets.length === 0) return;
        deepLinkHandled.current = true;
        const targetId = new URLSearchParams(window.location.search).get('target');
        if (!targetId) return;
        const match = targets.find((item) => item.id === targetId);
        if (match) {
            setSelected(match);
        } else {
            notifications.show({ color: 'orange', title: '未找到主机', message: `target=${targetId}` });
        }
    }, [loading, targets]);

    const openCreate = () => {
        setEditingId(null);
        setForm(createEmptyForm());
        setModalOpen(true);
    };

    const openEdit = (view: SshTargetView) => {
        setEditingId(view.id);
        setForm(viewToForm(view));
        setModalOpen(true);
    };

    const submit = async () => {
        if (!form.name.trim() || !form.host.trim() || !form.username.trim()) {
            notifications.show({ color: 'orange', title: '请填写必填项', message: '名称、主机地址、登录用户不能为空' });
            return;
        }
        if (!editingId && !form.keepCredential) {
            const hasSecret = form.authType === 'password'
                ? form.password.length > 0
                : form.privateKeyPem.trim().length > 0;
            if (!hasSecret) {
                notifications.show({ color: 'orange', title: '缺少凭证', message: '新建主机必须提供密码或私钥' });
                return;
            }
        }
        try {
            setSaving(true);
            const input = formToInput(form);
            if (editingId) {
                await updateSshTarget(editingId, input);
                notifications.show({ color: 'teal', title: '已保存', message: '主机配置已更新' });
            } else {
                await createSshTarget(input);
                notifications.show({ color: 'teal', title: '已创建', message: '主机已添加到运维终端' });
            }
            setModalOpen(false);
            const list = await loadTargets();
            if (selected) {
                setSelected(list.find((item) => item.id === selected.id) ?? null);
            }
        } catch (err) {
            notifications.show({ color: 'red', title: '保存失败', message: describeError(err) });
        } finally {
            setSaving(false);
        }
    };

    const removeTarget = (view: SshTargetView) => {
        modals.openConfirmModal({
            title: '删除主机',
            children: (
                <Text size="sm">
                    确认删除「{view.name}」？删除后其已保存凭证一并失效；若仍有活动会话，后端会拒绝删除。
                </Text>
            ),
            labels: { confirm: '删除', cancel: '取消' },
            confirmProps: { color: 'red' },
            onConfirm: async () => {
                try {
                    await deleteSshTarget(view.id);
                    notifications.show({ color: 'teal', title: '已删除', message: view.name });
                    if (selected?.id === view.id) setSelected(null);
                    await loadTargets();
                } catch (err) {
                    notifications.show({ color: 'red', title: '删除失败', message: describeError(err) });
                }
            },
        });
    };

    /** 用已保存凭证扫描指纹（不建连、不落库） */
    const probeSaved = async (view: SshTargetView) => {
        try {
            setBusyId(view.id);
            const result = await probeSshTarget(view.id);
            const changed = result.matched === false;
            notifications.show({
                color: changed ? 'orange' : 'teal',
                title: '指纹扫描完成',
                message: `${view.name}：${result.fingerprint}（${result.key_type}，${result.latency_ms}ms）${changed ? '，与记录不一致' : ''}`,
            });
            await loadTargets();
            if (selected?.id === view.id) {
                setSelected((prev) => (prev ? { ...prev, host_key_fingerprint: result.fingerprint } : prev));
            }
        } catch (err) {
            notifications.show({ color: 'red', title: '指纹扫描失败', message: describeError(err) });
        } finally {
            setBusyId(null);
        }
    };

    const closeSession = async (sessionId: string) => {
        try {
            setBusyId(sessionId);
            await closeSshSession(sessionId);
            await loadSessions();
        } catch (err) {
            notifications.show({ color: 'red', title: '关闭会话失败', message: describeError(err) });
        } finally {
            setBusyId(null);
        }
    };

    return (
        <Layout>
            <Stack gap="md">
                <Group justify="space-between">
                    <div>
                        <Title order={3}>运维终端</Title>
                        <Text size="sm" c="dimmed">
                            SSH 会话全程审计；凭证加密存储，主机指纹默认严格校验。
                        </Text>
                    </div>
                    <Group gap="xs">
                        <Button
                            variant="default"
                            leftSection={<IconRefresh size={16} />}
                            onClick={() => void loadData()}
                        >
                            刷新
                        </Button>
                        <Button leftSection={<IconPlus size={16} />} onClick={openCreate}>
                            新增主机
                        </Button>
                    </Group>
                </Group>

                {error && (
                    <Alert color="red" icon={<IconAlertCircle size={16} />} title="加载失败">
                        {error}
                    </Alert>
                )}

                {loading ? (
                    <Center py="xl"><Loader /></Center>
                ) : (
                    <Grid>
                        <Grid.Col span={{ base: 12, lg: 5 }}>
                            <Stack gap="md">
                                <SessionsTable
                                    sessions={sessions}
                                    busyId={busyId}
                                    onClose={closeSession}
                                />
                                <Table highlightOnHover withTableBorder verticalSpacing="xs">
                                    <Table.Thead>
                                        <Table.Tr>
                                            <Table.Th>主机</Table.Th>
                                            <Table.Th>用户</Table.Th>
                                            <Table.Th>状态</Table.Th>
                                            <Table.Th ta="right">操作</Table.Th>
                                        </Table.Tr>
                                    </Table.Thead>
                                    <Table.Tbody>
                                        {targets.length === 0 && (
                                            <Table.Tr>
                                                <Table.Td colSpan={4}>
                                                    <Text size="sm" c="dimmed" ta="center" py="md">
                                                        还没有主机，点击「新增主机」添加。
                                                    </Text>
                                                </Table.Td>
                                            </Table.Tr>
                                        )}
                                        {targets.map((view) => (
                                            <Table.Tr
                                                key={view.id}
                                                bg={selected?.id === view.id
                                                    ? 'var(--mantine-color-blue-light)'
                                                    : undefined}
                                            >
                                                <Table.Td>
                                                    <Stack gap={2}>
                                                        <Text size="sm" fw={600}>{view.name}</Text>
                                                        <Text size="xs" c="dimmed">
                                                            {view.host}:{view.port}
                                                            {view.strict_host_key_check ? ' · 严格指纹' : ' · 宽松指纹'}
                                                        </Text>
                                                    </Stack>
                                                </Table.Td>
                                                <Table.Td>
                                                    <Text size="sm">{view.username}</Text>
                                                    <Text size="xs" c="dimmed">
                                                        {view.auth_type === 'private_key' ? '私钥' : '密码'}
                                                        {view.has_credential ? '' : ' · 未配置'}
                                                    </Text>
                                                </Table.Td>
                                                <Table.Td>
                                                    <Badge color={view.enabled ? 'teal' : 'gray'} variant="light">
                                                        {view.enabled ? '启用' : '停用'}
                                                    </Badge>
                                                    {view.last_error && (
                                                        <Text size="xs" c="red" lineClamp={1}>{view.last_error}</Text>
                                                    )}
                                                </Table.Td>
                                                <Table.Td>
                                                    <Group gap={4} justify="flex-end" wrap="nowrap">
                                                        <Tooltip label="连接">
                                                            <ActionIcon
                                                                variant="light"
                                                                aria-label="连接"
                                                                disabled={!view.enabled}
                                                                onClick={() => setSelected(view)}
                                                            >
                                                                <IconPlayerPlay size={16} />
                                                            </ActionIcon>
                                                        </Tooltip>
                                                        <Tooltip label="扫描指纹">
                                                            <ActionIcon
                                                                variant="light"
                                                                aria-label="扫描指纹"
                                                                loading={busyId === view.id}
                                                                onClick={() => void probeSaved(view)}
                                                            >
                                                                <IconScan size={16} />
                                                            </ActionIcon>
                                                        </Tooltip>
                                                        <Tooltip label="编辑">
                                                            <ActionIcon
                                                                variant="light"
                                                                aria-label="编辑"
                                                                onClick={() => openEdit(view)}
                                                            >
                                                                <IconPencil size={16} />
                                                            </ActionIcon>
                                                        </Tooltip>
                                                        <Tooltip label="删除">
                                                            <ActionIcon
                                                                variant="light"
                                                                color="red"
                                                                aria-label="删除"
                                                                onClick={() => removeTarget(view)}
                                                            >
                                                                <IconTrash size={16} />
                                                            </ActionIcon>
                                                        </Tooltip>
                                                    </Group>
                                                </Table.Td>
                                            </Table.Tr>
                                        ))}
                                    </Table.Tbody>
                                </Table>
                            </Stack>
                        </Grid.Col>

                        <Grid.Col span={{ base: 12, lg: 7 }}>
                            <div style={{ height: 'calc(100vh - 220px)', minHeight: 420 }}>
                                <SshTerminalPanel
                                    target={selected}
                                    onSessionChange={() => void loadSessions()}
                                />
                            </div>
                        </Grid.Col>
                    </Grid>
                )}
            </Stack>


            <Modal
                opened={modalOpen}
                onClose={() => setModalOpen(false)}
                title={editingId ? '编辑主机' : '新增主机'}
                size="lg"
                centered
            >
                <Stack gap="md">
                    <SshTargetFormFields value={form} onChange={setForm} editing={editingId !== null} />
                    <Group justify="flex-end">
                        <Button variant="default" onClick={() => setModalOpen(false)}>取消</Button>
                        <Button loading={saving} onClick={() => void submit()}>保存</Button>
                    </Group>
                </Stack>
            </Modal>
        </Layout>
    );
}

interface SessionsTableProps {
    sessions: SshSessionView[];
    busyId: string | null;
    onClose: (sessionId: string) => Promise<void>;
}

/** 活动会话列表（无会话时不占版面） */
function SessionsTable({ sessions, busyId, onClose }: SessionsTableProps) {
    if (sessions.length === 0) return null;
    return (
        <Stack gap="xs">
            <Text size="sm" fw={600}>活动会话（{sessions.length}）</Text>
            <Table withTableBorder verticalSpacing="xs" highlightOnHover>
                <Table.Thead>
                    <Table.Tr>
                        <Table.Th>主机</Table.Th>
                        <Table.Th>来源</Table.Th>
                        <Table.Th>流量</Table.Th>
                        <Table.Th ta="right">操作</Table.Th>
                    </Table.Tr>
                </Table.Thead>
                <Table.Tbody>
                    {sessions.map((item) => (
                        <Table.Tr key={item.id}>
                            <Table.Td>
                                <Text size="sm">{item.target_name}</Text>
                                <Text size="xs" c="dimmed">
                                    {item.id.slice(0, 8)} · {item.state}
                                </Text>
                            </Table.Td>
                            <Table.Td>
                                <Text size="xs">{item.client_ip ?? '—'}</Text>
                                <Text size="xs" c="dimmed">{item.started_at ?? '—'}</Text>
                            </Table.Td>
                            <Table.Td>
                                <Text size="xs">
                                    ↑{item.bytes_in} / ↓{item.bytes_out}
                                </Text>
                            </Table.Td>
                            <Table.Td ta="right">
                                <Tooltip label="强制关闭">
                                    <ActionIcon
                                        variant="light"
                                        color="red"
                                        aria-label="强制关闭"
                                        loading={busyId === item.id}
                                        disabled={item.state !== 'active'}
                                        onClick={() => void onClose(item.id)}
                                    >
                                        <IconPlugOff size={16} />
                                    </ActionIcon>
                                </Tooltip>
                            </Table.Td>
                        </Table.Tr>
                    ))}
                </Table.Tbody>
            </Table>
        </Stack>
    );
}

