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
  IconLicense,
} from '@tabler/icons-react';
import { invoke } from '@tauri-apps/api/core';
import { notifySuccess, notifyError } from '@/utils/notify';

// ======================== 类型定义 ========================

interface IntangibleAssetView {
  id: number;
  asset_no: string;
  asset_type: string;
  category_id: number;
  asset_name: string;
  manufacturer: string | null;
  model: string | null;
  department_id: number | null;
  user_id: number | null;
  status: number;
  purchase_date: string | null;
  purchase_price: number | null;
  quantity: number | null;
  used_quantity: number | null;
  expire_date: string | null;
  description: string | null;
  created_by: number | null;
  created_at: string | null;
  updated_by: number | null;
  updated_at: string | null;
  deleted: number | null;
  // intangible_assets 扩展字段
  intangible_id: number | null;
  intangible_type: string | null;
  register_no: string | null;
  register_owner: string | null;
  register_date: string | null;
  valid_start_date: string | null;
  valid_end_date: string | null;
  right_status: string | null;
  license_key: string | null;
  license_type: string | null;
  authorized_scope: string | null;
  assigned_user_ids: string | null;
  bind_type: string | null;
  bind_info: string | null;
  version: string | null;
  download_link: string | null;
  amortization_method: string | null;
  useful_life: number | null;
  amortization_amount: number | null;
  residual_rate: number | null;
}

interface IntangibleAssetInput {
  category_id: number;
  asset_name: string;
  manufacturer: string | null;
  model: string | null;
  department_id: number | null;
  user_id: number | null;
  status: number | null;
  purchase_date: string | null;
  purchase_price: number | null;
  quantity: number | null;
  used_quantity: number | null;
  expire_date: string | null;
  description: string | null;
  intangible_type: string | null;
  register_no: string | null;
  register_owner: string | null;
  register_date: string | null;
  valid_start_date: string | null;
  valid_end_date: string | null;
  right_status: string | null;
  license_key: string | null;
  license_type: string | null;
  authorized_scope: string | null;
  assigned_user_ids: string | null;
  bind_type: string | null;
  bind_info: string | null;
  version: string | null;
  download_link: string | null;
  amortization_method: string | null;
  useful_life: number | null;
  amortization_amount: number | null;
  residual_rate: number | null;
}

interface Category {
  id: number;
  category_name: string;
  asset_type: string;
  parent_id: number;
  sort: number;
  description: string | null;
}

// 状态映射
const STATUS_MAP: Record<number, { label: string; color: string }> = {
  0: { label: '在库', color: 'blue' },
  1: { label: '使用中', color: 'green' },
  2: { label: '维修中', color: 'orange' },
  3: { label: '已报废', color: 'red' },
  4: { label: '已领用', color: 'teal' },
};

// 无形资产类型映射
const INTANGIBLE_TYPE_MAP: Record<string, string> = {
  patent: '专利',
  trademark: '商标',
  copyright: '著作权',
  license: '许可证',
  software: '软件',
  other: '其他',
};

// 授权状态映射
const RIGHT_STATUS_MAP: Record<string, { label: string; color: string }> = {
  valid: { label: '有效', color: 'green' },
  expiring: { label: '即将到期', color: 'orange' },
  expired: { label: '已过期', color: 'red' },
  pending: { label: '申请中', color: 'blue' },
};

const SoftwarePage: React.FC = () => {
  const [assets, setAssets] = useState<IntangibleAssetView[]>([]);
  const [categories, setCategories] = useState<Category[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchText, setSearchText] = useState('');

  // 表单弹窗
  const [formModalOpen, setFormModalOpen] = useState(false);
  const [formMode, setFormMode] = useState<'add' | 'edit'>('add');
  const [editingId, setEditingId] = useState<number | null>(null);
  const [saving, setSaving] = useState(false);

  // 表单字段 - 基础信息
  const [formCategoryId, setFormCategoryId] = useState<number>(0);
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

  // 表单字段 - 无形资产扩展
  const [formIntangibleType, setFormIntangibleType] = useState<string>('software');
  const [formRegisterNo, setFormRegisterNo] = useState('');
  const [formRegisterOwner, setFormRegisterOwner] = useState('');
  const [formRegisterDate, setFormRegisterDate] = useState('');
  const [formValidStartDate, setFormValidStartDate] = useState('');
  const [formValidEndDate, setFormValidEndDate] = useState('');
  const [formRightStatus, setFormRightStatus] = useState<string>('valid');
  const [formLicenseKey, setFormLicenseKey] = useState('');
  const [formLicenseType, setFormLicenseType] = useState('');
  const [formAuthorizedScope, setFormAuthorizedScope] = useState('');
  const [formAssignedUserIds, setFormAssignedUserIds] = useState('');
  const [formBindType, setFormBindType] = useState('');
  const [formBindInfo, setFormBindInfo] = useState('');
  const [formVersion, setFormVersion] = useState('');
  const [formDownloadLink, setFormDownloadLink] = useState('');
  const [formAmortizationMethod, setFormAmortizationMethod] = useState<string>('straight_line');
  const [formUsefulLife, setFormUsefulLife] = useState<number>(0);
  const [formAmortizationAmount, setFormAmortizationAmount] = useState<number>(0);
  const [formResidualRate, setFormResidualRate] = useState<number>(0);

  // 删除确认
  const [deleteModalOpen, setDeleteModalOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<IntangibleAssetView | null>(null);
  const [deleting, setDeleting] = useState(false);

  // 详情弹窗
  const [detailModalOpen, setDetailModalOpen] = useState(false);
  const [detailAsset, setDetailAsset] = useState<IntangibleAssetView | null>(null);

  useEffect(() => {
    fetchAssets();
    fetchCategories();
  }, []);

  const fetchAssets = async () => {
    try {
      setLoading(true);
      setError(null);
      const data = await invoke<IntangibleAssetView[]>('get_intangible_assets');
      setAssets(data);
    } catch (err) {
      console.error('获取无形资产列表失败:', err);
      setError(typeof err === 'string' ? err : '获取无形资产列表失败');
    } finally {
      setLoading(false);
    }
  };

  const fetchCategories = async () => {
    try {
      const data = await invoke<Category[]>('get_categories');
      setCategories(data.filter((c) => c.asset_type === 'intangible'));
    } catch (err) {
      console.error('获取分类列表失败:', err);
    }
  };

  // 获取分类名称
  const getCategoryName = (id: number): string => {
    const cat = categories.find((c) => c.id === id);
    return cat ? cat.category_name : `分类#${id}`;
  };

  // 过滤后的资产列表
  const filteredAssets = assets.filter((a) => {
    if (!searchText) return true;
    const s = searchText.toLowerCase();
    return (
      a.asset_name.toLowerCase().includes(s) ||
      a.asset_no.toLowerCase().includes(s) ||
      (a.register_no && a.register_no.toLowerCase().includes(s)) ||
      (a.manufacturer && a.manufacturer.toLowerCase().includes(s)) ||
      (a.version && a.version.toLowerCase().includes(s))
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
  const openEditModal = (asset: IntangibleAssetView) => {
    setFormMode('edit');
    setEditingId(asset.id);
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
    setFormIntangibleType(asset.intangible_type || 'intangible');
    setFormRegisterNo(asset.register_no || '');
    setFormRegisterOwner(asset.register_owner || '');
    setFormRegisterDate(asset.register_date || '');
    setFormValidStartDate(asset.valid_start_date || '');
    setFormValidEndDate(asset.valid_end_date || '');
    setFormRightStatus(asset.right_status || 'valid');
    setFormLicenseKey(asset.license_key || '');
    setFormLicenseType(asset.license_type || '');
    setFormAuthorizedScope(asset.authorized_scope || '');
    setFormAssignedUserIds(asset.assigned_user_ids || '');
    setFormBindType(asset.bind_type || '');
    setFormBindInfo(asset.bind_info || '');
    setFormVersion(asset.version || '');
    setFormDownloadLink(asset.download_link || '');
    setFormAmortizationMethod(asset.amortization_method || 'straight_line');
    setFormUsefulLife(asset.useful_life || 0);
    setFormAmortizationAmount(asset.amortization_amount || 0);
    setFormResidualRate(asset.residual_rate || 0);
    setFormModalOpen(true);
  };

  // 重置表单
  const resetForm = () => {
    setFormCategoryId(0);
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
    setFormIntangibleType('intangible');
    setFormRegisterNo('');
    setFormRegisterOwner('');
    setFormRegisterDate('');
    setFormValidStartDate('');
    setFormValidEndDate('');
    setFormRightStatus('valid');
    setFormLicenseKey('');
    setFormLicenseType('');
    setFormAuthorizedScope('');
    setFormAssignedUserIds('');
    setFormBindType('');
    setFormBindInfo('');
    setFormVersion('');
    setFormDownloadLink('');
    setFormAmortizationMethod('straight_line');
    setFormUsefulLife(0);
    setFormAmortizationAmount(0);
    setFormResidualRate(0);
  };

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

    setSaving(true);
    try {
      const input: IntangibleAssetInput = {
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
        intangible_type: formIntangibleType,
        register_no: formRegisterNo.trim() || null,
        register_owner: formRegisterOwner.trim() || null,
        register_date: formRegisterDate || null,
        valid_start_date: formValidStartDate || null,
        valid_end_date: formValidEndDate || null,
        right_status: formRightStatus,
        license_key: formLicenseKey.trim() || null,
        license_type: formLicenseType.trim() || null,
        authorized_scope: formAuthorizedScope.trim() || null,
        assigned_user_ids: formAssignedUserIds.trim() || null,
        bind_type: formBindType.trim() || null,
        bind_info: formBindInfo.trim() || null,
        version: formVersion.trim() || null,
        download_link: formDownloadLink.trim() || null,
        amortization_method: formAmortizationMethod,
        useful_life: formUsefulLife || null,
        amortization_amount: formAmortizationAmount || null,
        residual_rate: formResidualRate || null,
      };

      if (formMode === 'add') {
        await invoke('insert_intangible_asset', { input });
        notifySuccess('无形资产添加成功');
      } else if (editingId) {
        await invoke('update_intangible_asset', { id: editingId, input });
        notifySuccess('无形资产更新成功');
      }

      setFormModalOpen(false);
      fetchAssets();
    } catch (err) {
      console.error('保存无形资产失败:', err);
      notifyError('保存无形资产失败', typeof err === 'string' ? err : undefined);
    } finally {
      setSaving(false);
    }
  };

  // 打开删除确认
  const openDeleteModal = (asset: IntangibleAssetView) => {
    setDeleteTarget(asset);
    setDeleteModalOpen(true);
  };

  // 确认删除
  const handleDelete = async () => {
    if (!deleteTarget) return;
    setDeleting(true);
    try {
      await invoke('delete_intangible_asset', { id: deleteTarget.id });
      setDeleteModalOpen(false);
      setDeleteTarget(null);
      notifySuccess('无形资产删除成功');
      fetchAssets();
    } catch (err) {
      console.error('删除无形资产失败:', err);
      notifyError('删除无形资产失败', typeof err === 'string' ? err : undefined);
    } finally {
      setDeleting(false);
    }
  };

  // 查看详情
  const openDetailModal = (asset: IntangibleAssetView) => {
    setDetailAsset(asset);
    setDetailModalOpen(true);
  };

  return (
    <Layout>
      <Stack gap="lg">
        {/* 页面标题 */}
        <Group justify="space-between">
          <Group>
            <IconLicense size={28} />
            <div>
              <Title order={2}>无形资产</Title>
              <Text c="dimmed">管理软件、专利、商标等无形资产</Text>
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
              新增无形资产
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
          placeholder="搜索资产名称、编号、注册号..."
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
              {searchText ? '未找到匹配的资产' : '暂无无形资产数据，请新增'}
            </Text>
          ) : (
            <Table striped highlightOnHover withTableBorder>
              <Table.Thead>
                <Table.Tr>
                  <Table.Th>资产编号</Table.Th>
                  <Table.Th>资产名称</Table.Th>
                  <Table.Th>分类</Table.Th>
                  <Table.Th>类型</Table.Th>
                  <Table.Th>版本</Table.Th>
                  <Table.Th>授权状态</Table.Th>
                  <Table.Th>有效期</Table.Th>
                  <Table.Th>操作</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {filteredAssets.map((asset) => {
                  const rightStatusInfo = RIGHT_STATUS_MAP[asset.right_status || ''] || {
                    label: asset.right_status || '未知',
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
                        <Badge variant="light" color="violet" size="sm">
                          {INTANGIBLE_TYPE_MAP[asset.intangible_type || ''] ||
                            asset.intangible_type ||
                            '-'}
                        </Badge>
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm">{asset.version || '-'}</Text>
                      </Table.Td>
                      <Table.Td>
                        <Badge
                          variant="light"
                          color={rightStatusInfo.color}
                          size="sm"
                        >
                          {rightStatusInfo.label}
                        </Badge>
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm">
                          {asset.valid_end_date
                            ? new Date(
                              asset.valid_end_date
                            ).toLocaleDateString('zh-CN')
                            : '-'}
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
        title={formMode === 'add' ? '新增无形资产' : '编辑无形资产'}
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
                value={String(formCategoryId)}
                onChange={(val) => setFormCategoryId(Number(val) || 0)}
                searchable
              />
            </Grid.Col>
          </Grid>

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="供应商/开发商"
                placeholder="请输入供应商"
                value={formManufacturer}
                onChange={(e) => setFormManufacturer(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="版本"
                placeholder="请输入版本号"
                value={formVersion}
                onChange={(e) => setFormVersion(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Grid>
            <Grid.Col span={4}>
              <Select
                label="无形资产类型"
                data={[
                  { value: 'software', label: '软件' },
                  { value: 'patent', label: '专利' },
                  { value: 'trademark', label: '商标' },
                  { value: 'copyright', label: '著作权' },
                  { value: 'license', label: '许可证' },
                  { value: 'other', label: '其他' },
                ]}
                value={formIntangibleType}
                onChange={(val) => setFormIntangibleType(val || 'software')}
              />
            </Grid.Col>
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
              <Select
                label="授权状态"
                data={[
                  { value: 'valid', label: '有效' },
                  { value: 'expiring', label: '即将到期' },
                  { value: 'expired', label: '已过期' },
                  { value: 'pending', label: '申请中' },
                ]}
                value={formRightStatus}
                onChange={(val) => setFormRightStatus(val || 'valid')}
              />
            </Grid.Col>
          </Grid>

          <Grid>
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
            <Grid.Col span={4}>
              <NumberInput
                label="购买价格"
                placeholder="请输入价格"
                value={formPurchasePrice}
                onChange={(val) => setFormPurchasePrice(Number(val) || 0)}
                min={0}
                decimalScale={2}
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
                label="到期日期"
                type="date"
                value={formExpireDate}
                onChange={(e) => setFormExpireDate(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Divider label="知识产权信息" labelPosition="center" />

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="注册号/登记号"
                placeholder="请输入注册号"
                value={formRegisterNo}
                onChange={(e) => setFormRegisterNo(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="注册人/权利人"
                placeholder="请输入权利人"
                value={formRegisterOwner}
                onChange={(e) => setFormRegisterOwner(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="注册日期"
                type="date"
                value={formRegisterDate}
                onChange={(e) => setFormRegisterDate(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="有效期开始"
                type="date"
                value={formValidStartDate}
                onChange={(e) => setFormValidStartDate(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="有效期结束"
                type="date"
                value={formValidEndDate}
                onChange={(e) => setFormValidEndDate(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="授权范围"
                placeholder="请输入授权范围"
                value={formAuthorizedScope}
                onChange={(e) => setFormAuthorizedScope(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Divider label="许可证信息" labelPosition="center" />

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="许可证密钥"
                placeholder="请输入许可证密钥"
                value={formLicenseKey}
                onChange={(e) => setFormLicenseKey(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="许可证类型"
                placeholder="如：企业版、专业版"
                value={formLicenseType}
                onChange={(e) => setFormLicenseType(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="绑定类型"
                placeholder="如：MAC、用户数"
                value={formBindType}
                onChange={(e) => setFormBindType(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="绑定信息"
                placeholder="绑定详情"
                value={formBindInfo}
                onChange={(e) => setFormBindInfo(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="分配用户ID"
                placeholder="多个用逗号分隔"
                value={formAssignedUserIds}
                onChange={(e) => setFormAssignedUserIds(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="下载链接"
                placeholder="请输入下载链接"
                value={formDownloadLink}
                onChange={(e) => setFormDownloadLink(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Divider label="摊销信息" labelPosition="center" />

          <Grid>
            <Grid.Col span={4}>
              <Select
                label="摊销方法"
                data={[
                  { value: 'straight_line', label: '直线法' },
                  { value: 'double_declining', label: '双倍余额递减法' },
                  { value: 'sum_of_years', label: '年数总和法' },
                  { value: 'none', label: '不计提' },
                ]}
                value={formAmortizationMethod}
                onChange={(val) =>
                  setFormAmortizationMethod(val || 'straight_line')
                }
              />
            </Grid.Col>
            <Grid.Col span={4}>
              <NumberInput
                label="使用年限(年)"
                placeholder="请输入年限"
                value={formUsefulLife}
                onChange={(val) => setFormUsefulLife(Number(val) || 0)}
                min={0}
              />
            </Grid.Col>
            <Grid.Col span={4}>
              <NumberInput
                label="残值率(%)"
                placeholder="请输入残值率"
                value={formResidualRate}
                onChange={(val) => setFormResidualRate(Number(val) || 0)}
                min={0}
                max={100}
                decimalScale={2}
              />
            </Grid.Col>
          </Grid>

          <Grid>
            <Grid.Col span={6}>
              <NumberInput
                label="摊销金额"
                placeholder="请输入摊销金额"
                value={formAmortizationAmount}
                onChange={(val) => setFormAmortizationAmount(Number(val) || 0)}
                min={0}
                decimalScale={2}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="型号/规格"
                placeholder="请输入型号规格"
                value={formModel}
                onChange={(e) => setFormModel(e.target.value)}
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
            确定要删除无形资产{' '}
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
              <Text fw={600} size="sm" mb="sm">
                基本信息
              </Text>
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
                    类型
                  </Text>
                  <Badge variant="light" color="violet">
                    {INTANGIBLE_TYPE_MAP[
                      detailAsset.intangible_type || ''
                    ] || detailAsset.intangible_type || '-'}
                  </Badge>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    供应商
                  </Text>
                  <Text size="sm">{detailAsset.manufacturer || '-'}</Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    版本
                  </Text>
                  <Text size="sm">{detailAsset.version || '-'}</Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    状态
                  </Text>
                  <Badge
                    variant="light"
                    color={STATUS_MAP[detailAsset.status]?.color || 'gray'}
                  >
                    {STATUS_MAP[detailAsset.status]?.label || '未知'}
                  </Badge>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    授权状态
                  </Text>
                  <Badge
                    variant="light"
                    color={
                      RIGHT_STATUS_MAP[detailAsset.right_status || '']?.color ||
                      'gray'
                    }
                  >
                    {RIGHT_STATUS_MAP[detailAsset.right_status || '']?.label ||
                      detailAsset.right_status ||
                      '未知'}
                  </Badge>
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
                    购买日期
                  </Text>
                  <Text size="sm">
                    {detailAsset.purchase_date
                      ? new Date(
                        detailAsset.purchase_date
                      ).toLocaleDateString('zh-CN')
                      : '-'}
                  </Text>
                </Group>
              </Stack>
            </Paper>

            <Paper p="md" withBorder radius="sm">
              <Text fw={600} size="sm" mb="sm">
                知识产权信息
              </Text>
              <Stack gap="sm">
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    注册号/登记号
                  </Text>
                  <Text size="sm">{detailAsset.register_no || '-'}</Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    注册人/权利人
                  </Text>
                  <Text size="sm">{detailAsset.register_owner || '-'}</Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    注册日期
                  </Text>
                  <Text size="sm">
                    {detailAsset.register_date
                      ? new Date(detailAsset.register_date).toLocaleDateString('zh-CN')
                      : '-'}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    有效期开始
                  </Text>
                  <Text size="sm">
                    {detailAsset.valid_start_date
                      ? new Date(detailAsset.valid_start_date).toLocaleDateString('zh-CN')
                      : '-'}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    有效期结束
                  </Text>
                  <Text size="sm">
                    {detailAsset.valid_end_date
                      ? new Date(detailAsset.valid_end_date).toLocaleDateString('zh-CN')
                      : '-'}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    授权范围
                  </Text>
                  <Text size="sm">{detailAsset.authorized_scope || '-'}</Text>
                </Group>
              </Stack>
            </Paper>

            <Paper p="md" withBorder radius="sm">
              <Text fw={600} size="sm" mb="sm">
                许可证信息

              </Text>
              <Stack gap="sm">
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    许可证密钥
                  </Text>
                  <Text size="sm">{detailAsset.license_key || '-'}</Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    许可证类型
                  </Text>
                  <Text size="sm">{detailAsset.license_type || '-'}</Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    绑定类型
                  </Text>
                  <Text size="sm">{detailAsset.bind_type || '-'}</Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    绑定信息
                  </Text>
                  <Text size="sm">{detailAsset.bind_info || '-'}</Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    分配用户
                  </Text>
                  <Text size="sm">{detailAsset.assigned_user_ids || '-'}</Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    下载链接
                  </Text>
                  <Text size="sm">{detailAsset.download_link || '-'}</Text>
                </Group>
              </Stack>
            </Paper>

            <Paper p="md" withBorder radius="sm">
              <Text fw={600} size="sm" mb="sm">
                摊销信息
              </Text>
              <Stack gap="sm">
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    摊销方法
                  </Text>
                  <Text size="sm">
                    {detailAsset.amortization_method === 'straight_line' ? '直线法' :
                      detailAsset.amortization_method === 'double_declining' ? '双倍余额递减法' :
                        detailAsset.amortization_method === 'sum_of_years' ? '年数总和法' :
                          detailAsset.amortization_method === 'none' ? '不计提' :
                            detailAsset.amortization_method || '-'}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    使用年限
                  </Text>
                  <Text size="sm">{detailAsset.useful_life ? `${detailAsset.useful_life} 年` : '-'}</Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    摊销金额
                  </Text>
                  <Text size="sm">
                    {detailAsset.amortization_amount
                      ? `¥${detailAsset.amortization_amount.toFixed(2)}`
                      : '-'}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    残值率
                  </Text>
                  <Text size="sm">{detailAsset.residual_rate ? `${detailAsset.residual_rate}%` : '-'}</Text>
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

export default SoftwarePage;
