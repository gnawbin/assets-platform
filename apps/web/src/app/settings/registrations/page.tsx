'use client';

import React, { useState, useEffect } from 'react';
import {
    Title,
    Text,
    Card,
    Table,
    Badge,
    Button,
    Group,
    Stack,
    Modal,
    TextInput,
    Textarea,
    Select,
    ActionIcon,
    Tooltip,
    LoadingOverlay,
    Alert,
    Pagination,
} from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { notifications } from '@mantine/notifications';
import { IconCheck, IconX, IconEye, IconRefresh } from '@tabler/icons-react';
import Layout from '@/components/Layout';
import {
    getRegistrations,
    approveRegistration,
    rejectRegistration,
    RegisterResponse,
} from '@/services/registerService';
import { getTenants, Tenant } from '@/services/tenantService';
import { useAuthStore } from '@/store/authStore';

const statusMap: Record<number, { label: string; color: string }> = {
    0: { label: '待审核', color: 'yellow' },
    1: { label: '已通过', color: 'green' },
    2: { label: '已驳回', color: 'red' },
};

const RegistrationsPage: React.FC = () => {
    const [registrations, setRegistrations] = useState<RegisterResponse[]>([]);
    const [tenants, setTenants] = useState<Tenant[]>([]);
    const [loading, setLoading] = useState(false);
    const [statusFilter, setStatusFilter] = useState<string | null>(null);
    const [selected, setSelected] = useState<RegisterResponse | null>(null);
    const [opened, { open, close }] = useDisclosure(false);
    const [rejectOpened, { open: openReject, close: closeReject }] = useDisclosure(false);
    const [approveRemark, setApproveRemark] = useState('');
    const [rejectRemark, setRejectRemark] = useState('');
    const [selectedTenantId, setSelectedTenantId] = useState<string | null>(null);
    const [submitting, setSubmitting] = useState(false);
    const user = useAuthStore((s) => s.user);

    const fetchData = async () => {
        setLoading(true);
        try {
            const [regs, tenantList] = await Promise.all([
                getRegistrations(statusFilter ? Number(statusFilter) : undefined),
                getTenants(),
            ]);
            setRegistrations(regs);
            setTenants(tenantList);
        } catch (e: any) {
            notifications.show({ title: '加载失败', message: e.message || String(e), color: 'red' });
        } finally {
            setLoading(false);
        }
    };

    useEffect(() => {
        fetchData();
    }, [statusFilter]);

    const handleApprove = async () => {
        if (!selected || !selectedTenantId) return;
        setSubmitting(true);
        try {
            await approveRegistration(
                selected.id,
                String(user?.id ?? ''),
                selectedTenantId,
                approveRemark || undefined
            );
            notifications.show({ title: '审核通过', message: '注册申请已通过', color: 'green' });
            close();
            setApproveRemark('');
            setSelectedTenantId(null);
            fetchData();
        } catch (e: any) {
            notifications.show({ title: '操作失败', message: e.message || String(e), color: 'red' });
        } finally {
            setSubmitting(false);
        }
    };

    const handleReject = async () => {
        if (!selected) return;
        setSubmitting(true);
        try {
            await rejectRegistration(selected.id, String(user?.id ?? ''), rejectRemark || undefined);
            notifications.show({ title: '已驳回', message: '注册申请已驳回', color: 'orange' });
            closeReject();
            setRejectRemark('');
            fetchData();
        } catch (e: any) {
            notifications.show({ title: '操作失败', message: e.message || String(e), color: 'red' });
        } finally {
            setSubmitting(false);
        }
    };

    const openApproveModal = (reg: RegisterResponse) => {
        setSelected(reg);
        setApproveRemark('');
        setSelectedTenantId(null);
        open();
    };

    const openRejectModal = (reg: RegisterResponse) => {
        setSelected(reg);
        setRejectRemark('');
        openReject();
    };

    const rows = registrations.map((reg) => (
        <Table.Tr key={reg.id}>
            <Table.Td>{reg.username}</Table.Td>
            <Table.Td>{reg.real_name}</Table.Td>
            <Table.Td>{reg.email || '-'}</Table.Td>
            <Table.Td>{reg.department_name || '-'}</Table.Td>
            <Table.Td>{reg.company_name || '-'}</Table.Td>
            <Table.Td>
                <Badge color={statusMap[reg.status]?.color || 'gray'}>
                    {statusMap[reg.status]?.label || '未知'}
                </Badge>
            </Table.Td>
            <Table.Td>{reg.created_at || '-'}</Table.Td>
            <Table.Td>
                <Group gap="xs">
                    {reg.status === 0 && (
                        <>
                            <Tooltip label="审核通过">
                                <ActionIcon color="green" variant="light" onClick={() => openApproveModal(reg)}>
                                    <IconCheck size={16} />
                                </ActionIcon>
                            </Tooltip>
                            <Tooltip label="驳回">
                                <ActionIcon color="red" variant="light" onClick={() => openRejectModal(reg)}>
                                    <IconX size={16} />
                                </ActionIcon>
                            </Tooltip>
                        </>
                    )}
                    {reg.approve_remark && (
                        <Tooltip label={reg.approve_remark}>
                            <ActionIcon color="gray" variant="light">
                                <IconEye size={16} />
                            </ActionIcon>
                        </Tooltip>
                    )}
                </Group>
            </Table.Td>
        </Table.Tr>
    ));

    return (
        <Layout>
            <Stack gap="lg" pos="relative">
                <LoadingOverlay visible={loading} />

                <Group justify="space-between">
                    <div>
                        <Title order={2}>注册审核</Title>
                        <Text c="dimmed">审核用户注册申请，通过后自动创建用户并分配租户</Text>
                    </div>
                    <Group>
                        <Select
                            placeholder="全部状态"
                            data={[
                                { value: '', label: '全部' },
                                { value: '0', label: '待审核' },
                                { value: '1', label: '已通过' },
                                { value: '2', label: '已驳回' },
                            ]}
                            value={statusFilter}
                            onChange={setStatusFilter}
                            clearable
                            w={140}
                        />
                        <Button variant="light" leftSection={<IconRefresh size={16} />} onClick={fetchData}>
                            刷新
                        </Button>
                    </Group>
                </Group>

                {registrations.length === 0 && !loading ? (
                    <Alert color="blue" title="暂无数据">
                        目前没有注册申请记录。
                    </Alert>
                ) : (
                    <Card withBorder padding="lg" radius="md">
                        <Table striped highlightOnHover>
                            <Table.Thead>
                                <Table.Tr>
                                    <Table.Th>用户名</Table.Th>
                                    <Table.Th>姓名</Table.Th>
                                    <Table.Th>邮箱</Table.Th>
                                    <Table.Th>部门</Table.Th>
                                    <Table.Th>公司</Table.Th>
                                    <Table.Th>状态</Table.Th>
                                    <Table.Th>申请时间</Table.Th>
                                    <Table.Th>操作</Table.Th>
                                </Table.Tr>
                            </Table.Thead>
                            <Table.Tbody>{rows}</Table.Tbody>
                        </Table>
                    </Card>
                )}

                {/* 审核通过弹窗 */}
                <Modal opened={opened} onClose={close} title="审核通过" centered>
                    <Stack gap="md">
                        <Text size="sm">
                            确认通过 <strong>{selected?.username}</strong> 的注册申请？
                        </Text>
                        <Select
                            label="分配组织结构"
                            placeholder="选择组织结构"
                            data={tenants.map((t) => ({ value: String(t.id), label: t.tenant_name }))}
                            value={selectedTenantId}
                            onChange={setSelectedTenantId}
                            required
                            searchable
                        />
                        <Textarea
                            label="审核备注"
                            placeholder="可选，填写审核备注"
                            value={approveRemark}
                            onChange={(e) => setApproveRemark(e.currentTarget.value)}
                        />
                        <Group justify="flex-end">
                            <Button variant="default" onClick={close}>
                                取消
                            </Button>
                            <Button color="green" onClick={handleApprove} loading={submitting} disabled={!selectedTenantId}>
                                确认通过
                            </Button>
                        </Group>
                    </Stack>
                </Modal>

                {/* 驳回弹窗 */}
                <Modal opened={rejectOpened} onClose={closeReject} title="驳回申请" centered>
                    <Stack gap="md">
                        <Text size="sm">
                            确认驳回 <strong>{selected?.username}</strong> 的注册申请？
                        </Text>
                        <Textarea
                            label="驳回原因"
                            placeholder="请填写驳回原因"
                            value={rejectRemark}
                            onChange={(e) => setRejectRemark(e.currentTarget.value)}
                            required
                        />
                        <Group justify="flex-end">
                            <Button variant="default" onClick={closeReject}>
                                取消
                            </Button>
                            <Button color="red" onClick={handleReject} loading={submitting}>
                                确认驳回
                            </Button>
                        </Group>
                    </Stack>
                </Modal>
            </Stack>
        </Layout>
    );
};

export default RegistrationsPage;
