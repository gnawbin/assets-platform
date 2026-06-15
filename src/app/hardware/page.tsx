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
  Modal,
  TextInput,
  Textarea,
  NumberInput,
  Select,
  Loader,
  Alert,
  Table,
  Badge,
  ActionIcon,
  Tooltip,
  Divider,
  Grid,
  Paper,
} from '@mantine/core';
import {
  IconAlertCircle,
  IconTrash,
  IconEdit,
  IconPlus,
  IconRefresh,
  IconSearch,
  IconDeviceDesktop,
} from '@tabler/icons-react';
import { notifySuccess, notifyError } from '@/utils/notify';
import { useApi } from '@/hooks/useApi';
import { getCategories, type Category } from '@/services/categoryService';
import {
  getHardwareAssets,
  insertHardwareAsset,
  updateHardwareAsset,
  deleteHardwareAsset,
  type HardwareAssetView,
  type HardwareAssetInput,
} from '@/services/hardwareService';

// 状态映射
const STATUS_MAP: Record<number, { label: string; color: string }> = {
  0: { label: '在库', color: 'blue' },
  1: { label: '使用中', color: 'green' },
  2: { label: '维修中', color: 'orange' },
  3: { label: '已报废', color: 'red' },
  4: { label: '已领用', color: 'teal' },
};

const HardwarePage: React.FC = () => {
  const [assets, setAssets] = useState<HardwareAssetView[]>([]);
  const [categories, setCategories] = useState<Category[]>([]);
  const [searchText, setSearchText] = useState('');

  // 使用 useApi 管理数据获取
  const {
    data: fetchedAssets,
    loading,
    error,
    execute: fetchAssets,
  } = useApi(getHardwareAssets);

  // 使用 useApi 管理增删改操作
  const { execute: doInsert, loading: saving } = useApi(insertHardwareAsset);
  const { execute: doUpdate } = useApi(updateHardwareAsset);
  const { execute: doDelete, loading: deleting } = useApi(deleteHardwareAsset);

  // 表单弹窗
  const [formModalOpen, setFormModalOpen] = useState(false);
  const [formMode, setFormMode] = useState<'add' | 'edit'>('add');
  const [editingId, setEditingId] = useState<number | null>(null);

  // 表单字段
  const [formCategoryId, setFormCategoryId] = useState<string>('');
  const [formAssetName, setFormAssetName] = useState('');
  const [formManufacturer, setFormManufacturer] = useState('');
  const [formModel, setFormModel] = useState('');
  const [formStatus, setFormStatus] = useState<string>('0');
  const [formPurchaseDate, setFormPurchaseDate] = useState('');
  const [formPurchasePrice, setFormPurchasePrice] = useState<number>(0);
  const [formQuantity, setFormQuantity] = useState<number>(1);
  const [formUsedQuantity, setFormUsedQuantity] = useState<number>(0);
  const [formExpireDate, setFormExpireDate] = useState('');
  const [formDescription, setFormDescription] = useState('');
  // 硬件扩展字段
  const [formSn, setFormSn] = useState('');
  const [formMacAddress, setFormMacAddress] = useState('');
  const [formLocation, setFormLocation] = useState('');
  const [formHardwareConfig, setFormHardwareConfig] = useState('');
  const [formUseStartDate, setFormUseStartDate] = useState('');
  const [formMaintenanceVendor, setFormMaintenanceVendor] = useState('');
  const [formMaintenanceType, setFormMaintenanceType] = useState('');
  const [formMaintenanceExpireDate, setFormMaintenanceExpireDate] = useState('');
  const [formFaultDesc, setFormFaultDesc] = useState('');

  // 删除确认
  const [deleteModalOpen, setDeleteModalOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<HardwareAssetView | null>(null);

  // 详情弹窗
  const [detailModalOpen, setDetailModalOpen] = useState(false);
  const [detailAsset, setDetailAsset] = useState<HardwareAssetView | null>(null);

  // 当 fetchedAssets 变化时更新本地状态
  useEffect(() => {
    if (fetchedAssets) {
      setAssets(fetchedAssets);
    }
  }, [fetchedAssets]);

  useEffect(() => {
    fetchAssets();
    fetchCategories();
  }, []);

  const fetchCategories = async () => {
    try {
      const data = await getCategories();
      setCategories(data.filter((c) => c.asset_type === 'fixed' || c.asset_type === 'hardware'));
    } catch (err) {
      console.error('获取分类列表失败:', err);
    }
  };

  // 获取分类名称
  const getCategoryName = (id: string): string => {
    const cat = categories.find((c) => String(c.id) === id);
    return cat ? cat.category_name : `分类#${id}`;
  };

  // 过滤后的资产列表
  const filteredAssets = assets.filter((a) => {
    if (!searchText) return true;
    const s = searchText.toLowerCase();
    return (
      a.asset_name.toLowerCase().includes(s) ||
      a.asset_no.toLowerCase().includes(s) ||
      (a.sn && a.sn.toLowerCase().includes(s)) ||
      (a.manufacturer && a.manufacturer.toLowerCase().includes(s)) ||
      (a.model && a.model.toLowerCase().includes(s))
    );
  });

  // 打开新增弹窗
  const openAddModal = () => {
    setFormMode('add');
    setEditingId(null);
    resetForm();
    setFormModalOpen(true);
  };

  // 打开编辑弹窗
  const openEditModal = (asset: HardwareAssetView) => {
    setFormMode('edit');
    setEditingId(Number(asset.id));
    setFormCategoryId(asset.category_id);
    setFormAssetName(asset.asset_name);
    setFormManufacturer(asset.manufacturer || '');
    setFormModel(asset.model || '');
    setFormStatus(String(asset.status));
    setFormPurchaseDate(asset.purchase_date || '');
    setFormPurchasePrice(asset.purchase_price || 0);
    setFormQuantity(asset.quantity || 1);
    setFormUsedQuantity(asset.used_quantity || 0);
    setFormExpireDate(asset.expire_date || '');
    setFormDescription(asset.description || '');
    setFormSn(asset.sn || '');
    setFormMacAddress(asset.mac_address || '');
    setFormLocation(asset.location || '');
    setFormHardwareConfig(asset.hardware_config || '');
    setFormUseStartDate(asset.use_start_date || '');
    setFormMaintenanceVendor(asset.maintenance_vendor || '');
    setFormMaintenanceType(asset.maintenance_type || '');
    setFormMaintenanceExpireDate(asset.maintenance_expire_date || '');
    setFormFaultDesc(asset.fault_desc || '');
    setFormModalOpen(true);
  };

  // 重置表单
  const resetForm = () => {
    setFormCategoryId('');
    setFormAssetName('');
    setFormManufacturer('');
    setFormModel('');
    setFormStatus('0');
    setFormPurchaseDate('');
    setFormPurchasePrice(0);
    setFormQuantity(1);
    setFormUsedQuantity(0);
    setFormExpireDate('');
    setFormDescription('');
    setFormSn('');
    setFormMacAddress('');
    setFormLocation('');
    setFormHardwareConfig('');
    setFormUseStartDate('');
    setFormMaintenanceVendor('');
    setFormMaintenanceType('');
    setFormMaintenanceExpireDate('');
    setFormFaultDesc('');
  };

  // 构建表单输入对象
  const buildInput = (): HardwareAssetInput => ({
    category_id: formCategoryId,
    asset_name: formAssetName.trim(),
    manufacturer: formManufacturer.trim() || null,
    model: formModel.trim() || null,
    department_id: null,
    user_id: null,
    status: parseInt(formStatus),
    purchase_date: formPurchaseDate || null,
    purchase_price: formPurchasePrice || null,
    quantity: formQuantity,
    used_quantity: formUsedQuantity,
    expire_date: formExpireDate || null,
    description: formDescription.trim() || null,
    sn: formSn.trim() || null,
    mac_address: formMacAddress.trim() || null,
    location: formLocation.trim() || null,
    hardware_config: formHardwareConfig.trim() || null,
    use_user_id: null,
    use_start_date: formUseStartDate || null,
    maintenance_vendor: formMaintenanceVendor.trim() || null,
    maintenance_type: formMaintenanceType.trim() || null,
    maintenance_expire_date: formMaintenanceExpireDate || null,
    fault_desc: formFaultDesc.trim() || null,
  });

  // 保存
  const handleSave = async () => {
    if (!formAssetName.trim()) {
      notifyError('验证失败', '请输入资产名称');
      return;
    }
    if (!formCategoryId) {
      notifyError('验证失败', '请选择资产分类');
      return;
    }

    try {
      const input = buildInput();

      if (formMode === 'add') {
        await doInsert({ input });
        notifySuccess('固定资产添加成功');
      } else if (editingId) {
        await doUpdate({ id: String(editingId), input });
        notifySuccess('固定资产更新成功');
      }

      setFormModalOpen(false);
      fetchAssets();
    } catch (err) {
      console.error('保存固定资产失败:', err);
      notifyError('保存固定资产失败', typeof err === 'string' ? err : undefined);
    }
  };

  // 打开删除确认
  const openDeleteModal = (asset: HardwareAssetView) => {
    setDeleteTarget(asset);
    setDeleteModalOpen(true);
  };

  // 确认删除
  const handleDelete = async () => {
    if (!deleteTarget) return;
    try {
      await doDelete(deleteTarget.id);
      setDeleteModalOpen(false);
      setDeleteTarget(null);
      notifySuccess('固定资产删除成功');
      fetchAssets();
    } catch (err) {
      console.error('删除固定资产失败:', err);
      notifyError('删除固定资产失败', typeof err === 'string' ? err : undefined);
    }
  };

  // 查看详情
  const openDetailModal = (asset: HardwareAssetView) => {
    setDetailAsset(asset);
    setDetailModalOpen(true);
  };

  return (
    <Layout>
      <Stack gap="lg">
        {/* 页面标题 */}
        <Group justify="space-between">
          <Group>
            <IconDeviceDesktop size={28} />
            <div>
              <Title order={2}>固定资产</Title>
              <Text c="dimmed">管理所有硬件设备资产</Text>
            </div>
          </Group>
          <Group>
            <Button
              variant="light"
              leftSection={<IconRefresh size={16} />}
              onClick={fetchAssets}
              loading={loading}
            >
              刷新
            </Button>
            <Button
              leftSection={<IconPlus size={16} />}
              onClick={openAddModal}
            >
              新增固定资产
            </Button>
          </Group>
        </Group>

        {error && (
          <Alert icon={<IconAlertCircle size={16} />} title="错误" color="red">
            {error}
          </Alert>
        )}

        {/* 搜索栏 */}
        <TextInput
          placeholder="搜索资产名称、编号、序列号..."
          leftSection={<IconSearch size={16} />}
          value={searchText}
          onChange={(e) => setSearchText(e.target.value)}
          style={{ maxWidth: 400 }}
        />

        {/* 资产列表 */}
        <Card withBorder padding="lg" radius="md">
          {loading ? (
            <Group justify="center" py="xl">
              <Loader />
            </Group>
          ) : filteredAssets.length === 0 ? (
            <Text ta="center" c="dimmed" py="xl">
              {searchText ? '未找到匹配的资产' : '暂无固定资产数据，请新增'}
            </Text>
          ) : (
            <Table striped highlightOnHover withTableBorder>
              <Table.Thead>
                <Table.Tr>
                  <Table.Th>资产编号</Table.Th>
                  <Table.Th>资产名称</Table.Th>
                  <Table.Th>分类</Table.Th>
                  <Table.Th>品牌/型号</Table.Th>
                  <Table.Th>序列号</Table.Th>
                  <Table.Th>状态</Table.Th>
                  <Table.Th>数量</Table.Th>
                  <Table.Th>操作</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {filteredAssets.map((asset) => {
                  const statusInfo = STATUS_MAP[asset.status] || {
                    label: '未知',
                    color: 'gray',
                  };
                  return (
                    <Table.Tr
                      key={asset.id}
                      style={{ cursor: 'pointer' }}
                      onClick={() => openDetailModal(asset)}
                    >
                      <Table.Td>
                        <Text size="sm" fw={500}>
                          {asset.asset_no}
                        </Text>
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm">{asset.asset_name}</Text>
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm">
                          {getCategoryName(asset.category_id)}
                        </Text>
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm">
                          {[asset.manufacturer, asset.model]
                            .filter(Boolean)
                            .join(' / ') || '-'}
                        </Text>
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm">{asset.sn || '-'}</Text>
                      </Table.Td>
                      <Table.Td>
                        <Badge
                          variant="light"
                          color={statusInfo.color}
                          size="sm"
                        >
                          {statusInfo.label}
                        </Badge>
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm">
                          {asset.used_quantity || 0}/{asset.quantity || 0}
                        </Text>
                      </Table.Td>
                      <Table.Td>
                        <Group gap="xs" onClick={(e) => e.stopPropagation()}>
                          <Tooltip label="编辑">
                            <ActionIcon
                              variant="light"
                              color="blue"
                              size="sm"
                              onClick={() => openEditModal(asset)}
                            >
                              <IconEdit size={14} />
                            </ActionIcon>
                          </Tooltip>
                          <Tooltip label="删除">
                            <ActionIcon
                              variant="light"
                              color="red"
                              size="sm"
                              onClick={() => openDeleteModal(asset)}
                            >
                              <IconTrash size={14} />
                            </ActionIcon>
                          </Tooltip>
                        </Group>
                      </Table.Td>
                    </Table.Tr>
                  );
                })}
              </Table.Tbody>
            </Table>
          )}
        </Card>
      </Stack>

      {/* ======================== 新增/编辑弹窗 ======================== */}
      <Modal
        opened={formModalOpen}
        onClose={() => setFormModalOpen(false)}
        title={formMode === 'add' ? '新增固定资产' : '编辑固定资产'}
        size="xl"
      >
        <Stack gap="md">
          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="资产名称"
                placeholder="请输入资产名称"
                required
                value={formAssetName}
                onChange={(e) => setFormAssetName(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <Select
                label="资产分类"
                placeholder="请选择分类"
                required
                data={categories.map((c) => ({
                  value: String(c.id),
                  label: c.category_name,
                }))}
                value={formCategoryId}
                onChange={(val) => setFormCategoryId(val || '')}
                searchable
              />
            </Grid.Col>
          </Grid>

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="品牌/制造商"
                placeholder="请输入品牌"
                value={formManufacturer}
                onChange={(e) => setFormManufacturer(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="型号"
                placeholder="请输入型号"
                value={formModel}
                onChange={(e) => setFormModel(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Grid>
            <Grid.Col span={4}>
              <Select
                label="状态"
                data={[
                  { value: '0', label: '在库' },
                  { value: '1', label: '使用中' },
                  { value: '2', label: '维修中' },
                  { value: '3', label: '已报废' },
                  { value: '4', label: '已领用' },
                ]}
                value={formStatus}
                onChange={(val) => setFormStatus(val || '0')}
              />
            </Grid.Col>
            <Grid.Col span={4}>
              <NumberInput
                label="数量"
                placeholder="总数量"
                value={formQuantity}
                onChange={(val) => setFormQuantity(Number(val) || 0)}
                min={0}
              />
            </Grid.Col>
            <Grid.Col span={4}>
              <NumberInput
                label="已使用数量"
                placeholder="已使用数量"
                value={formUsedQuantity}
                onChange={(val) => setFormUsedQuantity(Number(val) || 0)}
                min={0}
              />
            </Grid.Col>
          </Grid>

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="购买日期"
                type="date"
                value={formPurchaseDate}
                onChange={(e) => setFormPurchaseDate(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="保修到期"
                type="date"
                value={formExpireDate}
                onChange={(e) => setFormExpireDate(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Grid>
            <Grid.Col span={6}>
              <NumberInput
                label="购买价格"
                placeholder="请输入价格"
                value={formPurchasePrice}
                onChange={(val) => setFormPurchasePrice(Number(val) || 0)}
                min={0}
                decimalScale={2}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="存放位置"
                placeholder="请输入存放位置"
                value={formLocation}
                onChange={(e) => setFormLocation(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Divider label="硬件信息" labelPosition="center" />

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="序列号(SN)"
                placeholder="请输入序列号"
                value={formSn}
                onChange={(e) => setFormSn(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="MAC地址"
                placeholder="请输入MAC地址"
                value={formMacAddress}
                onChange={(e) => setFormMacAddress(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="使用开始日期"
                type="date"
                value={formUseStartDate}
                onChange={(e) => setFormUseStartDate(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="硬件配置"
                placeholder="CPU/内存/硬盘等"
                value={formHardwareConfig}
                onChange={(e) => setFormHardwareConfig(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Divider label="维保信息" labelPosition="center" />

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="维保供应商"
                placeholder="请输入维保供应商"
                value={formMaintenanceVendor}
                onChange={(e) => setFormMaintenanceVendor(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="维保类型"
                placeholder="请输入维保类型"
                value={formMaintenanceType}
                onChange={(e) => setFormMaintenanceType(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="维保到期日期"
                type="date"
                value={formMaintenanceExpireDate}
                onChange={(e) => setFormMaintenanceExpireDate(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="故障描述"
                placeholder="请输入故障描述"
                value={formFaultDesc}
                onChange={(e) => setFormFaultDesc(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Textarea
            label="备注"
            placeholder="请输入备注信息"
            minRows={2}
            value={formDescription}
            onChange={(e) => setFormDescription(e.target.value)}
          />

          <Group justify="flex-end" mt="md">
            <Button variant="default" onClick={() => setFormModalOpen(false)}>
              取消
            </Button>
            <Button onClick={handleSave} loading={saving}>
              {formMode === 'add' ? '保存' : '保存修改'}
            </Button>
          </Group>
        </Stack>
      </Modal>

      {/* ======================== 删除确认弹窗 ======================== */}
      <Modal
        opened={deleteModalOpen}
        onClose={() => setDeleteModalOpen(false)}
        title="确认删除"
        size="sm"
      >
        <Stack gap="md">
          <Text>
            确定要删除固定资产{' '}
            <strong>{deleteTarget?.asset_name}</strong>（编号：
            {deleteTarget?.asset_no}）吗？
          </Text>
          <Text size="sm" c="dimmed">
            此操作将软删除该资产。
          </Text>
          <Group justify="flex-end" mt="md">
            <Button variant="default" onClick={() => setDeleteModalOpen(false)}>
              取消
            </Button>
            <Button color="red" onClick={handleDelete} loading={deleting}>
              确认删除
            </Button>
          </Group>
        </Stack>
      </Modal>

      {/* ======================== 详情弹窗 ======================== */}
      <Modal
        opened={detailModalOpen}
        onClose={() => setDetailModalOpen(false)}
        title="资产详情"
        size="lg"
      >
        {detailAsset && (
          <Stack gap="md">
            <Paper p="md" withBorder radius="sm">
              <Stack gap="sm">
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    资产编号
                  </Text>
                  <Text size="sm" fw={500}>
                    {detailAsset.asset_no}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    资产名称
                  </Text>
                  <Text size="sm" fw={500}>
                    {detailAsset.asset_name}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    分类
                  </Text>
                  <Text size="sm">
                    {getCategoryName(detailAsset.category_id)}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    品牌/型号
                  </Text>
                  <Text size="sm">
                    {[detailAsset.manufacturer, detailAsset.model]
                      .filter(Boolean)
                      .join(' / ') || '-'}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    状态
                  </Text>
                  <Badge
                    variant="light"
                    color={
                      STATUS_MAP[detailAsset.status]?.color || 'gray'
                    }
                  >
                    {STATUS_MAP[detailAsset.status]?.label || '未知'}
                  </Badge>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    数量
                  </Text>
                  <Text size="sm">
                    {detailAsset.used_quantity || 0}/{detailAsset.quantity || 0}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    购买日期
                  </Text>
                  <Text size="sm">
                    {detailAsset.purchase_date
                      ? new Date(detailAsset.purchase_date).toLocaleDateString(
                        'zh-CN'
                      )
                      : '-'}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    购买价格
                  </Text>
                  <Text size="sm">
                    {detailAsset.purchase_price
                      ? `¥${detailAsset.purchase_price.toFixed(2)}`
                      : '-'}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    保修到期
                  </Text>
                  <Text size="sm">
                    {detailAsset.expire_date
                      ? new Date(detailAsset.expire_date).toLocaleDateString(
                        'zh-CN'
                      )
                      : '-'}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    存放位置
                  </Text>
                  <Text size="sm">{detailAsset.location || '-'}</Text>
                </Group>
              </Stack>
            </Paper>

            <Paper p="md" withBorder radius="sm">
              <Text fw={600} size="sm" mb="sm">
                硬件信息
              </Text>
              <Stack gap="sm">
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    序列号
                  </Text>
                  <Text size="sm">{detailAsset.sn || '-'}</Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    MAC地址
                  </Text>
                  <Text size="sm">{detailAsset.mac_address || '-'}</Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    硬件配置
                  </Text>
                  <Text size="sm">{detailAsset.hardware_config || '-'}</Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    使用开始日期
                  </Text>
                  <Text size="sm">
                    {detailAsset.use_start_date
                      ? new Date(
                        detailAsset.use_start_date
                      ).toLocaleDateString('zh-CN')
                      : '-'}
                  </Text>
                </Group>
              </Stack>
            </Paper>

            <Paper p="md" withBorder radius="sm">
              <Text fw={600} size="sm" mb="sm">
                维保信息
              </Text>
              <Stack gap="sm">
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    维保供应商
                  </Text>
                  <Text size="sm">
                    {detailAsset.maintenance_vendor || '-'}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    维保类型
                  </Text>
                  <Text size="sm">
                    {detailAsset.maintenance_type || '-'}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    维保到期
                  </Text>
                  <Text size="sm">
                    {detailAsset.maintenance_expire_date
                      ? new Date(
                        detailAsset.maintenance_expire_date
                      ).toLocaleDateString('zh-CN')
                      : '-'}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    故障描述
                  </Text>
                  <Text size="sm">{detailAsset.fault_desc || '-'}</Text>
                </Group>
              </Stack>
            </Paper>

            <Group gap="xs">
              <Text size="xs" c="dimmed">
                创建时间:{' '}
                {detailAsset.created_at
                  ? new Date(detailAsset.created_at).toLocaleString('zh-CN')
                  : '-'}
              </Text>
              <Text size="xs" c="dimmed">
                | 更新时间:{' '}
                {detailAsset.updated_at
                  ? new Date(detailAsset.updated_at).toLocaleString('zh-CN')
                  : '-'}
              </Text>
            </Group>
          </Stack>
        )}
      </Modal>
    </Layout>
  );
};

export default HardwarePage;
