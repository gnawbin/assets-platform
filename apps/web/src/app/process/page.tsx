'use client';
import React from 'react';
import Layout from '@/components/Layout';
import { Title, Text, Card, Stack, Group, SimpleGrid } from '@mantine/core';
import {
  IconClipboardCheck,
  IconArrowBackUp,
  IconArrowsExchange,
  IconTool,
  IconTrash,
  IconShoppingCart,
} from '@tabler/icons-react';
import { useRouter } from 'next/navigation';

const processModules = [
  { label: '领用管理', path: '/process/receive', icon: IconClipboardCheck, color: 'blue' },
  { label: '归还管理', path: '/process/return', icon: IconArrowBackUp, color: 'green' },
  { label: '调拨管理', path: '/process/transfer', icon: IconArrowsExchange, color: 'violet' },
  { label: '维修管理', path: '/process/repair', icon: IconTool, color: 'orange' },
  { label: '报废管理', path: '/process/scrap', icon: IconTrash, color: 'red' },
  { label: '采购管理', path: '/process/purchase', icon: IconShoppingCart, color: 'teal' },
];

const ProcessPage: React.FC = () => {
  const router = useRouter();

  return (
    <Layout>
      <Stack gap="lg">
        <Title order={2}>流程管理</Title>
        <Text c="dimmed">选择需要管理的流程模块</Text>

        <SimpleGrid cols={{ base: 1, sm: 2, md: 3 }} spacing="lg">
          {processModules.map((module) => {
            const Icon = module.icon;
            return (
              <Card
                key={module.path}
                shadow="sm"
                padding="lg"
                radius="md"
                withBorder
                style={{ cursor: 'pointer' }}
                onClick={() => router.push(module.path)}
              >
                <Group>
                  <Icon size={32} color={`var(--mantine-color-${module.color}-6)`} />
                  <div>
                    <Text fw={500} size="lg">
                      {module.label}
                    </Text>
                    <Text size="sm" c="dimmed">
                      管理{module.label}相关记录
                    </Text>
                  </div>
                </Group>
              </Card>
            );
          })}
        </SimpleGrid>
      </Stack>
    </Layout>
  );
};

export default ProcessPage;
