'use client';
import React, { useState } from 'react';
import { useRouter } from 'next/navigation';
import Layout from '@/components/Layout';
import { Table, Title, Text, Card, Stack, Badge, Group, Button, Anchor } from '@mantine/core';
import { IconPlus } from '@tabler/icons-react';

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

// 模拟数据
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

const CategoriesPage: React.FC = () => {
  const router = useRouter();
  const [categories] = useState<Category[]>(mockCategories);

  const rows = categories.map((item) => (
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

  return (
    <Layout>
      <Stack gap="lg">
        <Group justify="space-between" align="center">
          <div>
            <Title order={2}>类别管理</Title>
            <Text c="dimmed">管理资产类别信息</Text>
          </div>
          <Button leftSection={<IconPlus size={16} />} onClick={() => router.push('/categories/new')}>
            添加类别
          </Button>
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
