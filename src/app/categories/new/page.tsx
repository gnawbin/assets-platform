'use client';
import React, { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import Layout from '@/components/Layout';
import {
  Title,
  Text,
  Card,
  Stack,
  Group,
  Button,
  TextInput,
  Select,
  NumberInput,
  Textarea,
} from '@mantine/core';
import { IconArrowLeft } from '@tabler/icons-react';
import { invoke } from '@tauri-apps/api/core';

interface Category {
  id: number;
  category_name: string;
  asset_type: string;
  parent_id: number;
  sort: number;
  description: string;
  created_by: number;
  created_at: string;
  updated_by: number;
  updated_at: string;
}

// 模拟数据（与列表页保持一致）
const mockCategories: Category[] = [
  {
    id: 1,
    category_name: '服务器',
    asset_type: '硬件资产',
    parent_id: 0,
    sort: 1,
    description: '各类服务器设备，包括机架式、塔式服务器等',
    created_by: 1,
    created_at: '2024-01-15 10:00:00',
    updated_by: 1,
    updated_at: '2024-01-15 10:00:00',
  },
  {
    id: 2,
    category_name: '网络设备',
    asset_type: '硬件资产',
    parent_id: 0,
    sort: 2,
    description: '交换机、路由器、防火墙等网络基础设施',
    created_by: 1,
    created_at: '2024-01-15 10:05:00',
    updated_by: 1,
    updated_at: '2024-01-15 10:05:00',
  },
  {
    id: 3,
    category_name: '办公设备',
    asset_type: '硬件资产',
    parent_id: 0,
    sort: 3,
    description: '台式机、笔记本、打印机等日常办公设备',
    created_by: 1,
    created_at: '2024-01-15 10:10:00',
    updated_by: 1,
    updated_at: '2024-01-15 10:10:00',
  },
  {
    id: 4,
    category_name: '操作系统',
    asset_type: '软件资产',
    parent_id: 0,
    sort: 4,
    description: 'Windows、Linux、macOS等操作系统软件',
    created_by: 1,
    created_at: '2024-01-15 10:15:00',
    updated_by: 1,
    updated_at: '2024-01-15 10:15:00',
  },
  {
    id: 5,
    category_name: '办公软件',
    asset_type: '软件资产',
    parent_id: 0,
    sort: 5,
    description: 'Microsoft Office、WPS等办公套件',
    created_by: 1,
    created_at: '2024-01-15 10:20:00',
    updated_by: 1,
    updated_at: '2024-01-15 10:20:00',
  },
  {
    id: 6,
    category_name: '开发工具',
    asset_type: '软件资产',
    parent_id: 0,
    sort: 6,
    description: 'IDE、数据库管理工具、版本控制等开发相关软件',
    created_by: 1,
    created_at: '2024-01-15 10:25:00',
    updated_by: 1,
    updated_at: '2024-01-15 10:25:00',
  },
  {
    id: 7,
    category_name: '机架式服务器',
    asset_type: '硬件资产',
    parent_id: 1,
    sort: 1,
    description: '标准机架式安装的服务器设备',
    created_by: 1,
    created_at: '2024-01-15 11:00:00',
    updated_by: 1,
    updated_at: '2024-01-15 11:00:00',
  },
  {
    id: 8,
    category_name: '塔式服务器',
    asset_type: '硬件资产',
    parent_id: 1,
    sort: 2,
    description: '独立放置的塔式服务器',
    created_by: 1,
    created_at: '2024-01-15 11:05:00',
    updated_by: 1,
    updated_at: '2024-01-15 11:05:00',
  },
];

const NewCategoryPage: React.FC = () => {
  const router = useRouter();
  const [categories, setParentCategories] = useState<Category[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [formData, setFormData] = useState({
    category_name: '',
    asset_type: '',
    parent_id: null as number | null,
    sort: 0,
    description: '',
  });
  
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    const fetchParentCategories = async () => {
      try {
        setLoading(true);
        setError(null);
        const data = await invoke<Category[]>('get_categories');
        console.log('获取类别数据:', data);
        setParentCategories(data);
    //    setFilteredCategories(data);
      } catch (err) {
        console.error('获取类别列表失败:', err);
        setError(typeof err === 'string' ? err : '获取类别列表失败，请稍后重试');
      } finally {
        setLoading(false);
      }
    };

    fetchParentCategories();
  }, []);
  // 构建父级类别选项（只显示顶级类别，即 parent_id === 0）
  const parentOptions = mockCategories
    .filter((c) => c.parent_id === 0)
    .map((c) => ({
      value: String(c.id),
      label: c.category_name,
    }));

  const handleSubmit = async () => {
    if (!formData.category_name.trim()) {
      alert('请输入类别名称');
      return;
    }
    if (!formData.asset_type) {
      alert('请选择资产类型');
      return;
    }

    setSaving(true);

    // 模拟保存延迟
    await new Promise((resolve) => setTimeout(resolve, 500));

    // 模拟保存成功
    alert('类别添加成功！');
    router.push('/categories');
  };

  return (
    <Layout>
      <Stack gap="lg">
        <Group>
          <Button
            variant="subtle"
            leftSection={<IconArrowLeft size={16} />}
            onClick={() => router.push('/categories')}
          >
            返回
          </Button>
          <div>
            <Title order={2}>添加类别</Title>
            <Text c="dimmed">创建新的资产类别</Text>
          </div>
        </Group>

        <Card withBorder padding="lg" radius="md">
          <Stack gap="md">
            <TextInput
              label="类别名称"
              placeholder="请输入类别名称"
              required
              value={formData.category_name}
              onChange={(e) =>
                setFormData({ ...formData, category_name: e.target.value })
              }
            />

            <Select
              label="资产类型"
              placeholder="请选择资产类型"
              required
              data={[
                { value: '硬件资产', label: '硬件资产' },
                { value: '软件资产', label: '软件资产' },
              ]}
              value={formData.asset_type}
              onChange={(value) =>
                setFormData({ ...formData, asset_type: value || '' })
              }
            />

            <Select
              label="父级类别"
              placeholder="请选择父级类别（可选）"
              clearable
              data={parentOptions}
              value={
                formData.parent_id !== null ? String(formData.parent_id) : null
              }
              onChange={(value) =>
                setFormData({
                  ...formData,
                  parent_id: value ? Number(value) : null,
                })
              }
            />

            <NumberInput
              label="排序"
              placeholder="请输入排序序号"
              min={0}
              value={formData.sort}
              onChange={(value) =>
                setFormData({ ...formData, sort: typeof value === 'number' ? value : 0 })
              }
            />

            <Textarea
              label="描述"
              placeholder="请输入类别描述"
              minRows={3}
              value={formData.description}
              onChange={(e) =>
                setFormData({ ...formData, description: e.target.value })
              }
            />

            <Group justify="flex-end" mt="md">
              <Button
                variant="default"
                onClick={() => router.push('/categories')}
              >
                取消
              </Button>
              <Button onClick={handleSubmit} loading={saving}>
                保存
              </Button>
            </Group>
          </Stack>
        </Card>
      </Stack>
    </Layout>
  );
};

export default NewCategoryPage;
