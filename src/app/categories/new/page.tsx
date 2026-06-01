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
        const data = await invoke<Category[]>('get_categories_parents');
        console.log('获取类别数据:', data);
        setParentCategories(data);
      } catch (err) {
        console.error('获取类别列表失败:', err);
        setError(typeof err === 'string' ? err : '获取类别列表失败，请稍后重试');
      } finally {
        setLoading(false);
      }
    };

    fetchParentCategories();
  }, []);
  // 构建父级类别选项（数据已从 Rust 后端获取，只包含顶级类别）
  const parentOptions = categories.map((c) => ({
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
    setError(null);

    const categoryPayload = {
      id: 0,
      category_name: formData.category_name.trim(),
      asset_type: formData.asset_type,
      parent_id: formData.parent_id ?? 0,
      sort: formData.sort,
      description: formData.description || null,
      created_by: null,
      created_at: null,
      updated_by: null,
      updated_at: null,
      deleted: 0,
    };

    try {
      await invoke<Category>('insert_category', { category: categoryPayload });
      alert('类别添加成功！');
      router.push('/categories');
    } catch (err) {
      console.error('新增类别失败:', err);
      setError(typeof err === 'string' ? err : '新增类别失败，请稍后重试');
      alert(typeof err === 'string' ? err : '新增类别失败，请稍后重试');
    } finally {
      setSaving(false);
    }
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
