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
    getPurchases,
    insertPurchase,
    updatePurchase,
    deletePurchase,
    type AssetPurchase,
    type AssetPurchaseInput,
} from '@/services/processService';

const PurchasePage: React.FC = () => {
    const [purchases, setPurchases] = useState<AssetPurchase[]>([]);
    const [modalOpened, setModalOpened] = useState(false);
    const [editingId, setEditingId] = useState<number | null>(null);
    const [form, setForm] = useState<AssetPurchaseInput>({
        asset_name: '',
        category_id: 0,
        model: '',
        manufacturer: '',
        quantity: 1,
        unit_price: 0,
        total_price: 0,
        apply_user: 0,
        dept_id: 0,
        reason: '',
        status: 0,
        supplier: '',
        purchase_date: '',
        arrive_date: '',
    });

    const { loading, error, execute: fetchPurchases } = useApi(getPurchases);
    const { loading: saving, execute: execInsert } = useApi(insertPurchase);
    const { execute: execUpdate } = useApi(updatePurchase);
    const { execute: execDelete } = useApi(deletePurchase);

    const loadData = async () => {
        const result = await fetchPurchases();
        if (result) setPurchases(result);
    };

    useEffect(() => {
        loadData();
    }, []);

    const openCreateModal = () => {
        setEditingId(null);
        setForm({
            asset_name: '',
            category_id: 0,
            model: '',
            manufacturer: '',
            quantity: 1,
            unit_price: 0,
            total_price: 0,
            apply_user: 0,
            dept_id: 0,
            reason: '',
            status: 0,
            supplier: '',
            purchase_date: '',
            arrive_date: '',
        });
        setModalOpened(true);
    };

    const openEditModal = (item: AssetPurchase) => {
        setEditingId(item.id);
        setForm({
            asset_name: item.asset_name,
            category_id: item.category_id,
            model: item.model || '',
            manufacturer: item.manufacturer || '',
            quantity: item.quantity,
            unit_price: item.unit_price || 0,
            total_price: item.total_price || 0,
            apply_user: item.apply_user,
            dept_id: item.dept_id,
            reason: item.reason,
            status: item.status,
            supplier: item.supplier || '',
            purchase_date: item.purchase_date || '',
            arrive_date: item.arrive_date || '',
        });
        setModalOpened(true);
    };

    const handleSubmit = async () => {
        if (!form.asset_name || !form.category_id || !form.quantity || !form.apply_user || !form.dept_id || !form.reason) {
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
        if (!confirm('确定删除此采购记录？')) return;
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
            3: { color: 'green', label: '已采购' },
            4: { color: 'teal', label: '已到货' },
        };
        const s = map[status] || { color: 'gray', label: '未知' };
        return <Badge color={s.color}>{s.label}</Badge>;
    };

    return (
        <Layout>
            <Stack gap="lg">
                <Group justify="space-between">
                    <Title order={2}>采购管理</Title>
                    <Button leftSection={<IconPlus size={16} />} onClick={openCreateModal}>
                        新增采购
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
                    ) : purchases.length === 0 ? (
                        <Text c="dimmed" ta="center" py="xl">
                            暂无采购记录
                        </Text>
                    ) : (
                        <Table striped highlightOnHover>
                            <Table.Thead>
                                <Table.Tr>
                                    <Table.Th>ID</Table.Th>
                                    <Table.Th>采购编号</Table.Th>
                                    <Table.Th>资产名称</Table.Th>
                                    <Table.Th>分类ID</Table.Th>
                                    <Table.Th>数量</Table.Th>
                                    <Table.Th>申请人</Table.Th>
                                    <Table.Th>部门</Table.Th>
                                    <Table.Th>状态</Table.Th>
                                    <Table.Th>操作</Table.Th>
                                </Table.Tr>
                            </Table.Thead>
                            <Table.Tbody>
                                {purchases.map((item) => (
                                    <Table.Tr key={item.id}>
                                        <Table.Td>{item.id}</Table.Td>
                                        <Table.Td>{item.purchase_no}</Table.Td>
                                        <Table.Td>{item.asset_name}</Table.Td>
                                        <Table.Td>{item.category_id}</Table.Td>
                                        <Table.Td>{item.quantity}</Table.Td>
                                        <Table.Td>{item.apply_user}</Table.Td>
                                        <Table.Td>{item.dept_id}</Table.Td>
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
                    title={editingId ? '编辑采购记录' : '新增采购记录'}
                    size="lg"
                >
                    <Stack gap="md">
                        <TextInput
                            label="资产名称"
                            value={form.asset_name}
                            onChange={(e) => setForm({ ...form, asset_name: e.currentTarget.value })}
                            required
                        />
                        <TextInput
                            label="分类ID"
                            type="number"
                            value={form.category_id || ''}
                            onChange={(e) => setForm({ ...form, category_id: parseInt(e.currentTarget.value) || 0 })}
                            required
                        />
                        <TextInput
                            label="型号"
                            value={form.model || ''}
                            onChange={(e) => setForm({ ...form, model: e.currentTarget.value })}
                        />
                        <TextInput
                            label="制造商"
                            value={form.manufacturer || ''}
                            onChange={(e) => setForm({ ...form, manufacturer: e.currentTarget.value })}
                        />
                        <TextInput
                            label="数量"
                            type="number"
                            value={form.quantity || ''}
                            onChange={(e) => setForm({ ...form, quantity: parseInt(e.currentTarget.value) || 1 })}
                            required
                        />
                        <TextInput
                            label="单价"
                            type="number"
                            value={form.unit_price || ''}
                            onChange={(e) => setForm({ ...form, unit_price: parseFloat(e.currentTarget.value) || 0 })}
                        />
                        <TextInput
                            label="总价"
                            type="number"
                            value={form.total_price || ''}
                            onChange={(e) => setForm({ ...form, total_price: parseFloat(e.currentTarget.value) || 0 })}
                        />
                        <TextInput
                            label="申请人ID"
                            type="number"
                            value={form.apply_user || ''}
                            onChange={(e) => setForm({ ...form, apply_user: parseInt(e.currentTarget.value) || 0 })}
                            required
                        />
                        <TextInput
                            label="部门ID"
                            type="number"
                            value={form.dept_id || ''}
                            onChange={(e) => setForm({ ...form, dept_id: parseInt(e.currentTarget.value) || 0 })}
                            required
                        />
                        <Textarea
                            label="采购原因"
                            value={form.reason}
                            onChange={(e) => setForm({ ...form, reason: e.currentTarget.value })}
                            required
                        />
                        <TextInput
                            label="供应商"
                            value={form.supplier || ''}
                            onChange={(e) => setForm({ ...form, supplier: e.currentTarget.value })}
                        />
                        <TextInput
                            label="采购日期"
                            type="date"
                            value={form.purchase_date || ''}
                            onChange={(e) => setForm({ ...form, purchase_date: e.currentTarget.value })}
                        />
                        <TextInput
                            label="到货日期"
                            type="date"
                            value={form.arrive_date || ''}
                            onChange={(e) => setForm({ ...form, arrive_date: e.currentTarget.value })}
                        />
                        <Select
                            label="状态"
                            data={[
                                { value: '0', label: '待审批' },
                                { value: '1', label: '已批准' },
                                { value: '2', label: '已拒绝' },
                                { value: '3', label: '已采购' },
                                { value: '4', label: '已到货' },
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

export default PurchasePage;
