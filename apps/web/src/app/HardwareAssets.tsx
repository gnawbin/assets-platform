import React from 'react';
import { Card, Text, Title, Stack, Group, Badge, Button } from '@mantine/core';
import { IconPlus, IconSearch } from '@tabler/icons-react';

interface HardwareAssetsProps {
  title: string;
}

const HardwareAssets: React.FC<HardwareAssetsProps> = ({ title }) => {
  return (
    <Stack gap="lg">
      <Group justify="space-between">
        <div>
          <Title order={2}>{title}</Title>
          <Text c="dimmed">硬资产管理 - {title}</Text>
        </div>
        <Group>
          <Button variant="light" leftSection={<IconSearch size={16} />}>
            搜索
          </Button>
          <Button leftSection={<IconPlus size={16} />}>
            新增资产
          </Button>
        </Group>
      </Group>

      <Card withBorder padding="lg" radius="md">
        <Stack gap="md" align="center" py="xl">
          <Text c="dimmed" size="lg">
            {title} - 功能开发中
          </Text>
          <Badge size="lg" variant="light" color="blue">
            即将上线
          </Badge>
        </Stack>
      </Card>
    </Stack>
  );
};

export default HardwareAssets;
