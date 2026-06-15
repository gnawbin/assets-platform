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
  getReturns,
  insertReturn,
  updateReturn,
  deleteReturn,
  type AssetReturn,
  type AssetReturnInput,
} from '@/services/processService';

const ReturnPage: React.FC = () => {
  const [returns, setReturns] = useState<AssetReturn[]>([]);
  const [modalOpened, setModalOpened] = useState(false);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [form, setForm] = useState<AssetReturnInput>({
    receive_id: 0,
    asset_id: 0,
    user_id: 0,
    return_date: '',
    asset_status: 0,
    remark: '',
    confirm_by: 0,
    confirm_time: '',
  });

  const { loading, error, execute: fetchReturns } = useApi(getReturns);
  const { loading: saving, execute: execInsert } = useApi(insertReturn);
  const { execute: execUpdate } = useApi(updateReturn);
  const { execute: execDelete } = useApi(deleteReturn);

  const loadData = async () => {
    const result = await fetchReturns();
    if (result) setReturns(result);
  };

  useEffect(() => {
    loadData();
  }, []);

  const openCreateModal = () => {
    setEditingId(null);
    setForm({
      receive_id: 0,
      asset_id: 0,
      user_id: 0,
      return_date: new Date().toISOString().split('T')[0],
      asset_status: 0,
      remark: '',
      confirm_by: 0,
      confirm_time: new Date().toISOString().split('T')[0],
    });
    setModalOpened(true);
  };

  const openEditModal = (item: AssetReturn) => {
    setEditingId(item.id);
    setForm({
      receive_id: item.receive_id,
      asset_id: item.asset_id,
      user_id: item.user_id,
      return_date: item.return_date,
      asset_status: item.asset_status,
      remark: item.remark || '',
      confirm_by: item.confirm_by,
      confirm_time: item.confirm_time,
    });
    setModalOpened(true);
  };

  const handleSubmit = async () => {
    if (!form.receive_id || !form.asset_id || !form.user_id || !form.return_date || !form.confirm_by) {
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
    if (!confirm('确定删除此归还记录？')) return;
    const result = await execDelete(id);
    if (result) {
      notifySuccess('删除成功');
      loadData();
    }
  };

  const statusBadge = (status: number) => {
    const map: Record<number, { color: string; label: string }> = {
      0: { color: 'gray', label: '正常' },
      1: { color: 'yellow', label: '有损坏' },
      2: { color: 'red', label: '丢失' },
    };
    const s = map[status] || { color: 'gray', label: '未知' };
    return <Badge color={s.color}>{s.label}</Badge>;
  };

  return (
    <Layout>
      <Stack gap="lg">
        <Group justify="space-between">
          <Title order={2}>归还管理</Title>
          <Button leftSection={<IconPlus size={16} />} onClick={openCreateModal}>
            新增归还
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
          ) : returns.length === 0 ? (
            <Text c="dimmed" ta="center" py="xl">
              暂无归还记录
            </Text>
          ) : (
            <Table striped highlightOnHover>
              <Table.Thead>
                <Table.Tr>
                  <Table.Th>ID</Table.Th>
                  <Table.Th>归还编号</Table.Th>
                  <Table.Th>领用ID</Table.Th>
                  <Table.Th>资产ID</Table.Th>
                  <Table.Th>用户ID</Table.Th>
                  <Table.Th>归还日期</Table.Th>
                  <Table.Th>资产状态</Table.Th>
                  <Table.Th>备注</Table.Th>
                  <Table.Th>操作</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {returns.map((item) => (
                  <Table.Tr key={item.id}>
                    <Table.Td>{item.id}</Table.Td>
                    <Table.Td>{item.return_no}</Table.Td>
                    <Table.Td>{item.receive_id}</Table.Td>
                    <Table.Td>{item.asset_id}</Table.Td>
                    <Table.Td>{item.user_id}</Table.Td>
                    <Table.Td>{item.return_date}</Table.Td>
                    <Table.Td>{statusBadge(item.asset_status)}</Table.Td>
                    <Table.Td>{item.remark}</Table.Td>
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
          title={editingId ? '编辑归还记录' : '新增归还记录'}
          size="lg"
        >
          <Stack gap="md">
            <TextInput
              label="领用ID"
              type="number"
              value={form.receive_id || ''}
              onChange={(e) => setForm({ ...form, receive_id: parseInt(e.currentTarget.value) || 0 })}
              required
            />
            <TextInput
              label="资产ID"
              type="number"
              value={form.asset_id || ''}
              onChange={(e) => setForm({ ...form, asset_id: parseInt(e.currentTarget.value) || 0 })}
              required
            />
            <TextInput
              label="用户ID"
              type="number"
              value={form.user_id || ''}
              onChange={(e) => setForm({ ...form, user_id: parseInt(e.currentTarget.value) || 0 })}
              required
            />
            <TextInput
              label="归还日期"
              type="date"
              value={form.return_date}
              onChange={(e) => setForm({ ...form, return_date: e.currentTarget.value })}
              required
            />
            <Select
              label="资产状态"
              data={[
                { value: '0', label: '正常' },
                { value: '1', label: '有损坏' },
                { value: '2', label: '丢失' },
              ]}
              value={String(form.asset_status ?? 0)}
              onChange={(v) => setForm({ ...form, asset_status: v ? parseInt(v) : 0 })}
            />
            <Textarea
              label="备注"
              value={form.remark || ''}
              onChange={(e) => setForm({ ...form, remark: e.currentTarget.value })}
            />
            <TextInput
              label="确认人ID"
              type="number"
              value={form.confirm_by || ''}
              onChange={(e) => setForm({ ...form, confirm_by: parseInt(e.currentTarget.value) || 0 })}
              required
            />
            <TextInput
              label="确认时间"
              type="date"
              value={form.confirm_time}
              onChange={(e) => setForm({ ...form, confirm_time: e.currentTarget.value })}
              required
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

export default ReturnPage;
