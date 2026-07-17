import React from 'react';
import { Card, Text, Title, Stack, Group, Badge, Button } from '@mantine/core';
import { IconPlus, IconSearch } from '@tabler/icons-react';

interface AssetLedgerProps {
  title: string;
}

const AssetLedger: React.FC<AssetLedgerProps> = ({ title }) => {
  return (
    <Stack gap="lg">
      <Group justify="space-between">
        <div>
          <Title order={2}>{title}</Title>
          <Text c="dimmed">资产台账 - {title}</Text>
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

export default AssetLedger;
