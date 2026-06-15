'use client';
import React, { useEffect, useState } from 'react';
import Layout from '@/components/Layout';
import {
    Title,
    Text,
    Card,
    Stack,
    Group,
    Button,
    Table,
    Modal,
    TextInput,
    Select,
    Loader,
    Alert,
    Badge,
    Textarea,
} from '@mantine/core';
import { IconAlertCircle, IconTrash, IconEdit, IconPlus } from '@tabler/icons-react';
import { notifySuccess, notifyError } from '@/utils/notify';
import { useApi } from '@/hooks/useApi';
import {
    getReceives,
    insertReceive,
    updateReceive,
    deleteReceive,
    type AssetReceive,
    type AssetReceiveInput,
} from '@/services/processService';
import AssetSelect from '@/components/AssetSelect';
import DepartmentUserSelect from '@/components/DepartmentUserSelect';

const ReceivePage: React.FC = () => {
    const [receives, setReceives] = useState<AssetReceive[]>([]);
    const [modalOpened, setModalOpened] = useState(false);
    const [editingId, setEditingId] = useState<number | null>(null);
    const [form, setForm] = useState<AssetReceiveInput>({
        asset_id: '',
        user_id: '',
        department_id: '',
        receive_date: '',
        reason: '',
        status: 0,
    });

    const { loading, error, execute: fetchReceives } = useApi(getReceives);
    const { loading: saving, execute: execInsert } = useApi(insertReceive);
    const { execute: execUpdate } = useApi(updateReceive);
    const { execute: execDelete } = useApi(deleteReceive);

    const loadData = async () => {
        const result = await fetchReceives();
        if (result) setReceives(result);
    };

    useEffect(() => {
        loadData();
    }, []);

    const openCreateModal = () => {
        setEditingId(null);
        setForm({
            asset_id: '',
            user_id: '',
            department_id: '',
            receive_date: new Date().toISOString().split('T')[0],
            reason: '',
            status: 0,
        });
        setModalOpened(true);
    };

    const openEditModal = (item: AssetReceive) => {
        setEditingId(item.id);
        setForm({
            asset_id: item.asset_id,
            user_id: item.user_id,
            department_id: item.department_id,
            receive_date: item.receive_date,
            reason: item.reason,
            status: item.status,
        });
        setModalOpened(true);
    };

    const handleSubmit = async () => {
        if (!form.asset_id || !form.user_id || !form.department_id || !form.receive_date || !form.reason) {
            notifyError('请填写必填字段');
            return;
        }

        if (editingId) {
            const result = await execUpdate(editingId, form);
            if (result) {
                notifySuccess('更新成功');
                setModalOpened(false);
                loadData();
            }
        } else {
            const result = await execInsert(form);
            if (result) {
                notifySuccess('创建成功');
                setModalOpened(false);
                loadData();
            }
        }
    };

    const handleDelete = async (id: number) => {
        if (!confirm('确定删除此领用记录？')) return;
        const result = await execDelete(id);
        if (result) {
            notifySuccess('删除成功');
            loadData();
        }
    };

    const statusBadge = (status: number) => {
        const map: Record<number, { color: string; label: string }> = {
            0: { color: 'gray', label: '待审批' },
            1: { color: 'blue', label: '已批准' },
            2: { color: 'red', label: '已拒绝' },
            3: { color: 'green', label: '已完成' },
        };
        const s = map[status] || { color: 'gray', label: '未知' };
        return <Badge color={s.color}>{s.label}</Badge>;
    };

    return (
        <Layout>
            <Stack gap="lg">
                <Group justify="space-between">
                    <Title order={2}>领用管理</Title>
                    <Button leftSection={<IconPlus size={16} />} onClick={openCreateModal}>
                        新增领用
                    </Button>
                </Group>

                {error && (
                    <Alert icon={<IconAlertCircle size={16} />} title="错误" color="red">
                        {error}
                    </Alert>
                )}

                <Card shadow="sm" padding="lg" radius="md" withBorder>
                    {loading ? (
                        <Group justify="center" py="xl">
                            <Loader />
                        </Group>
                    ) : receives.length === 0 ? (
                        <Text c="dimmed" ta="center" py="xl">
                            暂无领用记录
                        </Text>
                    ) : (
                        <Table striped highlightOnHover>
                            <Table.Thead>
                                <Table.Tr>
                                    <Table.Th>ID</Table.Th>
                                    <Table.Th>领用编号</Table.Th>
                                    <Table.Th>资产ID</Table.Th>
                                    <Table.Th>用户ID</Table.Th>
                                    <Table.Th>部门ID</Table.Th>
                                    <Table.Th>领用日期</Table.Th>
                                    <Table.Th>原因</Table.Th>
                                    <Table.Th>状态</Table.Th>
                                    <Table.Th>操作</Table.Th>
                                </Table.Tr>
                            </Table.Thead>
                            <Table.Tbody>
                                {receives.map((item) => (
                                    <Table.Tr key={item.id}>
                                        <Table.Td>{item.id}</Table.Td>
                                        <Table.Td>{item.receive_no}</Table.Td>
                                        <Table.Td>{item.asset_id}</Table.Td>
                                        <Table.Td>{item.user_id}</Table.Td>
                                        <Table.Td>{item.department_id}</Table.Td>
                                        <Table.Td>{item.receive_date}</Table.Td>
                                        <Table.Td>{item.reason}</Table.Td>
                                        <Table.Td>{statusBadge(item.status)}</Table.Td>
                                        <Table.Td>
                                            <Group gap="xs">
                                                <Button
                                                    size="xs"
                                                    variant="light"
                                                    leftSection={<IconEdit size={14} />}
                                                    onClick={() => openEditModal(item)}
                                                >
                                                    编辑
                                                </Button>
                                                <Button
                                                    size="xs"
                                                    variant="light"
                                                    color="red"
                                                    leftSection={<IconTrash size={14} />}
                                                    onClick={() => handleDelete(item.id)}
                                                >
                                                    删除
                                                </Button>
                                            </Group>
                                        </Table.Td>
                                    </Table.Tr>
                                ))}
                            </Table.Tbody>
                        </Table>
                    )}
                </Card>

                <Modal
                    opened={modalOpened}
                    onClose={() => setModalOpened(false)}
                    title={editingId ? '编辑领用记录' : '新增领用记录'}
                    size="lg"
                >
                    <Stack gap="md">
                        <div>
                            <Text size="sm" fw={500} mb={4}>选择资产</Text>
                            <AssetSelect
                                mode="single"
                                value={form.asset_id}
                                onChange={(id) => setForm({ ...form, asset_id: id ?? '' })}
                                label="选择资产"
                            />
                        </div>

                        <div>
                            <Text size="sm" fw={500} mb={4}>选择用户</Text>
                            <DepartmentUserSelect
                                departmentId={form.department_id}
                                userId={form.user_id}
                                onDepartmentChange={(deptId) =>
                                    setForm({ ...form, department_id: deptId ?? '' })
                                }
                                onUserChange={(userId) =>
                                    setForm({ ...form, user_id: userId ?? '' })
                                }
                            />
                        </div>


                        <TextInput
                            label="领用日期"
                            type="date"
                            value={form.receive_date}
                            onChange={(e) => setForm({ ...form, receive_date: e.currentTarget.value })}
                            required
                        />
                        <Textarea
                            label="原因"
                            value={form.reason}
                            onChange={(e) => setForm({ ...form, reason: e.currentTarget.value })}
                            required
                        />
                        <Select
                            label="状态"
                            data={[
                                { value: '0', label: '待审批' },
                                { value: '1', label: '已批准' },
                                { value: '2', label: '已拒绝' },
                                { value: '3', label: '已完成' },
                            ]}
                            value={String(form.status ?? 0)}
                            onChange={(v) => setForm({ ...form, status: v ? parseInt(v) : 0 })}
                        />
                        <Group justify="flex-end" mt="md">
                            <Button variant="default" onClick={() => setModalOpened(false)}>
                                取消
                            </Button>
                            <Button onClick={handleSubmit} loading={saving}>
                                {editingId ? '保存' : '创建'}
                            </Button>
                        </Group>
                    </Stack>
                </Modal>
            </Stack>
        </Layout>
    );
};

export default ReceivePage;
