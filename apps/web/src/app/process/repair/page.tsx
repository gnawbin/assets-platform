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
    getRepairs,
    insertRepair,
    updateRepair,
    deleteRepair,
    type AssetRepair,
    type AssetRepairInput,
} from '@/services/processService';
import AssetSelect from '@/components/AssetSelect';
import DepartmentUserSelect from '@/components/DepartmentUserSelect';

const RepairPage: React.FC = () => {
    const [repairs, setRepairs] = useState<AssetRepair[]>([]);
    const [modalOpened, setModalOpened] = useState(false);
    const [editingId, setEditingId] = useState<number | null>(null);
    const [form, setForm] = useState<AssetRepairInput>({
        asset_id: 0,
        fault_desc: '',
        repair_desc: '',
        repair_user_id: 0,
        repair_dept_id: 0,
        repair_file_url: '',
        repair_type: 0,
        vendor: '',
        cost: 0,
        apply_date: '',
        repair_date: '',
        finish_date: '',
        status: 0,
    });

    const { loading, error, execute: fetchRepairs } = useApi(getRepairs);
    const { loading: saving, execute: execInsert } = useApi(insertRepair);
    const { execute: execUpdate } = useApi(updateRepair);
    const { execute: execDelete } = useApi(deleteRepair);

    const loadData = async () => {
        const result = await fetchRepairs();
        if (result) setRepairs(result);
    };

    useEffect(() => {
        loadData();
    }, []);

    const openCreateModal = () => {
        setEditingId(null);
        setForm({
            asset_id: 0,
            fault_desc: '',
            repair_desc: '',
            repair_user_id: 0,
            repair_dept_id: 0,
            repair_file_url: '',
            repair_type: 0,
            vendor: '',
            cost: 0,
            apply_date: new Date().toISOString().split('T')[0],
            repair_date: '',
            finish_date: '',
            status: 0,
        });
        setModalOpened(true);
    };

    const openEditModal = (item: AssetRepair) => {
        setEditingId(item.id);
        setForm({
            asset_id: item.asset_id,
            fault_desc: item.fault_desc,
            repair_desc: item.repair_desc || '',
            repair_user_id: item.repair_user_id || 0,
            repair_dept_id: item.repair_dept_id || 0,
            repair_file_url: item.repair_file_url || '',
            repair_type: item.repair_type,
            vendor: item.vendor || '',
            cost: item.cost || 0,
            apply_date: item.apply_date,
            repair_date: item.repair_date || '',
            finish_date: item.finish_date || '',
            status: item.status,
        });
        setModalOpened(true);
    };

    const handleSubmit = async () => {
        if (!form.asset_id || !form.fault_desc || !form.apply_date) {
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
        if (!confirm('确定删除此维修记录？')) return;
        const result = await execDelete(id);
        if (result) {
            notifySuccess('删除成功');
            loadData();
        }
    };

    const statusBadge = (status: number) => {
        const map: Record<number, { color: string; label: string }> = {
            0: { color: 'gray', label: '待维修' },
            1: { color: 'blue', label: '维修中' },
            2: { color: 'green', label: '已完成' },
            3: { color: 'red', label: '无法修复' },
        };
        const s = map[status] || { color: 'gray', label: '未知' };
        return <Badge color={s.color}>{s.label}</Badge>;
    };

    return (
        <Layout>
            <Stack gap="lg">
                <Group justify="space-between">
                    <Title order={2}>维修管理</Title>
                    <Button leftSection={<IconPlus size={16} />} onClick={openCreateModal}>
                        新增维修
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
                    ) : repairs.length === 0 ? (
                        <Text c="dimmed" ta="center" py="xl">
                            暂无维修记录
                        </Text>
                    ) : (
                        <Table striped highlightOnHover>
                            <Table.Thead>
                                <Table.Tr>
                                    <Table.Th>ID</Table.Th>
                                    <Table.Th>维修编号</Table.Th>
                                    <Table.Th>资产ID</Table.Th>
                                    <Table.Th>故障描述</Table.Th>
                                    <Table.Th>维修类型</Table.Th>
                                    <Table.Th>申请日期</Table.Th>
                                    <Table.Th>状态</Table.Th>
                                    <Table.Th>操作</Table.Th>
                                </Table.Tr>
                            </Table.Thead>
                            <Table.Tbody>
                                {repairs.map((item) => (
                                    <Table.Tr key={item.id}>
                                        <Table.Td>{item.id}</Table.Td>
                                        <Table.Td>{item.repair_no}</Table.Td>
                                        <Table.Td>{item.asset_id}</Table.Td>
                                        <Table.Td>{item.fault_desc}</Table.Td>
                                        <Table.Td>{item.repair_type === 0 ? '内部维修' : '外部维修'}</Table.Td>
                                        <Table.Td>{item.apply_date}</Table.Td>
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
                    title={editingId ? '编辑维修记录' : '新增维修记录'}
                    size="lg"
                >
                    <Stack gap="md">
                        <div>
                            <Text size="sm" fw={500} mb={4}>选择资产</Text>
                            <AssetSelect
                                mode="single"
                                value={form.asset_id ? String(form.asset_id) : null}
                                onChange={(id) => setForm({ ...form, asset_id: id ? parseInt(id) : 0 })}
                                label="选择资产"
                            />
                        </div>
                        <Textarea
                            label="故障描述"
                            value={form.fault_desc}
                            onChange={(e) => setForm({ ...form, fault_desc: e.currentTarget.value })}
                            required
                        />
                        <Textarea
                            label="维修描述"
                            value={form.repair_desc || ''}
                            onChange={(e) => setForm({ ...form, repair_desc: e.currentTarget.value })}
                        />
                        <div>
                            <Text size="sm" fw={500} mb={4}>维修人</Text>
                            <DepartmentUserSelect
                                departmentId={form.repair_dept_id ? String(form.repair_dept_id) : null}
                                userId={form.repair_user_id ? String(form.repair_user_id) : null}
                                onDepartmentChange={(deptId) =>
                                    setForm({ ...form, repair_dept_id: deptId ? parseInt(deptId) : 0 })
                                }
                                onUserChange={(userId) =>
                                    setForm({ ...form, repair_user_id: userId ? parseInt(userId) : 0 })
                                }
                                userLabel="选择维修人"
                            />
                        </div>
                        <Select
                            label="维修类型"
                            data={[
                                { value: '0', label: '内部维修' },
                                { value: '1', label: '外部维修' },
                            ]}
                            value={String(form.repair_type ?? 0)}
                            onChange={(v) => setForm({ ...form, repair_type: v ? parseInt(v) : 0 })}
                        />
                        <TextInput
                            label="供应商"
                            value={form.vendor || ''}
                            onChange={(e) => setForm({ ...form, vendor: e.currentTarget.value })}
                        />
                        <TextInput
                            label="费用"
                            type="number"
                            value={form.cost || ''}
                            onChange={(e) => setForm({ ...form, cost: parseFloat(e.currentTarget.value) || 0 })}
                        />
                        <TextInput
                            label="申请日期"
                            type="date"
                            value={form.apply_date}
                            onChange={(e) => setForm({ ...form, apply_date: e.currentTarget.value })}
                            required
                        />
                        <TextInput
                            label="维修日期"
                            type="date"
                            value={form.repair_date || ''}
                            onChange={(e) => setForm({ ...form, repair_date: e.currentTarget.value })}
                        />
                        <TextInput
                            label="完成日期"
                            type="date"
                            value={form.finish_date || ''}
                            onChange={(e) => setForm({ ...form, finish_date: e.currentTarget.value })}
                        />
                        <Select
                            label="状态"
                            data={[
                                { value: '0', label: '待维修' },
                                { value: '1', label: '维修中' },
                                { value: '2', label: '已完成' },
                                { value: '3', label: '无法修复' },
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

export default RepairPage;
