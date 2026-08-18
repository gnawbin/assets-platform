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
  getScraps,
  insertScrap,
  updateScrap,
  deleteScrap,
  type AssetScrap,
  type AssetScrapInput,
} from '@/services/processService';
import AssetSelect from '@/components/AssetSelect';
import DepartmentUserSelect from '@/components/DepartmentUserSelect';

const ScrapPage: React.FC = () => {
  const [scraps, setScraps] = useState<AssetScrap[]>([]);
  const [modalOpened, setModalOpened] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [form, setForm] = useState<AssetScrapInput>({
    asset_id: '',
    reason: '',
    scrap_date: '',
    status: 0,
    handle_user: '',
  });

  const { loading, error, execute: fetchScraps } = useApi(getScraps);
  const { loading: saving, execute: execInsert } = useApi(insertScrap);
  const { execute: execUpdate } = useApi(updateScrap);
  const { execute: execDelete } = useApi(deleteScrap);

  const loadData = async () => {
    const result = await fetchScraps();
    if (result) setScraps(result);
  };

  useEffect(() => {
    loadData();
  }, []);

  const openCreateModal = () => {
    setEditingId(null);
    setForm({
      asset_id: '',
      reason: '',
      scrap_date: new Date().toISOString().split('T')[0],
      status: 0,
      handle_user: '',
    });
    setModalOpened(true);
  };

  const openEditModal = (item: AssetScrap) => {
    setEditingId(item.id);
    setForm({
      asset_id: item.asset_id,
      reason: item.reason,
      scrap_date: item.scrap_date,
      status: item.status,
      handle_user: item.handle_user || '',
    });
    setModalOpened(true);
  };

  const handleSubmit = async () => {
    if (!form.asset_id || !form.reason || !form.scrap_date) {
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
    if (!confirm('确定删除此报废记录？')) return;
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
          <Title order={2}>报废管理</Title>
          <Button leftSection={<IconPlus size={16} />} onClick={openCreateModal}>
            新增报废
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
          ) : scraps.length === 0 ? (
            <Text c="dimmed" ta="center" py="xl">
              暂无报废记录
            </Text>
          ) : (
            <Table striped highlightOnHover>
              <Table.Thead>
                <Table.Tr>
                  <Table.Th>ID</Table.Th>
                  <Table.Th>报废编号</Table.Th>
                  <Table.Th>资产ID</Table.Th>
                  <Table.Th>原因</Table.Th>
                  <Table.Th>报废日期</Table.Th>
                  <Table.Th>状态</Table.Th>
                  <Table.Th>操作</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {scraps.map((item) => (
                  <Table.Tr key={item.id}>
                    <Table.Td>{item.id}</Table.Td>
                    <Table.Td>{item.scrap_no}</Table.Td>
                    <Table.Td>{item.asset_id}</Table.Td>
                    <Table.Td>{item.reason}</Table.Td>
                    <Table.Td>{item.scrap_date}</Table.Td>
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
          title={editingId ? '编辑报废记录' : '新增报废记录'}
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
            <Textarea
              label="报废原因"
              value={form.reason}
              onChange={(e) => setForm({ ...form, reason: e.currentTarget.value })}
              required
            />
            <TextInput
              label="报废日期"
              type="date"
              value={form.scrap_date}
              onChange={(e) => setForm({ ...form, scrap_date: e.currentTarget.value })}
              required
            />
            <div>
              <Text size="sm" fw={500} mb={4}>处理人</Text>
              <DepartmentUserSelect
                departmentId={null}
                userId={form.handle_user ? String(form.handle_user) : null}
                onDepartmentChange={() => { }}
                onUserChange={(userId) =>
                  setForm({ ...form, handle_user: userId ?? '' })
                }
                userLabel="选择处理人"
              />
            </div>
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

export default ScrapPage;
