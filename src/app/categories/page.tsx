'use client';
import React, { useState, useEffect, useCallback } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import Layout from '@/components/Layout';
import { Table, Title, Text, Card, Stack, Badge, Group, Button, Anchor, Loader, Center, Alert, TextInput } from '@mantine/core';
import { IconPlus, IconAlertCircle, IconSearch, IconDatabaseOff } from '@tabler/icons-react';

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

const CategoriesPage: React.FC = () => {
  const router = useRouter();
  const [categories, setCategories] = useState<Category[]>([]);
  const [filteredCategories, setFilteredCategories] = useState<Category[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [showSearch, setShowSearch] = useState(false);

  useEffect(() => {
    const fetchCategories = async () => {
      try {
        setLoading(true);
        setError(null);
        const data = await invoke<Category[]>('get_categories');
        console.log('获取类别数据:', data);
        setCategories(data);
        setFilteredCategories(data);
      } catch (err) {
        console.error('获取类别列表失败:', err);
        setError(typeof err === 'string' ? err : '获取类别列表失败，请稍后重试');
      } finally {
        setLoading(false);
      }
    };

    fetchCategories();
  }, []);

  const handleSearch = useCallback(() => {
    if (!searchQuery.trim()) {
      setFilteredCategories(categories);
      return;
    }
    const query = searchQuery.trim().toLowerCase();
    const filtered = categories.filter(
      (item) =>
        item.category_name.toLowerCase().includes(query) ||
        item.asset_type.toLowerCase().includes(query) ||
        (item.description && item.description.toLowerCase().includes(query))
    );
    setFilteredCategories(filtered);
  }, [searchQuery, categories]);

  const rows = filteredCategories.map((item) => (
    <Table.Tr key={item.id}>
      <Table.Td>{item.id}</Table.Td>
      <Table.Td>
        <Anchor href={`/categories/${item.id}`} size="sm">
          {item.category_name}
        </Anchor>
      </Table.Td>
      <Table.Td>
        <Badge
          variant="light"
          color={item.asset_type === '硬件资产' ? 'blue' : 'violet'}
        >
          {item.asset_type}
        </Badge>
      </Table.Td>
      <Table.Td>{item.description}</Table.Td>
    </Table.Tr>
  ));

  if (loading) {
    return (
      <Layout>
        <Center h={400}>
          <Loader size="lg" />
        </Center>
      </Layout>
    );
  }

  if (error) {
    return (
      <Layout>
        <Stack gap="lg">
          <Group justify="space-between" align="center">
            <div>
              <Title order={2}>类别管理</Title>
              <Text c="dimmed">管理资产类别信息</Text>
            </div>
          </Group>
          <Alert icon={<IconAlertCircle size={16} />} title="加载失败" color="red">
            {error}
          </Alert>
        </Stack>
      </Layout>
    );
  }

  return (
    <Layout>
      <Stack gap="lg">
        <Group justify="space-between" align="center">
          <div>
            <Title order={2}>类别管理</Title>
            <Text c="dimmed">管理资产类别信息</Text>
          </div>
          <Group>
            {showSearch ? (
              <TextInput
                placeholder="搜索类别名称、资产类型或描述..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.currentTarget.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    handleSearch();
                  }
                }}
                rightSection={
                  <Button size="compact-sm" variant="subtle" onClick={handleSearch}>
                    查询
                  </Button>
                }
                rightSectionWidth={60}
              />
            ) : null}
            <Button
              variant="light"
              leftSection={<IconSearch size={16} />}
              onClick={() => {
                if (showSearch) {
                  handleSearch();
                }
                setShowSearch(!showSearch);
              }}
            >
              搜索
            </Button>
            <Button leftSection={<IconPlus size={16} />} onClick={() => router.push('/categories/new')}>
              添加类别
            </Button>
          </Group>
        </Group>

        <Card withBorder padding="lg" radius="md">
          <Table striped highlightOnHover withTableBorder>
            <Table.Thead>
              <Table.Tr>
                <Table.Th>序号</Table.Th>
                <Table.Th>类别名称</Table.Th>
                <Table.Th>资产类型</Table.Th>
                <Table.Th>描述</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>{rows}</Table.Tbody>
          </Table>
        </Card>
      </Stack>
    </Layout>
  );
};

export default CategoriesPage;
