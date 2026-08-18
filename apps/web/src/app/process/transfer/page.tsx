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
  getTransfers,
  insertTransfer,
  updateTransfer,
  deleteTransfer,
  type AssetTransfer,
  type AssetTransferInput,
} from '@/services/processService';
import AssetSelect from '@/components/AssetSelect';
import DepartmentUserSelect from '@/components/DepartmentUserSelect';

const TransferPage: React.FC = () => {
  const [transfers, setTransfers] = useState<AssetTransfer[]>([]);
  const [modalOpened, setModalOpened] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [form, setForm] = useState<AssetTransferInput>({
    asset_id: '',
    out_dept_id: '',
    in_dept_id: '',
    out_user_id: '',
    in_user_id: '',
    transfer_date: '',
    reason: '',
    status: 0,
  });

  const { loading, error, execute: fetchTransfers } = useApi(getTransfers);
  const { loading: saving, execute: execInsert } = useApi(insertTransfer);
  const { execute: execUpdate } = useApi(updateTransfer);
  const { execute: execDelete } = useApi(deleteTransfer);

  const loadData = async () => {
    const result = await fetchTransfers();
    if (result) setTransfers(result);
  };

  useEffect(() => {
    loadData();
  }, []);

  const openCreateModal = () => {
    setEditingId(null);
    setForm({
      asset_id: '',
      out_dept_id: '',
      in_dept_id: '',
      out_user_id: '',
      in_user_id: '',
      transfer_date: new Date().toISOString().split('T')[0],
      reason: '',
      status: 0,
    });
    setModalOpened(true);
  };

  const openEditModal = (item: AssetTransfer) => {
    setEditingId(item.id);
    setForm({
      asset_id: item.asset_id,
      out_dept_id: item.out_dept_id,
      in_dept_id: item.in_dept_id,
      out_user_id: item.out_user_id,
      in_user_id: item.in_user_id,
      transfer_date: item.transfer_date,
      reason: item.reason,
      status: item.status,
    });
    setModalOpened(true);
  };

  const handleSubmit = async () => {
    if (!form.asset_id || !form.out_dept_id || !form.in_dept_id || !form.out_user_id || !form.in_user_id || !form.transfer_date || !form.reason) {
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

  const handleDelete = async (id: string) => {
    if (!confirm('确定删除此调拨记录？')) return;
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
          <Title order={2}>调拨管理</Title>
          <Button leftSection={<IconPlus size={16} />} onClick={openCreateModal}>
            新增调拨
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
          ) : transfers.length === 0 ? (
            <Text c="dimmed" ta="center" py="xl">
              暂无调拨记录
            </Text>
          ) : (
            <Table striped highlightOnHover>
              <Table.Thead>
                <Table.Tr>
                  <Table.Th>ID</Table.Th>
                  <Table.Th>调拨编号</Table.Th>
                  <Table.Th>资产ID</Table.Th>
                  <Table.Th>调出部门</Table.Th>
                  <Table.Th>调入部门</Table.Th>
                  <Table.Th>调出人</Table.Th>
                  <Table.Th>调入人</Table.Th>
                  <Table.Th>调拨日期</Table.Th>
                  <Table.Th>原因</Table.Th>
                  <Table.Th>状态</Table.Th>
                  <Table.Th>操作</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {transfers.map((item) => (
                  <Table.Tr key={item.id}>
                    <Table.Td>{item.id}</Table.Td>
                    <Table.Td>{item.transfer_no}</Table.Td>
                    <Table.Td>{item.asset_id}</Table.Td>
                    <Table.Td>{item.out_dept_id}</Table.Td>
                    <Table.Td>{item.in_dept_id}</Table.Td>
                    <Table.Td>{item.out_user_id}</Table.Td>
                    <Table.Td>{item.in_user_id}</Table.Td>
                    <Table.Td>{item.transfer_date}</Table.Td>
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
          title={editingId ? '编辑调拨记录' : '新增调拨记录'}
          size="lg"
        >
          <Stack gap="md">
            <div>
              <Text size="sm" fw={500} mb={4}>选择资产</Text>
              <AssetSelect
                mode="single"
                value={form.asset_id ? String(form.asset_id) : null}
                onChange={(id) => setForm({ ...form, asset_id: id ?? '' })}
                label="选择资产"
              />
            </div>

            <div>
              <Text size="sm" fw={500} mb={4}>调出人</Text>
              <DepartmentUserSelect
                departmentId={form.out_dept_id ? String(form.out_dept_id) : null}
                userId={form.out_user_id ? String(form.out_user_id) : null}
                onDepartmentChange={(deptId) =>
                  setForm({ ...form, out_dept_id: deptId ?? '' })
                }
                onUserChange={(userId) =>
                  setForm({ ...form, out_user_id: userId ?? '' })
                }
                userLabel="选择调出人"
              />
            </div>

            <div>
              <Text size="sm" fw={500} mb={4}>调入人</Text>
              <DepartmentUserSelect
                departmentId={form.in_dept_id ? String(form.in_dept_id) : null}
                userId={form.in_user_id ? String(form.in_user_id) : null}
                onDepartmentChange={(deptId) =>
                  setForm({ ...form, in_dept_id: deptId ?? '' })
                }
                onUserChange={(userId) =>
                  setForm({ ...form, in_user_id: userId ?? '' })
                }
                userLabel="选择调入人"
              />
            </div>

            <TextInput
              label="调拨日期"
              type="date"
              value={form.transfer_date}
              onChange={(e) => setForm({ ...form, transfer_date: e.currentTarget.value })}
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

export default TransferPage;
