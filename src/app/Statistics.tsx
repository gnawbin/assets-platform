import React from 'react';
import { Card, Text, Title, Stack, Group, Badge, Button } from '@mantine/core';
import { IconChartBar, IconDownload } from '@tabler/icons-react';

interface StatisticsProps {
  title: string;
}

const Statistics: React.FC<StatisticsProps> = ({ title }) => {
  return (
    <Stack gap="lg">
      <Group justify="space-between">
        <div>
          <Title order={2}>{title}</Title>
          <Text c="dimmed">统计分析 - {title}</Text>
        </div>
        <Group>
          <Button variant="light" leftSection={<IconChartBar size={16} />}>
            查看图表
          </Button>
          <Button leftSection={<IconDownload size={16} />}>
            导出报表
          </Button>
        </Group>
      </Group>

      <Card withBorder padding="lg" radius="md">
        <Stack gap="md" align="center" py="xl">
          <Text c="dimmed" size="lg">
            {title} - 功能开发中
          </Text>
          <Badge size="lg" variant="light" color="violet">
            即将上线
          </Badge>
        </Stack>
      </Card>
    </Stack>
  );
};

export default Statistics;
