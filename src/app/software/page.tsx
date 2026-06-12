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
import { notifySuccess, notifyError } from '@/utils/notify';
import { useApi } from '@/hooks/useApi';
import { getCategories, type Category } from '@/services/categoryService';
import {
  getIntangibleAssets,
  insertIntangibleAsset,
  updateIntangibleAsset,
  deleteIntangibleAsset,
  type IntangibleAssetView,
  type IntangibleAssetInput,
} from '@/services/softwareService';

// 状态映射
const STATUS_MAP: Record<number, { label: string; color: string }> = {
  0: { label: '在库', color: 'blue' },
  1: { label: '使用中', color: 'green' },
  2: { label: '维修中', color: 'orange' },
  3: { label: '已报废', color: 'red' },
  4: { label: '已领用', color: 'teal' },
};

const SoftwarePage: React.FC = () => {
  const [assets, setAssets] = useState<IntangibleAssetView[]>([]);
  const [categories, setCategories] = useState<Category[]>([]);
  const [searchText, setSearchText] = useState('');

  // 使用 useApi 管理数据获取
  const {
    data: fetchedAssets,
    loading,
    error,
    execute: fetchAssets,
  } = useApi(getIntangibleAssets);

  // 使用 useApi 管理增删改操作
  const { execute: doInsert, loading: saving } = useApi(insertIntangibleAsset);
  const { execute: doUpdate } = useApi(updateIntangibleAsset);
  const { execute: doDelete, loading: deleting } = useApi(deleteIntangibleAsset);

  // 表单弹窗
  const [formModalOpen, setFormModalOpen] = useState(false);
  const [formMode, setFormMode] = useState<'add' | 'edit'>('add');
  const [editingId, setEditingId] = useState<number | null>(null);

  // 表单字段
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
  // 无形资产扩展字段
  const [formIntangibleType, setFormIntangibleType] = useState('');
  const [formRegisterNo, setFormRegisterNo] = useState('');
  const [formRegisterOwner, setFormRegisterOwner] = useState('');
  const [formRegisterDate, setFormRegisterDate] = useState('');
  const [formValidStartDate, setFormValidStartDate] = useState('');
  const [formValidEndDate, setFormValidEndDate] = useState('');
  const [formRightStatus, setFormRightStatus] = useState('');
  const [formLicenseKey, setFormLicenseKey] = useState('');
  const [formLicenseType, setFormLicenseType] = useState('');
  const [formAuthorizedScope, setFormAuthorizedScope] = useState('');
  const [formVersion, setFormVersion] = useState('');
  const [formDownloadLink, setFormDownloadLink] = useState('');
  const [formAmortizationMethod, setFormAmortizationMethod] = useState('');
  const [formUsefulLife, setFormUsefulLife] = useState<number>(0);
  const [formAmortizationAmount, setFormAmortizationAmount] = useState<number>(0);
  const [formResidualRate, setFormResidualRate] = useState<number>(0);

  // 删除确认
  const [deleteModalOpen, setDeleteModalOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<IntangibleAssetView | null>(null);

  // 详情弹窗
  const [detailModalOpen, setDetailModalOpen] = useState(false);
  const [detailAsset, setDetailAsset] = useState<IntangibleAssetView | null>(null);

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
      setCategories(data.filter((c) => c.asset_type === 'intangible'));
    } catch (err) {
      console.error('获取分类列表失败:', err);
    }
  };

  // 获取分类名称
  const getCategoryName = (id: number): string => {
    const cat = categories.find((c) => String(c.id) === String(id));
    return cat ? cat.category_name : `分类#${id}`;
  };

  // 过滤后的资产列表
  const filteredAssets = assets.filter((a) => {
    if (!searchText) return true;
    const s = searchText.toLowerCase();
    return (
      a.asset_name.toLowerCase().includes(s) ||
      a.asset_no.toLowerCase().includes(s) ||
      (a.manufacturer && a.manufacturer.toLowerCase().includes(s)) ||
      (a.model && a.model.toLowerCase().includes(s)) ||
      (a.register_no && a.register_no.toLowerCase().includes(s))
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
    setFormIntangibleType(asset.intangible_type || '');
    setFormRegisterNo(asset.register_no || '');
    setFormRegisterOwner(asset.register_owner || '');
    setFormRegisterDate(asset.register_date || '');
    setFormValidStartDate(asset.valid_start_date || '');
    setFormValidEndDate(asset.valid_end_date || '');
    setFormRightStatus(asset.right_status || '');
    setFormLicenseKey(asset.license_key || '');
    setFormLicenseType(asset.license_type || '');
    setFormAuthorizedScope(asset.authorized_scope || '');
    setFormVersion(asset.version || '');
    setFormDownloadLink(asset.download_link || '');
    setFormAmortizationMethod(asset.amortization_method || '');
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
    setFormIntangibleType('');
    setFormRegisterNo('');
    setFormRegisterOwner('');
    setFormRegisterDate('');
    setFormValidStartDate('');
    setFormValidEndDate('');
    setFormRightStatus('');
    setFormLicenseKey('');
    setFormLicenseType('');
    setFormAuthorizedScope('');
    setFormVersion('');
    setFormDownloadLink('');
    setFormAmortizationMethod('');
    setFormUsefulLife(0);
    setFormAmortizationAmount(0);
    setFormResidualRate(0);
  };

  // 构建表单输入对象
  const buildInput = (): IntangibleAssetInput => ({
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
    intangible_type: formIntangibleType.trim() || null,
    register_no: formRegisterNo.trim() || null,
    register_owner: formRegisterOwner.trim() || null,
    register_date: formRegisterDate || null,
    valid_start_date: formValidStartDate || null,
    valid_end_date: formValidEndDate || null,
    right_status: formRightStatus.trim() || null,
    license_key: formLicenseKey.trim() || null,
    license_type: formLicenseType.trim() || null,
    authorized_scope: formAuthorizedScope.trim() || null,
    assigned_user_ids: null,
    bind_type: null,
    bind_info: null,
    version: formVersion.trim() || null,
    download_link: formDownloadLink.trim() || null,
    amortization_method: formAmortizationMethod.trim() || null,
    useful_life: formUsefulLife || null,
    amortization_amount: formAmortizationAmount || null,
    residual_rate: formResidualRate || null,
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
        notifySuccess('无形资产添加成功');
      } else if (editingId) {
        await doUpdate({ id: editingId, input });
        notifySuccess('无形资产更新成功');
      }

      setFormModalOpen(false);
      fetchAssets();
    } catch (err) {
      console.error('保存无形资产失败:', err);
      notifyError('保存无形资产失败', typeof err === 'string' ? err : undefined);
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
    try {
      await doDelete(deleteTarget.id);
      setDeleteModalOpen(false);
      setDeleteTarget(null);
      notifySuccess('无形资产删除成功');
      fetchAssets();
    } catch (err) {
      console.error('删除无形资产失败:', err);
      notifyError('删除无形资产失败', typeof err === 'string' ? err : undefined);
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
              <Text c="dimmed">管理所有软件、专利、商标等无形资产</Text>
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
                  <Table.Th>注册号</Table.Th>
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
                        <Badge variant="light" color="violet" size="sm">
                          {asset.intangible_type || '-'}
                        </Badge>
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm">{asset.register_no || '-'}</Text>
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
                label="到期日期"
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
                label="版本号"
                placeholder="请输入版本号"
                value={formVersion}
                onChange={(e) => setFormVersion(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Divider label="知识产权信息" labelPosition="center" />

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="无形资产类型"
                placeholder="软件著作权/专利/商标"
                value={formIntangibleType}
                onChange={(e) => setFormIntangibleType(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="注册号"
                placeholder="请输入注册号"
                value={formRegisterNo}
                onChange={(e) => setFormRegisterNo(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="注册人"
                placeholder="请输入注册人"
                value={formRegisterOwner}
                onChange={(e) => setFormRegisterOwner(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="注册日期"
                type="date"
                value={formRegisterDate}
                onChange={(e) => setFormRegisterDate(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="有效期开始"
                type="date"
                value={formValidStartDate}
                onChange={(e) => setFormValidStartDate(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="有效期结束"
                type="date"
                value={formValidEndDate}
                onChange={(e) => setFormValidEndDate(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="权利状态"
                placeholder="有效/无效/申请中"
                value={formRightStatus}
                onChange={(e) => setFormRightStatus(e.target.value)}
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

          <Divider label="许可信息" labelPosition="center" />

          <Grid>
            <Grid.Col span={6}>
              <TextInput
                label="许可密钥"
                placeholder="请输入许可密钥"
                value={formLicenseKey}
                onChange={(e) => setFormLicenseKey(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={6}>
              <TextInput
                label="许可类型"
                placeholder="永久/订阅/试用"
                value={formLicenseType}
                onChange={(e) => setFormLicenseType(e.target.value)}
              />
            </Grid.Col>
          </Grid>

          <Grid>
            <Grid.Col span={12}>
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
              <TextInput
                label="摊销方法"
                placeholder="直线法/加速法"
                value={formAmortizationMethod}
                onChange={(e) => setFormAmortizationMethod(e.target.value)}
              />
            </Grid.Col>
            <Grid.Col span={4}>
              <NumberInput
                label="使用寿命(月)"
                placeholder="月数"
                value={formUsefulLife}
                onChange={(val) => setFormUsefulLife(Number(val) || 0)}
                min={0}
              />
            </Grid.Col>
            <Grid.Col span={4}>
              <NumberInput
                label="残值率(%)"
                placeholder="百分比"
                value={formResidualRate}
                onChange={(val) => setFormResidualRate(Number(val) || 0)}
                min={0}
                max={100}
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
                    {detailAsset.intangible_type || '-'}
                  </Badge>
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
                    版本号
                  </Text>
                  <Text size="sm">{detailAsset.version || '-'}</Text>
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
                    注册号
                  </Text>
                  <Text size="sm">{detailAsset.register_no || '-'}</Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    注册人
                  </Text>
                  <Text size="sm">{detailAsset.register_owner || '-'}</Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    注册日期
                  </Text>
                  <Text size="sm">
                    {detailAsset.register_date
                      ? new Date(detailAsset.register_date).toLocaleDateString(
                        'zh-CN'
                      )
                      : '-'}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    有效期
                  </Text>
                  <Text size="sm">
                    {detailAsset.valid_start_date
                      ? `${new Date(
                        detailAsset.valid_start_date
                      ).toLocaleDateString('zh-CN')} ~ ${detailAsset.valid_end_date
                        ? new Date(
                          detailAsset.valid_end_date
                        ).toLocaleDateString('zh-CN')
                        : '长期'
                      }`
                      : '-'}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    权利状态
                  </Text>
                  <Text size="sm">{detailAsset.right_status || '-'}</Text>
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
                许可信息
              </Text>
              <Stack gap="sm">
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    许可密钥
                  </Text>
                  <Text size="sm">{detailAsset.license_key || '-'}</Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    许可类型
                  </Text>
                  <Text size="sm">{detailAsset.license_type || '-'}</Text>
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
                    {detailAsset.amortization_method || '-'}
                  </Text>
                </Group>
                <Group>
                  <Text size="sm" c="dimmed" w={100}>
                    使用寿命
                  </Text>
                  <Text size="sm">
                    {detailAsset.useful_life ? `${detailAsset.useful_life} 月` : '-'}
                  </Text>
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
                  <Text size="sm">
                    {detailAsset.residual_rate
                      ? `${detailAsset.residual_rate}%`
                      : '-'}
                  </Text>
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
